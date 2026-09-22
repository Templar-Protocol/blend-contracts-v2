use soroban_fixed_point_math::FixedPoint;
use soroban_sdk::{contracttype, panic_with_error, unwrap::UnwrapOptimized, Address, Env};

use crate::{
    backstop_threshold::saturating_backstop_product,
    constants::SCALAR_7,
    dependencies::{CometClient, PoolFactoryClient},
    errors::BackstopError,
    storage,
};

/// The pool's backstop data
#[derive(Clone)]
#[contracttype]
pub struct PoolBackstopData {
    pub tokens: i128,  // the number of backstop tokens held in the pool's backstop
    pub shares: i128,  // the number of shares the pool's backstop has issued
    pub q4w_pct: i128, // the percentage of shares/tokens queued for withdrawal
    pub blnd: i128,    // the amount of blnd held in the pool's backstop via backstop tokens
    pub usdc: i128,    // the amount of usdc held in the pool's backstop via backstop tokens
    pub token_spot_price: i128, // the spot price sans fees in USDC of the backstop token (7 decimals)
}

pub fn load_pool_backstop_data(e: &Env, address: &Address) -> PoolBackstopData {
    let pool_balance = storage::get_pool_balance(e, address);
    let q4w_pct = if pool_balance.shares > 0 {
        pool_balance
            .q4w
            .fixed_div_ceil(pool_balance.shares, SCALAR_7)
            .unwrap_optimized()
    } else {
        0
    };

    let backstop_token = storage::get_backstop_token(e);
    let blnd_token = storage::get_blnd_token(e);
    let usdc_token = storage::get_usdc_token(e);
    let comet_client = CometClient::new(e, &backstop_token);
    let total_comet_shares = comet_client.get_total_supply();
    let total_blnd = comet_client.get_balance(&blnd_token);
    let total_usdc = comet_client.get_balance(&usdc_token);

    // underlying per LP token
    let blnd_per_tkn = total_blnd
        .fixed_div_floor(total_comet_shares, SCALAR_7)
        .unwrap_optimized();
    let usdc_per_tkn = total_usdc
        .fixed_div_floor(total_comet_shares, SCALAR_7)
        .unwrap_optimized();

    // spot price of backstop token in USDC, exlcuding slippage/fees
    // LP token is 20% USDC, so 5x is the spot price without slippage/fees
    let tkn_spot_price_sans_fee = usdc_per_tkn * 5;

    if pool_balance.tokens > 0 {
        let blnd = pool_balance
            .tokens
            .fixed_mul_floor(blnd_per_tkn, SCALAR_7)
            .unwrap_optimized();
        let usdc = pool_balance
            .tokens
            .fixed_mul_floor(usdc_per_tkn, SCALAR_7)
            .unwrap_optimized();
        PoolBackstopData {
            tokens: pool_balance.tokens,
            shares: pool_balance.shares,
            q4w_pct,
            blnd,
            usdc,
            token_spot_price: tkn_spot_price_sans_fee,
        }
    } else {
        PoolBackstopData {
            tokens: 0,
            shares: pool_balance.shares,
            q4w_pct,
            blnd: 0,
            usdc: 0,
            token_spot_price: tkn_spot_price_sans_fee,
        }
    }
}

/// Verify the pool address was deployed by the Pool Factory.
///
/// If the pool has an outstanding balance, it is assumed that it was verified before.
///
/// ### Arguments
/// * `address` - The pool address to verify
/// * `balance` - The balance of the pool. A balance of 0 indicates the pool has not been initialized.
///
/// ### Panics
/// If the pool address cannot be verified
pub fn require_is_from_pool_factory(e: &Env, address: &Address, balance: i128) {
    if balance == 0 {
        let pool_factory_client = PoolFactoryClient::new(e, &storage::get_pool_factory(e));
        if !pool_factory_client.is_pool(address) {
            panic_with_error!(e, BackstopError::NotPool);
        }
    }
}

/// Calculate the threshold for the pool's backstop balance
///
/// Returns true if the pool's backstop balance is above the threshold
pub fn is_pool_above_threshold(pool_backstop_data: &PoolBackstopData) -> bool {
    threshold_from_product(saturating_backstop_product(
        pool_backstop_data.blnd,
        pool_backstop_data.usdc,
    ))
}

