use soroban_sdk::{panic_with_error, Address, Env};

use crate::{events::PoolEvents, PoolError};

use super::{bad_debt::require_no_b_token_emissions, Pool, User};

/// Burn pool-owned orphan b-tokens once this reserve has no remaining debt.
///
/// Available in every status. Returns zero underlying and never recognizes token surplus.
/// Contradictory b-token emissions state or outstanding debt raises `BadRequest`.
pub fn execute_gulp(e: &Env, asset: &Address) -> i128 {
    let mut pool = Pool::load(e);
    let mut reserve = pool.load_reserve(e, asset, false);
    require_no_b_token_emissions(e, reserve.config.index);
    if reserve.data.d_supply > 0 {
        panic_with_error!(e, PoolError::BadRequest);
    }

    let mut pool_user = User::load(e, &e.current_contract_address());
    let amount = pool_user.get_supply(reserve.config.index);
    if amount == 0 {
        return 0;
    }

    pool_user.remove_supply(e, &mut reserve, amount);
    reserve.store(e);
    pool_user.store(e);
    PoolEvents::orphan_settled(e, asset.clone(), amount);
    0
}

#[cfg(test)]
mod tests {
    use crate::constants::{SCALAR_12, SCALAR_7};
    use crate::pool::{bad_debt, execute_gulp};
    use crate::storage::{self, PoolConfig, ReserveEmissionData, UserEmissionData};
    use crate::testutils;
    use sep_40_oracle::testutils::Asset;
    use soroban_sdk::{
        map,
        testutils::{Address as _, Events, Ledger, LedgerInfo},
        vec, Address, Env, IntoVal, Symbol,
    };

    #[test]
    fn test_execute_gulp_settles_orphan() {
        let e = Env::default();
        e.mock_all_auths();
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
        let bombadil = Address::generate(&e);
        let pool = testutils::create_pool(&e);

        let initial_backstop_credit = 500;
        let underlying_client_minted = 10 * SCALAR_7; // stray surplus the fork must ignore
        let orphan_supply = 3_4242_0000000;
        let (underlying, underlying_client) = testutils::create_token_contract(&e, &bombadil);
        let (reserve_config, mut reserve_data) = testutils::default_reserve_meta();
        reserve_data.b_rate = 1_000_000_000_000;
        reserve_data.d_rate = 1_000_000_000_000;
        reserve_data.d_supply = 0;
        reserve_data.b_supply = 1000 * SCALAR_7 + orphan_supply;
        reserve_data.backstop_credit = initial_backstop_credit;
        reserve_data.last_time = 100;
        testutils::create_reserve(&e, &pool, &underlying, &reserve_config, &reserve_data);

        // pool contract holds noncollateral supply for this reserve and stray raw tokens
        e.as_contract(&pool, || {
            let pool_user_positions = crate::storage::get_user_positions(&e, &pool);
            let mut positions = pool_user_positions.clone();
            positions.supply.set(0, orphan_supply);
            crate::storage::set_user_positions(&e, &pool, &positions);
        });
        underlying_client.mint(&pool, &underlying_client_minted);

        e.as_contract(&pool, || {
            let pool_config = PoolConfig {
                oracle: Address::generate(&e),
                min_collateral: 1_0000000,
                bstop_rate: 0_1000000,
                status: 2, // terminal settlement is available in every status
                max_positions: 4,
            };
            storage::set_pool_config(&e, &pool_config);

            let pre_balance = underlying_client.balance(&pool);
            let pre_events = e.events().all().len();
            let token_delta_result = execute_gulp(&e, &underlying);
            assert_eq!(token_delta_result, 0);

            // exact orphan event in b-token units as last emitted event
            let event = vec![&e, e.events().all().last_unchecked()];
            assert_eq!(
                event,
                vec![
                    &e,
                    (
                        pool.clone(),
                        (Symbol::new(&e, "orphan_settled"), underlying.clone()).into_val(&e),
                        orphan_supply.into_val(&e),
                    )
                ]
            );
            assert_eq!(e.events().all().len(), pre_events + 1);

            // exactly the pool-owned supply was removed from the reserve cache/store
            let new_reserve_data = storage::get_res_data(&e, &underlying);
            assert_eq!(
                new_reserve_data.b_supply,
                reserve_data.b_supply - orphan_supply
            );
            assert_eq!(new_reserve_data.d_supply, 0);
            assert_eq!(new_reserve_data.d_rate, 1_000_000_000_000);
            assert_eq!(new_reserve_data.last_time, 100);
            // no backstop credit mutation and no raw token movement
            assert_eq!(new_reserve_data.backstop_credit, initial_backstop_credit);
            assert_eq!(underlying_client.balance(&pool), pre_balance);

            // pool user position store cleared for this reserve
            let post_pool_positions = storage::get_user_positions(&e, &pool);
            assert_eq!(post_pool_positions.supply.len(), 0); // remove-on-zero deletes key
            assert_eq!(post_pool_positions.collateral.len(), 0);
        });
    }

