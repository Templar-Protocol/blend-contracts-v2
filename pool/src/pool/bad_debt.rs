use soroban_sdk::{panic_with_error, Address, Env, Vec};

use crate::{dependencies::BackstopClient, events::PoolEvents, storage, AuctionType, PoolError};

use super::{calc_pool_backstop_threshold, Pool, PositionData, User};

/// Handles any bad debt that exists for "user"
pub fn bad_debt(e: &Env, user: &Address) {
    let mut pool = Pool::load(e);
    let mut user_state = User::load(e, user);

    let backstop = storage::get_backstop(e);

    if user == &backstop {
        panic_with_error!(e, PoolError::BadRequest);
    }
    if storage::has_auction(e, &(AuctionType::UserLiquidation as u32), user) {
        panic_with_error!(e, PoolError::AuctionInProgress);
    }
    let had_bad_debt = check_and_handle_user_bad_debt(e, &mut pool, user, &mut user_state);

    if had_bad_debt {
        user_state.store(e);
        pool.store_cached_reserves(e);
    } else {
        panic_with_error!(e, PoolError::BadRequest);
    }
}

/// Check if a user has bad debt.
///
/// If aggregate raw collateral is zero, set off ordinary supply before defaulting residual debt.
/// Take custody of residual collateral only if debt was actually defaulted; full setoff retains it.
///
/// If not, this function does nothing.
///
/// `user_state` is modified in place, and is not stored to chain. If this function
/// is invoked, `user_state` must be written to chain afterwards.
///
/// `pool` is modified in place, and reserve updates are not stored to chain. If this function
/// is invoked, `pool.store_cached_reserves()` must be called afterwards.
///
/// ### Arguments
/// * pool - The pool
/// * user - The user's address
/// * user_state - The user's state
///
/// ### Returns
/// * `true` if the user's bad debt was handled, `false` otherwise
pub fn check_and_handle_user_bad_debt(
    e: &Env,
    pool: &mut Pool,
    user: &Address,
    user_state: &mut User,
) -> bool {
    if !user_state.has_liabilities()
        || PositionData::calculate_from_positions(e, pool, &user_state.positions).collateral_raw
            != 0
    {
        return false;
    }

    let reserve_list = storage::get_res_list(e);
    let liabilities = user_state.positions.liabilities.clone();
    let mut collateral: Vec<(Address, u32, i128)> = Vec::new(e);
    for (index, amount) in user_state.positions.collateral.iter() {
        if amount > 0 {
            collateral.push_back((reserve_list.get_unchecked(index), index, amount));
        }
    }
    let mut had_default = false;

    for (index, amount) in liabilities.iter() {
        let asset = reserve_list.get_unchecked(index);
        let mut reserve = pool.load_reserve(e, &asset, true);
        let mut defaulted = amount;
        let claim = user_state.get_supply(index);
        if claim > 0 && reserve.data.b_rate > 0 {
            let debt_assets = reserve.to_asset_from_d_token(e, amount);
            let b_tokens = claim.min(reserve.to_b_token_up(e, debt_assets));
            let covered_assets = reserve.to_asset_from_b_token(e, b_tokens);
            let repaid = amount.min(reserve.to_d_token_down(e, covered_assets));
            if repaid > 0 {
                user_state.remove_supply(e, &mut reserve, b_tokens);
                user_state.remove_liabilities(e, &mut reserve, repaid);
                defaulted -= repaid;
            }
        }
        if defaulted > 0 {
            user_state.default_liabilities(e, &mut reserve, defaulted);
            had_default = true;
            PoolEvents::defaulted_debt(e, asset, defaulted);
        }
        pool.cache_reserve(reserve);
    }

    if had_default && !collateral.is_empty() {
        let mut pool_user = User::load(e, &e.current_contract_address());
        for (asset, index, amount) in collateral.iter() {
            require_no_b_token_emissions(e, index);
            // Reload through the cache: this reserve may already have absorbed a default above.
            let mut reserve = pool.load_reserve(e, &asset, true);
            user_state.remove_collateral(e, &mut reserve, amount);
            pool_user.add_supply(e, &mut reserve, amount);
            pool.cache_reserve(reserve);
            PoolEvents::collateral_orphaned(e, user.clone(), asset, amount);
        }
        pool_user.store(e);
    }
    true
}

/// Orphan-custody b-token emissions prohibition; checked when orphan collateral is processed,
/// which follows an actual default. Unblocked debt-free supply setoff never reaches it.
pub(super) fn require_no_b_token_emissions(e: &Env, reserve_index: u32) {
    let id = reserve_index * 2 + 1;
    if storage::get_pool_emissions(e).contains_key(id)
        || storage::get_res_emis_data(e, &id).is_some()
        || storage::get_user_emissions(e, &e.current_contract_address(), &id).is_some()
    {
        panic_with_error!(e, PoolError::BadRequest);
    }
}