pub fn threshold_from_product(product: i128) -> bool {
    let threshold_pc = 10_000_000_000_000_000_000_000_000i128; // 1e25 (100k^5)
    product >= threshold_pc
}

/// The pool's backstop balances
#[derive(Clone)]
#[contracttype]
pub struct PoolBalance {
    pub shares: i128, // the amount of shares the pool has issued
    pub tokens: i128, // the number of tokens the pool holds in the backstop
    pub q4w: i128,    // the number of shares queued for withdrawal
}

impl PoolBalance {
    /// Convert a token balance to a share balance based on the current pool state
    ///
    /// ### Arguments
    /// * `tokens` - the token balance to convert
    pub fn convert_to_shares(&self, tokens: i128) -> i128 {
        if self.shares == 0 {
            return tokens;
        }
        if self.tokens == 0 {
            return 0;
        }

        tokens
            .fixed_mul_floor(self.shares, self.tokens)
            .unwrap_optimized()
    }

    /// Convert a pool share balance to a token balance based on the current pool state
    ///
    /// ### Arguments
    /// * `shares` - the pool share balance to convert
    pub fn convert_to_tokens(&self, shares: i128) -> i128 {
        if self.shares == 0 {
            return 0;
        }
        if self.shares == shares {
            return self.tokens;
        }

        shares
            .fixed_mul_floor(self.tokens, self.shares)
            .unwrap_optimized()
    }

    /// Determine the amount of effective tokens (not queued for withdrawal) in the pool
    pub fn non_queued_tokens(&self) -> i128 {
        self.tokens - self.convert_to_tokens(self.q4w)
    }

    /// Deposit tokens and shares into the pool
    ///
    /// ### Arguments
    /// * `tokens` - The amount of tokens to add
    /// * `shares` - The amount of shares to add
    pub fn deposit(&mut self, tokens: i128, shares: i128) {
        self.tokens += tokens;
        self.shares += shares;
    }

    /// Withdraw tokens and shares from the pool
    ///
    /// ### Arguments
    /// * `tokens` - The amount of tokens to withdraw
    /// * `shares` - The amount of shares to withdraw
    pub fn withdraw(&mut self, e: &Env, tokens: i128, shares: i128) {
        if let Err(error) = self.withdraw_balance(tokens, shares) {
            panic_with_error!(e, error);
        }
    }

    fn withdraw_balance(&mut self, tokens: i128, shares: i128) -> Result<(), BackstopError> {
        if tokens > self.tokens || shares > self.shares || shares > self.q4w {
            return Err(BackstopError::InsufficientFunds);
        }
        self.tokens -= tokens;
        self.shares -= shares;
        self.q4w -= shares;
        Ok(())
    }

    /// Queue withdraw for the pool
    ///
    /// ### Arguments
    /// * `shares` - The amount of shares to queue for withdraw
    pub fn queue_for_withdraw(&mut self, shares: i128) {
        self.q4w += shares;
    }

    /// Dequeue queued for withdraw for the pool
    ///
    /// ### Arguments
    /// * `shares` - The amount of shares to dequeue from q4w
    pub fn dequeue_q4w(&mut self, e: &Env, shares: i128) {
        if let Err(error) = self.dequeue_balance(shares) {
            panic_with_error!(e, error);
        }
    }