    #[test]
    #[should_panic(expected = "Error(Contract, #1200)")]
    fn test_execute_gulp_rejects_debt_positive() {
        let e = Env::default();
        e.mock_all_auths();
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
        let bombadil = Address::generate(&e);
        let pool = testutils::create_pool(&e);

        let (underlying, _) = testutils::create_token_contract(&e, &bombadil);
        let (reserve_config, mut reserve_data) = testutils::default_reserve_meta();
        reserve_data.b_rate = 1_000_000_000_000;
        reserve_data.d_rate = 1_000_000_000_000;
        reserve_data.d_supply = 500 * SCALAR_7;
        reserve_data.b_supply = 1000 * SCALAR_7;
        reserve_data.last_time = 100;
        testutils::create_reserve(&e, &pool, &underlying, &reserve_config, &reserve_data);

        e.as_contract(&pool, || {
            let pool_config = PoolConfig {
                oracle: Address::generate(&e),
                min_collateral: 1_0000000,
                bstop_rate: 0_1000000,
                status: 1,
                max_positions: 4,
            };
            storage::set_pool_config(&e, &pool_config);

            execute_gulp(&e, &underlying);
        });
    }

    #[test]
    fn test_execute_gulp_zero_delta_skips() {
        let e = Env::default();
        e.mock_all_auths_allowing_non_root_auth();
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
        let bombadil = Address::generate(&e);
        let pool = testutils::create_pool(&e);

        let (underlying, _) = testutils::create_token_contract(&e, &bombadil);
        let (reserve_config, mut reserve_data) = testutils::default_reserve_meta();
        reserve_data.b_rate = 1_000_000_000_000;
        reserve_data.d_rate = 1_000_000_000_000;
        reserve_data.d_supply = 0;
        reserve_data.b_supply = 1000 * SCALAR_7;
        reserve_data.backstop_credit = 0;
        reserve_data.last_time = 0;
        testutils::create_reserve(&e, &pool, &underlying, &reserve_config, &reserve_data);

        e.as_contract(&pool, || {
            let pool_config = PoolConfig {
                oracle: Address::generate(&e),
                min_collateral: 1_0000000,
                bstop_rate: 0_1000000,
                status: 0,
                max_positions: 4,
            };
            storage::set_pool_config(&e, &pool_config);

            let pre_events = e.events().all().len();
            let token_delta_result = execute_gulp(&e, &underlying);
            assert_eq!(token_delta_result, 0);

            // zero-orphan early return: nothing stored and no orphan event
            assert_eq!(e.events().all().len(), pre_events);
            let new_reserve_data = storage::get_res_data(&e, &underlying);
            assert_eq!(new_reserve_data.b_rate, 1_000_000_000_000);
            assert_eq!(new_reserve_data.last_time, 0);
            assert_eq!(new_reserve_data.backstop_credit, 0);
        });
    }

