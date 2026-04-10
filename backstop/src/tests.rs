#![cfg(test)]

mod backstop_src_backstop_withdrawal {
    use crate::{
        contract::require_nonnegative, dependencies::PoolClient, emissions, storage, BackstopError,
    };

    use sep_41_token::TokenClient;

    use soroban_sdk::{panic_with_error, unwrap::UnwrapOptimized, Address, Env};

    use crate::backstop::Q4W;

    pub(crate) use crate::backstop::withdrawal::*;

    mod tests {
        use mock_pool::Positions;
        use soroban_sdk::{
            map,
            testutils::{Address as _, Ledger, LedgerInfo},
            vec, Address,
        };

        use crate::{
            backstop::{execute_deposit, execute_donate, execute_draw},
            testutils::{
                assert_eq_vec_q4w, create_backstop, create_backstop_token, create_mock_pool,
                create_mock_pool_factory,
            },
        };

        use super::*;

        #[test]
        fn test_execute_queue_withdrawal() {
            let e = Env::default();
            e.mock_all_auths_allowing_non_root_auth();

            let backstop_address = create_backstop(&e);
            let pool_address = Address::generate(&e);
            let bombadil = Address::generate(&e);
            let samwise = Address::generate(&e);

            let (_, backstop_token_client) =
                create_backstop_token(&e, &backstop_address, &bombadil);
            backstop_token_client.mint(&samwise, &100_0000000);

            let (_, mock_pool_factory_client) = create_mock_pool_factory(&e, &backstop_address);
            mock_pool_factory_client.set_pool(&pool_address);

            // setup pool with deposits
            e.as_contract(&backstop_address, || {
                execute_deposit(&e, &samwise, &pool_address, 100_0000000);
            });

            e.ledger().set(LedgerInfo {
                protocol_version: 22,
                sequence_number: 200,
                timestamp: 10000,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 3110400,
            });

            e.as_contract(&backstop_address, || {
                execute_queue_withdrawal(&e, &samwise, &pool_address, 42_0000000);

                let new_user_balance = storage::get_user_balance(&e, &pool_address, &samwise);
                assert_eq!(new_user_balance.shares, 58_0000000);
                let expected_q4w = vec![
                    &e,
                    Q4W {
                        amount: 42_0000000,
                        exp: 10000 + 17 * 24 * 60 * 60,
                    },
                ];
                assert_eq_vec_q4w(&new_user_balance.q4w, &expected_q4w);

                let new_pool_balance = storage::get_pool_balance(&e, &pool_address);
                assert_eq!(new_pool_balance.q4w, 42_0000000);
                assert_eq!(new_pool_balance.shares, 100_0000000);
                assert_eq!(new_pool_balance.tokens, 100_0000000);

                assert_eq!(
                    backstop_token_client.balance(&backstop_address),
                    100_0000000
                );
                assert_eq!(backstop_token_client.balance(&samwise), 0);
            });
        }

        #[test]
        #[should_panic(expected = "Error(Contract, #8)")]
        fn test_execute_queue_withdrawal_negative_amount() {
            let e = Env::default();
            e.mock_all_auths_allowing_non_root_auth();

            let backstop_address = create_backstop(&e);
            let pool_address = Address::generate(&e);
            let bombadil = Address::generate(&e);
            let samwise = Address::generate(&e);

            let (_, backstop_token_client) =
                create_backstop_token(&e, &backstop_address, &bombadil);
            backstop_token_client.mint(&samwise, &100_0000000);

            let (_, mock_pool_factory_client) = create_mock_pool_factory(&e, &backstop_address);
            mock_pool_factory_client.set_pool(&pool_address);

            // setup pool with deposits
            e.as_contract(&backstop_address, || {
                execute_deposit(&e, &samwise, &pool_address, 100_0000000);
            });

            e.ledger().set(LedgerInfo {
                protocol_version: 22,
                sequence_number: 200,
                timestamp: 10000,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 3110400,
            });

            e.as_contract(&backstop_address, || {
                execute_queue_withdrawal(&e, &samwise, &pool_address, -42_0000000);
            });
        }

        #[test]
        fn test_execute_dequeue_withdrawal() {
            let e = Env::default();
            e.mock_all_auths_allowing_non_root_auth();

            let backstop_address = create_backstop(&e);
            let pool_address = Address::generate(&e);
            let bombadil = Address::generate(&e);
            let samwise = Address::generate(&e);

            let (_, backstop_token_client) =
                create_backstop_token(&e, &backstop_address, &bombadil);
            backstop_token_client.mint(&samwise, &100_0000000);

            let (_, mock_pool_factory_client) = create_mock_pool_factory(&e, &backstop_address);
            mock_pool_factory_client.set_pool(&pool_address);

            // queue shares for withdraw
            e.as_contract(&backstop_address, || {
                execute_deposit(&e, &samwise, &pool_address, 75_0000000);

                e.ledger().set(LedgerInfo {
                    protocol_version: 22,
                    sequence_number: 100,
                    timestamp: 10000,
                    network_id: Default::default(),
                    base_reserve: 10,
                    min_temp_entry_ttl: 10,
                    min_persistent_entry_ttl: 10,
                    max_entry_ttl: 3110400,
                });

                execute_queue_withdrawal(&e, &samwise, &pool_address, 25_0000000);

                e.ledger().set(LedgerInfo {
                    protocol_version: 22,
                    sequence_number: 100,
                    timestamp: 20000,
                    network_id: Default::default(),
                    base_reserve: 10,
                    min_temp_entry_ttl: 10,
                    min_persistent_entry_ttl: 10,
                    max_entry_ttl: 3110400,
                });

                execute_queue_withdrawal(&e, &samwise, &pool_address, 40_0000000);
            });

            e.ledger().set(LedgerInfo {
                protocol_version: 22,
                sequence_number: 200,
                timestamp: 30000,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 3110400,
            });

            e.as_contract(&backstop_address, || {
                execute_dequeue_withdrawal(&e, &samwise, &pool_address, 30_0000000);

                let new_user_balance = storage::get_user_balance(&e, &pool_address, &samwise);
                assert_eq!(new_user_balance.shares, 40_0000000);
                let expected_q4w = vec![
                    &e,
                    Q4W {
                        amount: 25_0000000,
                        exp: 10000 + 17 * 24 * 60 * 60,
                    },
                    Q4W {
                        amount: 10_0000000,
                        exp: 20000 + 17 * 24 * 60 * 60,
                    },
                ];
                assert_eq_vec_q4w(&new_user_balance.q4w, &expected_q4w);

                let new_pool_balance = storage::get_pool_balance(&e, &pool_address);
                assert_eq!(new_pool_balance.q4w, 35_0000000);
                assert_eq!(new_pool_balance.shares, 75_0000000);
                assert_eq!(new_pool_balance.tokens, 75_0000000);
            });
        }
        #[test]
        #[should_panic(expected = "Error(Contract, #8)")]
        fn test_execute_dequeue_withdrawal_negative_amount() {
            let e = Env::default();
            e.mock_all_auths_allowing_non_root_auth();

            let backstop_address = create_backstop(&e);
            let pool_address = Address::generate(&e);
            let bombadil = Address::generate(&e);
            let samwise = Address::generate(&e);

            let (_, backstop_token_client) =
                create_backstop_token(&e, &backstop_address, &bombadil);
            backstop_token_client.mint(&samwise, &100_0000000);

            let (_, mock_pool_factory_client) = create_mock_pool_factory(&e, &backstop_address);
            mock_pool_factory_client.set_pool(&pool_address);

            // queue shares for withdraw
            e.as_contract(&backstop_address, || {
                execute_deposit(&e, &samwise, &pool_address, 75_0000000);
                execute_queue_withdrawal(&e, &samwise, &pool_address, 25_0000000);

                e.ledger().set(LedgerInfo {
                    protocol_version: 22,
                    sequence_number: 100,
                    timestamp: 10000,
                    network_id: Default::default(),
                    base_reserve: 10,
                    min_temp_entry_ttl: 10,
                    min_persistent_entry_ttl: 10,
                    max_entry_ttl: 3110400,
                });

                execute_queue_withdrawal(&e, &samwise, &pool_address, 40_0000000);
            });

            e.ledger().set(LedgerInfo {
                protocol_version: 22,
                sequence_number: 200,
                timestamp: 20000,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 3110400,
            });

            e.as_contract(&backstop_address, || {
                execute_dequeue_withdrawal(&e, &samwise, &pool_address, -30_0000000);
            });
        }

        #[test]
        fn test_execute_withdrawal() {
            let e = Env::default();
            e.mock_all_auths_allowing_non_root_auth();

            let backstop_address = create_backstop(&e);
            let (pool_address, _) = create_mock_pool(&e);

            let bombadil = Address::generate(&e);
            let samwise = Address::generate(&e);

            let (_, backstop_token_client) =
                create_backstop_token(&e, &backstop_address, &bombadil);
            backstop_token_client.mint(&samwise, &150_0000000);

            let (_, mock_pool_factory_client) = create_mock_pool_factory(&e, &backstop_address);
            mock_pool_factory_client.set_pool(&pool_address);

            e.ledger().set(LedgerInfo {
                protocol_version: 22,
                sequence_number: 200,
                timestamp: 10000,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 3110400,
            });

            backstop_token_client.approve(
                &samwise,
                &backstop_address,
                &50_0000000,
                &e.ledger().sequence(),
            );
            // setup pool with queue for withdrawal and allow the backstop to incur a profit
            e.as_contract(&backstop_address, || {
                execute_deposit(&e, &samwise, &pool_address, 100_0000000);
                execute_queue_withdrawal(&e, &samwise, &pool_address, 42_0000000);
                execute_donate(&e, &samwise, &pool_address, 50_0000000);
            });

            e.ledger().set(LedgerInfo {
                protocol_version: 22,
                sequence_number: 200,
                timestamp: 10000 + 17 * 24 * 60 * 60 + 1,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 3110400,
            });

            e.as_contract(&backstop_address, || {
                let tokens = execute_withdraw(&e, &samwise, &pool_address, 42_0000000);

                let new_user_balance = storage::get_user_balance(&e, &pool_address, &samwise);
                assert_eq!(new_user_balance.shares, 100_0000000 - 42_0000000);
                assert_eq!(new_user_balance.q4w.len(), 0);

                let new_pool_balance = storage::get_pool_balance(&e, &pool_address);
                assert_eq!(new_pool_balance.q4w, 0);
                assert_eq!(new_pool_balance.shares, 100_0000000 - 42_0000000);
                assert_eq!(new_pool_balance.tokens, 150_0000000 - tokens);
                assert_eq!(tokens, 63_0000000);

                assert_eq!(
                    backstop_token_client.balance(&backstop_address),
                    150_0000000 - tokens
                );
                assert_eq!(backstop_token_client.balance(&samwise), tokens);
            });
        }

        #[test]
        #[should_panic(expected = "Error(Contract, #8)")]
        fn test_execute_withdrawal_negative_amount() {
            let e = Env::default();
            e.mock_all_auths_allowing_non_root_auth();

            let backstop_address = create_backstop(&e);
            let (pool_address, _) = create_mock_pool(&e);

            let bombadil = Address::generate(&e);
            let samwise = Address::generate(&e);

            let (_, backstop_token_client) =
                create_backstop_token(&e, &backstop_address, &bombadil);
            backstop_token_client.mint(&samwise, &150_0000000);

            let (_, mock_pool_factory_client) = create_mock_pool_factory(&e, &backstop_address);
            mock_pool_factory_client.set_pool(&pool_address);

            e.ledger().set(LedgerInfo {
                protocol_version: 22,
                sequence_number: 200,
                timestamp: 10000,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 3110400,
            });

            backstop_token_client.approve(
                &samwise,
                &backstop_address,
                &50_0000000,
                &e.ledger().sequence(),
            );
            // setup pool with queue for withdrawal and allow the backstop to incur a profit
            e.as_contract(&backstop_address, || {
                execute_deposit(&e, &samwise, &pool_address, 100_0000000);
                execute_queue_withdrawal(&e, &samwise, &pool_address, 42_0000000);
                execute_donate(&e, &samwise, &pool_address, 50_0000000);
            });

            e.ledger().set(LedgerInfo {
                protocol_version: 22,
                sequence_number: 200,
                timestamp: 10000 + 17 * 24 * 60 * 60 + 1,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 3110400,
            });

            e.as_contract(&backstop_address, || {
                execute_withdraw(&e, &samwise, &pool_address, -42_0000000);
            });
        }

        #[test]
        #[should_panic(expected = "Error(Contract, #1006)")]
        fn test_execute_withdrawal_zero_tokens() {
            let e = Env::default();
            e.mock_all_auths_allowing_non_root_auth();

            let backstop_address = create_backstop(&e);
            let (pool_address, _) = create_mock_pool(&e);

            let bombadil = Address::generate(&e);
            let samwise = Address::generate(&e);
            let frodo = Address::generate(&e);

            let (_, backstop_token_client) =
                create_backstop_token(&e, &backstop_address, &bombadil);
            backstop_token_client.mint(&samwise, &150_0000000);
            backstop_token_client.mint(&frodo, &150_0000000);

            let (_, mock_pool_factory_client) = create_mock_pool_factory(&e, &backstop_address);
            mock_pool_factory_client.set_pool(&pool_address);

            e.ledger().set(LedgerInfo {
                protocol_version: 22,
                sequence_number: 200,
                timestamp: 10000,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 3110400,
            });

            // setup pool with queue for withdrawal and allow the backstop to incur a profit
            e.as_contract(&backstop_address, || {
                execute_deposit(&e, &frodo, &pool_address, 1_0000001);
                execute_deposit(&e, &samwise, &pool_address, 1_0000000);
                execute_queue_withdrawal(&e, &samwise, &pool_address, 1_0000000);
                execute_draw(&e, &pool_address, 1_9999999, &frodo);
            });

            e.ledger().set(LedgerInfo {
                protocol_version: 22,
                sequence_number: 200,
                timestamp: 10000 + 17 * 24 * 60 * 60 + 1,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 3110400,
            });

            e.as_contract(&backstop_address, || {
                execute_withdraw(&e, &samwise, &pool_address, 1_0000000);
            });
        }

        #[test]
        #[should_panic(expected = "Error(Contract, #1011)")]
        fn test_execute_withdrawal_bad_debt_exists() {
            let e = Env::default();
            e.mock_all_auths_allowing_non_root_auth();

            let backstop_address = create_backstop(&e);
            let (pool_address, mock_pool_client) = create_mock_pool(&e);

            let bombadil = Address::generate(&e);
            let samwise = Address::generate(&e);

            let (_, backstop_token_client) =
                create_backstop_token(&e, &backstop_address, &bombadil);
            backstop_token_client.mint(&samwise, &150_0000000);

            let (_, mock_pool_factory_client) = create_mock_pool_factory(&e, &backstop_address);
            mock_pool_factory_client.set_pool(&pool_address);

            e.ledger().set(LedgerInfo {
                protocol_version: 22,
                sequence_number: 200,
                timestamp: 10000,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 3110400,
            });

            // give the backstop bad debt
            let backstop_positions = Positions {
                liabilities: map![&e, (0, 1_0000000)],
                collateral: map![&e],
                supply: map![&e],
            };
            mock_pool_client.set_positions(&backstop_address, &backstop_positions);

            backstop_token_client.approve(
                &samwise,
                &backstop_address,
                &50_0000000,
                &e.ledger().sequence(),
            );

            // setup pool with queue for withdrawal and allow the backstop to incur a profit
            e.as_contract(&backstop_address, || {
                execute_deposit(&e, &samwise, &pool_address, 100_0000000);
                execute_queue_withdrawal(&e, &samwise, &pool_address, 42_0000000);
                execute_donate(&e, &samwise, &pool_address, 50_0000000);
            });

            e.ledger().set(LedgerInfo {
                protocol_version: 22,
                sequence_number: 200,
                timestamp: 10000 + 17 * 24 * 60 * 60 + 1,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 3110400,
            });

            e.as_contract(&backstop_address, || {
                let tokens = execute_withdraw(&e, &samwise, &pool_address, 42_0000000);

                let new_user_balance = storage::get_user_balance(&e, &pool_address, &samwise);
                assert_eq!(new_user_balance.shares, 100_0000000 - 42_0000000);
                assert_eq!(new_user_balance.q4w.len(), 0);

                let new_pool_balance = storage::get_pool_balance(&e, &pool_address);
                assert_eq!(new_pool_balance.q4w, 0);
                assert_eq!(new_pool_balance.shares, 100_0000000 - 42_0000000);
                assert_eq!(new_pool_balance.tokens, 150_0000000 - tokens);
                assert_eq!(tokens, 63_0000000);

                assert_eq!(
                    backstop_token_client.balance(&backstop_address),
                    150_0000000 - tokens
                );
                assert_eq!(backstop_token_client.balance(&samwise), tokens);
            });
        }

        #[test]
        #[should_panic(expected = "Error(Contract, #1006)")]
        fn test_execute_withdrawal_drained_backstop() {
            let e = Env::default();
            e.mock_all_auths_allowing_non_root_auth();

            let backstop_address = create_backstop(&e);
            let (pool_address, _) = create_mock_pool(&e);

            let bombadil = Address::generate(&e);
            let samwise = Address::generate(&e);
            let frodo = Address::generate(&e);

            let (_, backstop_token_client) =
                create_backstop_token(&e, &backstop_address, &bombadil);
            backstop_token_client.mint(&samwise, &150_0000000);
            backstop_token_client.mint(&frodo, &150_0000000);

            let (_, mock_pool_factory_client) = create_mock_pool_factory(&e, &backstop_address);
            mock_pool_factory_client.set_pool(&pool_address);

            e.ledger().set(LedgerInfo {
                protocol_version: 22,
                sequence_number: 200,
                timestamp: 10000,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 3110400,
            });

            // setup pool with queue for withdrawal and allow the backstop to incur a profit
            e.as_contract(&backstop_address, || {
                execute_deposit(&e, &frodo, &pool_address, 1_0000001);
                execute_deposit(&e, &samwise, &pool_address, 1_0000000);
                execute_queue_withdrawal(&e, &samwise, &pool_address, 1_0000000);
                execute_draw(&e, &pool_address, 2_0000001, &frodo);
            });

            e.ledger().set(LedgerInfo {
                protocol_version: 22,
                sequence_number: 200,
                timestamp: 10000 + 17 * 24 * 60 * 60 + 1,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 3110400,
            });

            e.as_contract(&backstop_address, || {
                execute_withdraw(&e, &samwise, &pool_address, 1_0000000);
            });
        }

        #[test]
        fn test_execute_withdrawal_all_shares_over_1_rate() {
            let e = Env::default();
            e.mock_all_auths_allowing_non_root_auth();

            e.ledger().set(LedgerInfo {
                protocol_version: 22,
                sequence_number: 200,
                timestamp: 10000,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 3110400,
            });

            let backstop_address = create_backstop(&e);
            let (pool_address, _) = create_mock_pool(&e);

            let bombadil = Address::generate(&e);
            let samwise = Address::generate(&e);

            let (_, backstop_token_client) =
                create_backstop_token(&e, &backstop_address, &bombadil);

            let (_, mock_pool_factory_client) = create_mock_pool_factory(&e, &backstop_address);
            mock_pool_factory_client.set_pool(&pool_address);

            // setup pool with queue for withdrawal and allow the backstop to incur a profit
            let deposit_amount = 111_1111111;
            let donate_amount = 123;
            backstop_token_client.mint(&samwise, &(deposit_amount + donate_amount));
            backstop_token_client.approve(
                &samwise,
                &backstop_address,
                &donate_amount,
                &e.ledger().sequence(),
            );
            e.as_contract(&backstop_address, || {
                execute_deposit(&e, &samwise, &pool_address, deposit_amount);
                execute_queue_withdrawal(&e, &samwise, &pool_address, deposit_amount);
                execute_donate(&e, &samwise, &pool_address, donate_amount);
            });

            e.ledger().set(LedgerInfo {
                protocol_version: 22,
                sequence_number: 201,
                timestamp: 10000 + 17 * 24 * 60 * 60 + 1,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 3110400,
            });

            e.as_contract(&backstop_address, || {
                let tokens = execute_withdraw(&e, &samwise, &pool_address, deposit_amount);

                let new_user_balance = storage::get_user_balance(&e, &pool_address, &samwise);
                assert_eq!(new_user_balance.shares, 0);
                assert_eq!(new_user_balance.q4w.len(), 0);

                let new_pool_balance = storage::get_pool_balance(&e, &pool_address);
                assert_eq!(new_pool_balance.q4w, 0);
                assert_eq!(new_pool_balance.shares, 0);
                assert_eq!(new_pool_balance.tokens, 0);
                assert_eq!(tokens, deposit_amount + donate_amount);

                assert_eq!(backstop_token_client.balance(&backstop_address), 0);
                assert_eq!(backstop_token_client.balance(&samwise), tokens);
            });
        }

