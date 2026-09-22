use super::{assert_pool_invariants, token_index};
use crate::model::Operation;
use crate::{contract_call, Mode, RunReport};
use blend_contract_kernel::pool::{admin_status, backstop_threshold, next_status, StatusError};
use pool::{PoolConfig, PoolDataKey};
use soroban_sdk::testutils::Address as _;
use soroban_sdk::xdr::ScErrorType;
use soroban_sdk::{Address, Error, InvokeError};
use std::fmt;
use test_suites::test_fixture::TestFixture;
use test_suites::create_fixture_with_data;

pub fn run(operations: &[Operation], mode: Mode) -> RunReport {
    let fixture = create_fixture_with_data(mode.uses_wasm());
    let pool = &fixture.pools[0].pool;
    let proposed_admin = Address::generate(&fixture.env);
    let mut report = RunReport::decoded(operations.len());

    for operation in operations {
        match operation.code % 10 {
            0 => {
                let result = pool.try_propose_admin(&proposed_admin);
                let _ = contract_call(&fixture.env, result, &mut report);
            }
            1 => {
                let result = pool.try_accept_admin();
                let _ = contract_call(&fixture.env, result, &mut report);
            }
            2 => {
                let take_rate = operation.amount % 10_000_002;
                let max_positions = u32::from(operation.actor % 64);
                let magnitude = i128::from(operation.amount);
                let min_collateral = if operation.flags & 1 == 0 {
                    magnitude
                } else {
                    -magnitude - 1
                };
                let expected = take_rate < 10_000_000
                    && (2..=60).contains(&max_positions)
                    && min_collateral >= 0;
                let result = pool.try_update_pool(&take_rate, &max_positions, &min_collateral);
                let applied = contract_call(&fixture.env, result, &mut report).is_some();
                // MUTATION-CHECK: changing any factory/pool comparison changes
                // this independently computed accept/reject decision.
                assert_eq!(applied, expected);
                if applied {
                    let config = pool.get_config();
                    assert_eq!(config.bstop_rate, take_rate);
                    assert_eq!(config.max_positions, max_positions);
                    assert_eq!(config.min_collateral, min_collateral);
                }
            }
            3 => {
                let token = &fixture.tokens[token_index(operation.asset)];
                let mut config = pool.get_reserve(&token.address).config;
                config.enabled = operation.flags & 1 == 0;
                config.supply_cap = i128::from(operation.amount).saturating_add(1);
                let result = pool.try_queue_set_reserve(&token.address, &config);
                let _ = contract_call(&fixture.env, result, &mut report);
            }
            4 => {
                let token = &fixture.tokens[token_index(operation.asset)];
                let result = pool.try_cancel_set_reserve(&token.address);
                let _ = contract_call(&fixture.env, result, &mut report);
            }
            5 => {
                let token = &fixture.tokens[token_index(operation.asset)];
                let queued = fixture.env.as_contract(&pool.address, || {
                    fixture
                        .env
                        .storage()
                        .temporary()
                        .has(&PoolDataKey::ResInit(token.address.clone()))
                });
                if queued {
                    let result = pool.try_set_reserve(&token.address);
                    let _ = contract_call(&fixture.env, result, &mut report);
                } else {
                    report.noop();
                }
            }
            6 => {
                fixture.jump(7 * 24 * 60 * 60 + u64::from(operation.amount % 2));
                report.applied();
            }
            7 => {
                assert_set_status(&fixture, &mut report, operation.amount % 7);
            }
            8 => {
                assert_update_status(&fixture, &mut report);
            }
            _ => {
                assert_eq!(pool.get_reserve_list().len(), 3);
                let _ = pool.get_admin();
                let _ = pool.get_config();
                report.noop();
            }
        }
        assert_pool_invariants(&fixture, &mut report);
    }

    report
}

fn assert_set_status(fixture: &TestFixture<'_>, report: &mut RunReport, requested: u32) {
    let pool = &fixture.pools[0].pool;
    let before = pool.get_config();
    let backstop = fixture.backstop.pool_data(&pool.address);
    let met_threshold = backstop_threshold(backstop.blnd, backstop.usdc) >= 10_000_000;
    let expected = admin_status(requested, backstop.q4w_pct, met_threshold);
    let result = pool.try_set_status(&requested);

    match expected {
        Ok(expected_status) => {
            assert!(
                contract_call(&fixture.env, result, report).is_some(),
                "kernel-accepted set_status must succeed"
            );
            assert_config_status(&before, &pool.get_config(), expected_status);
        }
        Err(expected_error) => {
            assert_exact_status_error(&result, expected_error);
            assert!(contract_call(&fixture.env, result, report).is_none());
            assert_config_equal(&before, &pool.get_config());
        }
    }
}

