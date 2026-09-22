use super::{amount, assert_pool_invariants, create_actor, token_index};
use crate::model::Operation;
use crate::{contract_call, Mode, RunReport};
use num_bigint::BigInt;
use pool::{Request, RequestType};
use sep_40_oracle::{Asset, PriceFeedClient};
use soroban_sdk::{vec, Address, Vec};
use test_suites::create_fixture_with_data;
use test_suites::test_fixture::{TestFixture, TokenIndex};

pub fn run(operations: &[Operation], mode: Mode) -> RunReport {
    let mut fixture = create_fixture_with_data(mode.uses_wasm());
    let first = create_actor(&mut fixture);
    let second = create_actor(&mut fixture);
    let actors = [first, second];
    let mut report = RunReport::decoded(operations.len());

    for operation in operations {
        let actor = &actors[usize::from(operation.actor & 1)];
        let pool = &fixture.pools[0].pool;
        match operation.code % 9 {
            0 => {
                fixture.jump(u64::from(operation.amount % 86_401));
                report.applied();
            }
            1..=6 => {
                let request_type = match operation.code % 9 {
                    1 => RequestType::Supply,
                    2 => RequestType::Withdraw,
                    3 => RequestType::SupplyCollateral,
                    4 => RequestType::WithdrawCollateral,
                    5 => RequestType::Borrow,
                    _ => RequestType::Repay,
                };
                let check_health = matches!(
                    request_type,
                    RequestType::Borrow | RequestType::WithdrawCollateral
                );
                let token = token_index(operation.asset);
                let requests = vec![
                    &fixture.env,
                    Request {
                        request_type: request_type as u32,
                        address: fixture.tokens[token].address.clone(),
                        amount: amount(*operation, 1_000),
                    },
                ];
                let result = if operation.flags & 1 == 0 {
                    pool.try_submit(actor, actor, actor, &requests)
                } else {
                    pool.try_submit_with_allowance(actor, actor, actor, &requests)
                };
                if let Some(returned) = contract_call(&fixture.env, result, &mut report) {
                    let stored = pool.get_positions(actor);
                    assert_eq!(returned.collateral.len(), stored.collateral.len());
                    assert_eq!(returned.liabilities.len(), stored.liabilities.len());
                    assert_eq!(returned.supply.len(), stored.supply.len());
                    if check_health {
                        assert_submit_health(&fixture, actor);
                    }
                }
            }
            7 => {
                let reserve_ids: Vec<u32> = vec![&fixture.env, 0, 3];
                let result = pool.try_claim(actor, &reserve_ids, actor);
                if let Some(claimed) = contract_call(&fixture.env, result, &mut report) {
                    assert!(claimed >= 0);
                    report.observe_i128(claimed);
                }
            }
            _ => {
                let reserve_list = pool.get_reserve_list();
                assert_eq!(reserve_list.len(), 3);
                let _ = pool.get_config();
                let _ = pool.get_positions(actor);
                report.noop();
            }
        }
        assert_pool_invariants(&fixture, &mut report);
        assert_reserve_accounting(&fixture);
    }

    report
}

fn mul_div_floor(a: &BigInt, b: &BigInt, denominator: &BigInt) -> BigInt {
    assert!(a >= &BigInt::from(0));
    assert!(b >= &BigInt::from(0));
    assert!(denominator > &BigInt::from(0));
    a * b / denominator
}

fn mul_div_ceil(a: &BigInt, b: &BigInt, denominator: &BigInt) -> BigInt {
    assert!(a >= &BigInt::from(0));
    assert!(b >= &BigInt::from(0));
    assert!(denominator > &BigInt::from(0));
    (a * b + denominator - 1) / denominator
}

fn assert_reserve_accounting(fixture: &TestFixture<'_>) {
    let pool = &fixture.pools[0].pool;
    let rate_scalar = BigInt::from(1_000_000_000_000i128);

    for token_index in [TokenIndex::STABLE, TokenIndex::XLM, TokenIndex::WETH] {
        let token = &fixture.tokens[token_index];
        let reserve = pool.get_reserve(&token.address);
        let supplier_claim = mul_div_floor(
            &BigInt::from(reserve.data.b_supply),
            &BigInt::from(reserve.data.b_rate),
            &rate_scalar,
        );
        let borrower_debt = mul_div_ceil(
            &BigInt::from(reserve.data.d_supply),
            &BigInt::from(reserve.data.d_rate),
            &rate_scalar,
        );
        let assets = BigInt::from(token.balance(&pool.address)) + borrower_debt;
        let claims = supplier_claim + BigInt::from(reserve.data.backstop_credit);

        assert!(
            assets >= claims,
            "reserve accounting deficit for {:?}: assets={assets}, claims={claims}",
            token.address
        );
    }
}