        #[test]
        fn test_execute_withdrawal_all_shares_under_1_rate() {
            let e = Env::default();
            e.mock_all_auths_allowing_non_root_auth();

            e.ledger().set(LedgerInfo {
                protocol_version: 22,
                sequence_number: 200,
                timestamp: 10000,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 3110400,
            });

            let backstop_address = create_backstop(&e);
            let (pool_address, _) = create_mock_pool(&e);

            let bombadil = Address::generate(&e);
            let samwise = Address::generate(&e);

            let (_, backstop_token_client) =
                create_backstop_token(&e, &backstop_address, &bombadil);

            let (_, mock_pool_factory_client) = create_mock_pool_factory(&e, &backstop_address);
            mock_pool_factory_client.set_pool(&pool_address);

            // setup pool with queue for withdrawal and allow the backstop to incur a profit
            let deposit_amount = 111_1111111;
            let draw_amount = 123;
            backstop_token_client.mint(&samwise, &deposit_amount);
            e.as_contract(&backstop_address, || {
                execute_deposit(&e, &samwise, &pool_address, deposit_amount);
                execute_queue_withdrawal(&e, &samwise, &pool_address, deposit_amount);
                execute_draw(&e, &pool_address, draw_amount, &samwise);
            });

            e.ledger().set(LedgerInfo {
                protocol_version: 22,
                sequence_number: 201,
                timestamp: 10000 + 17 * 24 * 60 * 60 + 1,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 3110400,
            });

            e.as_contract(&backstop_address, || {
                let tokens = execute_withdraw(&e, &samwise, &pool_address, deposit_amount);

                let new_user_balance = storage::get_user_balance(&e, &pool_address, &samwise);
                assert_eq!(new_user_balance.shares, 0);
                assert_eq!(new_user_balance.q4w.len(), 0);

                let new_pool_balance = storage::get_pool_balance(&e, &pool_address);
                assert_eq!(new_pool_balance.q4w, 0);
                assert_eq!(new_pool_balance.shares, 0);
                assert_eq!(new_pool_balance.tokens, 0);
                assert_eq!(tokens, deposit_amount - draw_amount);

                assert_eq!(backstop_token_client.balance(&backstop_address), 0);
                assert_eq!(backstop_token_client.balance(&samwise), deposit_amount);
            });
        }
    }
}

mod backstop_src_backstop_user {
    use soroban_sdk::{contracttype, panic_with_error, vec, Env, Vec};

    use crate::{
        constants::{MAX_Q4W_SIZE, Q4W_LOCK_TIME},
        errors::BackstopError,
    };

    pub(crate) use crate::backstop::user::*;

    mod tests {
        use crate::testutils::assert_eq_vec_q4w;

        use super::*;
        use soroban_sdk::{
            testutils::{Ledger, LedgerInfo},
            vec,
        };

        /********** Share Management **********/

        #[test]
        fn test_add_shares() {
            let e = Env::default();

            let mut user = UserBalance {
                shares: 100,
                q4w: vec![&e],
            };

            let to_add = 12318972;
            user.add_shares(to_add);

            assert_eq!(user.shares, to_add + 100);
        }

        /********** Q4W Management **********/

        #[test]
        fn test_q4w_none_queued() {
            let e = Env::default();

            let mut user = UserBalance {
                shares: 1000,
                q4w: vec![&e],
            };

            e.ledger().set(LedgerInfo {
                protocol_version: 22,
                sequence_number: 1,
                timestamp: 10000,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 3110400,
            });

            let to_queue = 500;
            user.queue_shares_for_withdrawal(&e, to_queue);
            assert_eq_vec_q4w(
                &user.q4w,
                &vec![
                    &e,
                    Q4W {
                        amount: to_queue,
                        exp: 10000 + 17 * 24 * 60 * 60,
                    },
                ],
            );
        }

        #[test]
        fn test_q4w_new_placed_last() {
            let e = Env::default();

            let mut cur_q4w = vec![
                &e,
                Q4W {
                    amount: 200,
                    exp: 12592000,
                },
            ];
            let mut user = UserBalance {
                shares: 1000,
                q4w: cur_q4w.clone(),
            };

            e.ledger().set(LedgerInfo {
                protocol_version: 22,
                sequence_number: 1,
                timestamp: 11000000,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 3110400,
            });

            let to_queue = 500;
            user.queue_shares_for_withdrawal(&e, to_queue);
            cur_q4w.push_back(Q4W {
                amount: to_queue,
                exp: 11000000 + 17 * 24 * 60 * 60,
            });
            assert_eq_vec_q4w(&user.q4w, &cur_q4w);
        }

        #[test]
        fn test_q4w_new_to_max_works() {
            let e = Env::default();
            let exp = 12592000;
            let mut cur_q4w = vec![&e];
            for i in 0..19 {
                cur_q4w.push_back(Q4W {
                    amount: 200,
                    exp: exp + i,
                });
            }
            let mut user = UserBalance {
                shares: 1000,
                q4w: cur_q4w.clone(),
            };

            e.ledger().set(LedgerInfo {
                protocol_version: 22,
                sequence_number: 1,
                timestamp: 11000000,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 3110400,
            });

            let to_queue = 500;
            user.queue_shares_for_withdrawal(&e, to_queue);
            cur_q4w.push_back(Q4W {
                amount: to_queue,
                exp: 11000000 + 17 * 24 * 60 * 60,
            });
            assert_eq_vec_q4w(&user.q4w, &cur_q4w);
        }

        #[test]
        #[should_panic(expected = "Error(Contract, #1007)")]
        fn test_q4w_new_over_max_panics() {
            let e = Env::default();

            let exp = 12592000;
            let mut cur_q4w = vec![&e];
            for i in 0..20 {
                cur_q4w.push_back(Q4W {
                    amount: 200,
                    exp: exp + i,
                });
            }
            let mut user = UserBalance {
                shares: 1000,
                q4w: cur_q4w.clone(),
            };

            e.ledger().set(LedgerInfo {
                protocol_version: 22,
                sequence_number: 1,
                timestamp: 11000000,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 3110400,
            });

            let to_queue = 500;
            user.queue_shares_for_withdrawal(&e, to_queue);
        }

        #[test]
        #[should_panic(expected = "Error(Contract, #10)")]
        fn test_q4w_over_shares_panics() {
            let e = Env::default();

            let cur_q4w = vec![
                &e,
                Q4W {
                    amount: 200,
                    exp: 12592000,
                },
            ];
            let mut user = UserBalance {
                shares: 800,
                q4w: cur_q4w.clone(),
            };

            e.ledger().set(LedgerInfo {
                protocol_version: 22,
                sequence_number: 1,
                timestamp: 11000000,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 3110400,
            });

            let to_queue = 801;
            user.queue_shares_for_withdrawal(&e, to_queue);
        }

        // withdraw_shares

        #[test]
        #[should_panic(expected = "Error(Contract, #10)")]
        fn test_withdraw_shares_no_q4w_panics() {
            let e = Env::default();

            let mut user = UserBalance {
                shares: 1000,
                q4w: vec![&e],
            };

            e.ledger().set(LedgerInfo {
                protocol_version: 22,
                sequence_number: 1,
                timestamp: 11000000,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 3110400,
            });

            let to_wd = 1;
            user.withdraw_shares(&e, to_wd);
        }

        #[test]
        fn test_withdraw_shares_exact_amount() {
            let e = Env::default();

            let cur_q4w = vec![
                &e,
                Q4W {
                    amount: 200,
                    exp: 12592000,
                },
            ];
            let mut user = UserBalance {
                shares: 1000,
                q4w: cur_q4w.clone(),
            };

            e.ledger().set(LedgerInfo {
                protocol_version: 22,
                sequence_number: 1,
                timestamp: 12592000,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 3110400,
            });

            let to_wd = 200;
            user.withdraw_shares(&e, to_wd);

            assert_eq_vec_q4w(&user.q4w, &vec![&e]);
            assert_eq!(user.shares, 1000);
        }

        #[test]
        fn test_withdraw_shares_less_than_entry() {
            let e = Env::default();

            let cur_q4w = vec![
                &e,
                Q4W {
                    amount: 200,
                    exp: 12592000,
                },
            ];
            let mut user = UserBalance {
                shares: 1000,
                q4w: cur_q4w.clone(),
            };

            e.ledger().set(LedgerInfo {
                protocol_version: 22,
                sequence_number: 1,
                timestamp: 12592000,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 3110400,
            });

            let to_wd = 150;
            user.withdraw_shares(&e, to_wd);

            let expected_q4w = vec![
                &e,
                Q4W {
                    amount: 50,
                    exp: 12592000,
                },
            ];
            assert_eq_vec_q4w(&user.q4w, &expected_q4w);
            assert_eq!(user.shares, 1000);
        }

        #[test]
        fn test_withdraw_shares_multiple_entries() {
            let e = Env::default();

            let cur_q4w = vec![
                &e,
                Q4W {
                    amount: 125,
                    exp: 10000000,
                },
                Q4W {
                    amount: 200,
                    exp: 12592000,
                },
                Q4W {
                    amount: 50,
                    exp: 19592000,
                },
            ];
            let mut user = UserBalance {
                shares: 1000,
                q4w: cur_q4w.clone(),
            };

            e.ledger().set(LedgerInfo {
                protocol_version: 22,
                sequence_number: 1,
                timestamp: 22592000,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 3110400,
            });

            let to_wd = 300;
            user.withdraw_shares(&e, to_wd);

            let expected_q4w = vec![
                &e,
                Q4W {
                    amount: 25,
                    exp: 12592000,
                },
                Q4W {
                    amount: 50,
                    exp: 19592000,
                },
            ];
            assert_eq_vec_q4w(&user.q4w, &expected_q4w);
            assert_eq!(user.shares, 1000);
        }

        #[test]
        #[should_panic(expected = "Error(Contract, #1001)")]
        fn test_withdraw_shares_multiple_entries_not_exp() {
            let e = Env::default();

            let cur_q4w = vec![
                &e,
                Q4W {
                    amount: 125,
                    exp: 10000000,
                },
                Q4W {
                    amount: 200,
                    exp: 12592000,
                },
                Q4W {
                    amount: 50,
                    exp: 19592000,
                },
            ];
            let mut user = UserBalance {
                shares: 1000,
                q4w: cur_q4w.clone(),
            };

            e.ledger().set(LedgerInfo {
                protocol_version: 22,
                sequence_number: 1,
                timestamp: 11192000,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 3110400,
            });

            let to_wd = 300;
            user.withdraw_shares(&e, to_wd);
        }

        #[test]
        #[should_panic(expected = "Error(Contract, #10)")]
        fn test_withdraw_shares_over_total() {
            let e = Env::default();

            let cur_q4w = vec![
                &e,
                Q4W {
                    amount: 125,
                    exp: 10000000,
                },
                Q4W {
                    amount: 200,
                    exp: 11190000,
                },
                Q4W {
                    amount: 50,
                    exp: 11191000,
                },
            ];
            let mut user = UserBalance {
                shares: 1000,
                q4w: cur_q4w.clone(),
            };

            e.ledger().set(LedgerInfo {
                protocol_version: 22,
                sequence_number: 1,
                timestamp: 11192000,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 3110400,
            });

            let to_dequeue = 376;
            user.withdraw_shares(&e, to_dequeue);
        }

        // dequeue_shares

        #[test]
        #[should_panic(expected = "Error(Contract, #10)")]
        fn test_dequeue_shares_no_q4w_panics() {
            let e = Env::default();

            let mut user = UserBalance {
                shares: 1000,
                q4w: vec![&e],
            };

            e.ledger().set(LedgerInfo {
                protocol_version: 22,
                sequence_number: 1,
                timestamp: 11000000,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 3110400,
            });

            let to_wd = 1;
            user.dequeue_shares(&e, to_wd);
        }

        #[test]
        fn test_dequeue_shares_exact_amount() {
            let e = Env::default();

            let cur_q4w = vec![
                &e,
                Q4W {
                    amount: 200,
                    exp: 10000000,
                },
            ];
            let mut user = UserBalance {
                shares: 1000,
                q4w: cur_q4w.clone(),
            };

            e.ledger().set(LedgerInfo {
                protocol_version: 22,
                sequence_number: 1,
                timestamp: 12592000,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 3110400,
            });

            let to_dequeue = 200;
            user.dequeue_shares(&e, to_dequeue);

            assert_eq_vec_q4w(&user.q4w, &vec![&e]);
            assert_eq!(user.shares, 1000);
        }

        #[test]
        fn test_dequeue_shares_less_than_entry() {
            let e = Env::default();

            let cur_q4w = vec![
                &e,
                Q4W {
                    amount: 200,
                    exp: 14592000,
                },
            ];
            let mut user = UserBalance {
                shares: 1000,
                q4w: cur_q4w.clone(),
            };

            e.ledger().set(LedgerInfo {
                protocol_version: 22,
                sequence_number: 1,
                timestamp: 12592000,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 3110400,
            });

            let to_dequeue = 150;
            user.dequeue_shares(&e, to_dequeue);

            let expected_q4w = vec![
                &e,
                Q4W {
                    amount: 50,
                    exp: 14592000,
                },
            ];
            assert_eq_vec_q4w(&user.q4w, &expected_q4w);
            assert_eq!(user.shares, 1000);
        }

        #[test]
        fn test_dequeue_shares_multiple_entries_dequeue_newest() {
            let e = Env::default();

            let cur_q4w = vec![
                &e,
                Q4W {
                    amount: 125,
                    exp: 10000000,
                },
                Q4W {
                    amount: 200,
                    exp: 12592000,
                },
                Q4W {
                    amount: 50,
                    exp: 19592000,
                },
            ];
            let mut user = UserBalance {
                shares: 1000,
                q4w: cur_q4w.clone(),
            };

            e.ledger().set(LedgerInfo {
                protocol_version: 22,
                sequence_number: 1,
                timestamp: 22592000,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 3110400,
            });

            let to_dequeue = 150;
            user.dequeue_shares(&e, to_dequeue);

            let expected_q4w = vec![
                &e,
                Q4W {
                    amount: 125,
                    exp: 10000000,
                },
                Q4W {
                    amount: 100,
                    exp: 12592000,
                },
            ];
            assert_eq_vec_q4w(&user.q4w, &expected_q4w);
            assert_eq!(user.shares, 1000);
        }

        #[test]
        #[should_panic(expected = "Error(Contract, #10)")]
        fn test_dequeue_shares_over_total() {
            let e = Env::default();

            let cur_q4w = vec![
                &e,
                Q4W {
                    amount: 125,
                    exp: 10000000,
                },
                Q4W {
                    amount: 200,
                    exp: 11190000,
                },
                Q4W {
                    amount: 50,
                    exp: 11191000,
                },
            ];
            let mut user = UserBalance {
                shares: 1000,
                q4w: cur_q4w.clone(),
            };

            e.ledger().set(LedgerInfo {
                protocol_version: 22,
                sequence_number: 1,
                timestamp: 11192000,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 3110400,
            });

            let to_dequeue = 376;
            user.dequeue_shares(&e, to_dequeue);
        }
    }
}

mod backstop_src_backstop_fund_management {
    use crate::{contract::require_nonnegative, storage, BackstopError};

    use sep_41_token::TokenClient;

    use soroban_sdk::{panic_with_error, Address, Env};

    use crate::backstop::require_is_from_pool_factory;

    pub(crate) use crate::backstop::fund_management::*;

    mod tests {
        use soroban_sdk::{testutils::Address as _, Address};

        use crate::{
            backstop::execute_deposit,
            testutils::{create_backstop, create_backstop_token, create_mock_pool_factory},
        };

        use super::*;

        #[test]
        fn test_execute_donate() {
            let e = Env::default();
            e.mock_all_auths_allowing_non_root_auth();
            e.cost_estimate().budget().reset_unlimited();

            let backstop_id = create_backstop(&e);
            let pool_0_id = Address::generate(&e);
            let bombadil = Address::generate(&e);
            let samwise = Address::generate(&e);
            let frodo = Address::generate(&e);

            let (_, backstop_token_client) = create_backstop_token(&e, &backstop_id, &bombadil);
            backstop_token_client.mint(&samwise, &100_0000000);
            backstop_token_client.mint(&frodo, &100_0000000);

            let (_, mock_pool_factory_client) = create_mock_pool_factory(&e, &backstop_id);
            mock_pool_factory_client.set_pool(&pool_0_id);

            // initialize pool 0 with funds
            e.as_contract(&backstop_id, || {
                execute_deposit(&e, &frodo, &pool_0_id, 25_0000000);
            });

            backstop_token_client.approve(
                &samwise,
                &backstop_id,
                &30_0000000,
                &e.ledger().sequence(),
            );
            e.as_contract(&backstop_id, || {
                execute_donate(&e, &samwise, &pool_0_id, 30_0000000);

                let new_pool_balance = storage::get_pool_balance(&e, &pool_0_id);
                assert_eq!(new_pool_balance.shares, 25_0000000);
                assert_eq!(new_pool_balance.tokens, 55_0000000);
            });
        }

        #[test]
        #[should_panic(expected = "Error(Contract, #8)")]
        fn test_execute_donate_negative_amount() {
            let e = Env::default();
            e.mock_all_auths_allowing_non_root_auth();
            e.cost_estimate().budget().reset_unlimited();

            let backstop_id = create_backstop(&e);
            let pool_0_id = Address::generate(&e);
            let bombadil = Address::generate(&e);
            let samwise = Address::generate(&e);
            let frodo = Address::generate(&e);

            let (_, backstop_token_client) = create_backstop_token(&e, &backstop_id, &bombadil);
            backstop_token_client.mint(&samwise, &100_0000000);
            backstop_token_client.mint(&frodo, &100_0000000);

            let (_, mock_pool_factory_client) = create_mock_pool_factory(&e, &backstop_id);
            mock_pool_factory_client.set_pool(&pool_0_id);

            // initialize pool 0 with funds
            e.as_contract(&backstop_id, || {
                execute_deposit(&e, &frodo, &pool_0_id, 25_0000000);
            });

            e.as_contract(&backstop_id, || {
                execute_donate(&e, &samwise, &pool_0_id, -30_0000000);
            });
        }

        #[test]
        #[should_panic(expected = "Error(Contract, #1000)")]
        fn test_execute_donate_from_is_to() {
            let e = Env::default();
            e.mock_all_auths_allowing_non_root_auth();
            e.cost_estimate().budget().reset_unlimited();

            let backstop_id = create_backstop(&e);
            let pool_0_id = Address::generate(&e);
            let bombadil = Address::generate(&e);
            let samwise = Address::generate(&e);
            let frodo = Address::generate(&e);

            let (_, backstop_token_client) = create_backstop_token(&e, &backstop_id, &bombadil);
            backstop_token_client.mint(&samwise, &100_0000000);
            backstop_token_client.mint(&frodo, &100_0000000);

            let (_, mock_pool_factory_client) = create_mock_pool_factory(&e, &backstop_id);
            mock_pool_factory_client.set_pool(&pool_0_id);

            // initialize pool 0 with funds
            e.as_contract(&backstop_id, || {
                execute_deposit(&e, &frodo, &pool_0_id, 25_0000000);
            });

            e.as_contract(&backstop_id, || {
                execute_donate(&e, &pool_0_id, &pool_0_id, 10_0000000);
            });
        }

        #[test]
        #[should_panic(expected = "Error(Contract, #1000)")]
        fn test_execute_donate_from_is_self() {
            let e = Env::default();
            e.mock_all_auths_allowing_non_root_auth();
            e.cost_estimate().budget().reset_unlimited();

            let backstop_id = create_backstop(&e);
            let pool_0_id = Address::generate(&e);
            let bombadil = Address::generate(&e);
            let samwise = Address::generate(&e);
            let frodo = Address::generate(&e);

            let (_, backstop_token_client) = create_backstop_token(&e, &backstop_id, &bombadil);
            backstop_token_client.mint(&samwise, &100_0000000);
            backstop_token_client.mint(&frodo, &100_0000000);

            let (_, mock_pool_factory_client) = create_mock_pool_factory(&e, &backstop_id);
            mock_pool_factory_client.set_pool(&pool_0_id);

            // initialize pool 0 with funds
            e.as_contract(&backstop_id, || {
                execute_deposit(&e, &frodo, &pool_0_id, 25_0000000);
            });

            e.as_contract(&backstop_id, || {
                execute_donate(&e, &backstop_id, &pool_0_id, 10_0000000);
            });
        }

        #[test]
        #[should_panic(expected = "Error(Contract, #1004)")]
        fn test_execute_donate_not_pool() {
            let e = Env::default();
            e.mock_all_auths_allowing_non_root_auth();
            e.cost_estimate().budget().reset_unlimited();

            let backstop_id = create_backstop(&e);
            let pool_0_id = Address::generate(&e);
            let bombadil = Address::generate(&e);
            let samwise = Address::generate(&e);
            let frodo = Address::generate(&e);

            let (_, backstop_token_client) = create_backstop_token(&e, &backstop_id, &bombadil);
            backstop_token_client.mint(&samwise, &100_0000000);
            backstop_token_client.mint(&frodo, &100_0000000);

            create_mock_pool_factory(&e, &backstop_id);

            e.as_contract(&backstop_id, || {
                execute_donate(&e, &samwise, &pool_0_id, 30_0000000);
            });
        }