fn assert_update_status(fixture: &TestFixture<'_>, report: &mut RunReport) {
    let pool = &fixture.pools[0].pool;
    let before = pool.get_config();
    let backstop = fixture.backstop.pool_data(&pool.address);
    let met_threshold = backstop_threshold(backstop.blnd, backstop.usdc) >= 10_000_000;
    let expected = next_status(before.status, backstop.q4w_pct, met_threshold);
    let result = pool.try_update_status();

    match expected {
        Ok(expected_status) => {
            let returned = contract_call(&fixture.env, result, report)
                .expect("kernel-accepted update_status must succeed");
            assert_eq!(returned, expected_status);
            assert_config_status(&before, &pool.get_config(), expected_status);
        }
        Err(expected_error) => {
            assert_exact_status_error(&result, expected_error);
            assert!(contract_call(&fixture.env, result, report).is_none());
            assert_config_equal(&before, &pool.get_config());
        }
    }
}

fn assert_exact_status_error<T, E: fmt::Debug>(
    result: &Result<Result<T, E>, Result<Error, InvokeError>>,
    expected: StatusError,
) {
    let expected_code = match expected {
        StatusError::BadRequest => 1200,
        StatusError::StatusNotAllowed => 1204,
    };
    match result {
        Err(Ok(error)) => {
            assert!(
                error.is_type(ScErrorType::Contract),
                "unexpected non-contract invocation error: {error:?}"
            );
            assert_eq!(*error, Error::from_contract_error(expected_code));
        }
        _ => panic!("expected contract status error {expected_code}"),
    }
}

fn assert_config_equal(expected: &PoolConfig, actual: &PoolConfig) {
    assert_eq!(actual.oracle, expected.oracle);
    assert_eq!(actual.min_collateral, expected.min_collateral);
    assert_eq!(actual.bstop_rate, expected.bstop_rate);
    assert_eq!(actual.status, expected.status);
    assert_eq!(actual.max_positions, expected.max_positions);
}

fn assert_config_status(before: &PoolConfig, after: &PoolConfig, expected_status: u32) {
    assert_eq!(after.oracle, before.oracle);
    assert_eq!(after.min_collateral, before.min_collateral);
    assert_eq!(after.bstop_rate, before.bstop_rate);
    assert_eq!(after.status, expected_status);
    assert_eq!(after.max_positions, before.max_positions);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn status_regressions(mode: Mode) {
        let fixture = create_fixture_with_data(mode.uses_wasm());
        let pool = &fixture.pools[0].pool;
        let mut report = RunReport::decoded(8);

        for invalid in [1, 5, 6] {
            assert_set_status(&fixture, &mut report, invalid);
        }
        assert_eq!(report.rejected, 3);

        assert_set_status(&fixture, &mut report, 4);
        let frozen = pool.get_config();
        assert_eq!(frozen.status, 4);
        assert_update_status(&fixture, &mut report);
        assert_config_equal(&frozen, &pool.get_config());

        assert_set_status(&fixture, &mut report, 3);
        assert_update_status(&fixture, &mut report);
        let backstop = fixture.backstop.pool_data(&pool.address);
        let expected = next_status(
            3,
            backstop.q4w_pct,
            backstop_threshold(backstop.blnd, backstop.usdc) >= 10_000_000,
        )
        .expect("status 3 automatic transition must be legal");
        assert_eq!(pool.get_config().status, expected);
    }

    #[test]
    fn native_status_calls_match_kernel_and_error_mapping() {
        status_regressions(Mode::Native);
    }

    #[test]
    fn wasm_status_calls_match_kernel_and_error_mapping() {
        status_regressions(Mode::Wasm);
    }

    fn queued_half_transitions_admin_active_to_on_ice(mode: Mode) {
        let fixture = create_fixture_with_data(mode.uses_wasm());
        let pool = &fixture.pools[0].pool;
        let user = &fixture.users[0];
        let mut report = RunReport::decoded(2);

        assert_set_status(&fixture, &mut report, 0);
        let user_balance = fixture.backstop.user_balance(&pool.address, user);
        assert!(user_balance.q4w.is_empty());
        let queue_shares = (user_balance.shares + 1) / 2;
        fixture
            .backstop
            .queue_withdrawal(user, &pool.address, &queue_shares);
        let backstop = fixture.backstop.pool_data(&pool.address);
        assert!(backstop.q4w_pct >= 5_000_000);

        assert_update_status(&fixture, &mut report);
        assert_eq!(pool.get_config().status, 3);
    }

    #[test]
    fn native_queued_half_triggers_automatic_on_ice() {
        queued_half_transitions_admin_active_to_on_ice(Mode::Native);
    }

    #[test]
    fn wasm_queued_half_triggers_automatic_on_ice() {
        queued_half_transitions_admin_active_to_on_ice(Mode::Wasm);
    }
}
