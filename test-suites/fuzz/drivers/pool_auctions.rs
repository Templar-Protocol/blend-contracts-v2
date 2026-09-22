use super::assert_pool_invariants;
use crate::model::Operation;
use crate::{contract_call, Mode, RunReport};
use soroban_sdk::vec;
use test_suites::create_fixture_with_data;
use test_suites::test_fixture::TokenIndex;

pub fn run(operations: &[Operation], mode: Mode) -> RunReport {
    let fixture = create_fixture_with_data(mode.uses_wasm());
    fixture.jump(104 * 7 * 24 * 60 * 60);
    let pool = &fixture.pools[0].pool;
    let auction_user = fixture.backstop.address.clone();
    let mut interest_start = None;
    let mut report = RunReport::decoded(operations.len());

    for operation in operations {
        match operation.code % 7 {
            0 => {
                let blocks = operation.amount % 751;
                fixture.jump_with_sequence(u64::from(blocks) * 5);
                report.applied();
            }
            1 => {
                let bid = vec![&fixture.env, fixture.lp.address.clone()];
                let lot = vec![
                    &fixture.env,
                    fixture.tokens[TokenIndex::STABLE].address.clone(),
                    fixture.tokens[TokenIndex::WETH].address.clone(),
                    fixture.tokens[TokenIndex::XLM].address.clone(),
                ];
                let result = pool.try_new_auction(&2, &auction_user, &bid, &lot, &100);
                let created = contract_call(&fixture.env, result, &mut report);
                if let Some(auction) = created {
                    assert!(!auction.bid.is_empty());
                    assert!(!auction.lot.is_empty());
                    interest_start = Some(fixture.env.ledger().sequence());
                }
            }
            2 => {
                if interest_start.is_some() {
                    let result = pool.try_get_auction(&2, &auction_user);
                    contract_call(&fixture.env, result, &mut report)
                        .expect("created interest auction must remain readable");
                } else {
                    report.noop();
                }
            }
            3 => {
                let result = pool.try_del_auction(&2, &auction_user);
                let deleted = contract_call(&fixture.env, result, &mut report).is_some();
                if let Some(start) = interest_start {
                    let elapsed = fixture.env.ledger().sequence().saturating_sub(start);
                    // MUTATION-CHECK: the production stale-auction comparison is
                    // strict at 500 elapsed blocks; deletion starts at block 501.
                    assert_eq!(deleted, elapsed > 500);
                    if deleted {
                        interest_start = None;
                    }
                } else {
                    assert!(!deleted);
                }
            }
            4 => {
                let user = if operation.actor & 1 == 0 {
                    &fixture.users[0]
                } else {
                    &auction_user
                };
                let result = pool.try_bad_debt(user);
                let _ = contract_call(&fixture.env, result, &mut report);
            }
            5 => {
                let user = &fixture.users[0];
                let bid = vec![
                    &fixture.env,
                    fixture.tokens[TokenIndex::STABLE].address.clone(),
                ];
                let lot = vec![
                    &fixture.env,
                    fixture.tokens[TokenIndex::XLM].address.clone(),
                ];
                let percent = operation.amount % 101;
                let result = pool.try_new_auction(&0, user, &bid, &lot, &percent);
                let _ = contract_call(&fixture.env, result, &mut report);
            }
            _ => {
                let _ = pool.get_positions(&auction_user);
                report.noop();
            }
        }
        assert_pool_invariants(&fixture, &mut report);
    }

    report
}