        #[test]
        fn test_execute_draw() {
            let e = Env::default();
            e.mock_all_auths_allowing_non_root_auth();
            e.cost_estimate().budget().reset_unlimited();

            let backstop_address = create_backstop(&e);
            let pool_0_id = Address::generate(&e);
            let bombadil = Address::generate(&e);
            let samwise = Address::generate(&e);
            let frodo = Address::generate(&e);

            let (_, backstop_token_client) =
                create_backstop_token(&e, &backstop_address, &bombadil);
            backstop_token_client.mint(&frodo, &100_0000000);

            let (_, mock_pool_factory_client) = create_mock_pool_factory(&e, &backstop_address);
            mock_pool_factory_client.set_pool(&pool_0_id);

            // initialize pool 0 with funds
            e.as_contract(&backstop_address, || {
                execute_deposit(&e, &frodo, &pool_0_id, 50_0000000);
            });

            e.as_contract(&backstop_address, || {
                execute_draw(&e, &pool_0_id, 30_0000000, &samwise);

                let new_pool_balance = storage::get_pool_balance(&e, &pool_0_id);
                assert_eq!(new_pool_balance.shares, 50_0000000);
                assert_eq!(new_pool_balance.tokens, 20_0000000);
                assert_eq!(backstop_token_client.balance(&backstop_address), 20_0000000);
                assert_eq!(backstop_token_client.balance(&samwise), 30_0000000);
            });
        }

        #[test]
        #[should_panic(expected = "Error(Contract, #1003)")]
        fn test_execute_draw_only_can_take_from_pool() {
            let e = Env::default();
            e.mock_all_auths_allowing_non_root_auth();
            e.cost_estimate().budget().reset_unlimited();

            let backstop_id = create_backstop(&e);
            let pool_0_id = Address::generate(&e);
            let pool_1_id = Address::generate(&e);
            let bombadil = Address::generate(&e);
            let samwise = Address::generate(&e);
            let frodo = Address::generate(&e);

            let (_, backstop_token_client) = create_backstop_token(&e, &backstop_id, &bombadil);
            backstop_token_client.mint(&frodo, &100_0000000);

            let (_, mock_pool_factory_client) = create_mock_pool_factory(&e, &backstop_id);
            mock_pool_factory_client.set_pool(&pool_0_id);
            mock_pool_factory_client.set_pool(&pool_1_id);

            // initialize pool 0 with funds
            e.as_contract(&backstop_id, || {
                execute_deposit(&e, &frodo, &pool_0_id, 50_0000000);
                execute_deposit(&e, &frodo, &pool_1_id, 50_0000000);
            });

            e.as_contract(&backstop_id, || {
                execute_draw(&e, &pool_0_id, 51_0000000, &samwise);
            });
        }

        #[test]
        #[should_panic(expected = "Error(Contract, #8)")]
        fn test_execute_draw_negative_amount() {
            let e = Env::default();
            e.mock_all_auths_allowing_non_root_auth();
            e.cost_estimate().budget().reset_unlimited();

            let backstop_id = create_backstop(&e);
            let pool_0_id = Address::generate(&e);
            let bombadil = Address::generate(&e);
            let samwise = Address::generate(&e);
            let frodo = Address::generate(&e);

            let (_, backstop_token_client) = create_backstop_token(&e, &backstop_id, &bombadil);
            backstop_token_client.mint(&frodo, &100_0000000);

            let (_, mock_pool_factory_client) = create_mock_pool_factory(&e, &backstop_id);
            mock_pool_factory_client.set_pool(&pool_0_id);

            // initialize pool 0 with funds
            e.as_contract(&backstop_id, || {
                execute_deposit(&e, &frodo, &pool_0_id, 50_0000000);
            });

            e.as_contract(&backstop_id, || {
                execute_draw(&e, &pool_0_id, -30_0000000, &samwise);
            });
        }
    }
}

mod backstop_src_backstop_pool {
    use soroban_fixed_point_math::FixedPoint;

    use soroban_sdk::{contracttype, panic_with_error, unwrap::UnwrapOptimized, Address, Env};

    use crate::{
        constants::SCALAR_7,
        dependencies::{CometClient, PoolFactoryClient},
        errors::BackstopError,
        storage,
    };

    pub(crate) use crate::backstop::pool::*;

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
}

mod backstop_src_backstop_deposit {
    use crate::{contract::require_nonnegative, emissions, storage, BackstopError};

    use sep_41_token::TokenClient;

    use soroban_sdk::{panic_with_error, Address, Env};

    use crate::backstop::require_is_from_pool_factory;

    pub(crate) use crate::backstop::deposit::*;

    mod tests {
        use soroban_sdk::{testutils::Address as _, Address};

        use crate::{
            backstop::{execute_donate, execute_draw},
            constants::SCALAR_7,
            testutils::{create_backstop, create_backstop_token, create_mock_pool_factory},
        };

        use super::*;

        #[test]
        fn test_execute_deposit() {
            let e = Env::default();
            e.cost_estimate().budget().reset_unlimited();
            e.mock_all_auths_allowing_non_root_auth();

            let backstop_address = create_backstop(&e);
            let bombadil = Address::generate(&e);
            let samwise = Address::generate(&e);
            let frodo = Address::generate(&e);
            let pool_0_id = Address::generate(&e);
            let pool_1_id = Address::generate(&e);

            let (_, backstop_token_client) =
                create_backstop_token(&e, &backstop_address, &bombadil);
            backstop_token_client.mint(&samwise, &100_0000000);
            backstop_token_client.mint(&frodo, &100_0000000);

            let (_, mock_pool_factory_client) = create_mock_pool_factory(&e, &backstop_address);
            mock_pool_factory_client.set_pool(&pool_0_id);
            mock_pool_factory_client.set_pool(&pool_1_id);

            backstop_token_client.approve(
                &frodo,
                &backstop_address,
                &25_0000000,
                &e.ledger().sequence(),
            );
            // initialize pool 0 with funds + some profit
            e.as_contract(&backstop_address, || {
                execute_deposit(&e, &frodo, &pool_0_id, 25_0000000);
                execute_donate(&e, &frodo, &pool_0_id, 25_0000000);
            });

            e.as_contract(&backstop_address, || {
                let shares_0 = execute_deposit(&e, &samwise, &pool_0_id, 30_0000000);
                let shares_1 = execute_deposit(&e, &samwise, &pool_1_id, 70_0000000);

                let new_pool_0_balance = storage::get_pool_balance(&e, &pool_0_id);
                assert_eq!(new_pool_0_balance.shares, 40_0000000);
                assert_eq!(new_pool_0_balance.tokens, 80_0000000);
                assert_eq!(new_pool_0_balance.q4w, 0);

                let new_user_balance_0 = storage::get_user_balance(&e, &pool_0_id, &samwise);
                assert_eq!(new_user_balance_0.shares, shares_0);
                assert_eq!(shares_0, 15_0000000);

                let new_pool_1_balance = storage::get_pool_balance(&e, &pool_1_id);
                assert_eq!(new_pool_1_balance.shares, 70_0000000);
                assert_eq!(new_pool_1_balance.tokens, 70_0000000);
                assert_eq!(new_pool_1_balance.q4w, 0);

                let new_user_balance_1 = storage::get_user_balance(&e, &pool_1_id, &samwise);
                assert_eq!(new_user_balance_1.shares, shares_1);
                assert_eq!(shares_1, 70_0000000);

                assert_eq!(
                    backstop_token_client.balance(&backstop_address),
                    150_0000000
                );
                assert_eq!(backstop_token_client.balance(&samwise), 0);
            });
        }

        #[test]
        #[should_panic]
        fn test_execute_deposit_too_many_tokens() {
            let e = Env::default();
            e.mock_all_auths_allowing_non_root_auth();

            let backstop_address = create_backstop(&e);
            let pool_0_id = Address::generate(&e);
            let bombadil = Address::generate(&e);
            let samwise = Address::generate(&e);

            let (_, backstop_token_client) =
                create_backstop_token(&e, &backstop_address, &bombadil);
            backstop_token_client.mint(&samwise, &100_0000000);

            let (_, mock_pool_factory_client) = create_mock_pool_factory(&e, &backstop_address);
            mock_pool_factory_client.set_pool(&pool_0_id);

            e.as_contract(&backstop_address, || {
                execute_deposit(&e, &samwise, &pool_0_id, 100_0000001);

                assert!(false);
            });
        }

        #[test]
        #[should_panic(expected = "Error(Contract, #8)")]
        fn test_execute_deposit_negative_tokens() {
            let e = Env::default();
            e.mock_all_auths_allowing_non_root_auth();

            let backstop_address = create_backstop(&e);
            let pool_0_id = Address::generate(&e);
            let bombadil = Address::generate(&e);
            let samwise = Address::generate(&e);

            let (_, backstop_token_client) =
                create_backstop_token(&e, &backstop_address, &bombadil);
            backstop_token_client.mint(&samwise, &100_0000000);

            let (_, mock_pool_factory_client) = create_mock_pool_factory(&e, &backstop_address);
            mock_pool_factory_client.set_pool(&pool_0_id);

            e.as_contract(&backstop_address, || {
                execute_deposit(&e, &samwise, &pool_0_id, -100);
            });
        }

        #[test]
        #[should_panic(expected = "Error(Contract, #1000)")]
        fn test_execute_deposit_from_is_to() {
            let e = Env::default();
            e.mock_all_auths_allowing_non_root_auth();

            let backstop_address = create_backstop(&e);
            let pool_0_id = Address::generate(&e);
            let bombadil = Address::generate(&e);
            let samwise = Address::generate(&e);

            let (_, backstop_token_client) =
                create_backstop_token(&e, &backstop_address, &bombadil);
            backstop_token_client.mint(&samwise, &100_0000000);

            let (_, mock_pool_factory_client) = create_mock_pool_factory(&e, &backstop_address);
            mock_pool_factory_client.set_pool(&pool_0_id);

            e.as_contract(&backstop_address, || {
                execute_deposit(&e, &pool_0_id, &pool_0_id, 100);
            });
        }

        #[test]
        #[should_panic(expected = "Error(Contract, #1000)")]
        fn test_execute_deposit_from_self() {
            let e = Env::default();
            e.mock_all_auths_allowing_non_root_auth();

            let backstop_address = create_backstop(&e);
            let pool_0_id = Address::generate(&e);
            let bombadil = Address::generate(&e);
            let samwise = Address::generate(&e);

            let (_, backstop_token_client) =
                create_backstop_token(&e, &backstop_address, &bombadil);
            backstop_token_client.mint(&samwise, &100_0000000);

            let (_, mock_pool_factory_client) = create_mock_pool_factory(&e, &backstop_address);
            mock_pool_factory_client.set_pool(&pool_0_id);

            e.as_contract(&backstop_address, || {
                execute_deposit(&e, &backstop_address, &pool_0_id, 100);
            });
        }

        #[test]
        #[should_panic(expected = "Error(Contract, #1004)")]
        fn text_execute_deposit_not_pool() {
            let e = Env::default();
            e.mock_all_auths_allowing_non_root_auth();

            let backstop_address = create_backstop(&e);
            let pool_0_id = Address::generate(&e);
            let bombadil = Address::generate(&e);
            let samwise = Address::generate(&e);

            let (_, backstop_token_client) =
                create_backstop_token(&e, &backstop_address, &bombadil);
            backstop_token_client.mint(&samwise, &100_0000000);

            create_mock_pool_factory(&e, &backstop_address);

            e.as_contract(&backstop_address, || {
                execute_deposit(&e, &samwise, &pool_0_id, 100);
            });
        }

        #[test]
        #[should_panic(expected = "Error(Contract, #1005)")]
        fn test_execute_deposit_zero_share_mint() {
            let e = Env::default();
            e.cost_estimate().budget().reset_unlimited();
            e.mock_all_auths_allowing_non_root_auth();

            let backstop_address = create_backstop(&e);
            let bombadil = Address::generate(&e);
            let samwise = Address::generate(&e);
            let frodo = Address::generate(&e);
            let pool_0_id = Address::generate(&e);
            let pool_1_id = Address::generate(&e);

            let (_, backstop_token_client) =
                create_backstop_token(&e, &backstop_address, &bombadil);
            backstop_token_client.mint(&samwise, &100_0000000);
            backstop_token_client.mint(&frodo, &100_000_000_0000000);

            let (_, mock_pool_factory_client) = create_mock_pool_factory(&e, &backstop_address);
            mock_pool_factory_client.set_pool(&pool_0_id);
            mock_pool_factory_client.set_pool(&pool_1_id);

            backstop_token_client.approve(
                &frodo,
                &backstop_address,
                &(10_000_000 * SCALAR_7),
                &e.ledger().sequence(),
            );
            // initialize pool 0 with funds + some profit
            e.as_contract(&backstop_address, || {
                execute_deposit(&e, &frodo, &pool_0_id, SCALAR_7);
                execute_donate(&e, &frodo, &pool_0_id, 10_000_000 * SCALAR_7);
            });

            e.as_contract(&backstop_address, || {
                execute_deposit(&e, &samwise, &pool_0_id, SCALAR_7);
            });
        }

        #[test]
        #[should_panic(expected = "Error(Contract, #1005)")]
        fn test_execute_deposit_drained_backstop() {
            let e = Env::default();
            e.cost_estimate().budget().reset_unlimited();
            e.mock_all_auths_allowing_non_root_auth();

            let backstop_address = create_backstop(&e);
            let bombadil = Address::generate(&e);
            let samwise = Address::generate(&e);
            let frodo = Address::generate(&e);
            let pool_0_id = Address::generate(&e);
            let pool_1_id = Address::generate(&e);

            let (_, backstop_token_client) =
                create_backstop_token(&e, &backstop_address, &bombadil);
            backstop_token_client.mint(&samwise, &100_0000000);
            backstop_token_client.mint(&frodo, &100_000_000_0000000);

            let (_, mock_pool_factory_client) = create_mock_pool_factory(&e, &backstop_address);
            mock_pool_factory_client.set_pool(&pool_0_id);
            mock_pool_factory_client.set_pool(&pool_1_id);

            backstop_token_client.approve(
                &frodo,
                &backstop_address,
                &(10_000_000 * SCALAR_7),
                &e.ledger().sequence(),
            );
            // initialize pool 0 with funds than drain the backstop
            e.as_contract(&backstop_address, || {
                execute_deposit(&e, &frodo, &pool_0_id, SCALAR_7);
                execute_draw(&e, &pool_0_id, SCALAR_7, &frodo);
            });

            e.as_contract(&backstop_address, || {
                execute_deposit(&e, &samwise, &pool_0_id, SCALAR_7);
            });
        }
    }
}

mod backstop_src_emissions_claim {
    use crate::{
        dependencies::CometClient, errors::BackstopError, events::BackstopEvents, storage,
    };

    use soroban_fixed_point_math::FixedPoint;

    use soroban_sdk::{
        auth::{ContractContext, InvokerContractAuthEntry, SubContractInvocation},
        panic_with_error,
        unwrap::UnwrapOptimized,
        vec, Address, Env, IntoVal, Map, Symbol, Val, Vec,
    };

    use crate::emissions::distributor::claim_emissions;

    pub(crate) use crate::emissions::claim::*;

    mod tests {
        use crate::{
            backstop::{PoolBalance, UserBalance},
            storage::{BackstopEmissionData, UserEmissionData},
            testutils::{
                create_backstop, create_blnd_token, create_comet_lp_pool, create_usdc_token,
            },
        };

        use super::*;
        use soroban_sdk::{
            testutils::{Address as _, Ledger, LedgerInfo},
            unwrap::UnwrapOptimized,
            vec,
        };

        /********** claim **********/

        #[test]
        fn test_claim() {
            let e = Env::default();
            e.mock_all_auths();
            let block_timestamp = 1500000000 + 12345;
            e.ledger().set(LedgerInfo {
                timestamp: block_timestamp,
                protocol_version: 22,
                sequence_number: 0,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 3110400,
            });
            e.cost_estimate().budget().reset_unlimited();

            let backstop_address = create_backstop(&e);
            let pool_1_id = Address::generate(&e);
            let pool_2_id = Address::generate(&e);
            let bombadil = Address::generate(&e);
            let samwise = Address::generate(&e);

            let (blnd_address, blnd_token_client) =
                create_blnd_token(&e, &backstop_address, &bombadil);
            let (usdc_address, _) = create_usdc_token(&e, &backstop_address, &bombadil);
            blnd_token_client.mint(&backstop_address, &100_0000000);

            let backstop_1_emissions_data = BackstopEmissionData {
                expiration: 1500000000 + 7 * 24 * 60 * 60,
                eps: 0_10000000000000,
                index: 222220000000,
                last_time: 1500000000,
            };
            let user_1_emissions_data = UserEmissionData {
                index: 111110000000,
                accrued: 1_2345678,
            };

            let backstop_2_emissions_data = BackstopEmissionData {
                expiration: 1500000000 + 7 * 24 * 60 * 60,
                eps: 0_02000000000000,
                index: 0,
                last_time: 1500010000,
            };
            let user_2_emissions_data = UserEmissionData {
                index: 0,
                accrued: 0,
            };
            let (lp_address, lp_client) =
                create_comet_lp_pool(&e, &bombadil, &blnd_address, &usdc_address);
            e.as_contract(&backstop_address, || {
                storage::set_backstop_emis_data(&e, &pool_1_id, &backstop_1_emissions_data);
                storage::set_user_emis_data(&e, &pool_1_id, &samwise, &user_1_emissions_data);
                storage::set_backstop_emis_data(&e, &pool_2_id, &backstop_2_emissions_data);
                storage::set_user_emis_data(&e, &pool_2_id, &samwise, &user_2_emissions_data);
                storage::set_backstop_token(&e, &lp_address);
                storage::set_blnd_token(&e, &blnd_address);
                storage::set_pool_balance(
                    &e,
                    &pool_1_id,
                    &PoolBalance {
                        shares: 150_0000000,
                        tokens: 200_0000000,
                        q4w: 2_0000000,
                    },
                );
                storage::set_user_balance(
                    &e,
                    &pool_1_id,
                    &samwise,
                    &UserBalance {
                        shares: 9_0000000,
                        q4w: vec![&e],
                    },
                );
                storage::set_pool_balance(
                    &e,
                    &pool_2_id,
                    &PoolBalance {
                        shares: 70_0000000,
                        tokens: 75_0000000,
                        q4w: 3_5000000,
                    },
                );
                storage::set_user_balance(
                    &e,
                    &pool_2_id,
                    &samwise,
                    &UserBalance {
                        shares: 7_5000000,
                        q4w: vec![&e],
                    },
                );
                let backstop_lp_balance = lp_client.balance(&backstop_address);
                let pre_pool_tokens_1 = storage::get_pool_balance(&e, &pool_1_id).tokens;
                let pre_pool_tokens_2 = storage::get_pool_balance(&e, &pool_2_id).tokens;
                let pre_pool_shares_1 = storage::get_pool_balance(&e, &pool_1_id).shares;
                let pre_pool_shares_2 = storage::get_pool_balance(&e, &pool_2_id).shares;
                let result = execute_claim(
                    &e,
                    &samwise,
                    &vec![&e, pool_1_id.clone(), pool_2_id.clone()],
                    &6_4000000,
                );
                assert_eq!(result, 6_4729327);
                assert_eq!(
                    lp_client.balance(&backstop_address),
                    backstop_lp_balance + 6_4729327
                );
                assert_eq!(
                    blnd_token_client.balance(&backstop_address),
                    100_0000000 - (76_3155136 + 5_2894736)
                );
                let sam_balance_1 = storage::get_user_balance(&e, &pool_1_id, &samwise);
                assert_eq!(sam_balance_1.shares, 9_0000000 + 4_5400275);
                let sam_balance_2 = storage::get_user_balance(&e, &pool_2_id, &samwise);
                assert_eq!(sam_balance_2.shares, 7_5000000 + 0_3915917);

                let pool_balance_1 = storage::get_pool_balance(&e, &pool_1_id);
                assert_eq!(pool_balance_1.tokens, pre_pool_tokens_1 + 6_0533700);
                assert_eq!(pool_balance_1.shares, pre_pool_shares_1 + 4_5400275);
                let pool_balance_2 = storage::get_pool_balance(&e, &pool_2_id);
                assert_eq!(pool_balance_2.tokens, pre_pool_tokens_2 + 0_4195626);
                assert_eq!(pool_balance_2.shares, pre_pool_shares_2 + 0_3915917);

                let new_backstop_1_data =
                    storage::get_backstop_emis_data(&e, &pool_1_id).unwrap_optimized();
                let new_user_1_data =
                    storage::get_user_emis_data(&e, &pool_1_id, &samwise).unwrap_optimized();
                assert_eq!(new_backstop_1_data.last_time, block_timestamp);
                assert_eq!(new_backstop_1_data.index, 834343841621621);
                assert_eq!(new_user_1_data.accrued, 0);
                assert_eq!(new_user_1_data.index, 834343841621621);

                let new_backstop_2_data =
                    storage::get_backstop_emis_data(&e, &pool_2_id).unwrap_optimized();
                let new_user_2_data =
                    storage::get_user_emis_data(&e, &pool_2_id, &samwise).unwrap_optimized();
                assert_eq!(new_backstop_2_data.last_time, block_timestamp);
                assert_eq!(new_backstop_2_data.index, 70526315789473);
                assert_eq!(new_user_2_data.accrued, 0);
                assert_eq!(new_user_2_data.index, 70526315789473);
            });
        }