    fn dequeue_balance(&mut self, shares: i128) -> Result<(), BackstopError> {
        if shares > self.q4w {
            return Err(BackstopError::InsufficientFunds);
        }
        self.q4w -= shares;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use soroban_sdk::testutils::Address as _;

    use crate::testutils::{
        create_backstop, create_blnd_token, create_comet_lp_pool_with_tokens_per_share,
        create_mock_pool_factory, create_usdc_token,
    };

    use super::*;

    #[test]
    fn test_load_pool_data() {
        let e = Env::default();
        e.mock_all_auths();

        let bombadil = Address::generate(&e);
        let backstop_address = create_backstop(&e);
        let pool = Address::generate(&e);

        let (blnd_id, _) = create_blnd_token(&e, &backstop_address, &bombadil);
        let (usdc_id, _) = create_usdc_token(&e, &backstop_address, &bombadil);
        create_comet_lp_pool_with_tokens_per_share(
            &e,
            &backstop_address,
            &bombadil,
            &blnd_id,
            5_0000000,
            &usdc_id,
            0_0500000,
        );

        e.as_contract(&backstop_address, || {
            storage::set_pool_balance(
                &e,
                &pool,
                &PoolBalance {
                    shares: 150_0000000,
                    tokens: 250_0000000,
                    q4w: 50_0000000,
                },
            );

            let pool_data = load_pool_backstop_data(&e, &pool);

            assert_eq!(pool_data.tokens, 250_0000000);
            assert_eq!(pool_data.q4w_pct, 0_3333334); // rounds up
            assert_eq!(pool_data.blnd, 1_250_0000000);
            assert_eq!(pool_data.usdc, 12_5000000);
            assert_eq!(pool_data.token_spot_price, 0_2500000);
        });
    }

    #[test]
    fn test_load_pool_data_no_shares() {
        let e = Env::default();
        e.mock_all_auths();

        let bombadil = Address::generate(&e);
        let backstop_address = create_backstop(&e);
        let pool = Address::generate(&e);

        let (blnd_id, _) = create_blnd_token(&e, &backstop_address, &bombadil);
        let (usdc_id, _) = create_usdc_token(&e, &backstop_address, &bombadil);
        create_comet_lp_pool_with_tokens_per_share(
            &e,
            &backstop_address,
            &bombadil,
            &blnd_id,
            5_0000000,
            &usdc_id,
            0_0500000,
        );

        e.as_contract(&backstop_address, || {
            storage::set_pool_balance(
                &e,
                &pool,
                &PoolBalance {
                    shares: 0,
                    tokens: 250_0000000,
                    q4w: 0,
                },
            );

            let pool_data = load_pool_backstop_data(&e, &pool);

            assert_eq!(pool_data.tokens, 250_0000000);
            assert_eq!(pool_data.q4w_pct, 0);
            assert_eq!(pool_data.blnd, 1_250_0000000);
            assert_eq!(pool_data.usdc, 12_5000000);
            assert_eq!(pool_data.token_spot_price, 0_2500000);
        });
    }

    #[test]
    fn test_load_pool_data_no_tokens() {
        let e = Env::default();
        e.mock_all_auths();

        let bombadil = Address::generate(&e);
        let backstop_address = create_backstop(&e);
        let pool = Address::generate(&e);

        let (blnd_id, _) = create_blnd_token(&e, &backstop_address, &bombadil);
        let (usdc_id, _) = create_usdc_token(&e, &backstop_address, &bombadil);
        create_comet_lp_pool_with_tokens_per_share(
            &e,
            &backstop_address,
            &bombadil,
            &blnd_id,
            5_0000000,
            &usdc_id,
            0_0500000,
        );

        e.as_contract(&backstop_address, || {
            storage::set_pool_balance(
                &e,
                &pool,
                &PoolBalance {
                    shares: 100_0000000,
                    tokens: 0,
                    q4w: 0,
                },
            );

            let pool_data = load_pool_backstop_data(&e, &pool);

            assert_eq!(pool_data.tokens, 0);
            assert_eq!(pool_data.q4w_pct, 0);
            assert_eq!(pool_data.blnd, 0);
            assert_eq!(pool_data.usdc, 0);
            assert_eq!(pool_data.token_spot_price, 0_2500000);
        });
    }

    /********** require_is_from_pool_factory **********/

    #[test]
    fn test_require_is_from_pool_factory() {
        let e = Env::default();

        let backstop_address = create_backstop(&e);
        let pool_address = Address::generate(&e);

        let (_, mock_pool_factory) = create_mock_pool_factory(&e, &backstop_address);
        mock_pool_factory.set_pool(&pool_address);

        e.as_contract(&backstop_address, || {
            require_is_from_pool_factory(&e, &pool_address, 0);
            assert!(true);
        });
    }

    #[test]
    fn test_require_is_from_pool_factory_skips_if_balance() {
        let e = Env::default();

        let backstop_address = create_backstop(&e);
        let pool_address = Address::generate(&e);

        // don't initialize factory to force failure if pool_address is checked

        e.as_contract(&backstop_address, || {
            require_is_from_pool_factory(&e, &pool_address, 1);
            assert!(true);
        });
    }

    #[test]
    #[should_panic(expected = "Error(Contract, #1004)")]
    fn test_require_is_from_pool_factory_not_valid() {
        let e = Env::default();

        let backstop_address = create_backstop(&e);
        let pool_address = Address::generate(&e);
        let not_pool_address = Address::generate(&e);

        let (_, mock_pool_factory) = create_mock_pool_factory(&e, &backstop_address);
        mock_pool_factory.set_pool(&pool_address);

        e.as_contract(&backstop_address, || {
            require_is_from_pool_factory(&e, &not_pool_address, 0);
            assert!(false);
        });
    }

    /********** require_pool_above_threshold **********/

    #[test]
    fn test_require_pool_above_threshold_under() {
        let e = Env::default();
        e.cost_estimate().budget().reset_unlimited();

        let pool_backstop_data = PoolBackstopData {
            blnd: 200000_0000000,
            q4w_pct: 0,
            tokens: 20_000_0000000,
            shares: 15_000_0000000,
            usdc: 6_249_0000000,
            token_spot_price: 0_1000000,
        }; // ~99% threshold

        let result = is_pool_above_threshold(&pool_backstop_data);
        assert!(!result);
    }

    #[test]
    fn test_require_pool_above_threshold_zero() {
        let e = Env::default();
        e.cost_estimate().budget().reset_unlimited();

        let pool_backstop_data = PoolBackstopData {
            blnd: 5_000_0000000,
            q4w_pct: 0,
            tokens: 500_0000000,
            shares: 500_0000000,
            usdc: 1_000_0000000,
            token_spot_price: 0_1000000,
        }; // ~3.6% threshold - rounds to zero in calc

        let result = is_pool_above_threshold(&pool_backstop_data);
        assert!(!result);
    }

    #[test]
    fn test_require_pool_above_threshold_over() {
        let e = Env::default();
        e.cost_estimate().budget().reset_unlimited();

        let pool_backstop_data = PoolBackstopData {
            blnd: 200001_0000000,
            q4w_pct: 0,
            tokens: 15_000_0000000,
            shares: 14_000_0000000,
            usdc: 6_250_0000000,
            token_spot_price: 0_1000000,
        }; // 100% threshold

        let result = is_pool_above_threshold(&pool_backstop_data);
        assert!(result);
    }

    #[test]
    fn test_require_pool_above_threshold_saturates() {
        let e = Env::default();
        e.cost_estimate().budget().reset_unlimited();

        let pool_backstop_data = PoolBackstopData {
            blnd: 50_000_000_0000000,
            q4w_pct: 0,
            tokens: 999_999_0000000,
            shares: 1_099_999_0000000,
            usdc: 10_000_000_0000000,
            token_spot_price: 0_1000000,
        }; // 362x threshold

        let result = is_pool_above_threshold(&pool_backstop_data);
        assert!(result);
    }

    /********** Logic **********/

    #[test]
    fn test_non_queued_tokens() {
        let pool_balance = PoolBalance {
            shares: 80321,
            tokens: 103302,
            q4w: 40001,
        };

        let non_queued_tokens = pool_balance.non_queued_tokens();
        assert_eq!(non_queued_tokens, 51857);
    }

    #[test]
    fn test_non_queued_tokens_no_shares() {
        let pool_balance = PoolBalance {
            shares: 0,
            tokens: 0,
            q4w: 0,
        };

        let non_queued_tokens = pool_balance.non_queued_tokens();
        assert_eq!(non_queued_tokens, 0);
    }

    #[test]
    fn test_non_queued_tokens_drained_backstop() {
        let pool_balance = PoolBalance {
            shares: 8765,
            tokens: 0,
            q4w: 4321,
        };

        let non_queued_tokens = pool_balance.non_queued_tokens();
        assert_eq!(non_queued_tokens, 0);
    }

    #[test]
    fn test_non_queued_tokens_full_q4w() {
        let pool_balance = PoolBalance {
            shares: 80321,
            tokens: 103302,
            q4w: 80321,
        };

        let non_queued_tokens = pool_balance.non_queued_tokens();
        assert_eq!(non_queued_tokens, 0);
    }

    #[test]
    fn test_convert_to_shares_no_shares() {
        let pool_balance = PoolBalance {
            shares: 0,
            tokens: 0,
            q4w: 0,
        };

        let to_convert = 1234567;
        let shares = pool_balance.convert_to_shares(to_convert);
        assert_eq!(shares, to_convert);
    }

    #[test]
    fn test_convert_to_shares_drained_backstop() {
        let pool_balance = PoolBalance {
            shares: 87654321,
            tokens: 0,
            q4w: 0,
        };

        let to_convert = 1234567;
        let shares = pool_balance.convert_to_shares(to_convert);
        assert_eq!(shares, 0);
    }

    #[test]
    fn test_convert_to_shares() {
        let pool_balance = PoolBalance {
            shares: 80321,
            tokens: 103302,
            q4w: 0,
        };

        let to_convert = 1234567;
        let shares = pool_balance.convert_to_shares(to_convert);
        assert_eq!(shares, 959920);
    }

    #[test]
    fn test_convert_to_tokens_no_shares() {
        let pool_balance = PoolBalance {
            shares: 0,
            tokens: 0,
            q4w: 0,
        };

        let to_convert = 1234567;
        let shares = pool_balance.convert_to_tokens(to_convert);
        assert_eq!(shares, 0);
    }

    #[test]
    fn test_convert_to_tokens_drained_backstop() {
        let pool_balance = PoolBalance {
            shares: 87654321,
            tokens: 0,
            q4w: 0,
        };

        let to_convert = 1234567;
        let shares = pool_balance.convert_to_tokens(to_convert);
        assert_eq!(shares, 0);
    }

    #[test]
    fn test_convert_to_tokens() {
        let pool_balance = PoolBalance {
            shares: 80321,
            tokens: 103302,
            q4w: 0,
        };

        let to_convert = 40000;
        let shares = pool_balance.convert_to_tokens(to_convert);
        assert_eq!(shares, 51444);
    }

    #[test]
    fn test_convert_to_tokens_all_shares() {
        let pool_balance = PoolBalance {
            shares: 80321,
            tokens: 103302,
            q4w: 0,
        };

        let to_convert = 80321;
        let shares = pool_balance.convert_to_tokens(to_convert);
        assert_eq!(shares, 103302);
    }

    #[test]
    fn test_deposit() {
        let mut pool_balance = PoolBalance {
            shares: 100,
            tokens: 200,
            q4w: 25,
        };

        pool_balance.deposit(50, 25);

        assert_eq!(pool_balance.shares, 125);
        assert_eq!(pool_balance.tokens, 250);
        assert_eq!(pool_balance.q4w, 25);
    }

    #[test]
    fn test_withdraw() {
        let e = Env::default();
        let mut pool_balance = PoolBalance {
            shares: 100,
            tokens: 200,
            q4w: 25,
        };

        pool_balance.withdraw(&e, 50, 25);

        assert_eq!(pool_balance.shares, 75);
        assert_eq!(pool_balance.tokens, 150);
        assert_eq!(pool_balance.q4w, 0);
    }

    #[test]
    #[should_panic(expected = "Error(Contract, #1003)")]
    fn test_withdraw_too_much() {
        let e = Env::default();
        let mut pool_balance = PoolBalance {
            shares: 100,
            tokens: 200,
            q4w: 25,
        };

        pool_balance.withdraw(&e, 201, 25);
    }

    #[test]
    fn test_dequeue_q4w() {
        let e = Env::default();
        let mut pool_balance = PoolBalance {
            shares: 100,
            tokens: 200,
            q4w: 25,
        };

        pool_balance.dequeue_q4w(&e, 25);

        assert_eq!(pool_balance.shares, 100);
        assert_eq!(pool_balance.tokens, 200);
        assert_eq!(pool_balance.q4w, 0);
    }

    #[test]
    #[should_panic(expected = "Error(Contract, #1003)")]
    fn test_dequeue_q4w_too_much() {
        let e = Env::default();
        let mut pool_balance = PoolBalance {
            shares: 100,
            tokens: 200,
            q4w: 25,
        };

        pool_balance.dequeue_q4w(&e, 26);
    }

    #[test]
    fn test_q4w() {
        let e = Env::default();
        let mut pool_balance = PoolBalance {
            shares: 100,
            tokens: 200,
            q4w: 25,
        };

        pool_balance.withdraw(&e, 50, 25);

        assert_eq!(pool_balance.shares, 75);
        assert_eq!(pool_balance.tokens, 150);
        assert_eq!(pool_balance.q4w, 0);
    }
}

#[cfg(kani)]
mod verification {
    use super::*;
    #[kani::proof]
    fn prove_share_zero_and_full_redemption() {
        let tokens: i128 = kani::any();
        let shares: i128 = kani::any();
        kani::assume(tokens >= 0 && shares > 0);
        let balance = PoolBalance {
            tokens,
            shares,
            q4w: 0,
        };
        assert_eq!(balance.convert_to_tokens(shares), tokens);
        let empty = PoolBalance {
            tokens: 0,
            shares: 0,
            q4w: 0,
        };
        assert_eq!(empty.convert_to_shares(tokens), tokens);
        assert_eq!(empty.convert_to_tokens(shares), 0);
        let depleted = PoolBalance {
            tokens: 0,
            shares,
            q4w: 0,
        };
        assert_eq!(depleted.convert_to_shares(tokens), 0);
    }

