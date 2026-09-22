mod backstop_balances;
mod emissions;
mod pool_admin;
mod pool_auctions;
mod pool_factory;
mod pool_flash_loan;
mod pool_general;

use crate::model::Operation;
use crate::{Mode, RunReport, Target};
use soroban_sdk::testutils::Address as _;
use soroban_sdk::Address;
use test_suites::test_fixture::{TestFixture, TokenIndex, SCALAR_7};

pub fn run(target: Target, operations: &[Operation], mode: Mode) -> RunReport {
    match target {
        Target::PoolGeneral => pool_general::run(operations, mode),
        Target::PoolAdmin => pool_admin::run(operations, mode),
        Target::PoolAuctions => pool_auctions::run(operations, mode),
        Target::PoolFlashLoan => pool_flash_loan::run(operations, mode),
        Target::BackstopBalances => backstop_balances::run(operations, mode),
        Target::Emissions => emissions::run(operations, mode),
        Target::PoolFactory => pool_factory::run(operations, mode),
    }
}

pub fn create_actor(fixture: &mut TestFixture<'_>) -> Address {
    let actor = Address::generate(&fixture.env);
    fixture.users.push(actor.clone());
    for token_index in [TokenIndex::WETH, TokenIndex::XLM, TokenIndex::STABLE] {
        let amount = 1_000_000i128 * scalar(token_index);
        let token = &fixture.tokens[token_index];
        token.mint(&actor, &amount);
        token.approve(
            &actor,
            &fixture.pools[0].pool.address,
            &i128::MAX,
            &fixture.env.ledger().sequence().saturating_add(1_000_000),
        );
    }
    actor
}

pub const fn token_index(selector: u8) -> TokenIndex {
    match selector % 3 {
        0 => TokenIndex::STABLE,
        1 => TokenIndex::XLM,
        _ => TokenIndex::WETH,
    }
}

pub const fn scalar(token: TokenIndex) -> i128 {
    match token {
        TokenIndex::STABLE => 1_000_000,
        TokenIndex::WETH => 1_000_000_000,
        _ => SCALAR_7,
    }
}

pub fn amount(operation: Operation, maximum_whole: u32) -> i128 {
    let token = token_index(operation.asset);
    let whole = operation.amount % maximum_whole + 1;
    i128::from(whole) * scalar(token)
}

pub fn assert_pool_invariants(fixture: &TestFixture<'_>, report: &mut RunReport) {
    let pool = &fixture.pools[0].pool;
    let config = pool.get_config();

    // MUTATION-CHECK: pool/factory bound comparisons must keep every stored
    // configuration inside this independently restated public domain.
    assert!(config.bstop_rate < 10_000_000);
    assert!((2..=60).contains(&config.max_positions));
    assert!(config.min_collateral >= 0);
    assert!(config.status <= 6);
    report.observe_u64(u64::from(config.status));
    report.observe_u64(u64::from(config.max_positions));

    for token_index in [TokenIndex::STABLE, TokenIndex::XLM, TokenIndex::WETH] {
        let token = &fixture.tokens[token_index];
        let reserve = pool.get_reserve(&token.address);
        assert!(reserve.data.b_rate >= 0);
        assert!(reserve.data.d_rate >= 0);
        assert!(reserve.data.ir_mod >= 0);
        assert!(reserve.data.b_supply >= 0);
        assert!(reserve.data.d_supply >= 0);
        assert!(reserve.data.backstop_credit >= 0);
        assert!(token.balance(&pool.address) >= 0);
        report.observe_i128(reserve.data.b_supply);
        report.observe_i128(reserve.data.d_supply);
        report.observe_i128(reserve.data.backstop_credit);
    }

    for user in &fixture.users {
        let positions = pool.get_positions(user);
        for (_, balance) in positions.collateral.iter() {
            assert!(balance > 0);
        }
        for (_, balance) in positions.liabilities.iter() {
            assert!(balance > 0);
        }
        for (_, balance) in positions.supply.iter() {
            assert!(balance > 0);
        }
        report.observe_u64(u64::from(positions.effective_count()));
    }
}