        #[test]
        #[should_panic(expected = "Error(Contract, #20)")]
        fn test_claim_uses_min_lp_amount() {
            let e = Env::default();
            e.mock_all_auths();
            let block_timestamp = 1500000000 + 12345;
            e.ledger().set(LedgerInfo {
                timestamp: block_timestamp,
                protocol_version: 22,
                sequence_number: 0,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 3110400,
            });
            e.cost_estimate().budget().reset_unlimited();

            let backstop_address = create_backstop(&e);
            let pool_1_id = Address::generate(&e);
            let pool_2_id = Address::generate(&e);
            let bombadil = Address::generate(&e);
            let samwise = Address::generate(&e);

            let (blnd_address, blnd_token_client) =
                create_blnd_token(&e, &backstop_address, &bombadil);
            let (usdc_address, _) = create_usdc_token(&e, &backstop_address, &bombadil);
            blnd_token_client.mint(&backstop_address, &100_0000000);

            let backstop_1_emissions_data = BackstopEmissionData {
                expiration: 1500000000 + 7 * 24 * 60 * 60,
                eps: 0_10000000000000,
                index: 222220000000,
                last_time: 1500000000,
            };
            let user_1_emissions_data = UserEmissionData {
                index: 111110000000,
                accrued: 1_2345678,
            };

            let backstop_2_emissions_data = BackstopEmissionData {
                expiration: 1500000000 + 7 * 24 * 60 * 60,
                eps: 0_02000000000000,
                index: 0,
                last_time: 1500010000,
            };
            let user_2_emissions_data = UserEmissionData {
                index: 0,
                accrued: 0,
            };
            let (lp_address, _) = create_comet_lp_pool(&e, &bombadil, &blnd_address, &usdc_address);
            e.as_contract(&backstop_address, || {
                storage::set_backstop_emis_data(&e, &pool_1_id, &backstop_1_emissions_data);
                storage::set_user_emis_data(&e, &pool_1_id, &samwise, &user_1_emissions_data);
                storage::set_backstop_emis_data(&e, &pool_2_id, &backstop_2_emissions_data);
                storage::set_user_emis_data(&e, &pool_2_id, &samwise, &user_2_emissions_data);
                storage::set_backstop_token(&e, &lp_address);
                storage::set_blnd_token(&e, &blnd_address);
                storage::set_pool_balance(
                    &e,
                    &pool_1_id,
                    &PoolBalance {
                        shares: 150_0000000,
                        tokens: 200_0000000,
                        q4w: 2_0000000,
                    },
                );
                storage::set_user_balance(
                    &e,
                    &pool_1_id,
                    &samwise,
                    &UserBalance {
                        shares: 9_0000000,
                        q4w: vec![&e],
                    },
                );
                storage::set_pool_balance(
                    &e,
                    &pool_2_id,
                    &PoolBalance {
                        shares: 70_0000000,
                        tokens: 75_0000000,
                        q4w: 3_5000000,
                    },
                );
                storage::set_user_balance(
                    &e,
                    &pool_2_id,
                    &samwise,
                    &UserBalance {
                        shares: 7_5000000,
                        q4w: vec![&e],
                    },
                );
                execute_claim(
                    &e,
                    &samwise,
                    &vec![&e, pool_1_id.clone(), pool_2_id.clone()],
                    &6_5000000,
                );
            });
        }

        #[test]
        fn test_claim_twice() {
            let e = Env::default();
            e.cost_estimate().budget().reset_unlimited();
            e.mock_all_auths();

            let block_timestamp = 1500000000 + 12345;
            e.ledger().set(LedgerInfo {
                timestamp: block_timestamp,
                protocol_version: 22,
                sequence_number: 0,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 3110400,
            });

            let backstop_address = create_backstop(&e);
            let pool_1_id = Address::generate(&e);
            let pool_2_id = Address::generate(&e);
            let bombadil = Address::generate(&e);
            let samwise = Address::generate(&e);

            let (blnd_address, blnd_token_client) =
                create_blnd_token(&e, &backstop_address, &bombadil);
            let (usdc_address, _) = create_usdc_token(&e, &backstop_address, &bombadil);
            blnd_token_client.mint(&backstop_address, &300_0000000);

            let backstop_1_emissions_data = BackstopEmissionData {
                expiration: 1500000000 + 7 * 24 * 60 * 60,
                eps: 0_10000000000000,
                index: 222220000000,
                last_time: 1500000000,
            };
            let user_1_emissions_data = UserEmissionData {
                index: 111110000000,
                accrued: 1_2345678,
            };

            let backstop_2_emissions_data = BackstopEmissionData {
                expiration: 1500000000 + 7 * 24 * 60 * 60,
                eps: 0_02000000000000,
                index: 0,
                last_time: 1500010000,
            };
            let user_2_emissions_data = UserEmissionData {
                index: 0,
                accrued: 0,
            };
            let (lp_address, lp_client) =
                create_comet_lp_pool(&e, &bombadil, &blnd_address, &usdc_address);
            e.as_contract(&backstop_address, || {
                storage::set_backstop_emis_data(&e, &pool_1_id, &backstop_1_emissions_data);
                storage::set_user_emis_data(&e, &pool_1_id, &samwise, &user_1_emissions_data);
                storage::set_backstop_emis_data(&e, &pool_2_id, &backstop_2_emissions_data);
                storage::set_user_emis_data(&e, &pool_2_id, &samwise, &user_2_emissions_data);
                storage::set_backstop_token(&e, &lp_address);
                storage::set_blnd_token(&e, &blnd_address);
                storage::set_pool_balance(
                    &e,
                    &pool_1_id,
                    &PoolBalance {
                        shares: 150_0000000,
                        tokens: 200_0000000,
                        q4w: 2_0000000,
                    },
                );
                storage::set_user_balance(
                    &e,
                    &pool_1_id,
                    &samwise,
                    &UserBalance {
                        shares: 9_0000000,
                        q4w: vec![&e],
                    },
                );
                storage::set_pool_balance(
                    &e,
                    &pool_2_id,
                    &PoolBalance {
                        shares: 70_0000000,
                        tokens: 75_0000000,
                        q4w: 3_5000000,
                    },
                );
                storage::set_user_balance(
                    &e,
                    &pool_2_id,
                    &samwise,
                    &UserBalance {
                        shares: 7_5000000,
                        q4w: vec![&e],
                    },
                );
                let backstop_lp_balance = lp_client.balance(&backstop_address);
                let pre_pool_tokens_1 = storage::get_pool_balance(&e, &pool_1_id).tokens;
                let pre_pool_tokens_2 = storage::get_pool_balance(&e, &pool_2_id).tokens;
                let pre_pool_shares_1 = storage::get_pool_balance(&e, &pool_1_id).shares;
                let pre_pool_shares_2 = storage::get_pool_balance(&e, &pool_2_id).shares;
                let result = execute_claim(
                    &e,
                    &samwise,
                    &vec![&e, pool_1_id.clone(), pool_2_id.clone()],
                    &6_4000000,
                );
                assert_eq!(result, 6_4729327);
                assert_eq!(
                    lp_client.balance(&backstop_address),
                    backstop_lp_balance + 6_4729327
                );
                assert_eq!(
                    blnd_token_client.balance(&backstop_address),
                    300_0000000 - (76_3155136 + 5_2894736)
                );
                let sam_balance_1 = storage::get_user_balance(&e, &pool_1_id, &samwise);
                assert_eq!(sam_balance_1.shares, 9_0000000 + 4_5400275);
                let sam_balance_2 = storage::get_user_balance(&e, &pool_2_id, &samwise);
                assert_eq!(sam_balance_2.shares, 7_5000000 + 0_3915917);

                let pool_balance_1 = storage::get_pool_balance(&e, &pool_1_id);
                assert_eq!(pool_balance_1.tokens, pre_pool_tokens_1 + 6_0533700);
                assert_eq!(pool_balance_1.shares, pre_pool_shares_1 + 4_5400275);
                let pool_balance_2 = storage::get_pool_balance(&e, &pool_2_id);
                assert_eq!(pool_balance_2.tokens, pre_pool_tokens_2 + 0_4195626);
                assert_eq!(pool_balance_2.shares, pre_pool_shares_2 + 0_3915917);

                let new_backstop_1_data =
                    storage::get_backstop_emis_data(&e, &pool_1_id).unwrap_optimized();
                let new_user_1_data =
                    storage::get_user_emis_data(&e, &pool_1_id, &samwise).unwrap_optimized();
                assert_eq!(new_backstop_1_data.last_time, block_timestamp);
                assert_eq!(new_backstop_1_data.index, 834343841621621);
                assert_eq!(new_user_1_data.accrued, 0);
                assert_eq!(new_user_1_data.index, 834343841621621);

                let new_backstop_2_data =
                    storage::get_backstop_emis_data(&e, &pool_2_id).unwrap_optimized();
                let new_user_2_data =
                    storage::get_user_emis_data(&e, &pool_2_id, &samwise).unwrap_optimized();
                assert_eq!(new_backstop_2_data.last_time, block_timestamp);
                assert_eq!(new_backstop_2_data.index, 70526315789473);
                assert_eq!(new_user_2_data.accrued, 0);
                assert_eq!(new_user_2_data.index, 70526315789473);

                let block_timestamp_1 = 1500000000 + 12345 + 12345;
                e.ledger().set(LedgerInfo {
                    timestamp: block_timestamp_1,
                    protocol_version: 22,
                    sequence_number: 0,
                    network_id: Default::default(),
                    base_reserve: 10,
                    min_temp_entry_ttl: 10,
                    min_persistent_entry_ttl: 10,
                    max_entry_ttl: 3110400,
                });
                let backstop_lp_balance = lp_client.balance(&backstop_address);
                let pre_samwise_balance_1 =
                    storage::get_user_balance(&e, &pool_1_id, &samwise).shares;
                let pre_samwise_balance_2 =
                    storage::get_user_balance(&e, &pool_2_id, &samwise).shares;
                let pre_pool_tokens_1 = storage::get_pool_balance(&e, &pool_1_id).tokens;
                let pre_pool_tokens_2 = storage::get_pool_balance(&e, &pool_2_id).tokens;
                let pre_pool_shares_1 = storage::get_pool_balance(&e, &pool_1_id).shares;
                let pre_pool_shares_2 = storage::get_pool_balance(&e, &pool_2_id).shares;
                let result_1 = execute_claim(
                    &e,
                    &samwise,
                    &vec![&e, pool_1_id.clone(), pool_2_id.clone()],
                    &10_7000000,
                );
                assert_eq!(result_1, 10_7836702);
                assert_eq!(
                    blnd_token_client.balance(&backstop_address),
                    300_0000000 - (109_5788706 + 29_1282348) - (76_3155136 + 5_2894736)
                );
                assert_eq!(
                    lp_client.balance(&backstop_address),
                    backstop_lp_balance + 8_5191194 + 2_2645507 + 1
                );
                let sam_balance_1 = storage::get_user_balance(&e, &pool_1_id, &samwise);
                assert_eq!(sam_balance_1.shares, pre_samwise_balance_1 + 6_3893395);
                let sam_balance_2 = storage::get_user_balance(&e, &pool_2_id, &samwise);
                assert_eq!(sam_balance_2.shares, pre_samwise_balance_2 + 2_1135806);

                let pool_balance_1 = storage::get_pool_balance(&e, &pool_1_id);
                assert_eq!(pool_balance_1.tokens, pre_pool_tokens_1 + 8_5191194);
                assert_eq!(pool_balance_1.shares, pre_pool_shares_1 + 6_3893395);
                let pool_balance_2 = storage::get_pool_balance(&e, &pool_2_id);
                assert_eq!(pool_balance_2.tokens, pre_pool_tokens_2 + 2_2645507);
                assert_eq!(pool_balance_2.shares, pre_pool_shares_2 + 2_1135806);
                let new_backstop_1_data =
                    storage::get_backstop_emis_data(&e, &pool_1_id).unwrap_optimized();
                let new_user_1_data =
                    storage::get_user_emis_data(&e, &pool_1_id, &samwise).unwrap_optimized();
                assert_eq!(new_backstop_1_data.last_time, block_timestamp_1);
                assert_eq!(new_backstop_1_data.index, 1643639618102322);
                assert_eq!(new_user_1_data.accrued, 0);
                assert_eq!(new_user_1_data.index, 1643639618102322);

                let new_backstop_2_data =
                    storage::get_backstop_emis_data(&e, &pool_2_id).unwrap_optimized();
                let new_user_2_data =
                    storage::get_user_emis_data(&e, &pool_2_id, &samwise).unwrap_optimized();
                assert_eq!(new_backstop_2_data.last_time, block_timestamp_1);
                assert_eq!(new_backstop_2_data.index, 439631002529944);
                assert_eq!(new_user_2_data.accrued, 0);
                assert_eq!(new_user_2_data.index, 439631002529944);
            });
        }

        #[test]
        fn test_claim_no_deposits() {
            let e = Env::default();
            e.mock_all_auths();
            let block_timestamp = 1500000000 + 12345;
            e.ledger().set(LedgerInfo {
                timestamp: block_timestamp,
                protocol_version: 22,
                sequence_number: 0,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 3110400,
            });

            let backstop_address = create_backstop(&e);
            let pool_1_id = Address::generate(&e);
            let pool_2_id = Address::generate(&e);
            let bombadil = Address::generate(&e);
            let samwise = Address::generate(&e);
            let frodo = Address::generate(&e);

            let (_, blnd_token_client) = create_blnd_token(&e, &backstop_address, &bombadil);
            blnd_token_client.mint(&backstop_address, &100_0000000);

            let backstop_1_emissions_data = BackstopEmissionData {
                expiration: 1500000000 + 7 * 24 * 60 * 60,
                eps: 0_10000000000000,
                index: 222220000000,
                last_time: 1500000000,
            };

            let backstop_2_emissions_data = BackstopEmissionData {
                expiration: 1500000000 + 7 * 24 * 60 * 60,
                eps: 0_02000000000000,
                index: 0,
                last_time: 1500010000,
            };
            e.as_contract(&backstop_address, || {
                storage::set_backstop_emis_data(&e, &pool_1_id, &backstop_1_emissions_data);
                storage::set_backstop_emis_data(&e, &pool_2_id, &backstop_2_emissions_data);

                storage::set_pool_balance(
                    &e,
                    &pool_1_id,
                    &PoolBalance {
                        shares: 150_0000000,
                        tokens: 200_0000000,
                        q4w: 0,
                    },
                );
                storage::set_pool_balance(
                    &e,
                    &pool_2_id,
                    &PoolBalance {
                        shares: 70_0000000,
                        tokens: 75_0000000,
                        q4w: 0,
                    },
                );

                let result = execute_claim(
                    &e,
                    &samwise,
                    &vec![&e, pool_1_id.clone(), pool_2_id.clone()],
                    &0,
                );
                assert_eq!(result, 0);
                assert_eq!(blnd_token_client.balance(&frodo), 0);
                assert_eq!(blnd_token_client.balance(&backstop_address), 100_0000000);

                let new_backstop_1_data =
                    storage::get_backstop_emis_data(&e, &pool_1_id).unwrap_optimized();
                let new_user_1_data =
                    storage::get_user_emis_data(&e, &pool_1_id, &samwise).unwrap_optimized();
                assert_eq!(new_backstop_1_data.last_time, block_timestamp);
                assert_eq!(new_backstop_1_data.index, 823222220000000);
                assert_eq!(new_user_1_data.accrued, 0);
                assert_eq!(new_user_1_data.index, 823222220000000);

                let new_backstop_2_data =
                    storage::get_backstop_emis_data(&e, &pool_2_id).unwrap_optimized();
                let new_user_2_data =
                    storage::get_user_emis_data(&e, &pool_2_id, &samwise).unwrap_optimized();
                assert_eq!(new_backstop_2_data.last_time, block_timestamp);
                assert_eq!(new_backstop_2_data.index, 67000000000000);
                assert_eq!(new_user_2_data.accrued, 0);
                assert_eq!(new_user_2_data.index, 67000000000000);
            });
        }

        #[test]
        #[should_panic(expected = "Error(Contract, #1000)")]
        fn test_claim_duplicate() {
            let e = Env::default();
            e.mock_all_auths();
            let block_timestamp = 1500000000 + 12345;
            e.ledger().set(LedgerInfo {
                timestamp: block_timestamp,
                protocol_version: 22,
                sequence_number: 0,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 3110400,
            });
            e.cost_estimate().budget().reset_unlimited();

            let backstop_address = create_backstop(&e);
            let pool_1_id = Address::generate(&e);
            let pool_2_id = Address::generate(&e);
            let bombadil = Address::generate(&e);
            let samwise = Address::generate(&e);

            let (blnd_address, blnd_token_client) =
                create_blnd_token(&e, &backstop_address, &bombadil);
            let (usdc_address, _) = create_usdc_token(&e, &backstop_address, &bombadil);
            blnd_token_client.mint(&backstop_address, &100_0000000);

            let backstop_1_emissions_data = BackstopEmissionData {
                expiration: 1500000000 + 7 * 24 * 60 * 60,
                eps: 0_10000000000000,
                index: 222220000000,
                last_time: 1500000000,
            };
            let user_1_emissions_data = UserEmissionData {
                index: 111110000000,
                accrued: 1_2345678,
            };

            let backstop_2_emissions_data = BackstopEmissionData {
                expiration: 1500000000 + 7 * 24 * 60 * 60,
                eps: 0_02000000000000,
                index: 0,
                last_time: 1500010000,
            };
            let user_2_emissions_data = UserEmissionData {
                index: 0,
                accrued: 0,
            };
            let (lp_address, _) = create_comet_lp_pool(&e, &bombadil, &blnd_address, &usdc_address);
            e.as_contract(&backstop_address, || {
                storage::set_backstop_emis_data(&e, &pool_1_id, &backstop_1_emissions_data);
                storage::set_user_emis_data(&e, &pool_1_id, &samwise, &user_1_emissions_data);
                storage::set_backstop_emis_data(&e, &pool_2_id, &backstop_2_emissions_data);
                storage::set_user_emis_data(&e, &pool_2_id, &samwise, &user_2_emissions_data);
                storage::set_backstop_token(&e, &lp_address);
                storage::set_blnd_token(&e, &blnd_address);
                storage::set_pool_balance(
                    &e,
                    &pool_1_id,
                    &PoolBalance {
                        shares: 150_0000000,
                        tokens: 200_0000000,
                        q4w: 2_0000000,
                    },
                );
                storage::set_user_balance(
                    &e,
                    &pool_1_id,
                    &samwise,
                    &UserBalance {
                        shares: 9_0000000,
                        q4w: vec![&e],
                    },
                );
                storage::set_pool_balance(
                    &e,
                    &pool_2_id,
                    &PoolBalance {
                        shares: 70_0000000,
                        tokens: 75_0000000,
                        q4w: 3_5000000,
                    },
                );
                storage::set_user_balance(
                    &e,
                    &pool_2_id,
                    &samwise,
                    &UserBalance {
                        shares: 7_5000000,
                        q4w: vec![&e],
                    },
                );
                execute_claim(
                    &e,
                    &samwise,
                    &vec![&e, pool_1_id.clone(), pool_2_id.clone(), pool_1_id.clone()],
                    &6_4000000,
                );
            });
        }

        #[test]
        #[should_panic(expected = "Error(Contract, #1000)")]
        fn test_claim_empty() {
            let e = Env::default();
            e.mock_all_auths();
            let block_timestamp = 1500000000 + 12345;
            e.ledger().set(LedgerInfo {
                timestamp: block_timestamp,
                protocol_version: 22,
                sequence_number: 0,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 3110400,
            });
            e.cost_estimate().budget().reset_unlimited();

            let backstop_address = create_backstop(&e);
            let pool_1_id = Address::generate(&e);
            let pool_2_id = Address::generate(&e);
            let bombadil = Address::generate(&e);
            let samwise = Address::generate(&e);

            let (blnd_address, blnd_token_client) =
                create_blnd_token(&e, &backstop_address, &bombadil);
            let (usdc_address, _) = create_usdc_token(&e, &backstop_address, &bombadil);
            blnd_token_client.mint(&backstop_address, &100_0000000);

            let backstop_1_emissions_data = BackstopEmissionData {
                expiration: 1500000000 + 7 * 24 * 60 * 60,
                eps: 0_10000000000000,
                index: 222220000000,
                last_time: 1500000000,
            };
            let user_1_emissions_data = UserEmissionData {
                index: 111110000000,
                accrued: 1_2345678,
            };

            let backstop_2_emissions_data = BackstopEmissionData {
                expiration: 1500000000 + 7 * 24 * 60 * 60,
                eps: 0_02000000000000,
                index: 0,
                last_time: 1500010000,
            };
            let user_2_emissions_data = UserEmissionData {
                index: 0,
                accrued: 0,
            };
            let (lp_address, _) = create_comet_lp_pool(&e, &bombadil, &blnd_address, &usdc_address);
            e.as_contract(&backstop_address, || {
                storage::set_backstop_emis_data(&e, &pool_1_id, &backstop_1_emissions_data);
                storage::set_user_emis_data(&e, &pool_1_id, &samwise, &user_1_emissions_data);
                storage::set_backstop_emis_data(&e, &pool_2_id, &backstop_2_emissions_data);
                storage::set_user_emis_data(&e, &pool_2_id, &samwise, &user_2_emissions_data);
                storage::set_backstop_token(&e, &lp_address);
                storage::set_blnd_token(&e, &blnd_address);
                storage::set_pool_balance(
                    &e,
                    &pool_1_id,
                    &PoolBalance {
                        shares: 150_0000000,
                        tokens: 200_0000000,
                        q4w: 2_0000000,
                    },
                );
                storage::set_user_balance(
                    &e,
                    &pool_1_id,
                    &samwise,
                    &UserBalance {
                        shares: 9_0000000,
                        q4w: vec![&e],
                    },
                );
                storage::set_pool_balance(
                    &e,
                    &pool_2_id,
                    &PoolBalance {
                        shares: 70_0000000,
                        tokens: 75_0000000,
                        q4w: 3_5000000,
                    },
                );
                storage::set_user_balance(
                    &e,
                    &pool_2_id,
                    &samwise,
                    &UserBalance {
                        shares: 7_5000000,
                        q4w: vec![&e],
                    },
                );
                execute_claim(&e, &samwise, &vec![&e], &6_4000000);
            });
        }