    #[kani::proof]
    fn prove_share_round_trip_and_available_tokens() {
        // Deliberately bounded nonlinear domain, not protocol balance limits.
        let tokens = kani::any::<u8>() as i128;
        let shares = kani::any::<u8>() as i128;
        let amount = kani::any::<u8>() as i128;
        let queued = kani::any::<u8>() as i128;
        kani::assume(tokens > 0 && shares > 0 && queued <= shares);
        let balance = PoolBalance {
            tokens,
            shares,
            q4w: queued,
        };
        let minted = balance.convert_to_shares(amount);
        let redeemed = balance.convert_to_tokens(minted);
        assert!(minted >= 0 && redeemed >= 0 && redeemed <= amount);
        let available = balance.non_queued_tokens();
        assert!(available >= 0 && available <= tokens);
        if queued == 0 {
            assert_eq!(available, tokens);
        }
        if queued == shares {
            assert_eq!(available, 0);
        }
    }

    #[kani::proof]
    fn prove_deposit_and_queue_balances() {
        let tokens: i128 = kani::any();
        let shares: i128 = kani::any();
        let queued: i128 = kani::any();
        let added_tokens: i128 = kani::any();
        let added_shares: i128 = kani::any();
        kani::assume(tokens >= 0 && shares >= 0 && queued >= 0 && queued <= shares);
        kani::assume(added_tokens >= 0 && added_tokens <= i128::MAX - tokens);
        kani::assume(added_shares >= 0 && added_shares <= i128::MAX - shares);
        let mut balance = PoolBalance {
            tokens,
            shares,
            q4w: queued,
        };
        balance.deposit(added_tokens, added_shares);
        balance.queue_for_withdraw(added_shares);
        assert_eq!(balance.tokens - tokens, added_tokens);
        assert_eq!(balance.shares - shares, added_shares);
        assert_eq!(balance.q4w - queued, added_shares);
        assert!(balance.q4w <= balance.shares);
    }