    #[test]
    #[should_panic(expected = "Error(Contract, #1200)")]
    fn test_execute_gulp_rejects_pool_map_emissions() {
        let e = Env::default();
        e.mock_all_auths();
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
        let bombadil = Address::generate(&e);
        let pool = testutils::create_pool(&e);

        let (underlying, _) = testutils::create_token_contract(&e, &bombadil);
        let (reserve_config, mut reserve_data) = testutils::default_reserve_meta();
        reserve_data.b_rate = 1_000_000_000_000;
        reserve_data.d_rate = 1_000_000_000_000;
        reserve_data.d_supply = 0;
        reserve_data.b_supply = 1000 * SCALAR_7;
        reserve_data.last_time = 100;
        testutils::create_reserve(&e, &pool, &underlying, &reserve_config, &reserve_data);

        e.as_contract(&pool, || {
            let pool_config = PoolConfig {
                oracle: Address::generate(&e),
                min_collateral: 1_0000000,
                bstop_rate: 0_1000000,
                status: 1,
                max_positions: 4,
            };
            storage::set_pool_config(&e, &pool_config);

            let res_index = storage::get_res_list(&e)
                .first_index_of(&underlying)
                .unwrap();
            let mut emissions = storage::get_pool_emissions(&e);
            emissions.set(res_index * 2 + 1, 1_0000000);
            storage::set_pool_emissions(&e, &emissions);

            execute_gulp(&e, &underlying);
        });
    }

    #[test]
    #[should_panic(expected = "Error(Contract, #1200)")]
    fn test_execute_gulp_rejects_res_emis_data() {
        let e = Env::default();
        e.mock_all_auths();
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
        let bombadil = Address::generate(&e);
        let pool = testutils::create_pool(&e);

        let (underlying, _) = testutils::create_token_contract(&e, &bombadil);
        let (reserve_config, mut reserve_data) = testutils::default_reserve_meta();
        reserve_data.b_rate = 1_000_000_000_000;
        reserve_data.d_rate = 1_000_000_000_000;
        reserve_data.d_supply = 0;
        reserve_data.b_supply = 1000 * SCALAR_7;
        reserve_data.last_time = 100;
        testutils::create_reserve(&e, &pool, &underlying, &reserve_config, &reserve_data);

        e.as_contract(&pool, || {
            let pool_config = PoolConfig {
                oracle: Address::generate(&e),
                min_collateral: 1_0000000,
                bstop_rate: 0_1000000,
                status: 1,
                max_positions: 4,
            };
            storage::set_pool_config(&e, &pool_config);

            let res_index = storage::get_res_list(&e)
                .first_index_of(&underlying)
                .unwrap();
            storage::set_res_emis_data(
                &e,
                &(res_index * 2 + 1),
                &ReserveEmissionData {
                    expiration: 200,
                    eps: 1_0000000,
                    index: SCALAR_12 * res_index as i128,
                    last_time: 100,
                },
            );

            execute_gulp(&e, &underlying);
        });
    }

    #[test]
    #[should_panic(expected = "Error(Contract, #1200)")]
    fn test_execute_gulp_rejects_pool_user_emissions() {
        let e = Env::default();
        e.mock_all_auths();
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
        let bombadil = Address::generate(&e);
        let pool = testutils::create_pool(&e);

        let (underlying, _) = testutils::create_token_contract(&e, &bombadil);
        let (reserve_config, mut reserve_data) = testutils::default_reserve_meta();
        reserve_data.b_rate = 1_000_000_000_000;
        reserve_data.d_rate = 1_000_000_000_000;
        reserve_data.d_supply = 0;
        reserve_data.b_supply = 1000 * SCALAR_7;
        reserve_data.last_time = 100;
        testutils::create_reserve(&e, &pool, &underlying, &reserve_config, &reserve_data);

        e.as_contract(&pool, || {
            let pool_config = PoolConfig {
                oracle: Address::generate(&e),
                min_collateral: 1_0000000,
                bstop_rate: 0_1000000,
                status: 1,
                max_positions: 4,
            };
            storage::set_pool_config(&e, &pool_config);

            let res_index = storage::get_res_list(&e)
                .first_index_of(&underlying)
                .unwrap();
            storage::set_user_emissions(
                &e,
                &pool,
                &(res_index * 2 + 1),
                &UserEmissionData {
                    index: SCALAR_12 * res_index as i128,
                    accrued: 0,
                },
            );

            execute_gulp(&e, &underlying);
        });
    }