        #[test]
        #[should_panic(expected = "Error(Contract, #1000)")]
        fn test_claim_random_adddress() {
            let e = Env::default();
            e.mock_all_auths();
            let block_timestamp = 1500000000 + 12345;
            e.ledger().set(LedgerInfo {
                timestamp: block_timestamp,
                protocol_version: 22,
                sequence_number: 0,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 3110400,
            });
            e.cost_estimate().budget().reset_unlimited();

            let backstop_address = create_backstop(&e);
            let pool_1_id = Address::generate(&e);
            let pool_2_id = Address::generate(&e);
            let bombadil = Address::generate(&e);
            let samwise = Address::generate(&e);

            let (blnd_address, blnd_token_client) =
                create_blnd_token(&e, &backstop_address, &bombadil);
            let (usdc_address, _) = create_usdc_token(&e, &backstop_address, &bombadil);
            blnd_token_client.mint(&backstop_address, &100_0000000);

            let backstop_1_emissions_data = BackstopEmissionData {
                expiration: 1500000000 + 7 * 24 * 60 * 60,
                eps: 0_10000000000000,
                index: 222220000000,
                last_time: 1500000000,
            };
            let user_1_emissions_data = UserEmissionData {
                index: 111110000000,
                accrued: 1_2345678,
            };

            let backstop_2_emissions_data = BackstopEmissionData {
                expiration: 1500000000 + 7 * 24 * 60 * 60,
                eps: 0_02000000000000,
                index: 0,
                last_time: 1500010000,
            };
            let user_2_emissions_data = UserEmissionData {
                index: 0,
                accrued: 0,
            };
            let (lp_address, _) = create_comet_lp_pool(&e, &bombadil, &blnd_address, &usdc_address);
            e.as_contract(&backstop_address, || {
                storage::set_backstop_emis_data(&e, &pool_1_id, &backstop_1_emissions_data);
                storage::set_user_emis_data(&e, &pool_1_id, &samwise, &user_1_emissions_data);
                storage::set_backstop_emis_data(&e, &pool_2_id, &backstop_2_emissions_data);
                storage::set_user_emis_data(&e, &pool_2_id, &samwise, &user_2_emissions_data);
                storage::set_backstop_token(&e, &lp_address);
                storage::set_blnd_token(&e, &blnd_address);
                storage::set_pool_balance(
                    &e,
                    &pool_1_id,
                    &PoolBalance {
                        shares: 150_0000000,
                        tokens: 200_0000000,
                        q4w: 2_0000000,
                    },
                );
                storage::set_user_balance(
                    &e,
                    &pool_1_id,
                    &samwise,
                    &UserBalance {
                        shares: 9_0000000,
                        q4w: vec![&e],
                    },
                );
                storage::set_pool_balance(
                    &e,
                    &pool_2_id,
                    &PoolBalance {
                        shares: 70_0000000,
                        tokens: 75_0000000,
                        q4w: 3_5000000,
                    },
                );
                storage::set_user_balance(
                    &e,
                    &pool_2_id,
                    &samwise,
                    &UserBalance {
                        shares: 7_5000000,
                        q4w: vec![&e],
                    },
                );
                execute_claim(
                    &e,
                    &samwise,
                    &vec![&e, pool_1_id.clone(), Address::generate(&e)],
                    &1,
                );
            });
        }
    }
}

mod backstop_src_emissions_manager {
    use cast::{i128, u64};

    use sep_41_token::TokenClient;

    use soroban_fixed_point_math::FixedPoint;

    use soroban_sdk::{panic_with_error, unwrap::UnwrapOptimized, vec, Address, Env, Vec};

    use crate::{
        backstop::{is_pool_above_threshold, load_pool_backstop_data},
        constants::{MAX_BACKFILLED_EMISSIONS, MAX_RZ_SIZE, SCALAR_7},
        dependencies::EmitterClient,
        errors::BackstopError,
        storage::{self, BackstopEmissionData, RzEmissions},
        PoolBalance,
    };

    use crate::emissions::distributor::update_emission_data;

    pub(crate) use crate::emissions::manager::*;

    mod tests {
        use super::*;
        use soroban_sdk::{
            testutils::{Address as _, Ledger, LedgerInfo},
            vec, Vec,
        };

        use crate::{
            backstop::PoolBalance,
            testutils::{
                create_backstop, create_blnd_token, create_comet_lp_pool_with_tokens_per_share,
                create_emitter, create_usdc_token,
            },
        };

        /********** gulp_emissions **********/

        #[test]
        fn test_gulp_emissions_outside_rz() {
            let e = Env::default();
            e.cost_estimate().budget().reset_unlimited();

            e.ledger().set(LedgerInfo {
                timestamp: 1713139200,
                protocol_version: 22,
                sequence_number: 0,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 3110400,
            });

            let backstop = create_backstop(&e);
            let blnd_token_client = create_blnd_token(&e, &backstop, &Address::generate(&e)).1;
            let pool_1 = Address::generate(&e);
            let pool_2 = Address::generate(&e);
            let pool_3 = Address::generate(&e);
            let reward_zone: Vec<Address> = vec![&e, pool_2.clone(), pool_3.clone()];

            // setup pool 1 to have ongoing emissions - it was recently removed from RZ
            let pool_1_emissions_data = BackstopEmissionData {
                expiration: 1713139200 + 86400,
                eps: 0_10000000000000,
                index: 887766550000000,
                last_time: 1713139200 - 12345,
            };
            let pool_1_accrued = RzEmissions {
                accrued: 20_000_0000000,
                last_time: 0,
            };
            let pool_1_allowance: i128 = 100_123_0000000;
            e.as_contract(&backstop, || {
                storage::set_reward_zone(&e, &reward_zone);
                storage::set_backstop_emis_data(&e, &pool_1, &pool_1_emissions_data);
                storage::set_pool_balance(
                    &e,
                    &pool_1,
                    // 35_000_0000000 unqeued shares
                    &PoolBalance {
                        tokens: 150_000_0000000,
                        shares: 40_000_0000000,
                        q4w: 5_000_0000000,
                    },
                );
                blnd_token_client.approve(
                    &backstop,
                    &pool_1,
                    &pool_1_allowance,
                    &e.ledger().sequence(),
                );
                storage::set_rz_emis(&e, &pool_1, &pool_1_accrued);

                gulp_emissions(&e, &pool_1);

                assert_eq!(
                    blnd_token_client.allowance(&backstop, &pool_1),
                    pool_1_allowance + 6_000_0000000
                );
                let new_pool_1_data =
                    storage::get_backstop_emis_data(&e, &pool_1).unwrap_optimized();
                assert_eq!(new_pool_1_data.eps, 0_0374338_6243386);
                assert_eq!(new_pool_1_data.expiration, 1713139200 + 7 * 24 * 60 * 60);
                assert_eq!(
                    new_pool_1_data.index,
                    pool_1_emissions_data.index + 0_0352714_2857142
                );
                assert_eq!(new_pool_1_data.last_time, 1713139200);
                let rz_emis = storage::get_rz_emis(&e, &pool_1);
                assert_eq!(rz_emis.accrued, 0);
                assert_eq!(rz_emis.last_time, 1713139200);
            });
        }

        #[test]
        fn test_gulp_emissions() {
            let e = Env::default();
            e.cost_estimate().budget().reset_unlimited();

            e.ledger().set(LedgerInfo {
                timestamp: 1713139200,
                protocol_version: 22,
                sequence_number: 0,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 3110400,
            });

            let backstop = create_backstop(&e);
            let emitter_distro_time = 1713139200 - 10;
            let blnd_token_client = create_blnd_token(&e, &backstop, &Address::generate(&e)).1;
            create_emitter(
                &e,
                &backstop,
                &Address::generate(&e),
                &Address::generate(&e),
                emitter_distro_time,
            );
            let pool_1 = Address::generate(&e);
            let pool_2 = Address::generate(&e);
            let pool_3 = Address::generate(&e);
            let reward_zone: Vec<Address> =
                vec![&e, pool_1.clone(), pool_2.clone(), pool_3.clone()];

            // setup pool 1 to have ongoing emissions
            let pool_1_emissions_data = BackstopEmissionData {
                expiration: 1713139200 + 1000,
                eps: 0_10000000000000,
                index: 8877660000000,
                last_time: 1713139200 - 12345,
            };

            // setup pool 2 to have expired emissions
            let pool_2_emissions_data = BackstopEmissionData {
                expiration: 1713139200 - 12345,
                eps: 0_05000000000000,
                index: 4532340000000,
                last_time: 1713139200 - 12345,
            };
            // setup pool 3 to have no emissions
            e.as_contract(&backstop, || {
                storage::set_last_distribution_time(&e, &(emitter_distro_time - 7 * 24 * 60 * 60));
                storage::set_reward_zone(&e, &reward_zone);
                storage::set_backstop_emis_data(&e, &pool_1, &pool_1_emissions_data);
                storage::set_backstop_emis_data(&e, &pool_2, &pool_2_emissions_data);
                storage::set_pool_balance(
                    &e,
                    &pool_1,
                    &PoolBalance {
                        tokens: 300_000_0000000,
                        shares: 200_000_0000000,
                        q4w: 0,
                    },
                );
                storage::set_pool_balance(
                    &e,
                    &pool_2,
                    &PoolBalance {
                        tokens: 200_000_0000000,
                        shares: 150_000_0000000,
                        q4w: 0,
                    },
                );
                storage::set_pool_balance(
                    &e,
                    &pool_3,
                    &PoolBalance {
                        tokens: 500_000_0000000,
                        shares: 600_000_0000000,
                        q4w: 0,
                    },
                );
                blnd_token_client.approve(
                    &backstop,
                    &pool_1,
                    &100_123_0000000,
                    &e.ledger().sequence(),
                );

                distribute(&e);
                gulp_emissions(&e, &pool_1);
                gulp_emissions(&e, &pool_2);
                gulp_emissions(&e, &pool_3);

                assert_eq!(storage::get_last_distribution_time(&e), emitter_distro_time);
                assert_eq!(
                    storage::get_pool_balance(&e, &pool_1).tokens,
                    300_000_0000000
                );
                assert_eq!(
                    storage::get_pool_balance(&e, &pool_2).tokens,
                    200_000_0000000
                );
                assert_eq!(
                    storage::get_pool_balance(&e, &pool_3).tokens,
                    500_000_0000000
                );
                assert_eq!(
                    blnd_token_client.allowance(&backstop, &pool_1),
                    154_555_0000000
                );
                assert_eq!(
                    blnd_token_client.allowance(&backstop, &pool_2),
                    36_288_0000000
                );
                assert_eq!(
                    blnd_token_client.allowance(&backstop, &pool_3),
                    90_720_0000000
                );

                // validate backstop emissions

                let new_pool_1_data =
                    storage::get_backstop_emis_data(&e, &pool_1).unwrap_optimized();
                assert_eq!(new_pool_1_data.eps, 0_21016534391534);
                assert_eq!(new_pool_1_data.expiration, 1713139200 + 7 * 24 * 60 * 60);
                assert_eq!(new_pool_1_data.index, 9494910000000);
                assert_eq!(new_pool_1_data.last_time, 1713139200);
                let rz_emis_1 = storage::get_rz_emis(&e, &pool_1);
                assert_eq!(rz_emis_1.accrued, 0);
                assert_eq!(rz_emis_1.last_time, 1713139200);

                let new_pool_2_data =
                    storage::get_backstop_emis_data(&e, &pool_2).unwrap_optimized();
                assert_eq!(new_pool_2_data.eps, 0_14000000000000);
                assert_eq!(new_pool_2_data.expiration, 1713139200 + 7 * 24 * 60 * 60);
                assert_eq!(new_pool_2_data.index, 4532340000000);
                assert_eq!(new_pool_2_data.last_time, 1713139200);
                let rz_emis_2 = storage::get_rz_emis(&e, &pool_2);
                assert_eq!(rz_emis_2.accrued, 0);
                assert_eq!(rz_emis_2.last_time, 1713139200);

                let new_pool_3_data =
                    storage::get_backstop_emis_data(&e, &pool_3).unwrap_optimized();
                assert_eq!(new_pool_3_data.eps, 0_35000000000000);
                assert_eq!(new_pool_3_data.expiration, 1713139200 + 7 * 24 * 60 * 60);
                assert_eq!(new_pool_3_data.index, 0);
                assert_eq!(new_pool_3_data.last_time, 1713139200);
                let rz_emis_3 = storage::get_rz_emis(&e, &pool_3);
                assert_eq!(rz_emis_3.accrued, 0);
                assert_eq!(rz_emis_3.last_time, 1713139200);
            });
        }

        /********** distribute **********/

        #[test]
        fn test_distribute() {
            let e = Env::default();
            e.cost_estimate().budget().reset_unlimited();

            e.ledger().set(LedgerInfo {
                timestamp: 1713139200,
                protocol_version: 22,
                sequence_number: 0,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 3110400,
            });

            let backstop = create_backstop(&e);
            let emitter_distro_time = 1713139200 - 10;
            create_emitter(
                &e,
                &backstop,
                &Address::generate(&e),
                &Address::generate(&e),
                emitter_distro_time,
            );

            let pool_1 = Address::generate(&e);
            let pool_2 = Address::generate(&e);
            let pool_3 = Address::generate(&e);
            let reward_zone: Vec<Address> =
                vec![&e, pool_1.clone(), pool_2.clone(), pool_3.clone()];

            let start_pool_2_accrued = RzEmissions {
                accrued: 1_0000001,
                last_time: 123,
            };
            let start_pool_3_accrued = RzEmissions {
                accrued: 20_000_0000000,
                last_time: 0,
            };

            e.as_contract(&backstop, || {
                storage::set_backfill_status(&e, &false);
                storage::set_last_distribution_time(&e, &(emitter_distro_time - (60 * 60 * 24)));
                storage::set_reward_zone(&e, &reward_zone);
                storage::set_pool_balance(
                    &e,
                    &pool_1,
                    // 300_000_0000000 unqueued tokens
                    &PoolBalance {
                        tokens: 300_000_0000000,
                        shares: 200_000_0000000,
                        q4w: 0,
                    },
                );
                storage::set_pool_balance(
                    &e,
                    &pool_2,
                    // 200_000_0000000 unqueued tokens
                    &PoolBalance {
                        tokens: 400_000_0000000,
                        shares: 200_000_0000000,
                        q4w: 100_000_0000000,
                    },
                );
                storage::set_rz_emis(&e, &pool_2, &start_pool_2_accrued);
                storage::set_pool_balance(
                    &e,
                    &pool_3,
                    // 500_000_0000000 unqueued tokens
                    &PoolBalance {
                        tokens: 1_000_000_0000000,
                        shares: 1_200_000_0000000,
                        q4w: 600_000_0000000,
                    },
                );
                storage::set_rz_emis(&e, &pool_3, &start_pool_3_accrued);

                distribute(&e);

                let last_distro_time = storage::get_last_distribution_time(&e);
                assert_eq!(last_distro_time, emitter_distro_time);
                let backfilled_emissions = storage::get_backfill_emissions(&e);
                assert_eq!(backfilled_emissions, 0);

                let pool_1_accrued = storage::get_rz_emis(&e, &pool_1);
                assert_eq!(pool_1_accrued.accrued, 25_920_0000000 + 0);
                assert_eq!(pool_1_accrued.last_time, 0);
                let pool_2_accrued = storage::get_rz_emis(&e, &pool_2);
                assert_eq!(
                    pool_2_accrued.accrued,
                    17_280_0000000 + start_pool_2_accrued.accrued
                );
                assert_eq!(pool_2_accrued.last_time, start_pool_2_accrued.last_time);
                let pool_3_accrued = storage::get_rz_emis(&e, &pool_3);
                assert_eq!(
                    pool_3_accrued.accrued,
                    43_200_0000000 + start_pool_3_accrued.accrued
                );
                assert_eq!(pool_3_accrued.last_time, start_pool_3_accrued.last_time);

                // backfill status remains false
                let backfill_status = storage::get_backfill_status(&e);
                assert_eq!(backfill_status, Some(false));
            });
        }

        #[test]
        fn test_distribute_one_block_rounding_ok() {
            let e = Env::default();
            e.cost_estimate().budget().reset_unlimited();

            e.ledger().set(LedgerInfo {
                timestamp: 1713139200,
                protocol_version: 22,
                sequence_number: 0,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 3110400,
            });

            let backstop = create_backstop(&e);
            let emitter_distro_time = 1713139200 - 5;
            create_emitter(
                &e,
                &backstop,
                &Address::generate(&e),
                &Address::generate(&e),
                emitter_distro_time,
            );

            let pool_1 = Address::generate(&e);
            let pool_2 = Address::generate(&e);
            let pool_3 = Address::generate(&e);
            let reward_zone: Vec<Address> =
                vec![&e, pool_1.clone(), pool_2.clone(), pool_3.clone()];

            let start_pool_2_accrued = RzEmissions {
                accrued: 1_0000001,
                last_time: 123,
            };
            let start_pool_3_accrued = RzEmissions {
                accrued: 20_000_0000000,
                last_time: 0,
            };

            e.as_contract(&backstop, || {
                storage::set_backfill_status(&e, &false);
                // like distribute was called on previous block
                storage::set_last_distribution_time(&e, &(&emitter_distro_time - 5));
                storage::set_reward_zone(&e, &reward_zone);
                storage::set_pool_balance(
                    &e,
                    &pool_1,
                    &PoolBalance {
                        tokens: 10_000_0000000,
                        shares: 10_000_0000000,
                        q4w: 0,
                    },
                );
                storage::set_pool_balance(
                    &e,
                    &pool_2,
                    // 500_000_0000000 unqueued tokens
                    &PoolBalance {
                        tokens: 1_000_000_000_0000000,
                        shares: 200_000_0000000,
                        q4w: 100_000_0000000,
                    },
                );
                storage::set_rz_emis(&e, &pool_2, &start_pool_2_accrued);
                storage::set_pool_balance(
                    &e,
                    &pool_3,
                    // 1_000_000_000_0000000 unqueued tokens
                    &PoolBalance {
                        tokens: 2_000_000_000_0000000,
                        shares: 1_200_000_0000000,
                        q4w: 600_000_0000000,
                    },
                );
                storage::set_rz_emis(&e, &pool_3, &start_pool_3_accrued);

                distribute(&e);

                let last_distro_time = storage::get_last_distribution_time(&e);
                assert_eq!(last_distro_time, emitter_distro_time);
                let backfilled_emissions = storage::get_backfill_emissions(&e);
                assert_eq!(backfilled_emissions, 0);

                let pool_1_accrued = storage::get_rz_emis(&e, &pool_1);
                assert_eq!(pool_1_accrued.accrued, 330 + 0);
                assert_eq!(pool_1_accrued.last_time, 0);
                let pool_2_accrued = storage::get_rz_emis(&e, &pool_2);
                assert_eq!(
                    pool_2_accrued.accrued,
                    1_6666555 + start_pool_2_accrued.accrued
                );
                assert_eq!(pool_2_accrued.last_time, start_pool_2_accrued.last_time);
                let pool_3_accrued = storage::get_rz_emis(&e, &pool_3);
                assert_eq!(
                    pool_3_accrued.accrued,
                    3_3333110 + start_pool_3_accrued.accrued
                );
                assert_eq!(pool_3_accrued.last_time, start_pool_3_accrued.last_time);

                // backfill status remains false
                let backfill_status = storage::get_backfill_status(&e);
                assert_eq!(backfill_status, Some(false));
            });
        }

        #[test]
        #[should_panic(expected = "Error(Contract, #1000)")]
        fn test_distribute_empty_rz() {
            let e = Env::default();
            e.cost_estimate().budget().reset_unlimited();

            e.ledger().set(LedgerInfo {
                timestamp: 1713139200,
                protocol_version: 22,
                sequence_number: 0,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 3110400,
            });

            let backstop = create_backstop(&e);
            let emitter_distro_time = 1713139200 - 10;
            create_emitter(
                &e,
                &backstop,
                &Address::generate(&e),
                &Address::generate(&e),
                emitter_distro_time,
            );

            let pool_1 = Address::generate(&e);

            let reward_zone: Vec<Address> = vec![&e];

            e.as_contract(&backstop, || {
                storage::set_last_distribution_time(&e, &(emitter_distro_time - (60 * 60 * 24)));
                storage::set_reward_zone(&e, &reward_zone);
                storage::set_pool_balance(
                    &e,
                    &pool_1,
                    &PoolBalance {
                        tokens: 300_000_0000000,
                        shares: 200_000_0000000,
                        q4w: 0,
                    },
                );

                distribute(&e);
            });
        }

