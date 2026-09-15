use soroban_sdk::{panic_with_error, Address, Env, Vec};

use crate::{
    dependencies::BackstopClient, events::PoolEvents, math::FixedMath, storage,
    storage::ReserveData, AuctionType, PoolError,
};

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
        let claim = user_state.get_supply(index);
        let setoff = supply_setoff(e, &reserve.data, amount, claim);
        if setoff.repaid > 0 {
            user_state.remove_supply(e, &mut reserve, setoff.burn);
            user_state.remove_liabilities(e, &mut reserve, setoff.repaid);
        }
        if setoff.has_default() {
            user_state.default_liabilities(e, &mut reserve, setoff.residual);
            had_default = true;
            PoolEvents::defaulted_debt(e, asset, setoff.residual);
        }
        pool.cache_reserve(reserve);
    }

    if should_orphan_collateral(had_default, !collateral.is_empty()) {
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

struct SupplySetoff {
    burn: i128,
    repaid: i128,
    residual: i128,
}

impl SupplySetoff {
    fn has_default(&self) -> bool {
        self.residual > 0
    }
}

/// Calculate only committed setoff; rounded-zero repayment leaves the claim intact.
fn supply_setoff(
    math: &impl FixedMath,
    reserve: &ReserveData,
    amount: i128,
    claim: i128,
) -> SupplySetoff {
    if claim > 0 && reserve.b_rate > 0 {
        let debt_assets = reserve.to_asset_from_d_token(math, amount);
        let b_tokens = claim.min(reserve.to_b_token_up(math, debt_assets));
        let covered_assets = reserve.to_asset_from_b_token(math, b_tokens);
        let repaid = amount.min(reserve.to_d_token_down(math, covered_assets));
        if repaid > 0 {
            return SupplySetoff {
                burn: b_tokens,
                repaid,
                residual: amount - repaid,
            };
        }
    }
    SupplySetoff {
        burn: 0,
        repaid: 0,
        residual: amount,
    }
}

fn should_orphan_collateral(had_default: bool, has_collateral: bool) -> bool {
    had_default && has_collateral
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
        storage::{self, PoolConfig, ReserveEmissionData, UserEmissionData},
        testutils::{
            self, create_backstop, create_blnd_token, create_comet_lp_pool, create_pool,
            create_token_contract,
        },
        Positions,
    };
    use sep_40_oracle::testutils::Asset;
    use soroban_sdk::{
        map,
        testutils::{Address as _, Events, Ledger, LedgerInfo},
        vec, Address, IntoVal, Symbol,
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

        // the fork valuates eligibility through the strict oracle before rejecting
        let (oracle, oracle_client) = testutils::create_mock_oracle(&e);

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
        oracle_client.set_data(
            &bombadil,
            &Asset::Other(Symbol::new(&e, "USD")),
            &vec![
                &e,
                Asset::Stellar(underlying_0.clone()),
                Asset::Stellar(underlying_1.clone()),
            ],
            &7,
            &300,
        );
        oracle_client.set_price_stable(&vec![&e, 1_0000000, 1_0000000]);
        let pool_config = PoolConfig {
            oracle,
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
        let samwise = Address::generate(&e);
        let backstop_address = Address::generate(&e);

        let (oracle, oracle_client) = testutils::create_mock_oracle(&e);
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

        // liabilities on two reserves, no collateral: both default directly to suppliers
        let (underlying_0, _) = testutils::create_token_contract(&e, &bombadil);
        let (reserve_config, mut reserve_data_0) = testutils::default_reserve_meta();
        reserve_data_0.d_supply = 2_0000000;
        reserve_data_0.last_time = 100;
        testutils::create_reserve(&e, &pool, &underlying_0, &reserve_config, &reserve_data_0);

        let (underlying_1, _) = testutils::create_token_contract(&e, &bombadil);
        let (reserve_config, mut reserve_data_1) = testutils::default_reserve_meta();
        reserve_data_1.d_supply = 50_987_654_321;
        reserve_data_1.b_supply = reserve_data_1.d_supply;
        reserve_data_1.last_time = 100;
        testutils::create_reserve(&e, &pool, &underlying_1, &reserve_config, &reserve_data_1);

        oracle_client.set_data(
            &bombadil,
            &Asset::Other(Symbol::new(&e, "USD")),
            &vec![
                &e,
                Asset::Stellar(underlying_0.clone()),
                Asset::Stellar(underlying_1.clone()),
            ],
            &7,
            &300,
        );
        oracle_client.set_price_stable(&vec![&e, 1_0000000, 1_0000000]);
        let pool_config = PoolConfig {
            oracle,
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
        e.as_contract(&pool, || {
            storage::set_pool_config(&e, &pool_config);
            storage::set_user_positions(&e, &samwise, &positions);
            storage::set_backstop(&e, &backstop_address);

            let pre_events = e.events().all().len();
            bad_debt(&e, &samwise);

            // liabilities are forgiven after same-reserve ordinary supply is
            // burned against reserve 0 before any supplier loss.
            let post_positions = storage::get_user_positions(&e, &samwise);
            assert_eq!(post_positions.liabilities.len(), 0);
            assert_eq!(post_positions.collateral.len(), 0);
            assert_eq!(post_positions.supply, map![&e, (0, 986_234_567i128)]);

            // the fork never assigns debt to, or reads state from, the backstop
            let post_backstop_positions = storage::get_user_positions(&e, &backstop_address);
            assert_eq!(post_backstop_positions.liabilities.len(), 0);
            assert_eq!(post_backstop_positions.collateral.len(), 0);
            assert_eq!(post_backstop_positions.supply.len(), 0);

            // Only the uncovered reserve 1 liability is socialized.
            let all_events = e.events().all();
            assert_eq!(all_events.len(), pre_events + 1);
            assert_eq!(
                vec![&e, all_events.last_unchecked()],
                vec![
                    &e,
                    (
                        pool.clone(),
                        (Symbol::new(&e, "defaulted_debt"), underlying_1.clone()).into_val(&e),
                        50_987_654_321i128.into_val(&e),
                    )
                ]
            );

            // Reserve 0 clears through same-user setoff without a rate loss.
            let post_reserve_data_0 = storage::get_res_data(&e, &underlying_0);
            assert_eq!(post_reserve_data_0.last_time, 100);
            assert_eq!(
                post_reserve_data_0.d_supply,
                reserve_data_0.d_supply - 1_5000000
            );
            assert_eq!(post_reserve_data_0.d_rate, reserve_data_0.d_rate);
            assert_eq!(
                post_reserve_data_0.b_supply,
                reserve_data_0.b_supply - 1_5000000
            );
            assert_eq!(post_reserve_data_0.b_rate, reserve_data_0.b_rate);

            let post_reserve_data_1 = storage::get_res_data(&e, &underlying_1);
            assert_eq!(post_reserve_data_1.last_time, 100);
            assert_eq!(
                post_reserve_data_1.d_supply,
                reserve_data_1.d_supply - 50_987_654_321
            );
            assert_eq!(post_reserve_data_1.d_rate, reserve_data_1.d_rate);
            assert_eq!(post_reserve_data_1.b_supply, reserve_data_1.b_supply);
            assert_eq!(post_reserve_data_1.b_rate, 0); // clamped at zero
            assert_eq!(post_reserve_data_1.d_rate, reserve_data_1.d_rate);
        });
    }

    #[test]
    fn test_bad_debt_orphaned_same_reserve_cycle() {
        let e = Env::default();
        e.cost_estimate().budget().reset_unlimited();
        e.mock_all_auths();

        let pool = create_pool(&e);
        let bombadil = Address::generate(&e);
        let samwise = Address::generate(&e);
        let backstop_address = Address::generate(&e);

        let (oracle, oracle_client) = testutils::create_mock_oracle(&e);

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

        // same reserve supplies the liability and a dust collateral whose raw
        // value floors to zero under the oracle price
        let (underlying_0, _) = testutils::create_token_contract(&e, &bombadil);
        let (reserve_config, mut reserve_data) = testutils::default_reserve_meta();
        reserve_data.last_time = 100;
        testutils::create_reserve(&e, &pool, &underlying_0, &reserve_config, &reserve_data);

        oracle_client.set_data(
            &bombadil,
            &Asset::Other(Symbol::new(&e, "USD")),
            &vec![&e, Asset::Stellar(underlying_0.clone())],
            &7,
            &300,
        );
        oracle_client.set_price_stable(&vec![&e, 50]);

        let pool_config = PoolConfig {
            oracle,
            min_collateral: 1_0000000,
            bstop_rate: 0_1000000,
            status: 1,
            max_positions: 5,
        };
        let positions = Positions {
            liabilities: map![&e, (0, 50_0000000)],
            collateral: map![&e, (0, 1)], // 1 stroop of b-token floors to zero raw
            supply: map![&e],
        };
        e.as_contract(&pool, || {
            storage::set_pool_config(&e, &pool_config);
            storage::set_user_positions(&e, &samwise, &positions);
            storage::set_backstop(&e, &backstop_address);

            let pre_events = e.events().all().len();
            bad_debt(&e, &samwise);

            let post_positions = storage::get_user_positions(&e, &samwise);
            assert_eq!(post_positions.liabilities.len(), 0);
            assert_eq!(post_positions.collateral.len(), 0);

            // residual collateral moved into the pool contract's noncollateral supply
            let pool_positions = storage::get_user_positions(&e, &pool);
            assert_eq!(pool_positions.supply.len(), 1);
            assert_eq!(pool_positions.supply.get(0).unwrap(), 1);

            // defaulted_debt then collateral_orphaned, nothing else
            let all_events = e.events().all();
            assert_eq!(all_events.len(), pre_events + 2);
            assert_eq!(
                vec![&e, all_events.get_unchecked(all_events.len() - 2)],
                vec![
                    &e,
                    (
                        pool.clone(),
                        (Symbol::new(&e, "defaulted_debt"), underlying_0.clone()).into_val(&e),
                        50_0000000i128.into_val(&e),
                    )
                ]
            );
            assert_eq!(
                vec![&e, all_events.last_unchecked()],
                vec![
                    &e,
                    (
                        pool.clone(),
                        (
                            Symbol::new(&e, "collateral_orphaned"),
                            samwise.clone(),
                            underlying_0.clone(),
                        )
                            .into_val(&e),
                        1i128.into_val(&e), // b-token units
                    )
                ]
            );

            // stale-cache guard: default and relocation hit ONE authoritative cached
            // reserve; b_supply nets out exactly and d_supply keeps its full decrease
            let post_reserve_data = storage::get_res_data(&e, &underlying_0);
            assert_eq!(post_reserve_data.last_time, 100);
            assert_eq!(post_reserve_data.d_supply, 25_0000000);
            assert_eq!(post_reserve_data.b_supply, reserve_data.b_supply);
            assert_eq!(post_reserve_data.b_rate, 500_000_000_000);
            assert_eq!(post_reserve_data.d_rate, reserve_data.d_rate);
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
    #[should_panic(expected = "Error(Contract, #1200)")]
    fn test_bad_debt_backstop() {
        let e = Env::default();
        e.cost_estimate().budget().reset_unlimited();
        e.mock_all_auths();

        let pool = create_pool(&e);
        let backstop_address = Address::generate(&e);

        // the fork rejects every bad-debt call on the backstop before any state read
        e.as_contract(&pool, || {
            storage::set_backstop(&e, &backstop_address);
            bad_debt(&e, &backstop_address);
        });
    }

    #[test]
    #[should_panic(expected = "Error(Contract, #1200)")]
    fn test_bad_debt_backstop_rejection_precedes_auction() {
        let e = Env::default();
        e.cost_estimate().budget().reset_unlimited();
        e.mock_all_auths();

        let pool = create_pool(&e);
        let backstop_address = Address::generate(&e);

        // bad-debt backstop rejection (#1200) fires before the auction check even when a
        // BadDebtAuction for the backstop is live — the fork has no backstop liquidation path
        e.as_contract(&pool, || {
            storage::set_backstop(&e, &backstop_address);
            storage::set_auction(
                &e,
                &(AuctionType::BadDebtAuction as u32),
                &backstop_address,
                &AuctionData {
                    bid: map![&e],
                    block: 0,
                    lot: map![&e],
                },
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
        let (oracle, oracle_client) = testutils::create_mock_oracle(&e);

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
        // strict-oracle valuation still sees positive raw collateral -> ineligible
        oracle_client.set_data(
            &bombadil,
            &Asset::Other(Symbol::new(&e, "USD")),
            &vec![
                &e,
                Asset::Stellar(underlying_0.clone()),
                Asset::Stellar(underlying_1.clone()),
            ],
            &7,
            &300,
        );
        oracle_client.set_price_stable(&vec![&e, 1_0000000, 1_0000000]);
        let pool_config = PoolConfig {
            oracle,
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
            // valuation cached both reserves during the strict-oracle eligibility check
            assert_eq!(pool.reserves.len(), 2);
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
        let (reserve_config, mut reserve_data_0) = testutils::default_reserve_meta();
        reserve_data_0.last_time = 100; // keep exact-math asserts free of interest accrual
        testutils::create_reserve(&e, &pool, &underlying_0, &reserve_config, &reserve_data_0);

        let (underlying_1, _) = testutils::create_token_contract(&e, &bombadil);
        let (reserve_config, mut reserve_data_1) = testutils::default_reserve_meta();
        reserve_data_1.d_supply = 50_987_654_321;
        reserve_data_1.b_supply = reserve_data_1.d_supply;
        reserve_data_1.last_time = 100;
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
        let (oracle, oracle_client) = testutils::create_mock_oracle(&e);

        // two-reserve liabilities default through the shared Pool cache; the stock
        // backstop-assignment path is gone, so this fixture keeps a real backstop only
        // to witness that its positions are left untouched
        oracle_client.set_data(
            &bombadil,
            &Asset::Other(Symbol::new(&e, "USD")),
            &vec![
                &e,
                Asset::Stellar(underlying_0.clone()),
                Asset::Stellar(underlying_1.clone()),
            ],
            &7,
            &300,
        );
        oracle_client.set_price_stable(&vec![&e, 1_0000000, 1_0000000]);
        let pool_config = PoolConfig {
            oracle,
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

            let pre_events = e.events().all().len();
            let mut pool = Pool::load(&e);
            let mut user = User::load(&e, &samwise);

            let result = check_and_handle_user_bad_debt(&e, &mut pool, &samwise, &mut user);
            assert_eq!(result, true);

            // same-reserve ordinary supply clears the first liability before any supplier loss
            assert_eq!(user.positions.liabilities.len(), 0);
            assert_eq!(user.positions.collateral.len(), 0);
            assert_eq!(user.positions.supply, map![&e, (0, 98_6234567)]);

            // the fork never moves debt onto the backstop
            let post_backstop_positions = storage::get_user_positions(&e, &backstop_address);
            assert_eq!(
                post_backstop_positions.liabilities,
                backstop_positions.liabilities
            );
            assert_eq!(post_backstop_positions.collateral.len(), 0);
            assert_eq!(post_backstop_positions.supply.len(), 0);

            // Store reserves and retain an event only for supplier-socialized debt.
            pool.store_cached_reserves(&e);
            let all_events = e.events().all();
            assert_eq!(all_events.len(), pre_events + 1);
            assert_eq!(
                vec![&e, all_events.last_unchecked()],
                vec![
                    &e,
                    (
                        e.current_contract_address(),
                        (Symbol::new(&e, "defaulted_debt"), underlying_1.clone()).into_val(&e),
                        50_987_654_321i128.into_val(&e),
                    )
                ]
            );

            let post_reserve_data_0 = storage::get_res_data(&e, &underlying_0);
            assert_eq!(post_reserve_data_0.last_time, 100);
            assert_eq!(
                post_reserve_data_0.d_supply,
                reserve_data_0.d_supply - 1_5000000
            );
            assert_eq!(post_reserve_data_0.d_rate, reserve_data_0.d_rate);
            assert_eq!(
                post_reserve_data_0.b_supply,
                reserve_data_0.b_supply - 1_5000000
            );
            assert_eq!(post_reserve_data_0.b_rate, reserve_data_0.b_rate);
            let post_reserve_data_1 = storage::get_res_data(&e, &underlying_1);
            assert_eq!(post_reserve_data_1.last_time, 100);
            assert_eq!(
                post_reserve_data_1.d_supply,
                reserve_data_1.d_supply - 50_987_654_321
            );
            assert_eq!(post_reserve_data_1.d_rate, reserve_data_1.d_rate);
            assert_eq!(post_reserve_data_1.b_supply, reserve_data_1.b_supply);
            assert_eq!(post_reserve_data_1.b_rate, 0); // clamped at zero
        });
    }

    /***** custody-path emissions precheck *****/

    /// Shared dust-collateral fixture: liabilities on reserve 0, 1-stroop residual
    /// collateral (floors to zero raw at price 50), so the emissions precheck runs.
    fn setup_emissions_probe(e: &Env) -> (Address, Address, Address) {
        e.cost_estimate().budget().reset_unlimited();
        e.mock_all_auths();
        let pool = create_pool(e);
        let bombadil = Address::generate(e);
        let samwise = Address::generate(e);
        let backstop_address = Address::generate(e);

        let (oracle, oracle_client) = testutils::create_mock_oracle(e);

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

        let (underlying_0, _) = testutils::create_token_contract(e, &bombadil);
        let (reserve_config, mut reserve_data) = testutils::default_reserve_meta();
        reserve_data.last_time = 100;
        testutils::create_reserve(e, &pool, &underlying_0, &reserve_config, &reserve_data);

        oracle_client.set_data(
            &bombadil,
            &Asset::Other(Symbol::new(e, "USD")),
            &vec![e, Asset::Stellar(underlying_0.clone())],
            &7,
            &300,
        );
        oracle_client.set_price_stable(&vec![e, 50]);

        let pool_config = PoolConfig {
            oracle,
            min_collateral: 1_0000000,
            bstop_rate: 0_1000000,
            status: 1,
            max_positions: 5,
        };
        let positions = Positions {
            liabilities: map![e, (0, 50_0000000)],
            collateral: map![e, (0, 1)],
            supply: map![e],
        };
        e.as_contract(&pool, || {
            storage::set_pool_config(e, &pool_config);
            storage::set_user_positions(e, &samwise, &positions);
            storage::set_backstop(e, &backstop_address);
        });
        (pool, underlying_0, samwise)
    }

    #[test]
    fn test_full_setoff_retains_collateral_with_emissions() {
        let e = Env::default();
        let (pool_address, asset, samwise) = setup_emissions_probe(&e);

        e.as_contract(&pool_address, || {
            let mut pool = Pool::load(&e);
            let mut user = User::load(&e, &samwise);
            let mut reserve = pool.load_reserve(&e, &asset, true);
            let debt = user.get_liabilities(0);
            let claim = reserve.to_b_token_up(&e, reserve.to_asset_from_d_token(&e, debt));
            user.add_supply(&e, &mut reserve, claim);
            let expected_b_supply = reserve.data.b_supply - claim;
            let expected_d_supply = reserve.data.d_supply - debt;
            let expected_b_rate = reserve.data.b_rate;
            pool.cache_reserve(reserve);
            let collateral = user.positions.collateral.clone();
            let pool_positions = User::load(&e, &pool_address).positions;
            let mut emissions = storage::get_pool_emissions(&e);
            emissions.set(1, 1_0000000);
            storage::set_pool_emissions(&e, &emissions);
            let events_before = e.events().all().len();

            assert!(check_and_handle_user_bad_debt(
                &e, &mut pool, &samwise, &mut user
            ));

            assert!(!user.has_liabilities());
            assert_eq!(user.get_supply(0), 0);
            assert_eq!(user.positions.collateral, collateral);
            assert_eq!(
                User::load(&e, &pool_address).positions.liabilities,
                pool_positions.liabilities
            );
            assert_eq!(
                User::load(&e, &pool_address).positions.collateral,
                pool_positions.collateral
            );
            assert_eq!(
                User::load(&e, &pool_address).positions.supply,
                pool_positions.supply
            );
            let reserve = pool.load_reserve(&e, &asset, true);
            assert_eq!(reserve.data.b_supply, expected_b_supply);
            assert_eq!(reserve.data.d_supply, expected_d_supply);
            assert_eq!(reserve.data.b_rate, expected_b_rate);
            assert_eq!(e.events().all().len(), events_before);
        });
    }

    #[test]
    #[should_panic(expected = "Error(Contract, #1200)")]
    fn test_custody_rejects_pool_map_b_token_emissions() {
        let e = Env::default();
        let (pool, underlying_0, samwise) = setup_emissions_probe(&e);

        e.as_contract(&pool, || {
            let res_index = storage::get_res_list(&e)
                .first_index_of(&underlying_0)
                .unwrap();
            let mut emissions = storage::get_pool_emissions(&e);
            emissions.set(res_index * 2 + 1, 1_0000000);
            storage::set_pool_emissions(&e, &emissions);

            let mut pool = Pool::load(&e);
            let mut user = User::load(&e, &samwise);
            check_and_handle_user_bad_debt(&e, &mut pool, &samwise, &mut user);
        });
    }

    #[test]
    #[should_panic(expected = "Error(Contract, #1200)")]
    fn test_custody_rejects_reserve_b_token_emission_data() {
        let e = Env::default();
        let (pool, underlying_0, samwise) = setup_emissions_probe(&e);

        e.as_contract(&pool, || {
            let res_index = storage::get_res_list(&e)
                .first_index_of(&underlying_0)
                .unwrap();
            storage::set_res_emis_data(
                &e,
                &(res_index * 2 + 1),
                &ReserveEmissionData {
                    expiration: 200,
                    eps: 1_0000000,
                    index: 0,
                    last_time: 100,
                },
            );

            let mut pool = Pool::load(&e);
            let mut user = User::load(&e, &samwise);
            check_and_handle_user_bad_debt(&e, &mut pool, &samwise, &mut user);
        });
    }

    #[test]
    #[should_panic(expected = "Error(Contract, #1200)")]
    fn test_custody_rejects_pool_address_user_emissions() {
        let e = Env::default();
        let (pool, underlying_0, samwise) = setup_emissions_probe(&e);

        e.as_contract(&pool, || {
            let res_index = storage::get_res_list(&e)
                .first_index_of(&underlying_0)
                .unwrap();
            storage::set_user_emissions(
                &e,
                &pool.clone(),
                &(res_index * 2 + 1),
                &UserEmissionData {
                    index: 0,
                    accrued: 0,
                },
            );

            let mut pool = Pool::load(&e);
            let mut user = User::load(&e, &samwise);
            check_and_handle_user_bad_debt(&e, &mut pool, &samwise, &mut user);
        });
    }

    #[test]
    #[should_panic(expected = "Error(Contract, #1200)")]
    fn test_public_bad_debt_rejects_no_liability_user_without_valuation() {
        let e = Env::default();
        e.cost_estimate().budget().reset_unlimited();
        e.mock_all_auths();

        let pool = create_pool(&e);
        let bombadil = Address::generate(&e);
        let samwise = Address::generate(&e);
        let backstop_address = Address::generate(&e);

        // no reserves exist: if the wrapper ever reached valuation it would panic on
        // missing reserves instead — the fork rejects before touching any of them
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
        e.as_contract(&pool, || {
            storage::set_pool_config(&e, &pool_config);
            storage::set_backstop(&e, &backstop_address);
            bad_debt(&e, &samwise); // no stored positions => has_liabilities false
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

#[cfg(kani)]
mod verification {
    use super::*;
    use crate::constants::SCALAR_12;

    /// Rates are symbolic u8 hundredths of SCALAR_12 (0..=2.55 x unity): a
    /// deliberate proof-domain restriction, not a protocol rate bound.
    fn rate_hundredths(u: u8) -> i128 {
        (u as i128) * SCALAR_12 / 100
    }

    fn setoff_data(d_rate: i128, b_rate: i128) -> ReserveData {
        ReserveData {
            d_rate,
            b_rate,
            ir_mod: 0,
            b_supply: 0,
            d_supply: 0,
            backstop_credit: 0,
            last_time: 0,
        }
    }

    /// C1 (docs/kani.md): the same-reserve supply-setoff kernel from
    /// check_and_handle_user_bad_debt. Establishes, independently of the
    /// kernel's own arithmetic, the exact accepted burn policy and rounding:
    /// the committed burn is exactly the supply claim capped by the
    /// ceil-rounded debt-covering b-tokens, repayment is exactly the
    /// floor-rounded d-tokens covered by those b-tokens capped by the debt,
    /// and the returned residual is the kernel-computed remainder the
    /// production consumer defaults on. A setoff is committed if and only if
    /// the claim and b_rate are positive and that repayment rounds above zero.
    /// Returning an arbitrarily smaller positive burn, or a residual computed
    /// anywhere but inside the kernel, fails these assertions.
    ///
    /// Domain: amount (d-token debt) positive u8 cast, claim (b-token supply)
    /// u8 cast including zero, d_rate positive u8 hundredths of unity, b_rate
    /// u8 hundredths of unity with zero explicitly included. Product fit: the
    /// largest intermediate numerator is 66,705 * SCALAR_12 (~6.7e16), far
    /// inside i128, so the dependency FixedPoint conversions equal the
    /// production SorobanFixedPoint fast path; the host I256 fallback is never
    /// taken on this domain and is not claimed. Oracle arithmetic is u64:
    /// every operand and product stays below 2^63 on this domain, so Kani's
    /// default overflow checks discharge representability and all casts are
    /// lossless. No one-unit dust bound is asserted anywhere.
    #[kani::proof]
    fn prove_setoff_exact_policy_split_and_conditions() {
        let amount_u: u8 = kani::any();
        let claim_u: u8 = kani::any();
        let d_rate_u: u8 = kani::any();
        let b_rate_u: u8 = kani::any();
        // Liabilities map entries are positive; a zero d-rate cannot divide.
        kani::assume(amount_u > 0 && d_rate_u > 0);
        let amount = amount_u as i128;
        let claim = claim_u as i128;
        let data = setoff_data(rate_hundredths(d_rate_u), rate_hundredths(b_rate_u));

        let setoff = supply_setoff(&(), &data, amount, claim);

        // Independent u64 oracle over the same inputs: mirrors only the
        // ReserveData rounding contract (ceil/floor of x*y/z with these exact
        // operands), not the kernel's implementation.
        let scale = SCALAR_12 as u64;
        let d_rate_o = d_rate_u as u64 * scale / 100;
        let b_rate_o = b_rate_u as u64 * scale / 100;
        let (b_exp, repaid_exp) = if claim_u > 0 && b_rate_o > 0 {
            let debt_assets = (amount_u as u64 * d_rate_o).div_ceil(scale);
            let b_target = (debt_assets * scale).div_ceil(b_rate_o);
            let b_cap = (claim_u as u64).min(b_target);
            let covered = b_cap * b_rate_o / scale;
            let repaid = (amount_u as u64).min(covered * scale / d_rate_o);
            (b_cap, repaid)
        } else {
            (0, 0)
        };

        // Commit happens iff the real conditions hold: positive claim, positive
        // b_rate, and repayment rounding above zero.
        assert!((setoff.repaid > 0) == (repaid_exp > 0));

        if repaid_exp > 0 {
            // Exact accepted burn policy and rounding (caps follow: burn <=
            // claim and repaid <= amount by the oracle's own mins).
            assert!(setoff.burn == b_exp as i128);
            assert!(setoff.repaid == repaid_exp as i128);
            assert!(setoff.burn > 0 && setoff.burn <= claim);
            assert!(setoff.repaid > 0 && setoff.repaid <= amount);
            // True kernel debt split: the residual is the kernel's own
            // subtraction, consumed by the production default branch.
            assert!(setoff.residual >= 0 && setoff.repaid + setoff.residual == amount);
        } else {
            // No commit: zero claim, zero b_rate, or repayment rounding to
            // zero leaves the claim and the whole debt untouched.
            assert!(setoff.repaid == 0 && setoff.burn == 0 && setoff.residual == amount);
        }

        // Corrected C3 link (docs/kani.md C3 correction): with positive debt,
        // no committed setoff means a residual default, not absence of
        // default; only full repayment avoids the default branch.
        assert!(setoff.has_default() == (setoff.residual > 0));
        if setoff.repaid == 0 {
            assert!(setoff.residual == amount && setoff.has_default());
        }

        kani::cover!(setoff.repaid > 0 && setoff.residual == 0);
        kani::cover!(setoff.repaid > 0 && setoff.residual > 0);
        kani::cover!(setoff.repaid == 0 && claim == 0);
        kani::cover!(setoff.repaid == 0 && claim > 0 && data.b_rate == 0);
        kani::cover!(setoff.repaid == 0 && claim > 0 && data.b_rate > 0);
    }

    /// C3 (docs/kani.md): the residual-collateral decision from
    /// check_and_handle_user_bad_debt. Collateral is orphaned into pool
    /// custody only when a default actually happened and collateral exists;
    /// no residual default retains the user's collateral. The storage, map,
    /// emission and event effects behind this decision remain host boundaries
    /// and are not claimed here.
    #[kani::proof]
    fn prove_orphan_collateral_decision() {
        let had_default: bool = kani::any();
        let has_collateral: bool = kani::any();
        let orphan = should_orphan_collateral(had_default, has_collateral);
        assert!(orphan == (had_default && has_collateral));
        if !had_default {
            assert!(!orphan);
        }
        if !has_collateral {
            assert!(!orphan);
        }
        kani::cover!(had_default && has_collateral);
        kani::cover!(had_default && !has_collateral);
        kani::cover!(!had_default && has_collateral);
        kani::cover!(!had_default && !has_collateral);
    }

    // C1 S2: parametric caller over the four original inputs only (the C1
    // domain: amount,d_rate positive u8; claim,b_rate u8 incl zero). The two
    // verified helper families bind every intermediate to the original u64
    // oracle before this caller may be counted toward C1; caller PASS alone is
    // structural, not numeric closure.
    struct SetoffMath {
        ceil_args: [(i128, i128, i128); 2],
        ceil_results: [i128; 2],
        floor_args: [(i128, i128, i128); 2],
        floor_results: [i128; 2],
        ceil_calls: core::cell::Cell<u32>,
        floor_calls: core::cell::Cell<u32>,
    }

    impl FixedMath for SetoffMath {
        fn floor(&self, x: i128, y: i128, denominator: i128) -> i128 {
            let slot = self.floor_calls.get() as usize;
            assert!(slot < 2);
            assert_eq!((x, y, denominator), self.floor_args[slot]);
            self.floor_calls.set(self.floor_calls.get() + 1);
            self.floor_results[slot]
        }

        fn ceil(&self, x: i128, y: i128, denominator: i128) -> i128 {
            let slot = self.ceil_calls.get() as usize;
            assert!(slot < 2);
            assert_eq!((x, y, denominator), self.ceil_args[slot]);
            self.ceil_calls.set(self.ceil_calls.get() + 1);
            self.ceil_results[slot]
        }
    }

    #[kani::proof]
    fn prove_setoff_guarded_parametric() {
        let amount_u: u8 = kani::any();
        let claim_u: u8 = kani::any();
        let d_rate_h: u8 = kani::any();
        let b_rate_h: u8 = kani::any();
        // Liabilities map entries are positive; a zero d-rate cannot divide.
        kani::assume(amount_u > 0 && d_rate_h > 0);
        let amount = amount_u as i128;
        let claim = claim_u as i128;
        let data = setoff_data(rate_hundredths(d_rate_h), rate_hundredths(b_rate_h));

        // Numeric identity with the original u64 oracle is composed only
        // after all S0/S1 prerequisites pass; no numeric oracle is assumed here.

        // Helper-derived premises for the committed branch, per S0/S1:
        //   d_forward:  debt_assets == ceil(amount_u*d_rate_o/scale)
        //               in 1..=651
        //   b_inverse_up: b_target == ceil(debt_assets*scale/b_rate_o)
        //               in 1..=65100
        // Outside the guard, both conversions bypass arithmetic.
        let setoff;
        let mut pay_burn: i128 = 0;
        let mut pay_q: i128 = 0;
        if claim_u > 0 && data.b_rate > 0 {
            // Arbitrary helper outputs over supersets of the real images.
            // S0/S1 establish the original u64 identities and these bounds.
            let debt_assets: i128 = kani::any();
            kani::assume(debt_assets >= 1 && debt_assets <= 651);
            let b_target: i128 = kani::any();
            kani::assume(b_target >= 1 && b_target <= 65100);
            let burn_cap = claim.min(b_target);
            assert!(burn_cap > 0 && burn_cap <= 255);
            let covered: i128 = kani::any();
            kani::assume(covered >= 0 && covered <= 650);
            let repaid_target: i128 = kani::any();
            kani::assume(repaid_target >= 0 && repaid_target <= 65100);
            // Zero input is preserved by the real d-inverse helper.
            // The converse is false: a positive asset amount can round to zero.
            kani::assume(covered != 0 || repaid_target == 0);

            let math = SetoffMath {
                ceil_args: [
                    (amount, data.d_rate, SCALAR_12),
                    (debt_assets, SCALAR_12, data.b_rate),
                ],
                ceil_results: [debt_assets, b_target],
                floor_args: [
                    (burn_cap, data.b_rate, SCALAR_12),
                    (covered, SCALAR_12, data.d_rate),
                ],
                floor_results: [covered, repaid_target],
                ceil_calls: core::cell::Cell::new(0),
                floor_calls: core::cell::Cell::new(0),
            };
            setoff = supply_setoff(&math, &data, amount, claim);
            assert_eq!(math.ceil_calls.get(), 2);
            assert_eq!(math.floor_calls.get(), 2);
            pay_burn = burn_cap;
            pay_q = repaid_target;
        } else {
            // Zero claim/rate bypass: no arithmetic call fires.
            let math = SetoffMath {
                ceil_args: [(0, 0, 0); 2],
                ceil_results: [0; 2],
                floor_args: [(0, 0, 0); 2],
                floor_results: [0; 2],
                ceil_calls: core::cell::Cell::new(0),
                floor_calls: core::cell::Cell::new(0),
            };
            setoff = supply_setoff(&math, &data, amount, claim);
            assert_eq!(math.ceil_calls.get() + math.floor_calls.get(), 0);
        }

        // Exact mirrored policy (original assertions bad_debt.rs:1573-1598).
        let repaid_exp = amount.min(pay_q);
        if repaid_exp > 0 {
            assert!(setoff.burn == pay_burn);
            assert!(setoff.repaid == repaid_exp);
            assert!(setoff.burn > 0 && setoff.burn <= claim);
            assert!(setoff.repaid > 0 && setoff.repaid <= amount);
            assert!(setoff.residual >= 0 && setoff.repaid + setoff.residual == amount);
        } else {
            assert!(setoff.repaid == 0 && setoff.burn == 0 && setoff.residual == amount);
        }
        assert!(setoff.has_default() == (setoff.residual > 0));

        kani::cover!(setoff.repaid > 0 && setoff.residual == 0);
        kani::cover!(setoff.repaid > 0 && setoff.residual > 0);
        kani::cover!(setoff.repaid == 0 && claim == 0);
        kani::cover!(setoff.repaid == 0 && claim > 0 && data.b_rate == 0);
        kani::cover!(setoff.repaid == 0 && claim > 0 && data.b_rate > 0);
    }

    /// Witness harness for the guarded caller: actual () arithmetic on the
    /// five approved tuples (SettlementProofDesign S2 covers). Each tuple must
    /// exercise its named policy branch under the real kernel with the real
    /// dependency implementation; no abstraction, no symbolic value.
    #[kani::proof]
    fn prove_setoff_guarded_witnesses() {
        let scale = SCALAR_12 as u64;
        // (amount_u, claim_u, d_h, b_h, label)
        let full = (1u8, 1u8, 100u8, 100u8);
        let partial = (2u8, 1u8, 100u8, 100u8);
        let zero_claim = (1u8, 0u8, 100u8, 100u8);
        let zero_b_rate = (1u8, 1u8, 100u8, 0u8);
        let rounded_zero = (1u8, 1u8, 100u8, 1u8);

        for params in [full, partial, zero_claim, zero_b_rate, rounded_zero] {
            let (a_u, c_u, d_h, b_h) = params;
            let amount = a_u as i128;
            let claim = c_u as i128;
            let data = setoff_data(rate_hundredths(d_h), rate_hundredths(b_h));
            let setoff = supply_setoff(&(), &data, amount, claim);

            // Independent oracle for the same tuple.
            let d_rate_o = d_h as u64 * scale / 100;
            let b_rate_o = b_h as u64 * scale / 100;
            let (b_exp, repaid_exp) = if c_u > 0 && b_rate_o > 0 {
                let debt_assets = (a_u as u64 * d_rate_o).div_ceil(scale);
                let b_target = (debt_assets * scale).div_ceil(b_rate_o);
                let b_cap = (c_u as u64).min(b_target);
                let covered = b_cap * b_rate_o / scale;
                let repaid = (a_u as u64).min(covered * scale / d_rate_o);
                (b_cap, repaid)
            } else {
                (0, 0)
            };

            if repaid_exp > 0 {
                assert!(setoff.burn == b_exp as i128);
                assert!(setoff.repaid == repaid_exp as i128);
                assert!(setoff.burn > 0 && setoff.burn <= claim);
                assert!(setoff.repaid > 0 && setoff.repaid <= amount);
                assert!(setoff.residual >= 0 && setoff.repaid + setoff.residual == amount);
            } else {
                assert!(setoff.repaid == 0 && setoff.burn == 0 && setoff.residual == amount);
            }
            assert!(setoff.has_default() == (setoff.residual > 0));
        }

        assert!(
            supply_setoff(
                &(),
                &setoff_data(rate_hundredths(full.2), rate_hundredths(full.3)),
                full.0 as i128,
                full.1 as i128
            )
            .residual
                == 0
        ); // full witness: whole debt settled
        assert!(
            supply_setoff(
                &(),
                &setoff_data(rate_hundredths(partial.2), rate_hundredths(partial.3)),
                partial.0 as i128,
                partial.1 as i128
            )
            .repaid
                < partial.0 as i128
        ); // partial witness: strictly partial repayment
        assert_eq!(
            supply_setoff(
                &(),
                &setoff_data(rate_hundredths(zero_claim.2), rate_hundredths(zero_claim.3)),
                zero_claim.0 as i128,
                0
            )
            .repaid,
            0
        ); // zero-claim witness
        assert_eq!(
            supply_setoff(
                &(),
                &setoff_data(
                    rate_hundredths(zero_b_rate.2),
                    rate_hundredths(zero_b_rate.3)
                ),
                zero_b_rate.0 as i128,
                zero_b_rate.1 as i128
            )
            .repaid,
            0
        ); // zero-b-rate witness
        assert_eq!(
            supply_setoff(
                &(),
                &setoff_data(
                    rate_hundredths(rounded_zero.2),
                    rate_hundredths(rounded_zero.3)
                ),
                rounded_zero.0 as i128,
                rounded_zero.1 as i128
            )
            .repaid,
            0
        ); // rounded-zero no-commit witness
    }
}