    /// Full custody→settlement round trip through the public entries on ONE reserve:
    /// bad_debt relocates a zero-raw-collateral borrower's b-tokens into pool custody,
    /// then gulp burns exactly that pool-owned orphan supply.
    #[test]
    fn test_gulp_settles_collateral_relocated_by_bad_debt() {
        let e = Env::default();
        e.cost_estimate().budget().reset_unlimited();
        e.mock_all_auths();
        let pool = testutils::create_pool(&e);
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

        // 50 tokens of debt, 1 stroop residual collateral at price 50 => raw floors to zero;
        // default moves that 1-stroop b-token into the pool contract's supply
        let (underlying, _) = testutils::create_token_contract(&e, &bombadil);
        let (reserve_config, mut reserve_data) = testutils::default_reserve_meta();
        reserve_data.last_time = 100;
        reserve_data.d_supply = 50_0000000;
        testutils::create_reserve(&e, &pool, &underlying, &reserve_config, &reserve_data);

        oracle_client.set_data(
            &bombadil,
            &Asset::Other(Symbol::new(&e, "USD")),
            &vec![&e, Asset::Stellar(underlying.clone())],
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
        let positions = crate::Positions {
            liabilities: map![&e, (0, 50_0000000)],
            collateral: map![&e, (0, 1)],
            supply: map![&e],
        };
        e.as_contract(&pool, || {
            storage::set_pool_config(&e, &pool_config);
            storage::set_user_positions(&e, &samwise, &positions);
            storage::set_backstop(&e, &backstop_address);

            bad_debt(&e, &samwise); // emits defaulted_debt + collateral_orphaned(1)

            // custody witness: pool owns exactly the relocated 1-stroop b-token
            let custody_positions = storage::get_user_positions(&e, &pool);
            assert_eq!(custody_positions.supply.get(0).unwrap(), 1);
            let post_reserve_data = storage::get_res_data(&e, &underlying);
            assert_eq!(post_reserve_data.b_supply, reserve_data.b_supply); // default reduces only d_supply/b_rate; the custody move nets b_supply to zero delta

            // settlement round trip through public gulp
            let token_delta_result = execute_gulp(&e, &underlying);
            assert_eq!(token_delta_result, 0);

            let event = vec![&e, e.events().all().last_unchecked()];
            assert_eq!(
                event,
                vec![
                    &e,
                    (
                        pool.clone(),
                        (Symbol::new(&e, "orphan_settled"), underlying.clone()).into_val(&e),
                        1i128.into_val(&e),
                    )
                ]
            );

            let settled_reserve_data = storage::get_res_data(&e, &underlying);
            assert_eq!(
                settled_reserve_data.b_supply,
                post_reserve_data.b_supply - 1
            );
            assert_eq!(settled_reserve_data.d_supply, 0);

            let settled_pool_positions = storage::get_user_positions(&e, &pool);
            assert_eq!(settled_pool_positions.supply.len(), 0); // remove-on-zero deletes key

            // borrower state stays fully forgiven after both steps
            let post_positions = storage::get_user_positions(&e, &samwise);
            assert_eq!(post_positions.liabilities.len(), 0);
            assert_eq!(post_positions.collateral.len(), 0);
        });
    }
}