        #[test]
        fn test_distribute_no_last_dist_time() {
            let e = Env::default();
            e.cost_estimate().budget().reset_unlimited();

            e.ledger().set(LedgerInfo {
                timestamp: 1713139200,
                protocol_version: 22,
                sequence_number: 0,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 3110400,
            });

            let backstop = create_backstop(&e);
            let emitter_distro_time = 1713139200 - 10;
            create_emitter(
                &e,
                &backstop,
                &Address::generate(&e),
                &Address::generate(&e),
                emitter_distro_time,
            );

            let pool_1 = Address::generate(&e);
            let pool_2 = Address::generate(&e);
            let pool_3 = Address::generate(&e);
            let reward_zone: Vec<Address> =
                vec![&e, pool_1.clone(), pool_2.clone(), pool_3.clone()];

            e.as_contract(&backstop, || {
                storage::set_reward_zone(&e, &reward_zone);
                storage::set_pool_balance(
                    &e,
                    &pool_1,
                    &PoolBalance {
                        tokens: 300_000_0000000,
                        shares: 200_000_0000000,
                        q4w: 0,
                    },
                );
                storage::set_pool_balance(
                    &e,
                    &pool_2,
                    &PoolBalance {
                        tokens: 200_000_0000000,
                        shares: 150_000_0000000,
                        q4w: 0,
                    },
                );
                storage::set_pool_balance(
                    &e,
                    &pool_3,
                    &PoolBalance {
                        tokens: 500_000_0000000,
                        shares: 600_000_0000000,
                        q4w: 0,
                    },
                );

                let new_emissions = distribute(&e);

                assert_eq!(new_emissions, 0);
                let last_distro_time = storage::get_last_distribution_time(&e);
                assert_eq!(last_distro_time, emitter_distro_time);
                let pool_1_accrued_1 = storage::get_rz_emis(&e, &pool_1);
                assert_eq!(pool_1_accrued_1.accrued, 0);
                assert_eq!(pool_1_accrued_1.last_time, 0);
                let pool_2_accrued_1 = storage::get_rz_emis(&e, &pool_2);
                assert_eq!(pool_2_accrued_1.accrued, 0);
                assert_eq!(pool_2_accrued_1.last_time, 0);
                let pool_3_accrued_1 = storage::get_rz_emis(&e, &pool_3);
                assert_eq!(pool_3_accrued_1.accrued, 0);
                assert_eq!(pool_3_accrued_1.last_time, 0);

                // sets backfill status to false
                let backfill_status = storage::get_backfill_status(&e);
                assert_eq!(backfill_status, Some(false));
            });
        }

        #[test]
        #[should_panic(expected = "Error(Contract, #1000)")]
        fn test_distribute_under_5_block_time_panics() {
            let e = Env::default();
            e.cost_estimate().budget().reset_unlimited();

            e.ledger().set(LedgerInfo {
                timestamp: 1713139200,
                protocol_version: 22,
                sequence_number: 0,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 3110400,
            });

            let backstop = create_backstop(&e);
            let emitter_distro_time = 1713139200 - 5;
            create_emitter(
                &e,
                &backstop,
                &Address::generate(&e),
                &Address::generate(&e),
                emitter_distro_time,
            );

            let pool_1 = Address::generate(&e);
            let pool_2 = Address::generate(&e);
            let pool_3 = Address::generate(&e);
            let reward_zone: Vec<Address> =
                vec![&e, pool_1.clone(), pool_2.clone(), pool_3.clone()];

            e.as_contract(&backstop, || {
                storage::set_backfill_status(&e, &false);
                storage::set_last_distribution_time(&e, &(emitter_distro_time - 4));
                storage::set_reward_zone(&e, &reward_zone);
                storage::set_pool_balance(
                    &e,
                    &pool_1,
                    // 300_000_0000000 unqueued tokens
                    &PoolBalance {
                        tokens: 300_000_0000000,
                        shares: 200_000_0000000,
                        q4w: 0,
                    },
                );
                storage::set_pool_balance(
                    &e,
                    &pool_2,
                    // 200_000_0000000 unqueued tokens
                    &PoolBalance {
                        tokens: 400_000_0000000,
                        shares: 200_000_0000000,
                        q4w: 100_000_0000000,
                    },
                );
                storage::set_pool_balance(
                    &e,
                    &pool_3,
                    // 500_000_0000000 unqueued tokens
                    &PoolBalance {
                        tokens: 1_000_000_0000000,
                        shares: 1_200_000_0000000,
                        q4w: 600_000_0000000,
                    },
                );

                distribute(&e);
            });
        }

        #[test]
        #[should_panic(expected = "Error(Contract, #1000)")]
        fn test_distribute_last_distro_panics_errors() {
            let e = Env::default();
            e.cost_estimate().budget().reset_unlimited();

            e.ledger().set(LedgerInfo {
                timestamp: 1713139200,
                protocol_version: 22,
                sequence_number: 0,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 3110400,
            });

            let v1_backstop = create_backstop(&e);
            let backstop = create_backstop(&e);
            let emitter_distro_time = 1713139200 - 10;
            // set backstop to another address to force a panic
            create_emitter(
                &e,
                &v1_backstop,
                &Address::generate(&e),
                &Address::generate(&e),
                emitter_distro_time,
            );

            let pool_1 = Address::generate(&e);
            let pool_2 = Address::generate(&e);
            let pool_3 = Address::generate(&e);
            let reward_zone: Vec<Address> =
                vec![&e, pool_1.clone(), pool_2.clone(), pool_3.clone()];

            let start_pool_2_accrued = RzEmissions {
                accrued: 1_0000001,
                last_time: 123,
            };
            let start_pool_3_accrued = RzEmissions {
                accrued: 20_000_0000000,
                last_time: 0,
            };

            e.as_contract(&backstop, || {
                storage::set_backfill_status(&e, &false);
                storage::set_last_distribution_time(&e, &(emitter_distro_time - (60 * 60 * 24)));
                storage::set_reward_zone(&e, &reward_zone);
                storage::set_pool_balance(
                    &e,
                    &pool_1,
                    // 300_000_0000000 unqueued tokens
                    &PoolBalance {
                        tokens: 300_000_0000000,
                        shares: 200_000_0000000,
                        q4w: 0,
                    },
                );
                storage::set_pool_balance(
                    &e,
                    &pool_2,
                    // 200_000_0000000 unqueued tokens
                    &PoolBalance {
                        tokens: 400_000_0000000,
                        shares: 200_000_0000000,
                        q4w: 100_000_0000000,
                    },
                );
                storage::set_rz_emis(&e, &pool_2, &start_pool_2_accrued);
                storage::set_pool_balance(
                    &e,
                    &pool_3,
                    // 500_000_0000000 unqueued tokens
                    &PoolBalance {
                        tokens: 1_000_000_0000000,
                        shares: 1_200_000_0000000,
                        q4w: 600_000_0000000,
                    },
                );
                storage::set_rz_emis(&e, &pool_3, &start_pool_3_accrued);

                distribute(&e);
            });
        }

        #[test]
        fn test_distribute_backfill_emissions() {
            let e = Env::default();
            e.cost_estimate().budget().reset_unlimited();

            e.ledger().set(LedgerInfo {
                timestamp: 1713139200,
                protocol_version: 22,
                sequence_number: 0,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 3110400,
            });

            let v1_backstop = create_backstop(&e);
            let backstop = create_backstop(&e);
            let emitter_distro_time = 1713139200 - 1000;
            create_emitter(
                &e,
                &v1_backstop,
                &Address::generate(&e),
                &Address::generate(&e),
                emitter_distro_time,
            );

            let pool_1 = Address::generate(&e);
            let pool_2 = Address::generate(&e);
            let pool_3 = Address::generate(&e);
            let reward_zone: Vec<Address> =
                vec![&e, pool_1.clone(), pool_2.clone(), pool_3.clone()];
            let start_backfilled_emissions = 1_000_000 * SCALAR_7;
            let start_pool_2_accrued = RzEmissions {
                accrued: 1_0000001,
                last_time: 123,
            };
            let start_pool_3_accrued = RzEmissions {
                accrued: 20_000_0000000,
                last_time: 0,
            };

            e.as_contract(&backstop, || {
                storage::set_backfill_status(&e, &true);
                storage::set_backfill_emissions(&e, &start_backfilled_emissions);
                storage::set_last_distribution_time(&e, &(1713139200 - (60 * 60 * 24)));
                storage::set_reward_zone(&e, &reward_zone);
                storage::set_pool_balance(
                    &e,
                    &pool_1,
                    &PoolBalance {
                        tokens: 300_000_0000000,
                        shares: 200_000_0000000,
                        q4w: 0,
                    },
                );
                storage::set_pool_balance(
                    &e,
                    &pool_2,
                    &PoolBalance {
                        tokens: 200_000_0000000,
                        shares: 150_000_0000000,
                        q4w: 0,
                    },
                );
                storage::set_rz_emis(&e, &pool_2, &start_pool_2_accrued);
                storage::set_pool_balance(
                    &e,
                    &pool_3,
                    &PoolBalance {
                        tokens: 500_000_0000000,
                        shares: 600_000_0000000,
                        q4w: 0,
                    },
                );
                storage::set_rz_emis(&e, &pool_3, &start_pool_3_accrued);

                distribute(&e);

                let last_distro_time = storage::get_last_distribution_time(&e);
                assert_eq!(last_distro_time, e.ledger().timestamp());
                let backfilled_emissions = storage::get_backfill_emissions(&e);
                assert_eq!(
                    backfilled_emissions,
                    start_backfilled_emissions + (60 * 60 * 24) * SCALAR_7
                );
                let is_backfill = storage::get_backfill_status(&e);
                assert_eq!(is_backfill, Some(true));

                let pool_1_accrued = storage::get_rz_emis(&e, &pool_1);
                assert_eq!(pool_1_accrued.accrued, 25_920_0000000 + 0);
                assert_eq!(pool_1_accrued.last_time, 0);
                let pool_2_accrued = storage::get_rz_emis(&e, &pool_2);
                assert_eq!(
                    pool_2_accrued.accrued,
                    17_280_0000000 + start_pool_2_accrued.accrued
                );
                assert_eq!(pool_2_accrued.last_time, start_pool_2_accrued.last_time);
                let pool_3_accrued = storage::get_rz_emis(&e, &pool_3);
                assert_eq!(
                    pool_3_accrued.accrued,
                    43_200_0000000 + start_pool_3_accrued.accrued
                );
                assert_eq!(pool_3_accrued.last_time, start_pool_3_accrued.last_time);
            });
        }

        #[test]
        fn test_distribute_backfill_emissions_first_call() {
            let e = Env::default();
            e.cost_estimate().budget().reset_unlimited();

            e.ledger().set(LedgerInfo {
                timestamp: 1713139200,
                protocol_version: 22,
                sequence_number: 0,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 3110400,
            });

            let v1_backstop = create_backstop(&e);
            let backstop = create_backstop(&e);
            let emitter_distro_time = 1713139200 - 10;
            create_emitter(
                &e,
                &v1_backstop,
                &Address::generate(&e),
                &Address::generate(&e),
                emitter_distro_time,
            );

            let pool_1 = Address::generate(&e);
            let pool_2 = Address::generate(&e);
            let pool_3 = Address::generate(&e);
            let reward_zone: Vec<Address> =
                vec![&e, pool_1.clone(), pool_2.clone(), pool_3.clone()];

            e.as_contract(&backstop, || {
                storage::set_reward_zone(&e, &reward_zone);
                storage::set_pool_balance(
                    &e,
                    &pool_1,
                    &PoolBalance {
                        tokens: 300_000_0000000,
                        shares: 200_000_0000000,
                        q4w: 0,
                    },
                );
                storage::set_pool_balance(
                    &e,
                    &pool_2,
                    &PoolBalance {
                        tokens: 200_000_0000000,
                        shares: 150_000_0000000,
                        q4w: 0,
                    },
                );
                storage::set_pool_balance(
                    &e,
                    &pool_3,
                    &PoolBalance {
                        tokens: 500_000_0000000,
                        shares: 600_000_0000000,
                        q4w: 0,
                    },
                );

                distribute(&e);

                let last_distro_time = storage::get_last_distribution_time(&e);
                assert_eq!(last_distro_time, e.ledger().timestamp());
                let backfilled_emissions = storage::get_backfill_emissions(&e);
                assert_eq!(backfilled_emissions, 0);
                let is_backfill = storage::get_backfill_status(&e);
                assert_eq!(is_backfill, Some(true));
                let pool_1_accrued = storage::get_rz_emis(&e, &pool_1);
                assert_eq!(pool_1_accrued.accrued, 0);
                assert_eq!(pool_1_accrued.last_time, 0);
                let pool_2_accrued = storage::get_rz_emis(&e, &pool_2);
                assert_eq!(pool_2_accrued.accrued, 0);
                assert_eq!(pool_2_accrued.last_time, 0);
                let pool_3_accrued = storage::get_rz_emis(&e, &pool_3);
                assert_eq!(pool_3_accrued.accrued, 0);
                assert_eq!(pool_3_accrued.last_time, 0);
            });
        }

        #[test]
        fn test_distribute_backfill_emissions_distributes_at_most_max() {
            let e = Env::default();
            e.cost_estimate().budget().reset_unlimited();

            e.ledger().set(LedgerInfo {
                timestamp: 1713139200,
                protocol_version: 22,
                sequence_number: 0,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 3110400,
            });

            let v1_backstop = create_backstop(&e);
            let backstop = create_backstop(&e);
            let emitter_distro_time = 1713139200 - 10;
            create_emitter(
                &e,
                &v1_backstop,
                &Address::generate(&e),
                &Address::generate(&e),
                emitter_distro_time,
            );

            let pool_1 = Address::generate(&e);
            let pool_2 = Address::generate(&e);
            let pool_3 = Address::generate(&e);
            let reward_zone: Vec<Address> =
                vec![&e, pool_1.clone(), pool_2.clone(), pool_3.clone()];
            let start_backfilled_emissions = MAX_BACKFILLED_EMISSIONS - (60 * 60 * 24) * SCALAR_7;

            e.as_contract(&backstop, || {
                storage::set_backfill_emissions(&e, &start_backfilled_emissions);
                storage::set_last_distribution_time(&e, &(emitter_distro_time - 60 * 60 * 30));
                storage::set_reward_zone(&e, &reward_zone);
                storage::set_pool_balance(
                    &e,
                    &pool_1,
                    &PoolBalance {
                        tokens: 300_000_0000000,
                        shares: 200_000_0000000,
                        q4w: 200_000_0000000,
                    },
                );
                storage::set_pool_balance(
                    &e,
                    &pool_2,
                    &PoolBalance {
                        // 400k non-q4w, 40%
                        tokens: 500_000_0000000,
                        shares: 400_000_0000000,
                        q4w: 80_000_0000000,
                    },
                );
                storage::set_pool_balance(
                    &e,
                    &pool_3,
                    &PoolBalance {
                        tokens: 600_000_0000000,
                        shares: 700_000_0000000,
                        q4w: 0,
                    },
                );

                distribute(&e);
                let last_distro_time = storage::get_last_distribution_time(&e);
                assert_eq!(last_distro_time, e.ledger().timestamp());
                let backfilled_emissions = storage::get_backfill_emissions(&e);
                assert_eq!(backfilled_emissions, MAX_BACKFILLED_EMISSIONS);
                let is_backfill = storage::get_backfill_status(&e);
                assert_eq!(is_backfill, Some(true));

                let pool_1_accrued = storage::get_rz_emis(&e, &pool_1);
                assert_eq!(pool_1_accrued.accrued, 0);
                assert_eq!(pool_1_accrued.last_time, 0);
                let pool_2_accrued = storage::get_rz_emis(&e, &pool_2);
                assert_eq!(pool_2_accrued.accrued, 34_560_0000000);
                assert_eq!(pool_2_accrued.last_time, 0);
                let pool_3_accrued = storage::get_rz_emis(&e, &pool_3);
                assert_eq!(pool_3_accrued.accrued, 51_840_0000000);
                assert_eq!(pool_3_accrued.last_time, 0);
            });
        }

        #[test]
        #[should_panic(expected = "Error(Contract, #1010)")]
        fn test_distribute_backfill_emissions_at_max_panics() {
            let e = Env::default();
            e.cost_estimate().budget().reset_unlimited();

            e.ledger().set(LedgerInfo {
                timestamp: 1713139200,
                protocol_version: 22,
                sequence_number: 0,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 3110400,
            });

            let v1_backstop = create_backstop(&e);
            let backstop = create_backstop(&e);
            let emitter_distro_time = 1713139200 - 10;
            create_emitter(
                &e,
                &v1_backstop,
                &Address::generate(&e),
                &Address::generate(&e),
                emitter_distro_time,
            );

            let pool_1 = Address::generate(&e);
            let pool_2 = Address::generate(&e);
            let pool_3 = Address::generate(&e);
            let reward_zone: Vec<Address> =
                vec![&e, pool_1.clone(), pool_2.clone(), pool_3.clone()];
            let start_backfilled_emissions = MAX_BACKFILLED_EMISSIONS;

            e.as_contract(&backstop, || {
                storage::set_backfill_emissions(&e, &start_backfilled_emissions);
                storage::set_last_distribution_time(&e, &(emitter_distro_time - 60 * 60 * 24));
                storage::set_reward_zone(&e, &reward_zone);
                storage::set_pool_balance(
                    &e,
                    &pool_1,
                    &PoolBalance {
                        tokens: 300_000_0000000,
                        shares: 200_000_0000000,
                        q4w: 200_000_0000000,
                    },
                );
                storage::set_pool_balance(
                    &e,
                    &pool_2,
                    &PoolBalance {
                        // 400k non-q4w, 40%
                        tokens: 500_000_0000000,
                        shares: 400_000_0000000,
                        q4w: 80_000_0000000,
                    },
                );
                storage::set_pool_balance(
                    &e,
                    &pool_3,
                    &PoolBalance {
                        tokens: 600_000_0000000,
                        shares: 700_000_0000000,
                        q4w: 0,
                    },
                );

                distribute(&e);
            });
        }

        #[test]
        fn test_distribute_backfill_emissions_over_needs_reset() {
            let e = Env::default();
            e.cost_estimate().budget().reset_unlimited();

            e.ledger().set(LedgerInfo {
                timestamp: 1713139200,
                protocol_version: 22,
                sequence_number: 0,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 3110400,
            });

            let backstop = create_backstop(&e);
            let emitter_distro_time = 1713139200 - 10;
            create_emitter(
                &e,
                &backstop,
                &Address::generate(&e),
                &Address::generate(&e),
                emitter_distro_time,
            );

            let pool_1 = Address::generate(&e);
            let pool_2 = Address::generate(&e);
            let pool_3 = Address::generate(&e);
            let reward_zone: Vec<Address> =
                vec![&e, pool_1.clone(), pool_2.clone(), pool_3.clone()];
            let start_backfilled_emissions = 1_000_000 * SCALAR_7;
            let last_distro_time = 1713139200 - 10000;

            let start_pool_2_accrued = RzEmissions {
                accrued: 1_0000001,
                last_time: 123,
            };
            let start_pool_3_accrued = RzEmissions {
                accrued: 20_000_0000000,
                last_time: 0,
            };

            e.as_contract(&backstop, || {
                storage::set_backfill_status(&e, &true);
                storage::set_backfill_emissions(&e, &start_backfilled_emissions);
                storage::set_last_distribution_time(&e, &last_distro_time);
                storage::set_reward_zone(&e, &reward_zone);
                storage::set_pool_balance(
                    &e,
                    &pool_1,
                    &PoolBalance {
                        tokens: 300_000_0000000,
                        shares: 200_000_0000000,
                        q4w: 0,
                    },
                );
                storage::set_pool_balance(
                    &e,
                    &pool_2,
                    &PoolBalance {
                        tokens: 200_000_0000000,
                        shares: 150_000_0000000,
                        q4w: 0,
                    },
                );
                storage::set_rz_emis(&e, &pool_2, &start_pool_2_accrued);
                storage::set_pool_balance(
                    &e,
                    &pool_3,
                    &PoolBalance {
                        tokens: 500_000_0000000,
                        shares: 600_000_0000000,
                        q4w: 0,
                    },
                );
                storage::set_rz_emis(&e, &pool_3, &start_pool_3_accrued);

                distribute(&e);

                let last_distro_time = storage::get_last_distribution_time(&e);
                assert_eq!(last_distro_time, emitter_distro_time);
                let backfilled_emissions = storage::get_backfill_emissions(&e);
                assert_eq!(backfilled_emissions, start_backfilled_emissions);
                let is_backfill = storage::get_backfill_status(&e);
                assert_eq!(is_backfill, Some(false));
                let pool_1_accrued = storage::get_rz_emis(&e, &pool_1);
                assert_eq!(pool_1_accrued.accrued, 0);
                assert_eq!(pool_1_accrued.last_time, 0);
                let pool_2_accrued = storage::get_rz_emis(&e, &pool_2);
                assert_eq!(pool_2_accrued.accrued, start_pool_2_accrued.accrued);
                assert_eq!(pool_2_accrued.last_time, start_pool_2_accrued.last_time);
                let pool_3_accrued = storage::get_rz_emis(&e, &pool_3);
                assert_eq!(pool_3_accrued.accrued, start_pool_3_accrued.accrued);
                assert_eq!(pool_3_accrued.last_time, start_pool_3_accrued.last_time);
            });
        }

