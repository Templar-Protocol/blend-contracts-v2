#![cfg(test)]

mod pool_src_pool_user {
    use soroban_fixed_point_math::SorobanFixedPoint;

    use soroban_sdk::{contracttype, panic_with_error, Address, Env, Map};

    use crate::{
        constants::SCALAR_12, emissions, storage, validator::require_nonnegative, PoolError,
    };

    use crate::pool::{Pool, Reserve};

    pub(crate) use crate::pool::user::*;

    mod tests {
        use super::*;
        use crate::{
            constants::SCALAR_7, storage, testutils, ReserveEmissionData, UserEmissionData,
        };
        use soroban_fixed_point_math::SorobanFixedPoint;
        use soroban_sdk::{
            map,
            testutils::{Address as _, Ledger, LedgerInfo},
        };

        #[test]
        fn test_load_and_store() {
            let e = Env::default();
            e.mock_all_auths();
            let samwise = Address::generate(&e);
            let pool = testutils::create_pool(&e);

            let user = User {
                address: samwise.clone(),
                positions: Positions {
                    collateral: map![&e, (0, 10000)],
                    liabilities: map![&e],
                    supply: map![&e],
                },
            };
            e.as_contract(&pool, || {
                user.store(&e);
                let loaded_user = User::load(&e, &samwise);
                assert_eq!(loaded_user.address, samwise);
                assert_eq!(loaded_user.positions.collateral.len(), 1);
                assert_eq!(loaded_user.positions.collateral.get_unchecked(0), 10000);
                assert_eq!(loaded_user.positions.liabilities.len(), 0);
                assert_eq!(loaded_user.positions.supply.len(), 0);
            });
        }

        #[test]
        fn test_liabilities() {
            let e = Env::default();
            e.mock_all_auths();
            let samwise = Address::generate(&e);
            let pool = testutils::create_pool(&e);

            let mut reserve_0 = testutils::default_reserve(&e);
            let starting_d_supply_0 = reserve_0.data.d_supply;

            let mut reserve_1 = testutils::default_reserve(&e);
            reserve_1.config.index = 1;
            let starting_d_supply_1 = reserve_1.data.d_supply;

            let mut user = User {
                address: samwise.clone(),
                positions: Positions::env_default(&e),
            };
            e.as_contract(&pool, || {
                assert_eq!(user.get_liabilities(0), 0);
                assert_eq!(user.has_liabilities(), false);

                user.add_liabilities(&e, &mut reserve_0, 123);
                assert_eq!(user.get_liabilities(0), 123);
                assert_eq!(reserve_0.data.d_supply, starting_d_supply_0 + 123);
                assert_eq!(user.has_liabilities(), true);

                user.add_liabilities(&e, &mut reserve_1, 456);
                assert_eq!(user.get_liabilities(0), 123);
                assert_eq!(user.get_liabilities(1), 456);
                assert_eq!(reserve_1.data.d_supply, starting_d_supply_1 + 456);

                user.remove_liabilities(&e, &mut reserve_1, 100);
                assert_eq!(user.get_liabilities(1), 356);
                assert_eq!(reserve_1.data.d_supply, starting_d_supply_1 + 356);

                user.remove_liabilities(&e, &mut reserve_1, 356);
                assert_eq!(user.get_liabilities(1), 0);
                assert_eq!(user.positions.liabilities.len(), 1);
                assert_eq!(reserve_1.data.d_supply, starting_d_supply_1);

                user.remove_liabilities(&e, &mut reserve_0, 123);
                assert_eq!(user.get_liabilities(0), 0);
                assert_eq!(user.positions.liabilities.len(), 0);
                assert_eq!(reserve_0.data.d_supply, starting_d_supply_0);
                assert_eq!(user.has_liabilities(), false);
            });
        }

        #[test]
        #[should_panic(expected = "Error(Contract, #1218)")]
        fn test_add_liabilities_zero_mint() {
            let e = Env::default();
            e.mock_all_auths();
            let samwise = Address::generate(&e);
            let pool = testutils::create_pool(&e);

            let mut reserve_0 = testutils::default_reserve(&e);

            let mut user = User {
                address: samwise.clone(),
                positions: Positions::env_default(&e),
            };
            e.as_contract(&pool, || {
                assert_eq!(user.get_liabilities(0), 0);

                user.add_liabilities(&e, &mut reserve_0, 0);
            });
        }

        #[test]
        fn test_add_liabilities_accrues_emissions() {
            let e = Env::default();
            e.mock_all_auths();
            let samwise = Address::generate(&e);
            let pool = testutils::create_pool(&e);

            e.ledger().set(LedgerInfo {
                protocol_version: 22,
                sequence_number: 1,
                timestamp: 10001000,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 3110400,
            });

            let mut reserve_0 = testutils::default_reserve(&e);
            let starting_d_supply_0 = reserve_0.data.d_supply;

            let emis_res_data = ReserveEmissionData {
                expiration: 20000000,
                eps: 0_10000000000000,
                index: 10000000000,
                last_time: 10000000, // 1000s elapsed
            };
            let emis_user_data = UserEmissionData {
                index: 9000000000,
                accrued: 0,
            };

            let mut user = User {
                address: samwise.clone(),
                positions: Positions {
                    liabilities: map![&e, (reserve_0.config.index, 1000)],
                    collateral: map![&e],
                    supply: map![&e],
                },
            };

            e.as_contract(&pool, || {
                let res_0_d_token_index = reserve_0.config.index * 2 + 0;
                storage::set_res_emis_data(&e, &res_0_d_token_index, &emis_res_data);
                storage::set_user_emissions(&e, &samwise, &res_0_d_token_index, &emis_user_data);

                user.add_liabilities(&e, &mut reserve_0, 123);
                assert_eq!(user.get_liabilities(0), 1123);
                assert_eq!(reserve_0.data.d_supply, starting_d_supply_0 + 123);

                let new_emis_res_data =
                    storage::get_res_emis_data(&e, &res_0_d_token_index).unwrap();
                let new_index = 10000000000
                    + (1000i128 * 0_10000000000000).fixed_div_floor(
                        &e,
                        &starting_d_supply_0,
                        &SCALAR_7,
                    );
                assert_eq!(new_emis_res_data.last_time, 10001000);
                assert_eq!(new_emis_res_data.index, new_index);
                let user_emis_data =
                    storage::get_user_emissions(&e, &samwise, &res_0_d_token_index).unwrap();
                let new_accrual = 0
                    + (new_index - emis_user_data.index).fixed_mul_floor(
                        &e,
                        &1000,
                        &(SCALAR_7 * SCALAR_7),
                    );
                assert_eq!(user_emis_data.accrued, new_accrual);
            });
        }

        #[test]
        #[should_panic(expected = "Error(Contract, #1219)")]
        fn test_remove_liabilities_zero_burn() {
            let e = Env::default();
            e.mock_all_auths();
            let samwise = Address::generate(&e);
            let pool = testutils::create_pool(&e);

            let mut reserve_0 = testutils::default_reserve(&e);

            let mut user = User {
                address: samwise.clone(),
                positions: Positions::env_default(&e),
            };
            e.as_contract(&pool, || {
                assert_eq!(user.get_liabilities(0), 0);

                user.add_liabilities(&e, &mut reserve_0, 123);
                assert_eq!(user.get_liabilities(0), 123);

                user.remove_liabilities(&e, &mut reserve_0, 0);
            });
        }

        #[test]
        fn test_remove_liabilities_accrues_emissions() {
            let e = Env::default();
            e.mock_all_auths();
            let samwise = Address::generate(&e);
            let pool = testutils::create_pool(&e);

            e.ledger().set(LedgerInfo {
                protocol_version: 22,
                sequence_number: 1,
                timestamp: 10001000,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 3110400,
            });

            let mut reserve_0 = testutils::default_reserve(&e);
            let starting_d_supply_0 = reserve_0.data.d_supply;

            let emis_res_data = ReserveEmissionData {
                expiration: 20000000,
                eps: 0_10000000000000,
                index: 10000000000,
                last_time: 10000000, // 1000s elapsed
            };
            let emis_user_data = UserEmissionData {
                index: 9000000000,
                accrued: 0,
            };
            let mut user = User {
                address: samwise.clone(),
                positions: Positions {
                    liabilities: map![&e, (reserve_0.config.index, 1000)],
                    collateral: map![&e],
                    supply: map![&e],
                },
            };
            e.as_contract(&pool, || {
                let res_0_d_token_index = reserve_0.config.index * 2 + 0;
                storage::set_res_emis_data(&e, &res_0_d_token_index, &emis_res_data);
                storage::set_user_emissions(&e, &samwise, &res_0_d_token_index, &emis_user_data);

                user.remove_liabilities(&e, &mut reserve_0, 123);
                assert_eq!(user.get_liabilities(0), 877);
                assert_eq!(reserve_0.data.d_supply, starting_d_supply_0 - 123);

                let new_emis_res_data =
                    storage::get_res_emis_data(&e, &res_0_d_token_index).unwrap();
                let new_index = 10000000000
                    + (1000i128 * 0_1000000).fixed_div_floor(
                        &e,
                        &starting_d_supply_0,
                        &(SCALAR_7 * SCALAR_7),
                    );
                assert_eq!(new_emis_res_data.last_time, 10001000);
                assert_eq!(new_emis_res_data.index, new_index);
                let user_emis_data =
                    storage::get_user_emissions(&e, &samwise, &res_0_d_token_index).unwrap();
                let new_accrual = 0
                    + (new_index - emis_user_data.index).fixed_mul_floor(
                        &e,
                        &1000,
                        &(SCALAR_7 * SCALAR_7),
                    );
                assert_eq!(user_emis_data.accrued, new_accrual);
            });
        }

        #[test]
        #[should_panic(expected = "Error(Contract, #8)")]
        fn test_remove_liabilities_over_balance_panics() {
            let e = Env::default();
            e.mock_all_auths();
            let samwise = Address::generate(&e);
            let pool = testutils::create_pool(&e);

            let mut reserve_0 = testutils::default_reserve(&e);
            let mut user = User {
                address: samwise.clone(),
                positions: Positions::env_default(&e),
            };
            e.as_contract(&pool, || {
                user.add_liabilities(&e, &mut reserve_0, 123);
                assert_eq!(user.get_liabilities(0), 123);

                user.remove_liabilities(&e, &mut reserve_0, 124);
            });
        }

        #[test]
        fn test_default_liabilities_reduces_b_rate() {
            let e = Env::default();
            e.mock_all_auths();
            let samwise = Address::generate(&e);
            let pool = testutils::create_pool(&e);

            let mut reserve_0 = testutils::default_reserve(&e);
            reserve_0.data.d_rate = 1_500_000_000_000;
            reserve_0.data.d_supply = 500_0000000;
            reserve_0.data.b_rate = 1_250_000_000_000;
            reserve_0.data.b_supply = 750_0000000;

            let mut user = User {
                address: samwise.clone(),
                positions: Positions::env_default(&e),
            };
            e.as_contract(&pool, || {
                assert_eq!(user.get_liabilities(0), 0);

                user.add_liabilities(&e, &mut reserve_0, 20_0000000);
                assert_eq!(user.get_liabilities(0), 20_0000000);

                let d_supply = reserve_0.data.d_supply;
                let total_supply = reserve_0.total_supply(&e);
                let underlying_default_amount = reserve_0.to_asset_from_d_token(&e, 20_0000000);
                user.default_liabilities(&e, &mut reserve_0, 20_0000000);

                assert_eq!(user.get_liabilities(0), 0);
                assert_eq!(reserve_0.data.d_supply, d_supply - 20_0000000);
                assert_eq!(
                    reserve_0.total_supply(&e),
                    total_supply - underlying_default_amount
                );
                assert_eq!(reserve_0.data.b_rate, 1_210_000_000_000);
                assert_eq!(reserve_0.data.b_supply, 750_0000000);
            });
        }

        #[test]
        fn test_default_liabilities_reduces_b_rate_to_zero() {
            let e = Env::default();
            e.mock_all_auths();
            let samwise = Address::generate(&e);
            let pool = testutils::create_pool(&e);

            let mut reserve_0 = testutils::default_reserve(&e);
            reserve_0.data.d_rate = 1_500_000_000_000;
            reserve_0.data.d_supply = 500_0000000;
            reserve_0.data.b_rate = 0_100_000_000_000;
            reserve_0.data.b_supply = 750_0000000;

            let mut user = User {
                address: samwise.clone(),
                positions: Positions::env_default(&e),
            };
            e.as_contract(&pool, || {
                assert_eq!(user.get_liabilities(0), 0);

                user.add_liabilities(&e, &mut reserve_0, 100_0000000);
                assert_eq!(user.get_liabilities(0), 100_0000000);

                let d_supply = reserve_0.data.d_supply;
                user.default_liabilities(&e, &mut reserve_0, 100_0000000);

                assert_eq!(user.get_liabilities(0), 0);
                assert_eq!(reserve_0.data.d_supply, d_supply - 100_0000000);
                assert_eq!(reserve_0.total_supply(&e), 0);
                assert_eq!(reserve_0.data.b_rate, 0);
                assert_eq!(reserve_0.data.b_supply, 750_0000000);
            });
        }

        #[test]
        fn test_default_liabilities_reduces_b_rate_rounds_ceil() {
            let e = Env::default();
            e.mock_all_auths();
            let samwise = Address::generate(&e);
            let pool = testutils::create_pool(&e);

            let mut reserve_0 = testutils::default_reserve(&e);
            reserve_0.data.d_rate = 1_500_000_000_000;
            reserve_0.data.d_supply = 500_0000000;
            reserve_0.data.b_rate = 1_250_000_000_000;
            reserve_0.data.b_supply = 750_0000000;

            let mut user = User {
                address: samwise.clone(),
                positions: Positions::env_default(&e),
            };
            e.as_contract(&pool, || {
                assert_eq!(user.get_liabilities(0), 0);

                user.add_liabilities(&e, &mut reserve_0, 20_0000001);
                assert_eq!(user.get_liabilities(0), 20_0000001);

                let d_supply = reserve_0.data.d_supply;
                let total_supply = reserve_0.total_supply(&e);
                let underlying_default_amount = reserve_0.to_asset_from_d_token(&e, 20_0000001);
                user.default_liabilities(&e, &mut reserve_0, 20_0000001);

                // rounding loss of 1 stroop for resulting total supply
                assert_eq!(user.get_liabilities(0), 0);
                assert_eq!(reserve_0.data.d_supply, d_supply - 20_0000001);
                assert_eq!(
                    reserve_0.total_supply(&e),
                    total_supply - underlying_default_amount - 1
                );
                assert_eq!(reserve_0.data.b_rate, 1_209_999_999_733);
                assert_eq!(reserve_0.data.b_supply, 750_0000000);
            });
        }

        #[test]
        fn test_collateral() {
            let e = Env::default();
            e.mock_all_auths();
            let samwise = Address::generate(&e);
            let pool = testutils::create_pool(&e);

            let mut reserve_0 = testutils::default_reserve(&e);
            let starting_b_supply_0 = reserve_0.data.b_supply;

            let mut reserve_1 = testutils::default_reserve(&e);
            reserve_1.config.index = 1;
            let starting_b_supply_1 = reserve_1.data.b_supply;

            let mut user = User {
                address: samwise.clone(),
                positions: Positions::env_default(&e),
            };
            e.as_contract(&pool, || {
                assert_eq!(user.get_collateral(0), 0);
                assert_eq!(user.has_collateral(), false);

                user.add_collateral(&e, &mut reserve_0, 123);
                assert_eq!(user.get_collateral(0), 123);
                assert_eq!(reserve_0.data.b_supply, starting_b_supply_0 + 123);
                assert_eq!(user.has_collateral(), true);

                user.add_collateral(&e, &mut reserve_1, 456);
                assert_eq!(user.get_collateral(0), 123);
                assert_eq!(user.get_collateral(1), 456);
                assert_eq!(reserve_1.data.b_supply, starting_b_supply_1 + 456);
                assert_eq!(user.has_collateral(), true);

                user.remove_collateral(&e, &mut reserve_1, 100);
                assert_eq!(user.get_collateral(1), 356);
                assert_eq!(reserve_1.data.b_supply, starting_b_supply_1 + 356);
                assert_eq!(user.has_collateral(), true);

                user.remove_collateral(&e, &mut reserve_1, 356);
                assert_eq!(user.get_collateral(1), 0);
                assert_eq!(user.positions.collateral.len(), 1);
                assert_eq!(reserve_1.data.b_supply, starting_b_supply_1);
                assert_eq!(user.has_collateral(), true);

                user.remove_collateral(&e, &mut reserve_0, 123);
                assert_eq!(user.get_collateral(0), 0);
                assert_eq!(user.positions.collateral.len(), 0);
                assert_eq!(reserve_0.data.b_supply, starting_b_supply_0);
                assert_eq!(user.has_collateral(), false);
            });
        }

        #[test]
        #[should_panic(expected = "Error(Contract, #1216)")]
        fn test_add_collateral_zero_mint() {
            let e = Env::default();
            e.mock_all_auths();
            let samwise = Address::generate(&e);
            let pool = testutils::create_pool(&e);

            let mut reserve_0 = testutils::default_reserve(&e);

            let mut user = User {
                address: samwise.clone(),
                positions: Positions::env_default(&e),
            };
            e.as_contract(&pool, || {
                assert_eq!(user.get_collateral(0), 0);

                user.add_collateral(&e, &mut reserve_0, 0);
            });
        }

        #[test]
        fn test_add_collateral_accrues_emissions() {
            let e = Env::default();
            e.mock_all_auths();
            let samwise = Address::generate(&e);
            let pool = testutils::create_pool(&e);

            e.ledger().set(LedgerInfo {
                protocol_version: 22,
                sequence_number: 1,
                timestamp: 10001000,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 3110400,
            });

            let mut reserve_0 = testutils::default_reserve(&e);
            let starting_b_token_supply = reserve_0.data.b_supply;

            let emis_res_data = ReserveEmissionData {
                expiration: 20000000,
                eps: 0_10000000000000,
                index: 10000000000,
                last_time: 10000000, // 1000s elapsed
            };
            let emis_user_data = UserEmissionData {
                index: 9000000000,
                accrued: 0,
            };

            let mut user = User {
                address: samwise.clone(),
                positions: Positions {
                    liabilities: map![&e],
                    collateral: map![&e, (reserve_0.config.index, 700)],
                    supply: map![&e, (reserve_0.config.index, 300)],
                },
            };
            e.as_contract(&pool, || {
                let res_0_d_token_index = reserve_0.config.index * 2 + 1;
                storage::set_res_emis_data(&e, &res_0_d_token_index, &emis_res_data);
                storage::set_user_emissions(&e, &samwise, &res_0_d_token_index, &emis_user_data);

                user.add_collateral(&e, &mut reserve_0, 123);
                assert_eq!(user.get_collateral(0), 823);
                assert_eq!(reserve_0.data.b_supply, starting_b_token_supply + 123);

                let new_emis_res_data =
                    storage::get_res_emis_data(&e, &res_0_d_token_index).unwrap();
                let new_index = 10000000000
                    + (1000i128 * 0_1000000).fixed_div_floor(
                        &e,
                        &starting_b_token_supply,
                        &(SCALAR_7 * SCALAR_7),
                    );
                assert_eq!(new_emis_res_data.last_time, 10001000);
                assert_eq!(new_emis_res_data.index, new_index);
                let user_emis_data =
                    storage::get_user_emissions(&e, &samwise, &res_0_d_token_index).unwrap();
                let new_accrual = 0
                    + (new_index - emis_user_data.index).fixed_mul_floor(
                        &e,
                        &1000,
                        &(SCALAR_7 * SCALAR_7),
                    );
                assert_eq!(user_emis_data.accrued, new_accrual);
            });
        }

        #[test]
        #[should_panic(expected = "Error(Contract, #1217)")]
        fn test_remove_collateral_zero_burn() {
            let e = Env::default();
            e.mock_all_auths();
            let samwise = Address::generate(&e);
            let pool = testutils::create_pool(&e);

            let mut reserve_0 = testutils::default_reserve(&e);

            let mut user = User {
                address: samwise.clone(),
                positions: Positions::env_default(&e),
            };
            e.as_contract(&pool, || {
                assert_eq!(user.get_collateral(0), 0);

                user.add_collateral(&e, &mut reserve_0, 123);
                assert_eq!(user.get_collateral(0), 123);

                user.remove_collateral(&e, &mut reserve_0, 0);
            });
        }

        #[test]
        fn test_remove_collateral_accrues_emissions() {
            let e = Env::default();
            e.mock_all_auths();
            let samwise = Address::generate(&e);
            let pool = testutils::create_pool(&e);

            e.ledger().set(LedgerInfo {
                protocol_version: 22,
                sequence_number: 1,
                timestamp: 10001000,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 3110400,
            });

            let mut reserve_0 = testutils::default_reserve(&e);
            let starting_b_token_supply = reserve_0.data.b_supply;

            let emis_res_data = ReserveEmissionData {
                expiration: 20000000,
                eps: 0_10000000000000,
                index: 10000000000,
                last_time: 10000000, // 1000s elapsed
            };
            let emis_user_data = UserEmissionData {
                index: 9000000000,
                accrued: 0,
            };

            let mut user = User {
                address: samwise.clone(),
                positions: Positions {
                    liabilities: map![&e],
                    collateral: map![&e, (reserve_0.config.index, 700)],
                    supply: map![&e, (reserve_0.config.index, 300)],
                },
            };
            e.as_contract(&pool, || {
                let res_0_d_token_index = reserve_0.config.index * 2 + 1;
                storage::set_res_emis_data(&e, &res_0_d_token_index, &emis_res_data);
                storage::set_user_emissions(&e, &samwise, &res_0_d_token_index, &emis_user_data);

                user.remove_collateral(&e, &mut reserve_0, 123);
                assert_eq!(user.get_collateral(0), 577);
                assert_eq!(reserve_0.data.b_supply, starting_b_token_supply - 123);

                let new_emis_res_data =
                    storage::get_res_emis_data(&e, &res_0_d_token_index).unwrap();
                let new_index = 10000000000
                    + (1000i128 * 0_1000000).fixed_div_floor(
                        &e,
                        &starting_b_token_supply,
                        &(SCALAR_7 * SCALAR_7),
                    );
                assert_eq!(new_emis_res_data.last_time, 10001000);
                assert_eq!(new_emis_res_data.index, new_index);
                let user_emis_data =
                    storage::get_user_emissions(&e, &samwise, &res_0_d_token_index).unwrap();
                let new_accrual = 0
                    + (new_index - emis_user_data.index).fixed_mul_floor(
                        &e,
                        &1000,
                        &(SCALAR_7 * SCALAR_7),
                    );
                assert_eq!(user_emis_data.accrued, new_accrual);
            });
        }

        #[test]
        #[should_panic(expected = "Error(Contract, #8)")]
        fn test_remove_collateral_over_balance_panics() {
            let e = Env::default();
            e.mock_all_auths();
            let samwise = Address::generate(&e);
            let pool = testutils::create_pool(&e);

            let mut reserve_0 = testutils::default_reserve(&e);

            let mut user = User {
                address: samwise.clone(),
                positions: Positions::env_default(&e),
            };
            e.as_contract(&pool, || {
                user.add_collateral(&e, &mut reserve_0, 123);
                assert_eq!(user.get_collateral(0), 123);

                user.remove_collateral(&e, &mut reserve_0, 124);
            });
        }

        #[test]
        fn test_supply() {
            let e = Env::default();
            e.mock_all_auths();
            let samwise = Address::generate(&e);
            let pool = testutils::create_pool(&e);

            let mut reserve_0 = testutils::default_reserve(&e);
            let starting_b_supply_0 = reserve_0.data.b_supply;

            let mut reserve_1 = testutils::default_reserve(&e);
            reserve_1.config.index = 1;
            let starting_b_supply_1 = reserve_1.data.b_supply;

            let mut user = User {
                address: samwise.clone(),
                positions: Positions::env_default(&e),
            };
            e.as_contract(&pool, || {
                assert_eq!(user.get_supply(0), 0);

                user.add_supply(&e, &mut reserve_0, 123);
                assert_eq!(user.get_supply(0), 123);
                assert_eq!(reserve_0.data.b_supply, starting_b_supply_0 + 123);

                user.add_supply(&e, &mut reserve_1, 456);
                assert_eq!(user.get_supply(0), 123);
                assert_eq!(user.get_supply(1), 456);
                assert_eq!(reserve_1.data.b_supply, starting_b_supply_1 + 456);

                user.remove_supply(&e, &mut reserve_1, 100);
                assert_eq!(user.get_supply(1), 356);
                assert_eq!(reserve_1.data.b_supply, starting_b_supply_1 + 356);

                user.remove_supply(&e, &mut reserve_1, 356);
                assert_eq!(user.get_supply(2), 0);
                assert_eq!(user.positions.supply.len(), 1);
                assert_eq!(reserve_1.data.b_supply, starting_b_supply_1);
            });
        }

        #[test]
        #[should_panic(expected = "Error(Contract, #1216)")]
        fn test_add_supply_zero_mint() {
            let e = Env::default();
            e.mock_all_auths();
            let samwise = Address::generate(&e);
            let pool = testutils::create_pool(&e);

            let mut reserve_0 = testutils::default_reserve(&e);

            let mut user = User {
                address: samwise.clone(),
                positions: Positions::env_default(&e),
            };
            e.as_contract(&pool, || {
                assert_eq!(user.get_supply(0), 0);

                user.add_supply(&e, &mut reserve_0, 0);
            });
        }

        #[test]
        fn test_add_supply_accrues_emissions() {
            let e = Env::default();
            e.mock_all_auths();
            let samwise = Address::generate(&e);
            let pool = testutils::create_pool(&e);

            e.ledger().set(LedgerInfo {
                protocol_version: 22,
                sequence_number: 1,
                timestamp: 10001000,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 3110400,
            });

            let mut reserve_0 = testutils::default_reserve(&e);
            let starting_b_token_supply = reserve_0.data.b_supply;

            let emis_res_data = ReserveEmissionData {
                expiration: 20000000,
                eps: 0_10000000000000,
                index: 10000000000,
                last_time: 10000000, // 1000s elapsed
            };
            let emis_user_data = UserEmissionData {
                index: 9000000000,
                accrued: 0,
            };

            let mut user = User {
                address: samwise.clone(),
                positions: Positions {
                    liabilities: map![&e],
                    collateral: map![&e, (reserve_0.config.index, 700)],
                    supply: map![&e, (reserve_0.config.index, 300)],
                },
            };
            e.as_contract(&pool, || {
                let res_0_d_token_index = reserve_0.config.index * 2 + 1;
                storage::set_res_emis_data(&e, &res_0_d_token_index, &emis_res_data);
                storage::set_user_emissions(&e, &samwise, &res_0_d_token_index, &emis_user_data);

                user.add_supply(&e, &mut reserve_0, 123);
                assert_eq!(user.get_supply(0), 423);
                assert_eq!(reserve_0.data.b_supply, starting_b_token_supply + 123);

                let new_emis_res_data =
                    storage::get_res_emis_data(&e, &res_0_d_token_index).unwrap();
                let new_index = 10000000000
                    + (1000i128 * 0_1000000).fixed_div_floor(
                        &e,
                        &starting_b_token_supply,
                        &(SCALAR_7 * SCALAR_7),
                    );
                assert_eq!(new_emis_res_data.last_time, 10001000);
                assert_eq!(new_emis_res_data.index, new_index);
                let user_emis_data =
                    storage::get_user_emissions(&e, &samwise, &res_0_d_token_index).unwrap();
                let new_accrual = 0
                    + (new_index - emis_user_data.index).fixed_mul_floor(
                        &e,
                        &1000,
                        &(SCALAR_7 * SCALAR_7),
                    );
                assert_eq!(user_emis_data.accrued, new_accrual);
            });
        }

        #[test]
        #[should_panic(expected = "Error(Contract, #1217)")]
        fn test_remove_supply_zero_burn() {
            let e = Env::default();
            e.mock_all_auths();
            let samwise = Address::generate(&e);
            let pool = testutils::create_pool(&e);

            let mut reserve_0 = testutils::default_reserve(&e);

            let mut user = User {
                address: samwise.clone(),
                positions: Positions::env_default(&e),
            };
            e.as_contract(&pool, || {
                assert_eq!(user.get_supply(0), 0);

                user.add_supply(&e, &mut reserve_0, 123);
                assert_eq!(user.get_supply(0), 123);

                user.remove_supply(&e, &mut reserve_0, 0);
            });
        }

        #[test]
        fn test_remove_supply_accrues_emissions() {
            let e = Env::default();
            e.mock_all_auths();
            let samwise = Address::generate(&e);
            let pool = testutils::create_pool(&e);

            e.ledger().set(LedgerInfo {
                protocol_version: 22,
                sequence_number: 1,
                timestamp: 10001000,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 3110400,
            });

            let mut reserve_0 = testutils::default_reserve(&e);
            let starting_b_token_supply = reserve_0.data.b_supply;

            let emis_res_data = ReserveEmissionData {
                expiration: 20000000,
                eps: 0_10000000000000,
                index: 10000000000,
                last_time: 10000000, // 1000s elapsed
            };
            let emis_user_data = UserEmissionData {
                index: 9000000000,
                accrued: 0,
            };

            let mut user = User {
                address: samwise.clone(),
                positions: Positions {
                    liabilities: map![&e],
                    collateral: map![&e, (reserve_0.config.index, 700)],
                    supply: map![&e, (reserve_0.config.index, 300)],
                },
            };
            e.as_contract(&pool, || {
                let res_0_d_token_index = reserve_0.config.index * 2 + 1;
                storage::set_res_emis_data(&e, &res_0_d_token_index, &emis_res_data);
                storage::set_user_emissions(&e, &samwise, &res_0_d_token_index, &emis_user_data);

                user.remove_supply(&e, &mut reserve_0, 123);
                assert_eq!(user.get_supply(0), 177);
                assert_eq!(reserve_0.data.b_supply, starting_b_token_supply - 123);

                let new_emis_res_data =
                    storage::get_res_emis_data(&e, &res_0_d_token_index).unwrap();
                let new_index = 10000000000
                    + (1000i128 * 0_1000000).fixed_div_floor(
                        &e,
                        &starting_b_token_supply,
                        &(SCALAR_7 * SCALAR_7),
                    );
                assert_eq!(new_emis_res_data.last_time, 10001000);
                assert_eq!(new_emis_res_data.index, new_index);
                let user_emis_data =
                    storage::get_user_emissions(&e, &samwise, &res_0_d_token_index).unwrap();
                let new_accrual = 0
                    + (new_index - emis_user_data.index).fixed_mul_floor(
                        &e,
                        &1000,
                        &(SCALAR_7 * SCALAR_7),
                    );
                assert_eq!(user_emis_data.accrued, new_accrual);
            });
        }

        #[test]
        #[should_panic(expected = "Error(Contract, #8)")]
        fn test_remove_supply_over_balance_panics() {
            let e = Env::default();
            e.mock_all_auths();
            let samwise = Address::generate(&e);
            let pool = testutils::create_pool(&e);

            let mut reserve_0 = testutils::default_reserve(&e);

            let mut user = User {
                address: samwise.clone(),
                positions: Positions::env_default(&e),
            };
            e.as_contract(&pool, || {
                user.add_supply(&e, &mut reserve_0, 123);
                assert_eq!(user.get_supply(0), 123);

                user.remove_supply(&e, &mut reserve_0, 124);
            });
        }

        #[test]
        fn test_total_supply() {
            let e = Env::default();
            e.mock_all_auths();
            let samwise = Address::generate(&e);
            let pool = testutils::create_pool(&e);

            let mut reserve_0 = testutils::default_reserve(&e);

            let mut reserve_1 = testutils::default_reserve(&e);
            reserve_1.config.index = 1;

            let mut user = User {
                address: samwise.clone(),
                positions: Positions::env_default(&e),
            };
            e.as_contract(&pool, || {
                user.add_supply(&e, &mut reserve_0, 123);
                user.add_supply(&e, &mut reserve_1, 456);
                user.add_collateral(&e, &mut reserve_1, 789);
                assert_eq!(user.get_total_supply(0), 123);
                assert_eq!(user.get_total_supply(1), 456 + 789);
            });
        }
    }
}

mod pool_src_pool_submit {
    use moderc3156::FlashLoanClient;

    use sep_41_token::TokenClient;

    use soroban_sdk::{panic_with_error, Address, Env, Map, Vec};

    use crate::{events::PoolEvents, storage, AuctionType, PoolError};

    use crate::pool::{
        actions::{build_actions_from_request, Actions, Request},
        health_factor::PositionData,
        pool::Pool,
        FlashLoan, Positions, RequestType, User,
    };

    pub(crate) use crate::pool::submit::*;

    mod tests {
        use crate::{
            storage::{self, PoolConfig},
            testutils, AuctionData, RequestType,
        };

        use super::*;
        use sep_40_oracle::testutils::Asset;
        use soroban_sdk::{
            map,
            testutils::{Address as _, Ledger, LedgerInfo},
            vec, Symbol,
        };

        #[test]
        fn test_submit() {
            let e = Env::default();
            e.cost_estimate().budget().reset_unlimited();
            e.mock_all_auths_allowing_non_root_auth();

            e.ledger().set(LedgerInfo {
                timestamp: 600,
                protocol_version: 22,
                sequence_number: 1234,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 3110400,
            });

            let bombadil = Address::generate(&e);
            let samwise = Address::generate(&e);
            let frodo = Address::generate(&e);
            let merry = Address::generate(&e);
            let pool = testutils::create_pool(&e);
            let (oracle, oracle_client) = testutils::create_mock_oracle(&e);

            let (underlying_0, underlying_0_client) =
                testutils::create_token_contract(&e, &bombadil);
            let (reserve_config, reserve_data) = testutils::default_reserve_meta();
            testutils::create_reserve(&e, &pool, &underlying_0, &reserve_config, &reserve_data);

            let (underlying_1, underlying_1_client) =
                testutils::create_token_contract(&e, &bombadil);
            let (reserve_config, reserve_data) = testutils::default_reserve_meta();
            testutils::create_reserve(&e, &pool, &underlying_1, &reserve_config, &reserve_data);

            underlying_0_client.mint(&frodo, &16_0000000);

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
            oracle_client.set_price_stable(&vec![&e, 1_0000000, 5_0000000]);

            let pool_config = PoolConfig {
                oracle,
                min_collateral: 1_0000000,
                bstop_rate: 0_1000000,
                status: 0,
                max_positions: 2,
            };
            e.as_contract(&pool, || {
                e.mock_all_auths_allowing_non_root_auth();
                storage::set_pool_config(&e, &pool_config);

                let pre_pool_balance_0 = underlying_0_client.balance(&pool);
                let pre_pool_balance_1 = underlying_1_client.balance(&pool);

                let pre_res_0_data = storage::get_res_data(&e, &underlying_0);
                let pre_res_1_data = storage::get_res_data(&e, &underlying_1);

                let requests = vec![
                    &e,
                    Request {
                        request_type: RequestType::SupplyCollateral as u32,
                        address: underlying_0.clone(),
                        amount: 15_0000000,
                    },
                    Request {
                        request_type: RequestType::Borrow as u32,
                        address: underlying_1.clone(),
                        amount: 1_5000000,
                    },
                ];
                let positions = execute_submit(&e, &samwise, &frodo, &merry, requests, false);

                assert_eq!(positions.liabilities.len(), 1);
                assert_eq!(positions.collateral.len(), 1);
                assert_eq!(positions.supply.len(), 0);
                let b_tokens_minted = positions.collateral.get_unchecked(0);
                assert_eq!(b_tokens_minted, 14_9999884);
                let d_tokens_minted = positions.liabilities.get_unchecked(1);
                assert_eq!(d_tokens_minted, 1_4999983);

                let reserve_0 = storage::get_res_data(&e, &underlying_0);
                assert_eq!(
                    reserve_0.b_supply,
                    pre_res_0_data.b_supply + b_tokens_minted
                );

                let reserve_1 = storage::get_res_data(&e, &underlying_1);
                assert_eq!(
                    reserve_1.d_supply,
                    pre_res_1_data.d_supply + d_tokens_minted
                );

                assert_eq!(
                    underlying_0_client.balance(&pool),
                    pre_pool_balance_0 + 15_0000000
                );
                assert_eq!(
                    underlying_1_client.balance(&pool),
                    pre_pool_balance_1 - 1_5000000
                );

                assert_eq!(underlying_0_client.balance(&frodo), 1_0000000);
                assert_eq!(underlying_1_client.balance(&merry), 1_5000000);
            });
        }

        #[test]
        fn test_submit_use_allowance() {
            let e = Env::default();
            e.cost_estimate().budget().reset_unlimited();
            e.mock_all_auths_allowing_non_root_auth();

            e.ledger().set(LedgerInfo {
                timestamp: 600,
                protocol_version: 22,
                sequence_number: 1234,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 3110400,
            });

            let bombadil = Address::generate(&e);
            let samwise = Address::generate(&e);
            let frodo = Address::generate(&e);
            let merry = Address::generate(&e);
            let pool = testutils::create_pool(&e);
            let (oracle, oracle_client) = testutils::create_mock_oracle(&e);

            let (underlying_0, underlying_0_client) =
                testutils::create_token_contract(&e, &bombadil);
            let (reserve_config, reserve_data) = testutils::default_reserve_meta();
            testutils::create_reserve(&e, &pool, &underlying_0, &reserve_config, &reserve_data);

            let (underlying_1, underlying_1_client) =
                testutils::create_token_contract(&e, &bombadil);
            let (reserve_config, reserve_data) = testutils::default_reserve_meta();
            testutils::create_reserve(&e, &pool, &underlying_1, &reserve_config, &reserve_data);

            underlying_0_client.mint(&frodo, &15_0000000);

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
            oracle_client.set_price_stable(&vec![&e, 1_0000000, 5_0000000]);

            let pool_config = PoolConfig {
                oracle,
                min_collateral: 1_0000000,
                bstop_rate: 0_1000000,
                status: 0,
                max_positions: 4,
            };
            e.as_contract(&pool, || {
                e.mock_all_auths_allowing_non_root_auth();
                storage::set_pool_config(&e, &pool_config);

                let pre_pool_balance_0 = underlying_0_client.balance(&pool);
                let pre_pool_balance_1 = underlying_1_client.balance(&pool);

                let pre_res_0_data = storage::get_res_data(&e, &underlying_0);
                let pre_res_1_data = storage::get_res_data(&e, &underlying_1);

                let requests = vec![
                    &e,
                    Request {
                        request_type: RequestType::SupplyCollateral as u32,
                        address: underlying_0.clone(),
                        amount: 15_0000000,
                    },
                    Request {
                        request_type: RequestType::Borrow as u32,
                        address: underlying_1.clone(),
                        amount: 1_5000000,
                    },
                ];
                underlying_0_client.approve(&frodo, &pool, &15_0000000, &e.ledger().sequence());
                assert_eq!(underlying_0_client.allowance(&frodo, &pool), 15_0000000);

                let positions = execute_submit(&e, &samwise, &frodo, &merry, requests, true);

                assert_eq!(positions.liabilities.len(), 1);
                assert_eq!(positions.collateral.len(), 1);
                assert_eq!(positions.supply.len(), 0);
                let b_tokens_minted = positions.collateral.get_unchecked(0);
                assert_eq!(b_tokens_minted, 14_9999884);
                let d_tokens_minted = positions.liabilities.get_unchecked(1);
                assert_eq!(d_tokens_minted, 1_4999983);

                let reserve_0 = storage::get_res_data(&e, &underlying_0);
                assert_eq!(
                    reserve_0.b_supply,
                    pre_res_0_data.b_supply + b_tokens_minted
                );

                let reserve_1 = storage::get_res_data(&e, &underlying_1);
                assert_eq!(
                    reserve_1.d_supply,
                    pre_res_1_data.d_supply + d_tokens_minted
                );

                assert_eq!(
                    underlying_0_client.balance(&pool),
                    pre_pool_balance_0 + 15_0000000
                );
                assert_eq!(underlying_1_client.allowance(&frodo, &pool), 0);
                assert_eq!(
                    underlying_1_client.balance(&pool),
                    pre_pool_balance_1 - 1_5000000
                );

                assert_eq!(underlying_0_client.balance(&frodo), 0);
                assert_eq!(underlying_1_client.balance(&merry), 1_5000000);
            });

            underlying_0_client.mint(&frodo, &15_0000000);

            e.as_contract(&pool, || {
                e.mock_all_auths_allowing_non_root_auth();
                storage::set_pool_config(&e, &pool_config);

                let pre_pool_balance_0 = underlying_0_client.balance(&pool);

                let requests = vec![
                    &e,
                    Request {
                        request_type: RequestType::SupplyCollateral as u32,
                        address: underlying_0.clone(),
                        amount: 15_0000000,
                    },
                    Request {
                        request_type: RequestType::Borrow as u32,
                        address: underlying_0,
                        amount: 1_0000000,
                    },
                ];
                underlying_0_client.approve(&frodo, &pool, &14_0000000, &e.ledger().sequence());
                assert_eq!(underlying_0_client.allowance(&frodo, &pool), 14_0000000);
                let positions = execute_submit(&e, &samwise, &frodo, &merry, requests, true);

                // new_allowance = old_allowance - (deposit - borrow)
                assert_eq!(underlying_0_client.allowance(&frodo, &pool), 0);

                assert_eq!(positions.liabilities.len(), 2);
                assert_eq!(positions.collateral.len(), 1);
                assert_eq!(positions.supply.len(), 0);

                assert_eq!(positions.collateral.get_unchecked(0), 29_9999768);
                assert_eq!(positions.liabilities.get_unchecked(1), 1_4999983);

                assert_eq!(
                    underlying_0_client.balance(&pool),
                    pre_pool_balance_0 + 14_0000000
                );

                assert_eq!(underlying_0_client.balance(&frodo), 1_0000000);
            });
        }

        #[test]
        fn test_submit_use_allowance_over_repay() {
            let e = Env::default();
            e.cost_estimate().budget().reset_unlimited();
            e.mock_all_auths_allowing_non_root_auth();

            e.ledger().set(LedgerInfo {
                timestamp: 600,
                protocol_version: 22,
                sequence_number: 1234,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 3110400,
            });

            let bombadil = Address::generate(&e);
            let samwise = Address::generate(&e);
            let frodo = Address::generate(&e);
            let merry = Address::generate(&e);
            let pool = testutils::create_pool(&e);
            let (oracle, oracle_client) = testutils::create_mock_oracle(&e);

            let (underlying_0, underlying_0_client) =
                testutils::create_token_contract(&e, &bombadil);
            let (reserve_config, reserve_data) = testutils::default_reserve_meta();
            testutils::create_reserve(&e, &pool, &underlying_0, &reserve_config, &reserve_data);

            let (underlying_1, underlying_1_client) =
                testutils::create_token_contract(&e, &bombadil);
            let (reserve_config, reserve_data) = testutils::default_reserve_meta();
            testutils::create_reserve(&e, &pool, &underlying_1, &reserve_config, &reserve_data);

            underlying_0_client.mint(&frodo, &15_0000000);

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
            oracle_client.set_price_stable(&vec![&e, 1_0000000, 5_0000000]);

            let pool_config = PoolConfig {
                oracle,
                min_collateral: 1_0000000,
                bstop_rate: 0_1000000,
                status: 0,
                max_positions: 4,
            };
            e.as_contract(&pool, || {
                e.mock_all_auths_allowing_non_root_auth();
                storage::set_pool_config(&e, &pool_config);

                let requests = vec![
                    &e,
                    Request {
                        request_type: RequestType::SupplyCollateral as u32,
                        address: underlying_0,
                        amount: 15_0000000,
                    },
                    Request {
                        request_type: RequestType::Borrow as u32,
                        address: underlying_1.clone(),
                        amount: 1_5000000,
                    },
                ];
                underlying_0_client.approve(&frodo, &pool, &15_0000000, &e.ledger().sequence());
                assert_eq!(underlying_0_client.allowance(&frodo, &pool), 15_0000000);

                let positions = execute_submit(&e, &samwise, &frodo, &merry, requests, true);

                assert_eq!(positions.liabilities.len(), 1);
                assert_eq!(positions.collateral.len(), 1);
                assert_eq!(positions.supply.len(), 0);
                assert_eq!(positions.collateral.get_unchecked(0), 14_9999884);
                assert_eq!(positions.liabilities.get_unchecked(1), 1_4999983);

                underlying_1_client.mint(&frodo, &1_6000000);

                let pre_pool_balance_1 = underlying_1_client.balance(&pool);

                let requests = vec![
                    &e,
                    Request {
                        request_type: RequestType::Repay as u32,
                        address: underlying_1,
                        amount: 1_6000000,
                    },
                ];
                underlying_1_client.approve(&frodo, &pool, &1_5000001, &e.ledger().sequence());
                assert_eq!(underlying_1_client.allowance(&frodo, &pool), 1_5000001);
                let positions = execute_submit(&e, &samwise, &frodo, &merry, requests, true);

                // new_allowance = old_allowance - repay
                assert_eq!(underlying_1_client.allowance(&frodo, &pool), 0);

                assert_eq!(positions.liabilities.len(), 0);
                assert_eq!(positions.collateral.len(), 1);
                assert_eq!(positions.supply.len(), 0);

                assert_eq!(positions.collateral.get_unchecked(0), 14_9999884);

                assert_eq!(
                    underlying_1_client.balance(&pool),
                    pre_pool_balance_1 + 1_5000001
                );

                assert_eq!(underlying_1_client.balance(&frodo), 999999);
            });
        }

        #[test]
        #[should_panic(expected = "Error(Contract, #9)")]
        fn test_submit_use_allowance_no_allowance() {
            let e = Env::default();
            e.cost_estimate().budget().reset_unlimited();
            e.mock_all_auths_allowing_non_root_auth();

            e.ledger().set(LedgerInfo {
                timestamp: 600,
                protocol_version: 22,
                sequence_number: 1234,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 3110400,
            });

            let bombadil = Address::generate(&e);
            let samwise = Address::generate(&e);
            let frodo = Address::generate(&e);
            let merry = Address::generate(&e);
            let pool = testutils::create_pool(&e);
            let (oracle, oracle_client) = testutils::create_mock_oracle(&e);

            let (underlying_0, underlying_0_client) =
                testutils::create_token_contract(&e, &bombadil);
            let (reserve_config, reserve_data) = testutils::default_reserve_meta();
            testutils::create_reserve(&e, &pool, &underlying_0, &reserve_config, &reserve_data);

            let (underlying_1, _) = testutils::create_token_contract(&e, &bombadil);
            let (reserve_config, reserve_data) = testutils::default_reserve_meta();
            testutils::create_reserve(&e, &pool, &underlying_1, &reserve_config, &reserve_data);

            underlying_0_client.mint(&frodo, &16_0000000);

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
            oracle_client.set_price_stable(&vec![&e, 1_0000000, 5_0000000]);

            let pool_config = PoolConfig {
                oracle,
                min_collateral: 1_0000000,
                bstop_rate: 0_1000000,
                status: 0,
                max_positions: 2,
            };

            e.as_contract(&pool, || {
                e.mock_all_auths_allowing_non_root_auth();
                storage::set_pool_config(&e, &pool_config);
                let requests = vec![
                    &e,
                    Request {
                        request_type: RequestType::SupplyCollateral as u32,
                        address: underlying_0,
                        amount: 15_0000000,
                    },
                    Request {
                        request_type: RequestType::Borrow as u32,
                        address: underlying_1,
                        amount: 1_5000000,
                    },
                ];

                execute_submit(&e, &samwise, &frodo, &merry, requests, true);
            });
        }
        #[test]
        fn test_submit_no_liabilities_does_not_load_oracle() {
            let e = Env::default();
            e.cost_estimate().budget().reset_unlimited();
            e.mock_all_auths_allowing_non_root_auth();

            e.ledger().set(LedgerInfo {
                timestamp: 600,
                protocol_version: 22,
                sequence_number: 1234,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 3110400,
            });

            let bombadil = Address::generate(&e);
            let samwise = Address::generate(&e);
            let frodo = Address::generate(&e);
            let pool = testutils::create_pool(&e);
            let oracle = Address::generate(&e); // will fail if executed against

            let (underlying_0, underlying_0_client) =
                testutils::create_token_contract(&e, &bombadil);
            let (reserve_config, reserve_data) = testutils::default_reserve_meta();
            testutils::create_reserve(&e, &pool, &underlying_0, &reserve_config, &reserve_data);

            let (underlying_1, underlying_1_client) =
                testutils::create_token_contract(&e, &bombadil);
            let (reserve_config, reserve_data) = testutils::default_reserve_meta();
            testutils::create_reserve(&e, &pool, &underlying_1, &reserve_config, &reserve_data);

            underlying_0_client.mint(&frodo, &16_0000000);
            underlying_1_client.mint(&frodo, &10_0000000);

            let pool_config = PoolConfig {
                oracle,
                min_collateral: 1_0000000,
                bstop_rate: 0_1000000,
                status: 0,
                max_positions: 2,
            };
            e.as_contract(&pool, || {
                e.mock_all_auths_allowing_non_root_auth();
                storage::set_pool_config(&e, &pool_config);

                let pre_pool_balance_0 = underlying_0_client.balance(&pool);
                let pre_pool_balance_1 = underlying_1_client.balance(&pool);

                let requests = vec![
                    &e,
                    Request {
                        request_type: RequestType::SupplyCollateral as u32,
                        address: underlying_0,
                        amount: 15_0000000,
                    },
                    // force check_health to true
                    Request {
                        request_type: RequestType::Borrow as u32,
                        address: underlying_1.clone(),
                        amount: 1_5000000,
                    },
                    Request {
                        request_type: RequestType::Repay as u32,
                        address: underlying_1,
                        amount: 1_5000001,
                    },
                ];
                let positions = execute_submit(&e, &samwise, &frodo, &frodo, requests, false);

                assert_eq!(positions.liabilities.len(), 0);
                assert_eq!(positions.collateral.len(), 1);
                assert_eq!(positions.supply.len(), 0);
                assert_eq!(positions.collateral.get_unchecked(0), 14_9999884);

                assert_eq!(
                    underlying_0_client.balance(&pool),
                    pre_pool_balance_0 + 15_0000000
                );
                assert_eq!(
                    underlying_1_client.balance(&pool),
                    pre_pool_balance_1 + 1 // repayment rounded against user
                );

                assert_eq!(underlying_0_client.balance(&frodo), 1_0000000);
                assert_eq!(underlying_1_client.balance(&frodo), 10_0000000 - 1);
            });
        }

        #[test]
        #[should_panic(expected = "Error(Contract, #1205)")]
        fn test_submit_requires_healhty() {
            let e = Env::default();
            e.mock_all_auths();

            let bombadil = Address::generate(&e);
            let samwise = Address::generate(&e);
            let frodo = Address::generate(&e);
            let merry = Address::generate(&e);
            let pool = testutils::create_pool(&e);
            let (oracle, oracle_client) = testutils::create_mock_oracle(&e);

            let (underlying_0, underlying_0_client) =
                testutils::create_token_contract(&e, &bombadil);
            let (reserve_config, reserve_data) = testutils::default_reserve_meta();
            testutils::create_reserve(&e, &pool, &underlying_0, &reserve_config, &reserve_data);

            let (underlying_1, _) = testutils::create_token_contract(&e, &bombadil);
            let (reserve_config, reserve_data) = testutils::default_reserve_meta();
            testutils::create_reserve(&e, &pool, &underlying_1, &reserve_config, &reserve_data);

            underlying_0_client.mint(&frodo, &16_0000000);

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
            oracle_client.set_price_stable(&vec![&e, 1_0000000, 5_0000000]);

            e.ledger().set(LedgerInfo {
                timestamp: 600,
                protocol_version: 22,
                sequence_number: 1234,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 3110400,
            });
            let pool_config = PoolConfig {
                oracle,
                min_collateral: 1_0000000,
                bstop_rate: 0_1000000,
                status: 0,
                max_positions: 2,
            };
            e.as_contract(&pool, || {
                storage::set_pool_config(&e, &pool_config);

                let requests = vec![
                    &e,
                    Request {
                        request_type: RequestType::SupplyCollateral as u32,
                        address: underlying_0,
                        amount: 15_0000000,
                    },
                    Request {
                        request_type: RequestType::Borrow as u32,
                        address: underlying_1,
                        amount: 1_7500000,
                    },
                ];
                execute_submit(&e, &samwise, &frodo, &merry, requests, false);
            });
        }

        #[test]
        #[should_panic(expected = "Error(Contract, #1200)")]
        fn test_submit_from_is_not_self() {
            let e = Env::default();
            e.cost_estimate().budget().reset_unlimited();
            e.mock_all_auths_allowing_non_root_auth();

            e.ledger().set(LedgerInfo {
                timestamp: 600,
                protocol_version: 22,
                sequence_number: 1234,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 3110400,
            });

            let bombadil = Address::generate(&e);
            let samwise = Address::generate(&e);
            let pool = testutils::create_pool(&e);
            let (oracle, oracle_client) = testutils::create_mock_oracle(&e);

            let (underlying_0, underlying_0_client) =
                testutils::create_token_contract(&e, &bombadil);
            let (reserve_config, reserve_data) = testutils::default_reserve_meta();
            testutils::create_reserve(&e, &pool, &underlying_0, &reserve_config, &reserve_data);

            underlying_0_client.mint(&samwise, &16_0000000);

            oracle_client.set_data(
                &bombadil,
                &Asset::Other(Symbol::new(&e, "USD")),
                &vec![&e, Asset::Stellar(underlying_0.clone())],
                &7,
                &300,
            );
            oracle_client.set_price_stable(&vec![&e, 1_0000000]);

            let pool_config = PoolConfig {
                oracle,
                min_collateral: 1_0000000,
                bstop_rate: 0_1000000,
                status: 0,
                max_positions: 2,
            };
            e.as_contract(&pool, || {
                e.mock_all_auths_allowing_non_root_auth();
                storage::set_pool_config(&e, &pool_config);

                let requests = vec![
                    &e,
                    Request {
                        request_type: RequestType::SupplyCollateral as u32,
                        address: underlying_0,
                        amount: 15_0000000,
                    },
                ];
                execute_submit(&e, &pool, &samwise, &samwise, requests, false);
            });
        }

        #[test]
        #[should_panic(expected = "Error(Contract, #1200)")]
        fn test_submit_spender_is_not_self() {
            let e = Env::default();
            e.cost_estimate().budget().reset_unlimited();
            e.mock_all_auths_allowing_non_root_auth();

            e.ledger().set(LedgerInfo {
                timestamp: 600,
                protocol_version: 22,
                sequence_number: 1234,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 3110400,
            });

            let bombadil = Address::generate(&e);
            let samwise = Address::generate(&e);
            let pool = testutils::create_pool(&e);
            let (oracle, oracle_client) = testutils::create_mock_oracle(&e);

            let (underlying_0, underlying_0_client) =
                testutils::create_token_contract(&e, &bombadil);
            let (reserve_config, reserve_data) = testutils::default_reserve_meta();
            testutils::create_reserve(&e, &pool, &underlying_0, &reserve_config, &reserve_data);

            underlying_0_client.mint(&samwise, &16_0000000);

            oracle_client.set_data(
                &bombadil,
                &Asset::Other(Symbol::new(&e, "USD")),
                &vec![&e, Asset::Stellar(underlying_0.clone())],
                &7,
                &300,
            );
            oracle_client.set_price_stable(&vec![&e, 1_0000000]);

            let pool_config = PoolConfig {
                oracle,
                min_collateral: 1_0000000,
                bstop_rate: 0_1000000,
                status: 0,
                max_positions: 2,
            };
            e.as_contract(&pool, || {
                e.mock_all_auths_allowing_non_root_auth();
                storage::set_pool_config(&e, &pool_config);

                let requests = vec![
                    &e,
                    Request {
                        request_type: RequestType::SupplyCollateral as u32,
                        address: underlying_0,
                        amount: 15_0000000,
                    },
                ];
                execute_submit(&e, &samwise, &pool, &samwise, requests, false);
            });
        }

        #[test]
        #[should_panic(expected = "Error(Contract, #1200)")]
        fn test_submit_to_is_not_self() {
            let e = Env::default();
            e.cost_estimate().budget().reset_unlimited();
            e.mock_all_auths_allowing_non_root_auth();

            e.ledger().set(LedgerInfo {
                timestamp: 600,
                protocol_version: 22,
                sequence_number: 1234,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 3110400,
            });

            let bombadil = Address::generate(&e);
            let samwise = Address::generate(&e);
            let pool = testutils::create_pool(&e);
            let (oracle, oracle_client) = testutils::create_mock_oracle(&e);

            let (underlying_0, underlying_0_client) =
                testutils::create_token_contract(&e, &bombadil);
            let (reserve_config, reserve_data) = testutils::default_reserve_meta();
            testutils::create_reserve(&e, &pool, &underlying_0, &reserve_config, &reserve_data);

            underlying_0_client.mint(&samwise, &16_0000000);

            oracle_client.set_data(
                &bombadil,
                &Asset::Other(Symbol::new(&e, "USD")),
                &vec![&e, Asset::Stellar(underlying_0.clone())],
                &7,
                &300,
            );
            oracle_client.set_price_stable(&vec![&e, 1_0000000]);

            let pool_config = PoolConfig {
                oracle,
                min_collateral: 1_0000000,
                bstop_rate: 0_1000000,
                status: 0,
                max_positions: 2,
            };
            e.as_contract(&pool, || {
                e.mock_all_auths_allowing_non_root_auth();
                storage::set_pool_config(&e, &pool_config);

                let requests = vec![
                    &e,
                    Request {
                        request_type: RequestType::SupplyCollateral as u32,
                        address: underlying_0,
                        amount: 15_0000000,
                    },
                ];
                execute_submit(&e, &samwise, &samwise, &pool, requests, false);
            });
        }

        #[test]
        #[should_panic(expected = "Error(Contract, #1208)")]
        fn test_submit_over_max_positions() {
            let e = Env::default();
            e.cost_estimate().budget().reset_unlimited();
            e.mock_all_auths_allowing_non_root_auth();

            e.ledger().set(LedgerInfo {
                timestamp: 600,
                protocol_version: 22,
                sequence_number: 1234,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 3110400,
            });

            let bombadil = Address::generate(&e);
            let samwise = Address::generate(&e);
            let pool = testutils::create_pool(&e);
            let (oracle, oracle_client) = testutils::create_mock_oracle(&e);

            let (underlying_0, underlying_0_client) =
                testutils::create_token_contract(&e, &bombadil);
            let (reserve_config, reserve_data) = testutils::default_reserve_meta();
            testutils::create_reserve(&e, &pool, &underlying_0, &reserve_config, &reserve_data);

            let (underlying_1, underlying_1_client) =
                testutils::create_token_contract(&e, &bombadil);
            let (reserve_config, reserve_data) = testutils::default_reserve_meta();
            testutils::create_reserve(&e, &pool, &underlying_1, &reserve_config, &reserve_data);

            underlying_0_client.mint(&samwise, &10_0000000);
            underlying_1_client.mint(&samwise, &10_0000000);

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
            oracle_client.set_price_stable(&vec![&e, 1_0000000, 5_0000000]);

            let pool_config = PoolConfig {
                oracle,
                min_collateral: 1_0000000,
                bstop_rate: 0_1000000,
                status: 0,
                max_positions: 3,
            };
            let user_positions = Positions {
                liabilities: map![&e, (0, 1_0000000)],
                collateral: map![&e, (0, 15_0000000)],
                supply: map![&e],
            };
            e.as_contract(&pool, || {
                e.mock_all_auths_allowing_non_root_auth();
                storage::set_pool_config(&e, &pool_config);
                storage::set_user_positions(&e, &samwise, &user_positions);

                let requests = vec![
                    &e,
                    Request {
                        request_type: RequestType::Borrow as u32,
                        address: underlying_1.clone(),
                        amount: 1_5000000,
                    },
                    Request {
                        request_type: RequestType::SupplyCollateral as u32,
                        address: underlying_1,
                        amount: 1_0000000,
                    },
                ];
                execute_submit(&e, &samwise, &samwise, &samwise, requests, false);
            });
        }

        #[test]
        fn test_submit_over_max_positions_decrease_allowed() {
            let e = Env::default();
            e.cost_estimate().budget().reset_unlimited();
            e.mock_all_auths_allowing_non_root_auth();

            e.ledger().set(LedgerInfo {
                timestamp: 600,
                protocol_version: 22,
                sequence_number: 1234,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 3110400,
            });

            let bombadil = Address::generate(&e);
            let samwise = Address::generate(&e);
            let pool = testutils::create_pool(&e);
            let (oracle, oracle_client) = testutils::create_mock_oracle(&e);

            let (underlying_0, underlying_0_client) =
                testutils::create_token_contract(&e, &bombadil);
            let (reserve_config, reserve_data) = testutils::default_reserve_meta();
            testutils::create_reserve(&e, &pool, &underlying_0, &reserve_config, &reserve_data);

            let (underlying_1, underlying_1_client) =
                testutils::create_token_contract(&e, &bombadil);
            let (reserve_config, reserve_data) = testutils::default_reserve_meta();
            testutils::create_reserve(&e, &pool, &underlying_1, &reserve_config, &reserve_data);

            underlying_0_client.mint(&samwise, &10_0000000);
            underlying_1_client.mint(&samwise, &10_0000000);

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
            oracle_client.set_price_stable(&vec![&e, 1_0000000, 5_0000000]);

            let pool_config = PoolConfig {
                oracle,
                min_collateral: 1_0000000,
                bstop_rate: 0_1000000,
                status: 0,
                max_positions: 2,
            };
            let user_positions = Positions {
                liabilities: map![&e, (0, 1_0000000), (1, 1_0000000)],
                collateral: map![&e, (0, 15_0000000), (1, 15_0000000)],
                supply: map![&e],
            };
            e.as_contract(&pool, || {
                e.mock_all_auths_allowing_non_root_auth();
                storage::set_pool_config(&e, &pool_config);
                storage::set_user_positions(&e, &samwise, &user_positions);

                let pre_pool_balance_0 = underlying_0_client.balance(&pool);
                let pre_pool_balance_1 = underlying_1_client.balance(&pool);

                let requests = vec![
                    &e,
                    Request {
                        request_type: RequestType::SupplyCollateral as u32,
                        address: underlying_0,
                        amount: 1_0000000,
                    },
                    Request {
                        request_type: RequestType::Repay as u32,
                        address: underlying_1,
                        amount: 2_0000000,
                    },
                ];
                let result = execute_submit(&e, &samwise, &samwise, &samwise, requests, false);

                assert_eq!(result.liabilities.len(), 1);
                assert_eq!(result.collateral.len(), 2);

                assert_eq!(
                    underlying_0_client.balance(&pool),
                    pre_pool_balance_0 + 1_0000000
                );
                assert_eq!(
                    underlying_1_client.balance(&pool),
                    pre_pool_balance_1 + 1_0000012
                );

                assert_eq!(underlying_0_client.balance(&samwise), 9_0000000);
                assert_eq!(
                    underlying_1_client.balance(&samwise),
                    10_0000000 - 1_0000012
                );
            });
        }

        #[test]
        #[should_panic(expected = "Error(Contract, #1212)")]
        fn test_submit_with_ongoing_liquidation_blocked() {
            let e = Env::default();
            e.cost_estimate().budget().reset_unlimited();
            e.mock_all_auths_allowing_non_root_auth();

            e.ledger().set(LedgerInfo {
                timestamp: 600,
                protocol_version: 22,
                sequence_number: 1234,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 3110400,
            });

            let bombadil = Address::generate(&e);
            let samwise = Address::generate(&e);
            let pool = testutils::create_pool(&e);
            let (oracle, oracle_client) = testutils::create_mock_oracle(&e);

            let (underlying_0, underlying_0_client) =
                testutils::create_token_contract(&e, &bombadil);
            let (reserve_config, reserve_data) = testutils::default_reserve_meta();
            testutils::create_reserve(&e, &pool, &underlying_0, &reserve_config, &reserve_data);

            let (underlying_1, underlying_1_client) =
                testutils::create_token_contract(&e, &bombadil);
            let (reserve_config, reserve_data) = testutils::default_reserve_meta();
            testutils::create_reserve(&e, &pool, &underlying_1, &reserve_config, &reserve_data);

            underlying_0_client.mint(&samwise, &10_0000000);
            underlying_1_client.mint(&samwise, &10_0000000);

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
            oracle_client.set_price_stable(&vec![&e, 1_0000000, 5_0000000]);

            let auction_data = AuctionData {
                bid: map![&e, (underlying_0.clone(), 2_0000000)],
                lot: map![&e, (underlying_1.clone(), 2_0000000),],
                block: 1200,
            };
            let pool_config = PoolConfig {
                oracle,
                min_collateral: 1_0000000,
                bstop_rate: 0_1000000,
                status: 0,
                max_positions: 2,
            };
            let user_positions = Positions {
                liabilities: map![&e, (0, 5_0000000)],
                collateral: map![&e, (1, 6_0000000)],
                supply: map![&e],
            };
            e.as_contract(&pool, || {
                e.mock_all_auths_allowing_non_root_auth();
                storage::set_pool_config(&e, &pool_config);
                storage::set_user_positions(&e, &samwise, &user_positions);
                storage::set_auction(
                    &e,
                    &(AuctionType::UserLiquidation as u32),
                    &samwise,
                    &auction_data,
                );

                let requests = vec![
                    &e,
                    Request {
                        request_type: RequestType::Repay as u32,
                        address: underlying_0,
                        amount: 4_0000000,
                    },
                ];
                execute_submit(&e, &samwise, &samwise, &samwise, requests, false);
            });
        }

        #[test]
        fn test_submit_with_ongoing_liquidation_works_if_canceled() {
            let e = Env::default();
            e.cost_estimate().budget().reset_unlimited();
            e.mock_all_auths_allowing_non_root_auth();

            e.ledger().set(LedgerInfo {
                timestamp: 600,
                protocol_version: 22,
                sequence_number: 1234,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 3110400,
            });

            let bombadil = Address::generate(&e);
            let samwise = Address::generate(&e);
            let pool = testutils::create_pool(&e);
            let (oracle, oracle_client) = testutils::create_mock_oracle(&e);

            let (underlying_0, underlying_0_client) =
                testutils::create_token_contract(&e, &bombadil);
            let (reserve_config, reserve_data) = testutils::default_reserve_meta();
            testutils::create_reserve(&e, &pool, &underlying_0, &reserve_config, &reserve_data);

            let (underlying_1, underlying_1_client) =
                testutils::create_token_contract(&e, &bombadil);
            let (reserve_config, reserve_data) = testutils::default_reserve_meta();
            testutils::create_reserve(&e, &pool, &underlying_1, &reserve_config, &reserve_data);

            underlying_0_client.mint(&samwise, &10_0000000);
            underlying_1_client.mint(&samwise, &10_0000000);

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
            oracle_client.set_price_stable(&vec![&e, 1_0000000, 5_0000000]);

            let auction_data = AuctionData {
                bid: map![&e, (underlying_0.clone(), 2_0000000)],
                lot: map![&e, (underlying_1.clone(), 2_0000000),],
                block: 1200,
            };
            let pool_config = PoolConfig {
                oracle,
                min_collateral: 1_0000000,
                bstop_rate: 0_1000000,
                status: 0,
                max_positions: 2,
            };
            let user_positions = Positions {
                liabilities: map![&e, (0, 5_0000000)],
                collateral: map![&e, (1, 6_0000000)],
                supply: map![&e],
            };
            e.as_contract(&pool, || {
                e.mock_all_auths_allowing_non_root_auth();
                storage::set_pool_config(&e, &pool_config);
                storage::set_user_positions(&e, &samwise, &user_positions);
                storage::set_auction(
                    &e,
                    &(AuctionType::UserLiquidation as u32),
                    &samwise,
                    &auction_data,
                );

                let pre_pool_balance_0 = underlying_0_client.balance(&pool);

                let requests = vec![
                    &e,
                    Request {
                        request_type: RequestType::Repay as u32,
                        address: underlying_0,
                        amount: 4_0000000,
                    },
                    Request {
                        request_type: RequestType::DeleteLiquidationAuction as u32,
                        address: samwise.clone(),
                        amount: 0,
                    },
                ];
                let result = execute_submit(&e, &samwise, &samwise, &samwise, requests, false);

                assert_eq!(result.liabilities.len(), 1);
                assert_eq!(result.collateral.len(), 1);

                assert_eq!(result.collateral.get_unchecked(1), 6_0000000);
                assert_eq!(result.liabilities.get_unchecked(0), 1_0000046);

                assert_eq!(
                    underlying_0_client.balance(&pool),
                    pre_pool_balance_0 + 4_0000000
                );
                assert_eq!(
                    underlying_0_client.balance(&samwise),
                    10_0000000 - 4_0000000
                );

                assert!(!storage::has_auction(
                    &e,
                    &(AuctionType::UserLiquidation as u32),
                    &samwise
                ));
            });
        }

        #[test]
        #[should_panic(expected = "Error(Contract, #1224)")]
        fn test_submit_under_min_collateral_fails() {
            let e = Env::default();
            e.cost_estimate().budget().reset_unlimited();
            e.mock_all_auths_allowing_non_root_auth();

            e.ledger().set(LedgerInfo {
                timestamp: 600,
                protocol_version: 22,
                sequence_number: 1234,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 3110400,
            });

            let bombadil = Address::generate(&e);
            let samwise = Address::generate(&e);
            let frodo = Address::generate(&e);
            let merry = Address::generate(&e);
            let pool = testutils::create_pool(&e);
            let (oracle, oracle_client) = testutils::create_mock_oracle(&e);

            let (underlying_0, underlying_0_client) =
                testutils::create_token_contract(&e, &bombadil);
            let (reserve_config, reserve_data) = testutils::default_reserve_meta();
            testutils::create_reserve(&e, &pool, &underlying_0, &reserve_config, &reserve_data);

            let (underlying_1, _) = testutils::create_token_contract(&e, &bombadil);
            let (reserve_config, reserve_data) = testutils::default_reserve_meta();
            testutils::create_reserve(&e, &pool, &underlying_1, &reserve_config, &reserve_data);

            underlying_0_client.mint(&frodo, &16_0000000);

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
            oracle_client.set_price_stable(&vec![&e, 1_0000000, 5_0000000]);

            let pool_config = PoolConfig {
                oracle,
                min_collateral: 1_0000000,
                bstop_rate: 0_1000000,
                status: 0,
                max_positions: 2,
            };
            e.as_contract(&pool, || {
                e.mock_all_auths_allowing_non_root_auth();
                storage::set_pool_config(&e, &pool_config);

                let requests = vec![
                    &e,
                    Request {
                        request_type: RequestType::SupplyCollateral as u32,
                        address: underlying_0,
                        amount: 0_9000000,
                    },
                    Request {
                        request_type: RequestType::Borrow as u32,
                        address: underlying_1,
                        amount: 0_01000000,
                    },
                ];
                execute_submit(&e, &samwise, &frodo, &merry, requests, false);
            });
        }

        #[test]
        fn test_submit_withdraw_over_max_util() {
            let e = Env::default();
            e.cost_estimate().budget().reset_unlimited();
            e.mock_all_auths_allowing_non_root_auth();

            e.ledger().set(LedgerInfo {
                timestamp: 600,
                protocol_version: 22,
                sequence_number: 1234,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 3110400,
            });

            let bombadil = Address::generate(&e);
            let samwise = Address::generate(&e);
            let pool = testutils::create_pool(&e);
            let (oracle, oracle_client) = testutils::create_mock_oracle(&e);

            let (underlying_0, underlying_0_client) =
                testutils::create_token_contract(&e, &bombadil);
            let (mut reserve_config, mut reserve_data) = testutils::default_reserve_meta();
            reserve_config.max_util = 9000000;
            reserve_data.b_supply = 100_0000000;
            reserve_data.d_supply = 89_0000000;
            reserve_data.backstop_credit = 10_0000000;
            testutils::create_reserve(&e, &pool, &underlying_0, &reserve_config, &reserve_data);

            let (underlying_1, underlying_1_client) =
                testutils::create_token_contract(&e, &bombadil);
            let (mut reserve_config, mut reserve_data) = testutils::default_reserve_meta();
            reserve_config.max_util = 7000000;
            reserve_data.b_supply = 100_0000000;
            reserve_data.d_supply = 80_0000000;
            reserve_data.backstop_credit = 5_0000000;
            testutils::create_reserve(&e, &pool, &underlying_1, &reserve_config, &reserve_data);

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
            oracle_client.set_price_stable(&vec![&e, 1_0000000, 5_0000000]);

            let pool_config = PoolConfig {
                oracle,
                min_collateral: 1_0000000,
                bstop_rate: 0_1000000,
                status: 0,
                max_positions: 2,
            };
            let pre_positions = Positions {
                liabilities: map![&e],
                collateral: map![&e, (0, 10_0000000)],
                supply: map![&e, (1, 5_0000000)],
            };
            e.as_contract(&pool, || {
                e.mock_all_auths_allowing_non_root_auth();
                storage::set_pool_config(&e, &pool_config);
                storage::set_user_positions(&e, &samwise, &pre_positions);

                let pre_pool_balance_0 = underlying_0_client.balance(&pool);
                let pre_pool_balance_1 = underlying_1_client.balance(&pool);

                let pre_res_0_data = storage::get_res_data(&e, &underlying_0);
                let pre_res_1_data = storage::get_res_data(&e, &underlying_1);

                let requests = vec![
                    &e,
                    Request {
                        request_type: RequestType::WithdrawCollateral as u32,
                        address: underlying_0.clone(),
                        amount: 5_0000000,
                    },
                    Request {
                        request_type: RequestType::Withdraw as u32,
                        address: underlying_1.clone(),
                        amount: 2_5000000,
                    },
                ];
                let positions = execute_submit(&e, &samwise, &samwise, &samwise, requests, false);

                assert_eq!(positions.liabilities.len(), 0);
                assert_eq!(positions.collateral.len(), 1);
                assert_eq!(positions.supply.len(), 1);
                let b_tokens_0 = positions.collateral.get_unchecked(0);
                assert_eq!(b_tokens_0, 5_0000312);
                let b_tokens_1 = positions.supply.get_unchecked(1);
                assert_eq!(b_tokens_1, 2_5000063);

                let reserve_0 = storage::get_res_data(&e, &underlying_0);
                assert_eq!(
                    reserve_0.b_supply,
                    pre_res_0_data.b_supply - (10_0000000 - b_tokens_0)
                );

                let reserve_1 = storage::get_res_data(&e, &underlying_1);
                assert_eq!(
                    reserve_1.b_supply,
                    pre_res_1_data.b_supply - (5_0000000 - b_tokens_1)
                );

                assert_eq!(
                    underlying_0_client.balance(&pool),
                    pre_pool_balance_0 - 5_0000000
                );
                assert_eq!(
                    underlying_1_client.balance(&pool),
                    pre_pool_balance_1 - 2_5000000
                );

                assert_eq!(underlying_0_client.balance(&samwise), 5_0000000);
                assert_eq!(underlying_1_client.balance(&samwise), 2_5000000);
            });
        }

        #[test]
        #[should_panic(expected = "Error(Contract, #1207)")]
        fn test_submit_borrow_over_max_util() {
            let e = Env::default();
            e.cost_estimate().budget().reset_unlimited();
            e.mock_all_auths_allowing_non_root_auth();

            e.ledger().set(LedgerInfo {
                timestamp: 600,
                protocol_version: 22,
                sequence_number: 1234,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 3110400,
            });

            let bombadil = Address::generate(&e);
            let samwise = Address::generate(&e);
            let pool = testutils::create_pool(&e);
            let (oracle, oracle_client) = testutils::create_mock_oracle(&e);

            let (underlying_0, underlying_0_client) =
                testutils::create_token_contract(&e, &bombadil);
            let (mut reserve_config, mut reserve_data) = testutils::default_reserve_meta();
            reserve_config.max_util = 9000000;
            reserve_data.b_supply = 100_0000000;
            reserve_data.d_supply = 89_0000000;
            testutils::create_reserve(&e, &pool, &underlying_0, &reserve_config, &reserve_data);

            let (underlying_1, underlying_1_client) =
                testutils::create_token_contract(&e, &bombadil);
            let (mut reserve_config, mut reserve_data) = testutils::default_reserve_meta();
            reserve_config.max_util = 7000000;
            reserve_data.b_supply = 100_0000000;
            reserve_data.d_supply = 80_0000000;
            testutils::create_reserve(&e, &pool, &underlying_1, &reserve_config, &reserve_data);

            underlying_0_client.mint(&samwise, &20_0000000);
            underlying_1_client.mint(&samwise, &20_0000000);

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
            oracle_client.set_price_stable(&vec![&e, 1_0000000, 5_0000000]);

            let pool_config = PoolConfig {
                oracle,
                min_collateral: 1_0000000,
                bstop_rate: 0_1000000,
                status: 0,
                max_positions: 4,
            };
            let pre_positions = Positions {
                liabilities: map![&e],
                collateral: map![&e, (1, 10_0000000)],
                supply: map![&e],
            };
            e.as_contract(&pool, || {
                storage::set_pool_config(&e, &pool_config);
                storage::set_user_positions(&e, &samwise, &pre_positions);

                let requests = vec![
                    &e,
                    Request {
                        request_type: RequestType::Supply as u32,
                        address: underlying_0.clone(),
                        amount: 10_0000000,
                    },
                    Request {
                        request_type: RequestType::Borrow as u32,
                        address: underlying_0.clone(),
                        amount: 5_0000000,
                    },
                    Request {
                        request_type: RequestType::Withdraw as u32,
                        address: underlying_0.clone(),
                        amount: 10_0000000,
                    },
                ];
                execute_submit(&e, &samwise, &samwise, &samwise, requests, false);
            });
        }

        /***** submit_with_flash_loan *****/

        #[test]
        fn test_submit_with_flash_loan() {
            let e = Env::default();
            e.cost_estimate().budget().reset_unlimited();
            e.mock_all_auths_allowing_non_root_auth();

            e.ledger().set(LedgerInfo {
                timestamp: 600,
                protocol_version: 22,
                sequence_number: 1234,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 3110400,
            });

            let bombadil = Address::generate(&e);
            let samwise = Address::generate(&e);
            let pool = testutils::create_pool(&e);
            let (oracle, oracle_client) = testutils::create_mock_oracle(&e);

            let (flash_loan_receiver, _) = testutils::create_flashloan_receiver(&e);

            let (underlying_0, underlying_0_client) =
                testutils::create_token_contract(&e, &bombadil);
            let (mut reserve_config, mut reserve_data) = testutils::default_reserve_meta();
            reserve_config.max_util = 9500000;
            reserve_data.b_supply = 100_0000000;
            reserve_data.d_supply = 50_0000000;
            testutils::create_reserve(&e, &pool, &underlying_0, &reserve_config, &reserve_data);

            let (underlying_1, underlying_1_client) =
                testutils::create_token_contract(&e, &bombadil);
            let (reserve_config, reserve_data) = testutils::default_reserve_meta();
            testutils::create_reserve(&e, &pool, &underlying_1, &reserve_config, &reserve_data);

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
            oracle_client.set_price_stable(&vec![&e, 1_0000000, 5_0000000]);

            let pool_config = PoolConfig {
                oracle,
                min_collateral: 1_0000000,
                bstop_rate: 0_1000000,
                status: 0,
                max_positions: 4,
            };
            e.as_contract(&pool, || {
                storage::set_pool_config(&e, &pool_config);

                underlying_1_client.mint(&samwise, &25_0000000);
                underlying_1_client.approve(&samwise, &pool, &100_0000000, &10000);

                let pre_pool_balance_0 = underlying_0_client.balance(&pool);
                let pre_pool_balance_1 = underlying_1_client.balance(&pool);

                let pre_res_0_data = storage::get_res_data(&e, &underlying_0);
                let pre_res_1_data = storage::get_res_data(&e, &underlying_1);

                // pool has 100 supplied and 50 borrowed for asset_0
                // -> max util is 95%
                let flash_loan: FlashLoan = FlashLoan {
                    contract: flash_loan_receiver,
                    asset: underlying_0.clone(),
                    amount: 25_0000000,
                };

                let requests = vec![
                    &e,
                    Request {
                        request_type: RequestType::SupplyCollateral as u32,
                        address: underlying_1.clone(),
                        amount: 25_0000000,
                    },
                ];
                let positions = execute_submit_with_flash_loan(&e, &samwise, flash_loan, requests);

                assert_eq!(positions.liabilities.len(), 1);
                assert_eq!(positions.collateral.len(), 1);
                assert_eq!(positions.supply.len(), 0);
                let b_tokens_minted = positions.collateral.get_unchecked(1);
                assert_eq!(b_tokens_minted, 249999807);
                // actual is 24.999979375 - rounds up
                let d_tokens_minted = positions.liabilities.get_unchecked(0);
                assert_eq!(d_tokens_minted, 249999794);

                let reserve_0 = storage::get_res_data(&e, &underlying_0);
                assert_eq!(
                    reserve_0.d_supply,
                    pre_res_0_data.d_supply + d_tokens_minted
                );

                let reserve_1 = storage::get_res_data(&e, &underlying_1);
                assert_eq!(
                    reserve_1.b_supply,
                    pre_res_1_data.b_supply + b_tokens_minted
                );

                assert_eq!(
                    underlying_0_client.balance(&pool),
                    pre_pool_balance_0 - 25_0000000
                );
                assert_eq!(
                    underlying_1_client.balance(&pool),
                    pre_pool_balance_1 + 25_0000000
                );

                assert_eq!(underlying_0_client.balance(&samwise), 25_0000000);
                assert_eq!(underlying_1_client.balance(&samwise), 0);

                // check allowance is used
                assert_eq!(
                    underlying_1_client.allowance(&samwise, &pool),
                    100_0000000 - 25_0000000
                );
            });
        }

        #[test]
        fn test_submit_with_flash_loan_process_flash_loan_first() {
            let e = Env::default();
            e.cost_estimate().budget().reset_unlimited();
            e.mock_all_auths_allowing_non_root_auth();

            e.ledger().set(LedgerInfo {
                timestamp: 600,
                protocol_version: 22,
                sequence_number: 1234,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 3110400,
            });

            let bombadil = Address::generate(&e);
            let samwise = Address::generate(&e);
            let pool = testutils::create_pool(&e);
            let (oracle, oracle_client) = testutils::create_mock_oracle(&e);

            let (flash_loan_receiver, _) = testutils::create_flashloan_receiver(&e);

            let (underlying_0, underlying_0_client) =
                testutils::create_token_contract(&e, &bombadil);
            let (mut reserve_config, mut reserve_data) = testutils::default_reserve_meta();
            reserve_config.max_util = 9500000;
            reserve_data.b_supply = 100_0000000;
            reserve_data.d_supply = 50_0000000;
            testutils::create_reserve(&e, &pool, &underlying_0, &reserve_config, &reserve_data);

            let (underlying_1, underlying_1_client) =
                testutils::create_token_contract(&e, &bombadil);
            let (reserve_config, reserve_data) = testutils::default_reserve_meta();
            testutils::create_reserve(&e, &pool, &underlying_1, &reserve_config, &reserve_data);

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
            oracle_client.set_price_stable(&vec![&e, 1_0000000, 5_0000000]);

            let pool_config = PoolConfig {
                oracle,
                min_collateral: 1_0000000,
                bstop_rate: 0_1000000,
                status: 0,
                max_positions: 4,
            };
            e.as_contract(&pool, || {
                storage::set_pool_config(&e, &pool_config);

                underlying_0_client.mint(&samwise, &1_0000000);
                underlying_0_client.approve(&samwise, &pool, &100_0000000, &10000);

                let pre_pool_balance_0 = underlying_0_client.balance(&pool);
                let pre_pool_balance_1 = underlying_1_client.balance(&pool);

                let pre_res_0_data = storage::get_res_data(&e, &underlying_0);

                // pool has 100 supplied and 50 borrowed for asset_0
                // -> max util is 95%
                let flash_loan: FlashLoan = FlashLoan {
                    contract: flash_loan_receiver,
                    asset: underlying_0.clone(),
                    amount: 25_0000000,
                };

                let requests = vec![
                    &e,
                    Request {
                        request_type: RequestType::Repay as u32,
                        address: underlying_0.clone(),
                        amount: 25_0000010,
                    },
                ];
                let positions = execute_submit_with_flash_loan(&e, &samwise, flash_loan, requests);

                assert_eq!(positions.liabilities.len(), 0);
                assert_eq!(positions.collateral.len(), 0);
                assert_eq!(positions.supply.len(), 0);

                let reserve_0 = storage::get_res_data(&e, &underlying_0);
                assert_eq!(reserve_0.d_supply, pre_res_0_data.d_supply);

                assert_eq!(underlying_0_client.balance(&pool), pre_pool_balance_0 + 1,);
                assert_eq!(underlying_1_client.balance(&pool), pre_pool_balance_1,);

                // rounding causes 1 stroops to be lost
                assert_eq!(underlying_0_client.balance(&samwise), 0_9999999);
                assert_eq!(underlying_1_client.balance(&samwise), 0);

                // check allowance is used
                assert_eq!(
                    underlying_0_client.allowance(&samwise, &pool),
                    100_0000000 - 25_0000001
                );
            });
        }

        #[test]
        #[should_panic(expected = "Error(Contract, #1205)")]
        fn test_submit_with_flash_loan_checks_health() {
            let e = Env::default();
            e.cost_estimate().budget().reset_unlimited();
            e.mock_all_auths_allowing_non_root_auth();

            e.ledger().set(LedgerInfo {
                timestamp: 600,
                protocol_version: 22,
                sequence_number: 1234,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 3110400,
            });

            let bombadil = Address::generate(&e);
            let samwise = Address::generate(&e);
            let pool = testutils::create_pool(&e);
            let (oracle, oracle_client) = testutils::create_mock_oracle(&e);

            let (flash_loan_receiver, _) = testutils::create_flashloan_receiver(&e);

            let (underlying_0, _) = testutils::create_token_contract(&e, &bombadil);
            let (mut reserve_config, mut reserve_data) = testutils::default_reserve_meta();
            reserve_config.max_util = 9500000;
            reserve_data.b_supply = 100_0000000;
            reserve_data.d_supply = 50_0000000;
            testutils::create_reserve(&e, &pool, &underlying_0, &reserve_config, &reserve_data);

            let (underlying_1, underlying_1_client) =
                testutils::create_token_contract(&e, &bombadil);
            let (reserve_config, reserve_data) = testutils::default_reserve_meta();
            testutils::create_reserve(&e, &pool, &underlying_1, &reserve_config, &reserve_data);

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
            oracle_client.set_price_stable(&vec![&e, 1_0000000, 5_0000000]);

            let pool_config = PoolConfig {
                oracle,
                min_collateral: 1_0000000,
                bstop_rate: 0_1000000,
                status: 0,
                max_positions: 4,
            };
            e.as_contract(&pool, || {
                storage::set_pool_config(&e, &pool_config);

                underlying_1_client.mint(&samwise, &25_0000000);
                underlying_1_client.approve(&samwise, &pool, &100_0000000, &10000);

                // pool has 100 supplied and 50 borrowed for asset_0
                // -> max util is 95%
                let flash_loan: FlashLoan = FlashLoan {
                    contract: flash_loan_receiver,
                    asset: underlying_0,
                    amount: 25_0000000,
                };

                let requests = vec![
                    &e,
                    Request {
                        request_type: RequestType::SupplyCollateral as u32,
                        address: underlying_1,
                        amount: 8_0000000,
                    },
                ];
                execute_submit_with_flash_loan(&e, &samwise, flash_loan, requests);
            });
        }

        #[test]
        #[should_panic(expected = "Error(Contract, #1207)")]
        fn test_submit_with_flash_loan_checks_max_util() {
            let e = Env::default();
            e.cost_estimate().budget().reset_unlimited();
            e.mock_all_auths_allowing_non_root_auth();

            e.ledger().set(LedgerInfo {
                timestamp: 600,
                protocol_version: 22,
                sequence_number: 1234,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 3110400,
            });

            let bombadil = Address::generate(&e);
            let samwise = Address::generate(&e);
            let pool = testutils::create_pool(&e);
            let (oracle, oracle_client) = testutils::create_mock_oracle(&e);

            let (flash_loan_receiver, _) = testutils::create_flashloan_receiver(&e);

            let (underlying_0, _) = testutils::create_token_contract(&e, &bombadil);
            let (mut reserve_config, mut reserve_data) = testutils::default_reserve_meta();
            reserve_config.max_util = 9500000;
            reserve_data.b_supply = 100_0000000;
            reserve_data.d_supply = 50_0000000;
            testutils::create_reserve(&e, &pool, &underlying_0, &reserve_config, &reserve_data);

            let (underlying_1, underlying_1_client) =
                testutils::create_token_contract(&e, &bombadil);
            let (reserve_config, reserve_data) = testutils::default_reserve_meta();
            testutils::create_reserve(&e, &pool, &underlying_1, &reserve_config, &reserve_data);

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
            oracle_client.set_price_stable(&vec![&e, 1_0000000, 5_0000000]);

            let pool_config = PoolConfig {
                oracle,
                min_collateral: 1_0000000,
                bstop_rate: 0_1000000,
                status: 0,
                max_positions: 4,
            };
            e.as_contract(&pool, || {
                storage::set_pool_config(&e, &pool_config);

                underlying_1_client.mint(&samwise, &50_0000000);
                underlying_1_client.approve(&samwise, &pool, &100_0000000, &10000);

                // pool has 100 supplied and 50 borrowed for asset_0
                // -> max util is 95%
                let flash_loan: FlashLoan = FlashLoan {
                    contract: flash_loan_receiver,
                    asset: underlying_0,
                    amount: 46_0000000,
                };

                let requests = vec![
                    &e,
                    Request {
                        request_type: RequestType::SupplyCollateral as u32,
                        address: underlying_1,
                        amount: 50_0000000,
                    },
                ];
                execute_submit_with_flash_loan(&e, &samwise, flash_loan, requests);
            });
        }

        #[test]
        #[should_panic(expected = "Error(Contract, #1208)")]
        fn test_submit_with_flash_loan_over_max_positions() {
            let e = Env::default();
            e.cost_estimate().budget().reset_unlimited();
            e.mock_all_auths_allowing_non_root_auth();

            e.ledger().set(LedgerInfo {
                timestamp: 600,
                protocol_version: 22,
                sequence_number: 1234,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 3110400,
            });

            let bombadil = Address::generate(&e);
            let samwise = Address::generate(&e);
            let pool = testutils::create_pool(&e);
            let (oracle, oracle_client) = testutils::create_mock_oracle(&e);

            let (flash_loan_receiver, _) = testutils::create_flashloan_receiver(&e);

            let (underlying_0, underlying_0_client) =
                testutils::create_token_contract(&e, &bombadil);
            let (reserve_config, reserve_data) = testutils::default_reserve_meta();
            testutils::create_reserve(&e, &pool, &underlying_0, &reserve_config, &reserve_data);

            let (underlying_1, underlying_1_client) =
                testutils::create_token_contract(&e, &bombadil);
            let (reserve_config, reserve_data) = testutils::default_reserve_meta();
            testutils::create_reserve(&e, &pool, &underlying_1, &reserve_config, &reserve_data);

            underlying_0_client.mint(&samwise, &10_0000000);
            underlying_0_client.approve(&samwise, &pool, &10_0000000, &100000);
            underlying_1_client.mint(&samwise, &10_0000000);
            underlying_1_client.approve(&samwise, &pool, &10_0000000, &100000);

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
            oracle_client.set_price_stable(&vec![&e, 1_0000000, 5_0000000]);

            let pool_config = PoolConfig {
                oracle,
                min_collateral: 1_0000000,
                bstop_rate: 0_1000000,
                status: 0,
                max_positions: 3,
            };
            let user_positions = Positions {
                liabilities: map![&e, (1, 1_0000000)],
                collateral: map![&e, (0, 15_0000000)],
                supply: map![&e],
            };
            e.as_contract(&pool, || {
                e.mock_all_auths_allowing_non_root_auth();
                storage::set_pool_config(&e, &pool_config);
                storage::set_user_positions(&e, &samwise, &user_positions);

                let flash_loan: FlashLoan = FlashLoan {
                    contract: flash_loan_receiver,
                    asset: underlying_0,
                    amount: 1_0000000,
                };
                let requests = vec![
                    &e,
                    Request {
                        request_type: RequestType::SupplyCollateral as u32,
                        address: underlying_1,
                        amount: 2_0000000,
                    },
                ];
                execute_submit_with_flash_loan(&e, &samwise, flash_loan, requests);
            });
        }

        #[test]
        fn test_submit_with_flash_loan_over_max_positions_decrease_allowed() {
            let e = Env::default();
            e.cost_estimate().budget().reset_unlimited();
            e.mock_all_auths_allowing_non_root_auth();

            e.ledger().set(LedgerInfo {
                timestamp: 600,
                protocol_version: 22,
                sequence_number: 1234,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 3110400,
            });

            let bombadil = Address::generate(&e);
            let samwise = Address::generate(&e);
            let pool = testutils::create_pool(&e);
            let (oracle, oracle_client) = testutils::create_mock_oracle(&e);

            let (flash_loan_receiver, _) = testutils::create_flashloan_receiver(&e);

            let (underlying_0, underlying_0_client) =
                testutils::create_token_contract(&e, &bombadil);
            let (reserve_config, reserve_data) = testutils::default_reserve_meta();
            testutils::create_reserve(&e, &pool, &underlying_0, &reserve_config, &reserve_data);

            let (underlying_1, underlying_1_client) =
                testutils::create_token_contract(&e, &bombadil);
            let (reserve_config, reserve_data) = testutils::default_reserve_meta();
            testutils::create_reserve(&e, &pool, &underlying_1, &reserve_config, &reserve_data);

            underlying_0_client.mint(&samwise, &10_0000000);
            underlying_0_client.approve(&samwise, &pool, &10_0000000, &100000);
            underlying_1_client.mint(&samwise, &10_0000000);
            underlying_1_client.approve(&samwise, &pool, &10_0000000, &100000);

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
            oracle_client.set_price_stable(&vec![&e, 1_0000000, 5_0000000]);

            let pool_config = PoolConfig {
                oracle,
                min_collateral: 1_0000000,
                bstop_rate: 0_1000000,
                status: 0,
                max_positions: 2,
            };
            let user_positions = Positions {
                liabilities: map![&e, (0, 1_0000000), (1, 1_0000000)],
                collateral: map![&e, (0, 15_0000000), (1, 15_0000000)],
                supply: map![&e],
            };
            e.as_contract(&pool, || {
                e.mock_all_auths_allowing_non_root_auth();
                storage::set_pool_config(&e, &pool_config);
                storage::set_user_positions(&e, &samwise, &user_positions);

                let pre_pool_balance_0 = underlying_0_client.balance(&pool);
                let pre_pool_balance_1 = underlying_1_client.balance(&pool);

                let flash_loan: FlashLoan = FlashLoan {
                    contract: flash_loan_receiver,
                    asset: underlying_0,
                    amount: 1_0000000,
                };
                let requests = vec![
                    &e,
                    Request {
                        request_type: RequestType::Repay as u32,
                        address: underlying_1,
                        amount: 2_0000000,
                    },
                ];
                let result = execute_submit_with_flash_loan(&e, &samwise, flash_loan, requests);

                assert_eq!(result.liabilities.len(), 1);
                assert_eq!(result.collateral.len(), 2);

                assert_eq!(
                    underlying_0_client.balance(&pool),
                    pre_pool_balance_0 - 1_0000000
                );
                assert_eq!(
                    underlying_1_client.balance(&pool),
                    pre_pool_balance_1 + 1_0000012
                );

                assert_eq!(underlying_0_client.balance(&samwise), 11_0000000);
                assert_eq!(
                    underlying_1_client.balance(&samwise),
                    10_0000000 - 1_0000012
                );
            });
        }

        #[test]
        #[should_panic(expected = "Error(Contract, #1212)")]
        fn test_submit_with_flash_loan_with_ongoing_liquidation_blocked() {
            let e = Env::default();
            e.cost_estimate().budget().reset_unlimited();
            e.mock_all_auths_allowing_non_root_auth();

            e.ledger().set(LedgerInfo {
                timestamp: 600,
                protocol_version: 22,
                sequence_number: 1234,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 3110400,
            });

            let bombadil = Address::generate(&e);
            let samwise = Address::generate(&e);
            let pool = testutils::create_pool(&e);
            let (oracle, oracle_client) = testutils::create_mock_oracle(&e);

            let (flash_loan_receiver, _) = testutils::create_flashloan_receiver(&e);

            let (underlying_0, underlying_0_client) =
                testutils::create_token_contract(&e, &bombadil);
            let (reserve_config, reserve_data) = testutils::default_reserve_meta();
            testutils::create_reserve(&e, &pool, &underlying_0, &reserve_config, &reserve_data);

            let (underlying_1, underlying_1_client) =
                testutils::create_token_contract(&e, &bombadil);
            let (reserve_config, reserve_data) = testutils::default_reserve_meta();
            testutils::create_reserve(&e, &pool, &underlying_1, &reserve_config, &reserve_data);

            underlying_0_client.mint(&samwise, &10_0000000);
            underlying_1_client.mint(&samwise, &10_0000000);

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
            oracle_client.set_price_stable(&vec![&e, 1_0000000, 5_0000000]);

            let auction_data = AuctionData {
                bid: map![&e, (underlying_0.clone(), 2_0000000)],
                lot: map![&e, (underlying_1.clone(), 2_0000000),],
                block: 1200,
            };
            let pool_config = PoolConfig {
                oracle,
                min_collateral: 1_0000000,
                bstop_rate: 0_1000000,
                status: 0,
                max_positions: 2,
            };
            let user_positions = Positions {
                liabilities: map![&e, (0, 5_0000000)],
                collateral: map![&e, (1, 6_0000000)],
                supply: map![&e],
            };
            e.as_contract(&pool, || {
                e.mock_all_auths_allowing_non_root_auth();
                storage::set_pool_config(&e, &pool_config);
                storage::set_user_positions(&e, &samwise, &user_positions);
                storage::set_auction(
                    &e,
                    &(AuctionType::UserLiquidation as u32),
                    &samwise,
                    &auction_data,
                );

                let flash_loan: FlashLoan = FlashLoan {
                    contract: flash_loan_receiver,
                    asset: underlying_0.clone(),
                    amount: 1_0000000,
                };
                let requests = vec![
                    &e,
                    Request {
                        request_type: RequestType::Repay as u32,
                        address: underlying_0,
                        amount: 4_5000000,
                    },
                ];
                execute_submit_with_flash_loan(&e, &samwise, flash_loan, requests);
            });
        }

        #[test]
        #[should_panic(expected = "Error(Contract, #1224)")]
        fn test_submit_with_flash_loan_under_min_collateral_fails() {
            let e = Env::default();
            e.cost_estimate().budget().reset_unlimited();
            e.mock_all_auths_allowing_non_root_auth();

            e.ledger().set(LedgerInfo {
                timestamp: 600,
                protocol_version: 22,
                sequence_number: 1234,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 3110400,
            });

            let bombadil = Address::generate(&e);
            let samwise = Address::generate(&e);
            let pool = testutils::create_pool(&e);
            let (oracle, oracle_client) = testutils::create_mock_oracle(&e);

            let (flash_loan_receiver, _) = testutils::create_flashloan_receiver(&e);

            let (underlying_0, underlying_0_client) =
                testutils::create_token_contract(&e, &bombadil);
            let (reserve_config, reserve_data) = testutils::default_reserve_meta();
            testutils::create_reserve(&e, &pool, &underlying_0, &reserve_config, &reserve_data);

            let (underlying_1, underlying_1_client) =
                testutils::create_token_contract(&e, &bombadil);
            let (reserve_config, reserve_data) = testutils::default_reserve_meta();
            testutils::create_reserve(&e, &pool, &underlying_1, &reserve_config, &reserve_data);

            underlying_0_client.mint(&samwise, &20_0000000);
            underlying_0_client.approve(&samwise, &pool, &20_0000000, &100000);
            underlying_1_client.mint(&samwise, &20_0000000);
            underlying_1_client.approve(&samwise, &pool, &20_0000000, &100000);

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
            oracle_client.set_price_stable(&vec![&e, 1_0000000, 5_0000000]);

            let pool_config = PoolConfig {
                oracle,
                min_collateral: 1_0000000,
                bstop_rate: 0_1000000,
                status: 0,
                max_positions: 2,
            };
            e.as_contract(&pool, || {
                e.mock_all_auths_allowing_non_root_auth();
                storage::set_pool_config(&e, &pool_config);

                let flash_loan: FlashLoan = FlashLoan {
                    contract: flash_loan_receiver,
                    asset: underlying_1.clone(),
                    amount: 5_0000000,
                };
                let requests = vec![
                    &e,
                    Request {
                        request_type: RequestType::SupplyCollateral as u32,
                        address: underlying_0,
                        amount: 0_9000000,
                    },
                    Request {
                        request_type: RequestType::Repay as u32,
                        address: underlying_1,
                        amount: 4_9900000,
                    },
                ];
                execute_submit_with_flash_loan(&e, &samwise, flash_loan, requests);
            });
        }

        #[test]
        #[should_panic(expected = "Error(Contract, #1206)")]
        fn test_submit_with_flash_loan_checks_pool_status() {
            let e = Env::default();
            e.cost_estimate().budget().reset_unlimited();
            e.mock_all_auths_allowing_non_root_auth();

            e.ledger().set(LedgerInfo {
                timestamp: 600,
                protocol_version: 22,
                sequence_number: 1234,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 3110400,
            });

            let bombadil = Address::generate(&e);
            let samwise = Address::generate(&e);
            let pool = testutils::create_pool(&e);
            let (oracle, oracle_client) = testutils::create_mock_oracle(&e);

            let (flash_loan_receiver, _) = testutils::create_flashloan_receiver(&e);

            let (underlying_0, _) = testutils::create_token_contract(&e, &bombadil);
            let (mut reserve_config, mut reserve_data) = testutils::default_reserve_meta();
            reserve_config.max_util = 9500000;
            reserve_data.b_supply = 100_0000000;
            reserve_data.d_supply = 50_0000000;
            testutils::create_reserve(&e, &pool, &underlying_0, &reserve_config, &reserve_data);

            let (underlying_1, underlying_1_client) =
                testutils::create_token_contract(&e, &bombadil);
            let (reserve_config, reserve_data) = testutils::default_reserve_meta();
            testutils::create_reserve(&e, &pool, &underlying_1, &reserve_config, &reserve_data);

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
            oracle_client.set_price_stable(&vec![&e, 1_0000000, 5_0000000]);

            let pool_config = PoolConfig {
                oracle,
                min_collateral: 1_0000000,
                bstop_rate: 0_1000000,
                status: 2,
                max_positions: 4,
            };
            e.as_contract(&pool, || {
                storage::set_pool_config(&e, &pool_config);

                underlying_1_client.mint(&samwise, &25_0000000);
                underlying_1_client.approve(&samwise, &pool, &100_0000000, &10000);

                // pool has 100 supplied and 50 borrowed for asset_0
                // -> max util is 95%
                let flash_loan: FlashLoan = FlashLoan {
                    contract: flash_loan_receiver,
                    asset: underlying_0,
                    amount: 25_0000000,
                };

                let requests = vec![
                    &e,
                    Request {
                        request_type: RequestType::SupplyCollateral as u32,
                        address: underlying_1,
                        amount: 25_0000000,
                    },
                ];
                execute_submit_with_flash_loan(&e, &samwise, flash_loan, requests);
            });
        }

        #[test]
        #[should_panic(expected = "Error(Contract, #1223)")]
        fn test_submit_with_flash_loan_checks_reserve_status() {
            let e = Env::default();
            e.cost_estimate().budget().reset_unlimited();
            e.mock_all_auths_allowing_non_root_auth();

            e.ledger().set(LedgerInfo {
                timestamp: 600,
                protocol_version: 22,
                sequence_number: 1234,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 3110400,
            });

            let bombadil = Address::generate(&e);
            let samwise = Address::generate(&e);
            let pool = testutils::create_pool(&e);
            let (oracle, oracle_client) = testutils::create_mock_oracle(&e);

            let (flash_loan_receiver, _) = testutils::create_flashloan_receiver(&e);

            let (underlying_0, _) = testutils::create_token_contract(&e, &bombadil);
            let (mut reserve_config, mut reserve_data) = testutils::default_reserve_meta();
            reserve_config.max_util = 9500000;
            reserve_data.b_supply = 100_0000000;
            reserve_data.d_supply = 50_0000000;
            reserve_config.enabled = false;
            testutils::create_reserve(&e, &pool, &underlying_0, &reserve_config, &reserve_data);

            let (underlying_1, underlying_1_client) =
                testutils::create_token_contract(&e, &bombadil);
            let (reserve_config, reserve_data) = testutils::default_reserve_meta();
            testutils::create_reserve(&e, &pool, &underlying_1, &reserve_config, &reserve_data);

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
            oracle_client.set_price_stable(&vec![&e, 1_0000000, 5_0000000]);

            let pool_config = PoolConfig {
                oracle,
                min_collateral: 1_0000000,
                bstop_rate: 0_1000000,
                status: 0,
                max_positions: 4,
            };
            e.as_contract(&pool, || {
                storage::set_pool_config(&e, &pool_config);

                underlying_1_client.mint(&samwise, &25_0000000);
                underlying_1_client.approve(&samwise, &pool, &100_0000000, &10000);

                // pool has 100 supplied and 50 borrowed for asset_0
                // -> max util is 95%
                let flash_loan: FlashLoan = FlashLoan {
                    contract: flash_loan_receiver,
                    asset: underlying_0,
                    amount: 25_0000000,
                };

                let requests = vec![
                    &e,
                    Request {
                        request_type: RequestType::SupplyCollateral as u32,
                        address: underlying_1,
                        amount: 25_0000000,
                    },
                ];
                execute_submit_with_flash_loan(&e, &samwise, flash_loan, requests);
            });
        }
    }
}

mod pool_src_pool_status {
    use crate::{
        constants::SCALAR_7,
        dependencies::{BackstopClient, PoolBackstopData},
        storage, PoolError,
    };

    use soroban_sdk::{panic_with_error, Env};

    pub(crate) use crate::pool::status::*;

    mod tests {
        use crate::{
            storage::PoolConfig,
            testutils::{
                create_backstop, create_comet_lp_pool, create_pool, create_token_contract,
            },
        };

        use super::*;
        use soroban_sdk::{testutils::Address as _, vec, Address};

        #[test]
        fn test_set_pool_status_active() {
            let e = Env::default();
            e.cost_estimate().budget().reset_unlimited();
            e.mock_all_auths_allowing_non_root_auth();
            let pool_id = create_pool(&e);
            let oracle_id = Address::generate(&e);

            let bombadil = Address::generate(&e);
            let samwise = Address::generate(&e);

            let (blnd, blnd_client) = create_token_contract(&e, &bombadil);
            let (usdc, usdc_client) = create_token_contract(&e, &bombadil);
            let (lp_token, lp_token_client) = create_comet_lp_pool(&e, &bombadil, &blnd, &usdc);
            let (_, backstop_client) = create_backstop(&e, &pool_id, &lp_token, &usdc, &blnd);

            // mint lp tokens
            blnd_client.mint(&samwise, &500_001_0000000);
            blnd_client.approve(&samwise, &lp_token, &i128::MAX, &99999);
            usdc_client.mint(&samwise, &12_501_0000000);
            usdc_client.approve(&samwise, &lp_token, &i128::MAX, &99999);
            lp_token_client.join_pool(
                &50_000_0000000,
                &vec![&e, 500_001_0000000, 12_501_0000000],
                &samwise,
            );
            backstop_client.deposit(&samwise, &pool_id, &50_000_0000000);

            let pool_config = PoolConfig {
                oracle: oracle_id,
                min_collateral: 0,
                bstop_rate: 0,
                status: 1,
                max_positions: 4,
            };
            e.as_contract(&pool_id, || {
                storage::set_admin(&e, &bombadil);
                storage::set_pool_config(&e, &pool_config);

                execute_set_pool_status(&e, 0);

                let new_pool_config = storage::get_pool_config(&e);
                assert_eq!(new_pool_config.status, 0);
            });
        }

        #[test]
        #[should_panic(expected = "Error(Contract, #1204)")]
        fn test_set_pool_status_active_blocks_without_backstop_minimum() {
            let e = Env::default();
            e.cost_estimate().budget().reset_unlimited();
            e.mock_all_auths_allowing_non_root_auth();
            let pool_id = create_pool(&e);
            let oracle_id = Address::generate(&e);

            let bombadil = Address::generate(&e);
            let samwise = Address::generate(&e);

            let (blnd, blnd_client) = create_token_contract(&e, &bombadil);
            let (usdc, usdc_client) = create_token_contract(&e, &bombadil);
            let (lp_token, lp_token_client) = create_comet_lp_pool(&e, &bombadil, &blnd, &usdc);
            let (_, backstop_client) = create_backstop(&e, &pool_id, &lp_token, &usdc, &blnd);

            // mint lp tokens - under limit
            blnd_client.mint(&samwise, &400_001_0000000);
            blnd_client.approve(&samwise, &lp_token, &i128::MAX, &99999);
            usdc_client.mint(&samwise, &10_001_0000000);
            usdc_client.approve(&samwise, &lp_token, &i128::MAX, &99999);
            lp_token_client.join_pool(
                &40_000_0000000,
                &vec![&e, 400_001_0000000, 10_001_0000000],
                &samwise,
            );
            backstop_client.deposit(&samwise, &pool_id, &20_000_0000000);

            let pool_config = PoolConfig {
                oracle: oracle_id,
                min_collateral: 0,
                bstop_rate: 0,
                status: 1,
                max_positions: 4,
            };
            e.as_contract(&pool_id, || {
                storage::set_admin(&e, &bombadil);
                storage::set_pool_config(&e, &pool_config);

                execute_set_pool_status(&e, 0);
            });
        }

        #[test]
        #[should_panic(expected = "Error(Contract, #1204)")]
        fn test_set_pool_status_active_blocks_with_too_high_q4w() {
            let e = Env::default();
            e.cost_estimate().budget().reset_unlimited();
            e.mock_all_auths_allowing_non_root_auth();
            let pool_id = create_pool(&e);
            let oracle_id = Address::generate(&e);

            let bombadil = Address::generate(&e);
            let samwise = Address::generate(&e);

            let (blnd, blnd_client) = create_token_contract(&e, &bombadil);
            let (usdc, usdc_client) = create_token_contract(&e, &bombadil);
            let (lp_token, lp_token_client) = create_comet_lp_pool(&e, &bombadil, &blnd, &usdc);
            let (_, backstop_client) = create_backstop(&e, &pool_id, &lp_token, &usdc, &blnd);

            // mint lp tokens
            blnd_client.mint(&samwise, &500_001_0000000);
            blnd_client.approve(&samwise, &lp_token, &i128::MAX, &99999);
            usdc_client.mint(&samwise, &12_501_0000000);
            usdc_client.approve(&samwise, &lp_token, &i128::MAX, &99999);
            lp_token_client.join_pool(
                &50_000_0000000,
                &vec![&e, 500_001_0000000, 12_501_0000000],
                &samwise,
            );
            backstop_client.deposit(&samwise, &pool_id, &50_000_0000000);
            backstop_client.queue_withdrawal(&samwise, &pool_id, &30_000_0000000);

            let pool_config = PoolConfig {
                oracle: oracle_id,
                min_collateral: 0,
                bstop_rate: 0,
                status: 2,
                max_positions: 4,
            };
            e.as_contract(&pool_id, || {
                storage::set_admin(&e, &bombadil);
                storage::set_pool_config(&e, &pool_config);

                execute_set_pool_status(&e, 0);
            });
        }
        #[test]
        fn test_set_pool_status_on_ice() {
            let e = Env::default();
            e.cost_estimate().budget().reset_unlimited();
            e.mock_all_auths_allowing_non_root_auth();
            let pool_id = create_pool(&e);
            let oracle_id = Address::generate(&e);

            let bombadil = Address::generate(&e);
            let samwise = Address::generate(&e);

            let (blnd, blnd_client) = create_token_contract(&e, &bombadil);
            let (usdc, usdc_client) = create_token_contract(&e, &bombadil);
            let (lp_token, lp_token_client) = create_comet_lp_pool(&e, &bombadil, &blnd, &usdc);
            let (_, backstop_client) = create_backstop(&e, &pool_id, &lp_token, &usdc, &blnd);

            // mint lp tokens
            blnd_client.mint(&samwise, &500_001_0000000);
            blnd_client.approve(&samwise, &lp_token, &i128::MAX, &99999);
            usdc_client.mint(&samwise, &12_501_0000000);
            usdc_client.approve(&samwise, &lp_token, &i128::MAX, &99999);
            lp_token_client.join_pool(
                &50_000_0000000,
                &vec![&e, 500_001_0000000, 12_501_0000000],
                &samwise,
            );
            backstop_client.deposit(&samwise, &pool_id, &50_000_0000000);

            let pool_config = PoolConfig {
                oracle: oracle_id,
                min_collateral: 0,
                bstop_rate: 0,
                status: 1,
                max_positions: 4,
            };
            e.as_contract(&pool_id, || {
                storage::set_admin(&e, &bombadil);
                storage::set_pool_config(&e, &pool_config);

                execute_set_pool_status(&e, 2);

                let new_pool_config = storage::get_pool_config(&e);
                assert_eq!(new_pool_config.status, 2);
            });
        }

        #[test]
        #[should_panic(expected = "Error(Contract, #1204)")]
        fn test_set_pool_status_admin_on_ice_blocks_with_too_high_q4w() {
            let e = Env::default();
            e.cost_estimate().budget().reset_unlimited();
            e.mock_all_auths_allowing_non_root_auth();
            let pool_id = create_pool(&e);
            let oracle_id = Address::generate(&e);

            let bombadil = Address::generate(&e);
            let samwise = Address::generate(&e);

            let (blnd, blnd_client) = create_token_contract(&e, &bombadil);
            let (usdc, usdc_client) = create_token_contract(&e, &bombadil);
            let (lp_token, lp_token_client) = create_comet_lp_pool(&e, &bombadil, &blnd, &usdc);
            let (_, backstop_client) = create_backstop(&e, &pool_id, &lp_token, &usdc, &blnd);

            // mint lp tokens
            blnd_client.mint(&samwise, &500_001_0000000);
            blnd_client.approve(&samwise, &lp_token, &i128::MAX, &99999);
            usdc_client.mint(&samwise, &12_501_0000000);
            usdc_client.approve(&samwise, &lp_token, &i128::MAX, &99999);
            lp_token_client.join_pool(
                &50_000_0000000,
                &vec![&e, 500_001_0000000, 12_501_0000000],
                &samwise,
            );
            backstop_client.deposit(&samwise, &pool_id, &50_000_0000000);
            backstop_client.queue_withdrawal(&samwise, &pool_id, &40_000_0000000);

            let pool_config = PoolConfig {
                oracle: oracle_id,
                min_collateral: 0,
                bstop_rate: 0,
                status: 5,
                max_positions: 4,
            };
            e.as_contract(&pool_id, || {
                storage::set_admin(&e, &bombadil);
                storage::set_pool_config(&e, &pool_config);

                execute_set_pool_status(&e, 2);
            });
        }
        #[test]
        #[should_panic(expected = "Error(Contract, #1204)")]
        fn test_set_pool_status_backstop_on_ice_blocks_with_too_high_q4w() {
            let e = Env::default();
            e.cost_estimate().budget().reset_unlimited();
            e.mock_all_auths_allowing_non_root_auth();
            let pool_id = create_pool(&e);
            let oracle_id = Address::generate(&e);

            let bombadil = Address::generate(&e);
            let samwise = Address::generate(&e);

            let (blnd, blnd_client) = create_token_contract(&e, &bombadil);
            let (usdc, usdc_client) = create_token_contract(&e, &bombadil);
            let (lp_token, lp_token_client) = create_comet_lp_pool(&e, &bombadil, &blnd, &usdc);
            let (_, backstop_client) = create_backstop(&e, &pool_id, &lp_token, &usdc, &blnd);

            // mint lp tokens
            blnd_client.mint(&samwise, &500_001_0000000);
            blnd_client.approve(&samwise, &lp_token, &i128::MAX, &99999);
            usdc_client.mint(&samwise, &12_501_0000000);
            usdc_client.approve(&samwise, &lp_token, &i128::MAX, &99999);
            lp_token_client.join_pool(
                &50_000_0000000,
                &vec![&e, 500_001_0000000, 12_501_0000000],
                &samwise,
            );
            backstop_client.deposit(&samwise, &pool_id, &50_000_0000000);
            backstop_client.queue_withdrawal(&samwise, &pool_id, &40_000_0000000);

            let pool_config = PoolConfig {
                oracle: oracle_id,
                min_collateral: 0,
                bstop_rate: 0,
                status: 6,
                max_positions: 4,
            };
            e.as_contract(&pool_id, || {
                storage::set_admin(&e, &bombadil);
                storage::set_pool_config(&e, &pool_config);

                execute_set_pool_status(&e, 3);
            });
        }
        #[test]
        fn test_set_pool_status_frozen() {
            let e = Env::default();
            e.cost_estimate().budget().reset_unlimited();
            e.mock_all_auths_allowing_non_root_auth();
            let pool_id = create_pool(&e);
            let oracle_id = Address::generate(&e);

            let bombadil = Address::generate(&e);
            let samwise = Address::generate(&e);

            let (blnd, blnd_client) = create_token_contract(&e, &bombadil);
            let (usdc, usdc_client) = create_token_contract(&e, &bombadil);
            let (lp_token, lp_token_client) = create_comet_lp_pool(&e, &bombadil, &blnd, &usdc);
            let (_, backstop_client) = create_backstop(&e, &pool_id, &lp_token, &usdc, &blnd);

            // mint lp tokens
            blnd_client.mint(&samwise, &500_001_0000000);
            blnd_client.approve(&samwise, &lp_token, &i128::MAX, &99999);
            usdc_client.mint(&samwise, &12_501_0000000);
            usdc_client.approve(&samwise, &lp_token, &i128::MAX, &99999);
            lp_token_client.join_pool(
                &50_000_0000000,
                &vec![&e, 500_001_0000000, 12_501_0000000],
                &samwise,
            );
            backstop_client.deposit(&samwise, &pool_id, &50_000_0000000);

            let pool_config = PoolConfig {
                oracle: oracle_id,
                min_collateral: 0,
                bstop_rate: 0,
                status: 1,
                max_positions: 4,
            };
            e.as_contract(&pool_id, || {
                storage::set_admin(&e, &bombadil);
                storage::set_pool_config(&e, &pool_config);

                execute_set_pool_status(&e, 4);

                let new_pool_config = storage::get_pool_config(&e);
                assert_eq!(new_pool_config.status, 4);
            });
        }
        #[test]
        #[should_panic(expected = "Error(Contract, #1200)")]
        fn test_set_non_admin_pool_status_panics() {
            let e = Env::default();
            e.cost_estimate().budget().reset_unlimited();
            e.mock_all_auths_allowing_non_root_auth();
            let pool_id = create_pool(&e);
            let oracle_id = Address::generate(&e);

            let bombadil = Address::generate(&e);
            let samwise = Address::generate(&e);

            let (blnd, blnd_client) = create_token_contract(&e, &bombadil);
            let (usdc, usdc_client) = create_token_contract(&e, &bombadil);
            let (lp_token, lp_token_client) = create_comet_lp_pool(&e, &bombadil, &blnd, &usdc);
            let (_, backstop_client) = create_backstop(&e, &pool_id, &lp_token, &usdc, &blnd);

            // mint lp tokens
            blnd_client.mint(&samwise, &500_001_0000000);
            blnd_client.approve(&samwise, &lp_token, &i128::MAX, &99999);
            usdc_client.mint(&samwise, &12_501_0000000);
            usdc_client.approve(&samwise, &lp_token, &i128::MAX, &99999);
            lp_token_client.join_pool(
                &50_000_0000000,
                &vec![&e, 500_001_0000000, 12_501_0000000],
                &samwise,
            );
            backstop_client.deposit(&samwise, &pool_id, &50_000_0000000);

            let pool_config = PoolConfig {
                oracle: oracle_id,
                min_collateral: 0,
                bstop_rate: 0,
                status: 2,
                max_positions: 4,
            };
            e.as_contract(&pool_id, || {
                storage::set_admin(&e, &bombadil);
                storage::set_pool_config(&e, &pool_config);

                execute_set_pool_status(&e, 1);
            });
        }

        #[test]
        fn test_update_pool_status_active() {
            let e = Env::default();
            e.cost_estimate().budget().reset_unlimited();
            e.mock_all_auths_allowing_non_root_auth();
            let pool_id = create_pool(&e);
            let oracle_id = Address::generate(&e);

            let bombadil = Address::generate(&e);
            let samwise = Address::generate(&e);

            let (blnd, blnd_client) = create_token_contract(&e, &bombadil);
            let (usdc, usdc_client) = create_token_contract(&e, &bombadil);
            let (lp_token, lp_token_client) = create_comet_lp_pool(&e, &bombadil, &blnd, &usdc);
            let (_, backstop_client) = create_backstop(&e, &pool_id, &lp_token, &usdc, &blnd);

            // mint lp tokens
            blnd_client.mint(&samwise, &500_001_0000000);
            blnd_client.approve(&samwise, &lp_token, &i128::MAX, &99999);
            usdc_client.mint(&samwise, &12_501_0000000);
            usdc_client.approve(&samwise, &lp_token, &i128::MAX, &99999);
            lp_token_client.join_pool(
                &50_000_0000000,
                &vec![&e, 500_001_0000000, 12_501_0000000],
                &samwise,
            );
            backstop_client.deposit(&samwise, &pool_id, &50_000_0000000);

            let pool_config = PoolConfig {
                oracle: oracle_id,
                min_collateral: 0,
                bstop_rate: 0,
                status: 3,
                max_positions: 4,
            };
            e.as_contract(&pool_id, || {
                storage::set_admin(&e, &bombadil);
                storage::set_pool_config(&e, &pool_config);

                let status = execute_update_pool_status(&e);

                let new_pool_config = storage::get_pool_config(&e);
                assert_eq!(new_pool_config.status, status);
                assert_eq!(status, 1);
            });
        }

        #[test]
        fn test_update_pool_status_admin_set_no_changes() {
            let e = Env::default();
            e.cost_estimate().budget().reset_unlimited();
            e.mock_all_auths_allowing_non_root_auth();
            let pool_id = create_pool(&e);
            let oracle_id = Address::generate(&e);

            let bombadil = Address::generate(&e);
            let samwise = Address::generate(&e);

            let (blnd, blnd_client) = create_token_contract(&e, &bombadil);
            let (usdc, usdc_client) = create_token_contract(&e, &bombadil);
            let (lp_token, lp_token_client) = create_comet_lp_pool(&e, &bombadil, &blnd, &usdc);
            let (_, backstop_client) = create_backstop(&e, &pool_id, &lp_token, &usdc, &blnd);

            // mint lp tokens
            blnd_client.mint(&samwise, &500_001_0000000);
            blnd_client.approve(&samwise, &lp_token, &i128::MAX, &99999);
            usdc_client.mint(&samwise, &12_501_0000000);
            usdc_client.approve(&samwise, &lp_token, &i128::MAX, &99999);
            lp_token_client.join_pool(
                &50_000_0000000,
                &vec![&e, 500_001_0000000, 12_501_0000000],
                &samwise,
            );
            backstop_client.deposit(&samwise, &pool_id, &50_000_0000000);

            let pool_config = PoolConfig {
                oracle: oracle_id,
                min_collateral: 0,
                bstop_rate: 0,
                status: 0,
                max_positions: 4,
            };
            e.as_contract(&pool_id, || {
                storage::set_admin(&e, &bombadil);
                storage::set_pool_config(&e, &pool_config);

                let status = execute_update_pool_status(&e);

                let new_pool_config = storage::get_pool_config(&e);
                assert_eq!(new_pool_config.status, status);
                assert_eq!(status, 0);
            });
        }

        #[test]
        fn test_update_pool_status_on_ice_tokens() {
            let e = Env::default();
            e.cost_estimate().budget().reset_unlimited();
            e.mock_all_auths_allowing_non_root_auth();
            let pool_id = create_pool(&e);
            let oracle_id = Address::generate(&e);

            let bombadil = Address::generate(&e);
            let samwise = Address::generate(&e);

            let (blnd, blnd_client) = create_token_contract(&e, &bombadil);
            let (usdc, usdc_client) = create_token_contract(&e, &bombadil);
            let (lp_token, lp_token_client) = create_comet_lp_pool(&e, &bombadil, &blnd, &usdc);
            let (_, backstop_client) = create_backstop(&e, &pool_id, &lp_token, &usdc, &blnd);

            // mint lp tokens - under limit
            blnd_client.mint(&samwise, &400_001_0000000);
            blnd_client.approve(&samwise, &lp_token, &i128::MAX, &99999);
            usdc_client.mint(&samwise, &10_001_0000000);
            usdc_client.approve(&samwise, &lp_token, &i128::MAX, &99999);
            lp_token_client.join_pool(
                &40_000_0000000,
                &vec![&e, 400_001_0000000, 10_001_0000000],
                &samwise,
            );
            backstop_client.deposit(&samwise, &pool_id, &20_000_0000000);

            let pool_config = PoolConfig {
                oracle: oracle_id,
                min_collateral: 0,
                bstop_rate: 0,
                status: 1,
                max_positions: 4,
            };
            e.as_contract(&pool_id, || {
                storage::set_admin(&e, &bombadil);
                storage::set_pool_config(&e, &pool_config);

                let status = execute_update_pool_status(&e);

                let new_pool_config = storage::get_pool_config(&e);
                assert_eq!(new_pool_config.status, status);
                assert_eq!(status, 3);
            });
        }

        #[test]
        fn test_update_pool_status_on_ice_30_q4w() {
            let e = Env::default();
            e.cost_estimate().budget().reset_unlimited();
            e.mock_all_auths_allowing_non_root_auth();
            let pool_id = create_pool(&e);
            let oracle_id = Address::generate(&e);

            let bombadil = Address::generate(&e);
            let samwise = Address::generate(&e);

            let (blnd, blnd_client) = create_token_contract(&e, &bombadil);
            let (usdc, usdc_client) = create_token_contract(&e, &bombadil);
            let (lp_token, lp_token_client) = create_comet_lp_pool(&e, &bombadil, &blnd, &usdc);
            let (_, backstop_client) = create_backstop(&e, &pool_id, &lp_token, &usdc, &blnd);

            // mint lp tokens
            blnd_client.mint(&samwise, &500_001_0000000);
            blnd_client.approve(&samwise, &lp_token, &i128::MAX, &99999);
            usdc_client.mint(&samwise, &12_501_0000000);
            usdc_client.approve(&samwise, &lp_token, &i128::MAX, &99999);
            lp_token_client.join_pool(
                &50_000_0000000,
                &vec![&e, 500_001_0000000, 12_501_0000000],
                &samwise,
            );
            backstop_client.deposit(&samwise, &pool_id, &50_000_0000000);
            backstop_client.queue_withdrawal(&samwise, &pool_id, &15_000_0000000);

            let pool_config = PoolConfig {
                oracle: oracle_id,
                min_collateral: 0,
                bstop_rate: 0,
                status: 1,
                max_positions: 4,
            };
            e.as_contract(&pool_id, || {
                storage::set_admin(&e, &bombadil);
                storage::set_pool_config(&e, &pool_config);

                let status = execute_update_pool_status(&e);

                let new_pool_config = storage::get_pool_config(&e);
                assert_eq!(new_pool_config.status, status);
                assert_eq!(status, 3);
            });
        }

        #[test]
        fn test_update_pool_status_on_ice_30_q4w_admin_active() {
            let e = Env::default();
            e.cost_estimate().budget().reset_unlimited();
            e.mock_all_auths_allowing_non_root_auth();
            let pool_id = create_pool(&e);
            let oracle_id = Address::generate(&e);

            let bombadil = Address::generate(&e);
            let samwise = Address::generate(&e);

            let (blnd, blnd_client) = create_token_contract(&e, &bombadil);
            let (usdc, usdc_client) = create_token_contract(&e, &bombadil);
            let (lp_token, lp_token_client) = create_comet_lp_pool(&e, &bombadil, &blnd, &usdc);
            let (_, backstop_client) = create_backstop(&e, &pool_id, &lp_token, &usdc, &blnd);

            // mint lp tokens
            blnd_client.mint(&samwise, &500_001_0000000);
            blnd_client.approve(&samwise, &lp_token, &i128::MAX, &99999);
            usdc_client.mint(&samwise, &12_501_0000000);
            usdc_client.approve(&samwise, &lp_token, &i128::MAX, &99999);
            lp_token_client.join_pool(
                &50_000_0000000,
                &vec![&e, 500_001_0000000, 12_501_0000000],
                &samwise,
            );
            backstop_client.deposit(&samwise, &pool_id, &50_000_0000000);
            backstop_client.queue_withdrawal(&samwise, &pool_id, &15_000_0000000);

            let pool_config = PoolConfig {
                oracle: oracle_id,
                min_collateral: 0,
                bstop_rate: 0,
                status: 0,
                max_positions: 4,
            };
            e.as_contract(&pool_id, || {
                storage::set_admin(&e, &bombadil);
                storage::set_pool_config(&e, &pool_config);

                let status = execute_update_pool_status(&e);

                let new_pool_config = storage::get_pool_config(&e);
                assert_eq!(new_pool_config.status, status);
                assert_eq!(status, 0);
            });
        }

        #[test]
        fn test_update_pool_status_on_ice_50_q4w_admin_active() {
            let e = Env::default();
            e.cost_estimate().budget().reset_unlimited();
            e.mock_all_auths_allowing_non_root_auth();
            let pool_id = create_pool(&e);
            let oracle_id = Address::generate(&e);

            let bombadil = Address::generate(&e);
            let samwise = Address::generate(&e);

            let (blnd, blnd_client) = create_token_contract(&e, &bombadil);
            let (usdc, usdc_client) = create_token_contract(&e, &bombadil);
            let (lp_token, lp_token_client) = create_comet_lp_pool(&e, &bombadil, &blnd, &usdc);
            let (_, backstop_client) = create_backstop(&e, &pool_id, &lp_token, &usdc, &blnd);

            // mint lp tokens
            blnd_client.mint(&samwise, &500_001_0000000);
            blnd_client.approve(&samwise, &lp_token, &i128::MAX, &99999);
            usdc_client.mint(&samwise, &12_501_0000000);
            usdc_client.approve(&samwise, &lp_token, &i128::MAX, &99999);
            lp_token_client.join_pool(
                &50_000_0000000,
                &vec![&e, 500_001_0000000, 12_501_0000000],
                &samwise,
            );
            backstop_client.deposit(&samwise, &pool_id, &50_000_0000000);
            backstop_client.queue_withdrawal(&samwise, &pool_id, &25_000_0000000);

            let pool_config = PoolConfig {
                oracle: oracle_id,
                min_collateral: 0,
                bstop_rate: 0,
                status: 0,
                max_positions: 4,
            };
            e.as_contract(&pool_id, || {
                storage::set_admin(&e, &bombadil);
                storage::set_pool_config(&e, &pool_config);

                let status = execute_update_pool_status(&e);

                let new_pool_config = storage::get_pool_config(&e);
                assert_eq!(new_pool_config.status, status);
                assert_eq!(status, 3);
            });
        }

        #[test]
        fn test_update_pool_status_frozen() {
            let e = Env::default();
            e.cost_estimate().budget().reset_unlimited();
            e.mock_all_auths_allowing_non_root_auth();
            let pool_id = create_pool(&e);
            let oracle_id = Address::generate(&e);

            let bombadil = Address::generate(&e);
            let samwise = Address::generate(&e);

            let (blnd, blnd_client) = create_token_contract(&e, &bombadil);
            let (usdc, usdc_client) = create_token_contract(&e, &bombadil);
            let (lp_token, lp_token_client) = create_comet_lp_pool(&e, &bombadil, &blnd, &usdc);
            let (_, backstop_client) = create_backstop(&e, &pool_id, &lp_token, &usdc, &blnd);

            // mint lp tokens
            blnd_client.mint(&samwise, &500_001_0000000);
            blnd_client.approve(&samwise, &lp_token, &i128::MAX, &99999);
            usdc_client.mint(&samwise, &12_501_0000000);
            usdc_client.approve(&samwise, &lp_token, &i128::MAX, &99999);
            lp_token_client.join_pool(
                &50_000_0000000,
                &vec![&e, 500_001_0000000, 12_501_0000000],
                &samwise,
            );
            backstop_client.deposit(&samwise, &pool_id, &50_000_0000000);
            backstop_client.queue_withdrawal(&samwise, &pool_id, &30_000_0000000);

            let pool_config = PoolConfig {
                oracle: oracle_id,
                min_collateral: 0,
                bstop_rate: 0,
                status: 1,
                max_positions: 4,
            };
            e.as_contract(&pool_id, || {
                storage::set_admin(&e, &bombadil);
                storage::set_pool_config(&e, &pool_config);

                let status = execute_update_pool_status(&e);

                let new_pool_config = storage::get_pool_config(&e);
                assert_eq!(new_pool_config.status, status);
                assert_eq!(status, 5);
            });
        }
        #[test]
        fn test_update_pool_status_frozen_admin_on_ice() {
            let e = Env::default();
            e.cost_estimate().budget().reset_unlimited();
            e.mock_all_auths_allowing_non_root_auth();
            let pool_id = create_pool(&e);
            let oracle_id = Address::generate(&e);

            let bombadil = Address::generate(&e);
            let samwise = Address::generate(&e);

            let (blnd, blnd_client) = create_token_contract(&e, &bombadil);
            let (usdc, usdc_client) = create_token_contract(&e, &bombadil);
            let (lp_token, lp_token_client) = create_comet_lp_pool(&e, &bombadil, &blnd, &usdc);
            let (_, backstop_client) = create_backstop(&e, &pool_id, &lp_token, &usdc, &blnd);

            // mint lp tokens
            blnd_client.mint(&samwise, &500_001_0000000);
            blnd_client.approve(&samwise, &lp_token, &i128::MAX, &99999);
            usdc_client.mint(&samwise, &12_501_0000000);
            usdc_client.approve(&samwise, &lp_token, &i128::MAX, &99999);
            lp_token_client.join_pool(
                &50_000_0000000,
                &vec![&e, 500_001_0000000, 12_501_0000000],
                &samwise,
            );
            backstop_client.deposit(&samwise, &pool_id, &50_000_0000000);
            backstop_client.queue_withdrawal(&samwise, &pool_id, &30_000_0000000);

            let pool_config = PoolConfig {
                oracle: oracle_id,
                min_collateral: 0,
                bstop_rate: 0,
                status: 2,
                max_positions: 4,
            };
            e.as_contract(&pool_id, || {
                storage::set_admin(&e, &bombadil);
                storage::set_pool_config(&e, &pool_config);

                let status = execute_update_pool_status(&e);

                let new_pool_config = storage::get_pool_config(&e);
                assert_eq!(new_pool_config.status, status);
                assert_eq!(status, 2);
            });
        }

        #[test]
        fn test_update_pool_status_frozen_75_q4w() {
            let e = Env::default();
            e.cost_estimate().budget().reset_unlimited();
            e.mock_all_auths_allowing_non_root_auth();
            let pool_id = create_pool(&e);
            let oracle_id = Address::generate(&e);

            let bombadil = Address::generate(&e);
            let samwise = Address::generate(&e);

            let (blnd, blnd_client) = create_token_contract(&e, &bombadil);
            let (usdc, usdc_client) = create_token_contract(&e, &bombadil);
            let (lp_token, lp_token_client) = create_comet_lp_pool(&e, &bombadil, &blnd, &usdc);
            let (_, backstop_client) = create_backstop(&e, &pool_id, &lp_token, &usdc, &blnd);

            // mint lp tokens
            blnd_client.mint(&samwise, &500_001_0000000);
            blnd_client.approve(&samwise, &lp_token, &i128::MAX, &99999);
            usdc_client.mint(&samwise, &12_501_0000000);
            usdc_client.approve(&samwise, &lp_token, &i128::MAX, &99999);
            lp_token_client.join_pool(
                &50_000_0000000,
                &vec![&e, 500_001_0000000, 12_501_0000000],
                &samwise,
            );
            backstop_client.deposit(&samwise, &pool_id, &50_000_0000000);
            backstop_client.queue_withdrawal(&samwise, &pool_id, &40_000_0000000);

            let pool_config = PoolConfig {
                oracle: oracle_id,
                min_collateral: 0,
                bstop_rate: 0,
                status: 2,
                max_positions: 4,
            };
            e.as_contract(&pool_id, || {
                storage::set_admin(&e, &bombadil);
                storage::set_pool_config(&e, &pool_config);

                let status = execute_update_pool_status(&e);

                let new_pool_config = storage::get_pool_config(&e);
                assert_eq!(new_pool_config.status, status);
                assert_eq!(status, 5);
            });
        }

        #[test]
        #[should_panic(expected = "Error(Contract, #1204)")]
        fn test_update_pool_status_admin_frozen() {
            let e = Env::default();
            e.cost_estimate().budget().reset_unlimited();
            e.mock_all_auths_allowing_non_root_auth();
            let pool_id = create_pool(&e);
            let oracle_id = Address::generate(&e);

            let bombadil = Address::generate(&e);
            let samwise = Address::generate(&e);

            let (blnd, blnd_client) = create_token_contract(&e, &bombadil);
            let (usdc, usdc_client) = create_token_contract(&e, &bombadil);
            let (lp_token, lp_token_client) = create_comet_lp_pool(&e, &bombadil, &blnd, &usdc);
            let (_, backstop_client) = create_backstop(&e, &pool_id, &lp_token, &usdc, &blnd);

            // mint lp tokens
            blnd_client.mint(&samwise, &500_001_0000000);
            blnd_client.approve(&samwise, &lp_token, &i128::MAX, &99999);
            usdc_client.mint(&samwise, &12_501_0000000);
            usdc_client.approve(&samwise, &lp_token, &i128::MAX, &99999);
            lp_token_client.join_pool(
                &50_000_0000000,
                &vec![&e, 500_001_0000000, 12_501_0000000],
                &samwise,
            );
            backstop_client.deposit(&samwise, &pool_id, &50_000_0000000);

            let pool_config = PoolConfig {
                oracle: oracle_id,
                min_collateral: 0,
                bstop_rate: 0,
                status: 4,
                max_positions: 4,
            };
            e.as_contract(&pool_id, || {
                storage::set_admin(&e, &bombadil);
                storage::set_pool_config(&e, &pool_config);

                execute_update_pool_status(&e);
            });
        }

        #[test]
        #[should_panic(expected = "Error(Contract, #1204)")]
        fn test_update_pool_status_setup() {
            let e = Env::default();
            e.cost_estimate().budget().reset_unlimited();
            e.mock_all_auths_allowing_non_root_auth();
            let pool_id = create_pool(&e);
            let oracle_id = Address::generate(&e);

            let bombadil = Address::generate(&e);
            let samwise = Address::generate(&e);

            let (blnd, blnd_client) = create_token_contract(&e, &bombadil);
            let (usdc, usdc_client) = create_token_contract(&e, &bombadil);
            let (lp_token, lp_token_client) = create_comet_lp_pool(&e, &bombadil, &blnd, &usdc);
            let (_, backstop_client) = create_backstop(&e, &pool_id, &lp_token, &usdc, &blnd);

            // mint lp tokens
            blnd_client.mint(&samwise, &500_001_0000000);
            blnd_client.approve(&samwise, &lp_token, &i128::MAX, &99999);
            usdc_client.mint(&samwise, &12_501_0000000);
            usdc_client.approve(&samwise, &lp_token, &i128::MAX, &99999);
            lp_token_client.join_pool(
                &50_000_0000000,
                &vec![&e, 500_001_0000000, 12_501_0000000],
                &samwise,
            );
            backstop_client.deposit(&samwise, &pool_id, &50_000_0000000);

            let pool_config = PoolConfig {
                oracle: oracle_id,
                min_collateral: 0,
                bstop_rate: 0,
                status: 6,
                max_positions: 4,
            };
            e.as_contract(&pool_id, || {
                storage::set_admin(&e, &bombadil);
                storage::set_pool_config(&e, &pool_config);

                execute_update_pool_status(&e);
            });
        }

        #[test]
        fn test_admin_update_pool_status_unfreeze() {
            let e = Env::default();
            e.cost_estimate().budget().reset_unlimited();
            e.mock_all_auths_allowing_non_root_auth();

            let pool_id = create_pool(&e);
            let oracle_id = Address::generate(&e);

            let bombadil = Address::generate(&e);
            let samwise = Address::generate(&e);

            let (blnd, blnd_client) = create_token_contract(&e, &bombadil);
            let (usdc, usdc_client) = create_token_contract(&e, &bombadil);
            let (lp_token, lp_token_client) = create_comet_lp_pool(&e, &bombadil, &blnd, &usdc);
            let (_, backstop_client) = create_backstop(&e, &pool_id, &lp_token, &usdc, &blnd);

            // mint lp tokens
            blnd_client.mint(&samwise, &500_001_0000000);
            blnd_client.approve(&samwise, &lp_token, &i128::MAX, &99999);
            usdc_client.mint(&samwise, &12_501_0000000);
            usdc_client.approve(&samwise, &lp_token, &i128::MAX, &99999);
            lp_token_client.join_pool(
                &50_000_0000000,
                &vec![&e, 500_001_0000000, 12_501_0000000],
                &samwise,
            );
            backstop_client.deposit(&samwise, &pool_id, &50_000_0000000);
            backstop_client.queue_withdrawal(&samwise, &pool_id, &12_500_0000000);

            let pool_config = PoolConfig {
                oracle: oracle_id,
                min_collateral: 0,
                bstop_rate: 0,
                status: 5,
                max_positions: 4,
            };
            e.as_contract(&pool_id, || {
                storage::set_admin(&e, &bombadil);
                storage::set_pool_config(&e, &pool_config);

                execute_set_pool_status(&e, 0);

                let new_pool_config = storage::get_pool_config(&e);
                assert_eq!(new_pool_config.status, 0);
            });
        }

        #[test]
        fn test_calc_pool_backstop_threshold() {
            let e = Env::default();
            e.cost_estimate().budget().reset_unlimited();

            let pool_backstop_data = PoolBackstopData {
                blnd: 175_000_0000000,
                q4w_pct: 0,
                tokens: 20_000_0000000,
                shares: 50_000_0000000,
                usdc: 6_500_0000000,
                token_spot_price: 0_5000000,
            }; // ~90.5% threshold

            let result = calc_pool_backstop_threshold(&pool_backstop_data);
            assert_eq!(result, 0_6096289);
        }

        #[test]
        fn test_calc_pool_backstop_threshold_too_small() {
            let e = Env::default();
            e.cost_estimate().budget().reset_unlimited();

            let pool_backstop_data = PoolBackstopData {
                blnd: 5_000_0000000,
                q4w_pct: 0,
                tokens: 500_0000000,
                shares: 1_000_0000000,
                usdc: 1_000_0000000,
                token_spot_price: 0_5000000,
            }; // ~3.6% threshold

            let result = calc_pool_backstop_threshold(&pool_backstop_data);
            assert_eq!(result, 0);
        }

        #[test]
        fn test_calc_pool_backstop_threshold_over() {
            let e = Env::default();
            e.cost_estimate().budget().reset_unlimited();

            let pool_backstop_data = PoolBackstopData {
                blnd: 200_000_0000000,
                q4w_pct: 0,
                tokens: 15_000_0000000,
                shares: 1_000_0000000,
                usdc: 6_250_0000000,
                token_spot_price: 0_5000000,
            }; // 100% threshold

            let result = calc_pool_backstop_threshold(&pool_backstop_data);
            assert_eq!(result, 1_0000000);
        }

        #[test]
        fn test_calc_pool_backstop_threshold_saturates() {
            let e = Env::default();
            e.cost_estimate().budget().reset_unlimited();

            let pool_backstop_data = PoolBackstopData {
                blnd: 50_000_000_0000000,
                q4w_pct: 0,
                tokens: 999_999_0000000,
                shares: 999_999_0000000,
                usdc: 10_000_000_0000000,
                token_spot_price: 0_5000000,
            }; // 362x threshold

            let result = calc_pool_backstop_threshold(&pool_backstop_data);
            assert_eq!(result, 1701411_8346046);
        }

        #[test]
        fn test_calc_pool_backstop_threshold_10_percent() {
            let e = Env::default();
            e.cost_estimate().budget().reset_unlimited();

            let pool_backstop_data = PoolBackstopData {
                blnd: 20_000_0000000,
                q4w_pct: 0,
                tokens: 1_000_0000000,
                shares: 1_000_0000000,
                usdc: 625_0000000,
                token_spot_price: 0_5000000,
            }; // 10% threshold

            let result = calc_pool_backstop_threshold(&pool_backstop_data);
            assert_eq!(result, 0_0000100);
        }

        #[test]
        fn test_calc_pool_backstop_threshold_5pct() {
            let e = Env::default();
            e.cost_estimate().budget().reset_unlimited();

            let pool_backstop_data = PoolBackstopData {
                blnd: 10_000_0000000,
                q4w_pct: 0,
                tokens: 999_999_0000000,
                shares: 999_999_0000000,
                usdc: 312_5000000,
                token_spot_price: 0_5000000,
            }; // 5% threshold

            let result = calc_pool_backstop_threshold(&pool_backstop_data);
            assert_eq!(result, 0_0000003);
        }
    }
}

mod pool_src_pool_reserve {
    use cast::i128;

    use soroban_fixed_point_math::SorobanFixedPoint;

    use soroban_sdk::{contracttype, panic_with_error, Address, Env};

    use crate::{
        constants::{SCALAR_12, SCALAR_7},
        errors::PoolError,
        pool::actions::RequestType,
        storage::{self, PoolConfig, ReserveConfig, ReserveData},
    };

    use crate::pool::interest::calc_accrual;

    pub(crate) use crate::pool::reserve::*;

    mod tests {
        use super::*;
        use crate::testutils;
        use soroban_sdk::testutils::{Address as _, Ledger, LedgerInfo};

        #[test]
        fn test_load_reserve() {
            let e = Env::default();
            e.mock_all_auths();

            e.ledger().set(LedgerInfo {
                timestamp: 123456 * 5,
                protocol_version: 22,
                sequence_number: 123456,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 3110400,
            });

            let bombadil = Address::generate(&e);
            let pool = testutils::create_pool(&e);
            let oracle = Address::generate(&e);

            let (underlying, _) = testutils::create_token_contract(&e, &bombadil);
            let (reserve_config, mut reserve_data) = testutils::default_reserve_meta();
            reserve_data.d_rate = 1_345_678_123_000;
            reserve_data.b_rate = 1_123_456_789_000;
            reserve_data.d_supply = 65_0000000;
            reserve_data.b_supply = 99_0000000;
            testutils::create_reserve(&e, &pool, &underlying, &reserve_config, &reserve_data);

            let pool_config = PoolConfig {
                oracle,
                min_collateral: 1_0000000,
                bstop_rate: 0_2000000,
                status: 0,
                max_positions: 5,
            };
            e.as_contract(&pool, || {
                storage::set_pool_config(&e, &pool_config);
                let reserve = Reserve::load(&e, &pool_config, &underlying);

                // (accrual: 1_002_957_375_248, util: .7864353)
                assert_eq!(reserve.data.d_rate, 1_349_657_798_173);
                assert_eq!(reserve.data.b_rate, 1_125_547_124_242);
                assert_eq!(reserve.data.ir_mod, 1_0449815);
                assert_eq!(reserve.data.d_supply, 65_0000000);
                assert_eq!(reserve.data.b_supply, 99_0000000);
                assert_eq!(reserve.data.backstop_credit, 0_0517357);
                assert_eq!(reserve.data.last_time, 617280);
            });
        }

        #[test]
        fn test_load_reserve_accrues_b_rate() {
            let e = Env::default();
            e.mock_all_auths();

            e.ledger().set(LedgerInfo {
                timestamp: 1000,
                protocol_version: 22,
                sequence_number: 123456,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 3110400,
            });

            let bombadil = Address::generate(&e);
            let pool = testutils::create_pool(&e);
            let oracle = Address::generate(&e);

            // setup load reserve with minimal interest gained (5s / low util / high supply)
            // to validate b/d rate is still safely accrued
            let (underlying, _) = testutils::create_token_contract(&e, &bombadil);
            let (mut reserve_config, mut reserve_data) = testutils::default_reserve_meta();
            reserve_config.decimals = 18;
            let scalar = 10i128.pow(reserve_config.decimals);
            reserve_data.d_rate = 1_500_000_000_000;
            reserve_data.b_rate = 1_300_000_000_000;
            reserve_data.ir_mod = SCALAR_7;
            reserve_data.d_supply = 100_000_000 * scalar;
            reserve_data.b_supply = 10_000_000_000 * scalar;
            reserve_data.last_time = 995;
            testutils::create_reserve(&e, &pool, &underlying, &reserve_config, &reserve_data);

            let pool_config = PoolConfig {
                oracle,
                min_collateral: 1_0000000,
                bstop_rate: 0_2000000,
                status: 0,
                max_positions: 5,
            };
            e.as_contract(&pool, || {
                storage::set_pool_config(&e, &pool_config);
                let reserve = Reserve::load(&e, &pool_config, &underlying);

                // validate that b and d rates are updated
                assert_eq!(reserve.data.last_time, 1000);
                assert_eq!(reserve.data.b_rate, 1_300_000_000_020);
                assert_eq!(reserve.data.d_rate, 1_500_000_002_562);
                assert_eq!(reserve.data.ir_mod, 9999927);
                assert_eq!(reserve.data.backstop_credit, 0_051240000_000000000);
            });
        }

        #[test]
        fn test_load_reserve_zero_supply() {
            let e = Env::default();
            e.mock_all_auths();

            e.ledger().set(LedgerInfo {
                timestamp: 123456 * 5,
                protocol_version: 22,
                sequence_number: 123456,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 3110400,
            });

            let bombadil = Address::generate(&e);
            let pool = testutils::create_pool(&e);
            let oracle = Address::generate(&e);

            let (underlying, _) = testutils::create_token_contract(&e, &bombadil);
            let (reserve_config, mut reserve_data) = testutils::default_reserve_meta();
            reserve_data.d_rate = 0;
            reserve_data.b_rate = 0;
            reserve_data.d_supply = 0;
            reserve_data.b_supply = 0;
            testutils::create_reserve(&e, &pool, &underlying, &reserve_config, &reserve_data);

            let pool_config = PoolConfig {
                oracle,
                min_collateral: 1_0000000,
                bstop_rate: 0_2000000,
                status: 0,
                max_positions: 4,
            };
            e.as_contract(&pool, || {
                storage::set_pool_config(&e, &pool_config);
                let reserve = Reserve::load(&e, &pool_config, &underlying);

                assert_eq!(reserve.data.d_rate, 0);
                assert_eq!(reserve.data.b_rate, 0);
                assert_eq!(reserve.data.ir_mod, 10000000);
                assert_eq!(reserve.data.d_supply, 0);
                assert_eq!(reserve.data.b_supply, 0);
                assert_eq!(reserve.data.backstop_credit, 0);
                assert_eq!(reserve.data.last_time, 617280);
            });
        }

        #[test]
        fn test_load_reserve_zero_util() {
            let e = Env::default();
            e.mock_all_auths();

            e.ledger().set(LedgerInfo {
                timestamp: 123456 * 5,
                protocol_version: 22,
                sequence_number: 123456,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 3110400,
            });

            let bombadil = Address::generate(&e);
            let pool = testutils::create_pool(&e);
            let oracle = Address::generate(&e);

            let (underlying, _) = testutils::create_token_contract(&e, &bombadil);
            let (reserve_config, mut reserve_data) = testutils::default_reserve_meta();
            reserve_data.d_rate = 0;
            reserve_data.d_supply = 0;
            testutils::create_reserve(&e, &pool, &underlying, &reserve_config, &reserve_data);

            let pool_config = PoolConfig {
                oracle,
                min_collateral: 1_0000000,
                bstop_rate: 0_2000000,
                status: 0,
                max_positions: 4,
            };
            e.as_contract(&pool, || {
                storage::set_pool_config(&e, &pool_config);
                let reserve = Reserve::load(&e, &pool_config, &underlying);

                assert_eq!(reserve.data.d_rate, 0);
                assert_eq!(reserve.data.b_rate, reserve_data.b_rate);
                assert_eq!(reserve.data.ir_mod, reserve_data.ir_mod);
                assert_eq!(reserve.data.d_supply, 0);
                assert_eq!(reserve.data.b_supply, reserve_data.b_supply);
                assert_eq!(reserve.data.backstop_credit, 0);
                assert_eq!(reserve.data.last_time, 617280);
            });
        }

        #[test]
        fn test_load_reserve_zero_bstop_rate() {
            let e = Env::default();
            e.mock_all_auths();

            e.ledger().set(LedgerInfo {
                timestamp: 123456 * 5,
                protocol_version: 22,
                sequence_number: 123456,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 3110400,
            });

            let bombadil = Address::generate(&e);
            let pool = testutils::create_pool(&e);
            let oracle = Address::generate(&e);

            let (underlying, _) = testutils::create_token_contract(&e, &bombadil);
            let (reserve_config, mut reserve_data) = testutils::default_reserve_meta();
            reserve_data.d_rate = 1_345_678_123_000;
            reserve_data.b_rate = 1_123_456_789_000;
            reserve_data.d_supply = 65_0000000;
            reserve_data.b_supply = 99_0000000;
            testutils::create_reserve(&e, &pool, &underlying, &reserve_config, &reserve_data);

            let pool_config = PoolConfig {
                oracle,
                min_collateral: 1_0000000,
                bstop_rate: 0,
                status: 0,
                max_positions: 4,
            };
            e.as_contract(&pool, || {
                storage::set_pool_config(&e, &pool_config);
                let reserve = Reserve::load(&e, &pool_config, &underlying);

                // (accrual: 1_002_957_375_248, util: .7864353)
                assert_eq!(reserve.data.d_rate, 1_349_657_798_173);
                assert_eq!(reserve.data.b_rate, 1_126_069_707_070);
                assert_eq!(reserve.data.ir_mod, 1_0449815);
                assert_eq!(reserve.data.d_supply, 65_0000000);
                assert_eq!(reserve.data.b_supply, 99_0000000);
                assert_eq!(reserve.data.backstop_credit, 0);
                assert_eq!(reserve.data.last_time, 617280);
            });
        }

        #[test]
        fn test_store() {
            let e = Env::default();
            e.mock_all_auths();

            e.ledger().set(LedgerInfo {
                timestamp: 123456 * 5,
                protocol_version: 22,
                sequence_number: 123456,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 3110400,
            });

            let bombadil = Address::generate(&e);
            let pool = testutils::create_pool(&e);
            let oracle = Address::generate(&e);

            let (underlying, _) = testutils::create_token_contract(&e, &bombadil);
            let (reserve_config, mut reserve_data) = testutils::default_reserve_meta();
            reserve_data.d_rate = 1_345_678_123_000;
            reserve_data.b_rate = 1_123_456_789_000;
            reserve_data.d_supply = 65_0000000;
            reserve_data.b_supply = 99_0000000;
            testutils::create_reserve(&e, &pool, &underlying, &reserve_config, &reserve_data);

            let pool_config = PoolConfig {
                oracle,
                min_collateral: 1_0000000,
                bstop_rate: 0_2000000,
                status: 0,
                max_positions: 5,
            };
            e.as_contract(&pool, || {
                storage::set_pool_config(&e, &pool_config);
                let reserve = Reserve::load(&e, &pool_config, &underlying);
                reserve.store(&e);

                let reserve_data = storage::get_res_data(&e, &underlying);

                // (accrual: 1_002_957_375_248, util: .7864353)
                assert_eq!(reserve_data.d_rate, 1_349_657_798_173);
                assert_eq!(reserve_data.b_rate, 1_125_547_124_242);
                assert_eq!(reserve_data.ir_mod, 1_0449815);
                assert_eq!(reserve_data.d_supply, 65_0000000);
                assert_eq!(reserve_data.b_supply, 99_0000000);
                assert_eq!(reserve_data.backstop_credit, 0_0517357);
                assert_eq!(reserve_data.last_time, 617280);
            });
        }

        #[test]
        fn test_utilization() {
            let e = Env::default();

            let mut reserve = testutils::default_reserve(&e);
            reserve.data.d_rate = 1_345_678_123_000;
            reserve.data.b_rate = 1_123_456_789_000;
            reserve.data.b_supply = 99_0000000;
            reserve.data.d_supply = 65_0000000;

            let result = reserve.utilization(&e);

            assert_eq!(result, 0_7864353);
        }

        #[test]
        fn test_utilization_empty() {
            let e = Env::default();

            let mut reserve = testutils::default_reserve(&e);
            reserve.data.d_rate = 1_345_678_123_000;
            reserve.data.b_rate = 1_123_456_789_000;
            reserve.data.b_supply = 0;
            reserve.data.d_supply = 0;

            let result = reserve.utilization(&e);

            assert_eq!(result, 0);
        }

        #[test]
        fn test_utilization_no_liabilities() {
            let e = Env::default();

            let mut reserve = testutils::default_reserve(&e);
            reserve.data.d_rate = 1_345_678_123_000;
            reserve.data.b_rate = 1_123_456_789_000;
            reserve.data.b_supply = 1_1234567;
            reserve.data.d_supply = 0;

            let result = reserve.utilization(&e);

            assert_eq!(result, 0);
        }

        #[test]
        fn test_utilization_more_liabilities() {
            let e = Env::default();

            let mut reserve = testutils::default_reserve(&e);
            reserve.data.d_rate = 1_345_678_123_000;
            reserve.data.b_rate = 1_123_456_789_000;
            reserve.data.b_supply = 1_1234567;
            reserve.data.d_supply = 2_1234567;

            let result = reserve.utilization(&e);

            assert_eq!(result, SCALAR_7);
        }

        #[test]
        fn test_require_utilization_below_max_pass() {
            let e = Env::default();

            let mut reserve = testutils::default_reserve(&e);
            reserve.data.b_supply = 99_0000000;
            reserve.data.d_supply = 65_0000000;

            reserve.require_utilization_below_max(&e);
            // no panic
            assert!(true);
        }

        #[test]
        #[should_panic(expected = "Error(Contract, #1207)")]
        fn test_require_utilization_under_max_panic() {
            let e = Env::default();

            let mut reserve = testutils::default_reserve(&e);
            reserve.data.b_supply = 100_0000000;
            reserve.data.d_supply = 95_0000100;

            reserve.require_utilization_below_max(&e);
        }

        #[test]
        fn test_require_utilization_under_100_pass() {
            let e = Env::default();

            let mut reserve = testutils::default_reserve(&e);
            reserve.data.b_supply = 100_0000000;
            reserve.data.d_supply = 99_9000000;

            reserve.require_utilization_below_100(&e);
            // no panic
            assert!(true);
        }

        #[test]
        #[should_panic(expected = "Error(Contract, #1207)")]
        fn test_require_utilization_under_100_panic() {
            let e = Env::default();

            let mut reserve = testutils::default_reserve(&e);
            reserve.data.b_supply = 100_0000000;
            reserve.data.d_supply = 100_0000000;

            reserve.require_utilization_below_100(&e);
        }

        /***** Token Transfer Math *****/

        #[test]
        fn test_to_asset_from_d_token() {
            let e = Env::default();

            let mut reserve = testutils::default_reserve(&e);
            reserve.data.d_rate = 1_321_834_961_000;
            reserve.data.b_supply = 99_0000000;
            reserve.data.d_supply = 65_0000000;

            let result = reserve.to_asset_from_d_token(&e, 1_1234567);

            assert_eq!(result, 1_4850244);
        }

        #[test]
        fn test_to_asset_from_b_token() {
            let e = Env::default();

            let mut reserve = testutils::default_reserve(&e);
            reserve.data.b_rate = 1_321_834_961_000;
            reserve.data.b_supply = 99_0000000;
            reserve.data.d_supply = 65_0000000;

            let result = reserve.to_asset_from_b_token(&e, 1_1234567);

            assert_eq!(result, 1_4850243);
        }

        #[test]
        fn test_to_effective_asset_from_d_token() {
            let e = Env::default();

            let mut reserve = testutils::default_reserve(&e);
            reserve.data.d_rate = 1_321_834_961_000;
            reserve.data.b_supply = 99_0000000;
            reserve.data.d_supply = 65_0000000;
            reserve.config.l_factor = 1_1000000;

            let result = reserve.to_effective_asset_from_d_token(&e, 1_1234567);

            assert_eq!(result, 1_3500222);
        }

        #[test]
        fn test_to_effective_asset_from_b_token() {
            let e = Env::default();

            let mut reserve = testutils::default_reserve(&e);
            reserve.data.b_rate = 1_321_834_961_000;
            reserve.data.b_supply = 99_0000000;
            reserve.data.d_supply = 65_0000000;
            reserve.config.c_factor = 0_8500000;

            let result = reserve.to_effective_asset_from_b_token(&e, 1_1234567);

            assert_eq!(result, 1_2622706);
        }

        #[test]
        fn test_total_liabilities() {
            let e = Env::default();

            let mut reserve = testutils::default_reserve(&e);
            reserve.data.d_rate = 1_823_912_692_000;
            reserve.data.b_supply = 99_0000000;
            reserve.data.d_supply = 65_0000000;

            let result = reserve.total_liabilities(&e);

            assert_eq!(result, 118_5543250);
        }

        #[test]
        fn test_total_supply() {
            let e = Env::default();

            let mut reserve = testutils::default_reserve(&e);
            reserve.data.b_rate = 1_823_912_692_000;
            reserve.data.b_supply = 99_0000000;
            reserve.data.d_supply = 65_0000000;

            let result = reserve.total_supply(&e);

            assert_eq!(result, 180_5673565);
        }

        #[test]
        fn test_to_d_token_up() {
            let e = Env::default();

            let mut reserve = testutils::default_reserve(&e);
            reserve.data.d_rate = 1_321_834_961_999;
            reserve.data.b_supply = 99_0000000;
            reserve.data.d_supply = 65_0000000;

            let result = reserve.to_d_token_up(&e, 1_4850243);

            assert_eq!(result, 1_1234567);
        }

        #[test]
        fn test_to_d_token_down() {
            let e = Env::default();

            let mut reserve = testutils::default_reserve(&e);
            reserve.data.d_rate = 1_321_834_961_000;
            reserve.data.b_supply = 99_0000000;
            reserve.data.d_supply = 65_0000000;

            let result = reserve.to_d_token_down(&e, 1_4850243);

            assert_eq!(result, 1_1234566);
        }

        #[test]
        fn test_to_b_token_up() {
            let e = Env::default();

            let mut reserve = testutils::default_reserve(&e);
            reserve.data.b_rate = 1_321_834_961_999;
            reserve.data.b_supply = 99_0000000;
            reserve.data.d_supply = 65_0000000;

            let result = reserve.to_b_token_up(&e, 1_4850243);

            assert_eq!(result, 1_1234567);
        }

        #[test]
        fn test_to_b_token_down() {
            let e = Env::default();

            let mut reserve = testutils::default_reserve(&e);
            reserve.data.b_rate = 1_321_834_961_000;
            reserve.data.b_supply = 99_0000000;
            reserve.data.d_supply = 65_0000000;

            let result = reserve.to_b_token_down(&e, 1_4850243);

            assert_eq!(result, 1_1234566);
        }

        #[test]
        #[should_panic(expected = "Error(Contract, #1223)")]
        fn test_require_action_allowed_panics_if_supply_disabled_asset() {
            let e = Env::default();

            let mut reserve = testutils::default_reserve(&e);
            reserve.config.enabled = false;

            reserve.require_action_allowed(&e, RequestType::Supply as u32);
        }

        #[test]
        #[should_panic(expected = "Error(Contract, #1223)")]
        fn test_require_action_allowed_panics_if_supply_collateral_disabled_asset() {
            let e = Env::default();

            let mut reserve = testutils::default_reserve(&e);
            reserve.config.enabled = false;

            reserve.require_action_allowed(&e, RequestType::SupplyCollateral as u32);
        }

        #[test]
        #[should_panic(expected = "Error(Contract, #1223)")]
        fn test_require_action_allowed_panics_if_borrow_disabled_asset() {
            let e = Env::default();

            let mut reserve = testutils::default_reserve(&e);
            reserve.config.enabled = false;

            reserve.require_action_allowed(&e, RequestType::Borrow as u32);
        }

        #[test]
        fn test_require_action_allowed_passed_if_withdraw_or_repay() {
            let e = Env::default();

            let mut reserve = testutils::default_reserve(&e);
            reserve.config.enabled = false;

            reserve.require_action_allowed(&e, RequestType::Withdraw as u32);
            reserve.require_action_allowed(&e, RequestType::WithdrawCollateral as u32);
            reserve.require_action_allowed(&e, RequestType::Repay as u32);
        }

        #[test]
        fn test_accrue() {
            let e = Env::default();
            e.mock_all_auths();

            e.ledger().set(LedgerInfo {
                timestamp: 123456 * 5,
                protocol_version: 22,
                sequence_number: 123456,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 3110400,
            });

            let mut reserve = testutils::default_reserve(&e);
            reserve.data.backstop_credit = 0_1234567;

            reserve.accrue(&e, 0_2000000, 100_0000000);
            assert_eq!(reserve.data.backstop_credit, 20_0000000 + 0_1234567);
            assert_eq!(reserve.data.b_rate, 1_800_000_000_000);
            assert_eq!(reserve.data.last_time, 0);
        }

        #[test]
        fn test_accrue_negative_delta_no_change() {
            let e = Env::default();
            e.mock_all_auths();

            e.ledger().set(LedgerInfo {
                timestamp: 123456 * 5,
                protocol_version: 22,
                sequence_number: 123456,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 3110400,
            });

            let mut reserve = testutils::default_reserve(&e);
            reserve.data.backstop_credit = 0_1234567;

            reserve.accrue(&e, 0_2000000, -10_0000000);
            assert_eq!(reserve.data.backstop_credit, 0_1234567);
            assert_eq!(reserve.data.b_rate, 1_000_000_000_000);
            assert_eq!(reserve.data.last_time, 0);
        }
    }
}

mod pool_src_pool_pool {
    use soroban_sdk::{
        map, panic_with_error, unwrap::UnwrapOptimized, vec, Address, Env, Map, Vec,
    };

    use sep_40_oracle::{Asset, PriceFeedClient};

    use crate::{
        errors::PoolError,
        storage::{self, PoolConfig},
        Positions,
    };

    use crate::pool::reserve::Reserve;

    pub(crate) use crate::pool::pool::*;

    mod tests {
        use sep_40_oracle::testutils::Asset;
        use soroban_sdk::{
            testutils::{Address as _, Ledger, LedgerInfo},
            Symbol,
        };

        use crate::{pool::User, storage::ReserveData, testutils};

        use super::*;

        #[test]
        fn test_reserve_cache() {
            let e = Env::default();
            e.mock_all_auths();

            e.ledger().set(LedgerInfo {
                timestamp: 123456 * 5,
                protocol_version: 22,
                sequence_number: 123456,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 3110400,
            });

            let bombadil = Address::generate(&e);
            let pool = testutils::create_pool(&e);
            let oracle = Address::generate(&e);

            let (underlying, _) = testutils::create_token_contract(&e, &bombadil);
            let (reserve_config, reserve_data) = testutils::default_reserve_meta();
            testutils::create_reserve(&e, &pool, &underlying, &reserve_config, &reserve_data);

            let pool_config = PoolConfig {
                oracle,
                min_collateral: 1_0000000,
                bstop_rate: 0_2000000,
                status: 0,
                max_positions: 2,
            };
            e.as_contract(&pool, || {
                storage::set_pool_config(&e, &pool_config);
                let mut pool = Pool::load(&e);
                let reserve = pool.load_reserve(&e, &underlying, true);
                pool.cache_reserve(reserve.clone());

                // delete the reserve data from the ledger to ensure it is loaded from the cache
                storage::set_res_data(
                    &e,
                    &underlying,
                    &ReserveData {
                        b_rate: 0,
                        d_rate: 0,
                        ir_mod: 0,
                        b_supply: 0,
                        d_supply: 0,
                        last_time: 0,
                        backstop_credit: 0,
                    },
                );

                let new_reserve = pool.load_reserve(&e, &underlying, true);
                assert_eq!(new_reserve.data.d_rate, reserve.data.d_rate);

                // store all cached reserves and verify the data is updated
                pool.store_cached_reserves(&e);
                let new_reserve_data = storage::get_res_data(&e, &underlying);
                assert_eq!(new_reserve_data.d_rate, reserve.data.d_rate);
            });
        }

        #[test]
        fn test_reserve_cache_stores_only_marked() {
            let e = Env::default();
            e.mock_all_auths();

            e.ledger().set(LedgerInfo {
                timestamp: 123456 * 5,
                protocol_version: 22,
                sequence_number: 123456,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 3110400,
            });

            let bombadil = Address::generate(&e);
            let pool = testutils::create_pool(&e);
            let oracle = Address::generate(&e);

            let (underlying_0, _) = testutils::create_token_contract(&e, &bombadil);
            let (mut reserve_config, mut reserve_data) = testutils::default_reserve_meta();
            testutils::create_reserve(&e, &pool, &underlying_0, &reserve_config, &reserve_data);

            let (underlying_1, _) = testutils::create_token_contract(&e, &bombadil);
            reserve_config.index = 1;
            reserve_data.d_rate = 1_001_000_000_000;
            testutils::create_reserve(&e, &pool, &underlying_1, &reserve_config, &reserve_data);

            let (underlying_2, _) = testutils::create_token_contract(&e, &bombadil);
            reserve_config.index = 2;
            reserve_data.d_rate = 1_002_000_000_000;
            testutils::create_reserve(&e, &pool, &underlying_2, &reserve_config, &reserve_data);

            let pool_config = PoolConfig {
                oracle,
                min_collateral: 1_0000000,
                bstop_rate: 0_2000000,
                status: 0,
                max_positions: 2,
            };
            e.as_contract(&pool, || {
                storage::set_pool_config(&e, &pool_config);
                let mut pool = Pool::load(&e);
                let reserve_0 = pool.load_reserve(&e, &underlying_0, false);
                let mut reserve_1 = pool.load_reserve(&e, &underlying_1, true);
                let mut reserve_2 = pool.load_reserve(&e, &underlying_2, true);
                reserve_2.data.d_rate = 456;
                pool.cache_reserve(reserve_0.clone());
                pool.cache_reserve(reserve_1.clone());
                pool.cache_reserve(reserve_2.clone());

                // verify a duplicate cache takes the most recently cached
                reserve_1.data.d_rate = 123;
                pool.cache_reserve(reserve_1.clone());

                // verify reloading without store flag still stores reserve
                let _ = pool.load_reserve(&e, &underlying_2, false);

                // delete the reserve data from the ledger to ensure it is loaded from the cache
                storage::set_res_data(
                    &e,
                    &underlying_0,
                    &ReserveData {
                        b_rate: 0,
                        d_rate: 0,
                        ir_mod: 0,
                        b_supply: 0,
                        d_supply: 0,
                        last_time: 0,
                        backstop_credit: 0,
                    },
                );

                let new_reserve = pool.load_reserve(&e, &underlying_0, false);
                assert_eq!(new_reserve.data.d_rate, reserve_0.data.d_rate);

                // store all cached reserves and verify the temp one was not stored
                pool.store_cached_reserves(&e);
                let new_reserve_data = storage::get_res_data(&e, &underlying_0);
                assert_eq!(new_reserve_data.d_rate, 0);
                let new_reserve_data = storage::get_res_data(&e, &reserve_1.asset);
                assert_eq!(new_reserve_data.d_rate, 123);
                let new_reserve_data = storage::get_res_data(&e, &reserve_2.asset);
                assert_eq!(new_reserve_data.d_rate, 456);
            });
        }

        #[test]
        fn test_reserve_cache_does_nothing_if_nothing_marked() {
            let e = Env::default();
            e.mock_all_auths();

            e.ledger().set(LedgerInfo {
                timestamp: 123456 * 5,
                protocol_version: 22,
                sequence_number: 123456,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 3110400,
            });

            let bombadil = Address::generate(&e);
            let pool = testutils::create_pool(&e);
            let oracle = Address::generate(&e);

            let (underlying_0, _) = testutils::create_token_contract(&e, &bombadil);
            let (mut reserve_config, reserve_data_0) = testutils::default_reserve_meta();
            testutils::create_reserve(&e, &pool, &underlying_0, &reserve_config, &reserve_data_0);

            let (underlying_1, _) = testutils::create_token_contract(&e, &bombadil);
            let mut reserve_data_1 = reserve_data_0.clone();
            reserve_config.index = 1;
            reserve_data_1.d_rate = 1_001_000_000_000;
            testutils::create_reserve(&e, &pool, &underlying_1, &reserve_config, &reserve_data_1);

            let pool_config = PoolConfig {
                oracle,
                min_collateral: 1_0000000,
                bstop_rate: 0_2000000,
                status: 0,
                max_positions: 2,
            };
            e.as_contract(&pool, || {
                storage::set_pool_config(&e, &pool_config);
                let mut pool = Pool::load(&e);
                let mut reserve_0 = pool.load_reserve(&e, &underlying_0, false);
                reserve_0.data.d_rate = 0;
                let mut reserve_1 = pool.load_reserve(&e, &underlying_1, false);
                reserve_1.data.d_rate = 0;

                pool.cache_reserve(reserve_0.clone());
                pool.cache_reserve(reserve_1.clone());

                // store all cached reserves and verify the temp one was not stored
                pool.store_cached_reserves(&e);
                let new_reserve_data = storage::get_res_data(&e, &underlying_0);
                assert_eq!(new_reserve_data.d_rate, reserve_data_0.d_rate);
                let new_reserve_data = storage::get_res_data(&e, &underlying_1);
                assert_eq!(new_reserve_data.d_rate, reserve_data_1.d_rate);
            });
        }

        #[test]
        #[should_panic(expected = "Error(Contract, #1209)")]
        fn test_reserve_cache_panics_if_missing_reserve_to_store() {
            let e = Env::default();
            e.mock_all_auths();

            e.ledger().set(LedgerInfo {
                timestamp: 123456 * 5,
                protocol_version: 22,
                sequence_number: 123456,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 3110400,
            });

            let bombadil = Address::generate(&e);
            let pool = testutils::create_pool(&e);
            let oracle = Address::generate(&e);

            let (underlying_0, _) = testutils::create_token_contract(&e, &bombadil);
            let (mut reserve_config, mut reserve_data) = testutils::default_reserve_meta();
            testutils::create_reserve(&e, &pool, &underlying_0, &reserve_config, &reserve_data);

            let (underlying_1, _) = testutils::create_token_contract(&e, &bombadil);
            reserve_config.index = 1;
            reserve_data.d_rate = 1_001_000_000_000;
            testutils::create_reserve(&e, &pool, &underlying_1, &reserve_config, &reserve_data);

            let (underlying_2, _) = testutils::create_token_contract(&e, &bombadil);
            reserve_config.index = 2;
            reserve_data.d_rate = 1_002_000_000_000;
            testutils::create_reserve(&e, &pool, &underlying_2, &reserve_config, &reserve_data);

            let pool_config = PoolConfig {
                oracle,
                min_collateral: 1_0000000,
                bstop_rate: 0_2000000,
                status: 0,
                max_positions: 2,
            };
            e.as_contract(&pool, || {
                storage::set_pool_config(&e, &pool_config);
                let mut pool = Pool::load(&e);
                let reserve_0 = pool.load_reserve(&e, &underlying_0, false);
                let mut reserve_1 = pool.load_reserve(&e, &underlying_1, true);
                let mut reserve_2 = pool.load_reserve(&e, &underlying_2, true);
                reserve_1.data.b_rate = 123;
                reserve_2.data.d_rate = 456;
                pool.cache_reserve(reserve_0.clone());
                pool.cache_reserve(reserve_1.clone());
                // pool.cache_reserve(reserve_2.clone());

                pool.store_cached_reserves(&e);
            });
        }

        #[test]
        #[should_panic(expected = "Error(Contract, #1206)")]
        fn test_require_action_allowed_borrow_while_on_ice_panics() {
            let e = Env::default();
            e.mock_all_auths();
            let pool = testutils::create_pool(&e);
            let oracle = Address::generate(&e);
            let pool_config = PoolConfig {
                oracle,
                min_collateral: 1_0000000,
                bstop_rate: 0_2000000,
                status: 2,
                max_positions: 2,
            };
            e.as_contract(&pool, || {
                storage::set_pool_config(&e, &pool_config);
                let pool = Pool::load(&e);

                pool.require_action_allowed(&e, 4);
            });
        }

        #[test]
        fn test_require_action_allowed_borrow_while_active() {
            let e = Env::default();
            e.mock_all_auths();
            let pool = testutils::create_pool(&e);
            let oracle = Address::generate(&e);
            let pool_config = PoolConfig {
                oracle,
                min_collateral: 1_0000000,
                bstop_rate: 0_2000000,
                status: 1,
                max_positions: 2,
            };
            e.as_contract(&pool, || {
                storage::set_pool_config(&e, &pool_config);
                let pool = Pool::load(&e);

                pool.require_action_allowed(&e, 4);
            });
        }

        #[test]
        #[should_panic(expected = "Error(Contract, #1206)")]
        fn test_require_action_allowed_cancel_liquidation_while_on_ice_panics() {
            let e = Env::default();
            e.mock_all_auths();
            let pool = testutils::create_pool(&e);
            let oracle = Address::generate(&e);
            let pool_config = PoolConfig {
                oracle,
                min_collateral: 1_0000000,
                bstop_rate: 0_2000000,
                status: 2,
                max_positions: 2,
            };
            e.as_contract(&pool, || {
                storage::set_pool_config(&e, &pool_config);
                let pool = Pool::load(&e);

                pool.require_action_allowed(&e, 9);
            });
        }

        #[test]
        fn test_require_action_allowed_cancel_liquidation_while_active() {
            let e = Env::default();
            e.mock_all_auths();
            let pool = testutils::create_pool(&e);
            let oracle = Address::generate(&e);
            let pool_config = PoolConfig {
                oracle,
                min_collateral: 1_0000000,
                bstop_rate: 0_2000000,
                status: 1,
                max_positions: 2,
            };
            e.as_contract(&pool, || {
                storage::set_pool_config(&e, &pool_config);
                let pool = Pool::load(&e);

                pool.require_action_allowed(&e, 9);
            });
        }

        #[test]
        #[should_panic(expected = "Error(Contract, #1206)")]
        fn test_require_action_allowed_supply_while_frozen() {
            let e = Env::default();
            e.mock_all_auths();
            let pool = testutils::create_pool(&e);
            let oracle = Address::generate(&e);
            let pool_config = PoolConfig {
                oracle,
                min_collateral: 1_0000000,
                bstop_rate: 0_2000000,
                status: 4,
                max_positions: 2,
            };
            e.as_contract(&pool, || {
                storage::set_pool_config(&e, &pool_config);
                let pool = Pool::load(&e);

                pool.require_action_allowed(&e, 0);
            });
        }

        #[test]
        #[should_panic(expected = "Error(Contract, #1206)")]
        fn test_require_action_allowed_supply_collateral_while_frozen() {
            let e = Env::default();
            e.mock_all_auths();
            let pool = testutils::create_pool(&e);
            let oracle = Address::generate(&e);
            let pool_config = PoolConfig {
                oracle,
                min_collateral: 1_0000000,
                bstop_rate: 0_2000000,
                status: 4,
                max_positions: 2,
            };
            e.as_contract(&pool, || {
                storage::set_pool_config(&e, &pool_config);
                let pool = Pool::load(&e);

                pool.require_action_allowed(&e, 2);
            });
        }

        #[test]
        fn test_require_action_allowed_can_withdrawal_and_repay_while_frozen() {
            let e = Env::default();
            e.mock_all_auths();
            let pool = testutils::create_pool(&e);
            let oracle = Address::generate(&e);
            let pool_config = PoolConfig {
                oracle,
                min_collateral: 1_0000000,
                bstop_rate: 0_2000000,
                status: 4,
                max_positions: 2,
            };
            e.as_contract(&pool, || {
                storage::set_pool_config(&e, &pool_config);
                let pool = Pool::load(&e);

                pool.require_action_allowed(&e, 5);
                pool.require_action_allowed(&e, 1);
                pool.require_action_allowed(&e, 3);
                // no panic
                assert!(true);
            });
        }

        #[test]
        fn test_load_price_decimals() {
            let e = Env::default();
            e.mock_all_auths();

            let pool = testutils::create_pool(&e);
            let (oracle, oracle_client) = testutils::create_mock_oracle(&e);
            oracle_client.set_data(
                &Address::generate(&e),
                &Asset::Stellar(Address::generate(&e)),
                &vec![&e, Asset::Stellar(Address::generate(&e))],
                &7,
                &300,
            );
            let pool_config = PoolConfig {
                oracle,
                min_collateral: 1_0000000,
                bstop_rate: 0_2000000,
                status: 0,
                max_positions: 2,
            };
            e.as_contract(&pool, || {
                storage::set_pool_config(&e, &pool_config);
                let mut pool = Pool::load(&e);

                let decimals = pool.load_price_decimals(&e);
                assert_eq!(decimals, 7);
            });
        }

        #[test]
        fn test_load_price() {
            let e = Env::default();
            e.mock_all_auths_allowing_non_root_auth();

            let bombadil = Address::generate(&e);
            let pool = testutils::create_pool(&e);
            let asset_0 = Address::generate(&e);
            let asset_1 = Address::generate(&e);
            let (oracle, oracle_client) = testutils::create_mock_oracle(&e);

            oracle_client.set_data(
                &bombadil,
                &Asset::Other(Symbol::new(&e, "USD")),
                &vec![
                    &e,
                    Asset::Stellar(asset_0.clone()),
                    Asset::Stellar(asset_1.clone()),
                ],
                &7,
                &300,
            );
            oracle_client.set_price_stable(&vec![&e, 123, 456]);

            let pool_config = PoolConfig {
                oracle,
                min_collateral: 1_0000000,
                bstop_rate: 0_2000000,
                status: 0,
                max_positions: 2,
            };
            e.as_contract(&pool, || {
                storage::set_pool_config(&e, &pool_config);
                let mut pool = Pool::load(&e);

                let price = pool.load_price(&e, &asset_0);
                assert_eq!(price, 123);

                let price = pool.load_price(&e, &asset_1);
                assert_eq!(price, 456);

                // verify the price is cached
                oracle_client.set_price_stable(&vec![&e, 789, 101112]);
                let price = pool.load_price(&e, &asset_0);
                assert_eq!(price, 123);
            });
        }

        #[test]
        #[should_panic(expected = "Error(Contract, #1210)")]
        fn test_load_price_panics_if_stale() {
            let e = Env::default();
            e.mock_all_auths_allowing_non_root_auth();

            e.ledger().set(LedgerInfo {
                timestamp: 1000 + 24 * 60 * 60 + 1,
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
            let asset = Address::generate(&e);
            let (oracle, oracle_client) = testutils::create_mock_oracle(&e);
            oracle_client.set_data(
                &bombadil,
                &Asset::Other(Symbol::new(&e, "USD")),
                &vec![&e, Asset::Stellar(asset.clone())],
                &7,
                &300,
            );
            oracle_client.set_price(&vec![&e, 123], &1000);
            let pool_config = PoolConfig {
                oracle,
                min_collateral: 1_0000000,
                bstop_rate: 0_2000000,
                status: 0,
                max_positions: 2,
            };
            e.as_contract(&pool, || {
                storage::set_pool_config(&e, &pool_config);
                let mut pool = Pool::load(&e);

                pool.load_price(&e, &asset);
                assert!(false);
            });
        }

        #[test]
        #[should_panic(expected = "Error(Contract, #1210)")]
        fn test_load_price_panics_if_zero() {
            let e = Env::default();
            e.mock_all_auths_allowing_non_root_auth();

            e.ledger().set(LedgerInfo {
                timestamp: 1000 + 24 * 60 * 60 + 1,
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
            let asset = Address::generate(&e);
            let (oracle, oracle_client) = testutils::create_mock_oracle(&e);
            oracle_client.set_data(
                &bombadil,
                &Asset::Other(Symbol::new(&e, "USD")),
                &vec![&e, Asset::Stellar(asset.clone())],
                &7,
                &300,
            );
            oracle_client.set_price(&vec![&e, -1], &(1000 + 24 * 60 * 60));
            let pool_config = PoolConfig {
                oracle,
                min_collateral: 1_0000000,
                bstop_rate: 0_2000000,
                status: 0,
                max_positions: 2,
            };
            e.as_contract(&pool, || {
                storage::set_pool_config(&e, &pool_config);
                let mut pool = Pool::load(&e);

                pool.load_price(&e, &asset);
                assert!(false);
            });
        }

        #[test]
        fn test_require_under_max_empty() {
            let e = Env::default();
            e.mock_all_auths();
            let samwise = Address::generate(&e);
            let pool = testutils::create_pool(&e);

            let mut reserve_0 = testutils::default_reserve(&e);
            let (oracle, _) = testutils::create_mock_oracle(&e);
            let mut user = User {
                address: samwise.clone(),
                positions: Positions::env_default(&e),
            };
            let pool_config = PoolConfig {
                oracle,
                min_collateral: 1_0000000,
                bstop_rate: 0_2000000,
                status: 0,
                max_positions: 2,
            };
            e.as_contract(&pool, || {
                storage::set_pool_config(&e, &pool_config);
                let prev_positions = user.positions.effective_count();

                let pool = Pool::load(&e);
                user.add_collateral(&e, &mut reserve_0, 1);

                pool.require_under_max(&e, &user.positions, prev_positions);
            });
        }

        #[test]
        fn test_require_under_max_ignores_supply() {
            let e = Env::default();
            e.mock_all_auths();
            let samwise = Address::generate(&e);
            let pool = testutils::create_pool(&e);

            let mut reserve_0 = testutils::default_reserve(&e);
            let mut reserve_1 = testutils::default_reserve(&e);
            reserve_1.config.index = 1;

            let (oracle, _) = testutils::create_mock_oracle(&e);
            let mut user = User {
                address: samwise.clone(),
                positions: Positions::env_default(&e),
            };
            let pool_config = PoolConfig {
                oracle,
                min_collateral: 1_0000000,
                bstop_rate: 0_2000000,
                status: 0,
                max_positions: 2,
            };
            e.as_contract(&pool, || {
                storage::set_pool_config(&e, &pool_config);
                user.add_supply(&e, &mut reserve_0, 42);
                user.add_supply(&e, &mut reserve_1, 42);
                user.add_collateral(&e, &mut reserve_1, 1);
                let prev_positions = user.positions.effective_count();

                let pool = Pool::load(&e);
                user.add_liabilities(&e, &mut reserve_1, 2);

                pool.require_under_max(&e, &user.positions, prev_positions);
            });
        }

        #[test]
        fn test_require_under_max_allows_decreasing_change() {
            let e = Env::default();
            e.mock_all_auths();
            let samwise = Address::generate(&e);
            let pool = testutils::create_pool(&e);

            let mut reserve_0 = testutils::default_reserve(&e);
            let mut reserve_1 = testutils::default_reserve(&e);
            reserve_1.config.index = 1;

            let (oracle, _) = testutils::create_mock_oracle(&e);
            let mut user = User {
                address: samwise.clone(),
                positions: Positions::env_default(&e),
            };
            let pool_config = PoolConfig {
                oracle,
                min_collateral: 1_0000000,
                bstop_rate: 0_2000000,
                status: 0,
                max_positions: 2,
            };
            e.as_contract(&pool, || {
                storage::set_pool_config(&e, &pool_config);
                user.add_collateral(&e, &mut reserve_0, 42);
                user.add_collateral(&e, &mut reserve_1, 42);
                user.add_liabilities(&e, &mut reserve_0, 123);
                user.add_liabilities(&e, &mut reserve_1, 123);
                let prev_positions = user.positions.effective_count();

                let pool = Pool::load(&e);
                user.remove_collateral(&e, &mut reserve_1, 42);

                pool.require_under_max(&e, &user.positions, prev_positions);
            });
        }

        #[test]
        #[should_panic(expected = "Error(Contract, #1208)")]
        fn test_require_under_max_panics_if_over() {
            let e = Env::default();
            e.mock_all_auths();
            let samwise = Address::generate(&e);
            let pool = testutils::create_pool(&e);

            let mut reserve_0 = testutils::default_reserve(&e);
            let mut reserve_1 = testutils::default_reserve(&e);
            reserve_1.config.index = 1;

            let mut user = User {
                address: samwise.clone(),
                positions: Positions::env_default(&e),
            };
            let (oracle, _) = testutils::create_mock_oracle(&e);
            let pool_config = PoolConfig {
                oracle,
                min_collateral: 1_0000000,
                bstop_rate: 0_2000000,
                status: 0,
                max_positions: 2,
            };
            e.as_contract(&pool, || {
                storage::set_pool_config(&e, &pool_config);
                user.add_collateral(&e, &mut reserve_0, 123);
                user.add_liabilities(&e, &mut reserve_0, 789);
                let prev_positions = user.positions.effective_count();

                let pool = Pool::load(&e);
                user.add_liabilities(&e, &mut reserve_1, 42);

                pool.require_under_max(&e, &user.positions, prev_positions);
            });
        }
    }
}

mod pool_src_pool_interest {
    use cast::i128;

    use soroban_fixed_point_math::SorobanFixedPoint;

    use soroban_sdk::{panic_with_error, Env};

    use crate::{
        constants::{SCALAR_12, SCALAR_7, SECONDS_PER_YEAR},
        storage::ReserveConfig,
        PoolError,
    };

    pub(crate) use crate::pool::interest::*;

    mod tests {
        use super::*;
        use soroban_sdk::testutils::{Ledger, LedgerInfo};

        #[test]
        fn test_calc_accrual_util_under_target() {
            let e = Env::default();

            let reserve_config = ReserveConfig {
                decimals: 7,
                c_factor: 0_7500000,
                l_factor: 0_7500000,
                util: 0_7500000,
                max_util: 0_9500000,
                r_base: 0_0100000,
                r_one: 0_0500000,
                r_two: 0_5000000,
                r_three: 1_5000000,
                reactivity: 0_0000020,
                supply_cap: 1000000000000000000,
                index: 0,
                enabled: true,
            };
            let ir_mod: i128 = 1_0000000;

            e.ledger().set(LedgerInfo {
                timestamp: 500,
                protocol_version: 22,
                sequence_number: 100,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 3110400,
            });

            let (accrual, ir_mod) = calc_accrual(&e, &reserve_config, 0_6565656, ir_mod, 0);

            assert_eq!(accrual, 1_000_000_852_536);
            assert_eq!(ir_mod, 0_9999066);
        }

        #[test]
        fn test_calc_accrual_util_over_target() {
            let e = Env::default();

            let reserve_config = ReserveConfig {
                decimals: 7,
                c_factor: 0_7500000,
                l_factor: 0_7500000,
                util: 0_7500000,
                max_util: 0_9500000,
                r_base: 0_0100000,
                r_one: 0_0500000,
                r_two: 0_5000000,
                r_three: 1_5000000,
                reactivity: 0_0000020,
                supply_cap: 1000000000000000000,
                index: 0,
                enabled: true,
            };
            let ir_mod: i128 = 1_0000000;

            e.ledger().set(LedgerInfo {
                timestamp: 500,
                protocol_version: 22,
                sequence_number: 100,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 3110400,
            });

            let (accrual, ir_mod) = calc_accrual(&e, &reserve_config, 0_7979797, ir_mod, 0);

            assert_eq!(accrual, 1_000_002_853_078);
            assert_eq!(ir_mod, 1_0000479);
        }

        #[test]
        fn test_calc_accrual_util_over_95() {
            let e = Env::default();

            let reserve_config = ReserveConfig {
                decimals: 7,
                c_factor: 0_7500000,
                l_factor: 0_7500000,
                util: 0_7500000,
                max_util: 0_9500000,
                r_base: 0_0100000,
                r_one: 0_0500000,
                r_two: 0_5000000,
                r_three: 1_5000000,
                reactivity: 0_0000020,
                supply_cap: 1000000000000000000,
                index: 0,
                enabled: true,
            };
            let ir_mod: i128 = 1_0000000;

            e.ledger().set(LedgerInfo {
                timestamp: 500,
                protocol_version: 22,
                sequence_number: 100,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 3110400,
            });

            let (accrual, ir_mod) = calc_accrual(&e, &reserve_config, 0_9696969, ir_mod, 0);

            assert_eq!(accrual, 1_000_018_247_510);
            assert_eq!(ir_mod, 1_0002196);
        }

        #[test]
        fn test_calc_ir_mod_over_limit() {
            let e = Env::default();

            let reserve_config = ReserveConfig {
                decimals: 7,
                c_factor: 0_7500000,
                l_factor: 0_7500000,
                util: 0_7500000,
                max_util: 0_9500000,
                r_base: 0_0100000,
                r_one: 0_0500000,
                r_two: 0_5000000,
                r_three: 1_5000000,
                reactivity: 0_0000020,
                supply_cap: 1000000000000000000,
                index: 0,
                enabled: true,
            };
            let ir_mod: i128 = 9_9970000;

            e.ledger().set(LedgerInfo {
                timestamp: 12345,
                protocol_version: 22,
                sequence_number: 10000,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 3110400,
            });

            let (_accrual, ir_mod) = calc_accrual(&e, &reserve_config, 0_9696969, ir_mod, 0);

            assert_eq!(ir_mod, 10_0000000);
        }

        #[test]
        fn test_calc_ir_mod_under_limit() {
            let e = Env::default();

            let reserve_config = ReserveConfig {
                decimals: 7,
                c_factor: 0_7500000,
                l_factor: 0_7500000,
                util: 0_7500000,
                max_util: 0_9500000,
                r_base: 0_0100000,
                r_one: 0_0500000,
                r_two: 0_5000000,
                r_three: 1_5000000,
                reactivity: 0_0000020,
                supply_cap: 1000000000000000000,
                index: 0,
                enabled: true,
            };
            let ir_mod: i128 = 0_1500000;

            e.ledger().set(LedgerInfo {
                timestamp: 10000 * 5,
                protocol_version: 22,
                sequence_number: 10000,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 3110400,
            });

            let (_accrual, ir_mod) = calc_accrual(&e, &reserve_config, 0_2020202, ir_mod, 0);

            assert_eq!(ir_mod, 0_1000000);
        }

        #[test]
        fn test_calc_ir_mod_reactivity_0() {
            let e = Env::default();

            let reserve_config = ReserveConfig {
                decimals: 7,
                c_factor: 0_7500000,
                l_factor: 0_7500000,
                util: 0_7500000,
                max_util: 0_9500000,
                r_base: 0_0100000,
                r_one: 0_0500000,
                r_two: 0_5000000,
                r_three: 1_5000000,
                reactivity: 0,
                supply_cap: 1000000000000000000,
                index: 0,
                enabled: true,
            };
            let ir_mod: i128 = 1_0000000;

            e.ledger().set(LedgerInfo {
                timestamp: 500,
                protocol_version: 22,
                sequence_number: 100,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 3110400,
            });

            let (accrual, ir_mod) = calc_accrual(&e, &reserve_config, 0_6565656, ir_mod, 0);

            assert_eq!(accrual, 1_000_000_852_536);
            assert_eq!(ir_mod, 1_0000000);
        }

        #[test]
        fn test_calc_accrual_rounds_up() {
            let e = Env::default();

            let reserve_config = ReserveConfig {
                decimals: 7,
                c_factor: 0_7500000,
                l_factor: 0_7500000,
                util: 0_7500000,
                max_util: 0_9500000,
                r_base: 0_0001000,
                r_one: 0_0500000,
                r_two: 0_5000000,
                r_three: 1_5000000,
                reactivity: 0_0000020,
                supply_cap: 1000000000000000000,
                index: 0,
                enabled: true,
            };
            let ir_mod: i128 = 0_1000000;

            e.ledger().set(LedgerInfo {
                timestamp: 501,
                protocol_version: 22,
                sequence_number: 100,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 3110400,
            });

            let (accrual, ir_mod) = calc_accrual(&e, &reserve_config, 0_0000005, ir_mod, 500);

            assert_eq!(accrual, 1_000_000_000_001);
            assert_eq!(ir_mod, 0_1000000);
        }

        #[test]
        fn test_calc_accrual_fixed_rate() {
            let e = Env::default();

            let reserve_config = ReserveConfig {
                decimals: 7,
                c_factor: 0_7500000,
                l_factor: 0_7500000,
                util: 0_7500000,
                max_util: 0_9500000,
                r_base: 0_2500000,
                r_one: 0,
                r_two: 0,
                r_three: 0,
                reactivity: 0_0000020,
                supply_cap: 1000000000000000000,
                index: 0,
                enabled: true,
            };
            let ir_mod: i128 = 1_0000000;

            e.ledger().set(LedgerInfo {
                timestamp: 500,
                protocol_version: 22,
                sequence_number: 100,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 3110400,
            });

            let (accrual_0, ir_mod_0) = calc_accrual(&e, &reserve_config, 0, ir_mod, 0);
            let (accrual_1, ir_mod_1) = calc_accrual(&e, &reserve_config, 0_6565656, ir_mod, 0);
            let (accrual_2, ir_mod_2) = calc_accrual(&e, &reserve_config, 0_7565656, ir_mod, 0);
            let (accrual_3, ir_mod_3) = calc_accrual(&e, &reserve_config, 0_9565656, ir_mod, 0);

            assert_eq!(accrual_0, 1_000_003_963_724);
            assert_eq!(ir_mod_0, 0_9992500);
            assert_eq!(accrual_1, 1_000_003_963_724);
            assert_eq!(ir_mod_1, 0_9999066);
            assert_eq!(accrual_2, 1_000_003_963_724);
            assert_eq!(ir_mod_2, 1_0000065);
            assert_eq!(accrual_3, 1_000_003_963_724);
            assert_eq!(ir_mod_3, 1_0002065);
        }
    }
}

mod pool_src_pool_health_factor {
    use soroban_fixed_point_math::SorobanFixedPoint;

    use soroban_sdk::Env;

    use crate::{constants::SCALAR_7, storage};

    use crate::pool::{pool::Pool, Positions};

    pub(crate) use crate::pool::health_factor::*;

    mod tests {
        use super::*;
        use crate::{storage::PoolConfig, testutils};
        use sep_40_oracle::testutils::Asset;
        use soroban_sdk::{
            map,
            testutils::{Address as _, Ledger, LedgerInfo},
            vec, Address, Symbol,
        };

        #[test]
        fn test_calculate_from_positions() {
            let e = Env::default();
            e.cost_estimate().budget().reset_unlimited();
            e.mock_all_auths();

            let bombadil = Address::generate(&e);
            let pool = testutils::create_pool(&e);
            let (oracle, oracle_client) = testutils::create_mock_oracle(&e);

            let (underlying_0, _) = testutils::create_token_contract(&e, &bombadil);
            let (reserve_config, reserve_data) = testutils::default_reserve_meta();
            testutils::create_reserve(&e, &pool, &underlying_0, &reserve_config, &reserve_data);

            let (underlying_1, _) = testutils::create_token_contract(&e, &bombadil);
            let (mut reserve_config, mut reserve_data) = testutils::default_reserve_meta();
            reserve_config.decimals = 9;
            reserve_config.c_factor = 0_8500000;
            reserve_config.l_factor = 0_8000000;
            reserve_data.b_supply = 100_000_000_000;
            reserve_data.d_supply = 70_000_000_000;
            reserve_data.b_rate = 1_100_000_000_000;
            reserve_data.d_rate = 1_150_000_000_000;
            reserve_config.index = 1;
            testutils::create_reserve(&e, &pool, &underlying_1, &reserve_config, &reserve_data);

            let (underlying_2, _) = testutils::create_token_contract(&e, &bombadil);
            let (mut reserve_config, mut reserve_data) = testutils::default_reserve_meta();
            reserve_config.decimals = 6;
            reserve_config.index = 2;
            reserve_data.b_supply = 10_000_000;
            reserve_data.d_supply = 5_000_000;
            reserve_data.b_rate = 1_001_100_000_000;
            reserve_data.d_rate = 1_001_200_000_000;
            testutils::create_reserve(&e, &pool, &underlying_2, &reserve_config, &reserve_data);

            oracle_client.set_data(
                &bombadil,
                &Asset::Other(Symbol::new(&e, "USD")),
                &vec![
                    &e,
                    Asset::Stellar(underlying_0),
                    Asset::Stellar(underlying_1),
                    Asset::Stellar(underlying_2),
                ],
                &7,
                &300,
            );
            oracle_client.set_price_stable(&vec![&e, 1_0000000, 2_5000000, 1000_0000000]);

            e.ledger().set(LedgerInfo {
                timestamp: 0,
                protocol_version: 22,
                sequence_number: 1234,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 3110400,
            });
            let pool_config = PoolConfig {
                oracle,
                min_collateral: 1_0000000,
                bstop_rate: 0_2000000,
                status: 0,
                max_positions: 5,
            };

            let positions = Positions {
                liabilities: map![&e, (0, 1_5000000), (1, 50_987_654_321)],
                collateral: map![&e, (0, 100_1234567), (2, 0_250_000)],
                supply: map![&e, (1, 120_987_654_321)],
            };
            e.as_contract(&pool, || {
                storage::set_pool_config(&e, &pool_config);
                let mut pool = Pool::load(&e);
                let position_data =
                    PositionData::calculate_from_positions(&e, &mut pool, &positions);
                assert_eq!(position_data.collateral_base, 262_7985925);
                assert_eq!(position_data.liability_base, 185_2368828);
                assert_eq!(position_data.collateral_raw, 350_3984567);
                assert_eq!(position_data.liability_raw, 148_0895062);
                assert_eq!(position_data.scalar, SCALAR_7);
            });
        }

        #[test]
        fn test_as_health_factor_rounds_floor() {
            let e = Env::default();
            let position_data = PositionData {
                collateral_base: 9_1234567,
                collateral_raw: 0,
                liability_base: 9_1000000,
                liability_raw: 0,
                scalar: 1_0000000,
            };

            // actual: 1.002577659
            let result = position_data.as_health_factor(&e);
            assert_eq!(result, 1_0025776);
        }

        #[test]
        fn test_is_hf_under() {
            let e = Env::default();

            let position_data = PositionData {
                collateral_base: 9_1234567,
                collateral_raw: 12_0000000,
                liability_base: 9_1233333,
                liability_raw: 10_0000000,
                scalar: 1_0000000,
            };

            let result = position_data.is_hf_under(&e, 1_0000100);
            // no panic
            assert_eq!(result, false);
        }

        #[test]
        fn test_is_hf_under_odd_scalar() {
            let e = Env::default();

            let position_data = PositionData {
                collateral_base: 9_12345,
                collateral_raw: 12_00000,
                liability_base: 9_12333,
                liability_raw: 10_00000,
                scalar: 1_00000,
            };

            let result = position_data.is_hf_under(&e, 1_0000100);
            // no panic
            assert_eq!(result, false);
        }

        #[test]
        fn test_is_hf_under_no_liabilites() {
            let e = Env::default();

            let position_data = PositionData {
                collateral_base: 9_1234567,
                collateral_raw: 12_0000000,
                liability_base: 0,
                liability_raw: 0,
                scalar: 1_0000000,
            };

            let result = position_data.is_hf_under(&e, 1_0000100);
            // no panic
            assert_eq!(result, false);
        }

        #[test]
        fn test_is_hf_under_true() {
            let e = Env::default();

            let position_data = PositionData {
                collateral_base: 9_1234567,
                collateral_raw: 12_0000000,
                liability_base: 9_1234567,
                liability_raw: 10_0000000,
                scalar: 1_0000000,
            };

            let result = position_data.is_hf_under(&e, 1_0000100);
            // panic
            assert!(result);
        }

        #[test]
        fn test_is_hf_over() {
            let e = Env::default();

            let position_data = PositionData {
                collateral_base: 9_1234567,
                collateral_raw: 12_0000000,
                liability_base: 9_1233333,
                liability_raw: 10_0000000,
                scalar: 1_0000000,
            };

            let result = position_data.is_hf_over(&e, 1_1000000);
            // no panic
            assert_eq!(result, false);
        }

        #[test]
        fn test_is_hf_over_odd_scalar() {
            let e = Env::default();

            let position_data = PositionData {
                collateral_base: 9_1234567_000,
                collateral_raw: 12_0000000_000,
                liability_base: 9_1233333_000,
                liability_raw: 10_0000000_000,
                scalar: 1_0000000_000,
            };

            let result = position_data.is_hf_over(&e, 1_1000000);
            // no panic
            assert_eq!(result, false);
        }

        #[test]
        fn test_is_hf_over_no_liabilites() {
            let e = Env::default();

            let position_data = PositionData {
                collateral_base: 9_1234567,
                collateral_raw: 12_0000000,
                liability_base: 0,
                liability_raw: 0,
                scalar: 1_0000000,
            };

            let result = position_data.is_hf_over(&e, 1_0000100);
            // panic
            assert!(result);
        }
        #[test]
        fn test_is_hf_over_true() {
            let e = Env::default();

            let position_data = PositionData {
                collateral_base: 19_1234567,
                collateral_raw: 22_0000000,
                liability_base: 9_1234567,
                liability_raw: 10_0000000,
                scalar: 1_0000000,
            };

            let result = position_data.is_hf_over(&e, 1_0000100);
            // panic
            assert!(result);
        }
    }
}

mod pool_src_pool_gulp {
    use sep_41_token::TokenClient;

    use soroban_sdk::{Address, Env};

    use crate::pool::{Pool, RequestType, Reserve};

    pub(crate) use crate::pool::gulp::*;

    mod tests {
        use crate::constants::SCALAR_7;
        use crate::pool::execute_gulp;
        use crate::storage::{self, PoolConfig};
        use crate::testutils;
        use soroban_sdk::{
            testutils::{Address as _, Ledger, LedgerInfo},
            Address, Env,
        };

        #[test]
        fn test_execute_gulp() {
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
            let (oracle, _) = testutils::create_mock_oracle(&e);

            let initial_backstop_credit = 500;
            let (underlying, underlying_client) = testutils::create_token_contract(&e, &bombadil);
            let (reserve_config, mut reserve_data) = testutils::default_reserve_meta();
            reserve_data.b_rate = 1_000_000_000_000;
            reserve_data.d_rate = 1_000_000_000_000;
            reserve_data.d_supply = 500 * SCALAR_7;
            reserve_data.b_supply = 1000 * SCALAR_7;
            reserve_data.backstop_credit = initial_backstop_credit;
            reserve_data.last_time = 100;
            testutils::create_reserve(&e, &pool, &underlying, &reserve_config, &reserve_data);

            let additional_tokens = 10 * SCALAR_7;
            underlying_client.mint(&pool, &additional_tokens);
            e.as_contract(&pool, || {
                let pool_config = PoolConfig {
                    oracle,
                    min_collateral: 1_0000000,
                    bstop_rate: 0_1000000,
                    status: 1,
                    max_positions: 4,
                };
                storage::set_pool_config(&e, &pool_config);

                let token_delta_result = execute_gulp(&e, &underlying);
                assert_eq!(token_delta_result, additional_tokens);

                let new_reserve_data = storage::get_res_data(&e, &underlying);
                assert_eq!(new_reserve_data.last_time, 100);
                assert_eq!(
                    new_reserve_data.backstop_credit,
                    additional_tokens + initial_backstop_credit
                );
            });
        }

        #[test]
        fn test_execute_gulp_accrues_interest_before_gulp() {
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
            let (oracle, _) = testutils::create_mock_oracle(&e);

            let initial_backstop_credit = 500;
            let (underlying, underlying_client) = testutils::create_token_contract(&e, &bombadil);
            let (reserve_config, mut reserve_data) = testutils::default_reserve_meta();
            reserve_data.b_rate = 1_000_000_000_000;
            reserve_data.d_rate = 1_000_000_000_000;
            reserve_data.d_supply = 500 * SCALAR_7;
            reserve_data.b_supply = 1000 * SCALAR_7;
            reserve_data.backstop_credit = initial_backstop_credit;
            reserve_data.last_time = 0;
            testutils::create_reserve(&e, &pool, &underlying, &reserve_config, &reserve_data);

            let additional_tokens = 10 * SCALAR_7;
            underlying_client.mint(&pool, &additional_tokens);
            e.as_contract(&pool, || {
                let pool_config = PoolConfig {
                    oracle,
                    min_collateral: 1_0000000,
                    bstop_rate: 0_1000000,
                    status: 0,
                    max_positions: 4,
                };
                storage::set_pool_config(&e, &pool_config);

                let token_delta_result = execute_gulp(&e, &underlying);
                assert_eq!(token_delta_result, additional_tokens);

                let new_reserve_data = storage::get_res_data(&e, &underlying);
                assert_eq!(new_reserve_data.b_rate, 1_000_000_000_000 + 62000);
                assert_eq!(new_reserve_data.last_time, 100);
                // 68 is the backstop credit due to the interest accrued
                assert_eq!(
                    new_reserve_data.backstop_credit,
                    additional_tokens + initial_backstop_credit + 68
                );
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
            let (oracle, _) = testutils::create_mock_oracle(&e);

            let (underlying, _) = testutils::create_token_contract(&e, &bombadil);
            let (reserve_config, mut reserve_data) = testutils::default_reserve_meta();
            reserve_data.b_rate = 1_000_000_000_000;
            reserve_data.d_rate = 1_000_000_000_000;
            reserve_data.d_supply = 500 * SCALAR_7;
            reserve_data.b_supply = 1000 * SCALAR_7;
            reserve_data.backstop_credit = 0;
            reserve_data.last_time = 0;
            testutils::create_reserve(&e, &pool, &underlying, &reserve_config, &reserve_data);

            e.as_contract(&pool, || {
                let pool_config = PoolConfig {
                    oracle,
                    min_collateral: 1_0000000,
                    bstop_rate: 0_1000000,
                    status: 0,
                    max_positions: 4,
                };
                storage::set_pool_config(&e, &pool_config);

                let token_delta_result = execute_gulp(&e, &underlying);
                assert_eq!(token_delta_result, 0);

                // data not set
                let new_reserve_data = storage::get_res_data(&e, &underlying);
                assert_eq!(new_reserve_data.b_rate, 1_000_000_000_000);
                assert_eq!(new_reserve_data.last_time, 0);
                assert_eq!(new_reserve_data.backstop_credit, 0);
            });
        }

        #[test]
        fn test_execute_gulp_negative_delta_skips() {
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
            let (oracle, _) = testutils::create_mock_oracle(&e);

            let (underlying, underlying_client) = testutils::create_token_contract(&e, &bombadil);
            let (reserve_config, mut reserve_data) = testutils::default_reserve_meta();
            reserve_data.b_rate = 1_000_000_000_000;
            reserve_data.d_rate = 1_000_000_000_000;
            reserve_data.d_supply = 500 * SCALAR_7;
            reserve_data.b_supply = 1000 * SCALAR_7;
            reserve_data.backstop_credit = 0;
            reserve_data.last_time = 0;
            testutils::create_reserve(&e, &pool, &underlying, &reserve_config, &reserve_data);

            underlying_client.burn(&pool, &SCALAR_7);
            e.as_contract(&pool, || {
                let pool_config = PoolConfig {
                    oracle,
                    min_collateral: 1_0000000,
                    bstop_rate: 0_1000000,
                    status: 0,
                    max_positions: 4,
                };
                storage::set_pool_config(&e, &pool_config);

                let token_delta_result = execute_gulp(&e, &underlying);
                assert_eq!(token_delta_result, 0);

                // data not set
                let new_reserve_data = storage::get_res_data(&e, &underlying);
                assert_eq!(new_reserve_data.b_rate, 1_000_000_000_000);
                assert_eq!(new_reserve_data.last_time, 0);
                assert_eq!(new_reserve_data.backstop_credit, 0);
            });
        }

        #[test]
        #[should_panic(expected = "Error(Contract, #1206)")]
        fn test_execute_gulp_checks_status() {
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
            let (oracle, _) = testutils::create_mock_oracle(&e);

            let initial_backstop_credit = 500;
            let (underlying, underlying_client) = testutils::create_token_contract(&e, &bombadil);
            let (reserve_config, mut reserve_data) = testutils::default_reserve_meta();
            reserve_data.b_rate = 1_000_000_000_000;
            reserve_data.d_rate = 1_000_000_000_000;
            reserve_data.d_supply = 500 * SCALAR_7;
            reserve_data.b_supply = 1000 * SCALAR_7;
            reserve_data.backstop_credit = initial_backstop_credit;
            reserve_data.last_time = 100;
            testutils::create_reserve(&e, &pool, &underlying, &reserve_config, &reserve_data);

            let additional_tokens = 10 * SCALAR_7;
            underlying_client.mint(&pool, &additional_tokens);
            e.as_contract(&pool, || {
                let pool_config = PoolConfig {
                    oracle,
                    min_collateral: 1_0000000,
                    bstop_rate: 0_1000000,
                    status: 2,
                    max_positions: 4,
                };
                storage::set_pool_config(&e, &pool_config);

                execute_gulp(&e, &underlying);
            });
        }
    }
}

mod pool_src_pool_config {
    use crate::{
        constants::{MAX_RESERVES, SCALAR_12, SCALAR_7, SECONDS_PER_WEEK},
        errors::PoolError,
        storage::{
            self, has_queued_reserve_set, PoolConfig, QueuedReserveInit, ReserveConfig, ReserveData,
        },
    };

    use soroban_sdk::{panic_with_error, Address, Env, String};

    use crate::pool::{pool::Pool, Reserve};

    pub(crate) use crate::pool::config::*;

    mod tests {
        use crate::storage::QueuedReserveInit;
        use crate::testutils;

        use super::*;
        use soroban_sdk::testutils::{Address as _, Ledger, LedgerInfo};

        #[test]
        fn test_execute_initialize() {
            let e = Env::default();
            e.mock_all_auths();
            let pool = testutils::create_pool(&e);

            let admin = Address::generate(&e);
            let name = String::from_str(&e, "pool_name");
            let oracle = Address::generate(&e);
            let bstop_rate: u32 = 0_1000000;
            let max_positions = 2;
            let min_collateral = 1_0000000;
            let backstop_address = Address::generate(&e);
            let blnd_id = Address::generate(&e);

            e.as_contract(&pool, || {
                execute_initialize(
                    &e,
                    &admin,
                    &name,
                    &oracle,
                    &bstop_rate,
                    &max_positions,
                    &min_collateral,
                    &backstop_address,
                    &blnd_id,
                );

                assert_eq!(storage::get_admin(&e), admin);
                let pool_config = storage::get_pool_config(&e);
                assert_eq!(pool_config.oracle, oracle);
                assert_eq!(pool_config.bstop_rate, bstop_rate);
                assert_eq!(pool_config.min_collateral, min_collateral);
                assert_eq!(pool_config.max_positions, max_positions);
                assert_eq!(pool_config.status, 6);
                assert_eq!(storage::get_backstop(&e), backstop_address);
                assert_eq!(storage::get_blnd_token(&e), blnd_id);
            });
        }

        #[test]
        #[should_panic(expected = "Error(Contract, #1201)")]
        fn test_execute_initialize_bad_take_rate() {
            let e = Env::default();
            e.mock_all_auths();
            let pool = testutils::create_pool(&e);

            let admin = Address::generate(&e);
            let name = String::from_str(&e, "pool_name");
            let oracle = Address::generate(&e);
            let bstop_rate = 1_0000000;
            let max_positions = 3;
            let min_collateral = 1_0000000;
            let backstop_address = Address::generate(&e);
            let blnd_id = Address::generate(&e);

            e.as_contract(&pool, || {
                execute_initialize(
                    &e,
                    &admin,
                    &name,
                    &oracle,
                    &bstop_rate,
                    &max_positions,
                    &min_collateral,
                    &backstop_address,
                    &blnd_id,
                );
            });
        }

        #[test]
        #[should_panic(expected = "Error(Contract, #1201)")]
        fn test_execute_initialize_bad_max_positions() {
            let e = Env::default();
            e.mock_all_auths();
            let pool = testutils::create_pool(&e);

            let admin = Address::generate(&e);
            let name = String::from_str(&e, "pool_name");
            let oracle = Address::generate(&e);
            let bstop_rate = 0_1000000;
            let max_positions = 1;
            let min_collateral = 1_0000000;
            let backstop_address = Address::generate(&e);
            let blnd_id = Address::generate(&e);

            e.as_contract(&pool, || {
                execute_initialize(
                    &e,
                    &admin,
                    &name,
                    &oracle,
                    &bstop_rate,
                    &max_positions,
                    &min_collateral,
                    &backstop_address,
                    &blnd_id,
                );
            });
        }

        #[test]
        fn test_execute_update_pool() {
            let e = Env::default();
            e.mock_all_auths();
            let pool = testutils::create_pool(&e);

            let pool_config = PoolConfig {
                oracle: Address::generate(&e),
                min_collateral: 1_0000000,
                bstop_rate: 0_1000000,
                status: 0,
                max_positions: 2,
            };
            e.as_contract(&pool, || {
                storage::set_pool_config(&e, &pool_config);

                // happy path
                execute_update_pool(&e, 0_2000000, 4u32, 2_0000000);
                let new_pool_config = storage::get_pool_config(&e);
                assert_eq!(new_pool_config.bstop_rate, 0_2000000);
                assert_eq!(new_pool_config.oracle, pool_config.oracle);
                assert_eq!(new_pool_config.status, pool_config.status);
                assert_eq!(new_pool_config.max_positions, 4u32);
                assert_eq!(new_pool_config.min_collateral, 2_0000000);
            });
        }

        #[test]
        fn test_execute_update_pool_updates_reserves_if_backstop_rate_changes() {
            let e = Env::default();
            e.mock_all_auths();

            e.ledger().set(LedgerInfo {
                timestamp: 12345,
                protocol_version: 22,
                sequence_number: 123456,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 3110400,
            });

            let bombadil = Address::generate(&e);
            let pool = testutils::create_pool(&e);

            let (underlying_0, _) = testutils::create_token_contract(&e, &bombadil);
            let (mut reserve_config_0, mut reserve_data_0) = testutils::default_reserve_meta();
            reserve_data_0.last_time = 12345;
            reserve_data_0.b_supply = 1000_0000000;
            reserve_data_0.d_supply = 750_0000000;
            reserve_config_0.index = 0;
            testutils::create_reserve(&e, &pool, &underlying_0, &reserve_config_0, &reserve_data_0);

            let (underlying_1, _) = testutils::create_token_contract(&e, &bombadil);
            let (mut reserve_config_1, mut reserve_data_1) = testutils::default_reserve_meta();
            reserve_data_1.last_time = 12345;
            reserve_data_1.b_supply = 250_0000000;
            reserve_data_1.d_supply = 100_5000000;
            reserve_config_1.index = 1;
            testutils::create_reserve(&e, &pool, &underlying_1, &reserve_config_1, &reserve_data_1);

            let pool_config = PoolConfig {
                oracle: Address::generate(&e),
                min_collateral: 1_0000000,
                bstop_rate: 0_1000000,
                status: 0,
                max_positions: 2,
            };

            e.ledger().set(LedgerInfo {
                timestamp: 12345 * 5,
                protocol_version: 22,
                sequence_number: 123456,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 3110400,
            });
            e.as_contract(&pool, || {
                storage::set_pool_config(&e, &pool_config);

                execute_update_pool(&e, 0_2000000, 4u32, 2_0000000);

                let new_pool_config = storage::get_pool_config(&e);
                assert_eq!(new_pool_config.bstop_rate, 0_2000000);
                assert_eq!(new_pool_config.oracle, pool_config.oracle);
                assert_eq!(new_pool_config.status, pool_config.status);
                assert_eq!(new_pool_config.max_positions, 4u32);
                assert_eq!(new_pool_config.min_collateral, 2_0000000);

                let new_reserve_data_0 = storage::get_res_data(&e, &underlying_0);
                assert_eq!(new_reserve_data_0.last_time, 12345 * 5);
                assert!(new_reserve_data_0.d_rate > reserve_data_0.d_rate);
                assert!(new_reserve_data_0.b_rate > reserve_data_0.b_rate);
                let new_reserve_data_1 = storage::get_res_data(&e, &underlying_1);
                assert_eq!(new_reserve_data_1.last_time, 12345 * 5);
                assert!(new_reserve_data_1.d_rate > reserve_data_1.d_rate);
                assert!(new_reserve_data_1.b_rate > reserve_data_1.b_rate);
            });
        }

        #[test]
        #[should_panic(expected = "Error(Contract, #1201)")]
        fn test_execute_update_pool_validates_b_stop_rate() {
            let e = Env::default();
            e.mock_all_auths();
            let pool = testutils::create_pool(&e);

            let pool_config = PoolConfig {
                oracle: Address::generate(&e),
                min_collateral: 1_0000000,
                bstop_rate: 0_1000000,
                status: 0,
                max_positions: 2,
            };
            e.as_contract(&pool, || {
                storage::set_pool_config(&e, &pool_config);

                execute_update_pool(&e, 1_0000000, 4u32, 1_0000000);
            });
        }

        #[test]
        #[should_panic(expected = "Error(Contract, #1201)")]
        fn test_execute_update_pool_validates_min_collateral() {
            let e = Env::default();
            e.mock_all_auths();
            let pool = testutils::create_pool(&e);

            let pool_config = PoolConfig {
                oracle: Address::generate(&e),
                min_collateral: 1_0000000,
                bstop_rate: 0_1000000,
                status: 0,
                max_positions: 2,
            };
            e.as_contract(&pool, || {
                storage::set_pool_config(&e, &pool_config);

                execute_update_pool(&e, 0_2000000, 4u32, -1);
            });
        }

        #[test]
        #[should_panic(expected = "Error(Contract, #1201)")]
        fn test_execute_update_pool_validates_max_positions() {
            let e = Env::default();
            e.mock_all_auths();
            let pool = testutils::create_pool(&e);

            let pool_config = PoolConfig {
                oracle: Address::generate(&e),
                min_collateral: 1_0000000,
                bstop_rate: 0_1000000,
                status: 0,
                max_positions: 2,
            };
            e.as_contract(&pool, || {
                storage::set_pool_config(&e, &pool_config);

                execute_update_pool(&e, 0_2000000, 1 + 2 * MAX_RESERVES, 2_0000000);
            });
        }

        #[test]
        fn test_queue_set_reserve_status_6() {
            let e = Env::default();
            e.mock_all_auths();
            let pool = testutils::create_pool(&e);
            let bombadil = Address::generate(&e);

            let (asset_id_0, _) = testutils::create_token_contract(&e, &bombadil);

            let metadata = ReserveConfig {
                index: 0,
                decimals: 7,
                c_factor: 0_7500000,
                l_factor: 0_7500000,
                util: 0_5000000,
                max_util: 0_9500000,
                r_base: 0_0100000,
                r_one: 0_0500000,
                r_two: 0_5000000,
                r_three: 1_5000000,
                reactivity: 100,
                supply_cap: 1000000000000000000,
                enabled: true,
            };
            let pool_config = PoolConfig {
                oracle: Address::generate(&e),
                min_collateral: 1_0000000,
                bstop_rate: 0_1000000,
                status: 6,
                max_positions: 2,
            };
            e.as_contract(&pool, || {
                storage::set_pool_config(&e, &pool_config);
                execute_queue_set_reserve(&e, &asset_id_0, &metadata);
                let queued_res = storage::get_queued_reserve_set(&e, &asset_id_0);
                let res_config_0 = queued_res.new_config;
                assert_eq!(res_config_0.decimals, metadata.decimals);
                assert_eq!(res_config_0.c_factor, metadata.c_factor);
                assert_eq!(res_config_0.l_factor, metadata.l_factor);
                assert_eq!(res_config_0.util, metadata.util);
                assert_eq!(res_config_0.r_base, metadata.r_base);
                assert_eq!(res_config_0.r_one, metadata.r_one);
                assert_eq!(res_config_0.r_one, metadata.r_one);
                assert_eq!(res_config_0.r_two, metadata.r_two);
                assert_eq!(res_config_0.r_three, metadata.r_three);
                assert_eq!(res_config_0.reactivity, metadata.reactivity);
                assert_eq!(res_config_0.index, 0);
                assert_eq!(queued_res.unlock_time, e.ledger().timestamp());
            });
        }

        #[test]
        fn test_queue_set_reserve() {
            let e = Env::default();
            e.mock_all_auths();
            let pool = testutils::create_pool(&e);
            let bombadil = Address::generate(&e);

            let (asset_id_0, _) = testutils::create_token_contract(&e, &bombadil);

            let metadata = ReserveConfig {
                index: 0,
                decimals: 7,
                c_factor: 0_7500000,
                l_factor: 0_7500000,
                util: 0_5000000,
                max_util: 0_9500000,
                r_base: 0_0100000,
                r_one: 0_0500000,
                r_two: 0_5000000,
                r_three: 1_5000000,
                reactivity: 100,
                supply_cap: 1000000000000000000,
                enabled: true,
            };
            let pool_config = PoolConfig {
                oracle: Address::generate(&e),
                min_collateral: 1_0000000,
                bstop_rate: 0_1000000,
                status: 0,
                max_positions: 2,
            };
            e.as_contract(&pool, || {
                storage::set_pool_config(&e, &pool_config);
                execute_queue_set_reserve(&e, &asset_id_0, &metadata);
                let queued_init = storage::get_queued_reserve_set(&e, &asset_id_0);
                assert_eq!(queued_init.new_config.decimals, metadata.decimals);
                assert_eq!(queued_init.new_config.c_factor, metadata.c_factor);
                assert_eq!(queued_init.new_config.l_factor, metadata.l_factor);
                assert_eq!(queued_init.new_config.util, metadata.util);
                assert_eq!(queued_init.new_config.max_util, metadata.max_util);
                assert_eq!(queued_init.new_config.r_base, metadata.r_base);
                assert_eq!(queued_init.new_config.r_one, metadata.r_one);
                assert_eq!(queued_init.new_config.r_two, metadata.r_two);
                assert_eq!(queued_init.new_config.r_three, metadata.r_three);
                assert_eq!(queued_init.new_config.reactivity, metadata.reactivity);
                assert_eq!(queued_init.new_config.index, 0);
                assert_eq!(
                    queued_init.unlock_time,
                    e.ledger().timestamp() + SECONDS_PER_WEEK
                );
            });
        }

        #[test]
        #[should_panic(expected = "Error(Contract, #1200)")]
        fn test_queue_set_reserve_duplicate() {
            let e = Env::default();
            e.mock_all_auths();
            let pool = testutils::create_pool(&e);
            let bombadil = Address::generate(&e);

            let (asset_id_0, _) = testutils::create_token_contract(&e, &bombadil);

            let metadata = ReserveConfig {
                index: 0,
                decimals: 7,
                c_factor: 0_7500000,
                l_factor: 0_7500000,
                util: 0_5000000,
                max_util: 0_9500000,
                r_base: 0_0100000,
                r_one: 0_0500000,
                r_two: 0_5000000,
                r_three: 1_5000000,
                reactivity: 100,
                supply_cap: 1000000000000000000,
                enabled: true,
            };
            let pool_config = PoolConfig {
                oracle: Address::generate(&e),
                min_collateral: 1_0000000,
                bstop_rate: 0_1000000,
                status: 6,
                max_positions: 2,
            };
            e.as_contract(&pool, || {
                storage::set_pool_config(&e, &pool_config);
                execute_queue_set_reserve(&e, &asset_id_0, &metadata);
                let queued_res = storage::get_queued_reserve_set(&e, &asset_id_0);
                let res_config_0 = queued_res.new_config;
                assert_eq!(res_config_0.index, 0);

                // try and queue the same reserve
                execute_queue_set_reserve(&e, &asset_id_0, &metadata);
            });
        }

        #[test]
        #[should_panic(expected = "Error(Contract, #1202)")]
        fn test_queue_set_reserve_validates_metadata() {
            let e = Env::default();
            e.mock_all_auths();
            let pool = testutils::create_pool(&e);
            let bombadil = Address::generate(&e);
            let (asset_id, _) = testutils::create_token_contract(&e, &bombadil);

            let metadata = ReserveConfig {
                index: 0,
                decimals: 7,
                c_factor: 0_7500000,
                l_factor: 1_7500000,
                util: 1_0000000,
                max_util: 0_9500000,
                r_base: 0_0100000,
                r_one: 0_0500000,
                r_two: 0_5000000,
                r_three: 1_5000000,
                reactivity: 100,
                supply_cap: 1000000000000000000,
                enabled: true,
            };
            let pool_config = PoolConfig {
                oracle: Address::generate(&e),
                min_collateral: 1_0000000,
                bstop_rate: 0_1000000,
                status: 0,
                max_positions: 2,
            };
            e.as_contract(&pool, || {
                storage::set_pool_config(&e, &pool_config);
                execute_queue_set_reserve(&e, &asset_id, &metadata);
            });
        }

        #[test]
        fn test_queue_set_reserve_with_existing_res() {
            let e = Env::default();
            e.mock_all_auths();
            let pool = testutils::create_pool(&e);
            let bombadil = Address::generate(&e);

            let (asset_id_0, _) = testutils::create_token_contract(&e, &bombadil);

            let old_metadata = ReserveConfig {
                index: 1,
                decimals: 7,
                c_factor: 0_7500000,
                l_factor: 0_7500000,
                util: 0_5000000,
                max_util: 0_9500000,
                r_base: 0_0100000,
                r_one: 0_0500000,
                r_two: 0_5000000,
                r_three: 1_5000000,
                reactivity: 100,
                supply_cap: 1000000000000000000,
                enabled: true,
            };
            let metadata = ReserveConfig {
                index: 1,
                decimals: 7,
                c_factor: 0_6000000,
                l_factor: 0_5000000,
                util: 0_4000000,
                max_util: 0_9500000,
                r_base: 0_0100000,
                r_one: 0_0500000,
                r_two: 0_5000000,
                r_three: 1_5000000,
                reactivity: 100,
                supply_cap: 1000000000000000000,
                enabled: true,
            };
            let pool_config = PoolConfig {
                oracle: Address::generate(&e),
                min_collateral: 1_0000000,
                bstop_rate: 0_1000000,
                status: 5,
                max_positions: 2,
            };
            e.as_contract(&pool, || {
                storage::set_pool_config(&e, &pool_config);
                storage::set_res_config(&e, &asset_id_0, &old_metadata);
                execute_queue_set_reserve(&e, &asset_id_0, &metadata);
                let queued_init = storage::get_queued_reserve_set(&e, &asset_id_0);
                assert_eq!(queued_init.new_config.decimals, metadata.decimals);
                assert_eq!(queued_init.new_config.c_factor, metadata.c_factor);
                assert_eq!(queued_init.new_config.l_factor, metadata.l_factor);
                assert_eq!(queued_init.new_config.util, metadata.util);
                assert_eq!(queued_init.new_config.max_util, metadata.max_util);
                assert_eq!(queued_init.new_config.r_base, metadata.r_base);
                assert_eq!(queued_init.new_config.r_one, metadata.r_one);
                assert_eq!(queued_init.new_config.r_two, metadata.r_two);
                assert_eq!(queued_init.new_config.r_three, metadata.r_three);
                assert_eq!(queued_init.new_config.reactivity, metadata.reactivity);
                assert_eq!(queued_init.new_config.index, 1);
                assert_eq!(
                    queued_init.unlock_time,
                    e.ledger().timestamp() + SECONDS_PER_WEEK
                );
            });
        }

        #[test]
        #[should_panic(expected = "Error(Contract, #1202)")]
        fn test_queue_set_reserve_decimals_changed() {
            let e = Env::default();
            e.mock_all_auths();
            let pool = testutils::create_pool(&e);
            let bombadil = Address::generate(&e);

            let (asset_id_0, _) = testutils::create_token_contract(&e, &bombadil);

            let old_metadata = ReserveConfig {
                index: 0,
                decimals: 7,
                c_factor: 0_7500000,
                l_factor: 0_7500000,
                util: 0_5000000,
                max_util: 0_9500000,
                r_base: 0_0100000,
                r_one: 0_0500000,
                r_two: 0_5000000,
                r_three: 1_5000000,
                reactivity: 100,
                supply_cap: 1000000000000000000,
                enabled: true,
            };
            let metadata = ReserveConfig {
                index: 0,
                decimals: 8,
                c_factor: 0_7500000,
                l_factor: 0_7500000,
                util: 0_5000000,
                max_util: 0_9500000,
                r_base: 0_0100000,
                r_one: 0_0500000,
                r_two: 0_5000000,
                r_three: 1_5000000,
                reactivity: 100,
                supply_cap: 1000000000000000000,
                enabled: true,
            };
            let pool_config = PoolConfig {
                oracle: Address::generate(&e),
                min_collateral: 1_0000000,
                bstop_rate: 0_1000000,
                status: 6,
                max_positions: 2,
            };
            e.as_contract(&pool, || {
                storage::set_pool_config(&e, &pool_config);
                storage::set_res_config(&e, &asset_id_0, &old_metadata);
                execute_queue_set_reserve(&e, &asset_id_0, &metadata);
            });
        }

        #[test]
        #[should_panic(expected = "Error(Contract, #1202)")]
        fn test_queue_set_reserve_lf_removed() {
            let e = Env::default();
            e.mock_all_auths();
            let pool = testutils::create_pool(&e);
            let bombadil = Address::generate(&e);

            let (asset_id_0, _) = testutils::create_token_contract(&e, &bombadil);

            let old_metadata = ReserveConfig {
                index: 0,
                decimals: 7,
                c_factor: 0_7500000,
                l_factor: 0_7500000,
                util: 0_5000000,
                max_util: 0_9500000,
                r_base: 0_0100000,
                r_one: 0_0500000,
                r_two: 0_5000000,
                r_three: 1_5000000,
                reactivity: 100,
                supply_cap: 1000000000000000000,
                enabled: true,
            };
            let metadata = ReserveConfig {
                index: 0,
                decimals: 7,
                c_factor: 0_7500000,
                l_factor: 0,
                util: 0_5000000,
                max_util: 0_9500000,
                r_base: 0_0100000,
                r_one: 0_0500000,
                r_two: 0_5000000,
                r_three: 1_5000000,
                reactivity: 100,
                supply_cap: 1000000000000000000,
                enabled: true,
            };
            let pool_config = PoolConfig {
                oracle: Address::generate(&e),
                min_collateral: 1_0000000,
                bstop_rate: 0_1000000,
                status: 6,
                max_positions: 2,
            };
            e.as_contract(&pool, || {
                storage::set_pool_config(&e, &pool_config);
                storage::set_res_config(&e, &asset_id_0, &old_metadata);
                execute_queue_set_reserve(&e, &asset_id_0, &metadata);
            });
        }

        #[test]
        fn test_execute_cancel_queued_reserve_initialization() {
            let e = Env::default();
            e.mock_all_auths();
            let pool = testutils::create_pool(&e);
            let bombadil = Address::generate(&e);

            let (asset_id_0, _) = testutils::create_token_contract(&e, &bombadil);

            let metadata = ReserveConfig {
                index: 0,
                decimals: 7,
                c_factor: 0_7500000,
                l_factor: 0_7500000,
                util: 0_5000000,
                max_util: 0_9500000,
                r_base: 0_0100000,
                r_one: 0_0500000,
                r_two: 0_5000000,
                r_three: 1_5000000,
                reactivity: 100,
                supply_cap: 1000000000000000000,
                enabled: true,
            };
            e.as_contract(&pool, || {
                storage::set_queued_reserve_set(
                    &e,
                    &QueuedReserveInit {
                        new_config: metadata.clone(),
                        unlock_time: e.ledger().timestamp(),
                    },
                    &asset_id_0,
                );
                execute_cancel_queued_set_reserve(&e, &asset_id_0);
                let result = storage::has_queued_reserve_set(&e, &asset_id_0);

                assert!(!result);
            });
        }

        #[test]
        fn test_execute_set_reserve_first_reserve() {
            let e = Env::default();
            e.mock_all_auths();
            let pool = testutils::create_pool(&e);
            let bombadil = Address::generate(&e);

            let (asset_id_0, _) = testutils::create_token_contract(&e, &bombadil);

            let metadata = ReserveConfig {
                index: 0,
                decimals: 7,
                c_factor: 0_7500000,
                l_factor: 0_7500000,
                util: 0_5000000,
                max_util: 0_9500000,
                r_base: 0_0100000,
                r_one: 0_0500000,
                r_two: 0_5000000,
                r_three: 1_5000000,
                reactivity: 100,
                supply_cap: 1000000000000000000,
                enabled: true,
            };
            e.as_contract(&pool, || {
                storage::set_queued_reserve_set(
                    &e,
                    &QueuedReserveInit {
                        new_config: metadata.clone(),
                        unlock_time: e.ledger().timestamp(),
                    },
                    &asset_id_0,
                );
                execute_set_reserve(&e, &asset_id_0);
                let res_config_0: ReserveConfig = storage::get_res_config(&e, &asset_id_0);
                assert_eq!(res_config_0.decimals, metadata.decimals);
                assert_eq!(res_config_0.c_factor, metadata.c_factor);
                assert_eq!(res_config_0.l_factor, metadata.l_factor);
                assert_eq!(res_config_0.util, metadata.util);
                assert_eq!(res_config_0.max_util, metadata.max_util);
                assert_eq!(res_config_0.r_one, metadata.r_one);
                assert_eq!(res_config_0.r_two, metadata.r_two);
                assert_eq!(res_config_0.r_three, metadata.r_three);
                assert_eq!(res_config_0.reactivity, metadata.reactivity);
                assert_eq!(res_config_0.index, 0);
            });
        }

        #[test]
        #[should_panic(expected = "Error(Contract, #1203)")]
        fn test_execute_set_reserve_requires_block_passed() {
            let e = Env::default();
            e.mock_all_auths();
            let pool = testutils::create_pool(&e);
            let bombadil = Address::generate(&e);

            let (asset_id_0, _) = testutils::create_token_contract(&e, &bombadil);

            let metadata = ReserveConfig {
                index: 0,
                decimals: 7,
                c_factor: 0_7500000,
                l_factor: 0_7500000,
                util: 0_5000000,
                max_util: 0_9500000,
                r_base: 0_0100000,
                r_one: 0_0500000,
                r_two: 0_5000000,
                r_three: 1_5000000,
                reactivity: 100,
                supply_cap: 1000000000000000000,
                enabled: true,
            };
            e.as_contract(&pool, || {
                storage::set_queued_reserve_set(
                    &e,
                    &QueuedReserveInit {
                        new_config: metadata.clone(),
                        unlock_time: e.ledger().timestamp() + 1,
                    },
                    &asset_id_0,
                );
                execute_set_reserve(&e, &asset_id_0);
            });
        }

        #[test]
        fn test_execute_set_reserve_update() {
            let e = Env::default();
            e.mock_all_auths();
            e.ledger().set(LedgerInfo {
                timestamp: 500,
                protocol_version: 22,
                sequence_number: 100,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 3110400,
            });

            let pool = testutils::create_pool(&e);
            let bombadil = Address::generate(&e);

            let (underlying, _) = testutils::create_token_contract(&e, &bombadil);
            let (reserve_config, mut reserve_data) = testutils::default_reserve_meta();
            reserve_data.ir_mod = 1_001_000_000;
            testutils::create_reserve(&e, &pool, &underlying, &reserve_config, &reserve_data);

            let mut new_metadata = reserve_config.clone();
            new_metadata.index = 123;
            new_metadata.c_factor += 1;
            new_metadata.l_factor += 1;
            new_metadata.max_util += 1;
            new_metadata.reactivity += 1;

            e.ledger().set(LedgerInfo {
                timestamp: 10000,
                protocol_version: 22,
                sequence_number: 100,
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
                status: 0,
                max_positions: 2,
            };
            e.as_contract(&pool, || {
                storage::set_pool_config(&e, &pool_config);

                storage::set_queued_reserve_set(
                    &e,
                    &QueuedReserveInit {
                        new_config: new_metadata.clone(),
                        unlock_time: e.ledger().timestamp(),
                    },
                    &underlying,
                );
                execute_set_reserve(&e, &underlying);
                let res_config_updated = storage::get_res_config(&e, &underlying);
                assert_eq!(res_config_updated.decimals, new_metadata.decimals);
                assert_eq!(res_config_updated.c_factor, new_metadata.c_factor);
                assert_eq!(res_config_updated.l_factor, new_metadata.l_factor);
                assert_eq!(res_config_updated.util, new_metadata.util);
                assert_eq!(res_config_updated.max_util, new_metadata.max_util);
                assert_eq!(res_config_updated.r_base, new_metadata.r_base);
                assert_eq!(res_config_updated.r_one, new_metadata.r_one);
                assert_eq!(res_config_updated.r_two, new_metadata.r_two);
                assert_eq!(res_config_updated.r_three, new_metadata.r_three);
                assert_eq!(res_config_updated.reactivity, new_metadata.reactivity);
                assert_eq!(res_config_updated.index, reserve_config.index);

                // validate interest was accrued
                let res_data = storage::get_res_data(&e, &underlying);
                assert!(res_data.d_rate > 1_000_000_000_000);
                assert!(res_data.backstop_credit > 0);
                assert_eq!(res_data.last_time, 10000);
                assert!(res_data.ir_mod != 1_0000000);
            });
        }

        #[test]
        fn test_execute_set_reserve_update_resets_ir_mod() {
            let e = Env::default();
            e.mock_all_auths();
            e.ledger().set(LedgerInfo {
                timestamp: 500,
                protocol_version: 22,
                sequence_number: 100,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 3110400,
            });

            let pool = testutils::create_pool(&e);
            let bombadil = Address::generate(&e);

            let (underlying, _) = testutils::create_token_contract(&e, &bombadil);
            let (reserve_config, mut reserve_data) = testutils::default_reserve_meta();
            reserve_data.ir_mod = 1_100_000_000;
            testutils::create_reserve(&e, &pool, &underlying, &reserve_config, &reserve_data);

            let mut new_metadata = reserve_config.clone();
            new_metadata.r_base += 1;

            e.ledger().set(LedgerInfo {
                timestamp: 10000,
                protocol_version: 22,
                sequence_number: 100,
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
                status: 0,
                max_positions: 2,
            };
            e.as_contract(&pool, || {
                storage::set_pool_config(&e, &pool_config);

                storage::set_queued_reserve_set(
                    &e,
                    &QueuedReserveInit {
                        new_config: new_metadata.clone(),
                        unlock_time: e.ledger().timestamp(),
                    },
                    &underlying,
                );
                execute_set_reserve(&e, &underlying);
                let res_config_updated = storage::get_res_config(&e, &underlying);
                assert_eq!(res_config_updated.decimals, new_metadata.decimals);
                assert_eq!(res_config_updated.c_factor, new_metadata.c_factor);
                assert_eq!(res_config_updated.l_factor, new_metadata.l_factor);
                assert_eq!(res_config_updated.util, new_metadata.util);
                assert_eq!(res_config_updated.max_util, new_metadata.max_util);
                assert_eq!(res_config_updated.r_base, new_metadata.r_base);
                assert_eq!(res_config_updated.r_one, new_metadata.r_one);
                assert_eq!(res_config_updated.r_two, new_metadata.r_two);
                assert_eq!(res_config_updated.r_three, new_metadata.r_three);
                assert_eq!(res_config_updated.reactivity, new_metadata.reactivity);
                assert_eq!(res_config_updated.index, reserve_config.index);

                let res_data = storage::get_res_data(&e, &underlying);
                assert!(res_data.d_rate > 1_000_000_000_000);
                assert!(res_data.backstop_credit > 0);
                assert_eq!(res_data.last_time, 10000);
                assert_eq!(res_data.ir_mod, 1_0000000);
            });
        }

        #[test]
        #[should_panic(expected = "Error(Contract, #1202)")]
        fn test_execute_set_reserve_validates_decimals_stay_same() {
            let e = Env::default();
            e.mock_all_auths();
            e.ledger().set(LedgerInfo {
                timestamp: 500,
                protocol_version: 22,
                sequence_number: 100,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 3110400,
            });

            let pool = testutils::create_pool(&e);
            let bombadil = Address::generate(&e);

            let (underlying, _) = testutils::create_token_contract(&e, &bombadil);
            let (reserve_config, reserve_data) = testutils::default_reserve_meta();
            testutils::create_reserve(&e, &pool, &underlying, &reserve_config, &reserve_data);

            let new_metadata = ReserveConfig {
                index: 99,
                decimals: 8, // started at 18
                c_factor: 0_7500000,
                l_factor: 0_7500000,
                util: 0_0777777,
                max_util: 0_9500000,
                r_base: 0_0100000,
                r_one: 0_0500000,
                r_two: 0_5000000,
                r_three: 1_5000000,
                reactivity: 105,
                supply_cap: 1000000000000000000,
                enabled: true,
            };

            let pool_config = PoolConfig {
                oracle: Address::generate(&e),
                min_collateral: 1_0000000,
                bstop_rate: 0_1000000,
                status: 0,
                max_positions: 2,
            };
            e.as_contract(&pool, || {
                storage::set_pool_config(&e, &pool_config);

                storage::set_queued_reserve_set(
                    &e,
                    &QueuedReserveInit {
                        new_config: new_metadata.clone(),
                        unlock_time: e.ledger().timestamp(),
                    },
                    &underlying,
                );
                execute_set_reserve(&e, &underlying);
            });
        }

        #[test]
        fn test_initialize_reserve_sets_index() {
            let e = Env::default();
            e.mock_all_auths();
            let pool = testutils::create_pool(&e);
            let bombadil = Address::generate(&e);

            let (asset_id_0, _) = testutils::create_token_contract(&e, &bombadil);
            let (asset_id_1, _) = testutils::create_token_contract(&e, &bombadil);

            let metadata = ReserveConfig {
                index: 0,
                decimals: 7,
                c_factor: 0_7500000,
                l_factor: 0_7500000,
                util: 0_5000000,
                max_util: 0_9500000,
                r_base: 0_0100000,
                r_one: 0_0500000,
                r_two: 0_5000000,
                r_three: 1_5000000,
                reactivity: 100,
                supply_cap: 1000000000000000000,
                enabled: true,
            };
            e.as_contract(&pool, || {
                initialize_reserve(&e, &asset_id_0, &metadata);

                initialize_reserve(&e, &asset_id_1, &metadata);
                let res_config_0 = storage::get_res_config(&e, &asset_id_0);
                let res_config_1 = storage::get_res_config(&e, &asset_id_1);
                assert_eq!(res_config_0.decimals, metadata.decimals);
                assert_eq!(res_config_0.c_factor, metadata.c_factor);
                assert_eq!(res_config_0.l_factor, metadata.l_factor);
                assert_eq!(res_config_0.util, metadata.util);
                assert_eq!(res_config_0.max_util, metadata.max_util);
                assert_eq!(res_config_0.r_one, metadata.r_one);
                assert_eq!(res_config_0.r_two, metadata.r_two);
                assert_eq!(res_config_0.r_three, metadata.r_three);
                assert_eq!(res_config_0.reactivity, metadata.reactivity);
                assert_eq!(res_config_0.index, 0);
                assert_eq!(res_config_1.index, 1);
            });
        }

        #[test]
        fn test_validate_reserve_metadata() {
            let e = Env::default();

            // valid
            let metadata = ReserveConfig {
                index: 0,
                decimals: 18,
                c_factor: 0_7500000,
                l_factor: 0_7500000,
                util: 0_5000000,
                max_util: 0_9500000,
                r_base: 0_0001000,
                r_one: 0_0500000,
                r_two: 0_5000000,
                r_three: 1_5000000,
                reactivity: 100,
                supply_cap: 1000000000000000000,
                enabled: true,
            };
            require_valid_reserve_metadata(&e, &metadata);
            // no panic
            assert!(true);
        }

        #[test]
        #[should_panic(expected = "Error(Contract, #1202)")]
        fn test_validate_reserve_metadata_validates_decimals() {
            let e = Env::default();

            let metadata = ReserveConfig {
                index: 0,
                decimals: 19,
                c_factor: 0_7500000,
                l_factor: 0_7500000,
                util: 0_5000000,
                max_util: 0_9500000,
                r_base: 0_0001000,
                r_one: 0_0500000,
                r_two: 0_5000000,
                r_three: 1_5000000,
                reactivity: 100,
                supply_cap: 1000000000000000000,
                enabled: true,
            };
            require_valid_reserve_metadata(&e, &metadata);
        }

        #[test]
        #[should_panic(expected = "Error(Contract, #1202)")]
        fn test_validate_reserve_metadata_validates_c_factor() {
            let e = Env::default();

            let metadata = ReserveConfig {
                index: 0,
                decimals: 18,
                c_factor: 1_0000001,
                l_factor: 0_7500000,
                util: 0_5000000,
                max_util: 0_9500000,
                r_base: 0_0001000,
                r_one: 0_0500000,
                r_two: 0_5000000,
                r_three: 1_5000000,
                reactivity: 100,
                supply_cap: 1000000000000000000,
                enabled: true,
            };
            require_valid_reserve_metadata(&e, &metadata);
        }

        #[test]
        #[should_panic(expected = "Error(Contract, #1202)")]
        fn test_validate_reserve_metadata_validates_l_factor() {
            let e = Env::default();

            let metadata = ReserveConfig {
                index: 0,
                decimals: 18,
                c_factor: 0_7500000,
                l_factor: 1_0000001,
                util: 0_5000000,
                max_util: 0_9500000,
                r_base: 0_0001000,
                r_one: 0_0500000,
                r_two: 0_5000000,
                r_three: 1_5000000,
                reactivity: 100,
                supply_cap: 1000000000000000000,
                enabled: true,
            };
            require_valid_reserve_metadata(&e, &metadata);
        }

        #[test]
        #[should_panic(expected = "Error(Contract, #1202)")]
        fn test_validate_reserve_metadata_validates_util() {
            let e = Env::default();

            let metadata = ReserveConfig {
                index: 0,
                decimals: 18,
                c_factor: 0_7500000,
                l_factor: 0_7500000,
                util: 0_9000001,
                max_util: 0_9500000,
                r_base: 0_0001000,
                r_one: 0_0500000,
                r_two: 0_5000000,
                r_three: 1_5000000,
                reactivity: 100,
                supply_cap: 1000000000000000000,
                enabled: true,
            };
            require_valid_reserve_metadata(&e, &metadata);
        }

        #[test]
        #[should_panic(expected = "Error(Contract, #1202)")]
        fn test_validate_reserve_metadata_validates_max_util() {
            let e = Env::default();

            let metadata = ReserveConfig {
                index: 0,
                decimals: 18,
                c_factor: 0_7500000,
                l_factor: 0_7500000,
                util: 0_5000000,
                max_util: 1_0000001,
                r_base: 0_0001000,
                r_one: 0_0500000,
                r_two: 0_5000000,
                r_three: 1_5000000,
                reactivity: 100,
                supply_cap: 1000000000000000000,
                enabled: true,
            };
            require_valid_reserve_metadata(&e, &metadata);
        }

        #[test]
        #[should_panic(expected = "Error(Contract, #1202)")]
        fn test_validate_reserve_metadata_validates_r_base_too_high() {
            let e = Env::default();

            let metadata = ReserveConfig {
                index: 0,
                decimals: 18,
                c_factor: 0_7500000,
                l_factor: 0_7500000,
                util: 0_5000000,
                max_util: 0_9500000,
                r_base: 1_0000000,
                r_one: 0_0500000,
                r_two: 0_5000000,
                r_three: 1_5000000,
                reactivity: 100,
                supply_cap: 1000000000000000000,
                enabled: true,
            };
            require_valid_reserve_metadata(&e, &metadata);
        }

        #[test]
        #[should_panic(expected = "Error(Contract, #1202)")]
        fn test_validate_reserve_metadata_validates_r_base_too_low() {
            let e = Env::default();

            let metadata = ReserveConfig {
                index: 0,
                decimals: 18,
                c_factor: 0_7500000,
                l_factor: 0_7500000,
                util: 0_5000000,
                max_util: 0_9500000,
                r_base: 0_0000999,
                r_one: 0_0500000,
                r_two: 0_5000000,
                r_three: 1_5000000,
                reactivity: 100,
                supply_cap: 1000000000000000000,
                enabled: true,
            };
            require_valid_reserve_metadata(&e, &metadata);
        }

        #[test]
        #[should_panic(expected = "Error(Contract, #1202)")]
        fn test_validate_reserve_metadata_validates_r_order() {
            let e = Env::default();

            let metadata = ReserveConfig {
                index: 0,
                decimals: 18,
                c_factor: 0_7500000,
                l_factor: 0_7500000,
                util: 0_5000000,
                max_util: 0_9500000,
                r_base: 0_0000100,
                r_one: 0_5000001,
                r_two: 0_5000000,
                r_three: 1_5000000,
                reactivity: 100,
                supply_cap: 1000000000000000000,
                enabled: true,
            };
            require_valid_reserve_metadata(&e, &metadata);
        }

        #[test]
        #[should_panic(expected = "Error(Contract, #1202)")]
        fn test_validate_reserve_metadata_validates_reactivity() {
            let e = Env::default();

            let metadata = ReserveConfig {
                index: 0,
                decimals: 18,
                c_factor: 0_7500000,
                l_factor: 0_7500000,
                util: 0_5000000,
                max_util: 0_9500000,
                r_base: 0_0100000,
                r_one: 0_0500000,
                r_two: 0_5000000,
                r_three: 1_5000000,
                reactivity: 0_0001001,
                supply_cap: 1000000000000000000,
                enabled: true,
            };
            require_valid_reserve_metadata(&e, &metadata);
        }
    }
}

mod pool_src_pool_bad_debt {
    use soroban_sdk::{panic_with_error, Address, Env};

    use crate::{
        dependencies::BackstopClient, events::PoolEvents, storage, AuctionType, PoolError,
    };

    use crate::pool::{calc_pool_backstop_threshold, Pool, User};

    pub(crate) use crate::pool::bad_debt::*;

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
}

mod pool_src_pool_actions {
    use soroban_sdk::Map;

    use soroban_sdk::{contracttype, panic_with_error, Address, Env, Vec};

    use crate::events::PoolEvents;

    use crate::AuctionType;

    use crate::{auctions, errors::PoolError, validator::require_nonnegative};

    use crate::pool::pool::Pool;

    use crate::pool::User;

    pub(crate) use crate::pool::actions::*;

    mod tests {
        use crate::{
            constants::SCALAR_7,
            storage::{self, PoolConfig},
            testutils::{self, create_comet_lp_pool, create_pool},
            AuctionData, AuctionType, Positions,
        };

        use super::*;
        use soroban_sdk::{
            map,
            testutils::{Address as _, Ledger, LedgerInfo},
            vec,
        };

        /***** supply *****/

        #[test]
        fn test_build_actions_from_request_supply() {
            let e = Env::default();
            e.mock_all_auths();

            let bombadil = Address::generate(&e);
            let samwise = Address::generate(&e);
            let pool = testutils::create_pool(&e);

            let (underlying, _) = testutils::create_token_contract(&e, &bombadil);
            let (reserve_config, reserve_data) = testutils::default_reserve_meta();
            testutils::create_reserve(&e, &pool, &underlying, &reserve_config, &reserve_data);

            e.ledger().set(LedgerInfo {
                timestamp: 600,
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
                status: 0,
                max_positions: 2,
            };
            e.as_contract(&pool, || {
                storage::set_pool_config(&e, &pool_config);

                let mut pool = Pool::load(&e);

                let requests = vec![
                    &e,
                    Request {
                        request_type: RequestType::Supply as u32,
                        address: underlying.clone(),
                        amount: 10_1234567,
                    },
                ];

                let mut user = User::load(&e, &samwise);
                let actions = build_actions_from_request(&e, &mut pool, &mut user, requests);

                assert_eq!(actions.check_health, false);

                let spender_transfer = actions.spender_transfer;
                let pool_transfer = actions.pool_transfer;
                assert_eq!(spender_transfer.len(), 1);
                assert_eq!(
                    spender_transfer.get_unchecked(underlying.clone()),
                    10_1234567
                );
                assert_eq!(pool_transfer.len(), 0);

                let positions = user.positions.clone();
                assert_eq!(positions.liabilities.len(), 0);
                assert_eq!(positions.collateral.len(), 0);
                assert_eq!(positions.supply.len(), 1);
                assert_eq!(user.get_supply(0), 10_1234488);

                let reserve = pool.load_reserve(&e, &underlying, false);
                assert_eq!(
                    reserve.data.b_supply,
                    reserve_data.b_supply + user.get_supply(0)
                );
            });
        }

        /***** withdraw *****/

        #[test]
        fn test_build_actions_from_request_withdraw() {
            let e = Env::default();
            e.mock_all_auths();

            let bombadil = Address::generate(&e);
            let samwise = Address::generate(&e);
            let pool = testutils::create_pool(&e);

            let (underlying, _) = testutils::create_token_contract(&e, &bombadil);
            let (reserve_config, reserve_data) = testutils::default_reserve_meta();
            testutils::create_reserve(&e, &pool, &underlying, &reserve_config, &reserve_data);

            e.ledger().set(LedgerInfo {
                timestamp: 600,
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
                bstop_rate: 0_2000000,
                status: 0,
                max_positions: 2,
            };

            let user_positions = Positions {
                liabilities: map![&e],
                collateral: map![&e],
                supply: map![&e, (0, 20_0000000)],
            };
            e.as_contract(&pool, || {
                storage::set_pool_config(&e, &pool_config);
                storage::set_user_positions(&e, &samwise, &user_positions);

                let mut pool = Pool::load(&e);

                let requests = vec![
                    &e,
                    Request {
                        request_type: RequestType::Withdraw as u32,
                        address: underlying.clone(),
                        amount: 10_1234567,
                    },
                ];
                let mut user = User::load(&e, &samwise);
                let actions = build_actions_from_request(&e, &mut pool, &mut user, requests);

                assert_eq!(actions.check_health, false);
                assert_eq!(actions.check_max_util.len(), 0);

                let spender_transfer = actions.spender_transfer;
                let pool_transfer = actions.pool_transfer;
                assert_eq!(spender_transfer.len(), 0);
                assert_eq!(pool_transfer.len(), 1);
                assert_eq!(pool_transfer.get_unchecked(underlying.clone()), 10_1234567);

                let positions = user.positions.clone();
                assert_eq!(positions.liabilities.len(), 0);
                assert_eq!(positions.collateral.len(), 0);
                assert_eq!(positions.supply.len(), 1);
                assert_eq!(user.get_supply(0), 9_8765502);

                let reserve = pool.load_reserve(&e, &underlying, false);
                assert_eq!(
                    reserve.data.b_supply,
                    reserve_data.b_supply - (20_0000000 - 9_8765502)
                );
            });
        }

        #[test]
        fn test_build_actions_from_request_withdraw_over_balance() {
            let e = Env::default();
            e.mock_all_auths();

            let bombadil = Address::generate(&e);
            let samwise = Address::generate(&e);
            let pool = testutils::create_pool(&e);

            let (underlying, _) = testutils::create_token_contract(&e, &bombadil);
            let (reserve_config, reserve_data) = testutils::default_reserve_meta();
            testutils::create_reserve(&e, &pool, &underlying, &reserve_config, &reserve_data);

            e.ledger().set(LedgerInfo {
                timestamp: 600,
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
                bstop_rate: 0_2000000,
                status: 0,
                max_positions: 2,
            };
            let user_positions = Positions {
                liabilities: map![&e],
                collateral: map![&e],
                supply: map![&e, (0, 20_0000000)],
            };
            e.as_contract(&pool, || {
                storage::set_pool_config(&e, &pool_config);
                storage::set_user_positions(&e, &samwise, &user_positions);

                let mut pool = Pool::load(&e);

                let requests = vec![
                    &e,
                    Request {
                        request_type: RequestType::Withdraw as u32,
                        address: underlying.clone(),
                        amount: 21_0000000,
                    },
                ];
                let mut user = User::load(&e, &samwise);
                let actions = build_actions_from_request(&e, &mut pool, &mut user, requests);

                assert_eq!(actions.check_health, false);
                assert_eq!(actions.check_max_util.len(), 0);

                let spender_transfer = actions.spender_transfer;
                let pool_transfer = actions.pool_transfer;
                assert_eq!(spender_transfer.len(), 0);
                assert_eq!(pool_transfer.len(), 1);
                assert_eq!(pool_transfer.get_unchecked(underlying.clone()), 20_0000137);

                let positions = user.positions.clone();
                assert_eq!(positions.liabilities.len(), 0);
                assert_eq!(positions.collateral.len(), 0);
                assert_eq!(positions.supply.len(), 0);

                let reserve = pool.load_reserve(&e, &underlying.clone(), false);
                assert_eq!(reserve.data.b_supply, reserve_data.b_supply - 20_0000000);
            });
        }

        #[test]
        #[should_panic(expected = "Error(Contract, #1207")]
        fn test_build_actions_from_request_withdraw_blocks_over_100_util() {
            let e = Env::default();
            e.mock_all_auths();

            let bombadil = Address::generate(&e);
            let samwise = Address::generate(&e);
            let pool = testutils::create_pool(&e);

            let (underlying, _) = testutils::create_token_contract(&e, &bombadil);
            let (mut reserve_config, mut reserve_data) = testutils::default_reserve_meta();
            reserve_config.max_util = 0_9000000;
            reserve_data.b_supply = 100_0000000;
            reserve_data.d_supply = 89_0000000;
            reserve_data.backstop_credit = 10_0000000;
            testutils::create_reserve(&e, &pool, &underlying, &reserve_config, &reserve_data);

            e.ledger().set(LedgerInfo {
                timestamp: 600,
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
                bstop_rate: 0_2000000,
                status: 0,
                max_positions: 2,
            };

            let user_positions = Positions {
                liabilities: map![&e],
                collateral: map![&e],
                supply: map![&e, (0, 20_0000000)],
            };
            e.as_contract(&pool, || {
                storage::set_pool_config(&e, &pool_config);
                storage::set_user_positions(&e, &samwise, &user_positions);

                let mut pool = Pool::load(&e);

                let requests = vec![
                    &e,
                    Request {
                        request_type: RequestType::Withdraw as u32,
                        address: underlying.clone(),
                        amount: 11_0000000,
                    },
                ];
                let mut user = User::load(&e, &samwise);
                build_actions_from_request(&e, &mut pool, &mut user, requests);
            });
        }

        /***** supply collateral *****/

        #[test]
        fn test_build_actions_from_request_supply_collateral() {
            let e = Env::default();
            e.mock_all_auths();

            let bombadil = Address::generate(&e);
            let samwise = Address::generate(&e);
            let pool = testutils::create_pool(&e);

            let (underlying, _) = testutils::create_token_contract(&e, &bombadil);
            let (reserve_config, reserve_data) = testutils::default_reserve_meta();
            testutils::create_reserve(&e, &pool, &underlying, &reserve_config, &reserve_data);

            e.ledger().set(LedgerInfo {
                timestamp: 600,
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
                status: 0,
                max_positions: 2,
            };
            e.as_contract(&pool, || {
                storage::set_pool_config(&e, &pool_config);

                let mut pool = Pool::load(&e);

                let requests = vec![
                    &e,
                    Request {
                        request_type: RequestType::SupplyCollateral as u32,
                        address: underlying.clone(),
                        amount: 10_1234567,
                    },
                ];
                let mut user = User::load(&e, &samwise);
                let actions = build_actions_from_request(&e, &mut pool, &mut user, requests);

                assert_eq!(actions.check_health, false);

                let spender_transfer = actions.spender_transfer;
                let pool_transfer = actions.pool_transfer;
                assert_eq!(spender_transfer.len(), 1);
                assert_eq!(
                    spender_transfer.get_unchecked(underlying.clone()),
                    10_1234567
                );
                assert_eq!(pool_transfer.len(), 0);

                let positions = user.positions.clone();
                assert_eq!(positions.liabilities.len(), 0);
                assert_eq!(positions.collateral.len(), 1);
                assert_eq!(positions.supply.len(), 0);
                assert_eq!(user.get_collateral(0), 10_1234488);

                let reserve = pool.load_reserve(&e, &underlying.clone(), false);
                assert_eq!(
                    reserve.data.b_supply,
                    reserve_data.b_supply + user.get_collateral(0)
                );
            });
        }

        /***** withdraw collateral *****/

        #[test]
        fn test_build_actions_from_request_withdraw_collateral() {
            let e = Env::default();
            e.mock_all_auths();

            let bombadil = Address::generate(&e);
            let samwise = Address::generate(&e);
            let pool = testutils::create_pool(&e);

            let (underlying, _) = testutils::create_token_contract(&e, &bombadil);
            let (reserve_config, reserve_data) = testutils::default_reserve_meta();
            testutils::create_reserve(&e, &pool, &underlying, &reserve_config, &reserve_data);

            e.ledger().set(LedgerInfo {
                timestamp: 600,
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
                bstop_rate: 0_2000000,
                status: 0,
                max_positions: 2,
            };
            let user_positions = Positions {
                liabilities: map![&e],
                collateral: map![&e, (0, 20_0000000)],
                supply: map![&e],
            };
            e.as_contract(&pool, || {
                storage::set_pool_config(&e, &pool_config);
                storage::set_user_positions(&e, &samwise, &user_positions);

                let mut pool = Pool::load(&e);

                let requests = vec![
                    &e,
                    Request {
                        request_type: RequestType::WithdrawCollateral as u32,
                        address: underlying.clone(),
                        amount: 10_1234567,
                    },
                ];
                let mut user = User::load(&e, &samwise);
                let actions = build_actions_from_request(&e, &mut pool, &mut user, requests);

                assert_eq!(actions.check_health, true);
                assert_eq!(actions.check_max_util.len(), 0);

                let spender_transfer = actions.spender_transfer;
                let pool_transfer = actions.pool_transfer;
                assert_eq!(spender_transfer.len(), 0);
                assert_eq!(pool_transfer.len(), 1);
                assert_eq!(pool_transfer.get_unchecked(underlying.clone()), 10_1234567);

                let positions = user.positions.clone();
                assert_eq!(positions.liabilities.len(), 0);
                assert_eq!(positions.collateral.len(), 1);
                assert_eq!(positions.supply.len(), 0);
                assert_eq!(user.get_collateral(0), 9_8765502);

                let reserve = pool.load_reserve(&e, &underlying, false);
                assert_eq!(
                    reserve.data.b_supply,
                    reserve_data.b_supply - (20_0000000 - 9_8765502)
                );
            });
        }

        #[test]
        fn test_build_actions_from_request_withdraw_collateral_over_balance() {
            let e = Env::default();
            e.mock_all_auths();

            let bombadil = Address::generate(&e);
            let samwise = Address::generate(&e);
            let pool = testutils::create_pool(&e);

            let (underlying, _) = testutils::create_token_contract(&e, &bombadil);
            let (reserve_config, reserve_data) = testutils::default_reserve_meta();
            testutils::create_reserve(&e, &pool, &underlying, &reserve_config, &reserve_data);

            e.ledger().set(LedgerInfo {
                timestamp: 600,
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
                bstop_rate: 0_2000000,
                status: 0,
                max_positions: 2,
            };
            let user_positions = Positions {
                liabilities: map![&e],
                collateral: map![&e, (0, 20_0000000)],
                supply: map![&e],
            };
            e.as_contract(&pool, || {
                storage::set_pool_config(&e, &pool_config);
                storage::set_user_positions(&e, &samwise, &user_positions);

                let mut pool = Pool::load(&e);

                let requests = vec![
                    &e,
                    Request {
                        request_type: RequestType::WithdrawCollateral as u32,
                        address: underlying.clone(),
                        amount: 21_0000000,
                    },
                ];
                let mut user = User::load(&e, &samwise);
                let actions = build_actions_from_request(&e, &mut pool, &mut user, requests);

                assert_eq!(actions.check_health, true);
                assert_eq!(actions.check_max_util.len(), 0);

                let spender_transfer = actions.spender_transfer;
                let pool_transfer = actions.pool_transfer;
                assert_eq!(spender_transfer.len(), 0);
                assert_eq!(pool_transfer.len(), 1);
                assert_eq!(pool_transfer.get_unchecked(underlying.clone()), 20_0000137);

                let positions = user.positions.clone();
                assert_eq!(positions.liabilities.len(), 0);
                assert_eq!(positions.collateral.len(), 0);
                assert_eq!(positions.supply.len(), 0);

                let reserve = pool.load_reserve(&e, &underlying, false);
                assert_eq!(reserve.data.b_supply, reserve_data.b_supply - 20_0000000);
            });
        }

        #[test]
        #[should_panic(expected = "Error(Contract, #1207")]
        fn test_build_actions_from_request_withdraw_collateral_blocks_over_100_util() {
            let e = Env::default();
            e.mock_all_auths();

            let bombadil = Address::generate(&e);
            let samwise = Address::generate(&e);
            let pool = testutils::create_pool(&e);

            let (underlying, _) = testutils::create_token_contract(&e, &bombadil);
            let (mut reserve_config, mut reserve_data) = testutils::default_reserve_meta();
            reserve_config.max_util = 0_9000000;
            reserve_data.b_supply = 100_0000000;
            reserve_data.d_supply = 89_0000000;
            reserve_data.backstop_credit = 10_0000000;
            testutils::create_reserve(&e, &pool, &underlying, &reserve_config, &reserve_data);

            e.ledger().set(LedgerInfo {
                timestamp: 600,
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
                bstop_rate: 0_2000000,
                status: 0,
                max_positions: 2,
            };

            let user_positions = Positions {
                liabilities: map![&e],
                collateral: map![&e, (0, 20_0000000)],
                supply: map![&e],
            };
            e.as_contract(&pool, || {
                storage::set_pool_config(&e, &pool_config);
                storage::set_user_positions(&e, &samwise, &user_positions);

                let mut pool = Pool::load(&e);

                let requests = vec![
                    &e,
                    Request {
                        request_type: RequestType::WithdrawCollateral as u32,
                        address: underlying.clone(),
                        amount: 11_0000000,
                    },
                ];
                let mut user = User::load(&e, &samwise);
                build_actions_from_request(&e, &mut pool, &mut user, requests);
            });
        }

        /***** borrow *****/

        #[test]
        fn test_build_actions_from_request_borrow() {
            let e = Env::default();
            e.mock_all_auths();

            let bombadil = Address::generate(&e);
            let samwise = Address::generate(&e);
            let pool = testutils::create_pool(&e);

            let (underlying, _) = testutils::create_token_contract(&e, &bombadil);
            let (reserve_config, reserve_data) = testutils::default_reserve_meta();
            testutils::create_reserve(&e, &pool, &underlying, &reserve_config, &reserve_data);
            e.ledger().set(LedgerInfo {
                timestamp: 600,
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
                bstop_rate: 0_2000000,
                status: 0,
                max_positions: 2,
            };
            e.as_contract(&pool, || {
                storage::set_pool_config(&e, &pool_config);

                let mut pool = Pool::load(&e);

                let requests = vec![
                    &e,
                    Request {
                        request_type: RequestType::Borrow as u32,
                        address: underlying.clone(),
                        amount: 10_1234567,
                    },
                ];
                let mut user = User::load(&e, &samwise);
                let actions = build_actions_from_request(&e, &mut pool, &mut user, requests);

                assert_eq!(actions.check_health, true);
                assert_eq!(actions.check_max_util, vec![&e, underlying.clone()]);

                let spender_transfer = actions.spender_transfer;
                let pool_transfer = actions.pool_transfer;
                assert_eq!(spender_transfer.len(), 0);
                assert_eq!(pool_transfer.len(), 1);
                assert_eq!(pool_transfer.get_unchecked(underlying.clone()), 10_1234567);

                let positions = user.positions.clone();
                assert_eq!(positions.liabilities.len(), 1);
                assert_eq!(positions.collateral.len(), 0);
                assert_eq!(positions.supply.len(), 0);
                assert_eq!(user.get_liabilities(0), 10_1234452);

                let reserve = pool.load_reserve(&e, &underlying, false);
                assert_eq!(reserve.data.d_supply, reserve_data.d_supply + 10_1234452);
            });
        }

        #[test]
        fn test_build_actions_from_request_borrow_adds_check_util_safely() {
            let e = Env::default();
            e.mock_all_auths();

            let bombadil = Address::generate(&e);
            let samwise = Address::generate(&e);
            let pool = testutils::create_pool(&e);

            let (underlying_0, _) = testutils::create_token_contract(&e, &bombadil);
            let (reserve_config, reserve_data) = testutils::default_reserve_meta();
            testutils::create_reserve(&e, &pool, &underlying_0, &reserve_config, &reserve_data);

            let (underlying_1, _) = testutils::create_token_contract(&e, &bombadil);
            let (reserve_config, reserve_data) = testutils::default_reserve_meta();
            testutils::create_reserve(&e, &pool, &underlying_1, &reserve_config, &reserve_data);

            e.ledger().set(LedgerInfo {
                timestamp: 600,
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
                bstop_rate: 0_2000000,
                status: 0,
                max_positions: 4,
            };
            e.as_contract(&pool, || {
                storage::set_pool_config(&e, &pool_config);

                let mut pool = Pool::load(&e);

                let requests = vec![
                    &e,
                    Request {
                        request_type: RequestType::Borrow as u32,
                        address: underlying_0.clone(),
                        amount: 1_0000000,
                    },
                    Request {
                        request_type: RequestType::Borrow as u32,
                        address: underlying_1.clone(),
                        amount: 1_0000000,
                    },
                    Request {
                        request_type: RequestType::Borrow as u32,
                        address: underlying_0.clone(),
                        amount: 2_0000000,
                    },
                ];
                let mut user = User::load(&e, &samwise);
                let actions = build_actions_from_request(&e, &mut pool, &mut user, requests);

                assert_eq!(actions.check_health, true);
                assert_eq!(
                    actions.check_max_util,
                    vec![&e, underlying_0.clone(), underlying_1.clone()]
                );

                let spender_transfer = actions.spender_transfer;
                let pool_transfer = actions.pool_transfer;
                assert_eq!(spender_transfer.len(), 0);
                assert_eq!(pool_transfer.len(), 2);
                assert_eq!(pool_transfer.get_unchecked(underlying_0.clone()), 3_0000000);
                assert_eq!(pool_transfer.get_unchecked(underlying_1.clone()), 1_0000000);

                let positions = user.positions.clone();
                assert_eq!(positions.liabilities.len(), 2);
                assert_eq!(positions.collateral.len(), 0);
                assert_eq!(positions.supply.len(), 0);
                assert_eq!(user.get_liabilities(0), 2_9999967);
                assert_eq!(user.get_liabilities(1), 9999989);

                let reserve_0 = pool.load_reserve(&e, &underlying_0, false);
                assert_eq!(reserve_0.data.d_supply, reserve_data.d_supply + 2_9999967);
                let reserve_1 = pool.load_reserve(&e, &underlying_1, false);
                assert_eq!(reserve_1.data.d_supply, reserve_data.d_supply + 9999989);
            });
        }

        #[test]
        #[should_panic(expected = "Error(Contract, #1207)")]
        fn test_build_actions_from_request_borrow_blocks_over_100_util() {
            let e = Env::default();
            e.mock_all_auths();

            let bombadil = Address::generate(&e);
            let samwise = Address::generate(&e);
            let pool = testutils::create_pool(&e);

            let (underlying, _) = testutils::create_token_contract(&e, &bombadil);
            let (mut reserve_config, mut reserve_data) = testutils::default_reserve_meta();
            reserve_config.max_util = 0_9000000;
            reserve_data.b_supply = 100_0000000;
            reserve_data.d_supply = 89_0000000;
            reserve_data.backstop_credit = 10_0000000;
            testutils::create_reserve(&e, &pool, &underlying, &reserve_config, &reserve_data);

            e.ledger().set(LedgerInfo {
                timestamp: 600,
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
                bstop_rate: 0_2000000,
                status: 0,
                max_positions: 2,
            };

            let user_positions = Positions {
                liabilities: map![&e],
                collateral: map![&e, (0, 20_0000000)],
                supply: map![&e],
            };
            e.as_contract(&pool, || {
                storage::set_pool_config(&e, &pool_config);
                storage::set_user_positions(&e, &samwise, &user_positions);

                let mut pool = Pool::load(&e);

                let requests = vec![
                    &e,
                    Request {
                        request_type: RequestType::Borrow as u32,
                        address: underlying.clone(),
                        amount: 11_0000000,
                    },
                ];
                let mut user = User::load(&e, &samwise);
                build_actions_from_request(&e, &mut pool, &mut user, requests);
            });
        }

        /***** repay *****/

        #[test]
        fn test_build_actions_from_request_repay() {
            let e = Env::default();
            e.mock_all_auths();

            let bombadil = Address::generate(&e);
            let samwise = Address::generate(&e);
            let pool = testutils::create_pool(&e);

            let (underlying, _) = testutils::create_token_contract(&e, &bombadil);
            let (reserve_config, reserve_data) = testutils::default_reserve_meta();
            testutils::create_reserve(&e, &pool, &underlying, &reserve_config, &reserve_data);

            e.ledger().set(LedgerInfo {
                timestamp: 600,
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
                bstop_rate: 0_2000000,
                status: 0,
                max_positions: 2,
            };
            let user_positions = Positions {
                liabilities: map![&e, (0, 20_0000000)],
                collateral: map![&e],
                supply: map![&e],
            };
            e.as_contract(&pool, || {
                storage::set_pool_config(&e, &pool_config);
                storage::set_user_positions(&e, &samwise, &user_positions);

                let mut pool = Pool::load(&e);

                let requests = vec![
                    &e,
                    Request {
                        request_type: RequestType::Repay as u32,
                        address: underlying.clone(),
                        amount: 10_1234567,
                    },
                ];
                let mut user = User::load(&e, &samwise);
                let actions = build_actions_from_request(&e, &mut pool, &mut user, requests);

                assert_eq!(actions.check_health, false);

                let spender_transfer = actions.spender_transfer;
                let pool_transfer = actions.pool_transfer;
                assert_eq!(spender_transfer.len(), 1);
                assert_eq!(
                    spender_transfer.get_unchecked(underlying.clone()),
                    10_1234567
                );
                assert_eq!(pool_transfer.len(), 0);

                let positions = user.positions.clone();
                assert_eq!(positions.liabilities.len(), 1);
                assert_eq!(positions.collateral.len(), 0);
                assert_eq!(positions.supply.len(), 0);
                let d_tokens_repaid = 10_1234451;
                assert_eq!(user.get_liabilities(0), 20_0000000 - d_tokens_repaid);

                let reserve = pool.load_reserve(&e, &underlying, false);
                assert_eq!(
                    reserve.data.d_supply,
                    reserve_data.d_supply - d_tokens_repaid
                );
            });
        }

        #[test]
        fn test_build_actions_from_request_repay_over_balance() {
            let e = Env::default();
            e.mock_all_auths();

            let bombadil = Address::generate(&e);
            let samwise = Address::generate(&e);
            let pool = testutils::create_pool(&e);

            let (underlying, _) = testutils::create_token_contract(&e, &bombadil);
            let (reserve_config, reserve_data) = testutils::default_reserve_meta();
            testutils::create_reserve(&e, &pool, &underlying, &reserve_config, &reserve_data);

            e.ledger().set(LedgerInfo {
                timestamp: 600,
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
                bstop_rate: 0_2000000,
                status: 0,
                max_positions: 2,
            };
            let user_positions = Positions {
                liabilities: map![&e, (0, 20_0000000)],
                collateral: map![&e],
                supply: map![&e],
            };
            e.as_contract(&pool, || {
                storage::set_pool_config(&e, &pool_config);
                storage::set_user_positions(&e, &samwise, &user_positions);

                let mut pool = Pool::load(&e);

                let requests = vec![
                    &e,
                    Request {
                        request_type: RequestType::Repay as u32,
                        address: underlying.clone(),
                        amount: 21_0000000,
                    },
                ];
                let mut user = User::load(&e, &samwise);
                let actions = build_actions_from_request(&e, &mut pool, &mut user, requests);

                assert_eq!(actions.check_health, false);

                let spender_transfer = actions.spender_transfer;
                let pool_transfer = actions.pool_transfer;
                assert_eq!(spender_transfer.len(), 1);
                assert_eq!(
                    spender_transfer.get_unchecked(underlying.clone()),
                    21_0000000
                );
                assert_eq!(pool_transfer.len(), 1);
                assert_eq!(pool_transfer.get_unchecked(underlying.clone()), 0_9999771);

                let positions = user.positions.clone();
                assert_eq!(positions.liabilities.len(), 0);
                assert_eq!(positions.collateral.len(), 0);
                assert_eq!(positions.supply.len(), 0);

                let reserve = pool.load_reserve(&e, &underlying, false);
                assert_eq!(reserve.data.d_supply, reserve_data.d_supply - 20_0000000);
            });
        }

        #[test]
        fn test_aggregating_actions() {
            let e = Env::default();
            e.mock_all_auths();

            let bombadil = Address::generate(&e);
            let samwise = Address::generate(&e);
            let pool = testutils::create_pool(&e);

            let (underlying, _) = testutils::create_token_contract(&e, &bombadil);
            let (reserve_config, mut reserve_data) = testutils::default_reserve_meta();
            reserve_data.last_time = 600;
            testutils::create_reserve(
                &e,
                &pool,
                &underlying.clone(),
                &reserve_config,
                &reserve_data,
            );

            e.ledger().set(LedgerInfo {
                timestamp: 600,
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
                bstop_rate: 0_2000000,
                status: 0,
                max_positions: 2,
            };
            let user_positions = Positions::env_default(&e);
            e.as_contract(&pool, || {
                storage::set_pool_config(&e, &pool_config);
                storage::set_user_positions(&e, &samwise, &user_positions);

                let mut pool = Pool::load(&e);

                let requests = vec![
                    &e,
                    Request {
                        request_type: RequestType::Supply as u32,
                        address: underlying.clone(),
                        amount: 10_0000000,
                    },
                    Request {
                        request_type: RequestType::Withdraw as u32,
                        address: underlying.clone(),
                        amount: 5_0000000,
                    },
                    Request {
                        request_type: RequestType::SupplyCollateral as u32,
                        address: underlying.clone(),
                        amount: 10_0000000,
                    },
                    Request {
                        request_type: RequestType::WithdrawCollateral as u32,
                        address: underlying.clone(),
                        amount: 5_0000000,
                    },
                    Request {
                        request_type: RequestType::Borrow as u32,
                        address: underlying.clone(),
                        amount: 20_0000000,
                    },
                    Request {
                        request_type: RequestType::Repay as u32,
                        address: underlying.clone(),
                        amount: 21_0000000,
                    },
                ];
                let mut user = User::load(&e, &samwise);
                let actions = build_actions_from_request(&e, &mut pool, &mut user, requests);

                assert_eq!(actions.check_health, true);

                let spender_transfer = actions.spender_transfer;
                let pool_transfer = actions.pool_transfer;
                assert_eq!(spender_transfer.len(), 1);
                assert_eq!(
                    spender_transfer.get_unchecked(underlying.clone()),
                    10_0000000 + 10_0000000 + 21_0000000
                );
                assert_eq!(pool_transfer.len(), 1);
                assert_eq!(
                    pool_transfer.get_unchecked(underlying.clone()),
                    5_0000000 + 5_0000000 + 20_0000000 + 1_0000000
                );

                let positions = user.positions.clone();
                assert_eq!(positions.liabilities.len(), 0);
                assert_eq!(positions.collateral.len(), 1);
                assert_eq!(positions.supply.len(), 1);
                assert_eq!(positions.collateral.get_unchecked(0), 5_0000000);
                assert_eq!(positions.supply.get_unchecked(0), 5_0000000);
            });
        }

        #[test]
        fn test_fill_user_liquidation() {
            let e = Env::default();

            e.mock_all_auths();
            e.ledger().set(LedgerInfo {
                timestamp: 12345,
                protocol_version: 22,
                sequence_number: 176 + 200,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 3110400,
            });

            let bombadil = Address::generate(&e);
            let samwise = Address::generate(&e);
            let frodo = Address::generate(&e);

            let pool_address = create_pool(&e);

            let (oracle_address, _) = testutils::create_mock_oracle(&e);

            // creating reserves for a pool exhausts the budget
            e.cost_estimate().budget().reset_unlimited();
            let (underlying_0, _) = testutils::create_token_contract(&e, &bombadil);
            let (mut reserve_config_0, mut reserve_data_0) = testutils::default_reserve_meta();
            reserve_data_0.last_time = 12345;
            reserve_data_0.b_rate = 1_100_000_000_000;
            reserve_config_0.c_factor = 0_8500000;
            reserve_config_0.l_factor = 0_9000000;
            reserve_config_0.index = 0;
            testutils::create_reserve(
                &e,
                &pool_address,
                &underlying_0,
                &reserve_config_0,
                &reserve_data_0,
            );

            let (underlying_1, _) = testutils::create_token_contract(&e, &bombadil);
            let (mut reserve_config_1, mut reserve_data_1) = testutils::default_reserve_meta();
            reserve_data_1.b_rate = 1_200_000_000_000;
            reserve_config_1.c_factor = 0_7500000;
            reserve_config_1.l_factor = 0_7500000;
            reserve_data_1.last_time = 12345;
            reserve_config_1.index = 1;
            testutils::create_reserve(
                &e,
                &pool_address,
                &underlying_1,
                &reserve_config_1,
                &reserve_data_1,
            );

            let (underlying_2, _) = testutils::create_token_contract(&e, &bombadil);
            let (mut reserve_config_2, reserve_data_2) = testutils::default_reserve_meta();
            reserve_config_2.c_factor = 0_0000000;
            reserve_config_2.l_factor = 0_7000000;
            reserve_config_2.index = 2;
            testutils::create_reserve(
                &e,
                &pool_address,
                &underlying_2,
                &reserve_config_2,
                &reserve_data_2,
            );

            let auction_data = AuctionData {
                bid: map![&e, (underlying_2.clone(), 1_2375000)],
                lot: map![
                    &e,
                    (underlying_0.clone(), 30_5595329),
                    (underlying_1.clone(), 1_5395739)
                ],
                block: 176,
            };
            let pool_config = PoolConfig {
                oracle: oracle_address,
                min_collateral: 1_0000000,
                bstop_rate: 0_1000000,
                status: 0,
                max_positions: 4,
            };
            let positions: Positions = Positions {
                collateral: map![
                    &e,
                    (reserve_config_0.index, 90_9100000),
                    (reserve_config_1.index, 04_5800000),
                ],
                liabilities: map![&e, (reserve_config_2.index, 02_7500000),],
                supply: map![&e],
            };
            e.as_contract(&pool_address, || {
                storage::set_pool_config(&e, &pool_config);
                storage::set_user_positions(&e, &samwise, &positions);
                storage::set_auction(
                    &e,
                    &(AuctionType::UserLiquidation as u32),
                    &samwise,
                    &auction_data,
                );

                let mut pool = Pool::load(&e);

                let requests = vec![
                    &e,
                    Request {
                        request_type: RequestType::FillUserLiquidationAuction as u32,
                        address: samwise.clone(),
                        amount: 50,
                    },
                ];
                let mut user = User::load(&e, &frodo);
                let actions = build_actions_from_request(&e, &mut pool, &mut user, requests);

                assert_eq!(actions.check_health, true);
                let exp_new_auction = AuctionData {
                    bid: map![&e, (underlying_2.clone(), 6187500)],
                    lot: map![
                        &e,
                        (underlying_0.clone(), 15_2797665),
                        (underlying_1.clone(), 7697870)
                    ],
                    block: 176,
                };
                let new_auction =
                    storage::get_auction(&e, &(AuctionType::UserLiquidation as u32), &samwise);
                assert_eq!(exp_new_auction.bid, new_auction.bid);
                assert_eq!(exp_new_auction.lot, new_auction.lot);
                assert_eq!(exp_new_auction.block, new_auction.block);
                assert_eq!(actions.pool_transfer.len(), 0);
                assert_eq!(actions.spender_transfer.len(), 0);
            });
        }

        #[test]
        fn test_fill_bad_debt_auction() {
            let e = Env::default();

            e.mock_all_auths();
            e.ledger().set(LedgerInfo {
                timestamp: 12345,
                protocol_version: 22,
                sequence_number: 51 + 200,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 3110400,
            });

            let bombadil = Address::generate(&e);
            let samwise = Address::generate(&e);
            let frodo = Address::generate(&e);

            let pool_address = create_pool(&e);

            let (oracle_address, _) = testutils::create_mock_oracle(&e);

            // creating reserves for a pool exhausts the budget
            e.cost_estimate().budget().reset_unlimited();
            let (backstop_token_id, backstop_token_client) =
                testutils::create_token_contract(&e, &bombadil);
            let (backstop_address, backstop_client) = testutils::create_backstop(
                &e,
                &pool_address,
                &backstop_token_id,
                &Address::generate(&e),
                &Address::generate(&e),
            );
            let (underlying_0, _) = testutils::create_token_contract(&e, &bombadil);
            let (mut reserve_config_0, mut reserve_data_0) = testutils::default_reserve_meta();
            reserve_data_0.last_time = 12345;
            reserve_data_0.b_rate = 1_100_000_000_000;
            reserve_config_0.c_factor = 0_8500000;
            reserve_config_0.l_factor = 0_9000000;
            reserve_config_0.index = 0;
            testutils::create_reserve(
                &e,
                &pool_address,
                &underlying_0,
                &reserve_config_0,
                &reserve_data_0,
            );

            let (underlying_1, _) = testutils::create_token_contract(&e, &bombadil);
            let (mut reserve_config_1, mut reserve_data_1) = testutils::default_reserve_meta();
            reserve_data_1.b_rate = 1_200_000_000_000;
            reserve_config_1.c_factor = 0_7500000;
            reserve_config_1.l_factor = 0_7500000;
            reserve_data_1.last_time = 12345;
            reserve_config_1.index = 1;
            testutils::create_reserve(
                &e,
                &pool_address,
                &underlying_1,
                &reserve_config_1,
                &reserve_data_1,
            );
            let pool_config = PoolConfig {
                oracle: oracle_address,
                min_collateral: 1_0000000,
                bstop_rate: 0_1000000,
                status: 0,
                max_positions: 2,
            };
            let auction_data = AuctionData {
                bid: map![&e, (underlying_0, 10_0000000), (underlying_1, 2_5000000)],
                lot: map![&e, (backstop_token_id, 95_2000000)],
                block: 51,
            };
            let positions: Positions = Positions {
                collateral: map![&e],
                liabilities: map![
                    &e,
                    (reserve_config_0.index, 10_0000000),
                    (reserve_config_1.index, 2_5000000)
                ],
                supply: map![&e],
            };
            backstop_token_client.mint(&samwise, &95_2000000);
            backstop_token_client.approve(&samwise, &backstop_address, &i128::MAX, &1000000);
            backstop_client.deposit(&samwise, &pool_address, &95_2000000);
            e.as_contract(&pool_address, || {
                storage::set_pool_config(&e, &pool_config);
                storage::set_user_positions(&e, &backstop_address, &positions);
                storage::set_auction(
                    &e,
                    &(AuctionType::BadDebtAuction as u32),
                    &backstop_address,
                    &auction_data,
                );

                let mut pool = Pool::load(&e);

                let requests = vec![
                    &e,
                    Request {
                        request_type: RequestType::FillBadDebtAuction as u32,
                        address: backstop_address.clone(),
                        amount: 100,
                    },
                ];
                let mut user = User::load(&e, &frodo);
                let actions = build_actions_from_request(&e, &mut pool, &mut user, requests);

                assert_eq!(actions.check_health, true);
                assert_eq!(
                    storage::has_auction(
                        &e,
                        &(AuctionType::BadDebtAuction as u32),
                        &backstop_address
                    ),
                    false
                );
                assert_eq!(actions.pool_transfer.len(), 0);
                assert_eq!(actions.spender_transfer.len(), 0);
            });
        }

        #[test]
        fn test_fill_interest_auction() {
            let e = Env::default();
            e.cost_estimate().budget().reset_unlimited();
            e.mock_all_auths_allowing_non_root_auth();

            e.ledger().set(LedgerInfo {
                timestamp: 12345,
                protocol_version: 22,
                sequence_number: 51 + 250,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 3110400,
            });

            let bombadil = Address::generate(&e);
            let samwise = Address::generate(&e);

            let pool_address = create_pool(&e);
            let (usdc_id, usdc_client) = testutils::create_token_contract(&e, &bombadil);
            let (blnd_id, blnd_client) = testutils::create_blnd_token(&e, &pool_address, &bombadil);

            let (backstop_token_id, backstop_token_client) =
                create_comet_lp_pool(&e, &bombadil, &blnd_id, &usdc_id);
            let (backstop_address, backstop_client) = testutils::create_backstop(
                &e,
                &pool_address,
                &backstop_token_id,
                &usdc_id,
                &blnd_id,
            );
            blnd_client.mint(&samwise, &10_000_0000000);
            usdc_client.mint(&samwise, &250_0000000);
            let exp_ledger = e.ledger().sequence() + 100;
            blnd_client.approve(&bombadil, &backstop_token_id, &2_000_0000000, &exp_ledger);
            usdc_client.approve(&bombadil, &backstop_token_id, &2_000_0000000, &exp_ledger);
            backstop_token_client.join_pool(
                &(100 * SCALAR_7),
                &vec![&e, 10_000_0000000, 250_0000000],
                &samwise,
            );
            backstop_client.deposit(&bombadil, &pool_address, &(50 * SCALAR_7));

            let (underlying_0, underlying_0_client) =
                testutils::create_token_contract(&e, &bombadil);
            let (mut reserve_config_0, mut reserve_data_0) = testutils::default_reserve_meta();
            reserve_data_0.b_rate = 1_100_000_000_000;
            reserve_data_0.last_time = 12345;
            reserve_config_0.index = 0;
            testutils::create_reserve(
                &e,
                &pool_address,
                &underlying_0,
                &reserve_config_0,
                &reserve_data_0,
            );
            underlying_0_client.mint(&pool_address, &1_000_0000000);

            let (underlying_1, underlying_1_client) =
                testutils::create_token_contract(&e, &bombadil);
            let (mut reserve_config_1, mut reserve_data_1) = testutils::default_reserve_meta();
            reserve_data_1.b_rate = 1_100_000_000_000;
            reserve_data_1.last_time = 12345;
            reserve_config_1.index = 1;
            testutils::create_reserve(
                &e,
                &pool_address,
                &underlying_1,
                &reserve_config_1,
                &reserve_data_1,
            );
            underlying_1_client.mint(&pool_address, &1_000_0000000);

            let (underlying_2, underlying_2_client) =
                testutils::create_token_contract(&e, &bombadil);
            let (mut reserve_config_2, mut reserve_data_2) = testutils::default_reserve_meta();
            reserve_data_2.b_rate = 1_100_000_000_000;
            reserve_data_2.last_time = 12345;
            reserve_config_2.index = 1;
            testutils::create_reserve(
                &e,
                &pool_address,
                &underlying_2,
                &reserve_config_2,
                &reserve_data_2,
            );
            underlying_2_client.mint(&pool_address, &1_000_0000000);

            let pool_config = PoolConfig {
                oracle: Address::generate(&e),
                min_collateral: 1_0000000,
                bstop_rate: 0_1000000,
                status: 0,
                max_positions: 2,
            };
            let auction_data = AuctionData {
                bid: map![&e, (backstop_token_id.clone(), 100_0000000)],
                lot: map![
                    &e,
                    (underlying_0.clone(), 100_0000000),
                    (underlying_1.clone(), 25_0000000)
                ],
                block: 51,
            };

            backstop_token_client.approve(
                &samwise,
                &backstop_address,
                &100_0000000,
                &e.ledger().sequence(),
            );
            e.as_contract(&pool_address, || {
                e.mock_all_auths_allowing_non_root_auth();
                storage::set_pool_config(&e, &pool_config);
                storage::set_auction(
                    &e,
                    &(AuctionType::InterestAuction as u32),
                    &backstop_address,
                    &auction_data,
                );
                storage::set_backstop(&e, &backstop_address);

                let mut pool = Pool::load(&e);

                let requests = vec![
                    &e,
                    Request {
                        request_type: RequestType::FillInterestAuction as u32,
                        address: backstop_address.clone(),
                        amount: 100,
                    },
                ];
                let pre_fill_backstop_token_balance =
                    backstop_token_client.balance(&backstop_address);
                let mut user = User::load(&e, &samwise);
                let actions = build_actions_from_request(&e, &mut pool, &mut user, requests);

                assert_eq!(backstop_token_client.balance(&samwise), 25_0000000);
                assert_eq!(
                    backstop_token_client.balance(&backstop_address),
                    pre_fill_backstop_token_balance + 75_0000000
                );
                assert_eq!(underlying_0_client.balance(&samwise), 100_0000000);
                assert_eq!(underlying_1_client.balance(&samwise), 25_0000000);
                assert_eq!(actions.check_health, false);
                assert_eq!(
                    storage::has_auction(
                        &e,
                        &(AuctionType::InterestAuction as u32),
                        &backstop_address
                    ),
                    false
                );
                assert_eq!(actions.pool_transfer.len(), 0);
                assert_eq!(actions.spender_transfer.len(), 0);
            });
        }

        /***** delete liquidation auction *****/

        #[test]
        fn test_delete_liquidation_auction() {
            let e = Env::default();
            e.cost_estimate().budget().reset_unlimited();
            e.mock_all_auths_allowing_non_root_auth();

            e.ledger().set(LedgerInfo {
                timestamp: 12345,
                protocol_version: 22,
                sequence_number: 51 + 200,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 3110400,
            });

            let samwise = Address::generate(&e);
            let underlying_0 = Address::generate(&e);
            let underlying_1 = Address::generate(&e);

            let pool_address = create_pool(&e);

            let pool_config = PoolConfig {
                oracle: Address::generate(&e),
                min_collateral: 1_0000000,
                bstop_rate: 0_1000000,
                status: 0,
                max_positions: 2,
            };
            let auction_data = AuctionData {
                bid: map![&e, (underlying_0.clone(), 952_0000000)],
                lot: map![
                    &e,
                    (underlying_0.clone(), 100_0000000),
                    (underlying_1.clone(), 25_0000000)
                ],
                block: 51,
            };

            e.as_contract(&pool_address, || {
                e.mock_all_auths_allowing_non_root_auth();
                storage::set_pool_config(&e, &pool_config);
                storage::set_auction(
                    &e,
                    &(AuctionType::UserLiquidation as u32),
                    &samwise,
                    &auction_data,
                );

                let mut pool = Pool::load(&e);

                let requests = vec![
                    &e,
                    Request {
                        request_type: RequestType::DeleteLiquidationAuction as u32,
                        address: Address::generate(&e),
                        amount: 0,
                    },
                ];
                let mut user = User::load(&e, &samwise);
                let actions = build_actions_from_request(&e, &mut pool, &mut user, requests);

                assert_eq!(actions.check_health, true);
                assert_eq!(
                    storage::has_auction(&e, &(AuctionType::UserLiquidation as u32), &samwise),
                    false
                );
                assert_eq!(actions.pool_transfer.len(), 0);
                assert_eq!(actions.spender_transfer.len(), 0);
            });
        }

        /********** reserve conifg **********/

        #[test]
        #[should_panic(expected = "Error(Contract, #1220)")]
        fn test_exceed_supply_cap() {
            let e = Env::default();
            e.mock_all_auths();

            let bombadil = Address::generate(&e);
            let samwise = Address::generate(&e);
            let pool = testutils::create_pool(&e);

            let (underlying, _) = testutils::create_token_contract(&e, &bombadil);
            let (mut reserve_config, reserve_data) = testutils::default_reserve_meta();
            reserve_config.supply_cap = 10_0000000; // Set low collateral cap
            testutils::create_reserve(&e, &pool, &underlying, &reserve_config, &reserve_data);

            let pool_config = PoolConfig {
                oracle: Address::generate(&e),
                min_collateral: 1_0000000,
                bstop_rate: 0_2000000,
                status: 0,
                max_positions: 1,
            };

            let requests = vec![
                &e,
                Request {
                    request_type: RequestType::SupplyCollateral as u32,
                    address: underlying.clone(),
                    amount: 20_0000000, // Try to supply more than cap
                },
            ];

            e.as_contract(&pool, || {
                storage::set_pool_config(&e, &pool_config);
                let mut pool = Pool::load(&e);

                let mut user = User::load(&e, &samwise);
                build_actions_from_request(&e, &mut pool, &mut user, requests);
            });
        }

        #[test]
        #[should_panic(expected = "Error(Contract, #1223)")]
        fn test_build_actions_panic_borrow_disabled_asset() {
            let e = Env::default();
            e.mock_all_auths();

            let bombadil = Address::generate(&e);
            let samwise = Address::generate(&e);
            let pool = testutils::create_pool(&e);

            let (underlying, _) = testutils::create_token_contract(&e, &bombadil);
            let (mut reserve_config, reserve_data) = testutils::default_reserve_meta();
            reserve_config.enabled = false;
            testutils::create_reserve(&e, &pool, &underlying, &reserve_config, &reserve_data);

            let pool_config = PoolConfig {
                oracle: Address::generate(&e),
                min_collateral: 1_0000000,
                bstop_rate: 0_2000000,
                status: 0,
                max_positions: 1,
            };

            let requests = vec![
                &e,
                Request {
                    request_type: RequestType::Borrow as u32,
                    address: underlying.clone(),
                    amount: 20_0000000,
                },
            ];

            e.as_contract(&pool, || {
                storage::set_pool_config(&e, &pool_config);
                let mut pool = Pool::load(&e);
                let mut user = User::load(&e, &samwise);

                build_actions_from_request(&e, &mut pool, &mut user, requests);
            });
        }

        #[test]
        #[should_panic(expected = "Error(Contract, #1223)")]
        fn test_build_actions_panic_supply_collateral_disabled_asset() {
            let e = Env::default();
            e.mock_all_auths();

            let bombadil = Address::generate(&e);
            let samwise = Address::generate(&e);
            let pool = testutils::create_pool(&e);

            let (underlying, _) = testutils::create_token_contract(&e, &bombadil);
            let (mut reserve_config, reserve_data) = testutils::default_reserve_meta();
            reserve_config.enabled = false;
            testutils::create_reserve(&e, &pool, &underlying, &reserve_config, &reserve_data);

            let pool_config = PoolConfig {
                oracle: Address::generate(&e),
                min_collateral: 1_0000000,
                bstop_rate: 0_2000000,
                status: 0,
                max_positions: 1,
            };

            let requests = vec![
                &e,
                Request {
                    request_type: RequestType::SupplyCollateral as u32,
                    address: underlying.clone(),
                    amount: 20_0000000,
                },
            ];

            e.as_contract(&pool, || {
                storage::set_pool_config(&e, &pool_config);
                let mut pool = Pool::load(&e);
                let mut user = User::load(&e, &samwise);

                build_actions_from_request(&e, &mut pool, &mut user, requests);
            });
        }

        #[test]
        #[should_panic(expected = "Error(Contract, #1223)")]
        fn test_build_actions_panic_supply_disabled_asset() {
            let e = Env::default();
            e.mock_all_auths();

            let bombadil = Address::generate(&e);
            let samwise = Address::generate(&e);
            let pool = testutils::create_pool(&e);

            let (underlying, _) = testutils::create_token_contract(&e, &bombadil);
            let (mut reserve_config, reserve_data) = testutils::default_reserve_meta();
            reserve_config.enabled = false;
            testutils::create_reserve(&e, &pool, &underlying, &reserve_config, &reserve_data);

            let pool_config = PoolConfig {
                oracle: Address::generate(&e),
                min_collateral: 1_0000000,
                bstop_rate: 0_2000000,
                status: 0,
                max_positions: 1,
            };

            let requests = vec![
                &e,
                Request {
                    request_type: RequestType::Supply as u32,
                    address: underlying.clone(),
                    amount: 20_0000000,
                },
            ];

            e.as_contract(&pool, || {
                storage::set_pool_config(&e, &pool_config);
                let mut pool = Pool::load(&e);
                let mut user = User::load(&e, &samwise);

                build_actions_from_request(&e, &mut pool, &mut user, requests);
            });
        }
    }
}

mod pool_src_auctions_user_liquidation_auction {
    use cast::i128;

    use soroban_fixed_point_math::SorobanFixedPoint;

    use soroban_sdk::{map, panic_with_error, Address, Env, Vec};

    use crate::auctions::auction::AuctionData;

    use crate::pool::{check_and_handle_user_bad_debt, Pool, PositionData, User};

    use crate::Positions;

    use crate::{errors::PoolError, storage};

    use crate::auctions::AuctionType;

    pub(crate) use crate::auctions::user_liquidation_auction::*;

    mod tests {
        use crate::{
            auctions::auction::AuctionType,
            pool::Positions,
            storage::{self, PoolConfig},
            testutils::{self, create_pool},
        };

        use super::*;
        use sep_40_oracle::testutils::Asset;
        use soroban_sdk::{
            testutils::{Address as AddressTestTrait, Ledger, LedgerInfo},
            vec, Symbol,
        };

        #[test]
        #[should_panic(expected = "Error(Contract, #1212)")]
        fn test_create_liquidation_already_in_progress() {
            let e = Env::default();
            e.mock_all_auths();

            let pool_address = create_pool(&e);
            let (oracle, _) = testutils::create_mock_oracle(&e);
            let backstop_address = Address::generate(&e);

            let samwise = Address::generate(&e);

            e.ledger().set(LedgerInfo {
                timestamp: 12345,
                protocol_version: 22,
                sequence_number: 100,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 3110400,
            });

            let liq_pct = 50;

            let auction_data = AuctionData {
                bid: map![&e],
                lot: map![&e],
                block: 50,
            };
            let pool_config = PoolConfig {
                oracle,
                min_collateral: 1_0000000,
                bstop_rate: 0_1000000,
                status: 0,
                max_positions: 4,
            };
            e.as_contract(&pool_address, || {
                storage::set_pool_config(&e, &pool_config);
                storage::set_backstop(&e, &backstop_address);
                storage::set_auction(
                    &e,
                    &(AuctionType::UserLiquidation as u32),
                    &samwise,
                    &auction_data,
                );
                create_user_liq_auction_data(&e, &samwise, &vec![&e], &vec![&e], liq_pct);
            });
        }

        #[test]
        #[should_panic(expected = "Error(Contract, #1211)")]
        fn test_create_liquidation_user_is_pool() {
            let e = Env::default();
            e.mock_all_auths();

            let pool_address = create_pool(&e);
            let (oracle, _) = testutils::create_mock_oracle(&e);
            let backstop_address = Address::generate(&e);

            e.ledger().set(LedgerInfo {
                timestamp: 12345,
                protocol_version: 22,
                sequence_number: 100,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 3110400,
            });

            let liq_pct = 50;
            let pool_config = PoolConfig {
                oracle,
                min_collateral: 1_0000000,
                bstop_rate: 0_1000000,
                status: 0,
                max_positions: 4,
            };
            e.as_contract(&pool_address, || {
                storage::set_pool_config(&e, &pool_config);
                storage::set_backstop(&e, &backstop_address);
                create_user_liq_auction_data(&e, &pool_address, &vec![&e], &vec![&e], liq_pct);
            });
        }

        #[test]
        #[should_panic(expected = "Error(Contract, #1211)")]
        fn test_create_liquidation_user_is_backstop() {
            let e = Env::default();
            e.mock_all_auths();

            let pool_address = create_pool(&e);
            let (oracle, _) = testutils::create_mock_oracle(&e);
            let backstop_address = Address::generate(&e);

            e.ledger().set(LedgerInfo {
                timestamp: 12345,
                protocol_version: 22,
                sequence_number: 100,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 3110400,
            });

            let liq_pct = 50;
            let pool_config = PoolConfig {
                oracle,
                min_collateral: 1_0000000,
                bstop_rate: 0_1000000,
                status: 0,
                max_positions: 4,
            };
            e.as_contract(&pool_address, || {
                storage::set_pool_config(&e, &pool_config);
                storage::set_backstop(&e, &backstop_address);
                create_user_liq_auction_data(&e, &backstop_address, &vec![&e], &vec![&e], liq_pct);
            });
        }

        #[test]
        #[should_panic(expected = "Error(Contract, #1211)")]
        fn test_create_liquidation_percent_zero() {
            let e = Env::default();
            e.mock_all_auths();

            let pool_address = create_pool(&e);
            let (oracle, _) = testutils::create_mock_oracle(&e);
            let backstop_address = Address::generate(&e);

            e.ledger().set(LedgerInfo {
                timestamp: 12345,
                protocol_version: 22,
                sequence_number: 100,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 3110400,
            });

            let liq_pct = 0;
            let pool_config = PoolConfig {
                oracle,
                min_collateral: 1_0000000,
                bstop_rate: 0_1000000,
                status: 0,
                max_positions: 4,
            };
            e.as_contract(&pool_address, || {
                storage::set_pool_config(&e, &pool_config);
                storage::set_backstop(&e, &backstop_address);
                create_user_liq_auction_data(&e, &backstop_address, &vec![&e], &vec![&e], liq_pct);
            });
        }

        #[test]
        #[should_panic(expected = "Error(Contract, #1211)")]
        fn test_create_liquidation_percent_over_100() {
            let e = Env::default();
            e.mock_all_auths();

            let pool_address = create_pool(&e);
            let (oracle, _) = testutils::create_mock_oracle(&e);
            let backstop_address = Address::generate(&e);

            e.ledger().set(LedgerInfo {
                timestamp: 12345,
                protocol_version: 22,
                sequence_number: 100,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 3110400,
            });

            let liq_pct = 101;
            let pool_config = PoolConfig {
                oracle,
                min_collateral: 1_0000000,
                bstop_rate: 0_1000000,
                status: 0,
                max_positions: 4,
            };
            e.as_contract(&pool_address, || {
                storage::set_pool_config(&e, &pool_config);
                storage::set_backstop(&e, &backstop_address);
                create_user_liq_auction_data(&e, &backstop_address, &vec![&e], &vec![&e], liq_pct);
            });
        }

        #[test]
        #[should_panic(expected = "Error(Contract, #1221)")]
        fn test_create_user_liquidation_invalid_bid_empty() {
            let e = Env::default();

            e.mock_all_auths();
            e.ledger().set(LedgerInfo {
                timestamp: 12345,
                protocol_version: 22,
                sequence_number: 50,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 3110400,
            });

            let bombadil = Address::generate(&e);
            let samwise = Address::generate(&e);

            let pool_address = create_pool(&e);
            let (oracle_address, oracle_client) = testutils::create_mock_oracle(&e);
            let backstop_address = Address::generate(&e);

            // creating reserves for a pool exhausts the budget
            e.cost_estimate().budget().reset_unlimited();
            let (underlying_0, _) = testutils::create_token_contract(&e, &bombadil);
            let (mut reserve_config_0, mut reserve_data_0) = testutils::default_reserve_meta();
            reserve_data_0.last_time = 12345;
            reserve_data_0.b_rate = 1_100_000_000_000;
            reserve_data_0.d_rate = 1_150_000_000_000;
            reserve_config_0.c_factor = 0_8500000;
            reserve_config_0.l_factor = 0_9000000;
            reserve_config_0.index = 0;
            testutils::create_reserve(
                &e,
                &pool_address,
                &underlying_0,
                &reserve_config_0,
                &reserve_data_0,
            );

            let (underlying_1, _) = testutils::create_token_contract(&e, &bombadil);
            let (mut reserve_config_1, mut reserve_data_1) = testutils::default_reserve_meta();
            reserve_data_1.last_time = 12345;
            reserve_data_1.b_rate = 1_200_000_000_000;
            reserve_data_1.d_rate = 1_300_000_000_000;
            reserve_config_1.c_factor = 0_8000000;
            reserve_config_1.l_factor = 0_7500000;
            reserve_config_1.index = 1;
            testutils::create_reserve(
                &e,
                &pool_address,
                &underlying_1,
                &reserve_config_1,
                &reserve_data_1,
            );

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
            oracle_client.set_price_stable(&vec![&e, 2_0000000, 4_0000000]);

            let liq_pct = 50;
            let positions: Positions = Positions {
                collateral: map![&e, (reserve_config_0.index, 100_0000000),],
                liabilities: map![&e, (reserve_config_1.index, 30_0000000),],
                supply: map![&e],
            };
            let pool_config = PoolConfig {
                oracle: oracle_address,
                min_collateral: 1_0000000,
                bstop_rate: 0_1000000,
                status: 0,
                max_positions: 4,
            };
            e.as_contract(&pool_address, || {
                storage::set_user_positions(&e, &samwise, &positions);
                storage::set_pool_config(&e, &pool_config);
                storage::set_backstop(&e, &backstop_address);

                create_user_liq_auction_data(
                    &e,
                    &samwise,
                    &vec![&e],
                    &vec![&e, underlying_0.clone()],
                    liq_pct,
                );
            });
        }

        #[test]
        #[should_panic(expected = "Error(Contract, #1221)")]
        fn test_create_user_liquidation_invalid_bid_no_position() {
            let e = Env::default();

            e.mock_all_auths();
            e.ledger().set(LedgerInfo {
                timestamp: 12345,
                protocol_version: 22,
                sequence_number: 50,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 3110400,
            });

            let bombadil = Address::generate(&e);
            let samwise = Address::generate(&e);

            let pool_address = create_pool(&e);
            let (oracle_address, oracle_client) = testutils::create_mock_oracle(&e);
            let backstop_address = Address::generate(&e);

            // creating reserves for a pool exhausts the budget
            e.cost_estimate().budget().reset_unlimited();
            let (underlying_0, _) = testutils::create_token_contract(&e, &bombadil);
            let (mut reserve_config_0, mut reserve_data_0) = testutils::default_reserve_meta();
            reserve_data_0.last_time = 12345;
            reserve_data_0.b_rate = 1_100_000_000_000;
            reserve_data_0.d_rate = 1_150_000_000_000;
            reserve_config_0.c_factor = 0_8500000;
            reserve_config_0.l_factor = 0_9000000;
            reserve_config_0.index = 0;
            testutils::create_reserve(
                &e,
                &pool_address,
                &underlying_0,
                &reserve_config_0,
                &reserve_data_0,
            );

            let (underlying_1, _) = testutils::create_token_contract(&e, &bombadil);
            let (mut reserve_config_1, mut reserve_data_1) = testutils::default_reserve_meta();
            reserve_data_1.last_time = 12345;
            reserve_data_1.b_rate = 1_200_000_000_000;
            reserve_data_1.d_rate = 1_300_000_000_000;
            reserve_config_1.c_factor = 0_8000000;
            reserve_config_1.l_factor = 0_7500000;
            reserve_config_1.index = 1;
            testutils::create_reserve(
                &e,
                &pool_address,
                &underlying_1,
                &reserve_config_1,
                &reserve_data_1,
            );

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
            oracle_client.set_price_stable(&vec![&e, 2_0000000, 4_0000000]);

            let liq_pct = 50;
            let positions: Positions = Positions {
                collateral: map![&e, (reserve_config_0.index, 100_0000000),],
                liabilities: map![&e, (reserve_config_1.index, 30_0000000),],
                supply: map![&e],
            };
            let pool_config = PoolConfig {
                oracle: oracle_address,
                min_collateral: 1_0000000,
                bstop_rate: 0_1000000,
                status: 0,
                max_positions: 4,
            };
            e.as_contract(&pool_address, || {
                storage::set_user_positions(&e, &samwise, &positions);
                storage::set_pool_config(&e, &pool_config);
                storage::set_backstop(&e, &backstop_address);

                create_user_liq_auction_data(
                    &e,
                    &samwise,
                    &vec![&e, underlying_0.clone()],
                    &vec![&e, underlying_0.clone()],
                    liq_pct,
                );
            });
        }

        #[test]
        #[should_panic(expected = "Error(Contract, #1222)")]
        fn test_create_user_liquidation_invalid_lot_empty() {
            let e = Env::default();

            e.mock_all_auths();
            e.ledger().set(LedgerInfo {
                timestamp: 12345,
                protocol_version: 22,
                sequence_number: 50,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 3110400,
            });

            let bombadil = Address::generate(&e);
            let samwise = Address::generate(&e);

            let pool_address = create_pool(&e);
            let (oracle_address, oracle_client) = testutils::create_mock_oracle(&e);
            let backstop_address = Address::generate(&e);

            // creating reserves for a pool exhausts the budget
            e.cost_estimate().budget().reset_unlimited();
            let (underlying_0, _) = testutils::create_token_contract(&e, &bombadil);
            let (mut reserve_config_0, mut reserve_data_0) = testutils::default_reserve_meta();
            reserve_data_0.last_time = 12345;
            reserve_data_0.b_rate = 1_100_000_000_000;
            reserve_data_0.d_rate = 1_150_000_000_000;
            reserve_config_0.c_factor = 0_8500000;
            reserve_config_0.l_factor = 0_9000000;
            reserve_config_0.index = 0;
            testutils::create_reserve(
                &e,
                &pool_address,
                &underlying_0,
                &reserve_config_0,
                &reserve_data_0,
            );

            let (underlying_1, _) = testutils::create_token_contract(&e, &bombadil);
            let (mut reserve_config_1, mut reserve_data_1) = testutils::default_reserve_meta();
            reserve_data_1.last_time = 12345;
            reserve_data_1.b_rate = 1_200_000_000_000;
            reserve_data_1.d_rate = 1_300_000_000_000;
            reserve_config_1.c_factor = 0_8000000;
            reserve_config_1.l_factor = 0_7500000;
            reserve_config_1.index = 1;
            testutils::create_reserve(
                &e,
                &pool_address,
                &underlying_1,
                &reserve_config_1,
                &reserve_data_1,
            );

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
            oracle_client.set_price_stable(&vec![&e, 2_0000000, 4_0000000]);

            let liq_pct = 50;
            let positions: Positions = Positions {
                collateral: map![&e, (reserve_config_0.index, 100_0000000),],
                liabilities: map![&e, (reserve_config_1.index, 30_0000000),],
                supply: map![&e],
            };
            let pool_config = PoolConfig {
                oracle: oracle_address,
                min_collateral: 1_0000000,
                bstop_rate: 0_1000000,
                status: 0,
                max_positions: 4,
            };
            e.as_contract(&pool_address, || {
                storage::set_user_positions(&e, &samwise, &positions);
                storage::set_pool_config(&e, &pool_config);
                storage::set_backstop(&e, &backstop_address);

                create_user_liq_auction_data(
                    &e,
                    &samwise,
                    &vec![&e, underlying_1.clone()],
                    &vec![&e],
                    liq_pct,
                );
            });
        }

        #[test]
        #[should_panic(expected = "Error(Contract, #1222)")]
        fn test_create_user_liquidation_invalid_lot_no_position() {
            let e = Env::default();

            e.mock_all_auths();
            e.ledger().set(LedgerInfo {
                timestamp: 12345,
                protocol_version: 22,
                sequence_number: 50,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 3110400,
            });

            let bombadil = Address::generate(&e);
            let samwise = Address::generate(&e);

            let pool_address = create_pool(&e);
            let (oracle_address, oracle_client) = testutils::create_mock_oracle(&e);
            let backstop_address = Address::generate(&e);

            // creating reserves for a pool exhausts the budget
            e.cost_estimate().budget().reset_unlimited();
            let (underlying_0, _) = testutils::create_token_contract(&e, &bombadil);
            let (mut reserve_config_0, mut reserve_data_0) = testutils::default_reserve_meta();
            reserve_data_0.last_time = 12345;
            reserve_data_0.b_rate = 1_100_000_000_000;
            reserve_data_0.d_rate = 1_150_000_000_000;
            reserve_config_0.c_factor = 0_8500000;
            reserve_config_0.l_factor = 0_9000000;
            reserve_config_0.index = 0;
            testutils::create_reserve(
                &e,
                &pool_address,
                &underlying_0,
                &reserve_config_0,
                &reserve_data_0,
            );

            let (underlying_1, _) = testutils::create_token_contract(&e, &bombadil);
            let (mut reserve_config_1, mut reserve_data_1) = testutils::default_reserve_meta();
            reserve_data_1.last_time = 12345;
            reserve_data_1.b_rate = 1_200_000_000_000;
            reserve_data_1.d_rate = 1_300_000_000_000;
            reserve_config_1.c_factor = 0_8000000;
            reserve_config_1.l_factor = 0_7500000;
            reserve_config_1.index = 1;
            testutils::create_reserve(
                &e,
                &pool_address,
                &underlying_1,
                &reserve_config_1,
                &reserve_data_1,
            );

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
            oracle_client.set_price_stable(&vec![&e, 2_0000000, 4_0000000]);

            let liq_pct = 50;
            let positions: Positions = Positions {
                collateral: map![&e, (reserve_config_0.index, 100_0000000),],
                liabilities: map![&e, (reserve_config_1.index, 30_0000000),],
                supply: map![&e],
            };
            let pool_config = PoolConfig {
                oracle: oracle_address,
                min_collateral: 1_0000000,
                bstop_rate: 0_1000000,
                status: 0,
                max_positions: 4,
            };
            e.as_contract(&pool_address, || {
                storage::set_user_positions(&e, &samwise, &positions);
                storage::set_pool_config(&e, &pool_config);
                storage::set_backstop(&e, &backstop_address);

                create_user_liq_auction_data(
                    &e,
                    &samwise,
                    &vec![&e, underlying_1.clone()],
                    &vec![&e, underlying_1.clone()],
                    liq_pct,
                );
            });
        }

        #[test]
        #[should_panic(expected = "Error(Contract, #1208)")]
        fn test_create_user_liquidation_checks_max_positions() {
            let e = Env::default();

            e.mock_all_auths();
            e.ledger().set(LedgerInfo {
                timestamp: 12345,
                protocol_version: 22,
                sequence_number: 50,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 3110400,
            });

            let bombadil = Address::generate(&e);
            let samwise = Address::generate(&e);

            let pool_address = create_pool(&e);
            let (oracle_address, oracle_client) = testutils::create_mock_oracle(&e);
            let backstop_address = Address::generate(&e);

            // creating reserves for a pool exhausts the budget
            e.cost_estimate().budget().reset_unlimited();
            let (underlying_0, _) = testutils::create_token_contract(&e, &bombadil);
            let (mut reserve_config_0, mut reserve_data_0) = testutils::default_reserve_meta();
            reserve_data_0.last_time = 12345;
            reserve_data_0.b_rate = 1_100_000_000_000;
            reserve_config_0.c_factor = 0_8500000;
            reserve_config_0.l_factor = 0_9000000;
            reserve_config_0.index = 0;
            testutils::create_reserve(
                &e,
                &pool_address,
                &underlying_0,
                &reserve_config_0,
                &reserve_data_0,
            );

            let (underlying_1, _) = testutils::create_token_contract(&e, &bombadil);
            let (mut reserve_config_1, mut reserve_data_1) = testutils::default_reserve_meta();
            reserve_data_1.b_rate = 1_200_000_000_000;
            reserve_config_1.c_factor = 0_7500000;
            reserve_config_1.l_factor = 0_7500000;
            reserve_data_1.last_time = 12345;
            reserve_config_1.index = 1;
            testutils::create_reserve(
                &e,
                &pool_address,
                &underlying_1,
                &reserve_config_1,
                &reserve_data_1,
            );

            let (underlying_2, _) = testutils::create_token_contract(&e, &bombadil);
            let (mut reserve_config_2, reserve_data_2) = testutils::default_reserve_meta();
            reserve_config_2.c_factor = 0_0000000;
            reserve_config_2.l_factor = 0_7000000;
            reserve_config_2.index = 2;
            testutils::create_reserve(
                &e,
                &pool_address,
                &underlying_2,
                &reserve_config_2,
                &reserve_data_2,
            );

            oracle_client.set_data(
                &bombadil,
                &Asset::Other(Symbol::new(&e, "USD")),
                &vec![
                    &e,
                    Asset::Stellar(underlying_0.clone()),
                    Asset::Stellar(underlying_1.clone()),
                    Asset::Stellar(underlying_2.clone()),
                ],
                &7,
                &300,
            );
            oracle_client.set_price_stable(&vec![&e, 2_0000000, 4_0000000, 50_0000000]);

            let liq_pct = 45;
            let positions: Positions = Positions {
                collateral: map![
                    &e,
                    (reserve_config_0.index, 90_9100000),
                    (reserve_config_1.index, 04_5800000),
                ],
                liabilities: map![&e, (reserve_config_2.index, 02_7500000),],
                supply: map![&e],
            };
            let pool_config = PoolConfig {
                oracle: oracle_address,
                min_collateral: 1_0000000,
                bstop_rate: 0_1000000,
                status: 0,
                max_positions: 2,
            };
            e.as_contract(&pool_address, || {
                storage::set_user_positions(&e, &samwise, &positions);
                storage::set_pool_config(&e, &pool_config);
                storage::set_backstop(&e, &backstop_address);

                create_user_liq_auction_data(
                    &e,
                    &samwise,
                    &vec![&e, underlying_2.clone()],
                    &vec![&e, underlying_0.clone(), underlying_1.clone()],
                    liq_pct,
                );
            });
        }

        #[test]
        fn test_create_user_liquidation_auction_normal_scalars() {
            let e = Env::default();

            e.mock_all_auths();
            e.ledger().set(LedgerInfo {
                timestamp: 12345,
                protocol_version: 22,
                sequence_number: 50,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 3110400,
            });

            let bombadil = Address::generate(&e);
            let samwise = Address::generate(&e);

            let pool_address = create_pool(&e);
            let (oracle_address, oracle_client) = testutils::create_mock_oracle(&e);
            let backstop_address = Address::generate(&e);

            // creating reserves for a pool exhausts the budget
            e.cost_estimate().budget().reset_unlimited();
            let (underlying_0, _) = testutils::create_token_contract(&e, &bombadil);
            let (mut reserve_config_0, mut reserve_data_0) = testutils::default_reserve_meta();
            reserve_data_0.last_time = 12345;
            reserve_data_0.b_rate = 1_100_000_000_000;
            reserve_config_0.c_factor = 0_8500000;
            reserve_config_0.l_factor = 0_9000000;
            reserve_config_0.index = 0;
            testutils::create_reserve(
                &e,
                &pool_address,
                &underlying_0,
                &reserve_config_0,
                &reserve_data_0,
            );

            let (underlying_1, _) = testutils::create_token_contract(&e, &bombadil);
            let (mut reserve_config_1, mut reserve_data_1) = testutils::default_reserve_meta();
            reserve_data_1.b_rate = 1_200_000_000_000;
            reserve_config_1.c_factor = 0_7500000;
            reserve_config_1.l_factor = 0_7500000;
            reserve_data_1.last_time = 12345;
            reserve_config_1.index = 1;
            testutils::create_reserve(
                &e,
                &pool_address,
                &underlying_1,
                &reserve_config_1,
                &reserve_data_1,
            );

            let (underlying_2, _) = testutils::create_token_contract(&e, &bombadil);
            let (mut reserve_config_2, reserve_data_2) = testutils::default_reserve_meta();
            reserve_config_2.c_factor = 0_0000000;
            reserve_config_2.l_factor = 0_7000000;
            reserve_config_2.index = 2;
            testutils::create_reserve(
                &e,
                &pool_address,
                &underlying_2,
                &reserve_config_2,
                &reserve_data_2,
            );

            oracle_client.set_data(
                &bombadil,
                &Asset::Other(Symbol::new(&e, "USD")),
                &vec![
                    &e,
                    Asset::Stellar(underlying_0.clone()),
                    Asset::Stellar(underlying_1.clone()),
                    Asset::Stellar(underlying_2.clone()),
                ],
                &7,
                &300,
            );
            oracle_client.set_price_stable(&vec![&e, 2_0000000, 4_0000000, 50_0000000]);

            let liq_pct = 45;
            let positions: Positions = Positions {
                collateral: map![
                    &e,
                    (reserve_config_0.index, 90_9100000),
                    (reserve_config_1.index, 04_5800000),
                ],
                liabilities: map![&e, (reserve_config_2.index, 02_7500000),],
                supply: map![&e],
            };
            let pool_config = PoolConfig {
                oracle: oracle_address,
                min_collateral: 1_0000000,
                bstop_rate: 0_1000000,
                status: 0,
                max_positions: 4,
            };
            e.as_contract(&pool_address, || {
                storage::set_user_positions(&e, &samwise, &positions);
                storage::set_pool_config(&e, &pool_config);
                storage::set_backstop(&e, &backstop_address);

                let result = create_user_liq_auction_data(
                    &e,
                    &samwise,
                    &vec![&e, underlying_2.clone()],
                    &vec![&e, underlying_0.clone(), underlying_1.clone()],
                    liq_pct,
                );
                assert_eq!(result.block, 51);
                assert_eq!(result.bid.get_unchecked(underlying_2), 1_2375000);
                assert_eq!(result.bid.len(), 1);
                assert_eq!(result.lot.get_unchecked(underlying_0), 30_5595329);
                assert_eq!(result.lot.get_unchecked(underlying_1), 1_5395739);
                assert_eq!(result.lot.len(), 2);
            });
        }

        #[test]
        fn test_create_user_liquidation_auction_weird_scalar() {
            let e = Env::default();

            e.mock_all_auths();
            e.ledger().set(LedgerInfo {
                timestamp: 12345,
                protocol_version: 22,
                sequence_number: 50,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 3110400,
            });

            let bombadil = Address::generate(&e);
            let samwise = Address::generate(&e);

            let pool_address = create_pool(&e);
            let (oracle_address, oracle_client) = testutils::create_mock_oracle(&e);
            let backstop_address = Address::generate(&e);

            // creating reserves for a pool exhausts the budget
            e.cost_estimate().budget().reset_unlimited();
            let (underlying_0, _) = testutils::create_token_contract(&e, &bombadil);
            let (mut reserve_config_0, mut reserve_data_0) = testutils::default_reserve_meta();
            reserve_data_0.last_time = 12345;
            reserve_data_0.b_rate = 1_000_206_159_000;
            reserve_config_0.c_factor = 0_9000000;
            reserve_config_0.l_factor = 0_9000000;
            reserve_config_0.index = 0;
            testutils::create_reserve(
                &e,
                &pool_address,
                &underlying_0,
                &reserve_config_0,
                &reserve_data_0,
            );

            let (underlying_1, _) = testutils::create_token_contract(&e, &bombadil);
            let (mut reserve_config_1, mut reserve_data_1) = testutils::default_reserve_meta();
            reserve_config_1.c_factor = 0_0000000;
            reserve_config_1.l_factor = 0_9000000;
            reserve_config_1.index = 1;
            reserve_data_1.d_rate = 1_000_201_748_000;
            testutils::create_reserve(
                &e,
                &pool_address,
                &underlying_1,
                &reserve_config_1,
                &reserve_data_1,
            );

            oracle_client.set_data(
                &bombadil,
                &Asset::Other(Symbol::new(&e, "USD")),
                &vec![
                    &e,
                    Asset::Stellar(underlying_0.clone()),
                    Asset::Stellar(underlying_1.clone()),
                ],
                &14,
                &300,
            );
            oracle_client.set_price_stable(&vec![&e, 1418501_2444444, 1_0261166_9700969]);

            let liq_pct = 69;
            let positions: Positions = Positions {
                collateral: map![&e, (reserve_config_0.index, 8999_1357639),],
                liabilities: map![&e, (reserve_config_1.index, 1059_5526742),],
                supply: map![&e],
            };
            let pool_config = PoolConfig {
                oracle: oracle_address,
                min_collateral: 1_0000000,
                bstop_rate: 0_1000000,
                status: 0,
                max_positions: 4,
            };
            e.as_contract(&pool_address, || {
                storage::set_user_positions(&e, &samwise, &positions);
                storage::set_pool_config(&e, &pool_config);
                storage::set_backstop(&e, &backstop_address);

                let result = create_user_liq_auction_data(
                    &e,
                    &samwise,
                    &vec![&e, underlying_1.clone()],
                    &vec![&e, underlying_0.clone()],
                    liq_pct,
                );
                assert_eq!(result.block, 51);
                assert_eq!(result.bid.get_unchecked(underlying_1), 731_0913452);
                assert_eq!(result.bid.len(), 1);
                assert_eq!(result.lot.get_unchecked(underlying_0), 5791_1010712);
                assert_eq!(result.lot.len(), 1);
            });
        }

        #[test]
        fn test_create_user_liquidation_auction_full_liquidation() {
            let e = Env::default();

            e.mock_all_auths();
            e.ledger().set(LedgerInfo {
                timestamp: 12345,
                protocol_version: 22,
                sequence_number: 50,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 3110400,
            });

            let bombadil = Address::generate(&e);
            let samwise = Address::generate(&e);

            let pool_address = create_pool(&e);
            let (oracle_address, oracle_client) = testutils::create_mock_oracle(&e);
            let backstop_address = Address::generate(&e);

            // creating reserves for a pool exhausts the budget
            e.cost_estimate().budget().reset_unlimited();
            let (underlying_0, _) = testutils::create_token_contract(&e, &bombadil);
            let (mut reserve_config_0, mut reserve_data_0) = testutils::default_reserve_meta();
            reserve_data_0.last_time = 12345;
            reserve_data_0.b_rate = 1_000_206_159_000;
            reserve_config_0.c_factor = 0_9000000;
            reserve_config_0.l_factor = 0_9000000;
            reserve_config_0.index = 0;
            testutils::create_reserve(
                &e,
                &pool_address,
                &underlying_0,
                &reserve_config_0,
                &reserve_data_0,
            );

            let (underlying_1, _) = testutils::create_token_contract(&e, &bombadil);
            let (mut reserve_config_1, mut reserve_data_1) = testutils::default_reserve_meta();
            reserve_config_1.c_factor = 0_0000000;
            reserve_config_1.l_factor = 0_9000000;
            reserve_config_1.index = 1;
            reserve_config_1.decimals = 6;
            reserve_data_1.d_rate = 1_000_201_748_000;
            testutils::create_reserve(
                &e,
                &pool_address,
                &underlying_1,
                &reserve_config_1,
                &reserve_data_1,
            );

            oracle_client.set_data(
                &bombadil,
                &Asset::Other(Symbol::new(&e, "USD")),
                &vec![
                    &e,
                    Asset::Stellar(underlying_0.clone()),
                    Asset::Stellar(underlying_1.clone()),
                ],
                &5,
                &300,
            );
            oracle_client.set_price_stable(&vec![&e, 1_00000, 1_00000]);

            let liq_pct = 100;
            let positions: Positions = Positions {
                collateral: map![&e, (reserve_config_0.index, 8_000_0000),],
                liabilities: map![&e, (reserve_config_1.index, 100_000_000),],
                supply: map![&e],
            };
            let pool_config = PoolConfig {
                oracle: oracle_address,
                min_collateral: 1_0000000,
                bstop_rate: 0_1000000,
                status: 0,
                max_positions: 4,
            };
            e.as_contract(&pool_address, || {
                storage::set_user_positions(&e, &samwise, &positions);
                storage::set_pool_config(&e, &pool_config);
                storage::set_backstop(&e, &backstop_address);

                let result = create_user_liq_auction_data(
                    &e,
                    &samwise,
                    &vec![&e, underlying_1.clone()],
                    &vec![&e, underlying_0.clone()],
                    liq_pct,
                );
                assert_eq!(result.block, 51);
                assert_eq!(result.bid.get_unchecked(underlying_1), 10_0000000);
                assert_eq!(result.bid.len(), 1);
                assert_eq!(result.lot.get_unchecked(underlying_0), 8_0000000);
                assert_eq!(result.lot.len(), 1);
            });
        }

        #[test]
        fn test_create_user_liquidation_auction_over_95_percent_liqs_fully() {
            let e = Env::default();
            e.mock_all_auths();
            e.ledger().set(LedgerInfo {
                timestamp: 12345,
                protocol_version: 22,
                sequence_number: 50,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 3110400,
            });
            e.cost_estimate().budget().reset_unlimited();

            let bombadil = Address::generate(&e);
            let samwise = Address::generate(&e);

            let pool_address = create_pool(&e);
            let (oracle_address, oracle_client) = testutils::create_mock_oracle(&e);
            let backstop_address = Address::generate(&e);

            let (underlying_0, _) = testutils::create_token_contract(&e, &bombadil);
            let (mut reserve_config_0, mut reserve_data_0) = testutils::default_reserve_meta();
            reserve_data_0.last_time = 12345;
            reserve_data_0.b_rate = 1_000_206_159_000;
            reserve_config_0.c_factor = 0_9000000;
            reserve_config_0.l_factor = 0_9000000;
            reserve_config_0.index = 0;
            testutils::create_reserve(
                &e,
                &pool_address,
                &underlying_0,
                &reserve_config_0,
                &reserve_data_0,
            );

            let (underlying_1, _) = testutils::create_token_contract(&e, &bombadil);
            let (mut reserve_config_1, mut reserve_data_1) = testutils::default_reserve_meta();
            reserve_config_1.c_factor = 0_5000000;
            reserve_config_1.l_factor = 0_8000000;
            reserve_config_1.index = 1;
            reserve_data_1.d_rate = 1_050_001_748_000;
            testutils::create_reserve(
                &e,
                &pool_address,
                &underlying_1,
                &reserve_config_1,
                &reserve_data_1,
            );

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

            let liq_pct = 96;
            // true liquidation percent between 99-100%
            let positions: Positions = Positions {
                collateral: map![
                    &e,
                    (reserve_config_1.index, 75_500_0000),
                    (reserve_config_0.index, 50_000_0000)
                ],
                liabilities: map![
                    &e,
                    (reserve_config_1.index, 50_000_0000),
                    (reserve_config_0.index, 50_000_0000)
                ],
                supply: map![&e],
            };
            let pool_config = PoolConfig {
                oracle: oracle_address,
                min_collateral: 1_0000000,
                bstop_rate: 0_1000000,
                status: 0,
                max_positions: 4,
            };
            e.as_contract(&pool_address, || {
                storage::set_user_positions(&e, &samwise, &positions);
                storage::set_pool_config(&e, &pool_config);
                storage::set_backstop(&e, &backstop_address);

                let result = create_user_liq_auction_data(
                    &e,
                    &samwise,
                    &vec![&e, underlying_0.clone(), underlying_1.clone()],
                    &vec![&e, underlying_0.clone(), underlying_1.clone()],
                    liq_pct,
                );
                assert_eq!(result.block, 51);
                assert_eq!(result.bid.get_unchecked(underlying_1.clone()), 50_000_0000);
                assert_eq!(result.bid.get_unchecked(underlying_0.clone()), 50_000_0000);
                assert_eq!(result.bid.len(), 2);
                assert_eq!(result.lot.get_unchecked(underlying_1.clone()), 75_500_0000);
                assert_eq!(result.lot.get_unchecked(underlying_0.clone()), 50_000_0000);
                assert_eq!(result.lot.len(), 2);
            });
        }

        #[test]
        fn test_create_user_liquidation_auction_95_safe_can_liq_fully() {
            let e = Env::default();
            e.mock_all_auths();
            e.ledger().set(LedgerInfo {
                timestamp: 12345,
                protocol_version: 22,
                sequence_number: 50,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 3110400,
            });
            e.cost_estimate().budget().reset_unlimited();

            let bombadil = Address::generate(&e);
            let samwise = Address::generate(&e);

            let pool_address = create_pool(&e);
            let (oracle_address, oracle_client) = testutils::create_mock_oracle(&e);
            let backstop_address = Address::generate(&e);

            let (underlying_0, _) = testutils::create_token_contract(&e, &bombadil);
            let (mut reserve_config_0, mut reserve_data_0) = testutils::default_reserve_meta();
            reserve_data_0.last_time = 12345;
            reserve_data_0.b_rate = 1_100_000_000_000;
            reserve_data_0.d_rate = 1_150_000_000_000;
            reserve_config_0.c_factor = 0_8500000;
            reserve_config_0.l_factor = 0_9000000;
            reserve_config_0.index = 0;
            testutils::create_reserve(
                &e,
                &pool_address,
                &underlying_0,
                &reserve_config_0,
                &reserve_data_0,
            );

            let (underlying_1, _) = testutils::create_token_contract(&e, &bombadil);
            let (mut reserve_config_1, mut reserve_data_1) = testutils::default_reserve_meta();
            reserve_data_1.last_time = 12345;
            reserve_data_1.b_rate = 1_200_000_000_000;
            reserve_data_1.d_rate = 1_300_000_000_000;
            reserve_config_1.c_factor = 0_8000000;
            reserve_config_1.l_factor = 0_7500000;
            reserve_config_1.index = 1;
            testutils::create_reserve(
                &e,
                &pool_address,
                &underlying_1,
                &reserve_config_1,
                &reserve_data_1,
            );

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
            oracle_client.set_price_stable(&vec![&e, 2_0000000, 4_0000000]);

            // 95% liquidation results in a hf of 1.147
            let positions: Positions = Positions {
                collateral: map![
                    &e,
                    (reserve_config_0.index, 100_0000000),
                    (reserve_config_1.index, 100_0000000)
                ],
                liabilities: map![
                    &e,
                    (reserve_config_0.index, 82_7500000),
                    (reserve_config_1.index, 75_0000000)
                ],
                supply: map![&e],
            };
            let pool_config = PoolConfig {
                oracle: oracle_address,
                min_collateral: 1_0000000,
                bstop_rate: 0_1000000,
                status: 0,
                max_positions: 4,
            };
            e.as_contract(&pool_address, || {
                storage::set_user_positions(&e, &samwise, &positions);
                storage::set_pool_config(&e, &pool_config);
                storage::set_backstop(&e, &backstop_address);

                // validate 95% liquidation is valid
                let result_95 = create_user_liq_auction_data(
                    &e,
                    &samwise,
                    &vec![&e, underlying_0.clone(), underlying_1.clone()],
                    &vec![&e, underlying_0.clone(), underlying_1.clone()],
                    95,
                );
                assert_eq!(result_95.block, 51);
                assert_eq!(
                    result_95.bid.get_unchecked(underlying_0.clone()),
                    78_6125000
                );
                assert_eq!(
                    result_95.bid.get_unchecked(underlying_1.clone()),
                    71_2500000
                );
                assert_eq!(result_95.bid.len(), 2);
                assert_eq!(
                    result_95.lot.get_unchecked(underlying_0.clone()),
                    92_6529600
                );
                assert_eq!(
                    result_95.lot.get_unchecked(underlying_1.clone()),
                    92_6529600
                );
                assert_eq!(result_95.lot.len(), 2);

                // validate if 95% is valid, a full liquidation can be completed
                let result_100 = create_user_liq_auction_data(
                    &e,
                    &samwise,
                    &vec![&e, underlying_0.clone(), underlying_1.clone()],
                    &vec![&e, underlying_0.clone(), underlying_1.clone()],
                    100,
                );
                assert_eq!(result_100.block, 51);
                assert_eq!(
                    result_100.bid.get_unchecked(underlying_0.clone()),
                    82_7500000
                );
                assert_eq!(
                    result_100.bid.get_unchecked(underlying_1.clone()),
                    75_0000000
                );
                assert_eq!(result_100.bid.len(), 2);
                assert_eq!(
                    result_100.lot.get_unchecked(underlying_0.clone()),
                    100_0000000
                );
                assert_eq!(
                    result_100.lot.get_unchecked(underlying_1.clone()),
                    100_0000000
                );
                assert_eq!(result_100.lot.len(), 2);
            });
        }

        #[test]
        #[should_panic(expected = "Error(Contract, #1213)")]
        fn test_create_user_liquidation_auction_bad_full_liq() {
            let e = Env::default();

            e.mock_all_auths();
            e.ledger().set(LedgerInfo {
                timestamp: 12345,
                protocol_version: 22,
                sequence_number: 50,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 3110400,
            });

            let bombadil = Address::generate(&e);
            let samwise = Address::generate(&e);

            let pool_address = create_pool(&e);

            let (oracle_address, oracle_client) = testutils::create_mock_oracle(&e);
            let backstop_address = Address::generate(&e);

            // creating reserves for a pool exhausts the budget
            e.cost_estimate().budget().reset_unlimited();
            let (underlying_0, _) = testutils::create_token_contract(&e, &bombadil);
            let (mut reserve_config_0, mut reserve_data_0) = testutils::default_reserve_meta();
            reserve_data_0.last_time = 12345;
            reserve_data_0.b_rate = 1_100_000_000_000;
            reserve_config_0.c_factor = 0_8500000;
            reserve_config_0.l_factor = 0_9000000;
            reserve_config_0.index = 0;
            testutils::create_reserve(
                &e,
                &pool_address,
                &underlying_0,
                &reserve_config_0,
                &reserve_data_0,
            );

            let (underlying_1, _) = testutils::create_token_contract(&e, &bombadil);
            let (mut reserve_config_1, mut reserve_data_1) = testutils::default_reserve_meta();
            reserve_data_1.b_rate = 1_200_000_000_000;
            reserve_config_1.c_factor = 0_7500000;
            reserve_config_1.l_factor = 0_7500000;
            reserve_data_1.last_time = 12345;
            reserve_config_1.index = 1;
            testutils::create_reserve(
                &e,
                &pool_address,
                &underlying_1,
                &reserve_config_1,
                &reserve_data_1,
            );

            let (underlying_2, _) = testutils::create_token_contract(&e, &bombadil);
            let (mut reserve_config_2, reserve_data_2) = testutils::default_reserve_meta();
            reserve_config_2.c_factor = 0_0000000;
            reserve_config_2.l_factor = 0_7000000;
            reserve_config_2.index = 2;
            testutils::create_reserve(
                &e,
                &pool_address,
                &underlying_2,
                &reserve_config_2,
                &reserve_data_2,
            );

            oracle_client.set_data(
                &bombadil,
                &Asset::Other(Symbol::new(&e, "USD")),
                &vec![
                    &e,
                    Asset::Stellar(underlying_0.clone()),
                    Asset::Stellar(underlying_1.clone()),
                    Asset::Stellar(underlying_2.clone()),
                ],
                &8,
                &300,
            );
            oracle_client.set_price_stable(&vec![&e, 2_0000000_0, 4_0000000_0, 50_0000000_0]);

            let liq_pct = 100;
            let pool_config = PoolConfig {
                oracle: oracle_address,
                min_collateral: 1_0000000,
                bstop_rate: 0_1000000,
                status: 0,
                max_positions: 4,
            };
            let positions: Positions = Positions {
                collateral: map![
                    &e,
                    (reserve_config_0.index, 90_9100000),
                    (reserve_config_1.index, 04_5800000),
                ],
                liabilities: map![&e, (reserve_config_2.index, 02_7500000),],
                supply: map![&e],
            };
            e.as_contract(&pool_address, || {
                storage::set_user_positions(&e, &samwise, &positions);
                storage::set_pool_config(&e, &pool_config);
                storage::set_backstop(&e, &backstop_address);

                create_user_liq_auction_data(
                    &e,
                    &samwise,
                    &vec![&e, underlying_2.clone()],
                    &vec![&e, underlying_0.clone(), underlying_1.clone()],
                    liq_pct,
                );
            });
        }

        #[test]
        #[should_panic(expected = "Error(Contract, #1213)")]
        fn test_create_user_liquidation_auction_too_large() {
            let e = Env::default();

            e.mock_all_auths();
            e.ledger().set(LedgerInfo {
                timestamp: 12345,
                protocol_version: 22,
                sequence_number: 50,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 3110400,
            });

            let bombadil = Address::generate(&e);
            let samwise = Address::generate(&e);

            let pool_address = create_pool(&e);

            let (oracle_address, oracle_client) = testutils::create_mock_oracle(&e);
            let backstop_address = Address::generate(&e);

            // creating reserves for a pool exhausts the budget
            e.cost_estimate().budget().reset_unlimited();
            let (underlying_0, _) = testutils::create_token_contract(&e, &bombadil);
            let (mut reserve_config_0, mut reserve_data_0) = testutils::default_reserve_meta();
            reserve_data_0.last_time = 12345;
            reserve_data_0.b_rate = 1_100_000_000_000;
            reserve_config_0.c_factor = 0_8500000;
            reserve_config_0.l_factor = 0_9000000;
            reserve_config_0.index = 0;
            testutils::create_reserve(
                &e,
                &pool_address,
                &underlying_0,
                &reserve_config_0,
                &reserve_data_0,
            );

            let (underlying_1, _) = testutils::create_token_contract(&e, &bombadil);
            let (mut reserve_config_1, mut reserve_data_1) = testutils::default_reserve_meta();
            reserve_data_1.b_rate = 1_200_000_000_000;
            reserve_config_1.c_factor = 0_7500000;
            reserve_config_1.l_factor = 0_7500000;
            reserve_data_1.last_time = 12345;
            reserve_config_1.index = 1;
            testutils::create_reserve(
                &e,
                &pool_address,
                &underlying_1,
                &reserve_config_1,
                &reserve_data_1,
            );

            let (underlying_2, _) = testutils::create_token_contract(&e, &bombadil);
            let (mut reserve_config_2, reserve_data_2) = testutils::default_reserve_meta();
            reserve_config_2.c_factor = 0_0000000;
            reserve_config_2.l_factor = 0_7000000;
            reserve_config_2.index = 2;
            testutils::create_reserve(
                &e,
                &pool_address,
                &underlying_2,
                &reserve_config_2,
                &reserve_data_2,
            );

            oracle_client.set_data(
                &bombadil,
                &Asset::Other(Symbol::new(&e, "USD")),
                &vec![
                    &e,
                    Asset::Stellar(underlying_0.clone()),
                    Asset::Stellar(underlying_1.clone()),
                    Asset::Stellar(underlying_2.clone()),
                ],
                &6,
                &300,
            );
            oracle_client.set_price_stable(&vec![&e, 2_000000, 4_000000, 50_000000]);

            let liq_pct = 46;
            let pool_config = PoolConfig {
                oracle: oracle_address,
                min_collateral: 1_0000000,
                bstop_rate: 0_1000000,
                status: 0,
                max_positions: 4,
            };
            let positions: Positions = Positions {
                collateral: map![
                    &e,
                    (reserve_config_0.index, 90_9100000),
                    (reserve_config_1.index, 04_5800000),
                ],
                liabilities: map![&e, (reserve_config_2.index, 02_7500000),],
                supply: map![&e],
            };
            e.as_contract(&pool_address, || {
                storage::set_user_positions(&e, &samwise, &positions);
                storage::set_pool_config(&e, &pool_config);
                storage::set_backstop(&e, &backstop_address);

                create_user_liq_auction_data(
                    &e,
                    &samwise,
                    &vec![&e, underlying_2.clone()],
                    &vec![&e, underlying_0.clone(), underlying_1.clone()],
                    liq_pct,
                );
            });
        }

        #[test]
        #[should_panic(expected = "Error(Contract, #1214)")]
        fn test_create_user_liquidation_auction_too_small() {
            let e = Env::default();

            e.mock_all_auths();
            e.ledger().set(LedgerInfo {
                timestamp: 12345,
                protocol_version: 22,
                sequence_number: 50,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 3110400,
            });

            let bombadil = Address::generate(&e);
            let samwise = Address::generate(&e);

            let pool_address = create_pool(&e);

            let (oracle_address, oracle_client) = testutils::create_mock_oracle(&e);
            let backstop_address = Address::generate(&e);

            // creating reserves for a pool exhausts the budget
            e.cost_estimate().budget().reset_unlimited();
            let (underlying_0, _) = testutils::create_token_contract(&e, &bombadil);
            let (mut reserve_config_0, mut reserve_data_0) = testutils::default_reserve_meta();
            reserve_data_0.last_time = 12345;
            reserve_data_0.b_rate = 1_100_000_000_000;
            reserve_config_0.c_factor = 0_8500000;
            reserve_config_0.l_factor = 0_9000000;
            reserve_config_0.index = 0;
            testutils::create_reserve(
                &e,
                &pool_address,
                &underlying_0,
                &reserve_config_0,
                &reserve_data_0,
            );

            let (underlying_1, _) = testutils::create_token_contract(&e, &bombadil);
            let (mut reserve_config_1, mut reserve_data_1) = testutils::default_reserve_meta();
            reserve_data_1.b_rate = 1_200_000_000_000;
            reserve_config_1.c_factor = 0_7500000;
            reserve_config_1.l_factor = 0_7500000;
            reserve_data_1.last_time = 12345;
            reserve_config_1.index = 1;
            testutils::create_reserve(
                &e,
                &pool_address,
                &underlying_1,
                &reserve_config_1,
                &reserve_data_1,
            );

            let (underlying_2, _) = testutils::create_token_contract(&e, &bombadil);
            let (mut reserve_config_2, reserve_data_2) = testutils::default_reserve_meta();
            reserve_config_2.c_factor = 0_0000000;
            reserve_config_2.l_factor = 0_7000000;
            reserve_config_2.index = 2;
            testutils::create_reserve(
                &e,
                &pool_address,
                &underlying_2,
                &reserve_config_2,
                &reserve_data_2,
            );

            oracle_client.set_data(
                &bombadil,
                &Asset::Other(Symbol::new(&e, "USD")),
                &vec![
                    &e,
                    Asset::Stellar(underlying_0.clone()),
                    Asset::Stellar(underlying_1.clone()),
                    Asset::Stellar(underlying_2.clone()),
                ],
                &5,
                &300,
            );
            oracle_client.set_price_stable(&vec![&e, 2_00000, 4_00000, 50_00000]);

            let liq_pct = 25;
            let pool_config = PoolConfig {
                oracle: oracle_address,
                min_collateral: 1_0000000,
                bstop_rate: 0_1000000,
                status: 0,
                max_positions: 4,
            };
            let positions: Positions = Positions {
                collateral: map![
                    &e,
                    (reserve_config_0.index, 90_9100000),
                    (reserve_config_1.index, 04_5800000),
                ],
                liabilities: map![&e, (reserve_config_2.index, 02_7500000),],
                supply: map![&e],
            };
            e.as_contract(&pool_address, || {
                storage::set_user_positions(&e, &samwise, &positions);
                storage::set_pool_config(&e, &pool_config);
                storage::set_backstop(&e, &backstop_address);

                create_user_liq_auction_data(
                    &e,
                    &samwise,
                    &vec![&e, underlying_2.clone()],
                    &vec![&e, underlying_0.clone(), underlying_1.clone()],
                    liq_pct,
                );
            });
        }

        #[test]
        fn test_create_user_liquidation_partial() {
            let e = Env::default();
            e.mock_all_auths();
            e.cost_estimate().budget().reset_unlimited();

            e.ledger().set(LedgerInfo {
                timestamp: 12345,
                protocol_version: 22,
                sequence_number: 50,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 3110400,
            });

            let bombadil = Address::generate(&e);
            let samwise = Address::generate(&e);

            let pool_address = create_pool(&e);
            let (oracle_address, oracle_client) = testutils::create_mock_oracle(&e);
            let backstop_address = Address::generate(&e);

            let (underlying_0, _) = testutils::create_token_contract(&e, &bombadil);
            let (mut reserve_config_0, mut reserve_data_0) = testutils::default_reserve_meta();
            reserve_data_0.last_time = 12345;
            reserve_data_0.b_rate = 1_100_000_000_000;
            reserve_data_0.d_rate = 1_150_000_000_000;
            reserve_config_0.c_factor = 0_8500000;
            reserve_config_0.l_factor = 0_9000000;
            reserve_config_0.index = 0;
            testutils::create_reserve(
                &e,
                &pool_address,
                &underlying_0,
                &reserve_config_0,
                &reserve_data_0,
            );

            let (underlying_1, _) = testutils::create_token_contract(&e, &bombadil);
            let (mut reserve_config_1, mut reserve_data_1) = testutils::default_reserve_meta();
            reserve_data_1.last_time = 12345;
            reserve_data_1.b_rate = 1_200_000_000_000;
            reserve_data_1.d_rate = 1_300_000_000_000;
            reserve_config_1.c_factor = 0_8000000;
            reserve_config_1.l_factor = 0_7500000;
            reserve_config_1.index = 1;
            testutils::create_reserve(
                &e,
                &pool_address,
                &underlying_1,
                &reserve_config_1,
                &reserve_data_1,
            );

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
            oracle_client.set_price_stable(&vec![&e, 2_0000000, 4_0000000]);

            let liq_pct = 85;
            let positions: Positions = Positions {
                collateral: map![
                    &e,
                    (reserve_config_0.index, 50_0000000),
                    (reserve_config_1.index, 30_0000000),
                ],
                liabilities: map![
                    &e,
                    (reserve_config_0.index, 30_0000000),
                    (reserve_config_1.index, 20_0000000),
                ],
                supply: map![&e],
            };
            let pool_config = PoolConfig {
                oracle: oracle_address,
                min_collateral: 1_0000000,
                bstop_rate: 0_1000000,
                status: 0,
                max_positions: 4,
            };
            e.as_contract(&pool_address, || {
                storage::set_user_positions(&e, &samwise, &positions);
                storage::set_pool_config(&e, &pool_config);
                storage::set_backstop(&e, &backstop_address);

                let result = create_user_liq_auction_data(
                    &e,
                    &samwise,
                    &vec![&e, underlying_0.clone()],
                    &vec![&e, underlying_1.clone()],
                    liq_pct,
                );

                assert_eq!(result.block, 51);
                assert_eq!(result.bid.get_unchecked(underlying_0.clone()), 25_5000000);
                assert_eq!(result.bid.len(), 1);
                assert_eq!(result.lot.get_unchecked(underlying_1.clone()), 13_9293750);
                assert_eq!(result.lot.len(), 1);
            });
        }

        #[test]
        fn test_create_user_liquidation_partial_100() {
            let e = Env::default();
            e.mock_all_auths();
            e.cost_estimate().budget().reset_unlimited();

            e.ledger().set(LedgerInfo {
                timestamp: 12345,
                protocol_version: 22,
                sequence_number: 50,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 3110400,
            });

            let bombadil = Address::generate(&e);
            let samwise = Address::generate(&e);

            let pool_address = create_pool(&e);
            let (oracle_address, oracle_client) = testutils::create_mock_oracle(&e);
            let backstop_address = Address::generate(&e);

            let (underlying_0, _) = testutils::create_token_contract(&e, &bombadil);
            let (mut reserve_config_0, mut reserve_data_0) = testutils::default_reserve_meta();
            reserve_data_0.last_time = 12345;
            reserve_data_0.b_rate = 1_100_000_000_000;
            reserve_data_0.d_rate = 1_150_000_000_000;
            reserve_config_0.c_factor = 0_8500000;
            reserve_config_0.l_factor = 0_9000000;
            reserve_config_0.index = 0;
            testutils::create_reserve(
                &e,
                &pool_address,
                &underlying_0,
                &reserve_config_0,
                &reserve_data_0,
            );

            let (underlying_1, _) = testutils::create_token_contract(&e, &bombadil);
            let (mut reserve_config_1, mut reserve_data_1) = testutils::default_reserve_meta();
            reserve_data_1.last_time = 12345;
            reserve_data_1.b_rate = 1_200_000_000_000;
            reserve_data_1.d_rate = 1_300_000_000_000;
            reserve_config_1.c_factor = 0_8000000;
            reserve_config_1.l_factor = 0_7500000;
            reserve_config_1.index = 1;
            testutils::create_reserve(
                &e,
                &pool_address,
                &underlying_1,
                &reserve_config_1,
                &reserve_data_1,
            );

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
            oracle_client.set_price_stable(&vec![&e, 2_0000000, 4_0000000]);

            // liquidation can be safely filled by just liquidating a single liability
            // validate the collateral is auctioned correctly to create a fair liquidation
            // -> including the full position results in an ~60% liquidation, and a slightly larger
            //    liquidation overall due to the higher liability factor of reserve 1
            let liq_pct = 100;
            let positions: Positions = Positions {
                collateral: map![&e, (reserve_config_0.index, 100_0000000),],
                liabilities: map![
                    &e,
                    (reserve_config_0.index, 40_0000000),
                    (reserve_config_1.index, 15_0000000),
                ],
                supply: map![&e],
            };
            let pool_config = PoolConfig {
                oracle: oracle_address,
                min_collateral: 1_0000000,
                bstop_rate: 0_1000000,
                status: 0,
                max_positions: 4,
            };
            e.as_contract(&pool_address, || {
                storage::set_user_positions(&e, &samwise, &positions);
                storage::set_pool_config(&e, &pool_config);
                storage::set_backstop(&e, &backstop_address);

                let result = create_user_liq_auction_data(
                    &e,
                    &samwise,
                    &vec![&e, underlying_1.clone()],
                    &vec![&e, underlying_0.clone()],
                    liq_pct,
                );

                assert_eq!(result.block, 51);
                assert_eq!(result.bid.get_unchecked(underlying_1.clone()), 15_0000000);
                assert_eq!(result.bid.len(), 1);
                assert_eq!(result.lot.get_unchecked(underlying_0.clone()), 41_8806900);
                assert_eq!(result.lot.len(), 1);
            });
        }

        #[test]
        fn test_create_user_liquidation_partial_0_cf_lot() {
            let e = Env::default();
            e.mock_all_auths();
            e.cost_estimate().budget().reset_unlimited();

            e.ledger().set(LedgerInfo {
                timestamp: 12345,
                protocol_version: 22,
                sequence_number: 50,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 3110400,
            });

            let bombadil = Address::generate(&e);
            let samwise = Address::generate(&e);

            let pool_address = create_pool(&e);
            let (oracle_address, oracle_client) = testutils::create_mock_oracle(&e);
            let backstop_address = Address::generate(&e);

            let (underlying_0, _) = testutils::create_token_contract(&e, &bombadil);
            let (mut reserve_config_0, mut reserve_data_0) = testutils::default_reserve_meta();
            reserve_data_0.last_time = 12345;
            reserve_data_0.b_rate = 1_100_000_000_000;
            reserve_data_0.d_rate = 1_150_000_000_000;
            reserve_config_0.c_factor = 0_8500000;
            reserve_config_0.l_factor = 0_9000000;
            reserve_config_0.index = 0;
            testutils::create_reserve(
                &e,
                &pool_address,
                &underlying_0,
                &reserve_config_0,
                &reserve_data_0,
            );

            let (underlying_1, _) = testutils::create_token_contract(&e, &bombadil);
            let (mut reserve_config_1, mut reserve_data_1) = testutils::default_reserve_meta();
            reserve_data_1.last_time = 12345;
            reserve_data_1.b_rate = 1_200_000_000_000;
            reserve_data_1.d_rate = 1_300_000_000_000;
            reserve_config_1.c_factor = 0;
            reserve_config_1.l_factor = 0_7500000;
            reserve_config_1.index = 1;
            testutils::create_reserve(
                &e,
                &pool_address,
                &underlying_1,
                &reserve_config_1,
                &reserve_data_1,
            );

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
            oracle_client.set_price_stable(&vec![&e, 2_0000000, 4_0000000]);

            // validate a liquidation of only 0 CF assets is valid
            // -> asset 1 has 0 CF
            let liq_pct = 15;
            let positions: Positions = Positions {
                collateral: map![
                    &e,
                    (reserve_config_0.index, 100_0000000),
                    (reserve_config_1.index, 100_0000000),
                ],
                liabilities: map![&e, (reserve_config_0.index, 80_0000000),],
                supply: map![&e],
            };
            let pool_config = PoolConfig {
                oracle: oracle_address,
                min_collateral: 1_0000000,
                bstop_rate: 0_1000000,
                status: 0,
                max_positions: 4,
            };
            e.as_contract(&pool_address, || {
                storage::set_user_positions(&e, &samwise, &positions);
                storage::set_pool_config(&e, &pool_config);
                storage::set_backstop(&e, &backstop_address);

                let result = create_user_liq_auction_data(
                    &e,
                    &samwise,
                    &vec![&e, underlying_0.clone()],
                    &vec![&e, underlying_1.clone()],
                    liq_pct,
                );

                assert_eq!(result.block, 51);
                assert_eq!(result.bid.get_unchecked(underlying_0.clone()), 12_0000000);
                assert_eq!(result.bid.len(), 1);
                assert_eq!(result.lot.get_unchecked(underlying_1.clone()), 8_6250000);
                assert_eq!(result.lot.len(), 1);
            });
        }

        #[test]
        #[should_panic(expected = "Error(Contract, #1211)")]
        fn test_create_user_liquidation_partial_exclude_collateral_when_required_panics() {
            let e = Env::default();
            e.mock_all_auths();
            e.cost_estimate().budget().reset_unlimited();

            e.ledger().set(LedgerInfo {
                timestamp: 12345,
                protocol_version: 22,
                sequence_number: 50,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 3110400,
            });

            let bombadil = Address::generate(&e);
            let samwise = Address::generate(&e);

            let pool_address = create_pool(&e);
            let (oracle_address, oracle_client) = testutils::create_mock_oracle(&e);
            let backstop_address = Address::generate(&e);

            let (underlying_0, _) = testutils::create_token_contract(&e, &bombadil);
            let (mut reserve_config_0, mut reserve_data_0) = testutils::default_reserve_meta();
            reserve_data_0.last_time = 12345;
            reserve_data_0.b_rate = 1_100_000_000_000;
            reserve_data_0.d_rate = 1_150_000_000_000;
            reserve_config_0.c_factor = 0_8500000;
            reserve_config_0.l_factor = 0_9000000;
            reserve_config_0.index = 0;
            testutils::create_reserve(
                &e,
                &pool_address,
                &underlying_0,
                &reserve_config_0,
                &reserve_data_0,
            );

            let (underlying_1, _) = testutils::create_token_contract(&e, &bombadil);
            let (mut reserve_config_1, mut reserve_data_1) = testutils::default_reserve_meta();
            reserve_data_1.last_time = 12345;
            reserve_data_1.b_rate = 1_200_000_000_000;
            reserve_data_1.d_rate = 1_300_000_000_000;
            reserve_config_1.c_factor = 0_8000000;
            reserve_config_1.l_factor = 0_7500000;
            reserve_config_1.index = 1;
            testutils::create_reserve(
                &e,
                &pool_address,
                &underlying_1,
                &reserve_config_1,
                &reserve_data_1,
            );

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
            oracle_client.set_price_stable(&vec![&e, 2_0000000, 4_0000000]);

            // user holds most collateral in asset 0
            // liquidator attempts to create a bad liquidation by excluding asset 0
            // causing excess liabilities to be included in the bid
            let liq_pct = 40;
            let positions: Positions = Positions {
                collateral: map![
                    &e,
                    (reserve_config_0.index, 60_0000000),
                    (reserve_config_1.index, 10_0000000),
                ],
                liabilities: map![&e, (reserve_config_1.index, 25_0000000),],
                supply: map![&e],
            };
            let pool_config = PoolConfig {
                oracle: oracle_address,
                min_collateral: 1_0000000,
                bstop_rate: 0_1000000,
                status: 0,
                max_positions: 4,
            };
            e.as_contract(&pool_address, || {
                storage::set_user_positions(&e, &samwise, &positions);
                storage::set_pool_config(&e, &pool_config);
                storage::set_backstop(&e, &backstop_address);

                create_user_liq_auction_data(
                    &e,
                    &samwise,
                    &vec![&e, underlying_1.clone()],
                    &vec![&e, underlying_1.clone()],
                    liq_pct,
                );
            });
        }

        #[test]
        #[should_panic(expected = "Error(Contract, #1214)")]
        fn test_create_user_liquidation_partial_exclude_liabilities_when_required_too_small() {
            let e = Env::default();
            e.mock_all_auths();
            e.cost_estimate().budget().reset_unlimited();

            e.ledger().set(LedgerInfo {
                timestamp: 12345,
                protocol_version: 22,
                sequence_number: 50,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 3110400,
            });

            let bombadil = Address::generate(&e);
            let samwise = Address::generate(&e);

            let pool_address = create_pool(&e);
            let (oracle_address, oracle_client) = testutils::create_mock_oracle(&e);
            let backstop_address = Address::generate(&e);

            let (underlying_0, _) = testutils::create_token_contract(&e, &bombadil);
            let (mut reserve_config_0, mut reserve_data_0) = testutils::default_reserve_meta();
            reserve_data_0.last_time = 12345;
            reserve_data_0.b_rate = 1_100_000_000_000;
            reserve_data_0.d_rate = 1_150_000_000_000;
            reserve_config_0.c_factor = 0_8500000;
            reserve_config_0.l_factor = 0_9000000;
            reserve_config_0.index = 0;
            testutils::create_reserve(
                &e,
                &pool_address,
                &underlying_0,
                &reserve_config_0,
                &reserve_data_0,
            );

            let (underlying_1, _) = testutils::create_token_contract(&e, &bombadil);
            let (mut reserve_config_1, mut reserve_data_1) = testutils::default_reserve_meta();
            reserve_data_1.last_time = 12345;
            reserve_data_1.b_rate = 1_200_000_000_000;
            reserve_data_1.d_rate = 1_300_000_000_000;
            reserve_config_1.c_factor = 0_8000000;
            reserve_config_1.l_factor = 0_7500000;
            reserve_config_1.index = 1;
            testutils::create_reserve(
                &e,
                &pool_address,
                &underlying_1,
                &reserve_config_1,
                &reserve_data_1,
            );

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
            oracle_client.set_price_stable(&vec![&e, 2_0000000, 4_0000000]);

            // user holds most liabilities in asset 1
            // liquidator attempts to create a bad liquidation by excluding asset 1 from bid
            let liq_pct = 100;
            let positions: Positions = Positions {
                collateral: map![&e, (reserve_config_0.index, 100_0000000),],
                liabilities: map![
                    &e,
                    (reserve_config_0.index, 10_0000000),
                    (reserve_config_1.index, 25_0000000),
                ],
                supply: map![&e],
            };
            let pool_config = PoolConfig {
                oracle: oracle_address,
                min_collateral: 1_0000000,
                bstop_rate: 0_1000000,
                status: 0,
                max_positions: 4,
            };
            e.as_contract(&pool_address, || {
                storage::set_user_positions(&e, &samwise, &positions);
                storage::set_pool_config(&e, &pool_config);
                storage::set_backstop(&e, &backstop_address);

                create_user_liq_auction_data(
                    &e,
                    &samwise,
                    &vec![&e, underlying_0.clone()],
                    &vec![&e, underlying_0.clone()],
                    liq_pct,
                );
            });
        }

        #[test]
        #[should_panic(expected = "Error(Contract, #1211)")]
        fn test_create_user_liquidation_requires_unhealthy_user() {
            let e = Env::default();
            e.cost_estimate().budget().reset_unlimited();

            e.mock_all_auths();
            e.ledger().set(LedgerInfo {
                timestamp: 12345,
                protocol_version: 22,
                sequence_number: 50,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 3110400,
            });

            let bombadil = Address::generate(&e);
            let samwise = Address::generate(&e);

            let pool_address = create_pool(&e);
            let (oracle_address, oracle_client) = testutils::create_mock_oracle(&e);
            let backstop_address = Address::generate(&e);

            // setup reserves to make it simple to have collateral_base == liabilities_base
            let (underlying_0, _) = testutils::create_token_contract(&e, &bombadil);
            let (mut reserve_config_0, mut reserve_data_0) = testutils::default_reserve_meta();
            reserve_config_0.c_factor = 1_0000000;
            reserve_config_0.l_factor = 1_0000000;
            reserve_data_0.last_time = 12345;
            reserve_config_0.index = 0;
            testutils::create_reserve(
                &e,
                &pool_address,
                &underlying_0,
                &reserve_config_0,
                &reserve_data_0,
            );

            let (underlying_1, _) = testutils::create_token_contract(&e, &bombadil);
            let (mut reserve_config_1, mut reserve_data_1) = testutils::default_reserve_meta();
            reserve_config_1.c_factor = 1_0000000;
            reserve_config_1.l_factor = 1_0000000;
            reserve_data_1.last_time = 12345;
            reserve_config_1.index = 1;
            testutils::create_reserve(
                &e,
                &pool_address,
                &underlying_1,
                &reserve_config_1,
                &reserve_data_1,
            );

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
            oracle_client.set_price_stable(&vec![&e, 2_0000000, 4_0000000]);

            let liq_pct = 45;
            let positions: Positions = Positions {
                collateral: map![&e, (0, 10_0000000),],
                liabilities: map![&e, (1, 5_0000000),],
                supply: map![&e],
            };
            let pool_config = PoolConfig {
                oracle: oracle_address,
                min_collateral: 1_0000000,
                bstop_rate: 0_1000000,
                status: 0,
                max_positions: 4,
            };
            e.as_contract(&pool_address, || {
                storage::set_user_positions(&e, &samwise, &positions);
                storage::set_pool_config(&e, &pool_config);
                storage::set_backstop(&e, &backstop_address);

                create_user_liq_auction_data(
                    &e,
                    &samwise,
                    &vec![&e, underlying_1.clone()],
                    &vec![&e, underlying_0.clone()],
                    liq_pct,
                );
            });
        }

        #[test]
        fn test_fill_user_liquidation_auction() {
            let e = Env::default();

            e.mock_all_auths();
            e.ledger().set(LedgerInfo {
                timestamp: 12345,
                protocol_version: 22,
                sequence_number: 175,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 17280,
                min_persistent_entry_ttl: 17280,
                max_entry_ttl: 9999999,
            });

            let bombadil = Address::generate(&e);
            let samwise = Address::generate(&e);
            let frodo = Address::generate(&e);

            let pool_address = create_pool(&e);

            let (oracle_address, oracle_client) = testutils::create_mock_oracle(&e);

            // creating reserves for a pool exhausts the budget
            e.cost_estimate().budget().reset_unlimited();
            let (underlying_0, _) = testutils::create_token_contract(&e, &bombadil);
            let (mut reserve_config_0, mut reserve_data_0) = testutils::default_reserve_meta();
            reserve_data_0.last_time = 12345;
            reserve_data_0.b_rate = 1_100_000_000_000;
            reserve_config_0.c_factor = 0_8500000;
            reserve_config_0.l_factor = 0_9000000;
            reserve_config_0.index = 0;
            testutils::create_reserve(
                &e,
                &pool_address,
                &underlying_0,
                &reserve_config_0,
                &reserve_data_0,
            );

            let (underlying_1, _) = testutils::create_token_contract(&e, &bombadil);
            let (mut reserve_config_1, mut reserve_data_1) = testutils::default_reserve_meta();
            reserve_data_1.b_rate = 1_200_000_000_000;
            reserve_config_1.c_factor = 0_7500000;
            reserve_config_1.l_factor = 0_7500000;
            reserve_data_1.last_time = 12345;
            reserve_config_1.index = 1;
            testutils::create_reserve(
                &e,
                &pool_address,
                &underlying_1,
                &reserve_config_1,
                &reserve_data_1,
            );

            let (underlying_2, reserve_2_asset) = testutils::create_token_contract(&e, &bombadil);
            let (mut reserve_config_2, reserve_data_2) = testutils::default_reserve_meta();
            reserve_config_2.c_factor = 0_0000000;
            reserve_config_2.l_factor = 0_7000000;
            reserve_config_2.index = 2;
            testutils::create_reserve(
                &e,
                &pool_address,
                &underlying_2,
                &reserve_config_2,
                &reserve_data_2,
            );

            oracle_client.set_data(
                &bombadil,
                &Asset::Other(Symbol::new(&e, "USD")),
                &vec![
                    &e,
                    Asset::Stellar(underlying_0.clone()),
                    Asset::Stellar(underlying_1.clone()),
                    Asset::Stellar(underlying_2.clone()),
                ],
                &7,
                &300,
            );
            oracle_client.set_price_stable(&vec![&e, 2_0000000, 4_0000000, 50_0000000]);

            reserve_2_asset.mint(&frodo, &0_8000000);
            reserve_2_asset.approve(&frodo, &pool_address, &i128::MAX, &1000000);

            let mut auction_data = AuctionData {
                bid: map![&e, (underlying_2.clone(), 1_2375000)],
                lot: map![
                    &e,
                    (underlying_0.clone(), 30_5595329),
                    (underlying_1.clone(), 1_5395739)
                ],
                block: 176,
            };
            let pool_config = PoolConfig {
                oracle: oracle_address,
                min_collateral: 1_0000000,
                bstop_rate: 0_1000000,
                status: 0,
                max_positions: 4,
            };
            let positions: Positions = Positions {
                collateral: map![
                    &e,
                    (reserve_config_0.index, 90_9100000),
                    (reserve_config_1.index, 04_5800000),
                ],
                liabilities: map![&e, (reserve_config_2.index, 02_7500000),],
                supply: map![&e],
            };
            e.as_contract(&pool_address, || {
                storage::set_user_positions(&e, &samwise, &positions);
                storage::set_pool_config(&e, &pool_config);

                e.ledger().set(LedgerInfo {
                    timestamp: 12345 + 200 * 5,
                    protocol_version: 22,
                    sequence_number: 176 + 200,
                    network_id: Default::default(),
                    base_reserve: 10,
                    min_temp_entry_ttl: 17280,
                    min_persistent_entry_ttl: 17280,
                    max_entry_ttl: 9999999,
                });
                let mut pool = Pool::load(&e);
                let mut frodo_state = User::load(&e, &frodo);
                fill_user_liq_auction(
                    &e,
                    &mut pool,
                    &mut auction_data,
                    &samwise,
                    &mut frodo_state,
                    true,
                );
                let frodo_positions = frodo_state.positions;
                assert_eq!(
                    frodo_positions
                        .collateral
                        .get(reserve_config_0.index)
                        .unwrap(),
                    30_5595329
                );
                assert_eq!(
                    frodo_positions
                        .collateral
                        .get(reserve_config_1.index)
                        .unwrap(),
                    1_5395739
                );
                assert_eq!(
                    frodo_positions
                        .liabilities
                        .get(reserve_config_2.index)
                        .unwrap(),
                    1_2375000
                );
                let samwise_positions = storage::get_user_positions(&e, &samwise);
                assert_eq!(
                    samwise_positions
                        .collateral
                        .get(reserve_config_0.index)
                        .unwrap(),
                    90_9100000 - 30_5595329
                );
                assert_eq!(
                    samwise_positions
                        .collateral
                        .get(reserve_config_1.index)
                        .unwrap(),
                    04_5800000 - 1_5395739
                );
                assert_eq!(
                    samwise_positions
                        .liabilities
                        .get(reserve_config_2.index)
                        .unwrap(),
                    02_7500000 - 1_2375000
                );
            });
        }

        #[test]
        fn test_fill_user_liquidation_auction_hits_target() {
            let e = Env::default();

            e.mock_all_auths();
            e.ledger().set(LedgerInfo {
                timestamp: 12345,
                protocol_version: 22,
                sequence_number: 175,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 17280,
                min_persistent_entry_ttl: 17280,
                max_entry_ttl: 9999999,
            });

            let bombadil = Address::generate(&e);
            let samwise = Address::generate(&e);
            let frodo = Address::generate(&e);

            let pool_address = create_pool(&e);

            let (oracle_address, oracle_client) = testutils::create_mock_oracle(&e);

            // creating reserves for a pool exhausts the budget
            e.cost_estimate().budget().reset_unlimited();
            let (underlying_0, _) = testutils::create_token_contract(&e, &bombadil);
            let (mut reserve_config_0, mut reserve_data_0) = testutils::default_reserve_meta();
            reserve_data_0.last_time = 12345;
            reserve_data_0.b_rate = 1_100_000_000_000;
            reserve_config_0.c_factor = 0_8500000;
            reserve_config_0.l_factor = 0_9000000;
            reserve_config_0.index = 0;
            testutils::create_reserve(
                &e,
                &pool_address,
                &underlying_0,
                &reserve_config_0,
                &reserve_data_0,
            );

            let (underlying_1, _) = testutils::create_token_contract(&e, &bombadil);
            let (mut reserve_config_1, mut reserve_data_1) = testutils::default_reserve_meta();
            reserve_data_1.b_rate = 1_200_000_000_000;
            reserve_config_1.c_factor = 0_7500000;
            reserve_config_1.l_factor = 0_7500000;
            reserve_data_1.last_time = 12345;
            reserve_config_1.index = 1;
            testutils::create_reserve(
                &e,
                &pool_address,
                &underlying_1,
                &reserve_config_1,
                &reserve_data_1,
            );

            let (underlying_2, reserve_2_asset) = testutils::create_token_contract(&e, &bombadil);
            let (mut reserve_config_2, reserve_data_2) = testutils::default_reserve_meta();
            reserve_config_2.c_factor = 0_0000000;
            reserve_config_2.l_factor = 0_7000000;
            reserve_config_2.index = 2;
            testutils::create_reserve(
                &e,
                &pool_address,
                &underlying_2,
                &reserve_config_2,
                &reserve_data_2,
            );

            oracle_client.set_data(
                &bombadil,
                &Asset::Other(Symbol::new(&e, "USD")),
                &vec![
                    &e,
                    Asset::Stellar(underlying_0.clone()),
                    Asset::Stellar(underlying_1.clone()),
                    Asset::Stellar(underlying_2.clone()),
                ],
                &7,
                &300,
            );
            oracle_client.set_price_stable(&vec![&e, 2_0000000, 4_0000000, 50_0000000]);

            reserve_2_asset.mint(&frodo, &0_8000000);
            reserve_2_asset.approve(&frodo, &pool_address, &i128::MAX, &1000000);

            let mut auction_data = AuctionData {
                bid: map![&e, (underlying_2.clone(), 1_2375000)],
                lot: map![
                    &e,
                    (underlying_0.clone(), 30_5595329),
                    (underlying_1.clone(), 1_5395739)
                ],
                block: 176,
            };
            let pool_config = PoolConfig {
                oracle: oracle_address,
                min_collateral: 1_0000000,
                bstop_rate: 0_1000000,
                status: 0,
                max_positions: 4,
            };
            let positions: Positions = Positions {
                collateral: map![
                    &e,
                    (reserve_config_0.index, 90_9100000),
                    (reserve_config_1.index, 04_5800000),
                ],
                liabilities: map![&e, (reserve_config_2.index, 02_7500000),],
                supply: map![&e],
            };
            e.as_contract(&pool_address, || {
                storage::set_user_positions(&e, &samwise, &positions);
                storage::set_pool_config(&e, &pool_config);
                //scale up modifiers
                e.ledger().set(LedgerInfo {
                    timestamp: 12345 + 200 * 5,
                    protocol_version: 22,
                    sequence_number: 176 + 200,
                    network_id: Default::default(),
                    base_reserve: 10,
                    min_temp_entry_ttl: 17280,
                    min_persistent_entry_ttl: 17280,
                    max_entry_ttl: 9999999,
                });
                let mut pool = Pool::load(&e);
                let mut frodo_state = User::load(&e, &frodo);
                fill_user_liq_auction(
                    &e,
                    &mut pool,
                    &mut auction_data,
                    &samwise,
                    &mut frodo_state,
                    true,
                );
                let samwise_positions = storage::get_user_positions(&e, &samwise);
                let samwise_hf =
                    PositionData::calculate_from_positions(&e, &mut pool, &samwise_positions)
                        .as_health_factor(&e);
                assert_eq!(samwise_hf, 1_1458977);
            });
        }

        #[test]
        fn test_fill_user_liquidation_auction_empty_bid() {
            let e = Env::default();

            e.mock_all_auths();
            e.ledger().set(LedgerInfo {
                timestamp: 12345,
                protocol_version: 22,
                sequence_number: 175,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 17280,
                min_persistent_entry_ttl: 17280,
                max_entry_ttl: 9999999,
            });

            let bombadil = Address::generate(&e);
            let samwise = Address::generate(&e);
            let frodo = Address::generate(&e);

            let pool_address = create_pool(&e);

            let (oracle_address, oracle_client) = testutils::create_mock_oracle(&e);

            // creating reserves for a pool exhausts the budget
            e.cost_estimate().budget().reset_unlimited();
            let (underlying_0, _) = testutils::create_token_contract(&e, &bombadil);
            let (mut reserve_config_0, mut reserve_data_0) = testutils::default_reserve_meta();
            reserve_data_0.last_time = 12345;
            reserve_data_0.b_rate = 1_100_000_000_000;
            reserve_config_0.c_factor = 0_8500000;
            reserve_config_0.l_factor = 0_9000000;
            reserve_config_0.index = 0;
            testutils::create_reserve(
                &e,
                &pool_address,
                &underlying_0,
                &reserve_config_0,
                &reserve_data_0,
            );

            let (underlying_1, _) = testutils::create_token_contract(&e, &bombadil);
            let (mut reserve_config_1, mut reserve_data_1) = testutils::default_reserve_meta();
            reserve_data_1.b_rate = 1_200_000_000_000;
            reserve_config_1.c_factor = 0_7500000;
            reserve_config_1.l_factor = 0_7500000;
            reserve_data_1.last_time = 12345;
            reserve_config_1.index = 1;
            testutils::create_reserve(
                &e,
                &pool_address,
                &underlying_1,
                &reserve_config_1,
                &reserve_data_1,
            );

            let (underlying_2, reserve_2_asset) = testutils::create_token_contract(&e, &bombadil);
            let (mut reserve_config_2, reserve_data_2) = testutils::default_reserve_meta();
            reserve_config_2.c_factor = 0_0000000;
            reserve_config_2.l_factor = 0_7000000;
            reserve_config_2.index = 2;
            testutils::create_reserve(
                &e,
                &pool_address,
                &underlying_2,
                &reserve_config_2,
                &reserve_data_2,
            );

            oracle_client.set_data(
                &bombadil,
                &Asset::Other(Symbol::new(&e, "USD")),
                &vec![
                    &e,
                    Asset::Stellar(underlying_0.clone()),
                    Asset::Stellar(underlying_1.clone()),
                    Asset::Stellar(underlying_2.clone()),
                ],
                &7,
                &300,
            );
            oracle_client.set_price_stable(&vec![&e, 2_0000000, 4_0000000, 50_0000000]);

            reserve_2_asset.mint(&frodo, &0_8000000);
            reserve_2_asset.approve(&frodo, &pool_address, &i128::MAX, &1000000);

            let mut auction_data = AuctionData {
                bid: map![&e],
                lot: map![
                    &e,
                    (underlying_0.clone(), 30_5595329),
                    (underlying_1.clone(), 1_5395739)
                ],
                block: 176,
            };
            let pool_config = PoolConfig {
                oracle: oracle_address,
                min_collateral: 1_0000000,
                bstop_rate: 0_1000000,
                status: 0,
                max_positions: 4,
            };
            let positions: Positions = Positions {
                collateral: map![
                    &e,
                    (reserve_config_0.index, 90_9100000),
                    (reserve_config_1.index, 04_5800000),
                ],
                liabilities: map![&e, (reserve_config_2.index, 02_7500000),],
                supply: map![&e],
            };
            e.as_contract(&pool_address, || {
                storage::set_user_positions(&e, &samwise, &positions);
                storage::set_pool_config(&e, &pool_config);

                e.ledger().set(LedgerInfo {
                    timestamp: 12345 + 200 * 5,
                    protocol_version: 22,
                    sequence_number: 176 + 200,
                    network_id: Default::default(),
                    base_reserve: 10,
                    min_temp_entry_ttl: 17280,
                    min_persistent_entry_ttl: 17280,
                    max_entry_ttl: 9999999,
                });
                let mut pool = Pool::load(&e);
                let mut frodo_state = User::load(&e, &frodo);
                fill_user_liq_auction(
                    &e,
                    &mut pool,
                    &mut auction_data,
                    &samwise,
                    &mut frodo_state,
                    true,
                );
                let frodo_positions = frodo_state.positions;
                assert_eq!(
                    frodo_positions
                        .collateral
                        .get(reserve_config_0.index)
                        .unwrap(),
                    30_5595329
                );
                assert_eq!(
                    frodo_positions
                        .collateral
                        .get(reserve_config_1.index)
                        .unwrap(),
                    1_5395739
                );
                assert_eq!(frodo_positions.liabilities.len(), 0);
                let samwise_positions = storage::get_user_positions(&e, &samwise);
                assert_eq!(
                    samwise_positions
                        .collateral
                        .get(reserve_config_0.index)
                        .unwrap(),
                    90_9100000 - 30_5595329
                );
                assert_eq!(
                    samwise_positions
                        .collateral
                        .get(reserve_config_1.index)
                        .unwrap(),
                    04_5800000 - 1_5395739
                );
                assert_eq!(
                    samwise_positions
                        .liabilities
                        .get(reserve_config_2.index)
                        .unwrap(),
                    02_7500000 - 0
                );
            });
        }

        #[test]
        fn test_fill_user_liquidation_auction_assigns_bad_debt() {
            let e = Env::default();

            e.mock_all_auths();
            e.ledger().set(LedgerInfo {
                timestamp: 12345,
                protocol_version: 22,
                sequence_number: 175,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 17280,
                min_persistent_entry_ttl: 17280,
                max_entry_ttl: 9999999,
            });

            let pool_address = create_pool(&e);
            let bombadil = Address::generate(&e);
            let samwise = Address::generate(&e);
            let frodo = Address::generate(&e);
            let backstop_address = Address::generate(&e);

            let (oracle_address, oracle_client) = testutils::create_mock_oracle(&e);

            // creating reserves for a pool exhausts the budget
            e.cost_estimate().budget().reset_unlimited();
            let (underlying_0, _) = testutils::create_token_contract(&e, &bombadil);
            let (mut reserve_config_0, mut reserve_data_0) = testutils::default_reserve_meta();
            reserve_data_0.last_time = 12345;
            reserve_data_0.b_rate = 1_100_000_000_000;
            reserve_config_0.c_factor = 0_8500000;
            reserve_config_0.l_factor = 0_9000000;
            reserve_config_0.index = 0;
            testutils::create_reserve(
                &e,
                &pool_address,
                &underlying_0,
                &reserve_config_0,
                &reserve_data_0,
            );

            let (underlying_1, _) = testutils::create_token_contract(&e, &bombadil);
            let (mut reserve_config_1, mut reserve_data_1) = testutils::default_reserve_meta();
            reserve_data_1.b_rate = 1_200_000_000_000;
            reserve_config_1.c_factor = 0_7500000;
            reserve_config_1.l_factor = 0_7500000;
            reserve_data_1.last_time = 12345;
            reserve_config_1.index = 1;
            testutils::create_reserve(
                &e,
                &pool_address,
                &underlying_1,
                &reserve_config_1,
                &reserve_data_1,
            );

            let (underlying_2, reserve_2_asset) = testutils::create_token_contract(&e, &bombadil);
            let (mut reserve_config_2, reserve_data_2) = testutils::default_reserve_meta();
            reserve_config_2.c_factor = 0_0000000;
            reserve_config_2.l_factor = 0_7000000;
            reserve_config_2.index = 2;
            testutils::create_reserve(
                &e,
                &pool_address,
                &underlying_2,
                &reserve_config_2,
                &reserve_data_2,
            );

            oracle_client.set_data(
                &bombadil,
                &Asset::Other(Symbol::new(&e, "USD")),
                &vec![
                    &e,
                    Asset::Stellar(underlying_0.clone()),
                    Asset::Stellar(underlying_1.clone()),
                    Asset::Stellar(underlying_2.clone()),
                ],
                &7,
                &300,
            );
            oracle_client.set_price_stable(&vec![&e, 2_0000000, 4_0000000, 50_0000000]);

            reserve_2_asset.mint(&frodo, &0_8000000);
            reserve_2_asset.approve(&frodo, &pool_address, &i128::MAX, &1000000);

            let mut auction_data = AuctionData {
                bid: map![
                    &e,
                    (underlying_1.clone(), 8_0000000),
                    (underlying_2.clone(), 1_5000000)
                ],
                lot: map![&e, (underlying_0.clone(), 90_9100000),],
                block: 176,
            };
            let pool_config = PoolConfig {
                oracle: oracle_address,
                min_collateral: 1_0000000,
                bstop_rate: 0_1000000,
                status: 0,
                max_positions: 4,
            };
            let positions: Positions = Positions {
                collateral: map![&e, (reserve_config_0.index, 90_9100000),],
                liabilities: map![
                    &e,
                    (reserve_config_1.index, 12_0000000),
                    (reserve_config_2.index, 2_0000000),
                ],
                supply: map![&e],
            };
            e.as_contract(&pool_address, || {
                storage::set_backstop(&e, &backstop_address);
                storage::set_user_positions(&e, &samwise, &positions);
                storage::set_pool_config(&e, &pool_config);

                e.ledger().set(LedgerInfo {
                    timestamp: 12345 + 220 * 5,
                    protocol_version: 22,
                    sequence_number: 176 + 220,
                    network_id: Default::default(),
                    base_reserve: 10,
                    min_temp_entry_ttl: 17280,
                    min_persistent_entry_ttl: 17280,
                    max_entry_ttl: 9999999,
                });
                let mut pool = Pool::load(&e);
                let mut frodo_state = User::load(&e, &frodo);
                fill_user_liq_auction(
                    &e,
                    &mut pool,
                    &mut auction_data,
                    &samwise,
                    &mut frodo_state,
                    true,
                );
                let frodo_positions = frodo_state.positions;
                assert_eq!(frodo_positions.liabilities.len(), 2);
                assert_eq!(frodo_positions.collateral.len(), 1);
                assert_eq!(frodo_positions.supply.len(), 0);
                assert_eq!(
                    frodo_positions
                        .collateral
                        .get(reserve_config_0.index)
                        .unwrap(),
                    90_9100000
                );
                assert_eq!(
                    frodo_positions
                        .liabilities
                        .get(reserve_config_1.index)
                        .unwrap(),
                    8_0000000
                );
                assert_eq!(
                    frodo_positions
                        .liabilities
                        .get(reserve_config_2.index)
                        .unwrap(),
                    1_5000000
                );

                let samwise_positions = storage::get_user_positions(&e, &samwise);
                assert_eq!(samwise_positions.liabilities.len(), 0);
                assert_eq!(samwise_positions.collateral.len(), 0);
                assert_eq!(samwise_positions.supply.len(), 0);

                let backstop_positions = storage::get_user_positions(&e, &backstop_address);
                assert_eq!(backstop_positions.liabilities.len(), 2);
                assert_eq!(backstop_positions.collateral.len(), 0);
                assert_eq!(backstop_positions.supply.len(), 0);
                assert_eq!(
                    backstop_positions
                        .liabilities
                        .get(reserve_config_1.index)
                        .unwrap(),
                    4_0000000
                );
                assert_eq!(
                    backstop_positions
                        .liabilities
                        .get(reserve_config_2.index)
                        .unwrap(),
                    0_5000000
                );
            });
        }

        #[test]
        fn test_fill_user_liquidation_auction_no_bad_debt_if_collateral_remaining() {
            let e = Env::default();

            e.mock_all_auths();
            e.ledger().set(LedgerInfo {
                timestamp: 12345,
                protocol_version: 22,
                sequence_number: 175,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 17280,
                min_persistent_entry_ttl: 17280,
                max_entry_ttl: 9999999,
            });

            let pool_address = create_pool(&e);
            let bombadil = Address::generate(&e);
            let samwise = Address::generate(&e);
            let frodo = Address::generate(&e);
            let backstop_address = Address::generate(&e);

            let (oracle_address, oracle_client) = testutils::create_mock_oracle(&e);

            // creating reserves for a pool exhausts the budget
            e.cost_estimate().budget().reset_unlimited();
            let (underlying_0, _) = testutils::create_token_contract(&e, &bombadil);
            let (mut reserve_config_0, mut reserve_data_0) = testutils::default_reserve_meta();
            reserve_data_0.last_time = 12345;
            reserve_data_0.b_rate = 1_100_000_000_000;
            reserve_config_0.c_factor = 0_8500000;
            reserve_config_0.l_factor = 0_9000000;
            reserve_config_0.index = 0;
            testutils::create_reserve(
                &e,
                &pool_address,
                &underlying_0,
                &reserve_config_0,
                &reserve_data_0,
            );

            let (underlying_1, _) = testutils::create_token_contract(&e, &bombadil);
            let (mut reserve_config_1, mut reserve_data_1) = testutils::default_reserve_meta();
            reserve_data_1.b_rate = 1_200_000_000_000;
            reserve_config_1.c_factor = 0_7500000;
            reserve_config_1.l_factor = 0_7500000;
            reserve_data_1.last_time = 12345;
            reserve_config_1.index = 1;
            testutils::create_reserve(
                &e,
                &pool_address,
                &underlying_1,
                &reserve_config_1,
                &reserve_data_1,
            );

            let (underlying_2, reserve_2_asset) = testutils::create_token_contract(&e, &bombadil);
            let (mut reserve_config_2, reserve_data_2) = testutils::default_reserve_meta();
            reserve_config_2.c_factor = 0_0000000;
            reserve_config_2.l_factor = 0_7000000;
            reserve_config_2.index = 2;
            testutils::create_reserve(
                &e,
                &pool_address,
                &underlying_2,
                &reserve_config_2,
                &reserve_data_2,
            );

            oracle_client.set_data(
                &bombadil,
                &Asset::Other(Symbol::new(&e, "USD")),
                &vec![
                    &e,
                    Asset::Stellar(underlying_0.clone()),
                    Asset::Stellar(underlying_1.clone()),
                    Asset::Stellar(underlying_2.clone()),
                ],
                &7,
                &300,
            );
            oracle_client.set_price_stable(&vec![&e, 2_0000000, 4_0000000, 50_0000000]);

            reserve_2_asset.mint(&frodo, &0_8000000);
            reserve_2_asset.approve(&frodo, &pool_address, &i128::MAX, &1000000);

            let mut auction_data = AuctionData {
                bid: map![
                    &e,
                    (underlying_1.clone(), 8_0000000),
                    (underlying_2.clone(), 1_5000000)
                ],
                lot: map![&e, (underlying_0.clone(), 90_9100000),],
                block: 176,
            };
            let pool_config = PoolConfig {
                oracle: oracle_address,
                min_collateral: 1_0000000,
                bstop_rate: 0_1000000,
                status: 0,
                max_positions: 4,
            };
            let positions: Positions = Positions {
                collateral: map![
                    &e,
                    (reserve_config_0.index, 90_9100000),
                    (reserve_config_1.index, 00_6000000),
                ],
                liabilities: map![
                    &e,
                    (reserve_config_1.index, 12_0000000),
                    (reserve_config_2.index, 2_0000000),
                ],
                supply: map![&e],
            };
            e.as_contract(&pool_address, || {
                storage::set_backstop(&e, &backstop_address);
                storage::set_user_positions(&e, &samwise, &positions);
                storage::set_pool_config(&e, &pool_config);

                e.ledger().set(LedgerInfo {
                    timestamp: 12345 + 220 * 5,
                    protocol_version: 22,
                    sequence_number: 176 + 220,
                    network_id: Default::default(),
                    base_reserve: 10,
                    min_temp_entry_ttl: 17280,
                    min_persistent_entry_ttl: 17280,
                    max_entry_ttl: 9999999,
                });
                let mut pool = Pool::load(&e);
                let mut frodo_state = User::load(&e, &frodo);
                fill_user_liq_auction(
                    &e,
                    &mut pool,
                    &mut auction_data,
                    &samwise,
                    &mut frodo_state,
                    true,
                );
                let frodo_positions = frodo_state.positions;
                assert_eq!(frodo_positions.liabilities.len(), 2);
                assert_eq!(frodo_positions.collateral.len(), 1);
                assert_eq!(frodo_positions.supply.len(), 0);
                assert_eq!(
                    frodo_positions
                        .collateral
                        .get(reserve_config_0.index)
                        .unwrap(),
                    90_9100000
                );
                assert_eq!(
                    frodo_positions
                        .liabilities
                        .get(reserve_config_1.index)
                        .unwrap(),
                    8_0000000
                );
                assert_eq!(
                    frodo_positions
                        .liabilities
                        .get(reserve_config_2.index)
                        .unwrap(),
                    1_5000000
                );

                let samwise_positions = storage::get_user_positions(&e, &samwise);
                assert_eq!(samwise_positions.liabilities.len(), 2);
                assert_eq!(samwise_positions.collateral.len(), 1);
                assert_eq!(samwise_positions.supply.len(), 0);
                assert_eq!(
                    samwise_positions
                        .collateral
                        .get(reserve_config_1.index)
                        .unwrap(),
                    0_6000000
                );
                assert_eq!(
                    samwise_positions
                        .liabilities
                        .get(reserve_config_1.index)
                        .unwrap(),
                    4_0000000
                );
                assert_eq!(
                    samwise_positions
                        .liabilities
                        .get(reserve_config_2.index)
                        .unwrap(),
                    0_5000000
                );

                let backstop_positions = storage::get_user_positions(&e, &backstop_address);
                assert_eq!(backstop_positions.liabilities.len(), 0);
                assert_eq!(backstop_positions.collateral.len(), 0);
                assert_eq!(backstop_positions.supply.len(), 0);
            });
        }

        #[test]
        fn test_fill_user_liquidation_auction_no_bad_debt_if_not_100_fill() {
            let e = Env::default();

            e.mock_all_auths();
            e.ledger().set(LedgerInfo {
                timestamp: 12345,
                protocol_version: 22,
                sequence_number: 175,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 17280,
                min_persistent_entry_ttl: 17280,
                max_entry_ttl: 9999999,
            });

            let pool_address = create_pool(&e);
            let bombadil = Address::generate(&e);
            let samwise = Address::generate(&e);
            let frodo = Address::generate(&e);
            let backstop_address = Address::generate(&e);

            let (oracle_address, oracle_client) = testutils::create_mock_oracle(&e);

            // creating reserves for a pool exhausts the budget
            e.cost_estimate().budget().reset_unlimited();
            let (underlying_0, _) = testutils::create_token_contract(&e, &bombadil);
            let (mut reserve_config_0, mut reserve_data_0) = testutils::default_reserve_meta();
            reserve_data_0.last_time = 12345;
            reserve_data_0.b_rate = 1_100_000_000_000;
            reserve_config_0.c_factor = 0_8500000;
            reserve_config_0.l_factor = 0_9000000;
            reserve_config_0.index = 0;
            testutils::create_reserve(
                &e,
                &pool_address,
                &underlying_0,
                &reserve_config_0,
                &reserve_data_0,
            );

            let (underlying_1, _) = testutils::create_token_contract(&e, &bombadil);
            let (mut reserve_config_1, mut reserve_data_1) = testutils::default_reserve_meta();
            reserve_data_1.b_rate = 1_200_000_000_000;
            reserve_config_1.c_factor = 0_7500000;
            reserve_config_1.l_factor = 0_7500000;
            reserve_data_1.last_time = 12345;
            reserve_config_1.index = 1;
            testutils::create_reserve(
                &e,
                &pool_address,
                &underlying_1,
                &reserve_config_1,
                &reserve_data_1,
            );

            let (underlying_2, reserve_2_asset) = testutils::create_token_contract(&e, &bombadil);
            let (mut reserve_config_2, reserve_data_2) = testutils::default_reserve_meta();
            reserve_config_2.c_factor = 0_0000000;
            reserve_config_2.l_factor = 0_7000000;
            reserve_config_2.index = 2;
            testutils::create_reserve(
                &e,
                &pool_address,
                &underlying_2,
                &reserve_config_2,
                &reserve_data_2,
            );

            oracle_client.set_data(
                &bombadil,
                &Asset::Other(Symbol::new(&e, "USD")),
                &vec![
                    &e,
                    Asset::Stellar(underlying_0.clone()),
                    Asset::Stellar(underlying_1.clone()),
                    Asset::Stellar(underlying_2.clone()),
                ],
                &7,
                &300,
            );
            oracle_client.set_price_stable(&vec![&e, 2_0000000, 4_0000000, 50_0000000]);

            reserve_2_asset.mint(&frodo, &0_8000000);
            reserve_2_asset.approve(&frodo, &pool_address, &i128::MAX, &1000000);

            let mut auction_data = AuctionData {
                bid: map![
                    &e,
                    (underlying_1.clone(), 8_0000000),
                    (underlying_2.clone(), 1_5000000)
                ],
                lot: map![&e, (underlying_0.clone(), 90_9100000),],
                block: 176,
            };
            let pool_config = PoolConfig {
                oracle: oracle_address,
                min_collateral: 1_0000000,
                bstop_rate: 0_1000000,
                status: 0,
                max_positions: 4,
            };
            let positions: Positions = Positions {
                collateral: map![&e, (reserve_config_0.index, 90_9100000),],
                liabilities: map![
                    &e,
                    (reserve_config_1.index, 12_0000000),
                    (reserve_config_2.index, 2_0000000),
                ],
                supply: map![&e],
            };
            e.as_contract(&pool_address, || {
                storage::set_backstop(&e, &backstop_address);
                storage::set_user_positions(&e, &samwise, &positions);
                storage::set_pool_config(&e, &pool_config);

                e.ledger().set(LedgerInfo {
                    timestamp: 12345 + 220 * 5,
                    protocol_version: 22,
                    sequence_number: 176 + 220,
                    network_id: Default::default(),
                    base_reserve: 10,
                    min_temp_entry_ttl: 17280,
                    min_persistent_entry_ttl: 17280,
                    max_entry_ttl: 9999999,
                });
                let mut pool = Pool::load(&e);
                let mut frodo_state = User::load(&e, &frodo);
                // note - having no collateral remaining on the user without a 100%
                // fill is not possible. However, this test ensures it is checked to avoid
                // any edge cases.
                fill_user_liq_auction(
                    &e,
                    &mut pool,
                    &mut auction_data,
                    &samwise,
                    &mut frodo_state,
                    false,
                );
                let frodo_positions = frodo_state.positions;
                assert_eq!(frodo_positions.liabilities.len(), 2);
                assert_eq!(frodo_positions.collateral.len(), 1);
                assert_eq!(frodo_positions.supply.len(), 0);
                assert_eq!(
                    frodo_positions
                        .collateral
                        .get(reserve_config_0.index)
                        .unwrap(),
                    90_9100000
                );
                assert_eq!(
                    frodo_positions
                        .liabilities
                        .get(reserve_config_1.index)
                        .unwrap(),
                    8_0000000
                );
                assert_eq!(
                    frodo_positions
                        .liabilities
                        .get(reserve_config_2.index)
                        .unwrap(),
                    1_5000000
                );

                let samwise_positions = storage::get_user_positions(&e, &samwise);
                assert_eq!(samwise_positions.liabilities.len(), 2);
                assert_eq!(samwise_positions.collateral.len(), 0);
                assert_eq!(samwise_positions.supply.len(), 0);
                assert_eq!(
                    samwise_positions
                        .liabilities
                        .get(reserve_config_1.index)
                        .unwrap(),
                    4_0000000
                );
                assert_eq!(
                    samwise_positions
                        .liabilities
                        .get(reserve_config_2.index)
                        .unwrap(),
                    0_5000000
                );

                let backstop_positions = storage::get_user_positions(&e, &backstop_address);
                assert_eq!(backstop_positions.liabilities.len(), 0);
                assert_eq!(backstop_positions.collateral.len(), 0);
                assert_eq!(backstop_positions.supply.len(), 0);
            });
        }
    }
}

mod pool_src_emissions_manager {
    use crate::{
        constants::SCALAR_7,
        dependencies::BackstopClient,
        errors::PoolError,
        events::PoolEvents,
        storage::{self, ReserveConfig, ReserveEmissionData},
    };

    use cast::{i128, u64};

    use soroban_fixed_point_math::SorobanFixedPoint;

    use soroban_sdk::{
        contracttype, map, panic_with_error, unwrap::UnwrapOptimized, Address, Env, Map, Vec,
    };

    use crate::emissions::distributor;

    pub(crate) use crate::emissions::manager::*;

    mod tests {
        use crate::testutils;

        use super::*;
        use soroban_sdk::{
            testutils::{Address as _, Ledger, LedgerInfo},
            unwrap::UnwrapOptimized,
            vec, Address,
        };

        /********** gulp_emissions ********/

        #[test]
        fn test_gulp_emissions_no_pool_emissions_does_nothing() {
            let e = Env::default();
            e.mock_all_auths();
            e.ledger().set(LedgerInfo {
                timestamp: 1500000000,
                protocol_version: 22,
                sequence_number: 20100,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 3110400,
            });

            let pool = testutils::create_pool(&e);
            let bombadil = Address::generate(&e);

            let new_emissions: i128 = 302_400_0000000;
            let pool_emissions: Map<u32, u64> = map![&e];

            let (reserve_config, reserve_data) = testutils::default_reserve_meta();
            let (underlying_0, _) = testutils::create_token_contract(&e, &bombadil);
            testutils::create_reserve(&e, &pool, &underlying_0, &reserve_config, &reserve_data);
            let (underlying_1, _) = testutils::create_token_contract(&e, &bombadil);
            testutils::create_reserve(&e, &pool, &underlying_1, &reserve_config, &reserve_data);

            e.as_contract(&pool, || {
                storage::set_pool_emissions(&e, &pool_emissions);

                do_gulp_emissions(&e, new_emissions);

                assert!(storage::get_res_emis_data(&e, &0).is_none());
                assert!(storage::get_res_emis_data(&e, &1).is_none());
                assert!(storage::get_res_emis_data(&e, &2).is_none());
                assert!(storage::get_res_emis_data(&e, &3).is_none());
            });
        }

        #[test]
        fn test_gulp_emissions() {
            let e = Env::default();
            e.mock_all_auths();
            e.ledger().set(LedgerInfo {
                timestamp: 1500000000,
                protocol_version: 22,
                sequence_number: 20100,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 3110400,
            });

            let pool = testutils::create_pool(&e);
            let bombadil = Address::generate(&e);

            let new_emissions: i128 = 302_400_0000000;
            let pool_emissions: Map<u32, u64> = map![
                &e,
                (0, 0_2000000), // reserve_0 liability
                (2, 0_5500000), // reserve_1 liability
                (3, 0_2500000)  // reserve_1 supply
            ];

            let (reserve_config, mut reserve_data) = testutils::default_reserve_meta();
            reserve_data.last_time = 1499900000;
            let (underlying_0, _) = testutils::create_token_contract(&e, &bombadil);
            testutils::create_reserve(&e, &pool, &underlying_0, &reserve_config, &reserve_data);
            let (underlying_1, _) = testutils::create_token_contract(&e, &bombadil);
            testutils::create_reserve(&e, &pool, &underlying_1, &reserve_config, &reserve_data);
            let (underlying_2, _) = testutils::create_token_contract(&e, &bombadil);
            testutils::create_reserve(&e, &pool, &underlying_2, &reserve_config, &reserve_data);

            // setup reserve_0 liability to have emissions remaining
            let old_r_0_l_data = ReserveEmissionData {
                eps: 0_15000000000000,
                expiration: 1500000200,
                index: 999990000000,
                last_time: 1499980000,
            };

            // setup reserve_1 liability to have no emissions

            // steup reserve_1 supply to have emissions expired
            let old_r_1_s_data = ReserveEmissionData {
                eps: 0_35000000000000,
                expiration: 1499990000,
                index: 111110000000,
                last_time: 1499990000,
            };
            e.as_contract(&pool, || {
                storage::set_pool_emissions(&e, &pool_emissions);
                storage::set_res_emis_data(&e, &0, &old_r_0_l_data);
                storage::set_res_emis_data(&e, &3, &old_r_1_s_data);

                do_gulp_emissions(&e, new_emissions);

                assert!(storage::get_res_emis_data(&e, &1).is_none());
                assert!(storage::get_res_emis_data(&e, &4).is_none());
                assert!(storage::get_res_emis_data(&e, &5).is_none());

                // verify reserve_0 liability leftover emissions were carried over
                let r_0_l_config = storage::get_res_emis_data(&e, &0).unwrap_optimized();
                let r_0_l_data = storage::get_res_emis_data(&e, &0).unwrap_optimized();
                assert_eq!(r_0_l_config.expiration, 1500000000 + 7 * 24 * 60 * 60);
                assert_eq!(r_0_l_config.eps, 0_10004960317460);
                assert_eq!(r_0_l_data.index, (99999 + 40 * SCALAR_7) * SCALAR_7);
                assert_eq!(r_0_l_data.last_time, 1500000000);

                // verify reserve_1 liability initialized emissions
                let r_1_l_config = storage::get_res_emis_data(&e, &2).unwrap_optimized();
                let r_1_l_data = storage::get_res_emis_data(&e, &2).unwrap_optimized();
                assert_eq!(r_1_l_config.expiration, 1500000000 + 7 * 24 * 60 * 60);
                assert_eq!(r_1_l_config.eps, 0_27500000000000);
                assert_eq!(r_1_l_data.index, 0);
                assert_eq!(r_1_l_data.last_time, 1500000000);

                // verify reserve_1 supply updated reserve data to the correct timestamp
                let r_1_s_config = storage::get_res_emis_data(&e, &3).unwrap_optimized();
                let r_1_s_data = storage::get_res_emis_data(&e, &3).unwrap_optimized();
                assert_eq!(r_1_s_config.expiration, 1500000000 + 7 * 24 * 60 * 60);
                assert_eq!(r_1_s_config.eps, 0_12500000000000);
                assert_eq!(r_1_s_data.index, 111110000000);
                assert_eq!(r_1_s_data.last_time, 1500000000);
            });
        }

        #[test]
        fn test_gulp_emissions_when_a_reserve_disabled() {
            let e = Env::default();
            e.mock_all_auths();
            e.ledger().set(LedgerInfo {
                timestamp: 1500000000,
                protocol_version: 22,
                sequence_number: 20100,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 3110400,
            });

            let pool = testutils::create_pool(&e);
            let bombadil = Address::generate(&e);

            let new_emissions: i128 = 302_400_0000000;
            let pool_emissions: Map<u32, u64> = map![
                &e,
                (0, 0_2000000), // reserve_0 liability
                (2, 0_5500000), // reserve_1 liability
                (3, 0_2500000), // reserve_1 supply
                (4, 0_1000000), // reserve_2 liability
                (5, 0_1000000), // reserve_2 supply
            ];

            let (reserve_config, mut reserve_data) = testutils::default_reserve_meta();
            reserve_data.last_time = 1499900000;
            let (underlying_0, _) = testutils::create_token_contract(&e, &bombadil);
            testutils::create_reserve(&e, &pool, &underlying_0, &reserve_config, &reserve_data);
            let (underlying_1, _) = testutils::create_token_contract(&e, &bombadil);
            testutils::create_reserve(&e, &pool, &underlying_1, &reserve_config, &reserve_data);

            let mut reserve_config_disabled = reserve_config.clone();
            reserve_config_disabled.enabled = false;
            let (underlying_2, _) = testutils::create_token_contract(&e, &bombadil);
            testutils::create_reserve(
                &e,
                &pool,
                &underlying_2,
                &reserve_config_disabled,
                &reserve_data,
            );

            // setup reserve_0 liability to have emissions remaining
            let old_r_0_l_data = ReserveEmissionData {
                eps: 0_15000000000000,
                expiration: 1500000200,
                index: 999990000000,
                last_time: 1499980000,
            };

            // setup reserve_1 liability to have no emissions

            // steup reserve_1 supply to have emissions expired
            let old_r_1_s_data = ReserveEmissionData {
                eps: 0_35000000000000,
                expiration: 1499990000,
                index: 111110000000,
                last_time: 1499990000,
            };
            e.as_contract(&pool, || {
                storage::set_pool_emissions(&e, &pool_emissions);
                storage::set_res_emis_data(&e, &0, &old_r_0_l_data);
                storage::set_res_emis_data(&e, &3, &old_r_1_s_data);

                do_gulp_emissions(&e, new_emissions);

                assert!(storage::get_res_emis_data(&e, &1).is_none());
                assert!(storage::get_res_emis_data(&e, &4).is_none());
                assert!(storage::get_res_emis_data(&e, &5).is_none());

                // verify reserve_0 liability leftover emissions were carried over
                let r_0_l_config = storage::get_res_emis_data(&e, &0).unwrap_optimized();
                let r_0_l_data = storage::get_res_emis_data(&e, &0).unwrap_optimized();
                assert_eq!(r_0_l_config.expiration, 1500000000 + 7 * 24 * 60 * 60);
                assert_eq!(r_0_l_config.eps, 0_10004960317460);
                assert_eq!(r_0_l_data.index, (99999 + 40 * SCALAR_7) * SCALAR_7);
                assert_eq!(r_0_l_data.last_time, 1500000000);

                // verify reserve_1 liability initialized emissions
                let r_1_l_config = storage::get_res_emis_data(&e, &2).unwrap_optimized();
                let r_1_l_data = storage::get_res_emis_data(&e, &2).unwrap_optimized();
                assert_eq!(r_1_l_config.expiration, 1500000000 + 7 * 24 * 60 * 60);
                assert_eq!(r_1_l_config.eps, 0_27500000000000);
                assert_eq!(r_1_l_data.index, 0);
                assert_eq!(r_1_l_data.last_time, 1500000000);

                // verify reserve_1 supply updated reserve data to the correct timestamp
                let r_1_s_config = storage::get_res_emis_data(&e, &3).unwrap_optimized();
                let r_1_s_data = storage::get_res_emis_data(&e, &3).unwrap_optimized();
                assert_eq!(r_1_s_config.expiration, 1500000000 + 7 * 24 * 60 * 60);
                assert_eq!(r_1_s_config.eps, 0_12500000000000);
                assert_eq!(r_1_s_data.index, 111110000000);
                assert_eq!(r_1_s_data.last_time, 1500000000);

                // verify reserve_2 liability is None
                let r_2_l_config = storage::get_res_emis_data(&e, &4);
                let r_2_l_data = storage::get_res_emis_data(&e, &4);
                assert!(r_2_l_config.is_none());
                assert!(r_2_l_data.is_none());

                // verify reserve_2 supply is None
                let r_2_s_config = storage::get_res_emis_data(&e, &5);
                let r_2_s_data = storage::get_res_emis_data(&e, &5);
                assert!(r_2_s_config.is_none());
                assert!(r_2_s_data.is_none());
            });
        }

        #[test]
        #[should_panic(expected = "Error(Contract, #1200)")]
        fn test_gulp_emissions_too_small() {
            let e = Env::default();
            e.mock_all_auths();
            e.ledger().set(LedgerInfo {
                timestamp: 1500000000,
                protocol_version: 22,
                sequence_number: 20100,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 3110400,
            });

            let pool = testutils::create_pool(&e);
            let bombadil = Address::generate(&e);

            let new_emissions: i128 = 1000000;
            let pool_emissions: Map<u32, u64> = map![
                &e,
                (0, 0_2000000), // reserve_0 liability
                (2, 0_5500000), // reserve_1 liability
                (3, 0_2500000)  // reserve_1 supply
            ];

            let (reserve_config, mut reserve_data) = testutils::default_reserve_meta();
            reserve_data.last_time = 1499900000;
            let (underlying_0, _) = testutils::create_token_contract(&e, &bombadil);
            testutils::create_reserve(&e, &pool, &underlying_0, &reserve_config, &reserve_data);
            let (underlying_1, _) = testutils::create_token_contract(&e, &bombadil);
            testutils::create_reserve(&e, &pool, &underlying_1, &reserve_config, &reserve_data);
            let (underlying_2, _) = testutils::create_token_contract(&e, &bombadil);
            testutils::create_reserve(&e, &pool, &underlying_2, &reserve_config, &reserve_data);

            // setup reserve_0 liability to have emissions remaining
            let old_r_0_l_data = ReserveEmissionData {
                eps: 0_1500000,
                expiration: 1500000200,
                index: 99999,
                last_time: 1499980000,
            };

            // setup reserve_1 liability to have no emissions

            // steup reserve_1 supply to have emissions expired
            let old_r_1_s_data = ReserveEmissionData {
                eps: 0_3500000,
                expiration: 1499990000,
                index: 11111,
                last_time: 1499990000,
            };
            e.as_contract(&pool, || {
                storage::set_pool_emissions(&e, &pool_emissions);
                storage::set_res_emis_data(&e, &0, &old_r_0_l_data);
                storage::set_res_emis_data(&e, &3, &old_r_1_s_data);

                do_gulp_emissions(&e, new_emissions);
            });
        }

        /********** set_pool_emissions **********/

        #[test]
        fn test_set_pool_emissions() {
            let e = Env::default();
            e.mock_all_auths();
            e.cost_estimate().budget().reset_unlimited();

            e.ledger().set(LedgerInfo {
                timestamp: 1500000000,
                protocol_version: 22,
                sequence_number: 20100,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 3110400,
            });

            let pool = testutils::create_pool(&e);
            let bombadil = Address::generate(&e);

            let (reserve_config, reserve_data) = testutils::default_reserve_meta();
            let (underlying_0, _) = testutils::create_token_contract(&e, &bombadil);
            testutils::create_reserve(&e, &pool, &underlying_0, &reserve_config, &reserve_data);
            let (underlying_1, _) = testutils::create_token_contract(&e, &bombadil);
            testutils::create_reserve(&e, &pool, &underlying_1, &reserve_config, &reserve_data);
            let (underlying_2, _) = testutils::create_token_contract(&e, &bombadil);
            testutils::create_reserve(&e, &pool, &underlying_2, &reserve_config, &reserve_data);
            let (underlying_3, _) = testutils::create_token_contract(&e, &bombadil);
            testutils::create_reserve(&e, &pool, &underlying_3, &reserve_config, &reserve_data);

            let pool_emissions: Map<u32, u64> = map![&e, (2, 0_7500000),];
            let res_emission_metadata: Vec<ReserveEmissionMetadata> = vec![
                &e,
                ReserveEmissionMetadata {
                    res_index: 0,
                    res_type: 1,
                    share: 0_3500000,
                },
                ReserveEmissionMetadata {
                    res_index: 3,
                    res_type: 0,
                    share: 0_6500000,
                },
            ];

            e.as_contract(&pool, || {
                storage::set_pool_emissions(&e, &pool_emissions);

                set_pool_emissions(&e, res_emission_metadata);

                let new_pool_emissions = storage::get_pool_emissions(&e);
                assert_eq!(new_pool_emissions.len(), 2);
                assert_eq!(new_pool_emissions.get(1).unwrap_optimized(), 0_3500000);
                assert_eq!(new_pool_emissions.get(6).unwrap_optimized(), 0_6500000);
            });
        }

        #[test]
        #[should_panic(expected = "Error(Contract, #1200)")]
        fn test_set_pool_emissions_panics_if_anyone_share_equal_0() {
            let e = Env::default();
            e.mock_all_auths();
            e.ledger().set(LedgerInfo {
                timestamp: 1500000000,
                protocol_version: 22,
                sequence_number: 20100,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 3110400,
            });

            let pool = testutils::create_pool(&e);
            let bombadil = Address::generate(&e);

            let (reserve_config, reserve_data) = testutils::default_reserve_meta();
            let (underlying_0, _) = testutils::create_token_contract(&e, &bombadil);
            testutils::create_reserve(&e, &pool, &underlying_0, &reserve_config, &reserve_data);
            let (underlying_1, _) = testutils::create_token_contract(&e, &bombadil);
            testutils::create_reserve(&e, &pool, &underlying_1, &reserve_config, &reserve_data);
            let (underlying_2, _) = testutils::create_token_contract(&e, &bombadil);
            testutils::create_reserve(&e, &pool, &underlying_2, &reserve_config, &reserve_data);
            let (underlying_3, _) = testutils::create_token_contract(&e, &bombadil);
            testutils::create_reserve(&e, &pool, &underlying_3, &reserve_config, &reserve_data);

            let pool_emissions: Map<u32, u64> = map![&e, (2, 0_7500000),];
            let res_emission_metadata: Vec<ReserveEmissionMetadata> = vec![
                &e,
                ReserveEmissionMetadata {
                    res_index: 0,
                    res_type: 1,
                    share: 0_3500000,
                },
                ReserveEmissionMetadata {
                    res_index: 3,
                    res_type: 0,
                    share: 0_6500001,
                },
                ReserveEmissionMetadata {
                    res_index: 3,
                    res_type: 1,
                    share: 0,
                },
            ];

            e.as_contract(&pool, || {
                storage::set_pool_emissions(&e, &pool_emissions);

                set_pool_emissions(&e, res_emission_metadata);
            });
        }

        #[test]
        fn test_set_pool_emissions_ok_if_under_100() {
            let e = Env::default();
            e.mock_all_auths();
            e.cost_estimate().budget().reset_unlimited();

            e.ledger().set(LedgerInfo {
                timestamp: 1500000000,
                protocol_version: 22,
                sequence_number: 20100,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 3110400,
            });

            let pool = testutils::create_pool(&e);
            let bombadil = Address::generate(&e);

            let (reserve_config, reserve_data) = testutils::default_reserve_meta();
            let (underlying_0, _) = testutils::create_token_contract(&e, &bombadil);
            testutils::create_reserve(&e, &pool, &underlying_0, &reserve_config, &reserve_data);
            let (underlying_1, _) = testutils::create_token_contract(&e, &bombadil);
            testutils::create_reserve(&e, &pool, &underlying_1, &reserve_config, &reserve_data);
            let (underlying_2, _) = testutils::create_token_contract(&e, &bombadil);
            testutils::create_reserve(&e, &pool, &underlying_2, &reserve_config, &reserve_data);
            let (underlying_3, _) = testutils::create_token_contract(&e, &bombadil);
            testutils::create_reserve(&e, &pool, &underlying_3, &reserve_config, &reserve_data);

            let pool_emissions: Map<u32, u64> = map![&e, (2, 0_7500000),];
            let res_emission_metadata: Vec<ReserveEmissionMetadata> = vec![
                &e,
                ReserveEmissionMetadata {
                    res_index: 0,
                    res_type: 1,
                    share: 0_3400000,
                },
                ReserveEmissionMetadata {
                    res_index: 3,
                    res_type: 0,
                    share: 0_6500000,
                },
            ];

            e.as_contract(&pool, || {
                storage::set_pool_emissions(&e, &pool_emissions);

                set_pool_emissions(&e, res_emission_metadata);

                let new_pool_emissions = storage::get_pool_emissions(&e);
                assert_eq!(new_pool_emissions.len(), 2);
                assert_eq!(new_pool_emissions.get(1).unwrap_optimized(), 0_3400000);
                assert_eq!(new_pool_emissions.get(6).unwrap_optimized(), 0_6500000);
            });
        }
    }
}

mod pool_src_emissions_distributor {
    use cast::i128;

    use sep_41_token::TokenClient;

    use soroban_fixed_point_math::SorobanFixedPoint;

    use soroban_sdk::{panic_with_error, Address, Env, Vec};

    use crate::{
        constants::SCALAR_7,
        errors::PoolError,
        pool::User,
        storage::{self, ReserveEmissionData, UserEmissionData},
        validator::require_nonnegative,
    };

    pub(crate) use crate::emissions::distributor::*;

    mod tests {
        use crate::{pool::Positions, testutils};

        use super::*;
        use soroban_sdk::{
            map,
            testutils::{Address as AddressTestTrait, Ledger, LedgerInfo},
            unwrap::UnwrapOptimized,
            vec,
        };

        /********** update_emissions **********/

        #[test]
        fn test_update_emissions() {
            let e = Env::default();
            e.mock_all_auths();

            let pool = testutils::create_pool(&e);
            let samwise = Address::generate(&e);

            e.ledger().set(LedgerInfo {
                timestamp: 1501000000, // 10^6 seconds have passed
                protocol_version: 22,
                sequence_number: 123,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 3110400,
            });

            let supply: i128 = 50_0000000;
            let user_position: i128 = 2_0000000;
            e.as_contract(&pool, || {
                let reserve_emission_data = ReserveEmissionData {
                    expiration: 1600000000,
                    eps: 0_01000000000000,
                    index: 23456780000000,
                    last_time: 1500000000,
                };
                let user_emission_data = UserEmissionData {
                    index: 12345670000000,
                    accrued: 0_1000000,
                };
                let res_token_type = 0;
                let res_token_index = 1 * 2 + res_token_type;

                storage::set_res_emis_data(&e, &res_token_index, &reserve_emission_data);
                storage::set_user_emissions(&e, &samwise, &res_token_index, &user_emission_data);

                update_emissions(
                    &e,
                    res_token_index,
                    supply,
                    1_0000000,
                    &samwise,
                    user_position,
                );

                let new_reserve_emission_data =
                    storage::get_res_emis_data(&e, &res_token_index).unwrap_optimized();
                let new_user_emission_data =
                    storage::get_user_emissions(&e, &samwise, &res_token_index).unwrap_optimized();
                assert_eq!(new_reserve_emission_data.last_time, 1501000000);
                assert_eq!(
                    new_user_emission_data.index,
                    new_reserve_emission_data.index
                );
                assert_eq!(new_user_emission_data.accrued, 400_3222222);
            });
        }

        #[test]
        fn test_update_emissions_no_data_ignores() {
            let e = Env::default();
            e.mock_all_auths();

            let pool = testutils::create_pool(&e);
            let samwise = Address::generate(&e);

            e.ledger().set(LedgerInfo {
                timestamp: 1501000000, // 10^6 seconds have passed
                protocol_version: 22,
                sequence_number: 123,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 3110400,
            });

            let supply: i128 = 100_0000000;
            let user_position: i128 = 2_0000000;
            e.as_contract(&pool, || {
                let res_token_type = 1;
                let res_token_index = 1 * 2 + res_token_type;

                update_emissions(
                    &e,
                    res_token_index,
                    supply,
                    1_0000000,
                    &samwise,
                    user_position,
                );

                assert!(storage::get_res_emis_data(&e, &res_token_index).is_none());
                assert!(storage::get_user_emissions(&e, &samwise, &res_token_index).is_none());
            });
        }

        #[test]
        #[should_panic(expected = "attempt to subtract with overflow")]
        fn test_update_emissions_negative_time_diff() {
            let e = Env::default();
            e.mock_all_auths();

            let pool = testutils::create_pool(&e);
            let samwise = Address::generate(&e);

            e.ledger().set(LedgerInfo {
                timestamp: 1501000000,
                protocol_version: 22,
                sequence_number: 123,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 3110400,
            });

            let supply: i128 = 50_0000000;
            let user_position: i128 = 2_0000000;
            e.as_contract(&pool, || {
                let reserve_emission_data = ReserveEmissionData {
                    expiration: 1600000000,
                    eps: 0_01000000000000,
                    index: 2345678,
                    last_time: 1501000000 + 1,
                };
                let user_emission_data = UserEmissionData {
                    index: 1234567,
                    accrued: 0_1000000,
                };
                let res_token_type = 0;
                let res_token_index = 1 * 2 + res_token_type;

                storage::set_res_emis_data(&e, &res_token_index, &reserve_emission_data);
                storage::set_user_emissions(&e, &samwise, &res_token_index, &user_emission_data);

                update_emissions(
                    &e,
                    res_token_index,
                    supply,
                    1_0000000,
                    &samwise,
                    user_position,
                );
            });
        }

        #[test]
        fn test_update_emission_no_overflow() {
            let e = Env::default();
            e.mock_all_auths();

            let pool = testutils::create_pool(&e);
            let samwise = Address::generate(&e);

            e.ledger().set(LedgerInfo {
                timestamp: 1510000000, // 10^7 seconds have passed
                protocol_version: 22,
                sequence_number: 123,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 3110400,
            });
            // Supply of 1 trillion at 18 decimals
            let supply: i128 = 1000000000000_000000000000000000;
            let user_position: i128 = 1_000000000000000000;
            e.as_contract(&pool, || {
                let reserve_emission_data = ReserveEmissionData {
                    expiration: 1600000000,
                    eps: 100_00000000000000,
                    index: 23456780000000,
                    last_time: 1500000000,
                };
                let user_emission_data = UserEmissionData {
                    index: 12345670000000,
                    accrued: 0_1000000,
                };
                let res_token_type = 0;
                let res_token_index = 1 * 2 + res_token_type;

                storage::set_res_emis_data(&e, &res_token_index, &reserve_emission_data);
                storage::set_user_emissions(&e, &samwise, &res_token_index, &user_emission_data);

                // Intermediate index math should not overflow using SorobanFixedPoint
                // 10^7 * 10^16 * 10^18 = 10^41 > i128::MAX
                update_emissions(
                    &e,
                    res_token_index,
                    supply,
                    1_000000000000000000,
                    &samwise,
                    user_position,
                );

                let new_reserve_emission_data =
                    storage::get_res_emis_data(&e, &res_token_index).unwrap_optimized();
                let new_user_emission_data =
                    storage::get_user_emissions(&e, &samwise, &res_token_index).unwrap_optimized();
                assert_eq!(new_reserve_emission_data.last_time, 1510000000);
                assert_eq!(
                    new_user_emission_data.index,
                    new_reserve_emission_data.index
                );
                assert_eq!(new_user_emission_data.accrued, 2121111);
            });
        }

        /********** claim_emissions **********/

        #[test]
        fn test_claim_emissions() {
            let e = Env::default();
            e.mock_all_auths();

            let pool = testutils::create_pool(&e);
            let samwise = Address::generate(&e);

            e.ledger().set(LedgerInfo {
                timestamp: 1501000000, // 10^6 seconds have passed
                protocol_version: 22,
                sequence_number: 123,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 3110400,
            });

            let supply: i128 = 50_0000000;
            let user_position: i128 = 2_0000000;
            e.as_contract(&pool, || {
                let reserve_emission_data = ReserveEmissionData {
                    expiration: 1600000000,
                    eps: 0_01000000000000,
                    index: 23456780000000,
                    last_time: 1500000000,
                };
                let user_emission_data = UserEmissionData {
                    index: 12345670000000,
                    accrued: 0_1000000,
                };
                let res_token_type = 0;
                let res_token_index = 1 * 2 + res_token_type;

                storage::set_res_emis_data(&e, &res_token_index, &reserve_emission_data);
                storage::set_user_emissions(&e, &samwise, &res_token_index, &user_emission_data);

                let result = claim_emissions(
                    &e,
                    res_token_index,
                    supply,
                    1_0000000,
                    &samwise,
                    user_position,
                );

                assert_eq!(result, 400_3222222);
                let new_reserve_emission_data =
                    storage::get_res_emis_data(&e, &res_token_index).unwrap_optimized();
                let new_user_emission_data =
                    storage::get_user_emissions(&e, &samwise, &res_token_index).unwrap_optimized();
                assert_eq!(new_reserve_emission_data.last_time, 1501000000);
                assert_eq!(
                    new_user_emission_data.index,
                    new_reserve_emission_data.index
                );
                assert_eq!(new_user_emission_data.accrued, 0);
            });
        }

        #[test]
        fn test_claim_emissions_no_config_ignores() {
            let e = Env::default();
            e.mock_all_auths();

            let pool = testutils::create_pool(&e);
            let samwise = Address::generate(&e);

            e.ledger().set(LedgerInfo {
                timestamp: 1501000000, // 10^6 seconds have passed
                protocol_version: 22,
                sequence_number: 123,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 3110400,
            });

            let supply: i128 = 100_0000000;
            let user_position: i128 = 2_0000000;
            e.as_contract(&pool, || {
                let res_token_type = 1;
                let res_token_index = 1 * 2 + res_token_type;

                claim_emissions(
                    &e,
                    res_token_index,
                    supply,
                    1_0000000,
                    &samwise,
                    user_position,
                );

                assert!(storage::get_res_emis_data(&e, &res_token_index).is_none());
                assert!(storage::get_user_emissions(&e, &samwise, &res_token_index).is_none());
            });
        }

        /********** update emission data **********/

        #[test]
        fn test_update_emission_data_no_config_returns_none() {
            let e = Env::default();
            e.mock_all_auths();

            let pool = testutils::create_pool(&e);

            e.ledger().set(LedgerInfo {
                timestamp: 1501000000, // 10^6 seconds have passed
                protocol_version: 22,
                sequence_number: 123,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 3110400,
            });

            let supply = 50_0000000;
            let supply_scalar = 1_0000000;
            e.as_contract(&pool, || {
                let res_token_type = 1;
                let res_token_index = 1 * 2 + res_token_type;

                // no emission information stored

                let result = update_emission_data(&e, res_token_index, supply, supply_scalar);
                match result {
                    Some(_) => {
                        assert!(false)
                    }
                    None => {
                        assert!(storage::get_res_emis_data(&e, &res_token_index).is_none());
                    }
                }
            });
        }

        #[test]
        fn test_update_emission_data_expired_returns_old() {
            let e = Env::default();
            e.mock_all_auths();

            let pool = testutils::create_pool(&e);

            e.ledger().set(LedgerInfo {
                timestamp: 1601000000,
                protocol_version: 22,
                sequence_number: 123,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 3110400,
            });

            let supply = 50_0000000;
            let supply_scalar = 1_0000000;
            e.as_contract(&pool, || {
                let reserve_emission_data = ReserveEmissionData {
                    expiration: 1600000000,
                    eps: 0_01000000000000,
                    index: 2345678,
                    last_time: 1600000000,
                };

                let res_token_type = 0;
                let res_token_index = 1 * 2 + res_token_type;

                storage::set_res_emis_data(&e, &res_token_index, &reserve_emission_data);

                let result = update_emission_data(&e, res_token_index, supply, supply_scalar);
                match result {
                    Some(_) => {
                        let new_reserve_emission_data =
                            storage::get_res_emis_data(&e, &res_token_index).unwrap_optimized();
                        assert_eq!(
                            new_reserve_emission_data.last_time,
                            reserve_emission_data.last_time
                        );
                        assert_eq!(new_reserve_emission_data.index, reserve_emission_data.index);
                    }
                    None => assert!(false),
                }
            });
        }

        #[test]
        fn test_update_emission_data_updated_this_block_returns_old() {
            let e = Env::default();
            e.mock_all_auths();

            let pool = testutils::create_pool(&e);

            e.ledger().set(LedgerInfo {
                timestamp: 1501000000,
                protocol_version: 22,
                sequence_number: 123,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 3110400,
            });

            let supply = 50_0000000;
            let supply_scalar = 1_0000000;
            e.as_contract(&pool, || {
                let reserve_emission_data = ReserveEmissionData {
                    expiration: 1600000000,
                    eps: 0_01000000000000,
                    index: 2345678,
                    last_time: 1501000000,
                };

                let res_token_type = 1;
                let res_token_index = 1 * 2 + res_token_type;

                storage::set_res_emis_data(&e, &res_token_index, &reserve_emission_data);

                let result = update_emission_data(&e, res_token_index, supply, supply_scalar);
                match result {
                    Some(_) => {
                        let new_reserve_emission_data =
                            storage::get_res_emis_data(&e, &res_token_index).unwrap_optimized();
                        assert_eq!(
                            new_reserve_emission_data.last_time,
                            reserve_emission_data.last_time
                        );
                        assert_eq!(new_reserve_emission_data.index, reserve_emission_data.index);
                    }
                    None => assert!(false),
                }
            });
        }

        #[test]
        fn test_update_emission_data_no_eps_returns_old() {
            let e = Env::default();
            e.mock_all_auths();

            let pool = testutils::create_pool(&e);

            e.ledger().set(LedgerInfo {
                timestamp: 1501000000,
                protocol_version: 22,
                sequence_number: 123,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 3110400,
            });

            let supply = 50_0000000;
            let supply_scalar = 1_0000000;
            e.as_contract(&pool, || {
                let reserve_emission_data = ReserveEmissionData {
                    expiration: 1600000000,
                    eps: 0,
                    index: 2345678,
                    last_time: 1500000000,
                };

                let res_token_type = 0;
                let res_token_index = 1 * 2 + res_token_type;

                storage::set_res_emis_data(&e, &res_token_index, &reserve_emission_data);

                let result = update_emission_data(&e, res_token_index, supply, supply_scalar);
                match result {
                    Some(_) => {
                        let new_reserve_emission_data =
                            storage::get_res_emis_data(&e, &res_token_index).unwrap_optimized();
                        assert_eq!(
                            new_reserve_emission_data.last_time,
                            reserve_emission_data.last_time
                        );
                        assert_eq!(new_reserve_emission_data.index, reserve_emission_data.index);
                    }
                    None => assert!(false),
                }
            });
        }

        #[test]
        fn test_update_emission_data_no_supply_returns_old() {
            let e = Env::default();
            e.mock_all_auths();

            let pool = testutils::create_pool(&e);

            e.ledger().set(LedgerInfo {
                timestamp: 1501000000,
                protocol_version: 22,
                sequence_number: 123,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 3110400,
            });

            let supply = 0;
            let supply_scalar = 1_0000000;
            e.as_contract(&pool, || {
                let reserve_emission_data = ReserveEmissionData {
                    expiration: 1600000000,
                    eps: 0_01000000000000,
                    index: 2345678,
                    last_time: 1500000000,
                };

                let res_token_type = 1;
                let res_token_index = 1 * 2 + res_token_type;

                storage::set_res_emis_data(&e, &res_token_index, &reserve_emission_data);

                let result = update_emission_data(&e, res_token_index, supply, supply_scalar);
                match result {
                    Some(_) => {
                        let new_reserve_emission_data =
                            storage::get_res_emis_data(&e, &res_token_index).unwrap_optimized();
                        assert_eq!(
                            new_reserve_emission_data.last_time,
                            reserve_emission_data.last_time
                        );
                        assert_eq!(new_reserve_emission_data.index, reserve_emission_data.index);
                    }
                    None => assert!(false),
                }
            });
        }

        #[test]
        fn test_update_emission_data_past_exp() {
            let e = Env::default();
            e.mock_all_auths();

            let pool = testutils::create_pool(&e);

            e.ledger().set(LedgerInfo {
                timestamp: 1700000000,
                protocol_version: 22,
                sequence_number: 123,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 3110400,
            });

            let supply = 100_0000000;
            let supply_scalar = 1_0000000;
            e.as_contract(&pool, || {
                let reserve_emission_data = ReserveEmissionData {
                    expiration: 1600000001,
                    eps: 0_01000000000000,
                    index: 1234567890000000,
                    last_time: 1500000000,
                };

                let res_token_type = 0;
                let res_token_index = 1 * 2 + res_token_type;

                storage::set_res_emis_data(&e, &res_token_index, &reserve_emission_data);

                let result = update_emission_data(&e, res_token_index, supply, supply_scalar);
                match result {
                    Some(_) => {
                        let new_reserve_emission_data =
                            storage::get_res_emis_data(&e, &res_token_index).unwrap_optimized();
                        assert_eq!(new_reserve_emission_data.last_time, 1600000001);
                        assert_eq!(new_reserve_emission_data.index, 10012_34577890000000);
                    }
                    None => assert!(false),
                }
            });
        }

        #[test]
        fn test_update_emission_data_rounds_down() {
            let e = Env::default();
            e.mock_all_auths();

            let pool = testutils::create_pool(&e);

            e.ledger().set(LedgerInfo {
                timestamp: 1500000005,
                protocol_version: 22,
                sequence_number: 123,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 3110400,
            });

            let supply = 100_0001111;
            let supply_scalar = 1_0000000;
            e.as_contract(&pool, || {
                let reserve_emission_data = ReserveEmissionData {
                    expiration: 1600000000,
                    eps: 0_01000000000000,
                    index: 1234567890000000,
                    last_time: 1500000000,
                };

                let res_token_type = 1;
                let res_token_index = 1 * 2 + res_token_type;

                storage::set_res_emis_data(&e, &res_token_index, &reserve_emission_data);

                let result = update_emission_data(&e, res_token_index, supply, supply_scalar);
                match result {
                    Some(_) => {
                        let new_reserve_emission_data =
                            storage::get_res_emis_data(&e, &res_token_index).unwrap_optimized();
                        assert_eq!(new_reserve_emission_data.last_time, 1500000005);
                        assert_eq!(new_reserve_emission_data.index, 1234617889944450);
                    }
                    None => assert!(false),
                }
            });
        }

        /********** update_user_emissions **********/

        #[test]
        fn test_update_user_emissions_first_time() {
            let e = Env::default();
            e.mock_all_auths();

            let pool = testutils::create_pool(&e);
            let samwise = Address::generate(&e);

            e.ledger().set(LedgerInfo {
                timestamp: 1500000000,
                protocol_version: 22,
                sequence_number: 123,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 3110400,
            });

            let supply_scalar = 1_0000000;
            let user_balance = 0;
            e.as_contract(&pool, || {
                let reserve_emission_data = ReserveEmissionData {
                    expiration: 1600000000,
                    eps: 0_01000000000000,
                    index: 123456789,
                    last_time: 1500000000,
                };

                let res_token_type = 0;
                let res_token_index = 1 * 2 + res_token_type;
                update_user_emissions(
                    &e,
                    &reserve_emission_data,
                    res_token_index,
                    supply_scalar,
                    &samwise,
                    user_balance,
                    false,
                );

                let new_user_emission_data =
                    storage::get_user_emissions(&e, &samwise, &res_token_index).unwrap_optimized();
                assert_eq!(new_user_emission_data.index, reserve_emission_data.index);
                assert_eq!(new_user_emission_data.accrued, 0);
            });
        }

        #[test]
        fn test_update_user_emissions_first_time_had_tokens() {
            let e = Env::default();
            e.mock_all_auths();

            let pool = testutils::create_pool(&e);
            let samwise = Address::generate(&e);

            e.ledger().set(LedgerInfo {
                timestamp: 1500000000,
                protocol_version: 22,
                sequence_number: 123,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 3110400,
            });

            let supply_scalar = 1_0000000;
            let user_balance = 0_5000000;
            e.as_contract(&pool, || {
                let reserve_emission_data = ReserveEmissionData {
                    expiration: 1600000000,
                    eps: 0_01000000000000,
                    index: 1234567890000000,
                    last_time: 1500000000,
                };

                let res_token_type = 0;
                let res_token_index = 1 * 2 + res_token_type;
                update_user_emissions(
                    &e,
                    &reserve_emission_data,
                    res_token_index,
                    supply_scalar,
                    &samwise,
                    user_balance,
                    false,
                );

                let new_user_emission_data =
                    storage::get_user_emissions(&e, &samwise, &res_token_index).unwrap_optimized();
                assert_eq!(new_user_emission_data.index, reserve_emission_data.index);
                assert_eq!(new_user_emission_data.accrued, 6_1728394);
            });
        }

        #[test]
        fn test_update_user_emissions_no_bal_no_accrual() {
            let e = Env::default();
            e.mock_all_auths();
            let pool = testutils::create_pool(&e);

            let samwise = Address::generate(&e);

            e.ledger().set(LedgerInfo {
                timestamp: 1500000000,
                protocol_version: 22,
                sequence_number: 123,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 3110400,
            });

            let supply_scalar = 1_0000000;
            let user_balance = 0;
            e.as_contract(&pool, || {
                let reserve_emission_data = ReserveEmissionData {
                    expiration: 1600000000,
                    eps: 0_01000000000000,
                    index: 123456789,
                    last_time: 1500000000,
                };
                let user_emission_data = UserEmissionData {
                    index: 56789,
                    accrued: 0_1000000,
                };

                let res_token_type = 1;
                let res_token_index = 1 * 2 + res_token_type;
                storage::set_user_emissions(&e, &samwise, &res_token_index, &user_emission_data);

                update_user_emissions(
                    &e,
                    &reserve_emission_data,
                    res_token_index,
                    supply_scalar,
                    &samwise,
                    user_balance,
                    false,
                );

                let new_user_emission_data =
                    storage::get_user_emissions(&e, &samwise, &res_token_index).unwrap_optimized();
                assert_eq!(new_user_emission_data.index, reserve_emission_data.index);
                assert_eq!(new_user_emission_data.accrued, 0_1000000);
            });
        }

        #[test]
        fn test_update_user_emissions_if_accrued_skips() {
            let e = Env::default();
            e.mock_all_auths();

            let pool = testutils::create_pool(&e);

            let samwise = Address::generate(&e);

            e.ledger().set(LedgerInfo {
                timestamp: 1500000000,
                protocol_version: 22,
                sequence_number: 123,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 3110400,
            });

            let supply_scalar = 1_0000000;
            let user_balance = 0_5000000;
            e.as_contract(&pool, || {
                let reserve_emission_data = ReserveEmissionData {
                    expiration: 1600000000,
                    eps: 0_01000000000000,
                    index: 123456789,
                    last_time: 1500000000,
                };
                let user_emission_data = UserEmissionData {
                    index: 123456789,
                    accrued: 1_1000000,
                };

                let res_token_type = 0;
                let res_token_index = 1 * 2 + res_token_type;
                storage::set_user_emissions(&e, &samwise, &res_token_index, &user_emission_data);

                update_user_emissions(
                    &e,
                    &reserve_emission_data,
                    res_token_index,
                    supply_scalar,
                    &samwise,
                    user_balance,
                    false,
                );

                let new_user_emission_data =
                    storage::get_user_emissions(&e, &samwise, &res_token_index).unwrap_optimized();
                assert_eq!(new_user_emission_data.index, reserve_emission_data.index);
                assert_eq!(new_user_emission_data.accrued, user_emission_data.accrued);
            });
        }

        #[test]
        fn test_update_user_emissions_accrues() {
            let e = Env::default();
            e.mock_all_auths();

            let pool = testutils::create_pool(&e);
            let samwise = Address::generate(&e);

            e.ledger().set(LedgerInfo {
                timestamp: 1500000000,
                protocol_version: 22,
                sequence_number: 123,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 3110400,
            });

            let supply_scalar = 1_0000000;
            let user_balance = 0_5000000;
            e.as_contract(&pool, || {
                let reserve_emission_data = ReserveEmissionData {
                    expiration: 1600000000,
                    eps: 0_01000000000000,
                    index: 1234567890000000,
                    last_time: 1500000000,
                };
                let user_emission_data = UserEmissionData {
                    index: 567890000000,
                    accrued: 0_1000000,
                };

                let res_token_type = 1;
                let res_token_index = 1 * 2 + res_token_type;
                storage::set_user_emissions(&e, &samwise, &res_token_index, &user_emission_data);

                update_user_emissions(
                    &e,
                    &reserve_emission_data,
                    res_token_index,
                    supply_scalar,
                    &samwise,
                    user_balance,
                    false,
                );

                let new_user_emission_data =
                    storage::get_user_emissions(&e, &samwise, &res_token_index).unwrap_optimized();
                assert_eq!(new_user_emission_data.index, reserve_emission_data.index);
                assert_eq!(new_user_emission_data.accrued, 6_2700000);
            });
        }

        #[test]
        fn test_update_user_emissions_claim_returns_accrual() {
            let e = Env::default();
            e.mock_all_auths();

            let pool = testutils::create_pool(&e);

            let samwise = Address::generate(&e);

            e.ledger().set(LedgerInfo {
                timestamp: 1500000000,
                protocol_version: 22,
                sequence_number: 123,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 3110400,
            });

            let supply_scalar = 1_0000000;
            let user_balance = 0_5000000;
            e.as_contract(&pool, || {
                let reserve_emission_data = ReserveEmissionData {
                    expiration: 1600000000,
                    eps: 0_01000000000000,
                    index: 1234567890000000,
                    last_time: 1500000000,
                };
                let user_emission_data = UserEmissionData {
                    index: 567890000000,
                    accrued: 0_1000000,
                };

                let res_token_type = 1;
                let res_token_index = 1 * 2 + res_token_type;
                storage::set_user_emissions(&e, &samwise, &res_token_index, &user_emission_data);

                let result = update_user_emissions(
                    &e,
                    &reserve_emission_data,
                    res_token_index,
                    supply_scalar,
                    &samwise,
                    user_balance,
                    true,
                );

                let new_user_emission_data =
                    storage::get_user_emissions(&e, &samwise, &res_token_index).unwrap_optimized();
                assert_eq!(new_user_emission_data.index, reserve_emission_data.index);
                assert_eq!(new_user_emission_data.accrued, 0);
                assert_eq!(result, 6_2700000);
            });
        }

        #[test]
        fn test_update_user_emissions_claim_first_time_claims_tokens() {
            let e = Env::default();
            e.mock_all_auths();

            let pool = testutils::create_pool(&e);

            let samwise = Address::generate(&e);

            e.ledger().set(LedgerInfo {
                timestamp: 1500000000,
                protocol_version: 22,
                sequence_number: 123,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 3110400,
            });

            let supply_scalar = 1_0000000;
            let user_balance = 0_5000000;
            e.as_contract(&pool, || {
                let reserve_emission_data = ReserveEmissionData {
                    expiration: 1600000000,
                    eps: 0_01000000000000,
                    index: 1234567890000000,
                    last_time: 1500000000,
                };

                let res_token_type = 0;
                let res_token_index = 1 * 2 + res_token_type;
                let result = update_user_emissions(
                    &e,
                    &reserve_emission_data,
                    res_token_index,
                    supply_scalar,
                    &samwise,
                    user_balance,
                    true,
                );

                let new_user_emission_data =
                    storage::get_user_emissions(&e, &samwise, &res_token_index).unwrap_optimized();
                assert_eq!(new_user_emission_data.index, reserve_emission_data.index);
                assert_eq!(new_user_emission_data.accrued, 0);
                assert_eq!(result, 6_1728394);
            });
        }

        #[test]
        #[should_panic(expected = "Error(Contract, #8)")]
        fn test_update_user_emissions_negative_index() {
            let e = Env::default();
            e.mock_all_auths();

            let pool = testutils::create_pool(&e);

            let samwise = Address::generate(&e);

            e.ledger().set(LedgerInfo {
                timestamp: 1500000000,
                protocol_version: 22,
                sequence_number: 123,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 3110400,
            });

            let supply_scalar = 1_0000000;
            let user_balance = 0_5000000;
            e.as_contract(&pool, || {
                let reserve_emission_data = ReserveEmissionData {
                    expiration: 1600000000,
                    eps: 0_01000000000000,
                    index: 123456789,
                    last_time: 1500000000,
                };
                let user_emission_data = UserEmissionData {
                    index: 123456789 + 1,
                    accrued: 0_1000000,
                };

                let res_token_type = 1;
                let res_token_index = 1 * 2 + res_token_type;
                storage::set_user_emissions(&e, &samwise, &res_token_index, &user_emission_data);

                update_user_emissions(
                    &e,
                    &reserve_emission_data,
                    res_token_index,
                    supply_scalar,
                    &samwise,
                    user_balance,
                    true,
                );
            });
        }

        //********** execute claim **********//

        #[test]
        fn test_execute_claim() {
            let e = Env::default();
            e.mock_all_auths_allowing_non_root_auth();
            e.cost_estimate().budget().reset_unlimited();

            let pool = testutils::create_pool(&e);
            let bombadil = Address::generate(&e);
            let samwise = Address::generate(&e);
            let merry = Address::generate(&e);

            let (blnd, blnd_token_client) = testutils::create_blnd_token(&e, &pool, &bombadil);
            let (backstop, _) = testutils::create_backstop(
                &e,
                &pool,
                &Address::generate(&e),
                &Address::generate(&e),
                &blnd,
            );
            // mock backstop having emissions for pool
            e.as_contract(&backstop, || {
                blnd_token_client.approve(&backstop, &pool, &100_000_0000000_i128, &1000000);
            });
            blnd_token_client.mint(&backstop, &100_000_0000000);

            e.ledger().set(LedgerInfo {
                timestamp: 1501000000, // 10^6 seconds have passed
                protocol_version: 22,
                sequence_number: 123,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 3110400,
            });

            let (underlying_0, _) = testutils::create_token_contract(&e, &bombadil);
            let (mut reserve_config, mut reserve_data) = testutils::default_reserve_meta();
            reserve_config.decimals = 5;
            reserve_data.b_supply = 100_00000;
            reserve_data.d_supply = 50_00000;
            testutils::create_reserve(&e, &pool, &underlying_0, &reserve_config, &reserve_data);

            let (underlying_1, _) = testutils::create_token_contract(&e, &bombadil);
            let (mut reserve_config, mut reserve_data) = testutils::default_reserve_meta();
            reserve_config.decimals = 9;
            reserve_config.index = 1;
            reserve_data.b_supply = 100_000_000_000;
            reserve_data.d_supply = 50_000_000_000;
            testutils::create_reserve(&e, &pool, &underlying_1, &reserve_config, &reserve_data);

            let user_positions = Positions {
                liabilities: map![&e, (0, 2_00000)],
                collateral: map![&e, (1, 1_000_000_000)],
                supply: map![&e, (1, 1_000_000_000)],
            };
            e.as_contract(&pool, || {
                storage::set_backstop(&e, &backstop);
                storage::set_user_positions(&e, &samwise, &user_positions);

                let reserve_emission_data_0 = ReserveEmissionData {
                    expiration: 1600000000,
                    eps: 0_01000000000000,
                    index: 23456780000000,
                    last_time: 1500000000,
                };
                let user_emission_data_0 = UserEmissionData {
                    index: 12345670000000,
                    accrued: 0_1000000,
                };
                let res_token_index_0 = 0 * 2 + 0; // d_token for reserve 0

                let reserve_emission_data_1 = ReserveEmissionData {
                    expiration: 1600000000,
                    eps: 0_01500000000000,
                    index: 13456780000000,
                    last_time: 1500000000,
                };
                let user_emission_data_1 = UserEmissionData {
                    index: 12345670000000,
                    accrued: 1_0000000,
                };
                let res_token_index_1 = 1 * 2 + 1; // b_token for reserve 1

                storage::set_res_emis_data(&e, &res_token_index_0, &reserve_emission_data_0);
                storage::set_user_emissions(
                    &e,
                    &samwise,
                    &res_token_index_0,
                    &user_emission_data_0,
                );

                storage::set_res_emis_data(&e, &res_token_index_1, &reserve_emission_data_1);
                storage::set_user_emissions(
                    &e,
                    &samwise,
                    &res_token_index_1,
                    &user_emission_data_1,
                );

                let reserve_token_ids: Vec<u32> = vec![&e, res_token_index_0, res_token_index_1];
                let result = execute_claim(&e, &samwise, &reserve_token_ids, &merry);

                let new_reserve_emission_data =
                    storage::get_res_emis_data(&e, &res_token_index_0).unwrap_optimized();
                let new_user_emission_data =
                    storage::get_user_emissions(&e, &samwise, &res_token_index_0)
                        .unwrap_optimized();
                assert_eq!(new_reserve_emission_data.last_time, 1501000000);
                assert_eq!(
                    new_user_emission_data.index,
                    new_reserve_emission_data.index
                );
                assert_eq!(new_user_emission_data.accrued, 0);

                let new_reserve_emission_data_1 =
                    storage::get_res_emis_data(&e, &res_token_index_1).unwrap_optimized();
                let new_user_emission_data_1 =
                    storage::get_user_emissions(&e, &samwise, &res_token_index_1)
                        .unwrap_optimized();
                assert_eq!(new_reserve_emission_data_1.last_time, 1501000000);
                assert_eq!(
                    new_user_emission_data_1.index,
                    new_reserve_emission_data_1.index
                );
                assert_eq!(new_user_emission_data.accrued, 0);
                assert_eq!(result, 400_3222222 + 301_0222222);

                // verify tokens are sent
                assert_eq!(blnd_token_client.balance(&merry), 400_3222222 + 301_0222222);
                assert_eq!(
                    blnd_token_client.balance(&backstop),
                    100_000_0000000 - (400_3222222 + 301_0222222)
                )
            });
        }

        #[test]
        fn test_execute_claim_with_already_claimed_reserve() {
            let e = Env::default();
            e.mock_all_auths_allowing_non_root_auth();
            e.cost_estimate().budget().reset_unlimited();

            let pool = testutils::create_pool(&e);
            let bombadil = Address::generate(&e);
            let samwise = Address::generate(&e);
            let merry = Address::generate(&e);

            let (blnd, blnd_token_client) = testutils::create_blnd_token(&e, &pool, &bombadil);
            let (backstop, _) = testutils::create_backstop(
                &e,
                &pool,
                &Address::generate(&e),
                &Address::generate(&e),
                &blnd,
            );
            // mock backstop having emissions for pool
            e.as_contract(&backstop, || {
                blnd_token_client.approve(&backstop, &pool, &100_000_0000000_i128, &1000000);
            });
            blnd_token_client.mint(&backstop, &100_000_0000000);

            e.ledger().set(LedgerInfo {
                timestamp: 1501000000, // 10^6 seconds have passed
                protocol_version: 22,
                sequence_number: 123,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 3110400,
            });

            let (underlying_0, _) = testutils::create_token_contract(&e, &bombadil);
            let (mut reserve_config, mut reserve_data) = testutils::default_reserve_meta();
            reserve_config.decimals = 5;
            reserve_data.b_supply = 100_00000;
            reserve_data.d_supply = 50_00000;
            testutils::create_reserve(&e, &pool, &underlying_0, &reserve_config, &reserve_data);

            let (underlying_1, _) = testutils::create_token_contract(&e, &bombadil);
            let (mut reserve_config, mut reserve_data) = testutils::default_reserve_meta();
            reserve_config.decimals = 9;
            reserve_config.index = 1;
            reserve_data.b_supply = 100_000_000_000;
            reserve_data.d_supply = 50_000_000_000;
            testutils::create_reserve(&e, &pool, &underlying_1, &reserve_config, &reserve_data);

            let user_positions = Positions {
                liabilities: map![&e, (0, 2_00000)],
                collateral: map![&e, (1, 1_000_000_000)],
                supply: map![&e, (1, 1_000_000_000)],
            };
            e.as_contract(&pool, || {
                storage::set_backstop(&e, &backstop);
                storage::set_user_positions(&e, &samwise, &user_positions);

                let reserve_emission_data_0 = ReserveEmissionData {
                    expiration: 1600000000,
                    eps: 0_01000000000000,
                    index: 23456780000000,
                    last_time: 1500000000,
                };
                let user_emission_data_0 = UserEmissionData {
                    index: 12345670000000,
                    accrued: 0_1000000,
                };
                let res_token_index_0 = 0 * 2 + 0; // d_token for reserve 0

                let reserve_emission_data_1 = ReserveEmissionData {
                    expiration: 1600000000,
                    eps: 0_01500000000000,
                    index: 13456780000000,
                    last_time: 1501000000,
                };
                let user_emission_data_1 = UserEmissionData {
                    index: 13456780000000,
                    accrued: 0,
                };
                let res_token_index_1 = 1 * 2 + 1; // b_token for reserve 1

                storage::set_res_emis_data(&e, &res_token_index_0, &reserve_emission_data_0);
                storage::set_user_emissions(
                    &e,
                    &samwise,
                    &res_token_index_0,
                    &user_emission_data_0,
                );

                storage::set_res_emis_data(&e, &res_token_index_1, &reserve_emission_data_1);
                storage::set_user_emissions(
                    &e,
                    &samwise,
                    &res_token_index_1,
                    &user_emission_data_1,
                );

                let reserve_token_ids: Vec<u32> = vec![&e, res_token_index_0, res_token_index_1];
                let result = execute_claim(&e, &samwise, &reserve_token_ids, &merry);

                let new_reserve_emission_data =
                    storage::get_res_emis_data(&e, &res_token_index_0).unwrap_optimized();
                let new_user_emission_data =
                    storage::get_user_emissions(&e, &samwise, &res_token_index_0)
                        .unwrap_optimized();
                assert_eq!(new_reserve_emission_data.last_time, 1501000000);
                assert_eq!(
                    new_user_emission_data.index,
                    new_reserve_emission_data.index
                );
                assert_eq!(new_user_emission_data.accrued, 0);

                let new_reserve_emission_data_1 =
                    storage::get_res_emis_data(&e, &res_token_index_1).unwrap_optimized();
                let new_user_emission_data_1 =
                    storage::get_user_emissions(&e, &samwise, &res_token_index_1)
                        .unwrap_optimized();
                assert_eq!(new_reserve_emission_data_1.last_time, 1501000000);
                assert_eq!(
                    new_user_emission_data_1.index,
                    new_reserve_emission_data_1.index
                );
                assert_eq!(new_user_emission_data.accrued, 0);
                assert_eq!(result, 400_3222222);

                // verify tokens are sent
                assert_eq!(blnd_token_client.balance(&merry), 400_3222222);
                assert_eq!(
                    blnd_token_client.balance(&backstop),
                    100_000_0000000 - 400_3222222
                )
            });
        }

        #[test]
        #[should_panic(expected = "Error(Contract, #1200)")]
        fn test_calc_claim_with_invalid_reserve_panics() {
            let e = Env::default();
            e.mock_all_auths_allowing_non_root_auth();
            e.cost_estimate().budget().reset_unlimited();

            let pool = testutils::create_pool(&e);
            let bombadil = Address::generate(&e);
            let samwise = Address::generate(&e);
            let merry = Address::generate(&e);
            let (blnd, blnd_token_client) = testutils::create_blnd_token(&e, &pool, &bombadil);

            let (backstop, _) = testutils::create_backstop(
                &e,
                &pool,
                &Address::generate(&e),
                &Address::generate(&e),
                &blnd,
            );
            // mock backstop having emissions for pool
            e.as_contract(&backstop, || {
                blnd_token_client.approve(&backstop, &pool, &100_000_0000000_i128, &1000000);
            });
            blnd_token_client.mint(&backstop, &100_000_0000000);

            e.ledger().set(LedgerInfo {
                timestamp: 1501000000, // 10^6 seconds have passed
                protocol_version: 22,
                sequence_number: 123,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 3110400,
            });

            let (underlying_0, _) = testutils::create_token_contract(&e, &bombadil);
            let (mut reserve_config, mut reserve_data) = testutils::default_reserve_meta();
            reserve_config.decimals = 5;
            reserve_data.b_supply = 100_00000;
            reserve_data.d_supply = 50_00000;
            testutils::create_reserve(&e, &pool, &underlying_0, &reserve_config, &reserve_data);

            let (underlying_1, _) = testutils::create_token_contract(&e, &bombadil);
            let (mut reserve_config, mut reserve_data) = testutils::default_reserve_meta();
            reserve_config.decimals = 9;
            reserve_config.index = 1;
            reserve_data.b_supply = 100_000_000_000;
            reserve_data.d_supply = 50_000_000_000;
            testutils::create_reserve(&e, &pool, &underlying_1, &reserve_config, &reserve_data);

            let user_positions = Positions {
                liabilities: map![&e, (0, 2_00000)],
                collateral: map![&e, (1, 1_000_000_000)],
                supply: map![&e, (1, 1_000_000_000)],
            };
            e.as_contract(&pool, || {
                storage::set_backstop(&e, &backstop);
                storage::set_user_positions(&e, &samwise, &user_positions);

                let reserve_emission_data_0 = ReserveEmissionData {
                    expiration: 1600000000,
                    eps: 0_01000000000000,
                    index: 2345678,
                    last_time: 1500000000,
                };
                let user_emission_data_0 = UserEmissionData {
                    index: 1234567,
                    accrued: 0_1000000,
                };
                let res_token_index_0 = 0 * 2 + 0; // d_token for reserve 0

                let reserve_emission_data_1 = ReserveEmissionData {
                    expiration: 1600000000,
                    eps: 0_01500000000000,
                    index: 1345678,
                    last_time: 1500000000,
                };
                let user_emission_data_1 = UserEmissionData {
                    index: 1234567,
                    accrued: 1_0000000,
                };
                let res_token_index_1 = 1 * 2 + 1; // b_token for reserve 1

                storage::set_res_emis_data(&e, &res_token_index_0, &reserve_emission_data_0);
                storage::set_user_emissions(
                    &e,
                    &samwise,
                    &res_token_index_0,
                    &user_emission_data_0,
                );

                storage::set_res_emis_data(&e, &res_token_index_1, &reserve_emission_data_1);
                storage::set_user_emissions(
                    &e,
                    &samwise,
                    &res_token_index_1,
                    &user_emission_data_1,
                );

                let reserve_token_ids: Vec<u32> = vec![&e, res_token_index_0, res_token_index_1, 6];
                execute_claim(&e, &samwise, &reserve_token_ids, &merry);

                assert_eq!(blnd_token_client.balance(&backstop), 100_000_0000000)
            });
        }
    }
}

mod pool_src_auctions_bad_debt_auction {
    use crate::{
        constants::SCALAR_7,
        dependencies::BackstopClient,
        errors::PoolError,
        pool::{check_and_handle_backstop_bad_debt, Pool, User},
        storage,
    };

    use cast::i128;

    use soroban_fixed_point_math::SorobanFixedPoint;

    use soroban_sdk::{map, panic_with_error, Address, Env, Vec};

    use crate::auctions::{AuctionData, AuctionType};

    pub(crate) use crate::auctions::bad_debt_auction::*;

    mod tests {
        use crate::{
            auctions::auction::AuctionType,
            pool::Positions,
            storage::PoolConfig,
            testutils::{self, create_pool},
        };

        use super::*;
        use sep_40_oracle::testutils::Asset;
        use soroban_sdk::{
            testutils::{Address as _, Ledger, LedgerInfo},
            vec, Symbol,
        };

        #[test]
        #[should_panic(expected = "Error(Contract, #1212)")]
        fn test_create_bad_debt_auction_already_in_progress() {
            let e = Env::default();
            e.mock_all_auths_allowing_non_root_auth();
            e.cost_estimate().budget().reset_unlimited(); // setup exhausts budget

            let pool_address = create_pool(&e);
            let bombadil = Address::generate(&e);
            let samwise = Address::generate(&e);

            let (blnd, blnd_client) = testutils::create_blnd_token(&e, &pool_address, &bombadil);
            let (usdc, usdc_client) = testutils::create_token_contract(&e, &bombadil);
            let (lp_token, lp_token_client) =
                testutils::create_comet_lp_pool(&e, &bombadil, &blnd, &usdc);
            let (backstop_address, backstop_client) =
                testutils::create_backstop(&e, &pool_address, &lp_token, &usdc, &blnd);

            // mint lp tokens
            blnd_client.mint(&samwise, &500_001_0000000);
            blnd_client.approve(&samwise, &lp_token, &i128::MAX, &99999);
            usdc_client.mint(&samwise, &12_501_0000000);
            usdc_client.approve(&samwise, &lp_token, &i128::MAX, &99999);
            lp_token_client.join_pool(
                &50_000_0000000,
                &vec![&e, 500_001_0000000, 12_501_0000000],
                &samwise,
            );
            backstop_client.deposit(&samwise, &pool_address, &50_000_0000000);

            e.ledger().set(LedgerInfo {
                timestamp: 12345,
                protocol_version: 22,
                sequence_number: 100,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 3110400,
            });

            let auction_data = AuctionData {
                bid: map![&e],
                lot: map![&e],
                block: 50,
            };
            e.as_contract(&pool_address, || {
                storage::set_auction(
                    &e,
                    &(AuctionType::BadDebtAuction as u32),
                    &backstop_address,
                    &auction_data,
                );

                create_bad_debt_auction_data(
                    &e,
                    &backstop_address,
                    &vec![&e],
                    &vec![&e, lp_token.clone()],
                    100,
                );
            });
        }

        #[test]
        #[should_panic(expected = "Error(Contract, #1200)")]
        fn test_create_bad_debt_auction_user_not_backstop() {
            let e = Env::default();
            e.mock_all_auths_allowing_non_root_auth();
            e.cost_estimate().budget().reset_unlimited(); // setup exhausts budget

            let pool_address = create_pool(&e);
            let bombadil = Address::generate(&e);
            let samwise = Address::generate(&e);

            let (blnd, blnd_client) = testutils::create_blnd_token(&e, &pool_address, &bombadil);
            let (usdc, usdc_client) = testutils::create_token_contract(&e, &bombadil);
            let (lp_token, lp_token_client) =
                testutils::create_comet_lp_pool(&e, &bombadil, &blnd, &usdc);
            let (_, backstop_client) =
                testutils::create_backstop(&e, &pool_address, &lp_token, &usdc, &blnd);

            // mint lp tokens
            blnd_client.mint(&samwise, &500_001_0000000);
            blnd_client.approve(&samwise, &lp_token, &i128::MAX, &99999);
            usdc_client.mint(&samwise, &12_501_0000000);
            usdc_client.approve(&samwise, &lp_token, &i128::MAX, &99999);
            lp_token_client.join_pool(
                &50_000_0000000,
                &vec![&e, 500_001_0000000, 12_501_0000000],
                &samwise,
            );
            backstop_client.deposit(&samwise, &pool_address, &50_000_0000000);

            e.ledger().set(LedgerInfo {
                timestamp: 12345,
                protocol_version: 22,
                sequence_number: 100,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 3110400,
            });

            e.as_contract(&pool_address, || {
                create_bad_debt_auction_data(
                    &e,
                    &samwise,
                    &vec![&e],
                    &vec![&e, lp_token.clone()],
                    100,
                );
            });
        }

        #[test]
        #[should_panic(expected = "Error(Contract, #1200)")]
        fn test_create_bad_debt_auction_percent_not_100() {
            let e = Env::default();
            e.mock_all_auths_allowing_non_root_auth();
            e.cost_estimate().budget().reset_unlimited(); // setup exhausts budget

            let pool_address = create_pool(&e);
            let bombadil = Address::generate(&e);
            let samwise = Address::generate(&e);

            let (blnd, blnd_client) = testutils::create_blnd_token(&e, &pool_address, &bombadil);
            let (usdc, usdc_client) = testutils::create_token_contract(&e, &bombadil);
            let (lp_token, lp_token_client) =
                testutils::create_comet_lp_pool(&e, &bombadil, &blnd, &usdc);
            let (backstop_address, backstop_client) =
                testutils::create_backstop(&e, &pool_address, &lp_token, &usdc, &blnd);

            // mint lp tokens
            blnd_client.mint(&samwise, &500_001_0000000);
            blnd_client.approve(&samwise, &lp_token, &i128::MAX, &99999);
            usdc_client.mint(&samwise, &12_501_0000000);
            usdc_client.approve(&samwise, &lp_token, &i128::MAX, &99999);
            lp_token_client.join_pool(
                &50_000_0000000,
                &vec![&e, 500_001_0000000, 12_501_0000000],
                &samwise,
            );
            backstop_client.deposit(&samwise, &pool_address, &50_000_0000000);

            e.ledger().set(LedgerInfo {
                timestamp: 12345,
                protocol_version: 22,
                sequence_number: 100,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 3110400,
            });

            let auction_data = AuctionData {
                bid: map![&e],
                lot: map![&e],
                block: 50,
            };
            e.as_contract(&pool_address, || {
                storage::set_auction(
                    &e,
                    &(AuctionType::BadDebtAuction as u32),
                    &backstop_address,
                    &auction_data,
                );

                create_bad_debt_auction_data(
                    &e,
                    &backstop_address,
                    &vec![&e],
                    &vec![&e, lp_token.clone()],
                    99,
                );
            });
        }

        #[test]
        #[should_panic]
        fn test_create_bad_debt_auction_invalid_bid() {
            let e = Env::default();
            e.mock_all_auths_allowing_non_root_auth();
            e.cost_estimate().budget().reset_unlimited(); // setup exhausts budget

            e.ledger().set(LedgerInfo {
                timestamp: 12345,
                protocol_version: 22,
                sequence_number: 50,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 3110400,
            });

            let bombadil = Address::generate(&e);
            let samwise = Address::generate(&e);
            let pool_address = create_pool(&e);

            let (blnd, blnd_client) = testutils::create_blnd_token(&e, &pool_address, &bombadil);
            let (usdc, usdc_client) = testutils::create_token_contract(&e, &bombadil);
            let (lp_token, lp_token_client) =
                testutils::create_comet_lp_pool(&e, &bombadil, &blnd, &usdc);
            let (backstop_address, backstop_client) =
                testutils::create_backstop(&e, &pool_address, &lp_token, &usdc, &blnd);

            // mint lp tokens
            blnd_client.mint(&samwise, &500_001_0000000);
            blnd_client.approve(&samwise, &lp_token, &i128::MAX, &99999);
            usdc_client.mint(&samwise, &12_501_0000000);
            usdc_client.approve(&samwise, &lp_token, &i128::MAX, &99999);
            lp_token_client.join_pool(
                &50_000_0000000,
                &vec![&e, 500_001_0000000, 12_501_0000000],
                &samwise,
            );
            backstop_client.deposit(&samwise, &pool_address, &50_000_0000000);

            let (oracle_id, oracle_client) = testutils::create_mock_oracle(&e);

            let (underlying_0, _) = testutils::create_token_contract(&e, &bombadil);
            let (mut reserve_config_0, mut reserve_data_0) = testutils::default_reserve_meta();
            reserve_data_0.d_rate = 1_100_000_000_000;
            reserve_data_0.last_time = 12345;
            reserve_config_0.index = 0;
            testutils::create_reserve(
                &e,
                &pool_address,
                &underlying_0,
                &reserve_config_0,
                &reserve_data_0,
            );

            oracle_client.set_data(
                &bombadil,
                &Asset::Other(Symbol::new(&e, "USD")),
                &vec![
                    &e,
                    Asset::Stellar(underlying_0.clone()),
                    Asset::Stellar(usdc),
                ],
                &7,
                &300,
            );
            oracle_client.set_price_stable(&vec![&e, 2_0000000, 4_0000000]);

            let positions: Positions = Positions {
                collateral: map![&e],
                liabilities: map![&e, (reserve_config_0.index, 10_0000000),],
                supply: map![&e],
            };

            let pool_config = PoolConfig {
                oracle: oracle_id,
                min_collateral: 1_0000000,
                bstop_rate: 0_1000000,
                status: 0,
                max_positions: 4,
            };
            e.as_contract(&pool_address, || {
                storage::set_pool_config(&e, &pool_config);
                storage::set_user_positions(&e, &backstop_address, &positions);

                create_bad_debt_auction_data(
                    &e,
                    &backstop_address,
                    &vec![&e, lp_token.clone()],
                    &vec![&e, lp_token.clone()],
                    100,
                );
            });
        }

        #[test]
        #[should_panic(expected = "Error(Contract, #1221)")]
        fn test_create_bad_debt_auction_invalid_bid_no_position() {
            let e = Env::default();
            e.mock_all_auths_allowing_non_root_auth();
            e.cost_estimate().budget().reset_unlimited(); // setup exhausts budget

            e.ledger().set(LedgerInfo {
                timestamp: 12345,
                protocol_version: 22,
                sequence_number: 50,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 3110400,
            });

            let bombadil = Address::generate(&e);
            let samwise = Address::generate(&e);
            let pool_address = create_pool(&e);

            let (blnd, blnd_client) = testutils::create_blnd_token(&e, &pool_address, &bombadil);
            let (usdc, usdc_client) = testutils::create_token_contract(&e, &bombadil);
            let (lp_token, lp_token_client) =
                testutils::create_comet_lp_pool(&e, &bombadil, &blnd, &usdc);
            let (backstop_address, backstop_client) =
                testutils::create_backstop(&e, &pool_address, &lp_token, &usdc, &blnd);
            // mint lp tokens
            blnd_client.mint(&samwise, &500_001_0000000);
            blnd_client.approve(&samwise, &lp_token, &i128::MAX, &99999);
            usdc_client.mint(&samwise, &12_501_0000000);
            usdc_client.approve(&samwise, &lp_token, &i128::MAX, &99999);
            lp_token_client.join_pool(
                &50_000_0000000,
                &vec![&e, 500_001_0000000, 12_501_0000000],
                &samwise,
            );
            backstop_client.deposit(&samwise, &pool_address, &50_000_0000000);

            let (oracle_id, oracle_client) = testutils::create_mock_oracle(&e);

            let (underlying_0, _) = testutils::create_token_contract(&e, &bombadil);
            let (mut reserve_config_0, mut reserve_data_0) = testutils::default_reserve_meta();
            reserve_data_0.d_rate = 1_100_000_000_000;
            reserve_data_0.last_time = 12345;
            reserve_config_0.index = 0;
            testutils::create_reserve(
                &e,
                &pool_address,
                &underlying_0,
                &reserve_config_0,
                &reserve_data_0,
            );

            let (underlying_1, _) = testutils::create_token_contract(&e, &bombadil);
            let (mut reserve_config_1, mut reserve_data_1) = testutils::default_reserve_meta();
            reserve_data_1.last_time = 12345;
            reserve_config_1.index = 1;
            testutils::create_reserve(
                &e,
                &pool_address,
                &underlying_1,
                &reserve_config_1,
                &reserve_data_1,
            );

            oracle_client.set_data(
                &bombadil,
                &Asset::Other(Symbol::new(&e, "USD")),
                &vec![
                    &e,
                    Asset::Stellar(underlying_0.clone()),
                    Asset::Stellar(underlying_1.clone()),
                    Asset::Stellar(usdc),
                ],
                &7,
                &300,
            );
            oracle_client.set_price_stable(&vec![&e, 2_0000000, 4_0000000, 1_0000000]);

            let positions: Positions = Positions {
                collateral: map![&e],
                liabilities: map![&e, (reserve_config_0.index, 10_0000000),],
                supply: map![&e],
            };

            let pool_config = PoolConfig {
                oracle: oracle_id,
                min_collateral: 1_0000000,
                bstop_rate: 0_1000000,
                status: 0,
                max_positions: 4,
            };
            e.as_contract(&pool_address, || {
                storage::set_pool_config(&e, &pool_config);
                storage::set_user_positions(&e, &backstop_address, &positions);

                create_bad_debt_auction_data(
                    &e,
                    &backstop_address,
                    &vec![&e, underlying_0.clone(), underlying_1.clone()],
                    &vec![&e, lp_token.clone()],
                    100,
                );
            });
        }

        #[test]
        #[should_panic(expected = "Error(Contract, #1221)")]
        fn test_create_bad_debt_auction_invalid_bid_empty() {
            let e = Env::default();
            e.mock_all_auths_allowing_non_root_auth();
            e.cost_estimate().budget().reset_unlimited(); // setup exhausts budget

            e.ledger().set(LedgerInfo {
                timestamp: 12345,
                protocol_version: 22,
                sequence_number: 50,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 3110400,
            });

            let bombadil = Address::generate(&e);
            let samwise = Address::generate(&e);
            let pool_address = create_pool(&e);

            let (blnd, blnd_client) = testutils::create_blnd_token(&e, &pool_address, &bombadil);
            let (usdc, usdc_client) = testutils::create_token_contract(&e, &bombadil);
            let (lp_token, lp_token_client) =
                testutils::create_comet_lp_pool(&e, &bombadil, &blnd, &usdc);
            let (backstop_address, backstop_client) =
                testutils::create_backstop(&e, &pool_address, &lp_token, &usdc, &blnd);
            // mint lp tokens
            blnd_client.mint(&samwise, &500_001_0000000);
            blnd_client.approve(&samwise, &lp_token, &i128::MAX, &99999);
            usdc_client.mint(&samwise, &12_501_0000000);
            usdc_client.approve(&samwise, &lp_token, &i128::MAX, &99999);
            lp_token_client.join_pool(
                &50_000_0000000,
                &vec![&e, 500_001_0000000, 12_501_0000000],
                &samwise,
            );
            backstop_client.deposit(&samwise, &pool_address, &50_000_0000000);

            let (oracle_id, oracle_client) = testutils::create_mock_oracle(&e);

            let (underlying_0, _) = testutils::create_token_contract(&e, &bombadil);
            let (mut reserve_config_0, mut reserve_data_0) = testutils::default_reserve_meta();
            reserve_data_0.d_rate = 1_100_000_000_000;
            reserve_data_0.last_time = 12345;
            reserve_config_0.index = 0;
            testutils::create_reserve(
                &e,
                &pool_address,
                &underlying_0,
                &reserve_config_0,
                &reserve_data_0,
            );

            oracle_client.set_data(
                &bombadil,
                &Asset::Other(Symbol::new(&e, "USD")),
                &vec![
                    &e,
                    Asset::Stellar(underlying_0.clone()),
                    Asset::Stellar(usdc),
                ],
                &7,
                &300,
            );
            oracle_client.set_price_stable(&vec![&e, 2_0000000, 1_0000000]);

            let positions: Positions = Positions {
                collateral: map![&e],
                liabilities: map![&e, (reserve_config_0.index, 10_0000000),],
                supply: map![&e],
            };

            let pool_config = PoolConfig {
                oracle: oracle_id,
                min_collateral: 1_0000000,
                bstop_rate: 0_1000000,
                status: 0,
                max_positions: 4,
            };
            e.as_contract(&pool_address, || {
                storage::set_pool_config(&e, &pool_config);
                storage::set_user_positions(&e, &backstop_address, &positions);

                create_bad_debt_auction_data(
                    &e,
                    &backstop_address,
                    &vec![&e],
                    &vec![&e, lp_token.clone()],
                    100,
                );
            });
        }

        #[test]
        #[should_panic(expected = "Error(Contract, #1222)")]
        fn test_create_bad_debt_auction_invalid_lot() {
            let e = Env::default();
            e.mock_all_auths_allowing_non_root_auth();
            e.cost_estimate().budget().reset_unlimited(); // setup exhausts budget

            e.ledger().set(LedgerInfo {
                timestamp: 12345,
                protocol_version: 22,
                sequence_number: 50,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 3110400,
            });

            let bombadil = Address::generate(&e);
            let samwise = Address::generate(&e);
            let pool_address = create_pool(&e);

            let (blnd, blnd_client) = testutils::create_blnd_token(&e, &pool_address, &bombadil);
            let (usdc, usdc_client) = testutils::create_token_contract(&e, &bombadil);
            let (lp_token, lp_token_client) =
                testutils::create_comet_lp_pool(&e, &bombadil, &blnd, &usdc);
            let (backstop_address, backstop_client) =
                testutils::create_backstop(&e, &pool_address, &lp_token, &usdc, &blnd);
            // mint lp tokens
            blnd_client.mint(&samwise, &500_001_0000000);
            blnd_client.approve(&samwise, &lp_token, &i128::MAX, &99999);
            usdc_client.mint(&samwise, &12_501_0000000);
            usdc_client.approve(&samwise, &lp_token, &i128::MAX, &99999);
            lp_token_client.join_pool(
                &50_000_0000000,
                &vec![&e, 500_001_0000000, 12_501_0000000],
                &samwise,
            );
            backstop_client.deposit(&samwise, &pool_address, &50_000_0000000);

            let (oracle_id, oracle_client) = testutils::create_mock_oracle(&e);

            let (underlying_0, _) = testutils::create_token_contract(&e, &bombadil);
            let (mut reserve_config_0, mut reserve_data_0) = testutils::default_reserve_meta();
            reserve_data_0.d_rate = 1_100_000_000_000;
            reserve_data_0.last_time = 12345;
            reserve_config_0.index = 0;
            testutils::create_reserve(
                &e,
                &pool_address,
                &underlying_0,
                &reserve_config_0,
                &reserve_data_0,
            );

            oracle_client.set_data(
                &bombadil,
                &Asset::Other(Symbol::new(&e, "USD")),
                &vec![
                    &e,
                    Asset::Stellar(underlying_0.clone()),
                    Asset::Stellar(usdc),
                ],
                &7,
                &300,
            );
            oracle_client.set_price_stable(&vec![&e, 2_0000000, 4_0000000]);

            let positions: Positions = Positions {
                collateral: map![&e],
                liabilities: map![&e, (reserve_config_0.index, 10_0000000),],
                supply: map![&e],
            };

            let pool_config = PoolConfig {
                oracle: oracle_id,
                min_collateral: 1_0000000,
                bstop_rate: 0_1000000,
                status: 0,
                max_positions: 4,
            };
            e.as_contract(&pool_address, || {
                storage::set_pool_config(&e, &pool_config);
                storage::set_user_positions(&e, &backstop_address, &positions);

                create_bad_debt_auction_data(
                    &e,
                    &backstop_address,
                    &vec![&e, underlying_0.clone()],
                    &vec![&e, underlying_0.clone()],
                    100,
                );
            });
        }

        #[test]
        #[should_panic(expected = "Error(Contract, #1222)")]
        fn test_create_bad_debt_auction_no_backstop_tokens() {
            let e = Env::default();
            e.mock_all_auths_allowing_non_root_auth();
            e.cost_estimate().budget().reset_unlimited(); // setup exhausts budget

            e.ledger().set(LedgerInfo {
                timestamp: 12345,
                protocol_version: 22,
                sequence_number: 50,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 3110400,
            });

            let bombadil = Address::generate(&e);
            let samwise = Address::generate(&e);
            let pool_address = create_pool(&e);

            let (blnd, blnd_client) = testutils::create_blnd_token(&e, &pool_address, &bombadil);
            let (usdc, usdc_client) = testutils::create_token_contract(&e, &bombadil);
            let (lp_token, lp_token_client) =
                testutils::create_comet_lp_pool(&e, &bombadil, &blnd, &usdc);
            let (backstop_address, _) =
                testutils::create_backstop(&e, &pool_address, &lp_token, &usdc, &blnd);
            // mint lp tokens
            blnd_client.mint(&samwise, &500_001_0000000);
            blnd_client.approve(&samwise, &lp_token, &i128::MAX, &99999);
            usdc_client.mint(&samwise, &12_501_0000000);
            usdc_client.approve(&samwise, &lp_token, &i128::MAX, &99999);
            lp_token_client.join_pool(
                &50_000_0000000,
                &vec![&e, 500_001_0000000, 12_501_0000000],
                &samwise,
            );

            let (oracle_id, oracle_client) = testutils::create_mock_oracle(&e);

            let (underlying_0, _) = testutils::create_token_contract(&e, &bombadil);
            let (mut reserve_config_0, mut reserve_data_0) = testutils::default_reserve_meta();
            reserve_data_0.d_rate = 1_100_000_000_000;
            reserve_data_0.last_time = 12345;
            reserve_config_0.index = 0;
            testutils::create_reserve(
                &e,
                &pool_address,
                &underlying_0,
                &reserve_config_0,
                &reserve_data_0,
            );

            oracle_client.set_data(
                &bombadil,
                &Asset::Other(Symbol::new(&e, "USD")),
                &vec![
                    &e,
                    Asset::Stellar(underlying_0.clone()),
                    Asset::Stellar(usdc),
                ],
                &7,
                &300,
            );
            oracle_client.set_price_stable(&vec![&e, 2_0000000, 4_0000000]);

            let positions: Positions = Positions {
                collateral: map![&e],
                liabilities: map![&e, (reserve_config_0.index, 10_0000000),],
                supply: map![&e],
            };

            let pool_config = PoolConfig {
                oracle: oracle_id,
                min_collateral: 1_0000000,
                bstop_rate: 0_1000000,
                status: 0,
                max_positions: 4,
            };
            e.as_contract(&pool_address, || {
                storage::set_pool_config(&e, &pool_config);
                storage::set_user_positions(&e, &backstop_address, &positions);

                create_bad_debt_auction_data(
                    &e,
                    &backstop_address,
                    &vec![&e, underlying_0.clone()],
                    &vec![&e, lp_token.clone()],
                    100,
                );
            });
        }

        #[test]
        #[should_panic(expected = "Error(Contract, #1208)")]
        fn test_create_bad_debt_auction_checks_max_positions() {
            let e = Env::default();
            e.mock_all_auths_allowing_non_root_auth();
            e.cost_estimate().budget().reset_unlimited(); // setup exhausts budget

            e.ledger().set(LedgerInfo {
                timestamp: 12345,
                protocol_version: 22,
                sequence_number: 50,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 3110400,
            });

            let bombadil = Address::generate(&e);
            let samwise = Address::generate(&e);
            let pool_address = create_pool(&e);

            let (blnd, blnd_client) = testutils::create_blnd_token(&e, &pool_address, &bombadil);
            let (usdc, usdc_client) = testutils::create_token_contract(&e, &bombadil);
            let (lp_token, lp_token_client) =
                testutils::create_comet_lp_pool(&e, &bombadil, &blnd, &usdc);
            let (backstop_address, backstop_client) =
                testutils::create_backstop(&e, &pool_address, &lp_token, &usdc, &blnd);
            // mint lp tokens
            blnd_client.mint(&samwise, &500_001_0000000);
            blnd_client.approve(&samwise, &lp_token, &i128::MAX, &99999);
            usdc_client.mint(&samwise, &12_501_0000000);
            usdc_client.approve(&samwise, &lp_token, &i128::MAX, &99999);
            lp_token_client.join_pool(
                &50_000_0000000,
                &vec![&e, 500_001_0000000, 12_501_0000000],
                &samwise,
            );
            backstop_client.deposit(&samwise, &pool_address, &50_000_0000000);

            let (oracle_id, oracle_client) = testutils::create_mock_oracle(&e);

            let (underlying_0, _) = testutils::create_token_contract(&e, &bombadil);
            let (mut reserve_config_0, mut reserve_data_0) = testutils::default_reserve_meta();
            reserve_data_0.d_rate = 1_100_000_000_000;
            reserve_data_0.last_time = 12345;
            reserve_config_0.index = 0;
            testutils::create_reserve(
                &e,
                &pool_address,
                &underlying_0,
                &reserve_config_0,
                &reserve_data_0,
            );

            let (underlying_1, _) = testutils::create_token_contract(&e, &bombadil);
            let (mut reserve_config_1, mut reserve_data_1) = testutils::default_reserve_meta();
            reserve_data_1.d_rate = 1_200_000_000_000;
            reserve_data_1.last_time = 12345;
            reserve_config_1.index = 1;
            testutils::create_reserve(
                &e,
                &pool_address,
                &underlying_1,
                &reserve_config_1,
                &reserve_data_1,
            );

            let (underlying_2, _) = testutils::create_token_contract(&e, &bombadil);
            let (mut reserve_config_2, mut reserve_data_2) = testutils::default_reserve_meta();
            reserve_data_2.b_rate = 1_100_000_000_000;
            reserve_data_2.last_time = 12345;
            reserve_config_2.index = 1;
            testutils::create_reserve(
                &e,
                &pool_address,
                &underlying_2,
                &reserve_config_2,
                &reserve_data_2,
            );

            oracle_client.set_data(
                &bombadil,
                &Asset::Other(Symbol::new(&e, "USD")),
                &vec![
                    &e,
                    Asset::Stellar(underlying_0.clone()),
                    Asset::Stellar(underlying_1.clone()),
                    Asset::Stellar(underlying_2.clone()),
                    Asset::Stellar(usdc),
                ],
                &7,
                &300,
            );
            oracle_client.set_price_stable(&vec![&e, 2_0000000, 4_0000000, 100_0000000, 1_0000000]);

            let positions: Positions = Positions {
                collateral: map![&e],
                liabilities: map![
                    &e,
                    (reserve_config_0.index, 10_0000000),
                    (reserve_config_1.index, 2_5000000),
                    (reserve_config_2.index, 2_5000000)
                ],
                supply: map![&e],
            };

            let pool_config = PoolConfig {
                oracle: oracle_id,
                min_collateral: 1_0000000,
                bstop_rate: 0_1000000,
                status: 0,
                max_positions: 3,
            };
            e.as_contract(&pool_address, || {
                storage::set_pool_config(&e, &pool_config);
                storage::set_user_positions(&e, &backstop_address, &positions);

                create_bad_debt_auction_data(
                    &e,
                    &backstop_address,
                    &vec![
                        &e,
                        underlying_0.clone(),
                        underlying_1.clone(),
                        underlying_2.clone(),
                    ],
                    &vec![&e, lp_token.clone()],
                    100,
                );
            });
        }

        #[test]
        fn test_create_bad_debt_auction() {
            let e = Env::default();
            e.mock_all_auths_allowing_non_root_auth();
            e.cost_estimate().budget().reset_unlimited(); // setup exhausts budget

            e.ledger().set(LedgerInfo {
                timestamp: 12345,
                protocol_version: 22,
                sequence_number: 50,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 3110400,
            });

            let bombadil = Address::generate(&e);
            let samwise = Address::generate(&e);
            let pool_address = create_pool(&e);

            let (blnd, blnd_client) = testutils::create_blnd_token(&e, &pool_address, &bombadil);
            let (usdc, usdc_client) = testutils::create_token_contract(&e, &bombadil);
            let (lp_token, lp_token_client) =
                testutils::create_comet_lp_pool(&e, &bombadil, &blnd, &usdc);
            let (backstop_address, backstop_client) =
                testutils::create_backstop(&e, &pool_address, &lp_token, &usdc, &blnd);
            // mint lp tokens
            blnd_client.mint(&samwise, &500_001_0000000);
            blnd_client.approve(&samwise, &lp_token, &i128::MAX, &99999);
            usdc_client.mint(&samwise, &12_501_0000000);
            usdc_client.approve(&samwise, &lp_token, &i128::MAX, &99999);
            lp_token_client.join_pool(
                &50_000_0000000,
                &vec![&e, 500_001_0000000, 12_501_0000000],
                &samwise,
            );
            backstop_client.deposit(&samwise, &pool_address, &50_000_0000000);

            let (oracle_id, oracle_client) = testutils::create_mock_oracle(&e);

            let (underlying_0, _) = testutils::create_token_contract(&e, &bombadil);
            let (mut reserve_config_0, mut reserve_data_0) = testutils::default_reserve_meta();
            reserve_data_0.d_rate = 1_100_000_000_000;
            reserve_data_0.last_time = 12345;
            reserve_config_0.index = 0;
            testutils::create_reserve(
                &e,
                &pool_address,
                &underlying_0,
                &reserve_config_0,
                &reserve_data_0,
            );

            let (underlying_1, _) = testutils::create_token_contract(&e, &bombadil);
            let (mut reserve_config_1, mut reserve_data_1) = testutils::default_reserve_meta();
            reserve_data_1.d_rate = 1_200_000_000_000;
            reserve_data_1.last_time = 12345;
            reserve_config_1.index = 1;
            testutils::create_reserve(
                &e,
                &pool_address,
                &underlying_1,
                &reserve_config_1,
                &reserve_data_1,
            );

            let (underlying_2, _) = testutils::create_token_contract(&e, &bombadil);
            let (mut reserve_config_2, mut reserve_data_2) = testutils::default_reserve_meta();
            reserve_data_2.b_rate = 1_100_000_000_000;
            reserve_data_2.last_time = 12345;
            reserve_config_2.index = 1;
            testutils::create_reserve(
                &e,
                &pool_address,
                &underlying_2,
                &reserve_config_2,
                &reserve_data_2,
            );

            oracle_client.set_data(
                &bombadil,
                &Asset::Other(Symbol::new(&e, "USD")),
                &vec![
                    &e,
                    Asset::Stellar(underlying_0.clone()),
                    Asset::Stellar(underlying_1.clone()),
                    Asset::Stellar(underlying_2),
                    Asset::Stellar(usdc),
                ],
                &7,
                &300,
            );
            oracle_client.set_price_stable(&vec![&e, 2_0000000, 4_0000000, 100_0000000, 1_0000000]);

            let positions: Positions = Positions {
                collateral: map![&e],
                liabilities: map![
                    &e,
                    (reserve_config_0.index, 10_0000000),
                    (reserve_config_1.index, 2_5000000)
                ],
                supply: map![&e],
            };

            let pool_config = PoolConfig {
                oracle: oracle_id,
                min_collateral: 1_0000000,
                bstop_rate: 0_1000000,
                status: 0,
                max_positions: 3,
            };
            e.as_contract(&pool_address, || {
                storage::set_pool_config(&e, &pool_config);
                storage::set_user_positions(&e, &backstop_address, &positions);

                let result = create_bad_debt_auction_data(
                    &e,
                    &backstop_address,
                    &vec![&e, underlying_0.clone(), underlying_1.clone()],
                    &vec![&e, lp_token.clone()],
                    100,
                );

                assert_eq!(result.block, 51);
                assert_eq!(result.bid.get_unchecked(underlying_0), 10_0000000);
                assert_eq!(result.bid.get_unchecked(underlying_1), 2_5000000);
                assert_eq!(result.bid.len(), 2);
                assert_eq!(result.lot.get_unchecked(lp_token), 32_6400000);
                assert_eq!(result.lot.len(), 1);
            });
        }

        #[test]
        fn test_create_bad_debt_auction_oracle_14_decimals() {
            let e = Env::default();
            e.mock_all_auths_allowing_non_root_auth();
            e.cost_estimate().budget().reset_unlimited(); // setup exhausts budget

            e.ledger().set(LedgerInfo {
                timestamp: 12345,
                protocol_version: 22,
                sequence_number: 50,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 3110400,
            });

            let bombadil = Address::generate(&e);
            let samwise = Address::generate(&e);
            let pool_address = create_pool(&e);

            let (blnd, blnd_client) = testutils::create_blnd_token(&e, &pool_address, &bombadil);
            let (usdc, usdc_client) = testutils::create_token_contract(&e, &bombadil);
            let (lp_token, lp_token_client) =
                testutils::create_comet_lp_pool(&e, &bombadil, &blnd, &usdc);
            let (backstop_address, backstop_client) =
                testutils::create_backstop(&e, &pool_address, &lp_token, &usdc, &blnd);
            // mint lp tokens
            blnd_client.mint(&samwise, &500_001_0000000);
            blnd_client.approve(&samwise, &lp_token, &i128::MAX, &99999);
            usdc_client.mint(&samwise, &12_501_0000000);
            usdc_client.approve(&samwise, &lp_token, &i128::MAX, &99999);
            lp_token_client.join_pool(
                &50_000_0000000,
                &vec![&e, 500_001_0000000, 12_501_0000000],
                &samwise,
            );
            backstop_client.deposit(&samwise, &pool_address, &50_000_0000000);

            let (oracle_id, oracle_client) = testutils::create_mock_oracle(&e);

            let (underlying_0, _) = testutils::create_token_contract(&e, &bombadil);
            let (mut reserve_config_0, mut reserve_data_0) = testutils::default_reserve_meta();
            reserve_data_0.d_rate = 1_100_000_000_000;
            reserve_data_0.last_time = 12345;
            reserve_config_0.index = 0;
            testutils::create_reserve(
                &e,
                &pool_address,
                &underlying_0,
                &reserve_config_0,
                &reserve_data_0,
            );

            let (underlying_1, _) = testutils::create_token_contract(&e, &bombadil);
            let (mut reserve_config_1, mut reserve_data_1) = testutils::default_reserve_meta();
            reserve_data_1.d_rate = 1_200_000_000_000;
            reserve_data_1.last_time = 12345;
            reserve_config_1.index = 1;
            testutils::create_reserve(
                &e,
                &pool_address,
                &underlying_1,
                &reserve_config_1,
                &reserve_data_1,
            );

            let (underlying_2, _) = testutils::create_token_contract(&e, &bombadil);
            let (mut reserve_config_2, mut reserve_data_2) = testutils::default_reserve_meta();
            reserve_data_2.b_rate = 1_100_000_000_000;
            reserve_data_2.last_time = 12345;
            reserve_config_2.index = 1;
            testutils::create_reserve(
                &e,
                &pool_address,
                &underlying_2,
                &reserve_config_2,
                &reserve_data_2,
            );

            oracle_client.set_data(
                &bombadil,
                &Asset::Other(Symbol::new(&e, "USD")),
                &vec![
                    &e,
                    Asset::Stellar(underlying_0.clone()),
                    Asset::Stellar(underlying_1.clone()),
                    Asset::Stellar(underlying_2),
                    Asset::Stellar(usdc),
                ],
                &14,
                &300,
            );
            oracle_client.set_price_stable(&vec![
                &e,
                2_0000000_0000000,
                4_0000000_0000000,
                100_0000000_0000000,
                1_0000000_0000000,
            ]);

            let positions: Positions = Positions {
                collateral: map![&e],
                liabilities: map![
                    &e,
                    (reserve_config_0.index, 10_0000000),
                    (reserve_config_1.index, 2_5000000)
                ],
                supply: map![&e],
            };

            let pool_config = PoolConfig {
                oracle: oracle_id,
                min_collateral: 1_0000000,
                bstop_rate: 0_1000000,
                status: 0,
                max_positions: 4,
            };
            e.as_contract(&pool_address, || {
                storage::set_pool_config(&e, &pool_config);
                storage::set_user_positions(&e, &backstop_address, &positions);

                let result = create_bad_debt_auction_data(
                    &e,
                    &backstop_address,
                    &vec![&e, underlying_0.clone(), underlying_1.clone()],
                    &vec![&e, lp_token.clone()],
                    100,
                );

                assert_eq!(result.block, 51);
                assert_eq!(result.bid.get_unchecked(underlying_0), 10_0000000);
                assert_eq!(result.bid.get_unchecked(underlying_1), 2_5000000);
                assert_eq!(result.bid.len(), 2);
                assert_eq!(result.lot.get_unchecked(lp_token), 32_6400000);
                assert_eq!(result.lot.len(), 1);
            });
        }

        #[test]
        fn test_create_bad_debt_auction_oracle_2_decimals() {
            let e = Env::default();
            e.mock_all_auths_allowing_non_root_auth();
            e.cost_estimate().budget().reset_unlimited(); // setup exhausts budget

            e.ledger().set(LedgerInfo {
                timestamp: 12345,
                protocol_version: 22,
                sequence_number: 50,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 3110400,
            });

            let bombadil = Address::generate(&e);
            let samwise = Address::generate(&e);
            let pool_address = create_pool(&e);

            let (blnd, blnd_client) = testutils::create_blnd_token(&e, &pool_address, &bombadil);
            let (usdc, usdc_client) = testutils::create_token_contract(&e, &bombadil);
            let (lp_token, lp_token_client) =
                testutils::create_comet_lp_pool(&e, &bombadil, &blnd, &usdc);
            let (backstop_address, backstop_client) =
                testutils::create_backstop(&e, &pool_address, &lp_token, &usdc, &blnd);
            // mint lp tokens
            blnd_client.mint(&samwise, &500_001_0000000);
            blnd_client.approve(&samwise, &lp_token, &i128::MAX, &99999);
            usdc_client.mint(&samwise, &12_501_0000000);
            usdc_client.approve(&samwise, &lp_token, &i128::MAX, &99999);
            lp_token_client.join_pool(
                &50_000_0000000,
                &vec![&e, 500_001_0000000, 12_501_0000000],
                &samwise,
            );
            backstop_client.deposit(&samwise, &pool_address, &50_000_0000000);

            let (oracle_id, oracle_client) = testutils::create_mock_oracle(&e);

            let (underlying_0, _) = testutils::create_token_contract(&e, &bombadil);
            let (mut reserve_config_0, mut reserve_data_0) = testutils::default_reserve_meta();
            reserve_data_0.d_rate = 1_100_000_000_000;
            reserve_data_0.last_time = 12345;
            reserve_config_0.index = 0;
            testutils::create_reserve(
                &e,
                &pool_address,
                &underlying_0,
                &reserve_config_0,
                &reserve_data_0,
            );

            let (underlying_1, _) = testutils::create_token_contract(&e, &bombadil);
            let (mut reserve_config_1, mut reserve_data_1) = testutils::default_reserve_meta();
            reserve_data_1.d_rate = 1_200_000_000_000;
            reserve_data_1.last_time = 12345;
            reserve_config_1.index = 1;
            testutils::create_reserve(
                &e,
                &pool_address,
                &underlying_1,
                &reserve_config_1,
                &reserve_data_1,
            );

            let (underlying_2, _) = testutils::create_token_contract(&e, &bombadil);
            let (mut reserve_config_2, mut reserve_data_2) = testutils::default_reserve_meta();
            reserve_data_2.b_rate = 1_100_000_000_000;
            reserve_data_2.last_time = 12345;
            reserve_config_2.index = 1;
            testutils::create_reserve(
                &e,
                &pool_address,
                &underlying_2,
                &reserve_config_2,
                &reserve_data_2,
            );

            oracle_client.set_data(
                &bombadil,
                &Asset::Other(Symbol::new(&e, "USD")),
                &vec![
                    &e,
                    Asset::Stellar(underlying_0.clone()),
                    Asset::Stellar(underlying_1.clone()),
                    Asset::Stellar(underlying_2),
                    Asset::Stellar(usdc),
                ],
                &2,
                &300,
            );
            oracle_client.set_price_stable(&vec![&e, 2_00, 4_00, 100_00, 1_00]);

            let positions: Positions = Positions {
                collateral: map![&e],
                liabilities: map![
                    &e,
                    (reserve_config_0.index, 10_0000000),
                    (reserve_config_1.index, 2_5000000)
                ],
                supply: map![&e],
            };

            let pool_config = PoolConfig {
                oracle: oracle_id,
                min_collateral: 1_0000000,
                bstop_rate: 0_1000000,
                status: 0,
                max_positions: 4,
            };
            e.as_contract(&pool_address, || {
                storage::set_pool_config(&e, &pool_config);
                storage::set_user_positions(&e, &backstop_address, &positions);

                let result = create_bad_debt_auction_data(
                    &e,
                    &backstop_address,
                    &vec![&e, underlying_0.clone(), underlying_1.clone()],
                    &vec![&e, lp_token.clone()],
                    100,
                );

                assert_eq!(result.block, 51);
                assert_eq!(result.bid.get_unchecked(underlying_0), 10_0000000);
                assert_eq!(result.bid.get_unchecked(underlying_1), 2_5000000);
                assert_eq!(result.bid.len(), 2);
                assert_eq!(result.lot.get_unchecked(lp_token), 32_6400000);
                assert_eq!(result.lot.len(), 1);
            });
        }

        #[test]
        fn test_create_bad_debt_auction_max_balance() {
            let e = Env::default();
            e.mock_all_auths_allowing_non_root_auth();
            e.cost_estimate().budget().reset_unlimited(); // setup exhausts budget

            e.ledger().set(LedgerInfo {
                timestamp: 12345,
                protocol_version: 22,
                sequence_number: 50,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 3110400,
            });

            let bombadil = Address::generate(&e);
            let samwise = Address::generate(&e);

            let pool_address = create_pool(&e);
            let (blnd, blnd_client) = testutils::create_blnd_token(&e, &pool_address, &bombadil);
            let (usdc, usdc_client) = testutils::create_token_contract(&e, &bombadil);
            let (lp_token, lp_token_client) =
                testutils::create_comet_lp_pool(&e, &bombadil, &blnd, &usdc);
            let (backstop_address, backstop_client) =
                testutils::create_backstop(&e, &pool_address, &lp_token, &usdc, &blnd);
            // mint lp tokens - only deposit 32_0000000
            blnd_client.mint(&samwise, &500_001_0000000);
            blnd_client.approve(&samwise, &lp_token, &i128::MAX, &99999);
            usdc_client.mint(&samwise, &12_501_0000000);
            usdc_client.approve(&samwise, &lp_token, &i128::MAX, &99999);
            lp_token_client.join_pool(
                &50_000_0000000,
                &vec![&e, 500_001_0000000, 12_501_0000000],
                &samwise,
            );
            backstop_client.deposit(&samwise, &pool_address, &32_0000000);

            let (oracle_id, oracle_client) = testutils::create_mock_oracle(&e);

            let (underlying_0, _) = testutils::create_token_contract(&e, &bombadil);
            let (mut reserve_config_0, mut reserve_data_0) = testutils::default_reserve_meta();
            reserve_data_0.d_rate = 1_100_000_000_000;
            reserve_data_0.last_time = 12345;
            reserve_config_0.index = 0;
            testutils::create_reserve(
                &e,
                &pool_address,
                &underlying_0,
                &reserve_config_0,
                &reserve_data_0,
            );

            let (underlying_1, _) = testutils::create_token_contract(&e, &bombadil);
            let (mut reserve_config_1, mut reserve_data_1) = testutils::default_reserve_meta();
            reserve_data_1.d_rate = 1_200_000_000_000;
            reserve_data_1.last_time = 12345;
            reserve_config_1.index = 1;
            testutils::create_reserve(
                &e,
                &pool_address,
                &underlying_1,
                &reserve_config_1,
                &reserve_data_1,
            );

            let (underlying_2, _) = testutils::create_token_contract(&e, &bombadil);
            let (mut reserve_config_2, mut reserve_data_2) = testutils::default_reserve_meta();
            reserve_data_2.b_rate = 1_100_000_000_000;
            reserve_data_2.last_time = 12345;
            reserve_config_2.index = 1;
            testutils::create_reserve(
                &e,
                &pool_address,
                &underlying_2,
                &reserve_config_2,
                &reserve_data_2,
            );

            oracle_client.set_data(
                &bombadil,
                &Asset::Other(Symbol::new(&e, "USD")),
                &vec![
                    &e,
                    Asset::Stellar(underlying_0.clone()),
                    Asset::Stellar(underlying_1.clone()),
                    Asset::Stellar(underlying_2),
                    Asset::Stellar(usdc),
                ],
                &7,
                &300,
            );
            oracle_client.set_price_stable(&vec![&e, 2_0000000, 4_0000000, 100_0000000, 1_0000000]);

            let positions: Positions = Positions {
                collateral: map![&e],
                liabilities: map![
                    &e,
                    (reserve_config_0.index, 10_0000000),
                    (reserve_config_1.index, 2_5000000)
                ],
                supply: map![&e],
            };

            let pool_config = PoolConfig {
                oracle: oracle_id,
                min_collateral: 1_0000000,
                bstop_rate: 0_1000000,
                status: 0,
                max_positions: 4,
            };
            e.as_contract(&pool_address, || {
                storage::set_pool_config(&e, &pool_config);

                storage::set_user_positions(&e, &backstop_address, &positions);

                let result = create_bad_debt_auction_data(
                    &e,
                    &backstop_address,
                    &vec![&e, underlying_0.clone(), underlying_1.clone()],
                    &vec![&e, lp_token.clone()],
                    100,
                );

                assert_eq!(result.block, 51);
                assert_eq!(result.bid.get_unchecked(underlying_0), 10_0000000);
                assert_eq!(result.bid.get_unchecked(underlying_1), 2_5000000);
                assert_eq!(result.bid.len(), 2);
                assert_eq!(result.lot.get_unchecked(lp_token), 32_0000000);
                assert_eq!(result.lot.len(), 1);
            });
        }

        #[test]
        fn test_create_bad_debt_auction_applies_interest() {
            let e = Env::default();
            e.mock_all_auths_allowing_non_root_auth();
            e.cost_estimate().budget().reset_unlimited(); // setup exhausts budget

            e.ledger().set(LedgerInfo {
                timestamp: 12345,
                protocol_version: 22,
                sequence_number: 150,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 3110400,
            });

            let bombadil = Address::generate(&e);
            let samwise = Address::generate(&e);
            let pool_address = create_pool(&e);

            let (blnd, blnd_client) = testutils::create_blnd_token(&e, &pool_address, &bombadil);
            let (usdc, usdc_client) = testutils::create_token_contract(&e, &bombadil);
            let (lp_token, lp_token_client) =
                testutils::create_comet_lp_pool(&e, &bombadil, &blnd, &usdc);
            let (backstop_address, backstop_client) =
                testutils::create_backstop(&e, &pool_address, &lp_token, &usdc, &blnd);
            // mint lp tokens
            blnd_client.mint(&samwise, &500_001_0000000);
            blnd_client.approve(&samwise, &lp_token, &i128::MAX, &99999);
            usdc_client.mint(&samwise, &12_501_0000000);
            usdc_client.approve(&samwise, &lp_token, &i128::MAX, &99999);
            lp_token_client.join_pool(
                &50_000_0000000,
                &vec![&e, 500_001_0000000, 12_501_0000000],
                &samwise,
            );
            backstop_client.deposit(&samwise, &pool_address, &50_000_0000000);

            let (oracle_id, oracle_client) = testutils::create_mock_oracle(&e);

            let (underlying_0, _) = testutils::create_token_contract(&e, &bombadil);
            let (mut reserve_config_0, mut reserve_data_0) = testutils::default_reserve_meta();
            reserve_data_0.d_rate = 1_100_000_000_000;
            reserve_data_0.last_time = 11845;
            reserve_config_0.index = 0;
            testutils::create_reserve(
                &e,
                &pool_address,
                &underlying_0,
                &reserve_config_0,
                &reserve_data_0,
            );

            let (underlying_1, _) = testutils::create_token_contract(&e, &bombadil);
            let (mut reserve_config_1, mut reserve_data_1) = testutils::default_reserve_meta();
            reserve_data_1.d_rate = 1_200_000_000_000;
            reserve_data_1.last_time = 11845;
            reserve_config_1.index = 1;
            testutils::create_reserve(
                &e,
                &pool_address,
                &underlying_1,
                &reserve_config_1,
                &reserve_data_1,
            );

            let (underlying_2, _) = testutils::create_token_contract(&e, &bombadil);
            let (mut reserve_config_2, mut reserve_data_2) = testutils::default_reserve_meta();
            reserve_data_2.b_rate = 1_100_000_000_000;
            reserve_data_2.last_time = 11845;
            reserve_config_2.index = 1;
            testutils::create_reserve(
                &e,
                &pool_address,
                &underlying_2,
                &reserve_config_2,
                &reserve_data_2,
            );

            oracle_client.set_data(
                &bombadil,
                &Asset::Other(Symbol::new(&e, "USD")),
                &vec![
                    &e,
                    Asset::Stellar(underlying_0.clone()),
                    Asset::Stellar(underlying_1.clone()),
                    Asset::Stellar(underlying_2),
                    Asset::Stellar(usdc),
                ],
                &7,
                &300,
            );
            oracle_client.set_price_stable(&vec![&e, 2_0000000, 4_0000000, 100_0000000, 1_0000000]);

            let positions: Positions = Positions {
                collateral: map![&e],
                liabilities: map![
                    &e,
                    (reserve_config_0.index, 10_0000000),
                    (reserve_config_1.index, 2_5000000)
                ],
                supply: map![&e],
            };

            let pool_config = PoolConfig {
                oracle: oracle_id,
                min_collateral: 1_0000000,
                bstop_rate: 0_1000000,
                status: 0,
                max_positions: 4,
            };
            e.as_contract(&pool_address, || {
                storage::set_pool_config(&e, &pool_config);
                storage::set_backstop(&e, &backstop_address);
                storage::set_user_positions(&e, &backstop_address, &positions);

                let result = create_bad_debt_auction_data(
                    &e,
                    &backstop_address,
                    &vec![&e, underlying_0.clone(), underlying_1.clone()],
                    &vec![&e, lp_token.clone()],
                    100,
                );

                assert_eq!(result.block, 151);
                assert_eq!(result.bid.get_unchecked(underlying_0), 10_0000000);
                assert_eq!(result.bid.get_unchecked(underlying_1), 2_5000000);
                assert_eq!(result.bid.len(), 2);
                assert_eq!(result.lot.get_unchecked(lp_token), 32_6401624);
                assert_eq!(result.lot.len(), 1);
            });
        }

        #[test]
        fn test_create_bad_debt_auction_partial() {
            let e = Env::default();
            e.mock_all_auths_allowing_non_root_auth();
            e.cost_estimate().budget().reset_unlimited(); // setup exhausts budget

            e.ledger().set(LedgerInfo {
                timestamp: 12345,
                protocol_version: 22,
                sequence_number: 50,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 3110400,
            });

            let bombadil = Address::generate(&e);
            let samwise = Address::generate(&e);
            let pool_address = create_pool(&e);

            let (blnd, blnd_client) = testutils::create_blnd_token(&e, &pool_address, &bombadil);
            let (usdc, usdc_client) = testutils::create_token_contract(&e, &bombadil);
            let (lp_token, lp_token_client) =
                testutils::create_comet_lp_pool(&e, &bombadil, &blnd, &usdc);
            let (backstop_address, backstop_client) =
                testutils::create_backstop(&e, &pool_address, &lp_token, &usdc, &blnd);
            // mint lp tokens
            blnd_client.mint(&samwise, &500_001_0000000);
            blnd_client.approve(&samwise, &lp_token, &i128::MAX, &99999);
            usdc_client.mint(&samwise, &12_501_0000000);
            usdc_client.approve(&samwise, &lp_token, &i128::MAX, &99999);
            lp_token_client.join_pool(
                &50_000_0000000,
                &vec![&e, 500_001_0000000, 12_501_0000000],
                &samwise,
            );
            backstop_client.deposit(&samwise, &pool_address, &50_000_0000000);

            let (oracle_id, oracle_client) = testutils::create_mock_oracle(&e);

            let (underlying_0, _) = testutils::create_token_contract(&e, &bombadil);
            let (mut reserve_config_0, mut reserve_data_0) = testutils::default_reserve_meta();
            reserve_data_0.d_rate = 1_100_000_000_000;
            reserve_data_0.last_time = 12345;
            reserve_config_0.index = 0;
            testutils::create_reserve(
                &e,
                &pool_address,
                &underlying_0,
                &reserve_config_0,
                &reserve_data_0,
            );

            let (underlying_1, _) = testutils::create_token_contract(&e, &bombadil);
            let (mut reserve_config_1, mut reserve_data_1) = testutils::default_reserve_meta();
            reserve_data_1.d_rate = 1_200_000_000_000;
            reserve_data_1.last_time = 12345;
            reserve_config_1.index = 1;
            testutils::create_reserve(
                &e,
                &pool_address,
                &underlying_1,
                &reserve_config_1,
                &reserve_data_1,
            );

            let (underlying_2, _) = testutils::create_token_contract(&e, &bombadil);
            let (mut reserve_config_2, mut reserve_data_2) = testutils::default_reserve_meta();
            reserve_data_2.b_rate = 1_100_000_000_000;
            reserve_data_2.last_time = 12345;
            reserve_config_2.index = 1;
            testutils::create_reserve(
                &e,
                &pool_address,
                &underlying_2,
                &reserve_config_2,
                &reserve_data_2,
            );

            oracle_client.set_data(
                &bombadil,
                &Asset::Other(Symbol::new(&e, "USD")),
                &vec![
                    &e,
                    Asset::Stellar(underlying_0.clone()),
                    Asset::Stellar(underlying_1.clone()),
                    Asset::Stellar(underlying_2),
                    Asset::Stellar(usdc),
                ],
                &7,
                &300,
            );
            oracle_client.set_price_stable(&vec![&e, 2_0000000, 4_0000000, 100_0000000, 1_0000000]);

            let positions: Positions = Positions {
                collateral: map![&e],
                liabilities: map![
                    &e,
                    (reserve_config_0.index, 10_0000000),
                    (reserve_config_1.index, 2_5000000)
                ],
                supply: map![&e],
            };

            let pool_config = PoolConfig {
                oracle: oracle_id,
                min_collateral: 1_0000000,
                bstop_rate: 0_1000000,
                status: 0,
                max_positions: 4,
            };
            e.as_contract(&pool_address, || {
                storage::set_pool_config(&e, &pool_config);
                storage::set_user_positions(&e, &backstop_address, &positions);

                let result = create_bad_debt_auction_data(
                    &e,
                    &backstop_address,
                    &vec![&e, underlying_0.clone()],
                    &vec![&e, lp_token.clone()],
                    100,
                );

                assert_eq!(result.block, 51);
                assert_eq!(result.bid.get_unchecked(underlying_0), 10_0000000);
                assert_eq!(result.bid.len(), 1);
                assert_eq!(result.lot.get_unchecked(lp_token), 21_1200000);
                assert_eq!(result.lot.len(), 1);
            });
        }

        #[test]
        fn test_fill_bad_debt_auction() {
            let e = Env::default();
            e.mock_all_auths_allowing_non_root_auth();
            e.cost_estimate().budget().reset_unlimited(); // setup exhausts budget

            e.ledger().set(LedgerInfo {
                timestamp: 12345,
                protocol_version: 22,
                sequence_number: 51,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 3110400,
            });

            let bombadil = Address::generate(&e);
            let samwise = Address::generate(&e);

            let pool_address = create_pool(&e);

            let (blnd, blnd_client) = testutils::create_blnd_token(&e, &pool_address, &bombadil);
            let (usdc, usdc_client) = testutils::create_token_contract(&e, &bombadil);
            let (lp_token, lp_token_client) =
                testutils::create_comet_lp_pool(&e, &bombadil, &blnd, &usdc);
            let (backstop_address, backstop_client) =
                testutils::create_backstop(&e, &pool_address, &lp_token, &usdc, &blnd);
            // mint lp tokens
            blnd_client.mint(&samwise, &500_001_0000000);
            blnd_client.approve(&samwise, &lp_token, &i128::MAX, &99999);
            usdc_client.mint(&samwise, &12_501_0000000);
            usdc_client.approve(&samwise, &lp_token, &i128::MAX, &99999);
            lp_token_client.join_pool(
                &50_000_0000000,
                &vec![&e, 500_001_0000000, 12_501_0000000],
                &samwise,
            );
            backstop_client.deposit(&samwise, &pool_address, &50_000_0000000);

            let (underlying_0, _) = testutils::create_token_contract(&e, &bombadil);
            let (mut reserve_config_0, mut reserve_data_0) = testutils::default_reserve_meta();
            reserve_data_0.d_rate = 1_100_000_000_000;
            reserve_data_0.last_time = 12345;
            reserve_config_0.index = 0;
            testutils::create_reserve(
                &e,
                &pool_address,
                &underlying_0,
                &reserve_config_0,
                &reserve_data_0,
            );

            let (underlying_1, _) = testutils::create_token_contract(&e, &bombadil);
            let (mut reserve_config_1, mut reserve_data_1) = testutils::default_reserve_meta();
            reserve_data_1.d_rate = 1_200_000_000_000;
            reserve_data_1.last_time = 12345;
            reserve_config_1.index = 1;
            testutils::create_reserve(
                &e,
                &pool_address,
                &underlying_1,
                &reserve_config_1,
                &reserve_data_1,
            );

            let (underlying_2, _) = testutils::create_token_contract(&e, &bombadil);
            let (mut reserve_config_2, mut reserve_data_2) = testutils::default_reserve_meta();
            reserve_data_2.b_rate = 1_100_000_000_000;
            reserve_data_2.last_time = 12345;
            reserve_config_2.index = 1;
            testutils::create_reserve(
                &e,
                &pool_address,
                &underlying_2,
                &reserve_config_2,
                &reserve_data_2,
            );
            let pool_config = PoolConfig {
                oracle: Address::generate(&e),
                min_collateral: 1_0000000,
                bstop_rate: 0_1000000,
                status: 0,
                max_positions: 4,
            };
            let mut auction_data = AuctionData {
                bid: map![&e, (underlying_0, 10_0000000), (underlying_1, 2_5000000)],
                lot: map![&e, (lp_token.clone(), 47_6000000)],
                block: 51,
            };
            let positions: Positions = Positions {
                collateral: map![&e],
                liabilities: map![
                    &e,
                    (reserve_config_0.index, 10_0000000),
                    (reserve_config_1.index, 2_5000000)
                ],
                supply: map![&e],
            };

            e.as_contract(&pool_address, || {
                storage::set_auction(
                    &e,
                    &(AuctionType::BadDebtAuction as u32),
                    &backstop_address,
                    &auction_data,
                );
                storage::set_pool_config(&e, &pool_config);
                storage::set_user_positions(&e, &backstop_address, &positions);

                let mut pool = Pool::load(&e);
                let mut samwise_state = User::load(&e, &samwise);
                fill_bad_debt_auction(&e, &mut pool, &mut auction_data, &mut samwise_state, true);
                assert_eq!(
                    lp_token_client.balance(&backstop_address),
                    50_000_0000000 - 47_6000000
                );
                assert_eq!(lp_token_client.balance(&samwise), 47_6000000);
                let samwise_positions = samwise_state.positions;
                assert_eq!(
                    samwise_positions
                        .liabilities
                        .get(reserve_config_0.index)
                        .unwrap(),
                    10_0000000
                );
                assert_eq!(
                    samwise_positions
                        .liabilities
                        .get(reserve_config_1.index)
                        .unwrap(),
                    2_5000000
                );
                let backstop_positions = storage::get_user_positions(&e, &backstop_address);
                assert_eq!(backstop_positions.liabilities.len(), 0);
            });
        }

        #[test]
        fn test_fill_bad_debt_auction_leftover_debt_small_backstop_burns() {
            let e = Env::default();
            e.mock_all_auths_allowing_non_root_auth();
            e.cost_estimate().budget().reset_unlimited(); // setup exhausts budget

            e.ledger().set(LedgerInfo {
                timestamp: 12345,
                protocol_version: 22,
                sequence_number: 51,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 3110400,
            });

            let bombadil = Address::generate(&e);
            let samwise = Address::generate(&e);
            let pool_address = create_pool(&e);

            let (blnd, blnd_client) = testutils::create_blnd_token(&e, &pool_address, &bombadil);
            let (usdc, usdc_client) = testutils::create_token_contract(&e, &bombadil);
            let (lp_token, lp_token_client) =
                testutils::create_comet_lp_pool(&e, &bombadil, &blnd, &usdc);
            let (backstop_address, backstop_client) =
                testutils::create_backstop(&e, &pool_address, &lp_token, &usdc, &blnd);
            // mint lp tokens
            blnd_client.mint(&samwise, &500_001_0000000);
            blnd_client.approve(&samwise, &lp_token, &i128::MAX, &99999);
            usdc_client.mint(&samwise, &12_501_0000000);
            usdc_client.approve(&samwise, &lp_token, &i128::MAX, &99999);
            lp_token_client.join_pool(
                &50_000_0000000,
                &vec![&e, 500_001_0000000, 12_501_0000000],
                &samwise,
            );
            backstop_client.deposit(&samwise, &pool_address, &1_000_0000000);

            let (underlying_0, _) = testutils::create_token_contract(&e, &bombadil);
            let (mut reserve_config_0, mut reserve_data_0) = testutils::default_reserve_meta();
            reserve_data_0.d_rate = 1_100_000_000_000;
            reserve_data_0.last_time = 12345;
            reserve_config_0.index = 0;
            testutils::create_reserve(
                &e,
                &pool_address,
                &underlying_0,
                &reserve_config_0,
                &reserve_data_0,
            );

            let (underlying_1, _) = testutils::create_token_contract(&e, &bombadil);
            let (mut reserve_config_1, mut reserve_data_1) = testutils::default_reserve_meta();
            reserve_data_1.d_rate = 1_200_000_000_000;
            reserve_data_1.last_time = 12345;
            reserve_config_1.index = 1;
            testutils::create_reserve(
                &e,
                &pool_address,
                &underlying_1,
                &reserve_config_1,
                &reserve_data_1,
            );

            let (underlying_2, _) = testutils::create_token_contract(&e, &bombadil);
            let (mut reserve_config_2, mut reserve_data_2) = testutils::default_reserve_meta();
            reserve_data_2.b_rate = 1_100_000_000_000;
            reserve_data_2.last_time = 12345;
            reserve_config_2.index = 1;
            testutils::create_reserve(
                &e,
                &pool_address,
                &underlying_2,
                &reserve_config_2,
                &reserve_data_2,
            );
            let pool_config = PoolConfig {
                oracle: Address::generate(&e),
                min_collateral: 1_0000000,
                bstop_rate: 0_1000000,
                status: 0,
                max_positions: 4,
            };
            let mut auction_data = AuctionData {
                bid: map![
                    &e,
                    (underlying_0.clone(), 10_0000000 - 2_5000000),
                    (underlying_1.clone(), 2_5000000 - 6250000)
                ],
                lot: map![&e, (lp_token.clone(), 47_6000000)],
                block: 51,
            };
            let positions: Positions = Positions {
                collateral: map![&e],
                liabilities: map![
                    &e,
                    (reserve_config_0.index, 10_0000000),
                    (reserve_config_1.index, 2_5000000)
                ],
                supply: map![&e],
            };

            e.as_contract(&pool_address, || {
                storage::set_auction(
                    &e,
                    &(AuctionType::BadDebtAuction as u32),
                    &backstop_address,
                    &auction_data,
                );
                storage::set_pool_config(&e, &pool_config);
                storage::set_user_positions(&e, &backstop_address, &positions);

                let pre_fill_d_supply_0 = reserve_data_0.d_supply;
                let pre_fill_d_supply_1 = reserve_data_1.d_supply;
                let pre_fill_b_rate_0 = reserve_data_0.b_rate;
                let pre_fill_b_rate_1 = reserve_data_1.b_rate;
                let mut pool = Pool::load(&e);
                let mut samwise_state = User::load(&e, &samwise);
                fill_bad_debt_auction(&e, &mut pool, &mut auction_data, &mut samwise_state, true);
                assert_eq!(
                    lp_token_client.balance(&backstop_address),
                    1_000_0000000 - 47_6000000
                );
                assert_eq!(
                    lp_token_client.balance(&samwise),
                    50_000_0000000 - 1_000_0000000 + 47_6000000
                );
                let samwise_positions = samwise_state.positions;
                assert_eq!(
                    samwise_positions
                        .liabilities
                        .get(reserve_config_0.index)
                        .unwrap(),
                    10_0000000 - 2_5000000
                );
                assert_eq!(
                    samwise_positions
                        .liabilities
                        .get(reserve_config_1.index)
                        .unwrap(),
                    2_5000000 - 0_6250000
                );
                let backstop_positions = storage::get_user_positions(&e, &backstop_address);
                assert_eq!(backstop_positions.liabilities.len(), 0);
                assert_eq!(backstop_positions.collateral.len(), 0);
                assert_eq!(backstop_positions.supply.len(), 0);

                // verify reserve data is updated and set to be stored
                pool.store_cached_reserves(&e);
                let reserve_data_0 = storage::get_res_data(&e, &underlying_0);
                assert_eq!(reserve_data_0.d_supply, pre_fill_d_supply_0 - 2_5000000);
                assert!(reserve_data_0.b_rate < pre_fill_b_rate_0);
                let reserve_data_1 = storage::get_res_data(&e, &underlying_1);
                assert_eq!(reserve_data_1.d_supply, pre_fill_d_supply_1 - 0_6250000);
                assert!(reserve_data_1.b_rate < pre_fill_b_rate_1);
            });
        }

        #[test]
        fn test_fill_bad_debt_auction_leftover_debt_small_backstop_does_not_burn_if_not_full_liq() {
            let e = Env::default();
            e.mock_all_auths_allowing_non_root_auth();
            e.cost_estimate().budget().reset_unlimited(); // setup exhausts budget

            e.ledger().set(LedgerInfo {
                timestamp: 12345,
                protocol_version: 22,
                sequence_number: 51,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 3110400,
            });

            let bombadil = Address::generate(&e);
            let samwise = Address::generate(&e);
            let pool_address = create_pool(&e);

            let (blnd, blnd_client) = testutils::create_blnd_token(&e, &pool_address, &bombadil);
            let (usdc, usdc_client) = testutils::create_token_contract(&e, &bombadil);
            let (lp_token, lp_token_client) =
                testutils::create_comet_lp_pool(&e, &bombadil, &blnd, &usdc);
            let (backstop_address, backstop_client) =
                testutils::create_backstop(&e, &pool_address, &lp_token, &usdc, &blnd);
            // mint lp tokens
            blnd_client.mint(&samwise, &500_001_0000000);
            blnd_client.approve(&samwise, &lp_token, &i128::MAX, &99999);
            usdc_client.mint(&samwise, &12_501_0000000);
            usdc_client.approve(&samwise, &lp_token, &i128::MAX, &99999);
            lp_token_client.join_pool(
                &50_000_0000000,
                &vec![&e, 500_001_0000000, 12_501_0000000],
                &samwise,
            );
            backstop_client.deposit(&samwise, &pool_address, &1_000_0000000);

            let (underlying_0, _) = testutils::create_token_contract(&e, &bombadil);
            let (mut reserve_config_0, mut reserve_data_0) = testutils::default_reserve_meta();
            reserve_data_0.d_rate = 1_100_000_000_000;
            reserve_data_0.last_time = 12345;
            reserve_config_0.index = 0;
            testutils::create_reserve(
                &e,
                &pool_address,
                &underlying_0,
                &reserve_config_0,
                &reserve_data_0,
            );

            let (underlying_1, _) = testutils::create_token_contract(&e, &bombadil);
            let (mut reserve_config_1, mut reserve_data_1) = testutils::default_reserve_meta();
            reserve_data_1.d_rate = 1_200_000_000_000;
            reserve_data_1.last_time = 12345;
            reserve_config_1.index = 1;
            testutils::create_reserve(
                &e,
                &pool_address,
                &underlying_1,
                &reserve_config_1,
                &reserve_data_1,
            );

            let (underlying_2, _) = testutils::create_token_contract(&e, &bombadil);
            let (mut reserve_config_2, mut reserve_data_2) = testutils::default_reserve_meta();
            reserve_data_2.b_rate = 1_100_000_000_000;
            reserve_data_2.last_time = 12345;
            reserve_config_2.index = 1;
            testutils::create_reserve(
                &e,
                &pool_address,
                &underlying_2,
                &reserve_config_2,
                &reserve_data_2,
            );
            let pool_config = PoolConfig {
                oracle: Address::generate(&e),
                min_collateral: 1_0000000,
                bstop_rate: 0_1000000,
                status: 0,
                max_positions: 4,
            };
            let mut auction_data = AuctionData {
                bid: map![
                    &e,
                    (underlying_0.clone(), 10_0000000 - 2_5000000),
                    (underlying_1.clone(), 2_5000000 - 6250000)
                ],
                lot: map![&e, (lp_token.clone(), 47_6000000)],
                block: 51,
            };
            let positions: Positions = Positions {
                collateral: map![&e],
                liabilities: map![
                    &e,
                    (reserve_config_0.index, 10_0000000),
                    (reserve_config_1.index, 2_5000000)
                ],
                supply: map![&e],
            };

            e.as_contract(&pool_address, || {
                storage::set_auction(
                    &e,
                    &(AuctionType::BadDebtAuction as u32),
                    &backstop_address,
                    &auction_data,
                );
                storage::set_pool_config(&e, &pool_config);
                storage::set_user_positions(&e, &backstop_address, &positions);

                let mut pool = Pool::load(&e);
                let mut samwise_state = User::load(&e, &samwise);
                fill_bad_debt_auction(&e, &mut pool, &mut auction_data, &mut samwise_state, false);
                assert_eq!(
                    lp_token_client.balance(&backstop_address),
                    1_000_0000000 - 47_6000000
                );
                assert_eq!(
                    lp_token_client.balance(&samwise),
                    50_000_0000000 - 1_000_0000000 + 47_6000000
                );
                let samwise_positions = samwise_state.positions;
                assert_eq!(
                    samwise_positions
                        .liabilities
                        .get(reserve_config_0.index)
                        .unwrap(),
                    10_0000000 - 2_5000000
                );
                assert_eq!(
                    samwise_positions
                        .liabilities
                        .get(reserve_config_1.index)
                        .unwrap(),
                    2_5000000 - 0_6250000
                );
                let backstop_positions = storage::get_user_positions(&e, &backstop_address);
                assert_eq!(backstop_positions.liabilities.len(), 2);
                assert_eq!(backstop_positions.collateral.len(), 0);
                assert_eq!(backstop_positions.supply.len(), 0);
                assert_eq!(
                    backstop_positions
                        .liabilities
                        .get(reserve_config_0.index)
                        .unwrap(),
                    10_0000000 - (10_0000000 - 2_5000000)
                );
                assert_eq!(
                    backstop_positions
                        .liabilities
                        .get(reserve_config_1.index)
                        .unwrap(),
                    2_5000000 - (2_5000000 - 0_6250000)
                );
            });
        }

        #[test]
        fn test_fill_bad_debt_auction_leftover_debt_sufficient_balance() {
            let e = Env::default();
            e.mock_all_auths_allowing_non_root_auth();
            e.cost_estimate().budget().reset_unlimited(); // setup exhausts budget

            e.ledger().set(LedgerInfo {
                timestamp: 12345,
                protocol_version: 22,
                sequence_number: 51,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 3110400,
            });

            let bombadil = Address::generate(&e);
            let samwise = Address::generate(&e);
            let pool_address = create_pool(&e);

            let (blnd, blnd_client) = testutils::create_blnd_token(&e, &pool_address, &bombadil);
            let (usdc, usdc_client) = testutils::create_token_contract(&e, &bombadil);
            let (lp_token, lp_token_client) =
                testutils::create_comet_lp_pool(&e, &bombadil, &blnd, &usdc);
            let (backstop_address, backstop_client) =
                testutils::create_backstop(&e, &pool_address, &lp_token, &usdc, &blnd);

            // mint lp tokens
            blnd_client.mint(&samwise, &500_001_0000000);
            blnd_client.approve(&samwise, &lp_token, &i128::MAX, &99999);
            usdc_client.mint(&samwise, &12_501_0000000);
            usdc_client.approve(&samwise, &lp_token, &i128::MAX, &99999);
            lp_token_client.join_pool(
                &50_000_0000000,
                &vec![&e, 500_001_0000000, 12_501_0000000],
                &samwise,
            );
            backstop_client.deposit(&samwise, &pool_address, &2_500_0000000);

            let (underlying_0, _) = testutils::create_token_contract(&e, &bombadil);
            let (mut reserve_config_0, mut reserve_data_0) = testutils::default_reserve_meta();
            reserve_data_0.d_rate = 1_100_000_000_000;
            reserve_data_0.last_time = 12345;
            reserve_config_0.index = 0;
            testutils::create_reserve(
                &e,
                &pool_address,
                &underlying_0,
                &reserve_config_0,
                &reserve_data_0,
            );

            let (underlying_1, _) = testutils::create_token_contract(&e, &bombadil);
            let (mut reserve_config_1, mut reserve_data_1) = testutils::default_reserve_meta();
            reserve_data_1.d_rate = 1_200_000_000_000;
            reserve_data_1.last_time = 12345;
            reserve_config_1.index = 1;
            testutils::create_reserve(
                &e,
                &pool_address,
                &underlying_1,
                &reserve_config_1,
                &reserve_data_1,
            );

            let (underlying_2, _) = testutils::create_token_contract(&e, &bombadil);
            let (mut reserve_config_2, mut reserve_data_2) = testutils::default_reserve_meta();
            reserve_data_2.b_rate = 1_100_000_000_000;
            reserve_data_2.last_time = 12345;
            reserve_config_2.index = 1;
            testutils::create_reserve(
                &e,
                &pool_address,
                &underlying_2,
                &reserve_config_2,
                &reserve_data_2,
            );
            let pool_config = PoolConfig {
                oracle: Address::generate(&e),
                min_collateral: 1_0000000,
                bstop_rate: 0_1000000,
                status: 0,
                max_positions: 4,
            };
            let mut auction_data = AuctionData {
                bid: map![
                    &e,
                    (underlying_0.clone(), 10_0000000 - 2_5000000),
                    (underlying_1.clone(), 2_5000000 - 6250000)
                ],
                lot: map![&e, (lp_token.clone(), 47_6000000)],
                block: 51,
            };
            let positions: Positions = Positions {
                collateral: map![&e],
                liabilities: map![
                    &e,
                    (reserve_config_0.index, 10_0000000),
                    (reserve_config_1.index, 2_5000000)
                ],
                supply: map![&e],
            };
            e.as_contract(&pool_address, || {
                storage::set_auction(
                    &e,
                    &(AuctionType::BadDebtAuction as u32),
                    &backstop_address,
                    &auction_data,
                );
                storage::set_pool_config(&e, &pool_config);
                storage::set_user_positions(&e, &backstop_address, &positions);

                let pre_fill_d_supply_0 = reserve_data_0.d_supply;
                let pre_fill_d_supply_1 = reserve_data_1.d_supply;
                let pre_fill_b_rate_0 = reserve_data_0.b_rate;
                let pre_fill_b_rate_1 = reserve_data_1.b_rate;
                let mut pool = Pool::load(&e);
                let mut samwise_state = User::load(&e, &samwise);
                fill_bad_debt_auction(&e, &mut pool, &mut auction_data, &mut samwise_state, true);
                assert_eq!(
                    lp_token_client.balance(&backstop_address),
                    2_500_0000000 - 47_6000000
                );
                assert_eq!(
                    lp_token_client.balance(&samwise),
                    50_000_0000000 - 2_500_0000000 + 47_6000000
                );
                let samwise_positions = samwise_state.positions;
                assert_eq!(
                    samwise_positions
                        .liabilities
                        .get(reserve_config_0.index)
                        .unwrap(),
                    10_0000000 - 2_5000000
                );
                assert_eq!(
                    samwise_positions
                        .liabilities
                        .get(reserve_config_1.index)
                        .unwrap(),
                    2_5000000 - 6250000
                );
                let backstop_positions = storage::get_user_positions(&e, &backstop_address);
                assert_eq!(
                    backstop_positions
                        .liabilities
                        .get(reserve_config_0.index)
                        .unwrap(),
                    2_5000000
                );
                assert_eq!(
                    backstop_positions
                        .liabilities
                        .get(reserve_config_1.index)
                        .unwrap(),
                    6250000
                );

                // verify reserve data is updated and set to be stored
                pool.store_cached_reserves(&e);
                let reserve_data_0 = storage::get_res_data(&e, &underlying_0);
                assert_eq!(reserve_data_0.d_supply, pre_fill_d_supply_0);
                assert_eq!(reserve_data_0.b_rate, pre_fill_b_rate_0);
                let reserve_data_1 = storage::get_res_data(&e, &underlying_1);
                assert_eq!(reserve_data_1.d_supply, pre_fill_d_supply_1);
                assert_eq!(reserve_data_1.b_rate, pre_fill_b_rate_1);
            });
        }

        #[test]
        fn test_fill_bad_debt_auction_empty_bid() {
            let e = Env::default();
            e.mock_all_auths_allowing_non_root_auth();
            e.cost_estimate().budget().reset_unlimited(); // setup exhausts budget

            e.ledger().set(LedgerInfo {
                timestamp: 12345,
                protocol_version: 22,
                sequence_number: 51,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 3110400,
            });

            let bombadil = Address::generate(&e);
            let samwise = Address::generate(&e);

            let pool_address = create_pool(&e);

            let (blnd, blnd_client) = testutils::create_blnd_token(&e, &pool_address, &bombadil);
            let (usdc, usdc_client) = testutils::create_token_contract(&e, &bombadil);
            let (lp_token, lp_token_client) =
                testutils::create_comet_lp_pool(&e, &bombadil, &blnd, &usdc);
            let (backstop_address, backstop_client) =
                testutils::create_backstop(&e, &pool_address, &lp_token, &usdc, &blnd);
            // mint lp tokens
            blnd_client.mint(&samwise, &500_001_0000000);
            blnd_client.approve(&samwise, &lp_token, &i128::MAX, &99999);
            usdc_client.mint(&samwise, &12_501_0000000);
            usdc_client.approve(&samwise, &lp_token, &i128::MAX, &99999);
            lp_token_client.join_pool(
                &50_000_0000000,
                &vec![&e, 500_001_0000000, 12_501_0000000],
                &samwise,
            );
            backstop_client.deposit(&samwise, &pool_address, &50_000_0000000);

            let (underlying_0, _) = testutils::create_token_contract(&e, &bombadil);
            let (mut reserve_config_0, mut reserve_data_0) = testutils::default_reserve_meta();
            reserve_data_0.d_rate = 1_100_000_000_000;
            reserve_data_0.last_time = 12345;
            reserve_config_0.index = 0;
            testutils::create_reserve(
                &e,
                &pool_address,
                &underlying_0,
                &reserve_config_0,
                &reserve_data_0,
            );

            let (underlying_1, _) = testutils::create_token_contract(&e, &bombadil);
            let (mut reserve_config_1, mut reserve_data_1) = testutils::default_reserve_meta();
            reserve_data_1.d_rate = 1_200_000_000_000;
            reserve_data_1.last_time = 12345;
            reserve_config_1.index = 1;
            testutils::create_reserve(
                &e,
                &pool_address,
                &underlying_1,
                &reserve_config_1,
                &reserve_data_1,
            );

            let (underlying_2, _) = testutils::create_token_contract(&e, &bombadil);
            let (mut reserve_config_2, mut reserve_data_2) = testutils::default_reserve_meta();
            reserve_data_2.b_rate = 1_100_000_000_000;
            reserve_data_2.last_time = 12345;
            reserve_config_2.index = 1;
            testutils::create_reserve(
                &e,
                &pool_address,
                &underlying_2,
                &reserve_config_2,
                &reserve_data_2,
            );
            let pool_config = PoolConfig {
                oracle: Address::generate(&e),
                min_collateral: 1_0000000,
                bstop_rate: 0_1000000,
                status: 0,
                max_positions: 4,
            };
            let mut auction_data = AuctionData {
                bid: map![&e],
                lot: map![&e, (lp_token.clone(), 47_6000000)],
                block: 51,
            };
            let positions: Positions = Positions {
                collateral: map![&e],
                liabilities: map![
                    &e,
                    (reserve_config_0.index, 10_0000000),
                    (reserve_config_1.index, 2_5000000)
                ],
                supply: map![&e],
            };

            e.as_contract(&pool_address, || {
                storage::set_auction(
                    &e,
                    &(AuctionType::BadDebtAuction as u32),
                    &backstop_address,
                    &auction_data,
                );
                storage::set_pool_config(&e, &pool_config);
                storage::set_user_positions(&e, &backstop_address, &positions);

                let mut pool = Pool::load(&e);
                let mut samwise_state = User::load(&e, &samwise);
                fill_bad_debt_auction(&e, &mut pool, &mut auction_data, &mut samwise_state, true);
                assert_eq!(
                    lp_token_client.balance(&backstop_address),
                    50_000_0000000 - 47_6000000
                );
                assert_eq!(lp_token_client.balance(&samwise), 47_6000000);
                let samwise_positions = samwise_state.positions;
                assert_eq!(samwise_positions.liabilities.len(), 0);
                let backstop_positions = storage::get_user_positions(&e, &backstop_address);
                assert_eq!(
                    backstop_positions
                        .liabilities
                        .get(reserve_config_0.index)
                        .unwrap(),
                    10_0000000
                );
                assert_eq!(
                    backstop_positions
                        .liabilities
                        .get(reserve_config_1.index)
                        .unwrap(),
                    2_5000000
                );
            });
        }

        #[test]
        #[should_panic(expected = "Error(Contract, #1200)")]
        fn test_fill_bad_debt_auction_with_backstop() {
            let e = Env::default();
            e.mock_all_auths_allowing_non_root_auth();
            e.cost_estimate().budget().reset_unlimited(); // setup exhausts budget

            e.ledger().set(LedgerInfo {
                timestamp: 12345,
                protocol_version: 22,
                sequence_number: 51,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 3110400,
            });

            let bombadil = Address::generate(&e);
            let samwise = Address::generate(&e);

            let pool_address = create_pool(&e);

            let (blnd, blnd_client) = testutils::create_blnd_token(&e, &pool_address, &bombadil);
            let (usdc, usdc_client) = testutils::create_token_contract(&e, &bombadil);
            let (lp_token, lp_token_client) =
                testutils::create_comet_lp_pool(&e, &bombadil, &blnd, &usdc);
            let (backstop_address, backstop_client) =
                testutils::create_backstop(&e, &pool_address, &lp_token, &usdc, &blnd);
            // mint lp tokens
            blnd_client.mint(&samwise, &500_001_0000000);
            blnd_client.approve(&samwise, &lp_token, &i128::MAX, &99999);
            usdc_client.mint(&samwise, &12_501_0000000);
            usdc_client.approve(&samwise, &lp_token, &i128::MAX, &99999);
            lp_token_client.join_pool(
                &50_000_0000000,
                &vec![&e, 500_001_0000000, 12_501_0000000],
                &samwise,
            );
            backstop_client.deposit(&samwise, &pool_address, &50_000_0000000);

            let (underlying_0, _) = testutils::create_token_contract(&e, &bombadil);
            let (mut reserve_config_0, mut reserve_data_0) = testutils::default_reserve_meta();
            reserve_data_0.d_rate = 1_100_000_000_000;
            reserve_data_0.last_time = 12345;
            reserve_config_0.index = 0;
            testutils::create_reserve(
                &e,
                &pool_address,
                &underlying_0,
                &reserve_config_0,
                &reserve_data_0,
            );

            let (underlying_1, _) = testutils::create_token_contract(&e, &bombadil);
            let (mut reserve_config_1, mut reserve_data_1) = testutils::default_reserve_meta();
            reserve_data_1.d_rate = 1_200_000_000_000;
            reserve_data_1.last_time = 12345;
            reserve_config_1.index = 1;
            testutils::create_reserve(
                &e,
                &pool_address,
                &underlying_1,
                &reserve_config_1,
                &reserve_data_1,
            );

            let (underlying_2, _) = testutils::create_token_contract(&e, &bombadil);
            let (mut reserve_config_2, mut reserve_data_2) = testutils::default_reserve_meta();
            reserve_data_2.b_rate = 1_100_000_000_000;
            reserve_data_2.last_time = 12345;
            reserve_config_2.index = 1;
            testutils::create_reserve(
                &e,
                &pool_address,
                &underlying_2,
                &reserve_config_2,
                &reserve_data_2,
            );
            let pool_config = PoolConfig {
                oracle: Address::generate(&e),
                min_collateral: 1_0000000,
                bstop_rate: 0_1000000,
                status: 0,
                max_positions: 4,
            };
            let mut auction_data = AuctionData {
                bid: map![&e, (underlying_0, 10_0000000), (underlying_1, 2_5000000)],
                lot: map![&e, (lp_token.clone(), 47_6000000)],
                block: 51,
            };
            let positions: Positions = Positions {
                collateral: map![&e],
                liabilities: map![
                    &e,
                    (reserve_config_0.index, 10_0000000),
                    (reserve_config_1.index, 2_5000000)
                ],
                supply: map![&e],
            };

            e.as_contract(&pool_address, || {
                storage::set_auction(
                    &e,
                    &(AuctionType::BadDebtAuction as u32),
                    &backstop_address,
                    &auction_data,
                );
                storage::set_pool_config(&e, &pool_config);
                storage::set_user_positions(&e, &backstop_address, &positions);

                let mut pool = Pool::load(&e);
                let mut backstop_state = User::load(&e, &backstop_address);
                fill_bad_debt_auction(&e, &mut pool, &mut auction_data, &mut backstop_state, true);
            });
        }
    }
}

mod pool_src_auctions_auction {
    use crate::{
        constants::SCALAR_7,
        errors::PoolError,
        pool::{Pool, User},
        storage,
    };

    use cast::i128;

    use soroban_fixed_point_math::SorobanFixedPoint;

    use soroban_sdk::{contracttype, map, panic_with_error, Address, Env, Map, Vec};

    use crate::auctions::{
        backstop_interest_auction::{create_interest_auction_data, fill_interest_auction},
        bad_debt_auction::{create_bad_debt_auction_data, fill_bad_debt_auction},
        user_liquidation_auction::{create_user_liq_auction_data, fill_user_liq_auction},
    };

    pub(crate) use crate::auctions::auction::*;

    mod tests {
        use crate::{
            pool::Positions,
            storage::PoolConfig,
            testutils::{self, create_comet_lp_pool, create_pool},
        };

        use super::*;
        use sep_40_oracle::testutils::Asset;
        use soroban_sdk::{
            map,
            testutils::{Address as _, Ledger, LedgerInfo},
            unwrap::UnwrapOptimized,
            vec, Symbol,
        };

        #[test]
        fn test_create_bad_debt_auction() {
            let e = Env::default();
            e.mock_all_auths_allowing_non_root_auth();
            e.cost_estimate().budget().reset_unlimited(); // setup exhausts budget

            e.ledger().set(LedgerInfo {
                timestamp: 12345,
                protocol_version: 22,
                sequence_number: 50,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 3110400,
            });

            let bombadil = Address::generate(&e);
            let samwise = Address::generate(&e);
            let pool_address = create_pool(&e);

            let (blnd, blnd_client) = testutils::create_blnd_token(&e, &pool_address, &bombadil);
            let (usdc, usdc_client) = testutils::create_token_contract(&e, &bombadil);
            let (lp_token, lp_token_client) =
                testutils::create_comet_lp_pool(&e, &bombadil, &blnd, &usdc);
            let (backstop_address, backstop_client) =
                testutils::create_backstop(&e, &pool_address, &lp_token, &usdc, &blnd);
            // mint lp tokens
            blnd_client.mint(&samwise, &500_001_0000000);
            blnd_client.approve(&samwise, &lp_token, &i128::MAX, &99999);
            usdc_client.mint(&samwise, &12_501_0000000);
            usdc_client.approve(&samwise, &lp_token, &i128::MAX, &99999);
            lp_token_client.join_pool(
                &50_000_0000000,
                &vec![&e, 500_001_0000000, 12_501_0000000],
                &samwise,
            );
            backstop_client.deposit(&samwise, &pool_address, &50_000_0000000);

            let (oracle_id, oracle_client) = testutils::create_mock_oracle(&e);

            let (underlying_0, _) = testutils::create_token_contract(&e, &bombadil);
            let (mut reserve_config_0, mut reserve_data_0) = testutils::default_reserve_meta();
            reserve_data_0.d_rate = 1_100_000_000_000;
            reserve_data_0.last_time = 12345;
            reserve_config_0.index = 0;
            testutils::create_reserve(
                &e,
                &pool_address,
                &underlying_0,
                &reserve_config_0,
                &reserve_data_0,
            );

            let (underlying_1, _) = testutils::create_token_contract(&e, &bombadil);
            let (mut reserve_config_1, mut reserve_data_1) = testutils::default_reserve_meta();
            reserve_data_1.d_rate = 1_200_000_000_000;
            reserve_data_1.last_time = 12345;
            reserve_config_1.index = 1;
            testutils::create_reserve(
                &e,
                &pool_address,
                &underlying_1,
                &reserve_config_1,
                &reserve_data_1,
            );

            let (underlying_2, _) = testutils::create_token_contract(&e, &bombadil);
            let (mut reserve_config_2, mut reserve_data_2) = testutils::default_reserve_meta();
            reserve_data_2.b_rate = 1_100_000_000_000;
            reserve_data_2.last_time = 12345;
            reserve_config_2.index = 1;
            testutils::create_reserve(
                &e,
                &pool_address,
                &underlying_2,
                &reserve_config_2,
                &reserve_data_2,
            );
            oracle_client.set_data(
                &bombadil,
                &Asset::Other(Symbol::new(&e, "USD1")),
                &vec![
                    &e,
                    Asset::Stellar(underlying_0.clone()),
                    Asset::Stellar(underlying_1.clone()),
                    Asset::Stellar(underlying_2.clone()),
                    Asset::Stellar(usdc),
                    Asset::Stellar(blnd),
                ],
                &7,
                &300,
            );
            oracle_client.set_price_stable(&vec![
                &e,
                2_0000000,
                4_0000000,
                100_0000000,
                1_0000000,
                0_1000000,
            ]);

            let positions: Positions = Positions {
                collateral: map![&e],
                liabilities: map![
                    &e,
                    (reserve_config_0.index, 10_0000000),
                    (reserve_config_1.index, 2_5000000)
                ],
                supply: map![&e],
            };

            let pool_config = PoolConfig {
                oracle: oracle_id,
                min_collateral: 1_0000000,
                bstop_rate: 0_1000000,
                status: 0,
                max_positions: 4,
            };
            e.as_contract(&pool_address, || {
                storage::set_pool_config(&e, &pool_config);
                storage::set_user_positions(&e, &backstop_address, &positions);

                create_auction(
                    &e,
                    1,
                    &backstop_address,
                    &vec![&e, underlying_0, underlying_1],
                    &vec![&e, lp_token],
                    100,
                );
                assert!(storage::has_auction(&e, &1, &backstop_address));
            });
        }

        #[test]
        fn test_create_interest_auction() {
            let e = Env::default();
            e.mock_all_auths();
            e.cost_estimate().budget().reset_unlimited(); // setup exhausts budget

            e.ledger().set(LedgerInfo {
                timestamp: 12345,
                protocol_version: 22,
                sequence_number: 50,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 3110400,
            });

            let bombadil = Address::generate(&e);

            let pool_address = create_pool(&e);
            let (usdc_id, _) = testutils::create_token_contract(&e, &bombadil);
            let (blnd_id, _) = testutils::create_blnd_token(&e, &pool_address, &bombadil);

            let (backstop_token_id, _) = create_comet_lp_pool(&e, &bombadil, &blnd_id, &usdc_id);
            let (backstop_address, backstop_client) = testutils::create_backstop(
                &e,
                &pool_address,
                &backstop_token_id,
                &usdc_id,
                &blnd_id,
            );
            backstop_client.deposit(&bombadil, &pool_address, &(50 * SCALAR_7));
            let (oracle_id, oracle_client) = testutils::create_mock_oracle(&e);

            let (underlying_0, _) = testutils::create_token_contract(&e, &bombadil);
            let (mut reserve_config_0, mut reserve_data_0) = testutils::default_reserve_meta();
            reserve_data_0.last_time = 12345;
            reserve_data_0.backstop_credit = 100_0000000;
            reserve_data_0.b_supply = 1000_0000000;
            reserve_data_0.d_supply = 750_0000000;
            reserve_config_0.index = 0;
            testutils::create_reserve(
                &e,
                &pool_address,
                &underlying_0,
                &reserve_config_0,
                &reserve_data_0,
            );

            let (underlying_1, _) = testutils::create_token_contract(&e, &bombadil);
            let (mut reserve_config_1, mut reserve_data_1) = testutils::default_reserve_meta();
            reserve_data_1.last_time = 12345;
            reserve_data_1.backstop_credit = 25_0000000;
            reserve_data_1.b_supply = 250_0000000;
            reserve_data_1.d_supply = 187_5000000;
            reserve_config_1.index = 1;
            testutils::create_reserve(
                &e,
                &pool_address,
                &underlying_1,
                &reserve_config_1,
                &reserve_data_1,
            );

            let (underlying_2, _) = testutils::create_token_contract(&e, &bombadil);
            let (mut reserve_config_2, mut reserve_data_2) = testutils::default_reserve_meta();
            reserve_data_2.b_rate = 1_100_000_000_000;
            reserve_data_2.last_time = 12345;
            reserve_config_2.index = 1;
            testutils::create_reserve(
                &e,
                &pool_address,
                &underlying_2,
                &reserve_config_2,
                &reserve_data_2,
            );

            oracle_client.set_data(
                &bombadil,
                &Asset::Other(Symbol::new(&e, "USD")),
                &vec![
                    &e,
                    Asset::Stellar(underlying_0.clone()),
                    Asset::Stellar(underlying_1.clone()),
                    Asset::Stellar(underlying_2),
                    Asset::Stellar(usdc_id),
                ],
                &7,
                &300,
            );
            oracle_client.set_price_stable(&vec![&e, 2_0000000, 4_0000000, 100_0000000, 1_0000000]);

            let pool_config = PoolConfig {
                oracle: oracle_id,
                min_collateral: 1_0000000,
                bstop_rate: 0_1000000,
                status: 0,
                max_positions: 4,
            };
            e.as_contract(&pool_address, || {
                storage::set_pool_config(&e, &pool_config);

                create_auction(
                    &e,
                    2,
                    &backstop_address,
                    &vec![&e, backstop_token_id],
                    &vec![&e, underlying_0, underlying_1],
                    100,
                );
                assert!(storage::has_auction(&e, &2, &backstop_address));
            });
        }

        #[test]
        fn test_create_liquidation() {
            let e = Env::default();

            e.mock_all_auths();
            e.ledger().set(LedgerInfo {
                timestamp: 12345,
                protocol_version: 22,
                sequence_number: 50,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 3110400,
            });

            let bombadil = Address::generate(&e);
            let samwise = Address::generate(&e);

            let pool_address = create_pool(&e);
            let (oracle_address, oracle_client) = testutils::create_mock_oracle(&e);

            // creating reserves for a pool exhausts the budget
            e.cost_estimate().budget().reset_unlimited();
            let (underlying_0, _) = testutils::create_token_contract(&e, &bombadil);
            let (mut reserve_config_0, mut reserve_data_0) = testutils::default_reserve_meta();
            reserve_data_0.last_time = 12345;
            reserve_data_0.b_rate = 1_100_000_000_000;
            reserve_config_0.c_factor = 0_8500000;
            reserve_config_0.l_factor = 0_9000000;
            reserve_config_0.index = 0;
            testutils::create_reserve(
                &e,
                &pool_address,
                &underlying_0,
                &reserve_config_0,
                &reserve_data_0,
            );

            let (underlying_1, _) = testutils::create_token_contract(&e, &bombadil);
            let (mut reserve_config_1, mut reserve_data_1) = testutils::default_reserve_meta();
            reserve_data_1.b_rate = 1_200_000_000_000;
            reserve_config_1.c_factor = 0_7500000;
            reserve_config_1.l_factor = 0_7500000;
            reserve_data_1.last_time = 12345;
            reserve_config_1.index = 1;
            testutils::create_reserve(
                &e,
                &pool_address,
                &underlying_1,
                &reserve_config_1,
                &reserve_data_1,
            );

            let (underlying_2, _) = testutils::create_token_contract(&e, &bombadil);
            let (mut reserve_config_2, reserve_data_2) = testutils::default_reserve_meta();
            reserve_config_2.c_factor = 0_0000000;
            reserve_config_2.l_factor = 0_7000000;
            reserve_config_2.index = 2;
            testutils::create_reserve(
                &e,
                &pool_address,
                &underlying_2,
                &reserve_config_2,
                &reserve_data_2,
            );

            oracle_client.set_data(
                &bombadil,
                &Asset::Other(Symbol::new(&e, "USD")),
                &vec![
                    &e,
                    Asset::Stellar(underlying_0.clone()),
                    Asset::Stellar(underlying_1.clone()),
                    Asset::Stellar(underlying_2.clone()),
                ],
                &7,
                &300,
            );
            oracle_client.set_price_stable(&vec![&e, 2_0000000, 4_0000000, 50_0000000]);

            let liq_pct = 45;
            let positions: Positions = Positions {
                collateral: map![
                    &e,
                    (reserve_config_0.index, 90_9100000),
                    (reserve_config_1.index, 04_5800000),
                ],
                liabilities: map![&e, (reserve_config_2.index, 02_7500000),],
                supply: map![&e],
            };
            let pool_config = PoolConfig {
                oracle: oracle_address,
                min_collateral: 1_0000000,
                bstop_rate: 0_1000000,
                status: 0,
                max_positions: 4,
            };
            e.as_contract(&pool_address, || {
                storage::set_backstop(&e, &Address::generate(&e));
                storage::set_user_positions(&e, &samwise, &positions);
                storage::set_pool_config(&e, &pool_config);

                e.cost_estimate().budget().reset_unlimited();
                create_auction(
                    &e,
                    0,
                    &samwise,
                    &vec![&e, underlying_2],
                    &vec![&e, underlying_0, underlying_1],
                    liq_pct,
                );
                assert!(storage::has_auction(&e, &0, &samwise));
            });
        }

        #[test]
        #[should_panic(expected = "Error(Contract, #1211)")]
        fn test_create_liquidation_for_pool() {
            let e = Env::default();
            e.cost_estimate().budget().reset_unlimited();
            e.mock_all_auths();
            e.ledger().set(LedgerInfo {
                timestamp: 12345,
                protocol_version: 22,
                sequence_number: 50,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 3110400,
            });

            let bombadil = Address::generate(&e);

            let pool_address = create_pool(&e);
            let (oracle_address, oracle_client) = testutils::create_mock_oracle(&e);

            let (underlying_0, _) = testutils::create_token_contract(&e, &bombadil);
            let (mut reserve_config_0, mut reserve_data_0) = testutils::default_reserve_meta();
            reserve_data_0.last_time = 12345;
            reserve_data_0.b_rate = 1_100_000_000_000;
            reserve_config_0.c_factor = 0_8500000;
            reserve_config_0.l_factor = 0_9000000;
            reserve_config_0.index = 0;
            testutils::create_reserve(
                &e,
                &pool_address,
                &underlying_0,
                &reserve_config_0,
                &reserve_data_0,
            );

            let (underlying_1, _) = testutils::create_token_contract(&e, &bombadil);
            let (mut reserve_config_1, mut reserve_data_1) = testutils::default_reserve_meta();
            reserve_data_1.b_rate = 1_200_000_000_000;
            reserve_config_1.c_factor = 0_7500000;
            reserve_config_1.l_factor = 0_7500000;
            reserve_data_1.last_time = 12345;
            reserve_config_1.index = 1;
            testutils::create_reserve(
                &e,
                &pool_address,
                &underlying_1,
                &reserve_config_1,
                &reserve_data_1,
            );

            let (underlying_2, _) = testutils::create_token_contract(&e, &bombadil);
            let (mut reserve_config_2, reserve_data_2) = testutils::default_reserve_meta();
            reserve_config_2.c_factor = 0_0000000;
            reserve_config_2.l_factor = 0_7000000;
            reserve_config_2.index = 2;
            testutils::create_reserve(
                &e,
                &pool_address,
                &underlying_2,
                &reserve_config_2,
                &reserve_data_2,
            );

            oracle_client.set_data(
                &bombadil,
                &Asset::Other(Symbol::new(&e, "USD")),
                &vec![
                    &e,
                    Asset::Stellar(underlying_0.clone()),
                    Asset::Stellar(underlying_1.clone()),
                    Asset::Stellar(underlying_2.clone()),
                ],
                &7,
                &300,
            );
            oracle_client.set_price_stable(&vec![&e, 2_0000000, 4_0000000, 50_0000000]);

            let liq_pct = 45;
            let positions: Positions = Positions {
                collateral: map![
                    &e,
                    (reserve_config_0.index, 90_9100000),
                    (reserve_config_1.index, 04_5800000),
                ],
                liabilities: map![&e, (reserve_config_2.index, 02_7500000),],
                supply: map![&e],
            };
            let pool_config = PoolConfig {
                oracle: oracle_address,
                min_collateral: 1_0000000,
                bstop_rate: 0_1000000,
                status: 0,
                max_positions: 4,
            };
            e.as_contract(&pool_address, || {
                storage::set_backstop(&e, &Address::generate(&e));
                storage::set_user_positions(&e, &pool_address, &positions);
                storage::set_pool_config(&e, &pool_config);

                create_auction(
                    &e,
                    0,
                    &pool_address,
                    &vec![&e, underlying_2],
                    &vec![&e, underlying_0, underlying_1],
                    liq_pct,
                );
            });
        }

        #[test]
        #[should_panic(expected = "Error(Contract, #1211)")]
        fn test_create_liquidation_for_backstop() {
            let e = Env::default();
            e.cost_estimate().budget().reset_unlimited();
            e.mock_all_auths();
            e.ledger().set(LedgerInfo {
                timestamp: 12345,
                protocol_version: 22,
                sequence_number: 50,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 3110400,
            });

            let bombadil = Address::generate(&e);

            let pool_address = create_pool(&e);
            let backstop = Address::generate(&e);
            let (oracle_address, oracle_client) = testutils::create_mock_oracle(&e);

            let (underlying_0, _) = testutils::create_token_contract(&e, &bombadil);
            let (mut reserve_config_0, mut reserve_data_0) = testutils::default_reserve_meta();
            reserve_data_0.last_time = 12345;
            reserve_data_0.b_rate = 1_100_000_000_000;
            reserve_config_0.c_factor = 0_8500000;
            reserve_config_0.l_factor = 0_9000000;
            reserve_config_0.index = 0;
            testutils::create_reserve(
                &e,
                &pool_address,
                &underlying_0,
                &reserve_config_0,
                &reserve_data_0,
            );

            let (underlying_1, _) = testutils::create_token_contract(&e, &bombadil);
            let (mut reserve_config_1, mut reserve_data_1) = testutils::default_reserve_meta();
            reserve_data_1.b_rate = 1_200_000_000_000;
            reserve_config_1.c_factor = 0_7500000;
            reserve_config_1.l_factor = 0_7500000;
            reserve_data_1.last_time = 12345;
            reserve_config_1.index = 1;
            testutils::create_reserve(
                &e,
                &pool_address,
                &underlying_1,
                &reserve_config_1,
                &reserve_data_1,
            );

            let (underlying_2, _) = testutils::create_token_contract(&e, &bombadil);
            let (mut reserve_config_2, reserve_data_2) = testutils::default_reserve_meta();
            reserve_config_2.c_factor = 0_0000000;
            reserve_config_2.l_factor = 0_7000000;
            reserve_config_2.index = 2;
            testutils::create_reserve(
                &e,
                &pool_address,
                &underlying_2,
                &reserve_config_2,
                &reserve_data_2,
            );

            oracle_client.set_data(
                &bombadil,
                &Asset::Other(Symbol::new(&e, "USD")),
                &vec![
                    &e,
                    Asset::Stellar(underlying_0.clone()),
                    Asset::Stellar(underlying_1.clone()),
                    Asset::Stellar(underlying_2.clone()),
                ],
                &7,
                &300,
            );
            oracle_client.set_price_stable(&vec![&e, 2_0000000, 4_0000000, 50_0000000]);

            let liq_pct = 45;
            let positions: Positions = Positions {
                collateral: map![
                    &e,
                    (reserve_config_0.index, 90_9100000),
                    (reserve_config_1.index, 04_5800000),
                ],
                liabilities: map![&e, (reserve_config_2.index, 02_7500000),],
                supply: map![&e],
            };
            let pool_config = PoolConfig {
                oracle: oracle_address,
                min_collateral: 1_0000000,
                bstop_rate: 0_1000000,
                status: 0,
                max_positions: 4,
            };
            e.as_contract(&pool_address, || {
                storage::set_backstop(&e, &backstop);
                storage::set_user_positions(&e, &backstop, &positions);
                storage::set_pool_config(&e, &pool_config);

                create_auction(
                    &e,
                    0,
                    &backstop,
                    &vec![&e, underlying_2],
                    &vec![&e, underlying_0, underlying_1],
                    liq_pct,
                );
            });
        }

        #[test]
        #[should_panic(expected = "Error(Contract, #1200)")]
        fn test_create_auction_invalid_type() {
            let e = Env::default();
            e.mock_all_auths();
            e.cost_estimate().budget().reset_unlimited(); // setup exhausts budget

            e.ledger().set(LedgerInfo {
                timestamp: 12345,
                protocol_version: 22,
                sequence_number: 50,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 3110400,
            });

            let bombadil = Address::generate(&e);

            let pool_address = create_pool(&e);
            let (usdc_id, _) = testutils::create_token_contract(&e, &bombadil);
            let (blnd_id, _) = testutils::create_blnd_token(&e, &pool_address, &bombadil);

            let (backstop_token_id, _) = create_comet_lp_pool(&e, &bombadil, &blnd_id, &usdc_id);
            let (backstop_address, backstop_client) = testutils::create_backstop(
                &e,
                &pool_address,
                &backstop_token_id,
                &usdc_id,
                &blnd_id,
            );
            backstop_client.deposit(&bombadil, &pool_address, &(50 * SCALAR_7));
            let (oracle_id, oracle_client) = testutils::create_mock_oracle(&e);

            let (underlying_0, _) = testutils::create_token_contract(&e, &bombadil);
            let (mut reserve_config_0, mut reserve_data_0) = testutils::default_reserve_meta();
            reserve_data_0.last_time = 12345;
            reserve_data_0.backstop_credit = 200_0000000;
            reserve_data_0.b_supply = 1000_0000000;
            reserve_data_0.d_supply = 750_0000000;
            reserve_config_0.index = 0;
            testutils::create_reserve(
                &e,
                &pool_address,
                &underlying_0,
                &reserve_config_0,
                &reserve_data_0,
            );

            oracle_client.set_data(
                &bombadil,
                &Asset::Other(Symbol::new(&e, "USD")),
                &vec![
                    &e,
                    Asset::Stellar(underlying_0.clone()),
                    Asset::Stellar(usdc_id),
                ],
                &7,
                &300,
            );
            oracle_client.set_price_stable(&vec![&e, 2_0000000, 1_0000000]);

            let pool_config = PoolConfig {
                oracle: oracle_id,
                min_collateral: 1_0000000,
                bstop_rate: 0_1000000,
                status: 0,
                max_positions: 4,
            };
            e.as_contract(&pool_address, || {
                storage::set_pool_config(&e, &pool_config);

                create_auction(
                    &e,
                    3,
                    &backstop_address,
                    &vec![&e, backstop_token_id],
                    &vec![&e, underlying_0],
                    100,
                );
            });
        }

        #[test]
        #[should_panic(expected = "Error(Contract, #1200)")]
        fn test_create_auction_duplicate_bid() {
            let e = Env::default();
            e.mock_all_auths_allowing_non_root_auth();
            e.cost_estimate().budget().reset_unlimited(); // setup exhausts budget

            e.ledger().set(LedgerInfo {
                timestamp: 12345,
                protocol_version: 22,
                sequence_number: 50,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 3110400,
            });

            let bombadil = Address::generate(&e);
            let samwise = Address::generate(&e);
            let pool_address = create_pool(&e);

            let (blnd, blnd_client) = testutils::create_blnd_token(&e, &pool_address, &bombadil);
            let (usdc, usdc_client) = testutils::create_token_contract(&e, &bombadil);
            let (lp_token, lp_token_client) =
                testutils::create_comet_lp_pool(&e, &bombadil, &blnd, &usdc);
            let (backstop_address, backstop_client) =
                testutils::create_backstop(&e, &pool_address, &lp_token, &usdc, &blnd);
            // mint lp tokens
            blnd_client.mint(&samwise, &500_001_0000000);
            blnd_client.approve(&samwise, &lp_token, &i128::MAX, &99999);
            usdc_client.mint(&samwise, &12_501_0000000);
            usdc_client.approve(&samwise, &lp_token, &i128::MAX, &99999);
            lp_token_client.join_pool(
                &50_000_0000000,
                &vec![&e, 500_001_0000000, 12_501_0000000],
                &samwise,
            );
            backstop_client.deposit(&samwise, &pool_address, &50_000_0000000);

            let (oracle_id, oracle_client) = testutils::create_mock_oracle(&e);

            let (underlying_0, _) = testutils::create_token_contract(&e, &bombadil);
            let (mut reserve_config_0, mut reserve_data_0) = testutils::default_reserve_meta();
            reserve_data_0.d_rate = 1_100_000_000_000;
            reserve_data_0.last_time = 12345;
            reserve_config_0.index = 0;
            testutils::create_reserve(
                &e,
                &pool_address,
                &underlying_0,
                &reserve_config_0,
                &reserve_data_0,
            );

            let (underlying_1, _) = testutils::create_token_contract(&e, &bombadil);
            let (mut reserve_config_1, mut reserve_data_1) = testutils::default_reserve_meta();
            reserve_data_1.d_rate = 1_200_000_000_000;
            reserve_data_1.last_time = 12345;
            reserve_config_1.index = 1;
            testutils::create_reserve(
                &e,
                &pool_address,
                &underlying_1,
                &reserve_config_1,
                &reserve_data_1,
            );

            let (underlying_2, _) = testutils::create_token_contract(&e, &bombadil);
            let (mut reserve_config_2, mut reserve_data_2) = testutils::default_reserve_meta();
            reserve_data_2.b_rate = 1_100_000_000_000;
            reserve_data_2.last_time = 12345;
            reserve_config_2.index = 1;
            testutils::create_reserve(
                &e,
                &pool_address,
                &underlying_2,
                &reserve_config_2,
                &reserve_data_2,
            );
            oracle_client.set_data(
                &bombadil,
                &Asset::Other(Symbol::new(&e, "USD1")),
                &vec![
                    &e,
                    Asset::Stellar(underlying_0.clone()),
                    Asset::Stellar(underlying_1.clone()),
                    Asset::Stellar(underlying_2.clone()),
                    Asset::Stellar(usdc),
                    Asset::Stellar(blnd),
                ],
                &7,
                &300,
            );
            oracle_client.set_price_stable(&vec![
                &e,
                2_0000000,
                4_0000000,
                100_0000000,
                1_0000000,
                0_1000000,
            ]);

            let positions: Positions = Positions {
                collateral: map![&e],
                liabilities: map![
                    &e,
                    (reserve_config_0.index, 10_0000000),
                    (reserve_config_1.index, 2_5000000)
                ],
                supply: map![&e],
            };

            let pool_config = PoolConfig {
                oracle: oracle_id,
                min_collateral: 1_0000000,
                bstop_rate: 0_1000000,
                status: 0,
                max_positions: 4,
            };
            e.as_contract(&pool_address, || {
                storage::set_pool_config(&e, &pool_config);
                storage::set_user_positions(&e, &backstop_address, &positions);

                create_auction(
                    &e,
                    1,
                    &backstop_address,
                    &vec![&e, underlying_0.clone(), underlying_1, underlying_0],
                    &vec![&e, lp_token],
                    100,
                );
            });
        }

        #[test]
        #[should_panic(expected = "Error(Contract, #1200)")]
        fn test_create_auction_duplicate_lot() {
            let e = Env::default();
            e.mock_all_auths();
            e.cost_estimate().budget().reset_unlimited(); // setup exhausts budget

            e.ledger().set(LedgerInfo {
                timestamp: 12345,
                protocol_version: 22,
                sequence_number: 50,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 3110400,
            });

            let bombadil = Address::generate(&e);

            let pool_address = create_pool(&e);
            let (usdc_id, _) = testutils::create_token_contract(&e, &bombadil);
            let (blnd_id, _) = testutils::create_blnd_token(&e, &pool_address, &bombadil);

            let (backstop_token_id, _) = create_comet_lp_pool(&e, &bombadil, &blnd_id, &usdc_id);
            let (backstop_address, backstop_client) = testutils::create_backstop(
                &e,
                &pool_address,
                &backstop_token_id,
                &usdc_id,
                &blnd_id,
            );
            backstop_client.deposit(&bombadil, &pool_address, &(50 * SCALAR_7));
            let (oracle_id, oracle_client) = testutils::create_mock_oracle(&e);

            let (underlying_0, _) = testutils::create_token_contract(&e, &bombadil);
            let (mut reserve_config_0, mut reserve_data_0) = testutils::default_reserve_meta();
            reserve_data_0.last_time = 12345;
            reserve_data_0.backstop_credit = 100_0000000;
            reserve_data_0.b_supply = 1000_0000000;
            reserve_data_0.d_supply = 750_0000000;
            reserve_config_0.index = 0;
            testutils::create_reserve(
                &e,
                &pool_address,
                &underlying_0,
                &reserve_config_0,
                &reserve_data_0,
            );

            let (underlying_1, _) = testutils::create_token_contract(&e, &bombadil);
            let (mut reserve_config_1, mut reserve_data_1) = testutils::default_reserve_meta();
            reserve_data_1.last_time = 12345;
            reserve_data_1.backstop_credit = 25_0000000;
            reserve_data_1.b_supply = 250_0000000;
            reserve_data_1.d_supply = 187_5000000;
            reserve_config_1.index = 1;
            testutils::create_reserve(
                &e,
                &pool_address,
                &underlying_1,
                &reserve_config_1,
                &reserve_data_1,
            );

            let (underlying_2, _) = testutils::create_token_contract(&e, &bombadil);
            let (mut reserve_config_2, mut reserve_data_2) = testutils::default_reserve_meta();
            reserve_data_2.b_rate = 1_100_000_000_000;
            reserve_data_2.last_time = 12345;
            reserve_config_2.index = 1;
            testutils::create_reserve(
                &e,
                &pool_address,
                &underlying_2,
                &reserve_config_2,
                &reserve_data_2,
            );

            oracle_client.set_data(
                &bombadil,
                &Asset::Other(Symbol::new(&e, "USD")),
                &vec![
                    &e,
                    Asset::Stellar(underlying_0.clone()),
                    Asset::Stellar(underlying_1.clone()),
                    Asset::Stellar(underlying_2),
                    Asset::Stellar(usdc_id),
                ],
                &7,
                &300,
            );
            oracle_client.set_price_stable(&vec![&e, 2_0000000, 4_0000000, 100_0000000, 1_0000000]);

            let pool_config = PoolConfig {
                oracle: oracle_id,
                min_collateral: 1_0000000,
                bstop_rate: 0_1000000,
                status: 0,
                max_positions: 4,
            };
            e.as_contract(&pool_address, || {
                storage::set_pool_config(&e, &pool_config);

                create_auction(
                    &e,
                    2,
                    &backstop_address,
                    &vec![&e, backstop_token_id],
                    &vec![&e, underlying_0.clone(), underlying_1, underlying_0],
                    100,
                );
            });
        }

        #[test]
        fn test_delete_user_liquidation() {
            let e = Env::default();
            e.mock_all_auths();

            let pool_id = create_pool(&e);
            let samwise = Address::generate(&e);

            let auction_data = AuctionData {
                bid: map![&e],
                lot: map![&e],
                block: 100,
            };
            e.as_contract(&pool_id, || {
                storage::set_auction(
                    &e,
                    &(AuctionType::UserLiquidation as u32),
                    &samwise,
                    &auction_data,
                );

                delete_liquidation(&e, &samwise);
                assert!(!storage::has_auction(
                    &e,
                    &(AuctionType::UserLiquidation as u32),
                    &samwise
                ));
            });
        }

        #[test]
        #[should_panic(expected = "Error(Contract, #1200)")]
        fn test_delete_user_liquidation_does_not_exist() {
            let e = Env::default();
            e.mock_all_auths();
            let pool_id = create_pool(&e);

            let samwise = Address::generate(&e);

            e.as_contract(&pool_id, || {
                delete_liquidation(&e, &samwise);
            });
        }

        #[test]
        fn test_fill() {
            let e = Env::default();

            e.mock_all_auths();
            e.ledger().set(LedgerInfo {
                timestamp: 12345,
                protocol_version: 22,
                sequence_number: 175,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 172800,
                min_persistent_entry_ttl: 172800,
                max_entry_ttl: 9999999,
            });

            let bombadil = Address::generate(&e);
            let samwise = Address::generate(&e);
            let frodo = Address::generate(&e);

            let pool_address = create_pool(&e);

            let (oracle_address, _) = testutils::create_mock_oracle(&e);

            // creating reserves for a pool exhausts the budget
            e.cost_estimate().budget().reset_unlimited();
            let (underlying_0, _) = testutils::create_token_contract(&e, &bombadil);
            let (mut reserve_config_0, reserve_data_0) = testutils::default_reserve_meta();
            reserve_config_0.index = 0;
            testutils::create_reserve(
                &e,
                &pool_address,
                &underlying_0,
                &reserve_config_0,
                &reserve_data_0,
            );

            let (underlying_1, _) = testutils::create_token_contract(&e, &bombadil);
            let (mut reserve_config_1, reserve_data_1) = testutils::default_reserve_meta();
            reserve_config_1.index = 1;
            testutils::create_reserve(
                &e,
                &pool_address,
                &underlying_1,
                &reserve_config_1,
                &reserve_data_1,
            );

            let (underlying_2, _) = testutils::create_token_contract(&e, &bombadil);
            let (mut reserve_config_2, reserve_data_2) = testutils::default_reserve_meta();
            reserve_config_2.index = 2;
            testutils::create_reserve(
                &e,
                &pool_address,
                &underlying_2,
                &reserve_config_2,
                &reserve_data_2,
            );
            e.cost_estimate().budget().reset_unlimited();

            let auction_data = AuctionData {
                bid: map![&e, (underlying_2.clone(), 1_2375000)],
                lot: map![
                    &e,
                    (underlying_0.clone(), 30_5595329),
                    (underlying_1.clone(), 1_5395739)
                ],
                block: 176,
            };
            let pool_config = PoolConfig {
                oracle: oracle_address,
                min_collateral: 1_0000000,
                bstop_rate: 0_1000000,
                status: 0,
                max_positions: 4,
            };
            let positions: Positions = Positions {
                collateral: map![
                    &e,
                    (reserve_config_0.index, 90_9100000),
                    (reserve_config_1.index, 04_5800000),
                ],
                liabilities: map![&e, (reserve_config_2.index, 02_7500000),],
                supply: map![&e],
            };
            e.as_contract(&pool_address, || {
                storage::set_user_positions(&e, &samwise, &positions);
                storage::set_pool_config(&e, &pool_config);
                storage::set_auction(&e, &0, &samwise, &auction_data);

                e.ledger().set(LedgerInfo {
                    timestamp: 12345 + 200 * 5,
                    protocol_version: 22,
                    sequence_number: 176 + 200,
                    network_id: Default::default(),
                    base_reserve: 10,
                    min_temp_entry_ttl: 172800,
                    min_persistent_entry_ttl: 172800,
                    max_entry_ttl: 9999999,
                });
                e.cost_estimate().budget().reset_unlimited();
                let mut pool = Pool::load(&e);
                let mut frodo_state = User::load(&e, &frodo);
                fill(&e, &mut pool, 0, &samwise, &mut frodo_state, 100);
                let has_auction = storage::has_auction(&e, &0, &samwise);
                assert_eq!(has_auction, false);
            });
        }

        #[test]
        fn test_partial_fill() {
            let e = Env::default();

            e.mock_all_auths();
            e.ledger().set(LedgerInfo {
                timestamp: 12345,
                protocol_version: 22,
                sequence_number: 175,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 172800,
                min_persistent_entry_ttl: 172800,
                max_entry_ttl: 9999999,
            });

            let bombadil = Address::generate(&e);
            let samwise = Address::generate(&e);
            let frodo = Address::generate(&e);

            let pool_address = create_pool(&e);

            let (oracle_address, _) = testutils::create_mock_oracle(&e);

            // creating reserves for a pool exhausts the budget
            e.cost_estimate().budget().reset_unlimited();
            let (underlying_0, _) = testutils::create_token_contract(&e, &bombadil);
            let (mut reserve_config_0, reserve_data_0) = testutils::default_reserve_meta();
            reserve_config_0.index = 0;
            testutils::create_reserve(
                &e,
                &pool_address,
                &underlying_0,
                &reserve_config_0,
                &reserve_data_0,
            );

            let (underlying_1, _) = testutils::create_token_contract(&e, &bombadil);
            let (mut reserve_config_1, reserve_data_1) = testutils::default_reserve_meta();
            reserve_config_1.index = 1;
            testutils::create_reserve(
                &e,
                &pool_address,
                &underlying_1,
                &reserve_config_1,
                &reserve_data_1,
            );

            let (underlying_2, _) = testutils::create_token_contract(&e, &bombadil);
            let (mut reserve_config_2, reserve_data_2) = testutils::default_reserve_meta();
            reserve_config_2.index = 2;
            testutils::create_reserve(
                &e,
                &pool_address,
                &underlying_2,
                &reserve_config_2,
                &reserve_data_2,
            );
            e.cost_estimate().budget().reset_unlimited();

            let auction_data = AuctionData {
                bid: map![&e, (underlying_2.clone(), 1_2375000)],
                lot: map![
                    &e,
                    (underlying_0.clone(), 30_5595329),
                    (underlying_1.clone(), 1_5395739)
                ],
                block: 176,
            };
            let pool_config = PoolConfig {
                oracle: oracle_address,
                min_collateral: 1_0000000,
                bstop_rate: 0_1000000,
                status: 0,
                max_positions: 4,
            };
            let positions: Positions = Positions {
                collateral: map![
                    &e,
                    (reserve_config_0.index, 90_9100000),
                    (reserve_config_1.index, 04_5800000),
                ],
                liabilities: map![&e, (reserve_config_2.index, 02_7500000),],
                supply: map![&e],
            };
            e.as_contract(&pool_address, || {
                storage::set_user_positions(&e, &samwise, &positions);
                storage::set_pool_config(&e, &pool_config);
                storage::set_auction(&e, &0, &samwise, &auction_data);

                e.ledger().set(LedgerInfo {
                    timestamp: 12345 + 200 * 5,
                    protocol_version: 22,
                    sequence_number: 176 + 200,
                    network_id: Default::default(),
                    base_reserve: 10,
                    min_temp_entry_ttl: 172800,
                    min_persistent_entry_ttl: 172800,
                    max_entry_ttl: 9999999,
                });
                e.cost_estimate().budget().reset_unlimited();
                let mut pool = Pool::load(&e);
                let mut frodo_state = User::load(&e, &frodo);
                fill(&e, &mut pool, 0, &samwise, &mut frodo_state, 25);

                let expected_new_auction_data = AuctionData {
                    bid: map![&e, (underlying_2.clone(), 9281250)],
                    lot: map![
                        &e,
                        (underlying_0.clone(), 22_9196497),
                        (underlying_1.clone(), 1_1546805)
                    ],
                    block: 176,
                };
                let new_auction = storage::get_auction(&e, &0, &samwise);
                assert_eq!(new_auction.bid, expected_new_auction_data.bid);
                assert_eq!(new_auction.lot, expected_new_auction_data.lot);
                assert_eq!(new_auction.block, expected_new_auction_data.block);
            });
        }

        #[test]
        fn test_partial_partial_full_fill() {
            let e = Env::default();
            e.cost_estimate().budget().reset_unlimited();
            e.mock_all_auths();

            e.ledger().set(LedgerInfo {
                timestamp: 12345,
                protocol_version: 22,
                sequence_number: 175,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 172800,
                min_persistent_entry_ttl: 172800,
                max_entry_ttl: 9999999,
            });

            let bombadil = Address::generate(&e);
            let samwise = Address::generate(&e);
            let frodo = Address::generate(&e);

            let pool_address = create_pool(&e);

            let (oracle_address, _) = testutils::create_mock_oracle(&e);

            let (underlying_0, _) = testutils::create_token_contract(&e, &bombadil);
            let (mut reserve_config_0, reserve_data_0) = testutils::default_reserve_meta();

            reserve_config_0.index = 0;
            testutils::create_reserve(
                &e,
                &pool_address,
                &underlying_0,
                &reserve_config_0,
                &reserve_data_0,
            );

            let (underlying_1, _) = testutils::create_token_contract(&e, &bombadil);
            let (mut reserve_config_1, reserve_data_1) = testutils::default_reserve_meta();

            reserve_config_1.index = 1;
            testutils::create_reserve(
                &e,
                &pool_address,
                &underlying_1,
                &reserve_config_1,
                &reserve_data_1,
            );

            let (underlying_2, _) = testutils::create_token_contract(&e, &bombadil);
            let (mut reserve_config_2, reserve_data_2) = testutils::default_reserve_meta();

            reserve_config_2.index = 2;
            testutils::create_reserve(
                &e,
                &pool_address,
                &underlying_2,
                &reserve_config_2,
                &reserve_data_2,
            );

            let auction_data = AuctionData {
                bid: map![&e, (underlying_2.clone(), 100_000_0000)],
                lot: map![
                    &e,
                    (underlying_0.clone(), 10_000_0000),
                    (underlying_1.clone(), 1_000_0000)
                ],
                block: 176,
            };
            let pool_config = PoolConfig {
                oracle: oracle_address,
                min_collateral: 1_0000000,
                bstop_rate: 0_1000000,
                status: 0,
                max_positions: 4,
            };
            let positions: Positions = Positions {
                collateral: map![
                    &e,
                    (reserve_config_0.index, 30_000_0000),
                    (reserve_config_1.index, 3_000_0000),
                ],
                liabilities: map![&e, (reserve_config_2.index, 200_000_0000),],
                supply: map![&e],
            };
            e.as_contract(&pool_address, || {
                storage::set_user_positions(&e, &samwise, &positions);
                storage::set_pool_config(&e, &pool_config);
                storage::set_auction(&e, &0, &samwise, &auction_data);

                // Partial fill 1 - 25% @ 50% lot mod
                e.ledger().set(LedgerInfo {
                    timestamp: 12345 + 100 * 5,
                    protocol_version: 22,
                    sequence_number: 176 + 100,
                    network_id: Default::default(),
                    base_reserve: 10,
                    min_temp_entry_ttl: 172800,
                    min_persistent_entry_ttl: 172800,
                    max_entry_ttl: 9999999,
                });
                let mut pool = Pool::load(&e);
                let mut frodo_state = User::load(&e, &frodo);
                fill(&e, &mut pool, 0, &samwise, &mut frodo_state, 25);

                let expected_new_auction_data = AuctionData {
                    bid: map![&e, (underlying_2.clone(), 75_000_0000)],
                    lot: map![
                        &e,
                        (underlying_0.clone(), 7_500_0000),
                        (underlying_1.clone(), 750_0000)
                    ],
                    block: 176,
                };

                // Partial fill 2 - 66% @ 100% mods
                let new_auction = storage::get_auction(&e, &0, &samwise);
                assert_eq!(new_auction.bid, expected_new_auction_data.bid);
                assert_eq!(new_auction.lot, expected_new_auction_data.lot);
                assert_eq!(new_auction.block, expected_new_auction_data.block);

                e.ledger().set(LedgerInfo {
                    timestamp: 12345 + 200 * 5,
                    protocol_version: 22,
                    sequence_number: 176 + 200,
                    network_id: Default::default(),
                    base_reserve: 10,
                    min_temp_entry_ttl: 172800,
                    min_persistent_entry_ttl: 172800,
                    max_entry_ttl: 9999999,
                });
                let mut pool = Pool::load(&e);
                let mut frodo_state = User::load(&e, &frodo);
                fill(&e, &mut pool, 0, &samwise, &mut frodo_state, 67);

                let expected_new_auction_data = AuctionData {
                    bid: map![&e, (underlying_2.clone(), 24_7500000)],
                    lot: map![
                        &e,
                        (underlying_0.clone(), 2_4750000),
                        (underlying_1.clone(), 0_2475000)
                    ],
                    block: 176,
                };
                let new_auction = storage::get_auction(&e, &0, &samwise);
                assert_eq!(new_auction.bid, expected_new_auction_data.bid);
                assert_eq!(new_auction.lot, expected_new_auction_data.lot);
                assert_eq!(new_auction.block, expected_new_auction_data.block);

                // full fill at 50% bid mod
                e.ledger().set(LedgerInfo {
                    timestamp: 12345 + 300 * 5,
                    protocol_version: 22,
                    sequence_number: 176 + 300,
                    network_id: Default::default(),
                    base_reserve: 10,
                    min_temp_entry_ttl: 172800,
                    min_persistent_entry_ttl: 172800,
                    max_entry_ttl: 9999999,
                });
                let mut pool = Pool::load(&e);
                let mut frodo_state = User::load(&e, &frodo);
                fill(&e, &mut pool, 0, &samwise, &mut frodo_state, 100);
                let new_auction = storage::has_auction(&e, &0, &samwise);
                assert_eq!(new_auction, false);
                let samwise_positions = storage::get_user_positions(&e, &samwise);
                assert_eq!(
                    samwise_positions
                        .collateral
                        .get(reserve_config_0.index)
                        .unwrap_optimized(),
                    30_000_0000 - 1_250_0000 - 5_000_0002 - 2_499_9998
                );
                assert_eq!(
                    samwise_positions
                        .collateral
                        .get(reserve_config_1.index)
                        .unwrap_optimized(),
                    3_000_0000 - 125_0000 - 500_0000 - 250_0000
                );
                assert_eq!(
                    samwise_positions
                        .liabilities
                        .get(reserve_config_2.index)
                        .unwrap_optimized(),
                    200_000_0000 - 25_000_0000 - 50_000_0025 - 12_6249975
                );
            });
        }

        #[test]
        #[should_panic(expected = "Error(Contract, #1200)")]
        fn test_fill_fails_pct_too_large() {
            let e = Env::default();

            e.mock_all_auths();
            e.ledger().set(LedgerInfo {
                timestamp: 12345,
                protocol_version: 22,
                sequence_number: 175,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 172800,
                min_persistent_entry_ttl: 172800,
                max_entry_ttl: 9999999,
            });

            let bombadil = Address::generate(&e);
            let samwise = Address::generate(&e);
            let frodo = Address::generate(&e);

            let pool_address = create_pool(&e);

            let (oracle_address, _) = testutils::create_mock_oracle(&e);

            // creating reserves for a pool exhausts the budget
            e.cost_estimate().budget().reset_unlimited();
            let (underlying_0, _) = testutils::create_token_contract(&e, &bombadil);
            let (mut reserve_config_0, reserve_data_0) = testutils::default_reserve_meta();
            reserve_config_0.index = 0;
            testutils::create_reserve(
                &e,
                &pool_address,
                &underlying_0,
                &reserve_config_0,
                &reserve_data_0,
            );

            let (underlying_1, _) = testutils::create_token_contract(&e, &bombadil);
            let (mut reserve_config_1, reserve_data_1) = testutils::default_reserve_meta();
            reserve_config_1.index = 1;
            testutils::create_reserve(
                &e,
                &pool_address,
                &underlying_1,
                &reserve_config_1,
                &reserve_data_1,
            );

            let (underlying_2, _) = testutils::create_token_contract(&e, &bombadil);
            let (mut reserve_config_2, reserve_data_2) = testutils::default_reserve_meta();
            reserve_config_2.index = 2;
            testutils::create_reserve(
                &e,
                &pool_address,
                &underlying_2,
                &reserve_config_2,
                &reserve_data_2,
            );

            let auction_data = AuctionData {
                bid: map![&e, (underlying_2.clone(), 1_2375000)],
                lot: map![
                    &e,
                    (underlying_0.clone(), 30_5595329),
                    (underlying_1.clone(), 1_5395739)
                ],
                block: 176,
            };
            let pool_config = PoolConfig {
                oracle: oracle_address,
                min_collateral: 1_0000000,
                bstop_rate: 0_1000000,
                status: 0,
                max_positions: 4,
            };
            let positions: Positions = Positions {
                collateral: map![
                    &e,
                    (reserve_config_0.index, 90_9100000),
                    (reserve_config_1.index, 04_5800000),
                ],
                liabilities: map![&e, (reserve_config_2.index, 02_7500000),],
                supply: map![&e],
            };
            e.as_contract(&pool_address, || {
                storage::set_user_positions(&e, &samwise, &positions);
                storage::set_pool_config(&e, &pool_config);
                storage::set_auction(&e, &0, &samwise, &auction_data);

                e.ledger().set(LedgerInfo {
                    timestamp: 12345 + 200 * 5,
                    protocol_version: 22,
                    sequence_number: 176 + 200,
                    network_id: Default::default(),
                    base_reserve: 10,
                    min_temp_entry_ttl: 172800,
                    min_persistent_entry_ttl: 172800,
                    max_entry_ttl: 9999999,
                });
                e.cost_estimate().budget().reset_unlimited();
                let mut pool = Pool::load(&e);
                let mut frodo_state = User::load(&e, &frodo);
                fill(&e, &mut pool, 0, &samwise, &mut frodo_state, 101);

                let expected_new_auction_data = AuctionData {
                    bid: map![&e, (underlying_2.clone(), 9281250)],
                    lot: map![
                        &e,
                        (underlying_0.clone(), 22_9196497),
                        (underlying_1.clone(), 1_1546805)
                    ],
                    block: 176,
                };
                let new_auction = storage::get_auction(&e, &0, &samwise);
                assert_eq!(new_auction.bid, expected_new_auction_data.bid);
                assert_eq!(new_auction.lot, expected_new_auction_data.lot);
                assert_eq!(new_auction.block, expected_new_auction_data.block);
            });
        }

        #[test]
        #[should_panic(expected = "Error(Contract, #1200)")]
        fn test_fill_fails_pct_too_small() {
            let e = Env::default();

            e.mock_all_auths();
            e.ledger().set(LedgerInfo {
                timestamp: 12345,
                protocol_version: 22,
                sequence_number: 175,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 172800,
                min_persistent_entry_ttl: 172800,
                max_entry_ttl: 9999999,
            });

            let bombadil = Address::generate(&e);
            let samwise = Address::generate(&e);
            let frodo = Address::generate(&e);

            let pool_address = create_pool(&e);

            let (oracle_address, _) = testutils::create_mock_oracle(&e);

            // creating reserves for a pool exhausts the budget
            e.cost_estimate().budget().reset_unlimited();
            let (underlying_0, _) = testutils::create_token_contract(&e, &bombadil);
            let (mut reserve_config_0, reserve_data_0) = testutils::default_reserve_meta();
            reserve_config_0.index = 0;
            testutils::create_reserve(
                &e,
                &pool_address,
                &underlying_0,
                &reserve_config_0,
                &reserve_data_0,
            );

            let (underlying_1, _) = testutils::create_token_contract(&e, &bombadil);
            let (mut reserve_config_1, reserve_data_1) = testutils::default_reserve_meta();

            reserve_config_1.index = 1;
            testutils::create_reserve(
                &e,
                &pool_address,
                &underlying_1,
                &reserve_config_1,
                &reserve_data_1,
            );

            let (underlying_2, _) = testutils::create_token_contract(&e, &bombadil);
            let (mut reserve_config_2, reserve_data_2) = testutils::default_reserve_meta();

            reserve_config_2.index = 2;
            testutils::create_reserve(
                &e,
                &pool_address,
                &underlying_2,
                &reserve_config_2,
                &reserve_data_2,
            );
            e.cost_estimate().budget().reset_unlimited();
            let auction_data = AuctionData {
                bid: map![&e, (underlying_2.clone(), 1_2375000)],
                lot: map![
                    &e,
                    (underlying_0.clone(), 30_5595329),
                    (underlying_1.clone(), 1_5395739)
                ],
                block: 176,
            };
            let pool_config = PoolConfig {
                oracle: oracle_address,
                min_collateral: 1_0000000,
                bstop_rate: 0_1000000,
                status: 0,
                max_positions: 4,
            };
            let positions: Positions = Positions {
                collateral: map![
                    &e,
                    (reserve_config_0.index, 90_9100000),
                    (reserve_config_1.index, 04_5800000),
                ],
                liabilities: map![&e, (reserve_config_2.index, 02_7500000),],
                supply: map![&e],
            };
            e.as_contract(&pool_address, || {
                storage::set_user_positions(&e, &samwise, &positions);
                storage::set_pool_config(&e, &pool_config);
                storage::set_auction(&e, &0, &samwise, &auction_data);

                e.ledger().set(LedgerInfo {
                    timestamp: 12345 + 200 * 5,
                    protocol_version: 22,
                    sequence_number: 176 + 200,
                    network_id: Default::default(),
                    base_reserve: 10,
                    min_temp_entry_ttl: 172800,
                    min_persistent_entry_ttl: 172800,
                    max_entry_ttl: 9999999,
                });
                e.cost_estimate().budget().reset_unlimited();
                let mut pool = Pool::load(&e);
                let mut frodo_state = User::load(&e, &frodo);
                fill(&e, &mut pool, 0, &samwise, &mut frodo_state, 0);

                let expected_new_auction_data = AuctionData {
                    bid: map![&e, (underlying_2.clone(), 9281250)],
                    lot: map![
                        &e,
                        (underlying_0.clone(), 22_9196497),
                        (underlying_1.clone(), 1_1546805)
                    ],
                    block: 176,
                };
                let new_auction = storage::get_auction(&e, &0, &samwise);
                assert_eq!(new_auction.bid, expected_new_auction_data.bid);
                assert_eq!(new_auction.lot, expected_new_auction_data.lot);
                assert_eq!(new_auction.block, expected_new_auction_data.block);
            });
        }

        #[test]
        #[should_panic(expected = "Error(Contract, #1211)")]
        fn test_fill_liquidation_same_address() {
            let e = Env::default();

            e.mock_all_auths();
            e.ledger().set(LedgerInfo {
                timestamp: 12345,
                protocol_version: 22,
                sequence_number: 175,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 172800,
                min_persistent_entry_ttl: 172800,
                max_entry_ttl: 9999999,
            });

            let bombadil = Address::generate(&e);
            let samwise = Address::generate(&e);

            let pool_address = create_pool(&e);

            let (oracle_address, _) = testutils::create_mock_oracle(&e);

            // creating reserves for a pool exhausts the budget
            e.cost_estimate().budget().reset_unlimited();
            let (underlying_0, _) = testutils::create_token_contract(&e, &bombadil);
            let (mut reserve_config_0, reserve_data_0) = testutils::default_reserve_meta();
            reserve_config_0.index = 0;
            testutils::create_reserve(
                &e,
                &pool_address,
                &underlying_0,
                &reserve_config_0,
                &reserve_data_0,
            );

            let (underlying_1, _) = testutils::create_token_contract(&e, &bombadil);
            let (mut reserve_config_1, reserve_data_1) = testutils::default_reserve_meta();
            reserve_config_1.index = 1;
            testutils::create_reserve(
                &e,
                &pool_address,
                &underlying_1,
                &reserve_config_1,
                &reserve_data_1,
            );

            let (underlying_2, _) = testutils::create_token_contract(&e, &bombadil);
            let (mut reserve_config_2, reserve_data_2) = testutils::default_reserve_meta();
            reserve_config_2.index = 2;
            testutils::create_reserve(
                &e,
                &pool_address,
                &underlying_2,
                &reserve_config_2,
                &reserve_data_2,
            );
            e.cost_estimate().budget().reset_unlimited();

            let auction_data = AuctionData {
                bid: map![&e, (underlying_2.clone(), 1_2375000)],
                lot: map![
                    &e,
                    (underlying_0.clone(), 30_5595329),
                    (underlying_1.clone(), 1_5395739)
                ],
                block: 176,
            };
            let pool_config = PoolConfig {
                oracle: oracle_address,
                min_collateral: 1_0000000,
                bstop_rate: 0_1000000,
                status: 0,
                max_positions: 4,
            };
            let positions: Positions = Positions {
                collateral: map![
                    &e,
                    (reserve_config_0.index, 90_9100000),
                    (reserve_config_1.index, 04_5800000),
                ],
                liabilities: map![&e, (reserve_config_2.index, 02_7500000),],
                supply: map![&e],
            };
            e.as_contract(&pool_address, || {
                storage::set_user_positions(&e, &samwise, &positions);
                storage::set_pool_config(&e, &pool_config);
                storage::set_auction(&e, &0, &samwise, &auction_data);

                e.ledger().set(LedgerInfo {
                    timestamp: 12345 + 200 * 5,
                    protocol_version: 22,
                    sequence_number: 176 + 200,
                    network_id: Default::default(),
                    base_reserve: 10,
                    min_temp_entry_ttl: 172800,
                    min_persistent_entry_ttl: 172800,
                    max_entry_ttl: 9999999,
                });
                e.cost_estimate().budget().reset_unlimited();
                let mut pool = Pool::load(&e);
                let mut samwise_state = User::load(&e, &samwise);
                fill(&e, &mut pool, 0, &samwise, &mut samwise_state, 100);
            });
        }

        #[test]
        fn test_delete_stale_auction() {
            let e = Env::default();
            e.mock_all_auths();

            e.ledger().set(LedgerInfo {
                timestamp: 12345,
                protocol_version: 22,
                sequence_number: 1500,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 172800,
                min_persistent_entry_ttl: 172800,
                max_entry_ttl: 9999999,
            });

            let pool_address = create_pool(&e);
            let auction_type: u32 = 2;
            let user = Address::generate(&e);
            let underlying_0 = Address::generate(&e);
            let underlying_1 = Address::generate(&e);

            let auction_data = AuctionData {
                bid: map![&e, (underlying_0.clone(), 100_0000000)],
                lot: map![&e, (underlying_1.clone(), 100_0000000)],
                block: 1000,
            };
            e.as_contract(&pool_address, || {
                storage::set_auction(&e, &auction_type, &user, &auction_data);
                let has_auction = storage::has_auction(&e, &auction_type, &user);
                assert_eq!(has_auction, true);

                delete_stale_auction(&e, auction_type, &user);
                let has_auction = storage::has_auction(&e, &auction_type, &user);
                assert_eq!(has_auction, false);
            });
        }

        // #[test]
        // fn test_delete_stale_auction_bad_debt() {
        //     let e = Env::default();
        //     e.mock_all_auths();

        //     e.ledger().set(LedgerInfo {
        //         timestamp: 12345,
        //         protocol_version: 22,
        //         sequence_number: 1500,
        //         network_id: Default::default(),
        //         base_reserve: 10,
        //         min_temp_entry_ttl: 172800,
        //         min_persistent_entry_ttl: 172800,
        //         max_entry_ttl: 9999999,
        //     });

        //     let pool_address = create_pool(&e);
        //     let bombadil = Address::generate(&e);
        //     let frodo = Address::generate(&e);

        //     let (blnd, blnd_client) = create_blnd_token(&e, &pool_address, &bombadil);
        //     let (usdc, usdc_client) = create_token_contract(&e, &bombadil);
        //     let (lp_token, lp_token_client) = create_comet_lp_pool(&e, &bombadil, &blnd, &usdc);
        //     let (backstop_address, backstop_client) =
        //         create_backstop(&e, &pool_address, &lp_token, &usdc, &blnd);

        //     // mint lp tokens and deposit them into the pool's backstop
        //     let backstop_tokens = 1_500_0000000; // over 5% of threshold
        //     blnd_client.mint(&frodo, &500_001_0000000);
        //     blnd_client.approve(&frodo, &lp_token, &i128::MAX, &99999);
        //     usdc_client.mint(&frodo, &12_501_0000000);
        //     usdc_client.approve(&frodo, &lp_token, &i128::MAX, &99999);
        //     lp_token_client.join_pool(
        //         &backstop_tokens,
        //         &vec![&e, 500_001_0000000, 12_501_0000000],
        //         &frodo,
        //     );
        //     backstop_client.deposit(&frodo, &pool_address, &backstop_tokens);

        //     let (underlying_0, _) = testutils::create_token_contract(&e, &bombadil);
        //     let (reserve_config, reserve_data_0) = testutils::default_reserve_meta();
        //     testutils::create_reserve(
        //         &e,
        //         &pool_address,
        //         &underlying_0,
        //         &reserve_config,
        //         &reserve_data_0,
        //     );

        //     let (underlying_1, _) = testutils::create_token_contract(&e, &bombadil);
        //     let (reserve_config, reserve_data_1) = testutils::default_reserve_meta();
        //     testutils::create_reserve(
        //         &e,
        //         &pool_address,
        //         &underlying_1,
        //         &reserve_config,
        //         &reserve_data_1,
        //     );

        //     let auction_type: u32 = 1;
        //     let auction_data = AuctionData {
        //         bid: map![&e, (underlying_0.clone(), 100_0000000)],
        //         lot: map![&e, (underlying_1.clone(), 100_0000000)],
        //         block: 1000,
        //     };

        //     let backstop_positions = Positions {
        //         collateral: map![&e],
        //         liabilities: map![&e, (0, 100_0000000)],
        //         supply: map![&e,],
        //     };
        //     let pool_config = PoolConfig {
        //         oracle: Address::generate(&e),
        //         min_collateral: 1_0000000,
        //         bstop_rate: 0_1000000,
        //         status: 1,
        //         max_positions: 5,
        //     };
        //     e.as_contract(&pool_address, || {
        //         storage::set_pool_config(&e, &pool_config);
        //         storage::set_user_positions(&e, &backstop_address, &backstop_positions);
        //         storage::set_auction(&e, &auction_type, &backstop_address, &auction_data);
        //         let has_auction = storage::has_auction(&e, &auction_type, &backstop_address);
        //         assert_eq!(has_auction, true);

        //         delete_stale_auction(&e, auction_type, &backstop_address);
        //         let has_auction = storage::has_auction(&e, &auction_type, &backstop_address);
        //         assert_eq!(has_auction, false);

        //         // validate no other state changed
        //         let post_backstop_positions = storage::get_user_positions(&e, &backstop_address);
        //         assert_eq!(post_backstop_positions.collateral.len(), 0);
        //         assert_eq!(
        //             post_backstop_positions.liabilities,
        //             backstop_positions.liabilities
        //         );
        //         assert_eq!(post_backstop_positions.supply.len(), 0);

        //         let post_reserve_data_0 = storage::get_res_data(&e, &underlying_0);
        //         assert_eq!(post_reserve_data_0.last_time, 0);
        //         assert_eq!(post_reserve_data_0.d_supply, reserve_data_0.d_supply);
        //         let post_reserve_data_1 = storage::get_res_data(&e, &underlying_1);
        //         assert_eq!(post_reserve_data_1.last_time, 0);
        //         assert_eq!(post_reserve_data_1.d_supply, reserve_data_1.d_supply);
        //     });
        // }

        // #[test]
        // fn test_delete_stale_auction_bad_debt_needs_default() {
        //     let e = Env::default();
        //     e.mock_all_auths();

        //     e.ledger().set(LedgerInfo {
        //         timestamp: 12345,
        //         protocol_version: 22,
        //         sequence_number: 1500,
        //         network_id: Default::default(),
        //         base_reserve: 10,
        //         min_temp_entry_ttl: 172800,
        //         min_persistent_entry_ttl: 172800,
        //         max_entry_ttl: 9999999,
        //     });

        //     let pool_address = create_pool(&e);
        //     let bombadil = Address::generate(&e);
        //     let frodo = Address::generate(&e);

        //     let (blnd, blnd_client) = create_blnd_token(&e, &pool_address, &bombadil);
        //     let (usdc, usdc_client) = create_token_contract(&e, &bombadil);
        //     let (lp_token, lp_token_client) = create_comet_lp_pool(&e, &bombadil, &blnd, &usdc);
        //     let (backstop_address, backstop_client) =
        //         create_backstop(&e, &pool_address, &lp_token, &usdc, &blnd);

        //     // mint lp tokens and deposit them into the pool's backstop
        //     let backstop_tokens = 1_000_0000000; // under 5% of threshold
        //     blnd_client.mint(&frodo, &500_001_0000000);
        //     blnd_client.approve(&frodo, &lp_token, &i128::MAX, &99999);
        //     usdc_client.mint(&frodo, &12_501_0000000);
        //     usdc_client.approve(&frodo, &lp_token, &i128::MAX, &99999);
        //     lp_token_client.join_pool(
        //         &backstop_tokens,
        //         &vec![&e, 500_001_0000000, 12_501_0000000],
        //         &frodo,
        //     );
        //     backstop_client.deposit(&frodo, &pool_address, &backstop_tokens);

        //     let (underlying_0, _) = testutils::create_token_contract(&e, &bombadil);
        //     let (reserve_config, reserve_data_0) = testutils::default_reserve_meta();
        //     testutils::create_reserve(
        //         &e,
        //         &pool_address,
        //         &underlying_0,
        //         &reserve_config,
        //         &reserve_data_0,
        //     );

        //     let (underlying_1, _) = testutils::create_token_contract(&e, &bombadil);
        //     let (reserve_config, reserve_data_1) = testutils::default_reserve_meta();
        //     testutils::create_reserve(
        //         &e,
        //         &pool_address,
        //         &underlying_1,
        //         &reserve_config,
        //         &reserve_data_1,
        //     );

        //     let auction_type: u32 = 1;
        //     let auction_data = AuctionData {
        //         bid: map![&e, (underlying_0.clone(), 100_0000000)],
        //         lot: map![&e, (underlying_1.clone(), 100_0000000)],
        //         block: 1000,
        //     };

        //     let backstop_positions = Positions {
        //         collateral: map![&e],
        //         liabilities: map![&e, (0, 100_0000000)],
        //         supply: map![&e,],
        //     };
        //     let pool_config = PoolConfig {
        //         oracle: Address::generate(&e),
        //         min_collateral: 1_0000000,
        //         bstop_rate: 0_1000000,
        //         status: 1,
        //         max_positions: 5,
        //     };
        //     e.as_contract(&pool_address, || {
        //         storage::set_pool_config(&e, &pool_config);
        //         storage::set_user_positions(&e, &backstop_address, &backstop_positions);
        //         storage::set_auction(&e, &auction_type, &backstop_address, &auction_data);
        //         let has_auction = storage::has_auction(&e, &auction_type, &backstop_address);
        //         assert_eq!(has_auction, true);

        //         delete_stale_auction(&e, auction_type, &backstop_address);
        //         let has_auction = storage::has_auction(&e, &auction_type, &backstop_address);
        //         assert_eq!(has_auction, false);

        //         // validate backstop positions defaulted
        //         let post_backstop_positions = storage::get_user_positions(&e, &backstop_address);
        //         assert_eq!(post_backstop_positions.collateral.len(), 0);
        //         assert_eq!(post_backstop_positions.liabilities.len(), 0);
        //         assert_eq!(post_backstop_positions.supply.len(), 0);

        //         let post_reserve_data_0 = storage::get_res_data(&e, &underlying_0);
        //         assert_eq!(post_reserve_data_0.last_time, 12345);
        //         assert!(post_reserve_data_0.d_supply < reserve_data_0.d_supply);
        //         assert!(post_reserve_data_0.d_rate > reserve_data_0.d_rate);
        //         assert_eq!(post_reserve_data_0.b_supply, reserve_data_0.b_supply);
        //         assert!(post_reserve_data_0.b_rate < reserve_data_0.b_rate);
        //         // non-affected reserve not changed
        //         let post_reserve_data_1 = storage::get_res_data(&e, &underlying_1);
        //         assert_eq!(post_reserve_data_1.last_time, 0);
        //         assert_eq!(post_reserve_data_1.d_supply, reserve_data_1.d_supply);
        //     });
        // }

        // #[test]
        // fn test_delete_stale_auction_user_liquidation() {
        //     let e = Env::default();
        //     e.mock_all_auths();

        //     e.ledger().set(LedgerInfo {
        //         timestamp: 12345,
        //         protocol_version: 22,
        //         sequence_number: 1500,
        //         network_id: Default::default(),
        //         base_reserve: 10,
        //         min_temp_entry_ttl: 172800,
        //         min_persistent_entry_ttl: 172800,
        //         max_entry_ttl: 9999999,
        //     });

        //     let pool_address = create_pool(&e);
        //     let bombadil = Address::generate(&e);
        //     let frodo = Address::generate(&e);
        //     let samwise = Address::generate(&e);

        //     let (blnd, blnd_client) = create_blnd_token(&e, &pool_address, &bombadil);
        //     let (usdc, usdc_client) = create_token_contract(&e, &bombadil);
        //     let (lp_token, lp_token_client) = create_comet_lp_pool(&e, &bombadil, &blnd, &usdc);
        //     let (_, backstop_client) = create_backstop(&e, &pool_address, &lp_token, &usdc, &blnd);

        //     // mint lp tokens and deposit them into the pool's backstop
        //     let backstop_tokens = 1_500_0000000; // over 5% of threshold
        //     blnd_client.mint(&frodo, &500_001_0000000);
        //     blnd_client.approve(&frodo, &lp_token, &i128::MAX, &99999);
        //     usdc_client.mint(&frodo, &12_501_0000000);
        //     usdc_client.approve(&frodo, &lp_token, &i128::MAX, &99999);
        //     lp_token_client.join_pool(
        //         &backstop_tokens,
        //         &vec![&e, 500_001_0000000, 12_501_0000000],
        //         &frodo,
        //     );
        //     backstop_client.deposit(&frodo, &pool_address, &backstop_tokens);

        //     let (underlying_0, _) = testutils::create_token_contract(&e, &bombadil);
        //     let (reserve_config, reserve_data_0) = testutils::default_reserve_meta();
        //     testutils::create_reserve(
        //         &e,
        //         &pool_address,
        //         &underlying_0,
        //         &reserve_config,
        //         &reserve_data_0,
        //     );

        //     let (underlying_1, _) = testutils::create_token_contract(&e, &bombadil);
        //     let (reserve_config, reserve_data_1) = testutils::default_reserve_meta();
        //     testutils::create_reserve(
        //         &e,
        //         &pool_address,
        //         &underlying_1,
        //         &reserve_config,
        //         &reserve_data_1,
        //     );

        //     let auction_type: u32 = 0;
        //     let auction_data = AuctionData {
        //         bid: map![&e, (underlying_0.clone(), 100_0000000)],
        //         lot: map![&e, (underlying_1.clone(), 100_0000000)],
        //         block: 1000,
        //     };

        //     let positions = Positions {
        //         collateral: map![&e, (1, 100_0000000)],
        //         liabilities: map![&e, (0, 100_0000000)],
        //         supply: map![&e,],
        //     };
        //     let pool_config = PoolConfig {
        //         oracle: Address::generate(&e),
        //         min_collateral: 1_0000000,
        //         bstop_rate: 0_1000000,
        //         status: 1,
        //         max_positions: 5,
        //     };
        //     e.as_contract(&pool_address, || {
        //         storage::set_pool_config(&e, &pool_config);
        //         storage::set_user_positions(&e, &samwise, &positions);
        //         storage::set_auction(&e, &auction_type, &samwise, &auction_data);
        //         let has_auction = storage::has_auction(&e, &auction_type, &samwise);
        //         assert_eq!(has_auction, true);

        //         delete_stale_auction(&e, auction_type, &samwise);
        //         let has_auction = storage::has_auction(&e, &auction_type, &samwise);
        //         assert_eq!(has_auction, false);

        //         // validate no other state changed
        //         let post_positions = storage::get_user_positions(&e, &samwise);
        //         assert_eq!(post_positions.collateral, positions.collateral);
        //         assert_eq!(post_positions.liabilities, positions.liabilities);
        //         assert_eq!(post_positions.supply, positions.supply);

        //         let post_reserve_data_0 = storage::get_res_data(&e, &underlying_0);
        //         assert_eq!(post_reserve_data_0.last_time, 0);
        //         assert_eq!(post_reserve_data_0.d_supply, reserve_data_0.d_supply);
        //         let post_reserve_data_1 = storage::get_res_data(&e, &underlying_1);
        //         assert_eq!(post_reserve_data_1.last_time, 0);
        //         assert_eq!(post_reserve_data_1.d_supply, reserve_data_1.d_supply);
        //     });
        // }

        // #[test]
        // fn test_delete_stale_auction_user_liquidation_bad_debt() {
        //     let e = Env::default();
        //     e.mock_all_auths();

        //     e.ledger().set(LedgerInfo {
        //         timestamp: 12345,
        //         protocol_version: 22,
        //         sequence_number: 1500,
        //         network_id: Default::default(),
        //         base_reserve: 10,
        //         min_temp_entry_ttl: 172800,
        //         min_persistent_entry_ttl: 172800,
        //         max_entry_ttl: 9999999,
        //     });

        //     let pool_address = create_pool(&e);
        //     let bombadil = Address::generate(&e);
        //     let frodo = Address::generate(&e);
        //     let samwise = Address::generate(&e);

        //     let (blnd, blnd_client) = create_blnd_token(&e, &pool_address, &bombadil);
        //     let (usdc, usdc_client) = create_token_contract(&e, &bombadil);
        //     let (lp_token, lp_token_client) = create_comet_lp_pool(&e, &bombadil, &blnd, &usdc);
        //     let (backstop_address, backstop_client) =
        //         create_backstop(&e, &pool_address, &lp_token, &usdc, &blnd);

        //     // mint lp tokens and deposit them into the pool's backstop
        //     let backstop_tokens = 1_500_0000000; // over 5% of threshold
        //     blnd_client.mint(&frodo, &500_001_0000000);
        //     blnd_client.approve(&frodo, &lp_token, &i128::MAX, &99999);
        //     usdc_client.mint(&frodo, &12_501_0000000);
        //     usdc_client.approve(&frodo, &lp_token, &i128::MAX, &99999);
        //     lp_token_client.join_pool(
        //         &backstop_tokens,
        //         &vec![&e, 500_001_0000000, 12_501_0000000],
        //         &frodo,
        //     );
        //     backstop_client.deposit(&frodo, &pool_address, &backstop_tokens);

        //     let (underlying_0, _) = testutils::create_token_contract(&e, &bombadil);
        //     let (reserve_config, reserve_data_0) = testutils::default_reserve_meta();
        //     testutils::create_reserve(
        //         &e,
        //         &pool_address,
        //         &underlying_0,
        //         &reserve_config,
        //         &reserve_data_0,
        //     );

        //     let (underlying_1, _) = testutils::create_token_contract(&e, &bombadil);
        //     let (reserve_config, reserve_data_1) = testutils::default_reserve_meta();
        //     testutils::create_reserve(
        //         &e,
        //         &pool_address,
        //         &underlying_1,
        //         &reserve_config,
        //         &reserve_data_1,
        //     );

        //     let auction_type: u32 = 0;
        //     let auction_data = AuctionData {
        //         bid: map![&e, (underlying_0.clone(), 100_0000000)],
        //         lot: map![&e, (underlying_1.clone(), 100_0000000)],
        //         block: 1000,
        //     };

        //     let positions = Positions {
        //         collateral: map![&e],
        //         liabilities: map![&e, (0, 100_0000000)],
        //         supply: map![&e,],
        //     };
        //     let pool_config = PoolConfig {
        //         oracle: Address::generate(&e),
        //         min_collateral: 1_0000000,
        //         bstop_rate: 0_1000000,
        //         status: 1,
        //         max_positions: 5,
        //     };
        //     e.as_contract(&pool_address, || {
        //         storage::set_pool_config(&e, &pool_config);
        //         storage::set_user_positions(&e, &samwise, &positions);
        //         storage::set_auction(&e, &auction_type, &samwise, &auction_data);
        //         let has_auction = storage::has_auction(&e, &auction_type, &samwise);
        //         assert_eq!(has_auction, true);

        //         delete_stale_auction(&e, auction_type, &samwise);
        //         let has_auction = storage::has_auction(&e, &auction_type, &samwise);
        //         assert_eq!(has_auction, false);

        //         // validate bad debt assigned to backstop
        //         let post_positions = storage::get_user_positions(&e, &samwise);
        //         assert_eq!(post_positions.collateral.len(), 0);
        //         assert_eq!(post_positions.liabilities.len(), 0);
        //         assert_eq!(post_positions.supply.len(), 0);

        //         let backstop_positions = storage::get_user_positions(&e, &backstop_address);
        //         assert_eq!(backstop_positions.collateral.len(), 0);
        //         assert_eq!(backstop_positions.liabilities, positions.liabilities);
        //         assert_eq!(backstop_positions.supply.len(), 0);

        //         let post_reserve_data_0 = storage::get_res_data(&e, &underlying_0);
        //         assert_eq!(post_reserve_data_0.last_time, 12345);
        //         assert_eq!(post_reserve_data_0.d_supply, reserve_data_0.d_supply);
        //         let post_reserve_data_1 = storage::get_res_data(&e, &underlying_1);
        //         assert_eq!(post_reserve_data_1.last_time, 0);
        //         assert_eq!(post_reserve_data_1.d_supply, reserve_data_1.d_supply);
        //     });
        // }

        #[test]
        #[should_panic(expected = "Error(Contract, #1200)")]
        fn test_delete_stale_auction_not_stale() {
            let e = Env::default();
            e.mock_all_auths();

            e.ledger().set(LedgerInfo {
                timestamp: 12345,
                protocol_version: 22,
                sequence_number: 1500,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 172800,
                min_persistent_entry_ttl: 172800,
                max_entry_ttl: 9999999,
            });

            let pool_address = create_pool(&e);
            let user = Address::generate(&e);
            let underlying_0 = Address::generate(&e);
            let underlying_1 = Address::generate(&e);

            let auction_type: u32 = 2;
            let auction_data = AuctionData {
                bid: map![&e, (underlying_0.clone(), 100_0000000)],
                lot: map![&e, (underlying_1.clone(), 100_0000000)],
                block: 1001,
            };

            e.as_contract(&pool_address, || {
                storage::set_auction(&e, &auction_type, &user, &auction_data);
                let has_auction = storage::has_auction(&e, &auction_type, &user);
                assert_eq!(has_auction, true);

                delete_stale_auction(&e, auction_type, &user);
            });
        }

        #[test]
        #[should_panic(expected = "Error(Contract, #1200)")]
        fn test_delete_stale_auction_does_not_exist() {
            let e = Env::default();
            e.mock_all_auths();

            e.ledger().set(LedgerInfo {
                timestamp: 12345,
                protocol_version: 22,
                sequence_number: 1500,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 172800,
                min_persistent_entry_ttl: 172800,
                max_entry_ttl: 9999999,
            });

            let pool_address = create_pool(&e);
            let auction_type: u32 = 2;
            let user = Address::generate(&e);
            let underlying_0 = Address::generate(&e);
            let underlying_1 = Address::generate(&e);

            let auction_data = AuctionData {
                bid: map![&e, (underlying_0.clone(), 100_0000000)],
                lot: map![&e, (underlying_1.clone(), 100_0000000)],
                block: 1001,
            };

            e.as_contract(&pool_address, || {
                storage::set_auction(&e, &auction_type, &user, &auction_data);
                let has_auction = storage::has_auction(&e, &auction_type, &user);
                assert_eq!(has_auction, true);

                delete_stale_auction(&e, 0, &user);
            });
        }

        #[test]
        fn test_scale_auction_100_fill_pct() {
            // 0 blocks
            let e = Env::default();
            let underlying_0 = Address::generate(&e);
            let underlying_1 = Address::generate(&e);

            let base_auction_data = AuctionData {
                bid: map![&e, (underlying_0.clone(), 100_0000000)],
                lot: map![&e, (underlying_1.clone(), 100_0000000)],
                block: 1000,
            };

            // 0 blocks
            e.ledger().set(LedgerInfo {
                timestamp: 12345,
                protocol_version: 22,
                sequence_number: 1000,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 172800,
                min_persistent_entry_ttl: 172800,
                max_entry_ttl: 9999999,
            });
            let (scaled_auction, remaining_auction) = scale_auction(&e, &base_auction_data, 100);
            assert_eq!(
                scaled_auction.bid.get_unchecked(underlying_0.clone()),
                100_0000000
            );
            assert_eq!(scaled_auction.lot.len(), 0);
            assert!(remaining_auction.is_none());

            // 100 blocks
            e.ledger().set(LedgerInfo {
                timestamp: 12345,
                protocol_version: 22,
                sequence_number: 1100,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 172800,
                min_persistent_entry_ttl: 172800,
                max_entry_ttl: 9999999,
            });
            let (scaled_auction, remaining_auction) = scale_auction(&e, &base_auction_data, 100);
            assert_eq!(
                scaled_auction.bid.get_unchecked(underlying_0.clone()),
                100_0000000
            );
            assert_eq!(
                scaled_auction.lot.get_unchecked(underlying_1.clone()),
                50_0000000
            );
            assert!(remaining_auction.is_none());

            // 200 blocks
            e.ledger().set(LedgerInfo {
                timestamp: 12345,
                protocol_version: 22,
                sequence_number: 1200,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 172800,
                min_persistent_entry_ttl: 172800,
                max_entry_ttl: 9999999,
            });
            let (scaled_auction, remaining_auction) = scale_auction(&e, &base_auction_data, 100);
            assert_eq!(
                scaled_auction.bid.get_unchecked(underlying_0.clone()),
                100_0000000
            );
            assert_eq!(
                scaled_auction.lot.get_unchecked(underlying_1.clone()),
                100_0000000
            );
            assert!(remaining_auction.is_none());

            // 300 blocks
            e.ledger().set(LedgerInfo {
                timestamp: 12345,
                protocol_version: 22,
                sequence_number: 1300,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 172800,
                min_persistent_entry_ttl: 172800,
                max_entry_ttl: 9999999,
            });
            let (scaled_auction, remaining_auction) = scale_auction(&e, &base_auction_data, 100);
            assert_eq!(
                scaled_auction.bid.get_unchecked(underlying_0.clone()),
                50_0000000
            );
            assert_eq!(
                scaled_auction.lot.get_unchecked(underlying_1.clone()),
                100_0000000
            );
            assert!(remaining_auction.is_none());

            // 400 blocks
            e.ledger().set(LedgerInfo {
                timestamp: 12345,
                protocol_version: 22,
                sequence_number: 1400,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 172800,
                min_persistent_entry_ttl: 172800,
                max_entry_ttl: 9999999,
            });
            let (scaled_auction, remaining_auction) = scale_auction(&e, &base_auction_data, 100);
            assert_eq!(scaled_auction.bid.len(), 0);
            assert_eq!(
                scaled_auction.lot.get_unchecked(underlying_1.clone()),
                100_0000000
            );
            assert!(remaining_auction.is_none());
        }

        #[test]
        fn test_scale_auction_not_100_fill_pct() {
            // @dev: bids always round up, lots always round down
            //       the remaining is exact based on scaled auction
            let e = Env::default();
            let underlying_0 = Address::generate(&e);
            let underlying_1 = Address::generate(&e);

            let base_auction_data = AuctionData {
                bid: map![&e, (underlying_0.clone(), 25_0000005)],
                lot: map![&e, (underlying_1.clone(), 25_0000005)],
                block: 1000,
            };

            // 0 blocks
            e.ledger().set(LedgerInfo {
                timestamp: 12345,
                protocol_version: 22,
                sequence_number: 1000,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 172800,
                min_persistent_entry_ttl: 172800,
                max_entry_ttl: 9999999,
            });
            let (scaled_auction, remaining_auction_option) =
                scale_auction(&e, &base_auction_data, 50);
            let remaining_auction = remaining_auction_option.unwrap();
            assert_eq!(
                scaled_auction.bid.get_unchecked(underlying_0.clone()),
                12_5000003 // fill pct rounds up
            );
            assert_eq!(scaled_auction.lot.len(), 0);
            assert_eq!(
                remaining_auction.bid.get_unchecked(underlying_0.clone()),
                12_5000002
            );
            assert_eq!(
                remaining_auction.lot.get_unchecked(underlying_1.clone()),
                12_5000003
            );

            // 100 blocks
            e.ledger().set(LedgerInfo {
                timestamp: 12345,
                protocol_version: 22,
                sequence_number: 1100,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 172800,
                min_persistent_entry_ttl: 172800,
                max_entry_ttl: 9999999,
            });

            let (scaled_auction, remaining_auction_option) =
                scale_auction(&e, &base_auction_data, 60);
            let remaining_auction = remaining_auction_option.unwrap();
            assert_eq!(
                scaled_auction.bid.get_unchecked(underlying_0.clone()),
                15_0000003
            );
            assert_eq!(
                scaled_auction.lot.get_unchecked(underlying_1.clone()),
                7_5000001 // modifier rounds down
            );
            assert_eq!(
                remaining_auction.bid.get_unchecked(underlying_0.clone()),
                10_0000002
            );
            assert_eq!(
                remaining_auction.lot.get_unchecked(underlying_1.clone()),
                10_0000002
            );

            // 300 blocks
            e.ledger().set(LedgerInfo {
                timestamp: 12345,
                protocol_version: 22,
                sequence_number: 1300,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 172800,
                min_persistent_entry_ttl: 172800,
                max_entry_ttl: 9999999,
            });

            let (scaled_auction, remaining_auction_option) =
                scale_auction(&e, &base_auction_data, 60);
            let remaining_auction = remaining_auction_option.unwrap();
            assert_eq!(
                scaled_auction.bid.get_unchecked(underlying_0.clone()),
                7_5000002 // modifier rounds up
            );
            assert_eq!(
                scaled_auction.lot.get_unchecked(underlying_1.clone()),
                15_0000003
            );
            assert_eq!(
                remaining_auction.bid.get_unchecked(underlying_0.clone()),
                10_0000002
            );
            assert_eq!(
                remaining_auction.lot.get_unchecked(underlying_1.clone()),
                10_0000002
            );

            // 400 blocks
            e.ledger().set(LedgerInfo {
                timestamp: 12345,
                protocol_version: 22,
                sequence_number: 1400,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 172800,
                min_persistent_entry_ttl: 172800,
                max_entry_ttl: 9999999,
            });
            let (scaled_auction, remaining_auction_option) =
                scale_auction(&e, &base_auction_data, 50);
            let remaining_auction = remaining_auction_option.unwrap();
            assert_eq!(scaled_auction.bid.len(), 0);
            assert_eq!(
                scaled_auction.lot.get_unchecked(underlying_1.clone()),
                12_5000002 // fill pct rounds down
            );
            assert_eq!(
                remaining_auction.bid.get_unchecked(underlying_0.clone()),
                12_5000002
            );
            assert_eq!(
                remaining_auction.lot.get_unchecked(underlying_1.clone()),
                12_5000003
            );
        }

        #[test]
        #[should_panic(expected = "Error(Contract, #1200)")]
        fn test_scale_auction_fill_percentage_zero() {
            let e = Env::default();
            let underlying_0 = Address::generate(&e);
            let underlying_1 = Address::generate(&e);

            let base_auction_data = AuctionData {
                bid: map![&e, (underlying_0.clone(), 25_0000005)],
                lot: map![&e, (underlying_1.clone(), 25_0000005)],
                block: 1000,
            };

            // 0 blocks
            e.ledger().set(LedgerInfo {
                timestamp: 12345,
                protocol_version: 22,
                sequence_number: 1000,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 172800,
                min_persistent_entry_ttl: 172800,
                max_entry_ttl: 9999999,
            });

            let (_, _) = scale_auction(&e, &base_auction_data, 0);
        }

        #[test]
        #[should_panic(expected = "Error(Contract, #1200)")]
        fn test_scale_auction_fill_percentage_over_100() {
            let e = Env::default();
            let underlying_0 = Address::generate(&e);
            let underlying_1 = Address::generate(&e);

            let base_auction_data = AuctionData {
                bid: map![&e, (underlying_0.clone(), 25_0000005)],
                lot: map![&e, (underlying_1.clone(), 25_0000005)],
                block: 1000,
            };

            // 0 blocks
            e.ledger().set(LedgerInfo {
                timestamp: 12345,
                protocol_version: 22,
                sequence_number: 1000,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 172800,
                min_persistent_entry_ttl: 172800,
                max_entry_ttl: 9999999,
            });

            let (_, _) = scale_auction(&e, &base_auction_data, 101);
        }

        #[test]
        fn test_scale_auction_dust() {
            // @dev: bids always round up, lots always round down
            //       the remaining is exact based on scaled auction
            let e = Env::default();
            let underlying_0 = Address::generate(&e);
            let underlying_1 = Address::generate(&e);

            let base_auction_data = AuctionData {
                bid: map![&e, (underlying_0.clone(), 0_0000001)],
                lot: map![&e, (underlying_1.clone(), 0_0000001)],
                block: 1000,
            };

            // 0 blocks
            e.ledger().set(LedgerInfo {
                timestamp: 12345,
                protocol_version: 22,
                sequence_number: 1000,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 172800,
                min_persistent_entry_ttl: 172800,
                max_entry_ttl: 9999999,
            });
            let (scaled_auction, remaining_auction_option) =
                scale_auction(&e, &base_auction_data, 99);
            assert_eq!(scaled_auction.bid.get_unchecked(underlying_0.clone()), 1);
            assert_eq!(scaled_auction.lot.len(), 0);
            let remaining_auction = remaining_auction_option.unwrap();
            assert_eq!(remaining_auction.bid.len(), 0);
            assert_eq!(remaining_auction.lot.get_unchecked(underlying_1.clone()), 1);

            // 100 blocks
            e.ledger().set(LedgerInfo {
                timestamp: 12345,
                protocol_version: 22,
                sequence_number: 1100,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 172800,
                min_persistent_entry_ttl: 172800,
                max_entry_ttl: 9999999,
            });

            let (scaled_auction, remaining_auction_option) =
                scale_auction(&e, &base_auction_data, 100);
            assert_eq!(scaled_auction.bid.get_unchecked(underlying_0.clone()), 1);
            assert_eq!(scaled_auction.lot.len(), 0);
            assert!(remaining_auction_option.is_none());

            let (scaled_auction, remaining_auction_option) =
                scale_auction(&e, &base_auction_data, 99);
            assert_eq!(scaled_auction.bid.get_unchecked(underlying_0.clone()), 1);
            assert_eq!(scaled_auction.lot.len(), 0);
            let remaining_auction = remaining_auction_option.unwrap();
            assert_eq!(remaining_auction.bid.len(), 0);
            assert_eq!(remaining_auction.lot.get_unchecked(underlying_1.clone()), 1);

            // 200 blocks
            e.ledger().set(LedgerInfo {
                timestamp: 12345,
                protocol_version: 22,
                sequence_number: 1200,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 172800,
                min_persistent_entry_ttl: 172800,
                max_entry_ttl: 9999999,
            });

            let (scaled_auction, remaining_auction_option) =
                scale_auction(&e, &base_auction_data, 99);
            assert_eq!(scaled_auction.bid.get_unchecked(underlying_0.clone()), 1);
            assert_eq!(scaled_auction.lot.len(), 0);
            let remaining_auction = remaining_auction_option.unwrap();
            assert_eq!(remaining_auction.bid.len(), 0);
            assert_eq!(remaining_auction.lot.get_unchecked(underlying_1.clone()), 1);

            // 300 blocks
            e.ledger().set(LedgerInfo {
                timestamp: 12345,
                protocol_version: 22,
                sequence_number: 1300,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 172800,
                min_persistent_entry_ttl: 172800,
                max_entry_ttl: 9999999,
            });

            let (scaled_auction, remaining_auction_option) =
                scale_auction(&e, &base_auction_data, 100);
            assert_eq!(scaled_auction.bid.get_unchecked(underlying_0.clone()), 1);
            assert_eq!(scaled_auction.lot.get_unchecked(underlying_1.clone()), 1);
            assert!(remaining_auction_option.is_none());

            let (scaled_auction, remaining_auction_option) =
                scale_auction(&e, &base_auction_data, 99);
            assert_eq!(scaled_auction.bid.get_unchecked(underlying_0.clone()), 1);
            assert_eq!(scaled_auction.lot.len(), 0);
            let remaining_auction = remaining_auction_option.unwrap();
            assert_eq!(remaining_auction.bid.len(), 0);
            assert_eq!(remaining_auction.lot.get_unchecked(underlying_1.clone()), 1);

            // 399 blocks
            e.ledger().set(LedgerInfo {
                timestamp: 12345,
                protocol_version: 22,
                sequence_number: 1399,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 172800,
                min_persistent_entry_ttl: 172800,
                max_entry_ttl: 9999999,
            });
            let (scaled_auction, remaining_auction_option) =
                scale_auction(&e, &base_auction_data, 99);
            assert_eq!(scaled_auction.bid.get_unchecked(underlying_0.clone()), 1);
            assert_eq!(scaled_auction.lot.len(), 0);
            let remaining_auction = remaining_auction_option.unwrap();
            assert_eq!(remaining_auction.bid.len(), 0);
            assert_eq!(remaining_auction.lot.get_unchecked(underlying_1.clone()), 1);

            // 400 blocks
            e.ledger().set(LedgerInfo {
                timestamp: 12345,
                protocol_version: 22,
                sequence_number: 1400,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 172800,
                min_persistent_entry_ttl: 172800,
                max_entry_ttl: 9999999,
            });
            let (scaled_auction, remaining_auction_option) =
                scale_auction(&e, &base_auction_data, 99);
            assert_eq!(scaled_auction.bid.len(), 0);
            assert_eq!(scaled_auction.lot.len(), 0);
            let remaining_auction = remaining_auction_option.unwrap();
            assert_eq!(remaining_auction.bid.len(), 0);
            assert_eq!(remaining_auction.lot.get_unchecked(underlying_1.clone()), 1);

            // with 100 fill pct
            let (scaled_auction, remaining_auction_option) =
                scale_auction(&e, &base_auction_data, 100);
            assert_eq!(scaled_auction.bid.len(), 0);
            assert_eq!(scaled_auction.lot.get_unchecked(underlying_1.clone()), 1);
            assert!(remaining_auction_option.is_none());
        }
    }
}

mod pool_src_auctions_backstop_interest_auction {
    use crate::{
        constants::SCALAR_7, dependencies::BackstopClient, errors::PoolError, pool::Pool, storage,
    };

    use cast::i128;

    use sep_41_token::TokenClient;

    use soroban_fixed_point_math::SorobanFixedPoint;

    use soroban_sdk::{map, panic_with_error, Address, Env, Vec};

    use crate::auctions::{AuctionData, AuctionType};

    pub(crate) use crate::auctions::backstop_interest_auction::*;

    mod tests {
        use crate::{
            auctions::auction::AuctionType,
            storage::{self, PoolConfig},
            testutils::{self, create_comet_lp_pool, create_pool},
        };

        use super::*;
        use sep_40_oracle::testutils::Asset;
        use soroban_sdk::{
            testutils::{Address as _, Ledger, LedgerInfo},
            vec, Address, Symbol,
        };

        #[test]
        #[should_panic(expected = "Error(Contract, #1212)")]
        fn test_create_interest_auction_already_in_progress() {
            let e = Env::default();
            e.mock_all_auths();
            let pool_address = create_pool(&e);
            let backstop_address = Address::generate(&e);

            e.ledger().set(LedgerInfo {
                timestamp: 12345,
                protocol_version: 22,
                sequence_number: 100,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 3110400,
            });

            let auction_data = AuctionData {
                bid: map![&e],
                lot: map![&e],
                block: 50,
            };
            e.as_contract(&pool_address, || {
                storage::set_backstop(&e, &backstop_address);
                storage::set_auction(
                    &e,
                    &(AuctionType::InterestAuction as u32),
                    &backstop_address,
                    &auction_data,
                );

                create_interest_auction_data(&e, &backstop_address, &vec![&e], &vec![&e], 100);
            });
        }

        #[test]
        #[should_panic]
        fn test_create_interest_auction_no_reserve() {
            let e = Env::default();
            e.mock_all_auths();
            let pool_address = create_pool(&e);
            let backstop_address = Address::generate(&e);

            e.ledger().set(LedgerInfo {
                timestamp: 12345,
                protocol_version: 22,
                sequence_number: 100,
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
                status: 0,
                max_positions: 4,
            };
            e.as_contract(&pool_address, || {
                storage::set_pool_config(&e, &pool_config);

                create_interest_auction_data(
                    &e,
                    &backstop_address,
                    &vec![&e, Address::generate(&e)],
                    &vec![&e, Address::generate(&e)],
                    100,
                );
            });
        }

        #[test]
        #[should_panic(expected = "Error(Contract, #1200)")]
        fn test_create_interest_auction_user_not_backstop() {
            let e = Env::default();
            e.mock_all_auths();
            e.cost_estimate().budget().reset_unlimited(); // setup exhausts budget

            e.ledger().set(LedgerInfo {
                timestamp: 12345,
                protocol_version: 22,
                sequence_number: 50,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 3110400,
            });

            let pool_address = create_pool(&e);
            let backstop_address = Address::generate(&e);
            let backstop_token_id = Address::generate(&e);

            let pool_config = PoolConfig {
                oracle: Address::generate(&e),
                min_collateral: 1_0000000,
                bstop_rate: 0_1000000,
                status: 0,
                max_positions: 4,
            };
            e.as_contract(&pool_address, || {
                storage::set_pool_config(&e, &pool_config);
                storage::set_backstop(&e, &backstop_address);

                create_interest_auction_data(
                    &e,
                    &Address::generate(&e),
                    &vec![&e, backstop_token_id.clone()],
                    &vec![&e, backstop_token_id.clone()],
                    100,
                );
            });
        }

        #[test]
        #[should_panic(expected = "Error(Contract, #1200)")]
        fn test_create_interest_auction_percent_not_100() {
            let e = Env::default();
            e.mock_all_auths();
            e.cost_estimate().budget().reset_unlimited(); // setup exhausts budget

            e.ledger().set(LedgerInfo {
                timestamp: 12345,
                protocol_version: 22,
                sequence_number: 50,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 3110400,
            });

            let pool_address = create_pool(&e);
            let backstop_address = Address::generate(&e);
            let backstop_token_id = Address::generate(&e);

            let pool_config = PoolConfig {
                oracle: Address::generate(&e),
                min_collateral: 1_0000000,
                bstop_rate: 0_1000000,
                status: 0,
                max_positions: 4,
            };
            e.as_contract(&pool_address, || {
                storage::set_pool_config(&e, &pool_config);
                storage::set_backstop(&e, &backstop_address);
                create_interest_auction_data(
                    &e,
                    &backstop_address,
                    &vec![&e, backstop_token_id.clone()],
                    &vec![&e, backstop_token_id.clone()],
                    99,
                );
            });
        }

        #[test]
        #[should_panic(expected = "Error(Contract, #1215)")]
        fn test_create_interest_auction_under_threshold() {
            let e = Env::default();
            e.mock_all_auths();
            e.cost_estimate().budget().reset_unlimited(); // setup exhausts budget

            e.ledger().set(LedgerInfo {
                timestamp: 12345,
                protocol_version: 22,
                sequence_number: 50,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 3110400,
            });

            let bombadil = Address::generate(&e);

            let pool_address = create_pool(&e);
            let (usdc_id, _) = testutils::create_token_contract(&e, &bombadil);
            let backstop_token = Address::generate(&e);
            let (backstop_address, _) = testutils::create_backstop(
                &e,
                &pool_address,
                &backstop_token,
                &usdc_id,
                &Address::generate(&e),
            );
            let (oracle_id, oracle_client) = testutils::create_mock_oracle(&e);

            let (underlying_0, _) = testutils::create_token_contract(&e, &bombadil);
            let (mut reserve_config_0, mut reserve_data_0) = testutils::default_reserve_meta();
            reserve_data_0.b_rate = 1_100_000_000_000;
            reserve_data_0.last_time = 12345;
            reserve_data_0.backstop_credit = 10_0000000;
            reserve_config_0.index = 0;
            testutils::create_reserve(
                &e,
                &pool_address,
                &underlying_0,
                &reserve_config_0,
                &reserve_data_0,
            );

            let (underlying_1, _) = testutils::create_token_contract(&e, &bombadil);
            let (mut reserve_config_1, mut reserve_data_1) = testutils::default_reserve_meta();
            reserve_data_1.b_rate = 1_100_000_000_000;
            reserve_data_1.last_time = 12345;
            reserve_data_1.backstop_credit = 2_5000000;
            reserve_config_1.index = 1;
            testutils::create_reserve(
                &e,
                &pool_address,
                &underlying_1,
                &reserve_config_1,
                &reserve_data_1,
            );

            let (underlying_2, _) = testutils::create_token_contract(&e, &bombadil);
            let (mut reserve_config_2, mut reserve_data_2) = testutils::default_reserve_meta();
            reserve_data_2.b_rate = 1_100_000_000_000;
            reserve_data_2.last_time = 12345;
            reserve_config_2.index = 1;
            testutils::create_reserve(
                &e,
                &pool_address,
                &underlying_2,
                &reserve_config_2,
                &reserve_data_2,
            );

            oracle_client.set_data(
                &bombadil,
                &Asset::Other(Symbol::new(&e, "USD")),
                &vec![
                    &e,
                    Asset::Stellar(underlying_0.clone()),
                    Asset::Stellar(underlying_1.clone()),
                    Asset::Stellar(underlying_2.clone()),
                    Asset::Stellar(usdc_id.clone()),
                ],
                &7,
                &300,
            );
            oracle_client.set_price_stable(&vec![&e, 2_0000000, 4_0000000, 100_0000000, 1_0000000]);

            let pool_config = PoolConfig {
                oracle: oracle_id,
                min_collateral: 1_0000000,
                bstop_rate: 0_1000000,
                status: 0,
                max_positions: 4,
            };
            e.as_contract(&pool_address, || {
                storage::set_pool_config(&e, &pool_config);

                create_interest_auction_data(
                    &e,
                    &backstop_address,
                    &vec![&e, backstop_token.clone()],
                    &vec![
                        &e,
                        underlying_0.clone(),
                        underlying_1.clone(),
                        underlying_2.clone(),
                    ],
                    100,
                );
            });
        }

        #[test]
        #[should_panic(expected = "Error(Contract, #1221)")]
        fn test_create_interest_auction_invalid_bid() {
            let e = Env::default();
            e.mock_all_auths();
            e.cost_estimate().budget().reset_unlimited(); // setup exhausts budget

            e.ledger().set(LedgerInfo {
                timestamp: 12345,
                protocol_version: 22,
                sequence_number: 50,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 3110400,
            });

            let bombadil = Address::generate(&e);

            let pool_address = create_pool(&e);
            let (usdc_id, _) = testutils::create_token_contract(&e, &bombadil);
            let (blnd_id, _) = testutils::create_blnd_token(&e, &pool_address, &bombadil);

            let (backstop_token_id, _) = create_comet_lp_pool(&e, &bombadil, &blnd_id, &usdc_id);
            let (backstop_address, backstop_client) = testutils::create_backstop(
                &e,
                &pool_address,
                &backstop_token_id,
                &usdc_id,
                &blnd_id,
            );
            backstop_client.deposit(&bombadil, &pool_address, &(50 * SCALAR_7));
            let (oracle_id, oracle_client) = testutils::create_mock_oracle(&e);

            let (underlying_0, _) = testutils::create_token_contract(&e, &bombadil);
            let (mut reserve_config_0, mut reserve_data_0) = testutils::default_reserve_meta();
            reserve_data_0.last_time = 12345;
            reserve_data_0.backstop_credit = 200_0000000;
            reserve_data_0.b_supply = 1000_0000000;
            reserve_data_0.d_supply = 750_0000000;
            reserve_config_0.index = 0;
            testutils::create_reserve(
                &e,
                &pool_address,
                &underlying_0,
                &reserve_config_0,
                &reserve_data_0,
            );

            oracle_client.set_data(
                &bombadil,
                &Asset::Other(Symbol::new(&e, "USD")),
                &vec![
                    &e,
                    Asset::Stellar(underlying_0.clone()),
                    Asset::Stellar(usdc_id.clone()),
                ],
                &7,
                &300,
            );
            oracle_client.set_price_stable(&vec![&e, 2_0000000, 4_0000000]);

            let pool_config = PoolConfig {
                oracle: oracle_id,
                min_collateral: 1_0000000,
                bstop_rate: 0_1000000,
                status: 0,
                max_positions: 4,
            };
            e.as_contract(&pool_address, || {
                storage::set_pool_config(&e, &pool_config);

                create_interest_auction_data(
                    &e,
                    &backstop_address,
                    &vec![&e, underlying_0.clone()],
                    &vec![&e, underlying_0.clone()],
                    100,
                );
            });
        }

        #[test]
        #[should_panic]
        fn test_create_interest_auction_invalid_lot() {
            let e = Env::default();
            e.mock_all_auths();
            e.cost_estimate().budget().reset_unlimited(); // setup exhausts budget

            e.ledger().set(LedgerInfo {
                timestamp: 12345,
                protocol_version: 22,
                sequence_number: 50,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 3110400,
            });

            let bombadil = Address::generate(&e);

            let pool_address = create_pool(&e);
            let (usdc_id, _) = testutils::create_token_contract(&e, &bombadil);
            let (blnd_id, _) = testutils::create_blnd_token(&e, &pool_address, &bombadil);

            let (backstop_token_id, _) = create_comet_lp_pool(&e, &bombadil, &blnd_id, &usdc_id);
            let (backstop_address, backstop_client) = testutils::create_backstop(
                &e,
                &pool_address,
                &backstop_token_id,
                &usdc_id,
                &blnd_id,
            );
            backstop_client.deposit(&bombadil, &pool_address, &(50 * SCALAR_7));
            let (oracle_id, oracle_client) = testutils::create_mock_oracle(&e);

            let (underlying_0, _) = testutils::create_token_contract(&e, &bombadil);
            let (mut reserve_config_0, mut reserve_data_0) = testutils::default_reserve_meta();
            reserve_data_0.last_time = 12345;
            reserve_data_0.backstop_credit = 200_0000000;
            reserve_data_0.b_supply = 1000_0000000;
            reserve_data_0.d_supply = 750_0000000;
            reserve_config_0.index = 0;
            testutils::create_reserve(
                &e,
                &pool_address,
                &underlying_0,
                &reserve_config_0,
                &reserve_data_0,
            );

            oracle_client.set_data(
                &bombadil,
                &Asset::Other(Symbol::new(&e, "USD")),
                &vec![
                    &e,
                    Asset::Stellar(underlying_0.clone()),
                    Asset::Stellar(usdc_id.clone()),
                ],
                &7,
                &300,
            );
            oracle_client.set_price_stable(&vec![&e, 2_0000000, 4_0000000]);

            let pool_config = PoolConfig {
                oracle: oracle_id,
                min_collateral: 1_0000000,
                bstop_rate: 0_1000000,
                status: 0,
                max_positions: 4,
            };
            e.as_contract(&pool_address, || {
                storage::set_pool_config(&e, &pool_config);

                create_interest_auction_data(
                    &e,
                    &backstop_address,
                    &vec![&e, backstop_token_id.clone()],
                    &vec![&e, backstop_token_id.clone()],
                    100,
                );
            });
        }

        #[test]
        #[should_panic(expected = "Error(Contract, #1222)")]
        fn test_create_interest_auction_invalid_lot_empty() {
            let e = Env::default();
            e.mock_all_auths();
            e.cost_estimate().budget().reset_unlimited(); // setup exhausts budget

            e.ledger().set(LedgerInfo {
                timestamp: 12345,
                protocol_version: 22,
                sequence_number: 50,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 3110400,
            });

            let bombadil = Address::generate(&e);

            let pool_address = create_pool(&e);
            let (usdc_id, _) = testutils::create_token_contract(&e, &bombadil);
            let (blnd_id, _) = testutils::create_blnd_token(&e, &pool_address, &bombadil);

            let (backstop_token_id, _) = create_comet_lp_pool(&e, &bombadil, &blnd_id, &usdc_id);
            let (backstop_address, backstop_client) = testutils::create_backstop(
                &e,
                &pool_address,
                &backstop_token_id,
                &usdc_id,
                &blnd_id,
            );
            backstop_client.deposit(&bombadil, &pool_address, &(50 * SCALAR_7));
            let (oracle_id, oracle_client) = testutils::create_mock_oracle(&e);

            let (underlying_0, _) = testutils::create_token_contract(&e, &bombadil);
            let (mut reserve_config_0, mut reserve_data_0) = testutils::default_reserve_meta();
            reserve_data_0.last_time = 12345;
            reserve_data_0.backstop_credit = 200_0000000;
            reserve_data_0.b_supply = 1000_0000000;
            reserve_data_0.d_supply = 750_0000000;
            reserve_config_0.index = 0;
            testutils::create_reserve(
                &e,
                &pool_address,
                &underlying_0,
                &reserve_config_0,
                &reserve_data_0,
            );

            oracle_client.set_data(
                &bombadil,
                &Asset::Other(Symbol::new(&e, "USD")),
                &vec![
                    &e,
                    Asset::Stellar(underlying_0.clone()),
                    Asset::Stellar(usdc_id.clone()),
                ],
                &7,
                &300,
            );
            oracle_client.set_price_stable(&vec![&e, 2_0000000, 4_0000000]);

            let pool_config = PoolConfig {
                oracle: oracle_id,
                min_collateral: 1_0000000,
                bstop_rate: 0_1000000,
                status: 0,
                max_positions: 4,
            };
            e.as_contract(&pool_address, || {
                storage::set_pool_config(&e, &pool_config);

                create_interest_auction_data(
                    &e,
                    &backstop_address,
                    &vec![&e, backstop_token_id.clone()],
                    &vec![&e],
                    100,
                );
            });
        }

        #[test]
        #[should_panic(expected = "Error(Contract, #1208)")]
        fn test_create_interest_auction_checks_max_positions() {
            let e = Env::default();
            e.mock_all_auths();
            e.cost_estimate().budget().reset_unlimited(); // setup exhausts budget

            e.ledger().set(LedgerInfo {
                timestamp: 12345,
                protocol_version: 22,
                sequence_number: 50,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 3110400,
            });

            let bombadil = Address::generate(&e);

            let pool_address = create_pool(&e);
            let (usdc_id, _) = testutils::create_token_contract(&e, &bombadil);
            let (blnd_id, _) = testutils::create_blnd_token(&e, &pool_address, &bombadil);

            let (backstop_token_id, _) = create_comet_lp_pool(&e, &bombadil, &blnd_id, &usdc_id);
            let (backstop_address, backstop_client) = testutils::create_backstop(
                &e,
                &pool_address,
                &backstop_token_id,
                &usdc_id,
                &blnd_id,
            );
            backstop_client.deposit(&bombadil, &pool_address, &(50 * SCALAR_7));
            let (oracle_id, oracle_client) = testutils::create_mock_oracle(&e);

            let (underlying_0, _) = testutils::create_token_contract(&e, &bombadil);
            let (mut reserve_config_0, mut reserve_data_0) = testutils::default_reserve_meta();
            reserve_data_0.last_time = 12345;
            reserve_data_0.backstop_credit = 100_0000000;
            reserve_data_0.b_supply = 1000_0000000;
            reserve_data_0.d_supply = 750_0000000;
            reserve_config_0.index = 0;
            testutils::create_reserve(
                &e,
                &pool_address,
                &underlying_0,
                &reserve_config_0,
                &reserve_data_0,
            );

            let (underlying_1, _) = testutils::create_token_contract(&e, &bombadil);
            let (mut reserve_config_1, mut reserve_data_1) = testutils::default_reserve_meta();
            reserve_data_1.last_time = 12345;
            reserve_data_1.backstop_credit = 25_0000000;
            reserve_data_1.b_supply = 250_0000000;
            reserve_data_1.d_supply = 187_5000000;
            reserve_config_1.index = 1;
            testutils::create_reserve(
                &e,
                &pool_address,
                &underlying_1,
                &reserve_config_1,
                &reserve_data_1,
            );

            let (underlying_2, _) = testutils::create_token_contract(&e, &bombadil);
            let (mut reserve_config_2, mut reserve_data_2) = testutils::default_reserve_meta();
            reserve_data_2.last_time = 12345;
            reserve_data_1.backstop_credit = 1000;
            reserve_config_2.index = 1;
            testutils::create_reserve(
                &e,
                &pool_address,
                &underlying_2,
                &reserve_config_2,
                &reserve_data_2,
            );

            oracle_client.set_data(
                &bombadil,
                &Asset::Other(Symbol::new(&e, "USD")),
                &vec![
                    &e,
                    Asset::Stellar(underlying_0.clone()),
                    Asset::Stellar(underlying_1.clone()),
                    Asset::Stellar(underlying_2.clone()),
                    Asset::Stellar(usdc_id.clone()),
                ],
                &7,
                &300,
            );
            oracle_client.set_price_stable(&vec![&e, 2_0000000, 4_0000000, 100_0000000, 1_0000000]);

            let pool_config = PoolConfig {
                oracle: oracle_id,
                min_collateral: 1_0000000,
                bstop_rate: 0_1000000,
                status: 0,
                max_positions: 3,
            };
            e.as_contract(&pool_address, || {
                storage::set_pool_config(&e, &pool_config);

                create_interest_auction_data(
                    &e,
                    &backstop_address,
                    &vec![&e, backstop_token_id.clone()],
                    &vec![
                        &e,
                        underlying_0.clone(),
                        underlying_1.clone(),
                        underlying_2.clone(),
                    ],
                    100,
                );
            });
        }

        #[test]
        fn test_create_interest_auction() {
            let e = Env::default();
            e.mock_all_auths();
            e.cost_estimate().budget().reset_unlimited(); // setup exhausts budget

            e.ledger().set(LedgerInfo {
                timestamp: 12345,
                protocol_version: 22,
                sequence_number: 50,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 3110400,
            });

            let bombadil = Address::generate(&e);

            let pool_address = create_pool(&e);
            let (usdc_id, _) = testutils::create_token_contract(&e, &bombadil);
            let (blnd_id, _) = testutils::create_blnd_token(&e, &pool_address, &bombadil);

            let (backstop_token_id, _) = create_comet_lp_pool(&e, &bombadil, &blnd_id, &usdc_id);
            let (backstop_address, backstop_client) = testutils::create_backstop(
                &e,
                &pool_address,
                &backstop_token_id,
                &usdc_id,
                &blnd_id,
            );
            backstop_client.deposit(&bombadil, &pool_address, &(50 * SCALAR_7));
            let (oracle_id, oracle_client) = testutils::create_mock_oracle(&e);

            let (underlying_0, _) = testutils::create_token_contract(&e, &bombadil);
            let (mut reserve_config_0, mut reserve_data_0) = testutils::default_reserve_meta();
            reserve_data_0.last_time = 12345;
            reserve_data_0.backstop_credit = 100_0000000;
            reserve_data_0.b_supply = 1000_0000000;
            reserve_data_0.d_supply = 750_0000000;
            reserve_config_0.index = 0;
            testutils::create_reserve(
                &e,
                &pool_address,
                &underlying_0,
                &reserve_config_0,
                &reserve_data_0,
            );

            let (underlying_1, _) = testutils::create_token_contract(&e, &bombadil);
            let (mut reserve_config_1, mut reserve_data_1) = testutils::default_reserve_meta();
            reserve_data_1.last_time = 12345;
            reserve_data_1.backstop_credit = 25_0000000;
            reserve_data_1.b_supply = 250_0000000;
            reserve_data_1.d_supply = 187_5000000;
            reserve_config_1.index = 1;
            testutils::create_reserve(
                &e,
                &pool_address,
                &underlying_1,
                &reserve_config_1,
                &reserve_data_1,
            );

            let (underlying_2, _) = testutils::create_token_contract(&e, &bombadil);
            let (mut reserve_config_2, mut reserve_data_2) = testutils::default_reserve_meta();
            reserve_data_2.last_time = 12345;
            reserve_config_2.index = 1;
            testutils::create_reserve(
                &e,
                &pool_address,
                &underlying_2,
                &reserve_config_2,
                &reserve_data_2,
            );

            oracle_client.set_data(
                &bombadil,
                &Asset::Other(Symbol::new(&e, "USD")),
                &vec![
                    &e,
                    Asset::Stellar(underlying_0.clone()),
                    Asset::Stellar(underlying_1.clone()),
                    Asset::Stellar(underlying_2),
                    Asset::Stellar(usdc_id.clone()),
                ],
                &7,
                &300,
            );
            oracle_client.set_price_stable(&vec![&e, 2_0000000, 4_0000000, 100_0000000, 1_0000000]);

            let pool_config = PoolConfig {
                oracle: oracle_id,
                min_collateral: 1_0000000,
                bstop_rate: 0_1000000,
                status: 0,
                max_positions: 4,
            };
            e.as_contract(&pool_address, || {
                storage::set_pool_config(&e, &pool_config);

                let result = create_interest_auction_data(
                    &e,
                    &backstop_address,
                    &vec![&e, backstop_token_id.clone()],
                    &vec![&e, underlying_0.clone(), underlying_1.clone()],
                    100,
                );
                assert_eq!(result.block, 51);
                assert_eq!(result.bid.get_unchecked(backstop_token_id), 288_0000000);
                assert_eq!(result.bid.len(), 1);
                assert_eq!(result.lot.get_unchecked(underlying_0), 100_0000000);
                assert_eq!(result.lot.get_unchecked(underlying_1), 25_0000000);
                assert_eq!(result.lot.len(), 2);
            });
        }

        #[test]
        fn test_create_interest_auction_14_decimal_oracle() {
            let e = Env::default();
            e.mock_all_auths();
            e.cost_estimate().budget().reset_unlimited(); // setup exhausts budget

            e.ledger().set(LedgerInfo {
                timestamp: 12345,
                protocol_version: 22,
                sequence_number: 50,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 3110400,
            });

            let bombadil = Address::generate(&e);

            let pool_address = create_pool(&e);
            let (usdc_id, _) = testutils::create_token_contract(&e, &bombadil);
            let (blnd_id, _) = testutils::create_blnd_token(&e, &pool_address, &bombadil);

            let (backstop_token_id, _) = create_comet_lp_pool(&e, &bombadil, &blnd_id, &usdc_id);
            let (backstop_address, backstop_client) = testutils::create_backstop(
                &e,
                &pool_address,
                &backstop_token_id,
                &usdc_id,
                &blnd_id,
            );
            backstop_client.deposit(&bombadil, &pool_address, &(50 * SCALAR_7));
            let (oracle_id, oracle_client) = testutils::create_mock_oracle(&e);

            let (underlying_0, _) = testutils::create_token_contract(&e, &bombadil);
            let (mut reserve_config_0, mut reserve_data_0) = testutils::default_reserve_meta();
            reserve_data_0.last_time = 12345;
            reserve_data_0.backstop_credit = 100_0000000;
            reserve_data_0.b_supply = 1000_0000000;
            reserve_data_0.d_supply = 750_0000000;
            reserve_config_0.index = 0;
            testutils::create_reserve(
                &e,
                &pool_address,
                &underlying_0,
                &reserve_config_0,
                &reserve_data_0,
            );

            let (underlying_1, _) = testutils::create_token_contract(&e, &bombadil);
            let (mut reserve_config_1, mut reserve_data_1) = testutils::default_reserve_meta();
            reserve_data_1.last_time = 12345;
            reserve_data_1.backstop_credit = 25_0000000;
            reserve_data_1.b_supply = 250_0000000;
            reserve_data_1.d_supply = 187_5000000;
            reserve_config_1.index = 1;
            testutils::create_reserve(
                &e,
                &pool_address,
                &underlying_1,
                &reserve_config_1,
                &reserve_data_1,
            );

            let (underlying_2, _) = testutils::create_token_contract(&e, &bombadil);
            let (mut reserve_config_2, mut reserve_data_2) = testutils::default_reserve_meta();
            reserve_data_2.last_time = 12345;
            reserve_config_2.index = 1;
            testutils::create_reserve(
                &e,
                &pool_address,
                &underlying_2,
                &reserve_config_2,
                &reserve_data_2,
            );

            oracle_client.set_data(
                &bombadil,
                &Asset::Other(Symbol::new(&e, "USD")),
                &vec![
                    &e,
                    Asset::Stellar(underlying_0.clone()),
                    Asset::Stellar(underlying_1.clone()),
                    Asset::Stellar(underlying_2),
                    Asset::Stellar(usdc_id.clone()),
                ],
                &14,
                &300,
            );
            oracle_client.set_price_stable(&vec![
                &e,
                2_0000000_0000000,
                4_0000000_0000000,
                100_0000000_0000000,
                1_0000000_0000000,
            ]);

            let pool_config = PoolConfig {
                oracle: oracle_id,
                min_collateral: 1_0000000,
                bstop_rate: 0_1000000,
                status: 0,
                max_positions: 4,
            };
            e.as_contract(&pool_address, || {
                storage::set_pool_config(&e, &pool_config);

                let result = create_interest_auction_data(
                    &e,
                    &backstop_address,
                    &vec![&e, backstop_token_id.clone()],
                    &vec![&e, underlying_0.clone(), underlying_1.clone()],
                    100,
                );
                assert_eq!(result.block, 51);
                assert_eq!(result.bid.get_unchecked(backstop_token_id), 288_0000000);
                assert_eq!(result.bid.len(), 1);
                assert_eq!(result.lot.get_unchecked(underlying_0), 100_0000000);
                assert_eq!(result.lot.get_unchecked(underlying_1), 25_0000000);
                assert_eq!(result.lot.len(), 2);
            });
        }

        #[test]
        fn test_create_interest_auction_2_decimal_oracle() {
            let e = Env::default();
            e.mock_all_auths();
            e.cost_estimate().budget().reset_unlimited(); // setup exhausts budget

            e.ledger().set(LedgerInfo {
                timestamp: 12345,
                protocol_version: 22,
                sequence_number: 50,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 3110400,
            });

            let bombadil = Address::generate(&e);

            let pool_address = create_pool(&e);
            let (usdc_id, _) = testutils::create_token_contract(&e, &bombadil);
            let (blnd_id, _) = testutils::create_blnd_token(&e, &pool_address, &bombadil);

            let (backstop_token_id, _) = create_comet_lp_pool(&e, &bombadil, &blnd_id, &usdc_id);
            let (backstop_address, backstop_client) = testutils::create_backstop(
                &e,
                &pool_address,
                &backstop_token_id,
                &usdc_id,
                &blnd_id,
            );
            backstop_client.deposit(&bombadil, &pool_address, &(50 * SCALAR_7));
            let (oracle_id, oracle_client) = testutils::create_mock_oracle(&e);

            let (underlying_0, _) = testutils::create_token_contract(&e, &bombadil);
            let (mut reserve_config_0, mut reserve_data_0) = testutils::default_reserve_meta();
            reserve_data_0.last_time = 12345;
            reserve_data_0.backstop_credit = 100_0000000;
            reserve_data_0.b_supply = 1000_0000000;
            reserve_data_0.d_supply = 750_0000000;
            reserve_config_0.index = 0;
            testutils::create_reserve(
                &e,
                &pool_address,
                &underlying_0,
                &reserve_config_0,
                &reserve_data_0,
            );

            let (underlying_1, _) = testutils::create_token_contract(&e, &bombadil);
            let (mut reserve_config_1, mut reserve_data_1) = testutils::default_reserve_meta();
            reserve_data_1.last_time = 12345;
            reserve_data_1.backstop_credit = 25_0000000;
            reserve_data_1.b_supply = 250_0000000;
            reserve_data_1.d_supply = 187_5000000;
            reserve_config_1.index = 1;
            testutils::create_reserve(
                &e,
                &pool_address,
                &underlying_1,
                &reserve_config_1,
                &reserve_data_1,
            );

            let (underlying_2, _) = testutils::create_token_contract(&e, &bombadil);
            let (mut reserve_config_2, mut reserve_data_2) = testutils::default_reserve_meta();
            reserve_data_2.last_time = 12345;
            reserve_config_2.index = 1;
            testutils::create_reserve(
                &e,
                &pool_address,
                &underlying_2,
                &reserve_config_2,
                &reserve_data_2,
            );

            oracle_client.set_data(
                &bombadil,
                &Asset::Other(Symbol::new(&e, "USD")),
                &vec![
                    &e,
                    Asset::Stellar(underlying_0.clone()),
                    Asset::Stellar(underlying_1.clone()),
                    Asset::Stellar(underlying_2),
                    Asset::Stellar(usdc_id.clone()),
                ],
                &2,
                &300,
            );
            oracle_client.set_price_stable(&vec![&e, 2_00, 4_00, 100_00, 1_00]);

            let pool_config = PoolConfig {
                oracle: oracle_id,
                min_collateral: 1_0000000,
                bstop_rate: 0_1000000,
                status: 0,
                max_positions: 4,
            };
            e.as_contract(&pool_address, || {
                storage::set_pool_config(&e, &pool_config);

                let result = create_interest_auction_data(
                    &e,
                    &backstop_address,
                    &vec![&e, backstop_token_id.clone()],
                    &vec![&e, underlying_0.clone(), underlying_1.clone()],
                    100,
                );
                assert_eq!(result.block, 51);
                assert_eq!(result.bid.get_unchecked(backstop_token_id), 288_0000000);
                assert_eq!(result.bid.len(), 1);
                assert_eq!(result.lot.get_unchecked(underlying_0), 100_0000000);
                assert_eq!(result.lot.get_unchecked(underlying_1), 25_0000000);
                assert_eq!(result.lot.len(), 2);
            });
        }

        #[test]
        fn test_create_interest_auction_applies_interest() {
            let e = Env::default();
            e.mock_all_auths();
            e.cost_estimate().budget().reset_unlimited(); // setup exhausts budget

            e.ledger().set(LedgerInfo {
                timestamp: 12345,
                protocol_version: 22,
                sequence_number: 150,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 3110400,
            });

            let bombadil = Address::generate(&e);

            let pool_address = create_pool(&e);
            let (usdc_id, _) = testutils::create_token_contract(&e, &bombadil);
            let (blnd_id, _) = testutils::create_blnd_token(&e, &pool_address, &bombadil);

            let (backstop_token_id, _) = create_comet_lp_pool(&e, &bombadil, &blnd_id, &usdc_id);
            let (backstop_address, backstop_client) = testutils::create_backstop(
                &e,
                &pool_address,
                &backstop_token_id,
                &usdc_id,
                &blnd_id,
            );
            backstop_client.deposit(&bombadil, &pool_address, &(50 * SCALAR_7));

            let (oracle_id, oracle_client) = testutils::create_mock_oracle(&e);

            let (underlying_0, _) = testutils::create_token_contract(&e, &bombadil);
            let (mut reserve_config_0, mut reserve_data_0) = testutils::default_reserve_meta();
            reserve_data_0.last_time = 11845;
            reserve_data_0.backstop_credit = 100_0000000;
            reserve_data_0.b_supply = 1000_0000000;
            reserve_data_0.d_supply = 750_0000000;
            reserve_config_0.index = 0;
            testutils::create_reserve(
                &e,
                &pool_address,
                &underlying_0,
                &reserve_config_0,
                &reserve_data_0,
            );

            let (underlying_1, _) = testutils::create_token_contract(&e, &bombadil);
            let (mut reserve_config_1, mut reserve_data_1) = testutils::default_reserve_meta();
            reserve_data_1.last_time = 11845;
            reserve_data_1.backstop_credit = 25_0000000;
            reserve_data_1.b_supply = 250_0000000;
            reserve_data_1.d_supply = 187_5000000;
            reserve_config_1.index = 1;
            testutils::create_reserve(
                &e,
                &pool_address,
                &underlying_1,
                &reserve_config_1,
                &reserve_data_1,
            );

            let (underlying_2, _) = testutils::create_token_contract(&e, &bombadil);
            let (mut reserve_config_2, mut reserve_data_2) = testutils::default_reserve_meta();
            reserve_data_2.last_time = 11845;
            reserve_config_2.index = 2;
            testutils::create_reserve(
                &e,
                &pool_address,
                &underlying_2,
                &reserve_config_2,
                &reserve_data_2,
            );

            oracle_client.set_data(
                &bombadil,
                &Asset::Other(Symbol::new(&e, "USD")),
                &vec![
                    &e,
                    Asset::Stellar(underlying_0.clone()),
                    Asset::Stellar(underlying_1.clone()),
                    Asset::Stellar(underlying_2.clone()),
                    Asset::Stellar(usdc_id.clone()),
                ],
                &7,
                &300,
            );
            oracle_client.set_price_stable(&vec![&e, 2_0000000, 4_0000000, 100_0000000, 1_0000000]);

            let pool_config = PoolConfig {
                oracle: oracle_id,
                min_collateral: 1_0000000,
                bstop_rate: 0_1000000,
                status: 0,
                max_positions: 4,
            };
            e.as_contract(&pool_address, || {
                storage::set_pool_config(&e, &pool_config);

                let result = create_interest_auction_data(
                    &e,
                    &backstop_address,
                    &vec![&e, backstop_token_id.clone()],
                    &vec![
                        &e,
                        underlying_0.clone(),
                        underlying_1.clone(),
                        underlying_2.clone(),
                    ],
                    100,
                );
                assert_eq!(result.block, 151);
                assert_eq!(result.bid.get_unchecked(backstop_token_id), 288_0008868);
                assert_eq!(result.bid.len(), 1);
                assert_eq!(result.lot.get_unchecked(underlying_0), 100_0000713);
                assert_eq!(result.lot.get_unchecked(underlying_1), 25_0000178);
                assert_eq!(result.lot.get_unchecked(underlying_2), 71);
                assert_eq!(result.lot.len(), 3);
            });
        }

        #[test]
        fn test_fill_interest_auction() {
            let e = Env::default();
            e.mock_all_auths_allowing_non_root_auth();
            e.cost_estimate().budget().reset_unlimited();

            e.ledger().set(LedgerInfo {
                timestamp: 12345,
                protocol_version: 22,
                sequence_number: 301,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 3110400,
            });

            let bombadil = Address::generate(&e);
            let samwise = Address::generate(&e);

            let pool_address = create_pool(&e);

            let (usdc_id, usdc_client) = testutils::create_token_contract(&e, &bombadil);
            let (blnd_id, blnd_client) = testutils::create_blnd_token(&e, &pool_address, &bombadil);

            let (backstop_token_id, backstop_token_client) =
                create_comet_lp_pool(&e, &bombadil, &blnd_id, &usdc_id);
            blnd_client.mint(&samwise, &10_000_0000000);
            usdc_client.mint(&samwise, &250_0000000);
            let exp_ledger = e.ledger().sequence() + 100;
            blnd_client.approve(&bombadil, &backstop_token_id, &2_000_0000000, &exp_ledger);
            usdc_client.approve(&bombadil, &backstop_token_id, &2_000_0000000, &exp_ledger);
            backstop_token_client.join_pool(
                &(100 * SCALAR_7),
                &vec![&e, 10_000_0000000, 250_0000000],
                &samwise,
            );
            let (backstop_address, backstop_client) = testutils::create_backstop(
                &e,
                &pool_address,
                &backstop_token_id,
                &usdc_id,
                &blnd_id,
            );
            backstop_client.deposit(&bombadil, &pool_address, &(50 * SCALAR_7));

            let (underlying_0, underlying_0_client) =
                testutils::create_token_contract(&e, &bombadil);
            let (mut reserve_config_0, mut reserve_data_0) = testutils::default_reserve_meta();
            reserve_data_0.b_rate = 1_100_000_000_000;
            reserve_data_0.b_supply = 200_000_0000000;
            reserve_data_0.d_supply = 100_000_0000000;
            reserve_data_0.last_time = 12345;
            reserve_data_0.backstop_credit = 100_0000000;
            reserve_config_0.index = 0;
            testutils::create_reserve(
                &e,
                &pool_address,
                &underlying_0,
                &reserve_config_0,
                &reserve_data_0,
            );
            underlying_0_client.mint(&pool_address, &1_000_0000000);

            let (underlying_1, underlying_1_client) =
                testutils::create_token_contract(&e, &bombadil);
            let (mut reserve_config_1, mut reserve_data_1) = testutils::default_reserve_meta();
            reserve_data_1.b_rate = 1_100_000_000_000;
            reserve_data_0.b_supply = 10_000_0000000;
            reserve_data_0.b_supply = 7_000_0000000;
            reserve_data_1.last_time = 12345;
            reserve_data_1.backstop_credit = 30_0000000;
            reserve_config_1.index = 1;
            testutils::create_reserve(
                &e,
                &pool_address,
                &underlying_1,
                &reserve_config_1,
                &reserve_data_1,
            );
            underlying_1_client.mint(&pool_address, &1_000_0000000);

            let pool_config = PoolConfig {
                oracle: Address::generate(&e),
                min_collateral: 1_0000000,
                bstop_rate: 0_1000000,
                status: 0,
                max_positions: 4,
            };
            let mut auction_data = AuctionData {
                bid: map![&e, (backstop_token_id.clone(), 75_0000000)],
                lot: map![
                    &e,
                    (underlying_0.clone(), 100_0000000),
                    (underlying_1.clone(), 25_0000000)
                ],
                block: 51,
            };

            backstop_token_client.approve(
                &samwise,
                &backstop_address,
                &75_0000000,
                &e.ledger().sequence(),
            );
            e.as_contract(&pool_address, || {
                e.mock_all_auths_allowing_non_root_auth();
                storage::set_auction(
                    &e,
                    &(AuctionType::InterestAuction as u32),
                    &backstop_address,
                    &auction_data,
                );
                storage::set_pool_config(&e, &pool_config);
                storage::set_backstop(&e, &backstop_address);
                let mut pool = Pool::load(&e);
                let backstop_token_balance_pre_fill =
                    backstop_token_client.balance(&backstop_address);
                fill_interest_auction(&e, &mut pool, &mut auction_data, &samwise);
                pool.store_cached_reserves(&e);

                assert_eq!(backstop_token_client.balance(&samwise), 25_0000000);
                assert_eq!(
                    backstop_token_client.balance(&backstop_address),
                    backstop_token_balance_pre_fill + 75_0000000
                );
                assert_eq!(underlying_0_client.balance(&samwise), 100_0000000);
                assert_eq!(underlying_1_client.balance(&samwise), 25_0000000);
                // verify only filled backstop credits get deducted from total
                let reserve_0_data = storage::get_res_data(&e, &underlying_0);
                assert_eq!(reserve_0_data.backstop_credit, 0);
                let reserve_1_data = storage::get_res_data(&e, &underlying_1);
                assert_eq!(reserve_1_data.backstop_credit, 5_0000000);
            });
        }

        #[test]
        fn test_fill_interest_auction_empty_bid() {
            let e = Env::default();
            e.mock_all_auths_allowing_non_root_auth();
            e.cost_estimate().budget().reset_unlimited();

            e.ledger().set(LedgerInfo {
                timestamp: 12345,
                protocol_version: 22,
                sequence_number: 301,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 3110400,
            });

            let bombadil = Address::generate(&e);
            let samwise = Address::generate(&e);

            let pool_address = create_pool(&e);

            let (usdc_id, usdc_client) = testutils::create_token_contract(&e, &bombadil);
            let (blnd_id, blnd_client) = testutils::create_blnd_token(&e, &pool_address, &bombadil);

            let (backstop_token_id, backstop_token_client) =
                create_comet_lp_pool(&e, &bombadil, &blnd_id, &usdc_id);
            blnd_client.mint(&samwise, &10_000_0000000);
            usdc_client.mint(&samwise, &250_0000000);
            let exp_ledger = e.ledger().sequence() + 100;
            blnd_client.approve(&bombadil, &backstop_token_id, &2_000_0000000, &exp_ledger);
            usdc_client.approve(&bombadil, &backstop_token_id, &2_000_0000000, &exp_ledger);
            backstop_token_client.join_pool(
                &(100 * SCALAR_7),
                &vec![&e, 10_000_0000000, 250_0000000],
                &samwise,
            );
            let (backstop_address, backstop_client) = testutils::create_backstop(
                &e,
                &pool_address,
                &backstop_token_id,
                &usdc_id,
                &blnd_id,
            );
            backstop_client.deposit(&bombadil, &pool_address, &(50 * SCALAR_7));

            let (underlying_0, underlying_0_client) =
                testutils::create_token_contract(&e, &bombadil);
            let (mut reserve_config_0, mut reserve_data_0) = testutils::default_reserve_meta();
            reserve_data_0.b_rate = 1_100_000_000_000;
            reserve_data_0.b_supply = 200_000_0000000;
            reserve_data_0.d_supply = 100_000_0000000;
            reserve_data_0.last_time = 12345;
            reserve_data_0.backstop_credit = 100_0000000;
            reserve_config_0.index = 0;
            testutils::create_reserve(
                &e,
                &pool_address,
                &underlying_0,
                &reserve_config_0,
                &reserve_data_0,
            );
            underlying_0_client.mint(&pool_address, &1_000_0000000);

            let (underlying_1, underlying_1_client) =
                testutils::create_token_contract(&e, &bombadil);
            let (mut reserve_config_1, mut reserve_data_1) = testutils::default_reserve_meta();
            reserve_data_1.b_rate = 1_100_000_000_000;
            reserve_data_0.b_supply = 10_000_0000000;
            reserve_data_0.b_supply = 7_000_0000000;
            reserve_data_1.last_time = 12345;
            reserve_data_1.backstop_credit = 30_0000000;
            reserve_config_1.index = 1;
            testutils::create_reserve(
                &e,
                &pool_address,
                &underlying_1,
                &reserve_config_1,
                &reserve_data_1,
            );
            underlying_1_client.mint(&pool_address, &1_000_0000000);

            let pool_config = PoolConfig {
                oracle: Address::generate(&e),
                min_collateral: 1_0000000,
                bstop_rate: 0_1000000,
                status: 0,
                max_positions: 4,
            };
            let mut auction_data = AuctionData {
                bid: map![&e],
                lot: map![
                    &e,
                    (underlying_0.clone(), 100_0000000),
                    (underlying_1.clone(), 25_0000000)
                ],
                block: 51,
            };
            e.as_contract(&pool_address, || {
                e.mock_all_auths_allowing_non_root_auth();
                storage::set_auction(
                    &e,
                    &(AuctionType::InterestAuction as u32),
                    &backstop_address,
                    &auction_data,
                );
                storage::set_pool_config(&e, &pool_config);
                storage::set_backstop(&e, &backstop_address);
                let mut pool = Pool::load(&e);
                let backstop_token_balance_pre_fill =
                    backstop_token_client.balance(&backstop_address);
                fill_interest_auction(&e, &mut pool, &mut auction_data, &samwise);
                pool.store_cached_reserves(&e);

                assert_eq!(backstop_token_client.balance(&samwise), 100 * SCALAR_7);
                assert_eq!(
                    backstop_token_client.balance(&backstop_address),
                    backstop_token_balance_pre_fill
                );
                assert_eq!(underlying_0_client.balance(&samwise), 100_0000000);
                assert_eq!(underlying_1_client.balance(&samwise), 25_0000000);
                // verify only filled backstop credits get deducted from total
                let reserve_0_data = storage::get_res_data(&e, &underlying_0);
                assert_eq!(reserve_0_data.backstop_credit, 0);
                let reserve_1_data = storage::get_res_data(&e, &underlying_1);
                assert_eq!(reserve_1_data.backstop_credit, 5_0000000);
            });
        }

        #[test]
        #[should_panic(expected = "Error(Contract, #1200)")]
        fn test_fill_interest_auction_with_backstop() {
            let e = Env::default();
            e.mock_all_auths_allowing_non_root_auth();
            e.cost_estimate().budget().reset_unlimited();

            e.ledger().set(LedgerInfo {
                timestamp: 12345,
                protocol_version: 22,
                sequence_number: 301,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 3110400,
            });

            let bombadil = Address::generate(&e);
            let samwise = Address::generate(&e);

            let pool_address = create_pool(&e);

            let (usdc_id, usdc_client) = testutils::create_token_contract(&e, &bombadil);
            let (backstop_address, _) = testutils::create_backstop(
                &e,
                &pool_address,
                &Address::generate(&e),
                &usdc_id,
                &Address::generate(&e),
            );

            let (underlying_0, underlying_0_client) =
                testutils::create_token_contract(&e, &bombadil);
            let (mut reserve_config_0, reserve_data_0) = testutils::default_reserve_meta();
            reserve_config_0.index = 0;
            testutils::create_reserve(
                &e,
                &pool_address,
                &underlying_0,
                &reserve_config_0,
                &reserve_data_0,
            );
            underlying_0_client.mint(&pool_address, &1_000_0000000);

            let (underlying_1, underlying_1_client) =
                testutils::create_token_contract(&e, &bombadil);
            let (mut reserve_config_1, reserve_data_1) = testutils::default_reserve_meta();
            reserve_config_1.index = 1;
            testutils::create_reserve(
                &e,
                &pool_address,
                &underlying_1,
                &reserve_config_1,
                &reserve_data_1,
            );
            underlying_1_client.mint(&pool_address, &1_000_0000000);

            let pool_config = PoolConfig {
                oracle: Address::generate(&e),
                min_collateral: 1_0000000,
                bstop_rate: 0_1000000,
                status: 0,
                max_positions: 4,
            };
            let mut auction_data = AuctionData {
                bid: map![&e, (usdc_id.clone(), 95_0000000)],
                lot: map![
                    &e,
                    (underlying_0.clone(), 100_0000000),
                    (underlying_1.clone(), 25_0000000)
                ],
                block: 51,
            };
            usdc_client.mint(&samwise, &100_0000000);
            e.as_contract(&pool_address, || {
                e.mock_all_auths_allowing_non_root_auth();
                storage::set_auction(
                    &e,
                    &(AuctionType::InterestAuction as u32),
                    &backstop_address,
                    &auction_data,
                );
                storage::set_pool_config(&e, &pool_config);
                storage::set_backstop(&e, &backstop_address);

                let mut pool = Pool::load(&e);
                fill_interest_auction(&e, &mut pool, &mut auction_data, &backstop_address);
            });
        }
    }
}