/// Check if the backstop's bad debt needs to be defaulted. This occurs when the backstop has less than
/// 5% of the backstop threshold in tokens, as this implies there likely isn't enough backstop tokens
/// to reasonalby auction off bad debt.
///
/// If the backstop has less than 5% of the threshold, default the bad debt.
///
/// If not, this function does nothing.
///
/// `backstop_state` is modified in place, and is not stored to chain. If this function
/// is invoked, `backstop_state` must be written to chain afterwards.
///
/// `pool` is modified in place, and reserve updates are not stored to chain. If this function
/// is invoked, `pool.store_cached_reserves()` must be called afterwards.
///
/// ### Arguments
/// * pool - The pool
/// * backstop_state - The backstop's state
///
/// ### Returns
/// * `true` if the backstop's bad debt was defaulted, `false` otherwise
pub fn check_and_handle_backstop_bad_debt(
    e: &Env,
    pool: &mut Pool,
    backstop_address: &Address,
    backstop_state: &mut User,
) -> bool {
    if backstop_state.has_liabilities() {
        let backstop_client = BackstopClient::new(e, backstop_address);
        let pool_backstop_data = backstop_client.pool_data(&e.current_contract_address());
        let threshold = calc_pool_backstop_threshold(&pool_backstop_data);
        if threshold < 0_0000003 {
            // ~5% of threshold
            let reserve_list = storage::get_res_list(e);
            for (reserve_index, liability_balance) in backstop_state.positions.liabilities.iter() {
                let res_asset_address = reserve_list.get_unchecked(reserve_index);
                let mut reserve = pool.load_reserve(e, &res_asset_address, true);
                backstop_state.default_liabilities(e, &mut reserve, liability_balance);
                pool.cache_reserve(reserve);

                PoolEvents::defaulted_debt(e, res_asset_address, liability_balance);
            }
            return true;
        }
    }
    return false;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        auctions::AuctionData,
        storage::PoolConfig,
        testutils::{
            self, create_backstop, create_blnd_token, create_comet_lp_pool, create_pool,
            create_token_contract,
        },
        Positions,
    };
    use soroban_sdk::{
        map,
        testutils::{Address as _, Ledger, LedgerInfo},
        vec, Address,
    };

    /***** bad_debt *****/

    #[test]
    #[should_panic(expected = "Error(Contract, #1200)")]
    fn test_bad_debt_user_panics_no_change() {
        let e = Env::default();
        e.cost_estimate().budget().reset_unlimited();
        e.mock_all_auths();

        let pool = create_pool(&e);
        let bombadil = Address::generate(&e);
        let frodo = Address::generate(&e);
        let samwise = Address::generate(&e);

        let (blnd, blnd_client) = create_blnd_token(&e, &pool, &bombadil);
        let (usdc, usdc_client) = create_token_contract(&e, &bombadil);
        let (lp_token, lp_token_client) = create_comet_lp_pool(&e, &bombadil, &blnd, &usdc);
        let (_, backstop_client) = create_backstop(&e, &pool, &lp_token, &usdc, &blnd);

        // mint lp tokens and deposit them into the pool's backstop
        let backstop_tokens = 1_500_0000000; // over 5% of threshold
        blnd_client.mint(&frodo, &500_001_0000000);
        blnd_client.approve(&frodo, &lp_token, &i128::MAX, &99999);
        usdc_client.mint(&frodo, &12_501_0000000);
        usdc_client.approve(&frodo, &lp_token, &i128::MAX, &99999);
        lp_token_client.join_pool(
            &backstop_tokens,
            &vec![&e, 500_001_0000000, 12_501_0000000],
            &frodo,
        );
        backstop_client.deposit(&frodo, &pool, &backstop_tokens);

        let (underlying_0, _) = testutils::create_token_contract(&e, &bombadil);
        let (reserve_config, reserve_data) = testutils::default_reserve_meta();
        testutils::create_reserve(&e, &pool, &underlying_0, &reserve_config, &reserve_data);

        let (underlying_1, _) = testutils::create_token_contract(&e, &bombadil);
        let (reserve_config, reserve_data) = testutils::default_reserve_meta();
        testutils::create_reserve(&e, &pool, &underlying_1, &reserve_config, &reserve_data);

        e.ledger().set(LedgerInfo {
            timestamp: 100,
            protocol_version: 22,
            sequence_number: 1234,
            network_id: Default::default(),
            base_reserve: 10,
            min_temp_entry_ttl: 10,
            min_persistent_entry_ttl: 10,
            max_entry_ttl: 3110400,
        });
        let pool_config = PoolConfig {
            oracle: Address::generate(&e),
            min_collateral: 1_0000000,
            bstop_rate: 0_1000000,
            status: 1,
            max_positions: 5,
        };
        let positions = Positions {
            liabilities: map![&e, (0, 1_5000000), (1, 50_987_654_321)],
            collateral: map![&e, (0, 100_1234567)],
            supply: map![&e],
        };
        e.as_contract(&pool, || {
            storage::set_pool_config(&e, &pool_config);
            storage::set_user_positions(&e, &samwise, &positions);

            bad_debt(&e, &samwise);
        });
    }

    #[test]
    fn test_bad_debt_user() {
        let e = Env::default();
        e.cost_estimate().budget().reset_unlimited();
        e.mock_all_auths();

        let pool = create_pool(&e);
        let bombadil = Address::generate(&e);
        let frodo = Address::generate(&e);
        let samwise = Address::generate(&e);

        let (blnd, blnd_client) = create_blnd_token(&e, &pool, &bombadil);
        let (usdc, usdc_client) = create_token_contract(&e, &bombadil);
        let (lp_token, lp_token_client) = create_comet_lp_pool(&e, &bombadil, &blnd, &usdc);
        let (backstop_address, backstop_client) =
            create_backstop(&e, &pool, &lp_token, &usdc, &blnd);

        // mint lp tokens and deposit them into the pool's backstop
        let backstop_tokens = 1_500_0000000; // over 5% of threshold
        blnd_client.mint(&frodo, &500_001_0000000);
        blnd_client.approve(&frodo, &lp_token, &i128::MAX, &99999);
        usdc_client.mint(&frodo, &12_501_0000000);
        usdc_client.approve(&frodo, &lp_token, &i128::MAX, &99999);
        lp_token_client.join_pool(
            &backstop_tokens,
            &vec![&e, 500_001_0000000, 12_501_0000000],
            &frodo,
        );
        backstop_client.deposit(&frodo, &pool, &backstop_tokens);

        let (underlying_0, _) = testutils::create_token_contract(&e, &bombadil);
        let (reserve_config, reserve_data_0) = testutils::default_reserve_meta();
        testutils::create_reserve(&e, &pool, &underlying_0, &reserve_config, &reserve_data_0);

        let (underlying_1, _) = testutils::create_token_contract(&e, &bombadil);
        let (reserve_config, reserve_data_1) = testutils::default_reserve_meta();
        testutils::create_reserve(&e, &pool, &underlying_1, &reserve_config, &reserve_data_1);

        e.ledger().set(LedgerInfo {
            timestamp: 100,
            protocol_version: 22,
            sequence_number: 1234,
            network_id: Default::default(),
            base_reserve: 10,
            min_temp_entry_ttl: 10,
            min_persistent_entry_ttl: 10,
            max_entry_ttl: 3110400,
        });
        let pool_config = PoolConfig {
            oracle: Address::generate(&e),
            min_collateral: 1_0000000,
            bstop_rate: 0_1000000,
            status: 1,
            max_positions: 5,
        };
        let positions = Positions {
            liabilities: map![&e, (0, 1_5000000), (1, 50_987_654_321)],
            collateral: map![&e],
            supply: map![&e, (0, 100_1234567)],
        };
        let backstop_positions = Positions {
            liabilities: map![&e, (0, 0_5000000)],
            collateral: map![&e],
            supply: map![&e],
        };
        e.as_contract(&pool, || {
            storage::set_pool_config(&e, &pool_config);
            storage::set_user_positions(&e, &samwise, &positions);
            storage::set_user_positions(&e, &backstop_address, &backstop_positions);

            bad_debt(&e, &samwise);

            // assert user forgiven liabilities and assigned to backstop
            let post_positions = storage::get_user_positions(&e, &samwise);
            assert_eq!(post_positions.liabilities.len(), 0);
            assert_eq!(post_positions.collateral.len(), 0);
            assert_eq!(post_positions.supply, positions.supply);

            let post_backstop_positions = storage::get_user_positions(&e, &backstop_address);
            assert_eq!(
                post_backstop_positions.liabilities,
                map![&e, (0, 0_5000000 + 1_5000000), (1, 50_987_654_321)]
            );
            assert_eq!(post_backstop_positions.collateral.len(), 0);
            assert_eq!(post_backstop_positions.supply.len(), 0);

            // assert pool reserves updated
            let post_reserve_data_0 = storage::get_res_data(&e, &underlying_0);
            assert_eq!(post_reserve_data_0.last_time, 100);
            assert_eq!(post_reserve_data_0.d_supply, reserve_data_0.d_supply);
            assert!(post_reserve_data_0.d_rate > reserve_data_0.d_rate);
            assert_eq!(post_reserve_data_0.b_supply, reserve_data_0.b_supply);
            assert!(post_reserve_data_0.b_rate > reserve_data_0.b_rate);
            let post_reserve_data_1 = storage::get_res_data(&e, &underlying_1);
            assert_eq!(post_reserve_data_1.last_time, 100);
            assert_eq!(post_reserve_data_1.d_supply, reserve_data_1.d_supply);
            assert!(post_reserve_data_1.d_rate > reserve_data_1.d_rate);
            assert_eq!(post_reserve_data_0.b_supply, reserve_data_0.b_supply);
            assert!(post_reserve_data_0.b_rate > reserve_data_0.b_rate);
        });
    }

    #[test]
    #[should_panic(expected = "Error(Contract, #1212)")]
    fn test_bad_debt_user_with_ongoing_auction() {
        let e = Env::default();
        e.cost_estimate().budget().reset_unlimited();
        e.mock_all_auths();

        let pool = create_pool(&e);
        let bombadil = Address::generate(&e);
        let frodo = Address::generate(&e);
        let samwise = Address::generate(&e);

        let (blnd, blnd_client) = create_blnd_token(&e, &pool, &bombadil);
        let (usdc, usdc_client) = create_token_contract(&e, &bombadil);
        let (lp_token, lp_token_client) = create_comet_lp_pool(&e, &bombadil, &blnd, &usdc);
        let (backstop_address, backstop_client) =
            create_backstop(&e, &pool, &lp_token, &usdc, &blnd);

        // mint lp tokens and deposit them into the pool's backstop
        let backstop_tokens = 1_500_0000000; // over 5% of threshold
        blnd_client.mint(&frodo, &500_001_0000000);
        blnd_client.approve(&frodo, &lp_token, &i128::MAX, &99999);
        usdc_client.mint(&frodo, &12_501_0000000);
        usdc_client.approve(&frodo, &lp_token, &i128::MAX, &99999);
        lp_token_client.join_pool(
            &backstop_tokens,
            &vec![&e, 500_001_0000000, 12_501_0000000],
            &frodo,
        );
        backstop_client.deposit(&frodo, &pool, &backstop_tokens);

        let (underlying_0, _) = testutils::create_token_contract(&e, &bombadil);
        let (reserve_config, reserve_data_0) = testutils::default_reserve_meta();
        testutils::create_reserve(&e, &pool, &underlying_0, &reserve_config, &reserve_data_0);

        let (underlying_1, _) = testutils::create_token_contract(&e, &bombadil);
        let (reserve_config, reserve_data_1) = testutils::default_reserve_meta();
        testutils::create_reserve(&e, &pool, &underlying_1, &reserve_config, &reserve_data_1);

        e.ledger().set(LedgerInfo {
            timestamp: 100,
            protocol_version: 22,
            sequence_number: 1234,
            network_id: Default::default(),
            base_reserve: 10,
            min_temp_entry_ttl: 10,
            min_persistent_entry_ttl: 10,
            max_entry_ttl: 3110400,
        });
        let pool_config = PoolConfig {
            oracle: Address::generate(&e),
            min_collateral: 1_0000000,
            bstop_rate: 0_1000000,
            status: 1,
            max_positions: 5,
        };
        let positions = Positions {
            liabilities: map![&e, (0, 1_5000000), (1, 50_987_654_321)],
            collateral: map![&e],
            supply: map![&e, (0, 100_1234567)],
        };
        let backstop_positions = Positions {
            liabilities: map![&e, (0, 0_5000000)],
            collateral: map![&e],
            supply: map![&e],
        };
        let auction = AuctionData {
            bid: map![&e],
            block: 0,
            lot: map![&e],
        };
        e.as_contract(&pool, || {
            storage::set_pool_config(&e, &pool_config);
            storage::set_user_positions(&e, &samwise, &positions);
            storage::set_user_positions(&e, &backstop_address, &backstop_positions);
            storage::set_auction(
                &e,
                &(AuctionType::UserLiquidation as u32),
                &samwise,
                &auction,
            );

            bad_debt(&e, &samwise);
        });
    }

    #[test]
    #[should_panic(expected = "Error(Contract, #1200)")]
    fn test_bad_debt_backstop_no_change() {
        let e = Env::default();
        e.cost_estimate().budget().reset_unlimited();
        e.mock_all_auths();

        let pool = create_pool(&e);
        let bombadil = Address::generate(&e);
        let frodo = Address::generate(&e);

        let (blnd, blnd_client) = create_blnd_token(&e, &pool, &bombadil);
        let (usdc, usdc_client) = create_token_contract(&e, &bombadil);
        let (lp_token, lp_token_client) = create_comet_lp_pool(&e, &bombadil, &blnd, &usdc);
        let (backstop_address, backstop_client) =
            create_backstop(&e, &pool, &lp_token, &usdc, &blnd);

        // mint lp tokens and deposit them into the pool's backstop
        let backstop_tokens = 1_500_0000000; // over 5% of threshold
        blnd_client.mint(&frodo, &500_001_0000000);
        blnd_client.approve(&frodo, &lp_token, &i128::MAX, &99999);
        usdc_client.mint(&frodo, &12_501_0000000);
        usdc_client.approve(&frodo, &lp_token, &i128::MAX, &99999);
        lp_token_client.join_pool(
            &backstop_tokens,
            &vec![&e, 500_001_0000000, 12_501_0000000],
            &frodo,
        );
        backstop_client.deposit(&frodo, &pool, &backstop_tokens);

        let (underlying_0, _) = testutils::create_token_contract(&e, &bombadil);
        let (reserve_config, reserve_data_0) = testutils::default_reserve_meta();
        testutils::create_reserve(&e, &pool, &underlying_0, &reserve_config, &reserve_data_0);

        let (underlying_1, _) = testutils::create_token_contract(&e, &bombadil);
        let (reserve_config, reserve_data_1) = testutils::default_reserve_meta();
        testutils::create_reserve(&e, &pool, &underlying_1, &reserve_config, &reserve_data_1);

        e.ledger().set(LedgerInfo {
            timestamp: 100,
            protocol_version: 22,
            sequence_number: 1234,
            network_id: Default::default(),
            base_reserve: 10,
            min_temp_entry_ttl: 10,
            min_persistent_entry_ttl: 10,
            max_entry_ttl: 3110400,
        });
        let pool_config = PoolConfig {
            oracle: Address::generate(&e),
            min_collateral: 1_0000000,
            bstop_rate: 0_1000000,
            status: 1,
            max_positions: 5,
        };
        let backstop_positions = Positions {
            liabilities: map![&e, (0, 1_5000000), (1, 3_5000000)],
            collateral: map![&e],
            supply: map![&e],
        };
        e.as_contract(&pool, || {
            storage::set_pool_config(&e, &pool_config);
            storage::set_user_positions(&e, &backstop_address, &backstop_positions);

            bad_debt(&e, &backstop_address);
        });
    }

    #[test]
    fn test_bad_debt_backstop() {
        let e = Env::default();
        e.cost_estimate().budget().reset_unlimited();
        e.mock_all_auths();

        let pool = create_pool(&e);
        let bombadil = Address::generate(&e);
        let frodo = Address::generate(&e);

        let (blnd, blnd_client) = create_blnd_token(&e, &pool, &bombadil);
        let (usdc, usdc_client) = create_token_contract(&e, &bombadil);
        let (lp_token, lp_token_client) = create_comet_lp_pool(&e, &bombadil, &blnd, &usdc);
        let (backstop_address, backstop_client) =
            create_backstop(&e, &pool, &lp_token, &usdc, &blnd);

        // mint lp tokens and deposit them into the pool's backstop
        let backstop_tokens = 1_000_0000000; // under 5% of threshold
        blnd_client.mint(&frodo, &500_001_0000000);
        blnd_client.approve(&frodo, &lp_token, &i128::MAX, &99999);
        usdc_client.mint(&frodo, &12_501_0000000);
        usdc_client.approve(&frodo, &lp_token, &i128::MAX, &99999);
        lp_token_client.join_pool(
            &backstop_tokens,
            &vec![&e, 500_001_0000000, 12_501_0000000],
            &frodo,
        );
        backstop_client.deposit(&frodo, &pool, &backstop_tokens);

        let (underlying_0, _) = testutils::create_token_contract(&e, &bombadil);
        let (reserve_config, reserve_data_0) = testutils::default_reserve_meta();
        testutils::create_reserve(&e, &pool, &underlying_0, &reserve_config, &reserve_data_0);

        let (underlying_1, _) = testutils::create_token_contract(&e, &bombadil);
        let (reserve_config, reserve_data_1) = testutils::default_reserve_meta();
        testutils::create_reserve(&e, &pool, &underlying_1, &reserve_config, &reserve_data_1);

        e.ledger().set(LedgerInfo {
            timestamp: 100,
            protocol_version: 22,
            sequence_number: 1234,
            network_id: Default::default(),
            base_reserve: 10,
            min_temp_entry_ttl: 10,
            min_persistent_entry_ttl: 10,
            max_entry_ttl: 3110400,
        });
        let pool_config = PoolConfig {
            oracle: Address::generate(&e),
            min_collateral: 1_0000000,
            bstop_rate: 0_1000000,
            status: 1,
            max_positions: 5,
        };
        let backstop_positions = Positions {
            liabilities: map![&e, (0, 1_5000000), (1, 3_5000000)],
            collateral: map![&e],
            supply: map![&e],
        };
        e.as_contract(&pool, || {
            storage::set_pool_config(&e, &pool_config);
            storage::set_user_positions(&e, &backstop_address, &backstop_positions);

            bad_debt(&e, &backstop_address);

            // assert backstop forgiven liabilities
            let post_backstop_positions = storage::get_user_positions(&e, &backstop_address);
            assert_eq!(post_backstop_positions.liabilities.len(), 0);
            assert_eq!(
                post_backstop_positions.collateral,
                backstop_positions.collateral
            );
            assert_eq!(post_backstop_positions.supply, backstop_positions.supply);

            // assert pool reserves updated
            let post_reserve_data_0 = storage::get_res_data(&e, &underlying_0);
            assert_eq!(post_reserve_data_0.last_time, 100);
            assert!(post_reserve_data_0.d_supply < reserve_data_0.d_supply);
            assert!(post_reserve_data_0.d_rate > reserve_data_0.d_rate);
            assert_eq!(post_reserve_data_0.b_supply, reserve_data_0.b_supply);
            assert!(post_reserve_data_0.b_rate < reserve_data_0.b_rate);
            let post_reserve_data_1 = storage::get_res_data(&e, &underlying_1);
            assert_eq!(post_reserve_data_1.last_time, 100);
            assert!(post_reserve_data_1.d_supply < reserve_data_1.d_supply);
            assert!(post_reserve_data_1.d_rate > reserve_data_1.d_rate);
            assert_eq!(post_reserve_data_1.b_supply, reserve_data_1.b_supply);
            assert!(post_reserve_data_1.b_rate < reserve_data_1.b_rate);
        });
    }

    #[test]
    #[should_panic(expected = "Error(Contract, #1212)")]
    fn test_bad_debt_backstop_ongoing_auction() {
        let e = Env::default();
        e.cost_estimate().budget().reset_unlimited();
        e.mock_all_auths();

        let pool = create_pool(&e);
        let bombadil = Address::generate(&e);
        let frodo = Address::generate(&e);

        let (blnd, blnd_client) = create_blnd_token(&e, &pool, &bombadil);
        let (usdc, usdc_client) = create_token_contract(&e, &bombadil);
        let (lp_token, lp_token_client) = create_comet_lp_pool(&e, &bombadil, &blnd, &usdc);
        let (backstop_address, backstop_client) =
            create_backstop(&e, &pool, &lp_token, &usdc, &blnd);

        // mint lp tokens and deposit them into the pool's backstop
        let backstop_tokens = 1_000_0000000; // under 5% of threshold
        blnd_client.mint(&frodo, &500_001_0000000);
        blnd_client.approve(&frodo, &lp_token, &i128::MAX, &99999);
        usdc_client.mint(&frodo, &12_501_0000000);
        usdc_client.approve(&frodo, &lp_token, &i128::MAX, &99999);
        lp_token_client.join_pool(
            &backstop_tokens,
            &vec![&e, 500_001_0000000, 12_501_0000000],
            &frodo,
        );
        backstop_client.deposit(&frodo, &pool, &backstop_tokens);

        let (underlying_0, _) = testutils::create_token_contract(&e, &bombadil);
        let (reserve_config, reserve_data_0) = testutils::default_reserve_meta();
        testutils::create_reserve(&e, &pool, &underlying_0, &reserve_config, &reserve_data_0);

        let (underlying_1, _) = testutils::create_token_contract(&e, &bombadil);
        let (reserve_config, reserve_data_1) = testutils::default_reserve_meta();
        testutils::create_reserve(&e, &pool, &underlying_1, &reserve_config, &reserve_data_1);

        e.ledger().set(LedgerInfo {
            timestamp: 100,
            protocol_version: 22,
            sequence_number: 1234,
            network_id: Default::default(),
            base_reserve: 10,
            min_temp_entry_ttl: 10,
            min_persistent_entry_ttl: 10,
            max_entry_ttl: 3110400,
        });
        let pool_config = PoolConfig {
            oracle: Address::generate(&e),
            min_collateral: 1_0000000,
            bstop_rate: 0_1000000,
            status: 1,
            max_positions: 5,
        };
        let backstop_positions = Positions {
            liabilities: map![&e, (0, 1_5000000), (1, 3_5000000)],
            collateral: map![&e],
            supply: map![&e],
        };
        let auction = AuctionData {
            bid: map![&e],
            block: 0,
            lot: map![&e],
        };
        e.as_contract(&pool, || {
            storage::set_pool_config(&e, &pool_config);
            storage::set_user_positions(&e, &backstop_address, &backstop_positions);
            storage::set_auction(
                &e,
                &(AuctionType::BadDebtAuction as u32),
                &backstop_address,
                &auction,
            );

            bad_debt(&e, &backstop_address);
        });
    }

    /***** check_and_handle_user_bad_debt *****/

    #[test]
    fn test_check_and_handle_user_bad_debt_with_collateral() {
        let e = Env::default();
        e.cost_estimate().budget().reset_unlimited();
        e.mock_all_auths();

        let pool = create_pool(&e);
        let bombadil = Address::generate(&e);
        let frodo = Address::generate(&e);
        let samwise = Address::generate(&e);

        let (blnd, blnd_client) = create_blnd_token(&e, &pool, &bombadil);
        let (usdc, usdc_client) = create_token_contract(&e, &bombadil);
        let (lp_token, lp_token_client) = create_comet_lp_pool(&e, &bombadil, &blnd, &usdc);
        let (_, backstop_client) = create_backstop(&e, &pool, &lp_token, &usdc, &blnd);

        // mint lp tokens and deposit them into the pool's backstop
        let backstop_tokens = 1_500_0000000; // over 5% of threshold
        blnd_client.mint(&frodo, &500_001_0000000);
        blnd_client.approve(&frodo, &lp_token, &i128::MAX, &99999);
        usdc_client.mint(&frodo, &12_501_0000000);
        usdc_client.approve(&frodo, &lp_token, &i128::MAX, &99999);
        lp_token_client.join_pool(
            &backstop_tokens,
            &vec![&e, 500_001_0000000, 12_501_0000000],
            &frodo,
        );
        backstop_client.deposit(&frodo, &pool, &backstop_tokens);

        let (underlying_0, _) = testutils::create_token_contract(&e, &bombadil);
        let (reserve_config, reserve_data) = testutils::default_reserve_meta();
        testutils::create_reserve(&e, &pool, &underlying_0, &reserve_config, &reserve_data);

        let (underlying_1, _) = testutils::create_token_contract(&e, &bombadil);
        let (reserve_config, reserve_data) = testutils::default_reserve_meta();
        testutils::create_reserve(&e, &pool, &underlying_1, &reserve_config, &reserve_data);

        e.ledger().set(LedgerInfo {
            timestamp: 100,
            protocol_version: 22,
            sequence_number: 1234,
            network_id: Default::default(),
            base_reserve: 10,
            min_temp_entry_ttl: 10,
            min_persistent_entry_ttl: 10,
            max_entry_ttl: 3110400,
        });
        let pool_config = PoolConfig {
            oracle: Address::generate(&e),
            min_collateral: 1_0000000,
            bstop_rate: 0_1000000,
            status: 1,
            max_positions: 5,
        };
        let positions = Positions {
            liabilities: map![&e, (0, 1_5000000), (1, 50_987_654_321)],
            collateral: map![&e, (0, 100_1234567)],
            supply: map![&e],
        };
        e.as_contract(&pool, || {
            storage::set_pool_config(&e, &pool_config);
            storage::set_user_positions(&e, &samwise, &positions);

            let mut pool = Pool::load(&e);
            let mut user = User::load(&e, &samwise);

            let result = check_and_handle_user_bad_debt(&e, &mut pool, &samwise, &mut user);
            assert_eq!(result, false);

            // assert user not modified
            assert_eq!(user.positions.liabilities, positions.liabilities);
            assert_eq!(user.positions.collateral, positions.collateral);
            assert_eq!(user.positions.supply, positions.supply);

            // assert no pool reserves were loaded
            assert_eq!(pool.reserves.len(), 0);
        });
    }

    #[test]
    fn test_check_and_handle_user_bad_debt() {
        let e = Env::default();
        e.cost_estimate().budget().reset_unlimited();
        e.mock_all_auths();

        let pool = create_pool(&e);
        let bombadil = Address::generate(&e);
        let frodo = Address::generate(&e);
        let samwise = Address::generate(&e);

        let (blnd, blnd_client) = create_blnd_token(&e, &pool, &bombadil);
        let (usdc, usdc_client) = create_token_contract(&e, &bombadil);
        let (lp_token, lp_token_client) = create_comet_lp_pool(&e, &bombadil, &blnd, &usdc);
        let (backstop_address, backstop_client) =
            create_backstop(&e, &pool, &lp_token, &usdc, &blnd);

        // mint lp tokens and deposit them into the pool's backstop
        let backstop_tokens = 1_500_0000000; // over 5% of threshold
        blnd_client.mint(&frodo, &500_001_0000000);
        blnd_client.approve(&frodo, &lp_token, &i128::MAX, &99999);
        usdc_client.mint(&frodo, &12_501_0000000);
        usdc_client.approve(&frodo, &lp_token, &i128::MAX, &99999);
        lp_token_client.join_pool(
            &backstop_tokens,
            &vec![&e, 500_001_0000000, 12_501_0000000],
            &frodo,
        );
        backstop_client.deposit(&frodo, &pool, &backstop_tokens);

        let (underlying_0, _) = testutils::create_token_contract(&e, &bombadil);
        let (reserve_config, reserve_data_0) = testutils::default_reserve_meta();
        testutils::create_reserve(&e, &pool, &underlying_0, &reserve_config, &reserve_data_0);

        let (underlying_1, _) = testutils::create_token_contract(&e, &bombadil);
        let (reserve_config, reserve_data_1) = testutils::default_reserve_meta();
        testutils::create_reserve(&e, &pool, &underlying_1, &reserve_config, &reserve_data_1);

        e.ledger().set(LedgerInfo {
            timestamp: 100,
            protocol_version: 22,
            sequence_number: 1234,
            network_id: Default::default(),
            base_reserve: 10,
            min_temp_entry_ttl: 10,
            min_persistent_entry_ttl: 10,
            max_entry_ttl: 3110400,
        });
        let pool_config = PoolConfig {
            oracle: Address::generate(&e),
            min_collateral: 1_0000000,
            bstop_rate: 0_1000000,
            status: 1,
            max_positions: 5,
        };
        let positions = Positions {
            liabilities: map![&e, (0, 1_5000000), (1, 50_987_654_321)],
            collateral: map![&e],
            supply: map![&e, (0, 100_1234567)],
        };
        let backstop_positions = Positions {
            liabilities: map![&e, (0, 0_5000000)],
            collateral: map![&e],
            supply: map![&e],
        };
        e.as_contract(&pool, || {
            storage::set_pool_config(&e, &pool_config);
            storage::set_user_positions(&e, &samwise, &positions);
            storage::set_user_positions(&e, &backstop_address, &backstop_positions);

            let mut pool = Pool::load(&e);
            let mut user = User::load(&e, &samwise);

            let result = check_and_handle_user_bad_debt(&e, &mut pool, &samwise, &mut user);
            assert_eq!(result, true);

            // assert user forgiven liabilities and assigned to backstop
            assert_eq!(user.positions.liabilities.len(), 0);
            assert_eq!(user.positions.collateral.len(), 0);
            assert_eq!(user.positions.supply, positions.supply);

            let post_backstop_positions = storage::get_user_positions(&e, &backstop_address);
            assert_eq!(
                post_backstop_positions.liabilities,
                map![&e, (0, 0_5000000 + 1_5000000), (1, 50_987_654_321)]
            );
            assert_eq!(post_backstop_positions.collateral.len(), 0);
            assert_eq!(post_backstop_positions.supply.len(), 0);

            // store pool reserves and assert they got updated
            pool.store_cached_reserves(&e);
            let post_reserve_data_0 = storage::get_res_data(&e, &underlying_0);
            assert_eq!(post_reserve_data_0.last_time, 100);
            assert_eq!(post_reserve_data_0.d_supply, reserve_data_0.d_supply);
            assert!(post_reserve_data_0.d_rate > reserve_data_0.d_rate);
            assert_eq!(post_reserve_data_0.b_supply, reserve_data_0.b_supply);
            assert!(post_reserve_data_0.b_rate > reserve_data_0.b_rate);
            let post_reserve_data_1 = storage::get_res_data(&e, &underlying_1);
            assert_eq!(post_reserve_data_1.last_time, 100);
            assert_eq!(post_reserve_data_1.d_supply, reserve_data_1.d_supply);
            assert!(post_reserve_data_1.d_rate > reserve_data_1.d_rate);
            assert_eq!(post_reserve_data_0.b_supply, reserve_data_0.b_supply);
            assert!(post_reserve_data_0.b_rate > reserve_data_0.b_rate);
        });
    }

    /***** check_and_handle_backstop_bad_debt *****/

    #[test]
    fn test_check_and_handle_backstop_bad_debt() {
        let e = Env::default();
        e.cost_estimate().budget().reset_unlimited();
        e.mock_all_auths();

        let pool = create_pool(&e);
        let bombadil = Address::generate(&e);
        let frodo = Address::generate(&e);

        let (blnd, blnd_client) = create_blnd_token(&e, &pool, &bombadil);
        let (usdc, usdc_client) = create_token_contract(&e, &bombadil);
        let (lp_token, lp_token_client) = create_comet_lp_pool(&e, &bombadil, &blnd, &usdc);
        let (backstop_address, backstop_client) =
            create_backstop(&e, &pool, &lp_token, &usdc, &blnd);

        // mint lp tokens and deposit them into the pool's backstop
        let backstop_tokens = 1_500_0000000; // over 5% of threshold
        blnd_client.mint(&frodo, &500_001_0000000);
        blnd_client.approve(&frodo, &lp_token, &i128::MAX, &99999);
        usdc_client.mint(&frodo, &12_501_0000000);
        usdc_client.approve(&frodo, &lp_token, &i128::MAX, &99999);
        lp_token_client.join_pool(
            &backstop_tokens,
            &vec![&e, 500_001_0000000, 12_501_0000000],
            &frodo,
        );
        backstop_client.deposit(&frodo, &pool, &backstop_tokens);

        let (underlying_0, _) = testutils::create_token_contract(&e, &bombadil);
        let (reserve_config, reserve_data_0) = testutils::default_reserve_meta();
        testutils::create_reserve(&e, &pool, &underlying_0, &reserve_config, &reserve_data_0);

        let (underlying_1, _) = testutils::create_token_contract(&e, &bombadil);
        let (reserve_config, reserve_data_1) = testutils::default_reserve_meta();
        testutils::create_reserve(&e, &pool, &underlying_1, &reserve_config, &reserve_data_1);

        e.ledger().set(LedgerInfo {
            timestamp: 100,
            protocol_version: 22,
            sequence_number: 1234,
            network_id: Default::default(),
            base_reserve: 10,
            min_temp_entry_ttl: 10,
            min_persistent_entry_ttl: 10,
            max_entry_ttl: 3110400,
        });
        let pool_config = PoolConfig {
            oracle: Address::generate(&e),
            min_collateral: 1_0000000,
            bstop_rate: 0_1000000,
            status: 1,
            max_positions: 5,
        };
        let backstop_positions = Positions {
            liabilities: map![&e, (0, 1_5000000), (1, 3_5000000)],
            collateral: map![&e],
            supply: map![&e],
        };
        e.as_contract(&pool, || {
            storage::set_pool_config(&e, &pool_config);
            storage::set_user_positions(&e, &backstop_address, &backstop_positions);

            let mut pool = Pool::load(&e);
            let mut backstop_user = User::load(&e, &backstop_address);

            let result = check_and_handle_backstop_bad_debt(
                &e,
                &mut pool,
                &backstop_address,
                &mut backstop_user,
            );
            assert_eq!(result, false);

            // assert nothing happens to backstop position
            assert_eq!(
                backstop_user.positions.liabilities,
                backstop_positions.liabilities
            );
            assert_eq!(
                backstop_user.positions.collateral,
                backstop_positions.collateral
            );
            assert_eq!(backstop_user.positions.supply, backstop_positions.supply);

            // assert no pool reserves were loaded
            assert_eq!(pool.reserves.len(), 0);
        });
    }

    #[test]
    fn test_check_and_handle_backstop_bad_debt_with_no_liabilities() {
        let e = Env::default();
        e.cost_estimate().budget().reset_unlimited();
        e.mock_all_auths();

        let pool = create_pool(&e);
        let bombadil = Address::generate(&e);
        let frodo = Address::generate(&e);

        let (blnd, blnd_client) = create_blnd_token(&e, &pool, &bombadil);
        let (usdc, usdc_client) = create_token_contract(&e, &bombadil);
        let (lp_token, lp_token_client) = create_comet_lp_pool(&e, &bombadil, &blnd, &usdc);
        let (backstop_address, backstop_client) =
            create_backstop(&e, &pool, &lp_token, &usdc, &blnd);

        // mint lp tokens and deposit them into the pool's backstop
        let backstop_tokens = 1_000_0000000; // under 5% of threshold
        blnd_client.mint(&frodo, &500_001_0000000);
        blnd_client.approve(&frodo, &lp_token, &i128::MAX, &99999);
        usdc_client.mint(&frodo, &12_501_0000000);
        usdc_client.approve(&frodo, &lp_token, &i128::MAX, &99999);
        lp_token_client.join_pool(
            &backstop_tokens,
            &vec![&e, 500_001_0000000, 12_501_0000000],
            &frodo,
        );
        backstop_client.deposit(&frodo, &pool, &backstop_tokens);

        let (underlying_0, _) = testutils::create_token_contract(&e, &bombadil);
        let (reserve_config, reserve_data_0) = testutils::default_reserve_meta();
        testutils::create_reserve(&e, &pool, &underlying_0, &reserve_config, &reserve_data_0);

        let (underlying_1, _) = testutils::create_token_contract(&e, &bombadil);
        let (reserve_config, reserve_data_1) = testutils::default_reserve_meta();
        testutils::create_reserve(&e, &pool, &underlying_1, &reserve_config, &reserve_data_1);

        e.ledger().set(LedgerInfo {
            timestamp: 100,
            protocol_version: 22,
            sequence_number: 1234,
            network_id: Default::default(),
            base_reserve: 10,
            min_temp_entry_ttl: 10,
            min_persistent_entry_ttl: 10,
            max_entry_ttl: 3110400,
        });
        let pool_config = PoolConfig {
            oracle: Address::generate(&e),
            min_collateral: 1_0000000,
            bstop_rate: 0_1000000,
            status: 1,
            max_positions: 5,
        };
        let backstop_positions = Positions {
            liabilities: map![&e],
            collateral: map![&e],
            supply: map![&e],
        };
        e.as_contract(&pool, || {
            storage::set_pool_config(&e, &pool_config);
            storage::set_user_positions(&e, &backstop_address, &backstop_positions);

            let mut pool = Pool::load(&e);
            let mut backstop_user = User::load(&e, &backstop_address);

            let result = check_and_handle_backstop_bad_debt(
                &e,
                &mut pool,
                &backstop_address,
                &mut backstop_user,
            );
            assert_eq!(result, false);

            // assert nothing happens to backstop position
            assert_eq!(
                backstop_user.positions.liabilities,
                backstop_positions.liabilities
            );
            assert_eq!(
                backstop_user.positions.collateral,
                backstop_positions.collateral
            );
            assert_eq!(backstop_user.positions.supply, backstop_positions.supply);

            // assert no pool reserves were loaded
            assert_eq!(pool.reserves.len(), 0);
        });
    }

    #[test]
    fn test_check_and_handle_backstop_bad_debt_with_unhealthy_backstop_defaults() {
        let e = Env::default();
        e.cost_estimate().budget().reset_unlimited();
        e.mock_all_auths();

        let pool = create_pool(&e);
        let bombadil = Address::generate(&e);
        let frodo = Address::generate(&e);

        let (blnd, blnd_client) = create_blnd_token(&e, &pool, &bombadil);
        let (usdc, usdc_client) = create_token_contract(&e, &bombadil);
        let (lp_token, lp_token_client) = create_comet_lp_pool(&e, &bombadil, &blnd, &usdc);
        let (backstop_address, backstop_client) =
            create_backstop(&e, &pool, &lp_token, &usdc, &blnd);

        // mint lp tokens and deposit them into the pool's backstop
        let backstop_tokens = 1_000_0000000; // under 5% of threshold
        blnd_client.mint(&frodo, &500_001_0000000);
        blnd_client.approve(&frodo, &lp_token, &i128::MAX, &99999);
        usdc_client.mint(&frodo, &12_501_0000000);
        usdc_client.approve(&frodo, &lp_token, &i128::MAX, &99999);
        lp_token_client.join_pool(
            &backstop_tokens,
            &vec![&e, 500_001_0000000, 12_501_0000000],
            &frodo,
        );
        backstop_client.deposit(&frodo, &pool, &backstop_tokens);

        let (underlying_0, _) = testutils::create_token_contract(&e, &bombadil);
        let (reserve_config, reserve_data_0) = testutils::default_reserve_meta();
        testutils::create_reserve(&e, &pool, &underlying_0, &reserve_config, &reserve_data_0);

        let (underlying_1, _) = testutils::create_token_contract(&e, &bombadil);
        let (reserve_config, reserve_data_1) = testutils::default_reserve_meta();
        testutils::create_reserve(&e, &pool, &underlying_1, &reserve_config, &reserve_data_1);

        e.ledger().set(LedgerInfo {
            timestamp: 100,
            protocol_version: 22,
            sequence_number: 1234,
            network_id: Default::default(),
            base_reserve: 10,
            min_temp_entry_ttl: 10,
            min_persistent_entry_ttl: 10,
            max_entry_ttl: 3110400,
        });
        let pool_config = PoolConfig {
            oracle: Address::generate(&e),
            min_collateral: 1_0000000,
            bstop_rate: 0_1000000,
            status: 1,
            max_positions: 5,
        };
        let backstop_positions = Positions {
            liabilities: map![&e, (0, 1_5000000), (1, 3_5000000)],
            collateral: map![&e],
            supply: map![&e],
        };
        e.as_contract(&pool, || {
            storage::set_pool_config(&e, &pool_config);
            storage::set_user_positions(&e, &backstop_address, &backstop_positions);

            let mut pool = Pool::load(&e);
            let mut backstop_user = User::load(&e, &backstop_address);

            let result = check_and_handle_backstop_bad_debt(
                &e,
                &mut pool,
                &backstop_address,
                &mut backstop_user,
            );
            assert_eq!(result, true);

            // assert backstop user updated
            assert_eq!(backstop_user.positions.liabilities.len(), 0);
            assert_eq!(
                backstop_user.positions.collateral,
                backstop_positions.collateral
            );
            assert_eq!(backstop_user.positions.supply, backstop_positions.supply);

            // store pool reserves and assert they got updated
            pool.store_cached_reserves(&e);
            let post_reserve_data_0 = storage::get_res_data(&e, &underlying_0);
            assert_eq!(post_reserve_data_0.last_time, 100);
            assert!(post_reserve_data_0.d_supply < reserve_data_0.d_supply);
            assert!(post_reserve_data_0.d_rate > reserve_data_0.d_rate);
            assert_eq!(post_reserve_data_0.b_supply, reserve_data_0.b_supply);
            assert!(post_reserve_data_0.b_rate < reserve_data_0.b_rate);
            let post_reserve_data_1 = storage::get_res_data(&e, &underlying_1);
            assert_eq!(post_reserve_data_1.last_time, 100);
            assert!(post_reserve_data_1.d_supply < reserve_data_1.d_supply);
            assert!(post_reserve_data_1.d_rate > reserve_data_1.d_rate);
            assert_eq!(post_reserve_data_1.b_supply, reserve_data_1.b_supply);
            assert!(post_reserve_data_1.b_rate < reserve_data_1.b_rate);
        });
    }
}