        /********** add_to_reward_zone **********/

        #[test]
        fn test_add_to_rz_empty_adds_pool() {
            let e = Env::default();
            e.ledger().set(LedgerInfo {
                timestamp: 1713139200,
                protocol_version: 22,
                sequence_number: 0,
                base_reserve: 10,
                network_id: Default::default(),
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 3110400,
            });
            e.mock_all_auths();

            let bombadil = Address::generate(&e);
            let backstop_id = create_backstop(&e);
            let to_add = Address::generate(&e);

            let (blnd_id, _) = create_blnd_token(&e, &backstop_id, &bombadil);
            let (usdc_id, _) = create_usdc_token(&e, &backstop_id, &bombadil);
            create_comet_lp_pool_with_tokens_per_share(
                &e,
                &backstop_id,
                &bombadil,
                &blnd_id,
                5_0000000,
                &usdc_id,
                0_1000000,
            );

            e.as_contract(&backstop_id, || {
                storage::set_last_distribution_time(&e, &0);
                storage::set_pool_balance(
                    &e,
                    &to_add,
                    &PoolBalance {
                        shares: 90_000_0000000,
                        tokens: 100_000_0000000,
                        q4w: 1_000_0000000,
                    },
                );

                add_to_reward_zone(&e, to_add.clone(), None);
                let actual_rz = storage::get_reward_zone(&e);
                let expected_rz: Vec<Address> = vec![&e, to_add];
                assert_eq!(actual_rz, expected_rz);
            });
        }

        #[test]
        fn test_add_to_rz_before_max() {
            let e = Env::default();
            e.ledger().set(LedgerInfo {
                timestamp: 1713139200 - 100000,
                protocol_version: 22,
                sequence_number: 0,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 3110400,
            });
            e.mock_all_auths();

            let bombadil = Address::generate(&e);
            let backstop_id = create_backstop(&e);
            let to_add = Address::generate(&e);

            let (blnd_id, _) = create_blnd_token(&e, &backstop_id, &bombadil);
            let (usdc_id, _) = create_usdc_token(&e, &backstop_id, &bombadil);
            create_comet_lp_pool_with_tokens_per_share(
                &e,
                &backstop_id,
                &bombadil,
                &blnd_id,
                5_0000000,
                &usdc_id,
                0_1000000,
            );
            let mut reward_zone: Vec<Address> = vec![
                &e,
                Address::generate(&e),
                Address::generate(&e),
                Address::generate(&e),
                Address::generate(&e),
                Address::generate(&e),
                Address::generate(&e),
                Address::generate(&e),
                Address::generate(&e),
                Address::generate(&e),
            ];

            e.as_contract(&backstop_id, || {
                storage::set_last_distribution_time(&e, &(1713139200 - 100));
                storage::set_reward_zone(&e, &reward_zone);
                storage::set_pool_balance(
                    &e,
                    &to_add,
                    &PoolBalance {
                        shares: 90_000_0000000,
                        tokens: 100_000_0000000,
                        q4w: 1_000_0000000,
                    },
                );

                add_to_reward_zone(&e, to_add.clone(), None);
                let actual_rz = storage::get_reward_zone(&e);
                reward_zone.push_front(to_add);
                assert_eq!(actual_rz, reward_zone);
            });
        }

        #[test]
        #[should_panic(expected = "Error(Contract, #1002)")]
        fn test_add_to_rz_empty_pool_under_backstop_threshold() {
            let e = Env::default();
            e.ledger().set(LedgerInfo {
                timestamp: 1713139200,
                protocol_version: 22,
                sequence_number: 0,
                base_reserve: 10,
                network_id: Default::default(),
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 3110400,
            });
            e.mock_all_auths();

            let bombadil = Address::generate(&e);
            let backstop_id = create_backstop(&e);
            let to_add = Address::generate(&e);

            let (blnd_id, _) = create_blnd_token(&e, &backstop_id, &bombadil);
            let (usdc_id, _) = create_usdc_token(&e, &backstop_id, &bombadil);
            create_comet_lp_pool_with_tokens_per_share(
                &e,
                &backstop_id,
                &bombadil,
                &blnd_id,
                5_0000000,
                &usdc_id,
                0_1000000,
            );

            e.as_contract(&backstop_id, || {
                storage::set_last_distribution_time(&e, &(1713139200 - 100));
                storage::set_pool_balance(
                    &e,
                    &to_add,
                    &PoolBalance {
                        shares: 30_000_0000000,
                        tokens: 40_000_0000000,
                        q4w: 1_000_0000000,
                    },
                );
                // storage::set_lp_token_val(&e, &(5_0000000, 0_1000000));

                add_to_reward_zone(&e, to_add.clone(), None);
                let actual_rz = storage::get_reward_zone(&e);
                let expected_rz: Vec<Address> = vec![&e, to_add];
                assert_eq!(actual_rz, expected_rz);
            });
        }

        #[test]
        #[should_panic(expected = "Error(Contract, #1009)")]
        fn test_add_to_rz_respects_max_size() {
            let e = Env::default();
            e.ledger().set(LedgerInfo {
                timestamp: 1713139200,
                protocol_version: 22,
                sequence_number: 0,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 3110400,
            });
            e.mock_all_auths();

            let bombadil = Address::generate(&e);
            let backstop_id = create_backstop(&e);
            let to_add = Address::generate(&e);

            let (blnd_id, _) = create_blnd_token(&e, &backstop_id, &bombadil);
            let (usdc_id, _) = create_usdc_token(&e, &backstop_id, &bombadil);
            create_comet_lp_pool_with_tokens_per_share(
                &e,
                &backstop_id,
                &bombadil,
                &blnd_id,
                5_0000000,
                &usdc_id,
                0_1000000,
            );
            let mut reward_zone: Vec<Address> = vec![&e];
            for _ in 0..30 {
                reward_zone.push_back(Address::generate(&e));
            }
            e.as_contract(&backstop_id, || {
                storage::set_last_distribution_time(&e, &(1713139200 - 100));
                storage::set_reward_zone(&e, &reward_zone);
                storage::set_pool_balance(
                    &e,
                    &to_add,
                    &PoolBalance {
                        shares: 90_000_0000000,
                        tokens: 100_000_0000000,
                        q4w: 1_000_0000000,
                    },
                );

                assert!(reward_zone.len() == 30);

                // This should fail due to the reward zone being full and not having a pool to remove
                add_to_reward_zone(&e, to_add.clone(), None);
            });
        }

        #[test]
        fn test_add_to_rz_swap_happy_path() {
            let e = Env::default();
            e.ledger().set(LedgerInfo {
                timestamp: 1713139200,
                protocol_version: 22,
                sequence_number: 0,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 3110400,
            });

            let backstop_id = create_backstop(&e);
            create_blnd_token(&e, &backstop_id, &Address::generate(&e));
            e.mock_all_auths();

            let bombadil = Address::generate(&e);
            let backstop_id = create_backstop(&e);
            let to_add = Address::generate(&e);
            let to_remove = Address::generate(&e);

            let (blnd_id, _) = create_blnd_token(&e, &backstop_id, &bombadil);
            let (usdc_id, _) = create_usdc_token(&e, &backstop_id, &bombadil);
            create_comet_lp_pool_with_tokens_per_share(
                &e,
                &backstop_id,
                &bombadil,
                &blnd_id,
                5_0000000,
                &usdc_id,
                0_1000000,
            );
            let mut reward_zone: Vec<Address> = vec![&e];
            for _ in 0..30 {
                reward_zone.push_back(Address::generate(&e));
            }
            reward_zone.set(7, to_remove.clone());

            e.as_contract(&backstop_id, || {
                storage::set_reward_zone(&e, &reward_zone);
                storage::set_last_distribution_time(&e, &(1713139200 - 100));
                storage::set_pool_balance(
                    &e,
                    &to_add,
                    &PoolBalance {
                        shares: 90_000_0000000,
                        tokens: 100_001_0000000,
                        q4w: 1_000_0000000,
                    },
                );
                storage::set_pool_balance(
                    &e,
                    &to_remove,
                    &PoolBalance {
                        shares: 90_000_0000000,
                        tokens: 100_000_0000000,
                        q4w: 1_000_0000000,
                    },
                );
                storage::set_backstop_emis_data(
                    &e,
                    &to_remove,
                    &BackstopEmissionData {
                        eps: 0_10000000000000,
                        expiration: 1713139200 + 1000,
                        index: 0,
                        last_time: 1713139200 - 12345,
                    },
                );
                add_to_reward_zone(&e, to_add.clone(), Some(to_remove.clone()));
                let actual_rz = storage::get_reward_zone(&e);
                assert_eq!(actual_rz.len(), 30);
                reward_zone.remove(7);
                reward_zone.push_front(to_add.clone());
                assert_eq!(actual_rz, reward_zone);
            });
        }

        #[test]
        #[should_panic(expected = "Error(Contract, #1002)")]
        fn test_add_to_rz_swap_not_enough_tokens() {
            let e = Env::default();
            e.ledger().set(LedgerInfo {
                timestamp: 1713139200,
                protocol_version: 22,
                sequence_number: 0,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 3110400,
            });
            e.mock_all_auths();

            let bombadil = Address::generate(&e);
            let backstop_id = create_backstop(&e);
            let to_add = Address::generate(&e);
            let to_remove = Address::generate(&e);

            let (blnd_id, _) = create_blnd_token(&e, &backstop_id, &bombadil);
            let (usdc_id, _) = create_usdc_token(&e, &backstop_id, &bombadil);
            create_comet_lp_pool_with_tokens_per_share(
                &e,
                &backstop_id,
                &bombadil,
                &blnd_id,
                5_0000000,
                &usdc_id,
                0_1000000,
            );
            let mut reward_zone: Vec<Address> = vec![&e];
            for _ in 0..30 {
                reward_zone.push_back(Address::generate(&e));
            }
            reward_zone.set(7, to_remove.clone());

            e.as_contract(&backstop_id, || {
                storage::set_reward_zone(&e, &reward_zone);
                storage::set_last_distribution_time(&e, &(1713139200 - 60 * 60));
                storage::set_pool_balance(
                    &e,
                    &to_add,
                    &PoolBalance {
                        shares: 90_000_0000000,
                        tokens: 100_000_0000000,
                        q4w: 1_000_0000000,
                    },
                );
                storage::set_pool_balance(
                    &e,
                    &to_remove,
                    &PoolBalance {
                        shares: 90_000_0000000,
                        tokens: 100_000_0000000,
                        q4w: 1_000_0000000,
                    },
                );

                add_to_reward_zone(&e, to_add.clone(), Some(to_remove));
            });
        }

        #[test]
        #[should_panic(expected = "Error(Contract, #1000)")]
        fn test_add_to_rz_swap_distribution_too_long_ago() {
            let e = Env::default();
            e.ledger().set(LedgerInfo {
                timestamp: 1713139200,
                protocol_version: 22,
                sequence_number: 0,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 3110400,
            });
            e.mock_all_auths();

            let bombadil = Address::generate(&e);
            let backstop_id = create_backstop(&e);
            let to_add = Address::generate(&e);
            let to_remove = Address::generate(&e);

            let (blnd_id, _) = create_blnd_token(&e, &backstop_id, &bombadil);
            let (usdc_id, _) = create_usdc_token(&e, &backstop_id, &bombadil);
            create_comet_lp_pool_with_tokens_per_share(
                &e,
                &backstop_id,
                &bombadil,
                &blnd_id,
                5_0000000,
                &usdc_id,
                0_1000000,
            );
            let mut reward_zone: Vec<Address> = vec![&e];
            for _ in 0..30 {
                reward_zone.push_back(Address::generate(&e));
            }
            reward_zone.set(7, to_remove.clone());

            e.as_contract(&backstop_id, || {
                storage::set_reward_zone(&e, &reward_zone);
                storage::set_last_distribution_time(&e, &(1713139200 - 60 * 60 - 1));
                storage::set_pool_balance(
                    &e,
                    &to_add,
                    &PoolBalance {
                        shares: 90_000_0000000,
                        tokens: 100_001_0000000,
                        q4w: 1_000_0000000,
                    },
                );
                storage::set_pool_balance(
                    &e,
                    &to_remove,
                    &PoolBalance {
                        shares: 90_000_0000000,
                        tokens: 100_000_0000000,
                        q4w: 1_000_0000000,
                    },
                );

                add_to_reward_zone(&e, to_add.clone(), Some(to_remove));
            });
        }

        #[test]
        #[should_panic(expected = "Error(Contract, #1002)")]
        fn test_add_to_rz_to_remove_not_in_rz() {
            let e = Env::default();
            e.ledger().set(LedgerInfo {
                timestamp: 1713139200,
                protocol_version: 22,
                sequence_number: 0,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 3110400,
            });
            e.mock_all_auths();

            let bombadil = Address::generate(&e);
            let backstop_id = create_backstop(&e);
            let to_add = Address::generate(&e);
            let to_remove = Address::generate(&e);

            let (blnd_id, _) = create_blnd_token(&e, &backstop_id, &bombadil);
            let (usdc_id, _) = create_usdc_token(&e, &backstop_id, &bombadil);
            create_comet_lp_pool_with_tokens_per_share(
                &e,
                &backstop_id,
                &bombadil,
                &blnd_id,
                5_0000000,
                &usdc_id,
                0_1000000,
            );
            let mut reward_zone: Vec<Address> = vec![&e];
            for _ in 0..30 {
                reward_zone.push_back(Address::generate(&e));
            }

            e.as_contract(&backstop_id, || {
                storage::set_reward_zone(&e, &reward_zone);
                storage::set_last_distribution_time(&e, &(1713139200 - 60 * 60));
                storage::set_pool_balance(
                    &e,
                    &to_add,
                    &PoolBalance {
                        shares: 90_000_0000000,
                        tokens: 100_001_0000000,
                        q4w: 1_000_0000000,
                    },
                );
                storage::set_pool_balance(
                    &e,
                    &to_remove,
                    &PoolBalance {
                        shares: 90_000_0000000,
                        tokens: 100_000_0000000,
                        q4w: 1_000_0000000,
                    },
                );

                add_to_reward_zone(&e, to_add.clone(), Some(to_remove));
            });
        }

        #[test]
        #[should_panic(expected = "Error(Contract, #1000)")]
        fn test_add_to_rz_already_exists_panics() {
            let e = Env::default();
            e.ledger().set(LedgerInfo {
                timestamp: 1713139200,
                protocol_version: 22,
                sequence_number: 0,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 3110400,
            });
            e.mock_all_auths();

            let bombadil = Address::generate(&e);
            let backstop_id = create_backstop(&e);
            let to_add = Address::generate(&e);
            let to_remove = Address::generate(&e);

            let (blnd_id, _) = create_blnd_token(&e, &backstop_id, &bombadil);
            let (usdc_id, _) = create_usdc_token(&e, &backstop_id, &bombadil);
            create_comet_lp_pool_with_tokens_per_share(
                &e,
                &backstop_id,
                &bombadil,
                &blnd_id,
                5_0000000,
                &usdc_id,
                0_1000000,
            );
            let reward_zone: Vec<Address> = vec![
                &e,
                Address::generate(&e),
                to_remove.clone(),
                Address::generate(&e),
                Address::generate(&e),
                Address::generate(&e),
                Address::generate(&e),
                Address::generate(&e),
                to_add.clone(),
                Address::generate(&e),
                Address::generate(&e),
            ];

            e.as_contract(&backstop_id, || {
                storage::set_reward_zone(&e, &reward_zone);
                storage::set_last_distribution_time(&e, &(1713139200 - 60 * 60));
                storage::set_pool_balance(
                    &e,
                    &to_add,
                    &PoolBalance {
                        shares: 90_000_0000000,
                        tokens: 100_001_0000000,
                        q4w: 1_000_0000000,
                    },
                );
                storage::set_pool_balance(
                    &e,
                    &to_remove,
                    &PoolBalance {
                        shares: 90_000_0000000,
                        tokens: 100_000_0000000,
                        q4w: 1_000_0000000,
                    },
                );

                add_to_reward_zone(&e, to_add.clone(), Some(to_remove.clone()));
            });
        }

        /********** remove_from_reward_zone **********/

        #[test]
        fn test_remove_from_rz() {
            let e = Env::default();
            e.ledger().set(LedgerInfo {
                timestamp: 1713139200,
                protocol_version: 22,
                sequence_number: 0,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 3110400,
            });
            e.mock_all_auths();

            let bombadil = Address::generate(&e);
            let backstop_id = create_backstop(&e);
            let to_remove = Address::generate(&e);

            let (blnd_id, _) = create_blnd_token(&e, &backstop_id, &bombadil);
            let (usdc_id, _) = create_usdc_token(&e, &backstop_id, &bombadil);
            create_comet_lp_pool_with_tokens_per_share(
                &e,
                &backstop_id,
                &bombadil,
                &blnd_id,
                5_0000000,
                &usdc_id,
                0_1000000,
            );
            let mut reward_zone: Vec<Address> = vec![
                &e,
                Address::generate(&e),
                to_remove.clone(), // index 7
            ];

            e.as_contract(&backstop_id, || {
                storage::set_reward_zone(&e, &reward_zone);
                storage::set_last_distribution_time(&e, &(1713139200 - 60 * 60));
                storage::set_pool_balance(
                    &e,
                    &to_remove,
                    &PoolBalance {
                        shares: 90_000_0000000,
                        tokens: 100_001_0000000,
                        q4w: 1_000_0000000,
                    },
                );
                storage::set_pool_balance(
                    &e,
                    &to_remove,
                    &PoolBalance {
                        shares: 35_000_0000000,
                        tokens: 40_000_0000000,
                        q4w: 1_000_0000000,
                    },
                );
                storage::set_backstop_emis_data(
                    &e,
                    &to_remove,
                    &BackstopEmissionData {
                        eps: 0_10000000000000,
                        expiration: 1713139200 + 1000,
                        index: 0,
                        last_time: 1713139200 - 12345,
                    },
                );
                remove_from_reward_zone(&e, to_remove.clone());
                let actual_rz = storage::get_reward_zone(&e);
                reward_zone.remove(1);
                assert_eq!(actual_rz.len(), 1);
                assert_eq!(actual_rz, reward_zone);
            });
        }

        #[test]
        #[should_panic(expected = "Error(Contract, #1000)")]
        fn test_remove_from_rz_above_threshold() {
            let e = Env::default();
            e.ledger().set(LedgerInfo {
                timestamp: 1713139200,
                protocol_version: 22,
                sequence_number: 0,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 3110400,
            });
            e.mock_all_auths();

            let bombadil = Address::generate(&e);
            let backstop_id = create_backstop(&e);
            let to_remove = Address::generate(&e);

            let (blnd_id, _) = create_blnd_token(&e, &backstop_id, &bombadil);
            let (usdc_id, _) = create_usdc_token(&e, &backstop_id, &bombadil);
            create_comet_lp_pool_with_tokens_per_share(
                &e,
                &backstop_id,
                &bombadil,
                &blnd_id,
                5_0000000,
                &usdc_id,
                0_1000000,
            );
            let reward_zone: Vec<Address> = vec![
                &e,
                Address::generate(&e),
                to_remove.clone(), // index 7
            ];

            e.as_contract(&backstop_id, || {
                storage::set_reward_zone(&e, &reward_zone);
                storage::set_last_distribution_time(&e, &(1713139200 - 60 * 60));
                storage::set_pool_balance(
                    &e,
                    &to_remove,
                    &PoolBalance {
                        shares: 80_000_0000000,
                        tokens: 90_000_0000000,
                        q4w: 1_000_0000000,
                    },
                );
                storage::set_backstop_emis_data(
                    &e,
                    &to_remove,
                    &BackstopEmissionData {
                        eps: 0_10000000000000,
                        expiration: 1713139200 + 1000,
                        index: 0,
                        last_time: 1713139200 - 12345,
                    },
                );

                remove_from_reward_zone(&e, to_remove.clone());
            });
        }