fn assert_submit_health(fixture: &TestFixture<'_>, actor: &Address) {
    let pool = &fixture.pools[0].pool;
    let positions = pool.get_positions(actor);
    let config = pool.get_config();
    let oracle = PriceFeedClient::new(&fixture.env, &fixture.oracle.address);
    let oracle_scalar = BigInt::from(10).pow(oracle.decimals());
    let rate_scalar = BigInt::from(1_000_000_000_000i128);
    let factor_scalar = BigInt::from(10_000_000i128);
    let mut collateral_base = BigInt::from(0);
    let mut liability_base = BigInt::from(0);

    for asset in pool.get_reserve_list() {
        let reserve = pool.get_reserve(&asset);
        let collateral_shares = positions.collateral.get(reserve.config.index).unwrap_or(0);
        let debt_shares = positions.liabilities.get(reserve.config.index).unwrap_or(0);
        if collateral_shares == 0 && debt_shares == 0 {
            continue;
        }

        let price = oracle
            .lastprice(&Asset::Stellar(asset))
            .expect("fixture oracle must contain every reserve price")
            .price;
        assert!(price > 0, "fixture oracle price must be positive");
        let price = BigInt::from(price);
        let asset_scalar = BigInt::from(10).pow(reserve.config.decimals);

        if collateral_shares > 0 {
            let collateral_assets = mul_div_floor(
                &BigInt::from(collateral_shares),
                &BigInt::from(reserve.data.b_rate),
                &rate_scalar,
            );
            let effective_collateral = mul_div_floor(
                &collateral_assets,
                &BigInt::from(reserve.config.c_factor),
                &factor_scalar,
            );
            collateral_base += mul_div_floor(&price, &effective_collateral, &asset_scalar);
        }

        if debt_shares > 0 {
            let debt_assets = mul_div_ceil(
                &BigInt::from(debt_shares),
                &BigInt::from(reserve.data.d_rate),
                &rate_scalar,
            );
            let effective_debt = mul_div_ceil(
                &debt_assets,
                &factor_scalar,
                &BigInt::from(reserve.config.l_factor),
            );
            liability_base += mul_div_ceil(&price, &effective_debt, &asset_scalar);
        }
    }

    if positions.liabilities.is_empty() {
        return;
    }

    if liability_base > BigInt::from(0) {
        let health = mul_div_floor(&collateral_base, &oracle_scalar, &liability_base);
        let minimum = mul_div_floor(&oracle_scalar, &BigInt::from(1_0000100i128), &factor_scalar);
        assert!(
            health >= minimum,
            "post-submit health below minimum: health={health}, minimum={minimum}"
        );
    }
    assert!(
        collateral_base >= BigInt::from(config.min_collateral),
        "post-submit collateral below pool minimum"
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use pool::PoolDataKey;
    use soroban_sdk::{xdr::ScErrorType, Error};

    fn assert_funded_fixture(mode: Mode) {
        let fixture = create_fixture_with_data(mode.uses_wasm());
        let stable = &fixture.tokens[TokenIndex::STABLE];
        let reserve = fixture.pools[0].pool.get_reserve(&stable.address);
        assert!(reserve.data.d_rate > 1_000_000_000_000);
        assert_reserve_accounting(&fixture);
        assert_submit_health(&fixture, &fixture.users[0]);
    }

    #[test]
    fn funded_borrow_with_accrual_satisfies_oracles() {
        for mode in [Mode::Native, Mode::Wasm] {
            assert_funded_fixture(mode);
        }
    }

    #[test]
    fn debt_rejection_then_donation_gulp_preserve_reserve_coverage() {
        for mode in [Mode::Native, Mode::Wasm] {
            let fixture = create_fixture_with_data(mode.uses_wasm());
            let pool = &fixture.pools[0].pool;
            let stable = &fixture.tokens[TokenIndex::STABLE];
            let before_rejection = fixture.read_reserve_data(0, TokenIndex::STABLE);

            match pool.try_gulp(&stable.address) {
                Err(Ok(error)) => {
                    assert!(error.is_type(ScErrorType::Contract));
                    assert_eq!(error, Error::from_contract_error(1200));
                }
                other => panic!("debt-positive gulp must reject with 1200: {other:?}"),
            }
            let after_rejection = fixture.read_reserve_data(0, TokenIndex::STABLE);
            assert_eq!(after_rejection.d_rate, before_rejection.d_rate);
            assert_eq!(after_rejection.b_rate, before_rejection.b_rate);
            assert_eq!(after_rejection.ir_mod, before_rejection.ir_mod);
            assert_eq!(after_rejection.b_supply, before_rejection.b_supply);
            assert_eq!(after_rejection.d_supply, before_rejection.d_supply);
            assert_eq!(
                after_rejection.backstop_credit,
                before_rejection.backstop_credit
            );
            assert_eq!(after_rejection.last_time, before_rejection.last_time);

            let borrower = &fixture.users[0];
            let requests = soroban_sdk::vec![
                &fixture.env,
                Request {
                    request_type: RequestType::Repay as u32,
                    address: stable.address.clone(),
                    amount: stable.balance(borrower),
                },
            ];
            pool.submit(borrower, borrower, borrower, &requests);
            assert_eq!(fixture.read_reserve_data(0, TokenIndex::STABLE).d_supply, 0);

            let donation = 123 * 1_000_000i128;
            stable.mint(&pool.address, &donation);
            assert_reserve_accounting(&fixture);
            assert_eq!(pool.gulp(&stable.address), 0);
            assert_reserve_accounting(&fixture);
        }
    }

    fn assert_unbacked_supplier_shares_fail(mode: Mode) {
        let fixture = create_fixture_with_data(mode.uses_wasm());
        let pool = &fixture.pools[0].pool;
        let stable = &fixture.tokens[TokenIndex::STABLE];
        let mut data = fixture.read_reserve_data(0, TokenIndex::STABLE);
        data.b_supply += 1_000_000 * 1_000_000i128;
        data.last_time = fixture.env.ledger().timestamp();
        fixture.env.as_contract(&pool.address, || {
            fixture
                .env
                .storage()
                .persistent()
                .set(&PoolDataKey::ResData(stable.address.clone()), &data);
        });

        assert_reserve_accounting(&fixture);
    }

    #[test]
    #[should_panic(expected = "reserve accounting deficit")]
    fn native_unbacked_supplier_shares_are_detected() {
        assert_unbacked_supplier_shares_fail(Mode::Native);
    }

    #[test]
    #[should_panic(expected = "reserve accounting deficit")]
    fn wasm_unbacked_supplier_shares_are_detected() {
        assert_unbacked_supplier_shares_fail(Mode::Wasm);
    }

    fn assert_unsafe_recorded_position_fails(mode: Mode) {
        let mut fixture = create_fixture_with_data(mode.uses_wasm());
        let actor = create_actor(&mut fixture);
        let pool = &fixture.pools[0].pool;
        let stable = &fixture.tokens[TokenIndex::STABLE];
        let requests = soroban_sdk::vec![
            &fixture.env,
            Request {
                request_type: RequestType::SupplyCollateral as u32,
                address: stable.address.clone(),
                amount: 100 * 1_000_000,
            },
            Request {
                request_type: RequestType::Borrow as u32,
                address: stable.address.clone(),
                amount: 50 * 1_000_000,
            },
        ];
        let mut positions = pool.submit(&actor, &actor, &actor, &requests);
        positions.liabilities.set(0, 90 * 1_000_000);
        fixture.env.as_contract(&pool.address, || {
            fixture
                .env
                .storage()
                .persistent()
                .set(&PoolDataKey::Positions(actor.clone()), &positions);
        });

        assert_submit_health(&fixture, &actor);
    }

    #[test]
    #[should_panic(expected = "post-submit health below minimum")]
    fn native_unsafe_recorded_position_is_detected() {
        assert_unsafe_recorded_position_fails(Mode::Native);
    }

    #[test]
    #[should_panic(expected = "post-submit health below minimum")]
    fn wasm_unsafe_recorded_position_is_detected() {
        assert_unsafe_recorded_position_fails(Mode::Wasm);
    }
}