    #[kani::proof]
    fn prove_withdraw_and_dequeue_balances() {
        let tokens: i128 = kani::any();
        let shares: i128 = kani::any();
        let queued: i128 = kani::any();
        let payout: i128 = kani::any();
        let burn: i128 = kani::any();
        kani::assume(tokens >= 0 && shares >= 0 && queued >= 0 && queued <= shares);
        kani::assume(payout >= 0 && burn >= 0);
        let mut balance = PoolBalance {
            tokens,
            shares,
            q4w: queued,
        };
        let result = balance.withdraw_balance(payout, burn);
        assert_eq!(result.is_ok(), payout <= tokens && burn <= queued);
        if result.is_ok() {
            assert_eq!(balance.tokens + payout, tokens);
            assert_eq!(balance.shares + burn, shares);
            assert_eq!(balance.q4w + burn, queued);
            assert!(balance.tokens >= 0 && balance.q4w >= 0 && balance.q4w <= balance.shares);
        } else {
            assert_eq!(balance.tokens, tokens);
            assert_eq!(balance.shares, shares);
            assert_eq!(balance.q4w, queued);
        }
        let mut dequeue = PoolBalance {
            tokens,
            shares,
            q4w: queued,
        };
        let result = dequeue.dequeue_balance(burn);
        assert_eq!(result.is_ok(), burn <= queued);
        assert_eq!(dequeue.tokens, tokens);
        assert_eq!(dequeue.shares, shares);
        if result.is_ok() {
            assert_eq!(dequeue.q4w + burn, queued);
            assert!(dequeue.q4w >= 0 && dequeue.q4w <= shares);
        } else {
            assert_eq!(dequeue.q4w, queued);
        }
        kani::cover!(payout == tokens && burn == queued && balance.q4w == 0);
        kani::cover!(payout > tokens);
        kani::cover!(burn > queued);
    }
}