        #[test]
        #[should_panic(expected = "Error(Contract, #1000)")]
        fn test_remove_from_rz_last_distribution_too_long_ago() {
            let e = Env::default();
            e.ledger().set(LedgerInfo {
                timestamp: 1713139200,
                protocol_version: 22,
                sequence_number: 0,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 3110400,
            });
            e.mock_all_auths();

            let bombadil = Address::generate(&e);
            let backstop_id = create_backstop(&e);
            let to_remove = Address::generate(&e);

            let (blnd_id, _) = create_blnd_token(&e, &backstop_id, &bombadil);
            let (usdc_id, _) = create_usdc_token(&e, &backstop_id, &bombadil);
            create_comet_lp_pool_with_tokens_per_share(
                &e,
                &backstop_id,
                &bombadil,
                &blnd_id,
                5_0000000,
                &usdc_id,
                0_1000000,
            );
            let reward_zone: Vec<Address> = vec![
                &e,
                Address::generate(&e),
                to_remove.clone(), // index 7
            ];

            e.as_contract(&backstop_id, || {
                storage::set_reward_zone(&e, &reward_zone);
                storage::set_last_distribution_time(&e, &(1713139200 - 60 * 60 - 1));
                storage::set_pool_balance(
                    &e,
                    &to_remove,
                    &PoolBalance {
                        shares: 80_000_0000000,
                        tokens: 90_000_0000000,
                        q4w: 1_000_0000000,
                    },
                );
                storage::set_backstop_emis_data(
                    &e,
                    &to_remove,
                    &BackstopEmissionData {
                        eps: 0_10000000000000,
                        expiration: 1713139200 + 1000,
                        index: 0,
                        last_time: 1713139200 - 12345,
                    },
                );

                remove_from_reward_zone(&e, to_remove.clone());
            });
        }

        #[test]
        #[should_panic(expected = "Error(Contract, #1002)")]
        fn test_remove_from_rz_not_in_rz() {
            let e = Env::default();
            e.ledger().set(LedgerInfo {
                timestamp: 1713139200,
                protocol_version: 22,
                sequence_number: 0,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 3110400,
            });
            e.mock_all_auths();

            let bombadil = Address::generate(&e);
            let backstop_id = create_backstop(&e);
            let to_remove = Address::generate(&e);

            let (blnd_id, _) = create_blnd_token(&e, &backstop_id, &bombadil);
            let (usdc_id, _) = create_usdc_token(&e, &backstop_id, &bombadil);
            create_comet_lp_pool_with_tokens_per_share(
                &e,
                &backstop_id,
                &bombadil,
                &blnd_id,
                5_0000000,
                &usdc_id,
                0_1000000,
            );
            let reward_zone: Vec<Address> = vec![&e, Address::generate(&e)];

            e.as_contract(&backstop_id, || {
                storage::set_reward_zone(&e, &reward_zone);
                storage::set_last_distribution_time(&e, &(1713139200 - 60 * 60));
                storage::set_pool_balance(
                    &e,
                    &to_remove,
                    &PoolBalance {
                        shares: 35_000_0000000,
                        tokens: 40_000_0000000,
                        q4w: 1_000_0000000,
                    },
                );
                storage::set_backstop_emis_data(
                    &e,
                    &to_remove,
                    &BackstopEmissionData {
                        eps: 0_10000000000000,
                        expiration: 1713139200 + 1000,
                        index: 0,
                        last_time: 1713139200 - 12345,
                    },
                );
                remove_from_reward_zone(&e, to_remove.clone());
            });
        }
    }
}

mod backstop_src_emissions_distributor {
    use cast::i128;

    use soroban_fixed_point_math::FixedPoint;

    use soroban_sdk::{panic_with_error, unwrap::UnwrapOptimized, Address, Env};

    use crate::{
        backstop::{PoolBalance, UserBalance},
        constants::{SCALAR_14, SCALAR_7},
        require_nonnegative,
        storage::{self, BackstopEmissionData, UserEmissionData},
        BackstopError,
    };

    pub(crate) use crate::emissions::distributor::*;

    mod tests {
        use crate::{testutils::create_backstop, Q4W};

        use super::*;
        use soroban_sdk::{
            testutils::{Address as _, Ledger, LedgerInfo},
            vec,
        };

        /********** update_emissions **********/

        #[test]
        fn test_update_emissions() {
            let e = Env::default();
            let block_timestamp = 1713139200 + 1234;
            e.ledger().set(LedgerInfo {
                timestamp: block_timestamp,
                protocol_version: 22,
                sequence_number: 0,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 3110400,
            });

            let backstop_id = create_backstop(&e);
            let pool_1 = Address::generate(&e);
            let samwise = Address::generate(&e);

            let backstop_emissions_data = BackstopEmissionData {
                expiration: 1713139200 + 7 * 24 * 60 * 60,
                eps: 0_10000000000000,
                index: 222220000000,
                last_time: 1713139200,
            };
            let user_emissions_data = UserEmissionData {
                index: 111110000000,
                accrued: 3,
            };
            e.as_contract(&backstop_id, || {
                storage::set_last_distribution_time(&e, &1713139200);
                storage::set_backstop_emis_data(&e, &pool_1, &backstop_emissions_data);
                storage::set_user_emis_data(&e, &pool_1, &samwise, &user_emissions_data);

                let pool_balance = PoolBalance {
                    shares: 150_0000000,
                    tokens: 200_0000000,
                    q4w: 0,
                };
                storage::set_pool_balance(&e, &pool_1, &pool_balance);
                let user_balance = UserBalance {
                    shares: 9_0000000,
                    q4w: vec![&e],
                };

                update_emissions(&e, &pool_1, &pool_balance, &samwise, &user_balance);

                let new_backstop_data =
                    storage::get_backstop_emis_data(&e, &pool_1).unwrap_optimized();
                let new_user_data =
                    storage::get_user_emis_data(&e, &pool_1, &samwise).unwrap_optimized();
                assert_eq!(new_backstop_data.last_time, block_timestamp);
                assert_eq!(new_backstop_data.index, 82488886666666);
                assert_eq!(new_user_data.accrued, 7_4140001);
                assert_eq!(new_user_data.index, 82488886666666);
            });
        }

        #[test]
        fn test_update_emissions_no_data() {
            let e = Env::default();
            let block_timestamp = 1713139200 + 1234;
            e.ledger().set(LedgerInfo {
                timestamp: block_timestamp,
                protocol_version: 22,
                sequence_number: 0,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 3110400,
            });

            let backstop_id = create_backstop(&e);
            let pool_1 = Address::generate(&e);
            let samwise = Address::generate(&e);

            e.as_contract(&backstop_id, || {
                storage::set_last_distribution_time(&e, &1713139200);

                let pool_balance = PoolBalance {
                    shares: 150_0000000,
                    tokens: 200_0000000,
                    q4w: 0,
                };
                let user_balance = UserBalance {
                    shares: 9_0000000,
                    q4w: vec![&e],
                };

                update_emissions(&e, &pool_1, &pool_balance, &samwise, &user_balance);

                let new_backstop_data = storage::get_backstop_emis_data(&e, &pool_1);
                let new_user_data = storage::get_user_emis_data(&e, &pool_1, &samwise);
                assert!(new_backstop_data.is_none());
                assert!(new_user_data.is_none());
            });
        }

        #[test]
        fn test_update_emissions_first_action() {
            let e = Env::default();
            let block_timestamp = 1713139200 + 12345;
            e.ledger().set(LedgerInfo {
                timestamp: block_timestamp,
                protocol_version: 22,
                sequence_number: 0,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 3110400,
            });

            let backstop_id = create_backstop(&e);
            let pool_1 = Address::generate(&e);
            let samwise = Address::generate(&e);

            let backstop_emissions_data = BackstopEmissionData {
                expiration: 1713139200 + 7 * 24 * 60 * 60,
                eps: 0_04200000000000,
                index: 222220000000,
                last_time: 1713139200,
            };
            e.as_contract(&backstop_id, || {
                storage::set_last_distribution_time(&e, &1713139200);

                storage::set_backstop_emis_data(&e, &pool_1, &backstop_emissions_data);

                let pool_balance = PoolBalance {
                    shares: 150_0000000,
                    tokens: 200_0000000,
                    q4w: 0,
                };
                let user_balance = UserBalance {
                    shares: 0,
                    q4w: vec![&e],
                };

                update_emissions(&e, &pool_1, &pool_balance, &samwise, &user_balance);

                let new_backstop_data =
                    storage::get_backstop_emis_data(&e, &pool_1).unwrap_optimized();
                let new_user_data =
                    storage::get_user_emis_data(&e, &pool_1, &samwise).unwrap_optimized();
                assert_eq!(new_backstop_data.last_time, block_timestamp);
                assert_eq!(new_backstop_data.index, 345882220000000);
                assert_eq!(new_user_data.accrued, 0);
                assert_eq!(new_user_data.index, 345882220000000);
            });
        }

        #[test]
        fn test_update_emissions_config_set_after_user() {
            let e = Env::default();
            let block_timestamp = 1713139200 + 12345;
            e.ledger().set(LedgerInfo {
                timestamp: block_timestamp,
                protocol_version: 22,
                sequence_number: 0,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 3110400,
            });

            let backstop_id = create_backstop(&e);
            let pool_1 = Address::generate(&e);
            let samwise = Address::generate(&e);

            let backstop_emissions_data = BackstopEmissionData {
                expiration: 1713139200 + 7 * 24 * 60 * 60,
                eps: 0_04200000000000,
                index: 0,
                last_time: 1713139200,
            };
            e.as_contract(&backstop_id, || {
                storage::set_last_distribution_time(&e, &1713139200);

                storage::set_backstop_emis_data(&e, &pool_1, &backstop_emissions_data);

                let pool_balance = PoolBalance {
                    shares: 150_0000000,
                    tokens: 200_0000000,
                    q4w: 0,
                };
                let user_balance = UserBalance {
                    shares: 9_0000000,
                    q4w: vec![&e],
                };

                update_emissions(&e, &pool_1, &pool_balance, &samwise, &user_balance);

                let new_backstop_data =
                    storage::get_backstop_emis_data(&e, &pool_1).unwrap_optimized();
                let new_user_data =
                    storage::get_user_emis_data(&e, &pool_1, &samwise).unwrap_optimized();
                assert_eq!(new_backstop_data.last_time, block_timestamp);
                assert_eq!(new_backstop_data.index, 345660000000000);
                assert_eq!(new_user_data.accrued, 31_1094000);
                assert_eq!(new_user_data.index, 345660000000000);
            });
        }

        #[test]
        fn test_update_emissions_q4w_not_counted() {
            let e = Env::default();
            let block_timestamp = 1713139200 + 1234;
            e.ledger().set(LedgerInfo {
                timestamp: block_timestamp,
                protocol_version: 22,
                sequence_number: 0,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 3110400,
            });

            let backstop_id = create_backstop(&e);
            let pool_1 = Address::generate(&e);
            let samwise = Address::generate(&e);

            let backstop_emissions_data = BackstopEmissionData {
                expiration: 1713139200 + 7 * 24 * 60 * 60,
                eps: 0_10000000000000,
                index: 222220000000,
                last_time: 1713139200,
            };
            let user_emissions_data = UserEmissionData {
                index: 111110000000,
                accrued: 3,
            };
            e.as_contract(&backstop_id, || {
                storage::set_last_distribution_time(&e, &1713139200);

                storage::set_backstop_emis_data(&e, &pool_1, &backstop_emissions_data);
                storage::set_user_emis_data(&e, &pool_1, &samwise, &user_emissions_data);

                let pool_balance = PoolBalance {
                    shares: 150_0000000,
                    tokens: 200_0000000,
                    q4w: 4_5000000,
                };
                let q4w: Q4W = Q4W {
                    amount: (4_5000000),
                    exp: (5000),
                };
                let user_balance = UserBalance {
                    shares: 4_5000000,
                    q4w: vec![&e, q4w],
                };

                update_emissions(&e, &pool_1, &pool_balance, &samwise, &user_balance);

                let new_backstop_data =
                    storage::get_backstop_emis_data(&e, &pool_1).unwrap_optimized();
                let new_user_data =
                    storage::get_user_emis_data(&e, &pool_1, &samwise).unwrap_optimized();
                assert_eq!(new_backstop_data.last_time, block_timestamp);
                assert_eq!(new_backstop_data.index, 85033216563573);
                assert_eq!(new_user_data.accrued, 38214950);
                assert_eq!(new_user_data.index, 85033216563573);
            });
        }

        #[test]
        fn test_update_emissions_fully_q4w_emissions_lost() {
            let e = Env::default();
            let block_timestamp = 1713139200 + 1234;
            e.ledger().set(LedgerInfo {
                timestamp: block_timestamp,
                protocol_version: 22,
                sequence_number: 0,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 3110400,
            });

            let backstop_id = create_backstop(&e);
            let pool_1 = Address::generate(&e);
            let samwise = Address::generate(&e);

            let backstop_emissions_data = BackstopEmissionData {
                expiration: 1713139200 + 7 * 24 * 60 * 60,
                eps: 0_10000000000000,
                index: 222220000000,
                last_time: 1713139200,
            };
            let user_emissions_data = UserEmissionData {
                index: 111110000000,
                accrued: 3,
            };
            e.as_contract(&backstop_id, || {
                storage::set_last_distribution_time(&e, &1713139200);

                storage::set_backstop_emis_data(&e, &pool_1, &backstop_emissions_data);
                storage::set_user_emis_data(&e, &pool_1, &samwise, &user_emissions_data);

                let pool_balance = PoolBalance {
                    shares: 150_0000000,
                    tokens: 200_0000000,
                    q4w: 150_0000000,
                };
                let q4w: Q4W = Q4W {
                    amount: (150_0000000),
                    exp: (5000),
                };
                let user_balance = UserBalance {
                    shares: 4_5000000,
                    q4w: vec![&e, q4w],
                };

                update_emissions(&e, &pool_1, &pool_balance, &samwise, &user_balance);

                let new_backstop_data =
                    storage::get_backstop_emis_data(&e, &pool_1).unwrap_optimized();
                let new_user_data =
                    storage::get_user_emis_data(&e, &pool_1, &samwise).unwrap_optimized();
                assert_eq!(new_backstop_data.last_time, block_timestamp);
                assert_eq!(new_backstop_data.index, backstop_emissions_data.index);
                assert_eq!(new_user_data.accrued, 50002);
                assert_eq!(new_user_data.index, backstop_emissions_data.index);
            });
        }

        #[test]
        fn test_claim_emissions() {
            let e = Env::default();
            let block_timestamp = 1713139200 + 1234;
            e.ledger().set(LedgerInfo {
                timestamp: block_timestamp,
                protocol_version: 22,
                sequence_number: 0,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 3110400,
            });

            let backstop_id = create_backstop(&e);
            let pool_1 = Address::generate(&e);
            let samwise = Address::generate(&e);

            let backstop_emissions_data = BackstopEmissionData {
                expiration: 1713139200 + 7 * 24 * 60 * 60,
                eps: 0_10000000000000,
                index: 222220000000,
                last_time: 1713139200,
            };
            let user_emissions_data = UserEmissionData {
                index: 111110000000,
                accrued: 3,
            };
            e.as_contract(&backstop_id, || {
                storage::set_last_distribution_time(&e, &1713139200);

                storage::set_backstop_emis_data(&e, &pool_1, &backstop_emissions_data);
                storage::set_user_emis_data(&e, &pool_1, &samwise, &user_emissions_data);

                let pool_balance = PoolBalance {
                    shares: 150_0000000,
                    tokens: 200_0000000,
                    q4w: 0,
                };
                storage::set_pool_balance(&e, &pool_1, &pool_balance);
                let user_balance = UserBalance {
                    shares: 9_0000000,
                    q4w: vec![&e],
                };

                let result = claim_emissions(&e, &pool_1, &pool_balance, &samwise, &user_balance);

                let new_backstop_data =
                    storage::get_backstop_emis_data(&e, &pool_1).unwrap_optimized();
                let new_user_data =
                    storage::get_user_emis_data(&e, &pool_1, &samwise).unwrap_optimized();
                assert_eq!(result, 7_4140001);
                assert_eq!(new_backstop_data.last_time, block_timestamp);
                assert_eq!(new_backstop_data.index, 82488886666666);
                assert_eq!(new_user_data.accrued, 0);
                assert_eq!(new_user_data.index, 82488886666666);
            });
        }

        #[test]
        #[should_panic(expected = "Error(Contract, #1000)")]
        fn test_claim_emissions_no_config() {
            let e = Env::default();
            let block_timestamp = 1713139200 + 1234;
            e.ledger().set(LedgerInfo {
                timestamp: block_timestamp,
                protocol_version: 22,
                sequence_number: 0,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 3110400,
            });

            let backstop_id = create_backstop(&e);
            let pool_1 = Address::generate(&e);
            let samwise = Address::generate(&e);

            e.as_contract(&backstop_id, || {
                storage::set_last_distribution_time(&e, &1713139200);

                let pool_balance = PoolBalance {
                    shares: 150_0000000,
                    tokens: 200_0000000,
                    q4w: 0,
                };
                let user_balance = UserBalance {
                    shares: 9_0000000,
                    q4w: vec![&e],
                };

                claim_emissions(&e, &pool_1, &pool_balance, &samwise, &user_balance);
            });
        }

        // @dev: The below tests should be impossible states to reach, but are left
        //       in to ensure any bad state does not result in incorrect emissions.

        #[test]
        #[should_panic(expected = "Error(Contract, #8)")]
        fn test_update_emissions_more_q4w_than_shares_panics() {
            let e = Env::default();
            let block_timestamp = 1713139200 + 1234;
            e.ledger().set(LedgerInfo {
                timestamp: block_timestamp,
                protocol_version: 22,
                sequence_number: 0,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 3110400,
            });

            let backstop_id = create_backstop(&e);
            let pool_1 = Address::generate(&e);
            let samwise = Address::generate(&e);

            let backstop_emissions_data = BackstopEmissionData {
                expiration: 1713139200 + 7 * 24 * 60 * 60,
                eps: 0_10000000000000,
                index: 22222,
                last_time: 1713139200,
            };
            let user_emissions_data = UserEmissionData {
                index: 11111,
                accrued: 3,
            };
            e.as_contract(&backstop_id, || {
                storage::set_last_distribution_time(&e, &1713139200);

                storage::set_backstop_emis_data(&e, &pool_1, &backstop_emissions_data);
                storage::set_user_emis_data(&e, &pool_1, &samwise, &user_emissions_data);

                let pool_balance = PoolBalance {
                    shares: 150_0000000,
                    tokens: 200_0000000,
                    q4w: 150_0000001,
                };
                let q4w: Q4W = Q4W {
                    amount: (4_5000000),
                    exp: (5000),
                };
                let user_balance = UserBalance {
                    shares: 4_5000000,
                    q4w: vec![&e, q4w],
                };

                update_emissions(&e, &pool_1, &pool_balance, &samwise, &user_balance);
            });
        }

        #[test]
        #[should_panic(expected = "attempt to subtract with overflow")]
        fn test_update_emissions_negative_time_dif() {
            let e = Env::default();
            let block_timestamp = 1713139200 + 1234;
            e.ledger().set(LedgerInfo {
                timestamp: block_timestamp,
                protocol_version: 22,
                sequence_number: 0,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 3110400,
            });

            let backstop_id = create_backstop(&e);
            let pool_1 = Address::generate(&e);
            let samwise = Address::generate(&e);

            let backstop_emissions_data = BackstopEmissionData {
                expiration: 1713139200 + 7 * 24 * 60 * 60,
                eps: 0_10000000000000,
                index: 22222,
                last_time: block_timestamp + 1,
            };
            let user_emissions_data = UserEmissionData {
                index: 11111,
                accrued: 3,
            };
            e.as_contract(&backstop_id, || {
                storage::set_last_distribution_time(&e, &1713139200);

                storage::set_backstop_emis_data(&e, &pool_1, &backstop_emissions_data);
                storage::set_user_emis_data(&e, &pool_1, &samwise, &user_emissions_data);

                let pool_balance = PoolBalance {
                    shares: 150_0000000,
                    tokens: 200_0000000,
                    q4w: 0,
                };
                let user_balance = UserBalance {
                    shares: 4_5000000,
                    q4w: vec![&e],
                };

                update_emissions(&e, &pool_1, &pool_balance, &samwise, &user_balance);
            });
        }

        #[test]
        #[should_panic(expected = "Error(Contract, #8)")]
        fn test_update_emissions_negative_user_index() {
            let e = Env::default();
            let block_timestamp = 1713139200 + 1234;
            e.ledger().set(LedgerInfo {
                timestamp: block_timestamp,
                protocol_version: 22,
                sequence_number: 0,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 10,
                min_persistent_entry_ttl: 10,
                max_entry_ttl: 3110400,
            });

            let backstop_id = create_backstop(&e);
            let pool_1 = Address::generate(&e);
            let samwise = Address::generate(&e);

            let backstop_emissions_data = BackstopEmissionData {
                expiration: 1713139200 + 7 * 24 * 60 * 60,
                eps: 0_10000000000000,
                index: 222220000000,
                last_time: 1713139200,
            };
            let user_emissions_data = UserEmissionData {
                index: 345660000000000 + 1,
                accrued: 3,
            };
            e.as_contract(&backstop_id, || {
                storage::set_last_distribution_time(&e, &1713139200);

                storage::set_backstop_emis_data(&e, &pool_1, &backstop_emissions_data);
                storage::set_user_emis_data(&e, &pool_1, &samwise, &user_emissions_data);

                let pool_balance = PoolBalance {
                    shares: 150_0000000,
                    tokens: 200_0000000,
                    q4w: 0,
                };
                let user_balance = UserBalance {
                    shares: 4_5000000,
                    q4w: vec![&e],
                };

                update_emissions(&e, &pool_1, &pool_balance, &samwise, &user_balance);
            });
        }
    }
}
