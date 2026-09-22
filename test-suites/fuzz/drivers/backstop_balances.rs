use crate::model::Operation;
use crate::{contract_call, Mode, RunReport};
use test_suites::create_fixture_with_data;
use test_suites::test_fixture::SCALAR_7;

pub fn run(operations: &[Operation], mode: Mode) -> RunReport {
    let fixture = create_fixture_with_data(mode.uses_wasm());
    let user = fixture.users[0].clone();
    let pool = fixture.pools[0].pool.address.clone();
    let mut report = RunReport::decoded(operations.len());

    for operation in operations {
        let amount = i128::from(operation.amount % 1_000 + 1) * SCALAR_7;
        match operation.code % 8 {
            0 => {
                let before = fixture.backstop.pool_data(&pool);
                let result = fixture.backstop.try_deposit(&user, &pool, &amount);
                if let Some(minted) = contract_call(&fixture.env, result, &mut report) {
                    let after = fixture.backstop.pool_data(&pool);
                    assert!(minted > 0);
                    assert_eq!(after.tokens - before.tokens, amount);
                    assert_eq!(after.shares - before.shares, minted);
                }
            }
            1 => {
                let before = fixture.backstop.pool_data(&pool);
                let result = fixture.backstop.try_queue_withdrawal(&user, &pool, &amount);
                if let Some(queue) = contract_call(&fixture.env, result, &mut report) {
                    let after = fixture.backstop.pool_data(&pool);
                    assert_eq!(queue.amount, amount);
                    assert_eq!(after.tokens, before.tokens);
                    assert_eq!(after.shares, before.shares);
                    assert!(queue.exp > fixture.env.ledger().timestamp());
                }
            }
            2 => {
                let before = fixture.backstop.pool_data(&pool);
                let result = fixture
                    .backstop
                    .try_dequeue_withdrawal(&user, &pool, &amount);
                if contract_call(&fixture.env, result, &mut report).is_some() {
                    let after = fixture.backstop.pool_data(&pool);
                    assert_eq!(after.tokens, before.tokens);
                    assert_eq!(after.shares, before.shares);
                    assert!(after.q4w_pct <= before.q4w_pct);
                }
            }
            3 => {
                let days = 16 + u64::from(operation.amount % 3);
                fixture.jump(days * 24 * 60 * 60);
                report.applied();
            }
            4 => {
                let before = fixture.backstop.pool_data(&pool);
                let queued = fixture.backstop.user_balance(&pool, &user);
                let now = fixture.env.ledger().timestamp();
                let matured = queued
                    .q4w
                    .iter()
                    .filter(|entry| entry.exp <= now)
                    .fold(0_i128, |total, entry| total + entry.amount);
                let user_tokens = fixture.lp.balance(&user);
                let result = fixture.backstop.try_withdraw(&user, &pool, &amount);
                let withdrawn = contract_call(&fixture.env, result, &mut report);
                // MUTATION-CHECK: a queued withdrawal matures at expiration,
                // not one second after it.
                assert_eq!(withdrawn.is_some(), matured >= amount);
                if let Some(withdrawn) = withdrawn {
                    let after = fixture.backstop.pool_data(&pool);
                    assert!(withdrawn > 0);
                    assert_eq!(before.tokens - after.tokens, withdrawn);
                    assert_eq!(before.shares - after.shares, amount);
                    assert_eq!(fixture.lp.balance(&user) - user_tokens, withdrawn);
                }
            }
            5 => {
                fixture.lp.approve(
                    &user,
                    &fixture.backstop.address,
                    &amount,
                    &fixture.env.ledger().sequence().saturating_add(1),
                );
                let before = fixture.backstop.pool_data(&pool);
                let result = fixture.backstop.try_donate(&user, &pool, &amount);
                if contract_call(&fixture.env, result, &mut report).is_some() {
                    let after = fixture.backstop.pool_data(&pool);
                    assert_eq!(after.tokens - before.tokens, amount);
                    assert_eq!(after.shares, before.shares);
                }
            }
            6 => {
                let before = fixture.backstop.pool_data(&pool);
                let result = fixture.backstop.try_draw(&pool, &amount, &user);
                if contract_call(&fixture.env, result, &mut report).is_some() {
                    let after = fixture.backstop.pool_data(&pool);
                    assert_eq!(before.tokens - after.tokens, amount);
                    assert_eq!(after.shares, before.shares);
                }
            }
            _ => {
                assert_eq!(fixture.backstop.backstop_token(), fixture.lp.address);
                let _ = fixture.backstop.user_balance(&pool, &user);
                report.noop();
            }
        }
        assert_invariants(&fixture, &pool, &user, &mut report);
    }

    report
}

fn assert_invariants(
    fixture: &test_suites::test_fixture::TestFixture<'_>,
    pool: &soroban_sdk::Address,
    user: &soroban_sdk::Address,
    report: &mut RunReport,
) {
    let pool_data = fixture.backstop.pool_data(pool);
    let user_balance = fixture.backstop.user_balance(pool, user);
    assert!(pool_data.tokens >= 0);
    assert!(pool_data.shares >= 0);
    assert!((0..=SCALAR_7).contains(&pool_data.q4w_pct));
    assert!(user_balance.shares >= 0);
    for queued in user_balance.q4w.iter() {
        assert!(queued.amount > 0);
    }
    // With one configured pool, every LP token held by the backstop belongs to
    // that pool's independently queried accounting balance.
    assert_eq!(
        fixture.lp.balance(&fixture.backstop.address),
        pool_data.tokens
    );
    report.observe_i128(pool_data.tokens);
    report.observe_i128(pool_data.shares);
    report.observe_i128(pool_data.q4w_pct);
}
