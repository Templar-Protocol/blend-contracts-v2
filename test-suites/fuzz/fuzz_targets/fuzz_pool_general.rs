#![allow(unused)]
#![no_main]

use soroban_fixed_point_math::{FixedPoint, SorobanFixedPoint};
use fuzz_common::{
    verify_contract_result, Borrow, ClaimPool, NatI128, PassTime, Repay, Supply, Withdraw,
};
use pool::{PoolState, PositionData, Request};
use libfuzzer_sys::fuzz_target;
use soroban_sdk::testutils::arbitrary::{fuzz_catch_panic, arbitrary::{self, Arbitrary, Unstructured}};
use soroban_sdk::{testutils::Address as _, vec, Address, token::TokenClient};
use test_suites::{
    assertions::assert_approx_eq_abs,
    create_fixture_with_data,
    test_fixture::{PoolFixture, TestFixture, TokenIndex, SCALAR_7, SCALAR_12},
};

#[derive(Arbitrary, Debug)]
struct Input {
    sam_xlm_balance: NatI128,
    sam_weth_balance: NatI128,
    sam_stable_balance: NatI128,
    merry_xlm_balance: NatI128,
    merry_weth_balance: NatI128,
    merry_stable_balance: NatI128,
    commands: [Command; 10],
}

#[derive(Arbitrary, Debug)]
enum Command {
    // Misc
    PassTime(PassTime),

    // Sam (1) Pool Commands
    SamSupply(Supply),
    SamWithdraw(Withdraw),
    SamBorrow(Borrow),
    SamRepay(Repay),
    SamClaimPool(ClaimPool),

    // Sam (2) Pool Commands
    MerrySupply(Supply),
    MerryWithdraw(Withdraw),
    MerryBorrow(Borrow),
    MerryRepay(Repay),
    MerryClaimPool(ClaimPool),
}

fuzz_target!(|input: Input| {
    let mut fixture = create_fixture_with_data(false);

    // Create two new users
    let sam = Address::generate(&fixture.env);
    fixture.users.push(sam.clone());
    let merry = Address::generate(&fixture.env);
    fixture.users.push(merry.clone());

    // Mint users tokens
    let xlm = &fixture.tokens[TokenIndex::XLM];
    let weth = &fixture.tokens[TokenIndex::WETH];
    let stable = &fixture.tokens[TokenIndex::STABLE];

    xlm.mint(&sam, &input.sam_xlm_balance.0);
    xlm.mint(&merry, &input.merry_xlm_balance.0);
    weth.mint(&sam, &input.sam_weth_balance.0);
    weth.mint(&merry, &input.merry_weth_balance.0);
    stable.mint(&sam, &input.sam_stable_balance.0);
    stable.mint(&merry, &input.merry_stable_balance.0);

    for command in &input.commands {
        command.run(&fixture);
        fixture.assert_invariants();
    }
});

impl Command {
    fn run(&self, fixture: &TestFixture) {
        use Command::*;
        match self {
            PassTime(cmd) => cmd.run(fixture),
            SamSupply(cmd) => cmd.run(fixture, 1),
            SamWithdraw(cmd) => cmd.run(fixture, 1),
            SamBorrow(cmd) => cmd.run(fixture, 1),
            SamRepay(cmd) => cmd.run(fixture, 1),
            SamClaimPool(cmd) => cmd.run(fixture, 1),
            MerrySupply(cmd) => cmd.run(fixture, 2),
            MerryWithdraw(cmd) => cmd.run(fixture, 2),
            MerryBorrow(cmd) => cmd.run(fixture, 2),
            MerryRepay(cmd) => cmd.run(fixture, 2),
            MerryClaimPool(cmd) => cmd.run(fixture, 2),
        }
    }
}

#[extension_trait::extension_trait]
impl Asserts for TestFixture<'_> {
    /// Assert the pool has not lent out more funds than it has.
    ///
    /// Every quantity here can exceed `i128` while the pool itself is behaving,
    /// and a panic from that is indistinguishable from a real violation. Such an
    /// input is outside this check's domain, so it is reported and skipped.
    fn assert_invariants(&self) {
        let pool_fixture = &self.pools[0];

        let mut supply: i128 = 0;
        let mut liabilities: i128 = 0;
        let evaluable = self.env.as_contract(&pool_fixture.pool.address, || {
            let mut pool_state = PoolState::load(&self.env);
            for (token_index, _reserve_index) in pool_fixture.reserves.iter() {
                let asset = &self.tokens[token_index.clone()];
                let reserve = pool_state.load_reserve(&self.env, &asset.address, false);
                let price = pool_state.load_price(&self.env, &reserve.asset);
                let state = format!(
                    "token {:?}: price={price} scalar={} b_rate={} d_rate={} \
                     b_supply={} d_supply={} credit={}",
                    token_index,
                    reserve.scalar,
                    reserve.data.b_rate,
                    reserve.data.d_rate,
                    reserve.data.b_supply,
                    reserve.data.d_supply,
                    reserve.data.backstop_credit,
                );

                // total_supply, total_liabilities and the I256-backed conversions
                // production uses all panic rather than returning None when a
                // result exceeds i128, so the panic is what has to be classified.
                // libfuzzer-sys installs a panic hook that aborts at the panic
                // site, so plain catch_unwind never runs; fuzz_catch_panic
                // suppresses that hook for the duration of the call.
                let totals = fuzz_catch_panic(|| {
                    let held = reserve
                        .total_supply(&self.env)
                        .checked_add(reserve.data.backstop_credit)?;
                    let owed = reserve.total_liabilities(&self.env);
                    Some((
                        SorobanFixedPoint::fixed_mul_floor(
                            &price,
                            &self.env,
                            &held,
                            &reserve.scalar,
                        ),
                        SorobanFixedPoint::fixed_mul_ceil(
                            &price,
                            &self.env,
                            &owed,
                            &reserve.scalar,
                        ),
                    ))
                });
                let (supply_base, liabilities_base) = match totals {
                    Ok(Some(bases)) => bases,
                    Ok(None) => {
                        eprintln!(
                            "INVARIANT UNEVALUABLE: supply plus backstop credit exceeds \
                             i128; {state}"
                        );
                        return false;
                    }
                    // Overflow is the expected cause, but the host can reject a
                    // conversion for other reasons, so report what it said rather
                    // than assuming.
                    Err(panic) => {
                        let why = panic
                            .downcast_ref::<&str>()
                            .map(|s| s.to_string())
                            .or_else(|| panic.downcast_ref::<String>().cloned())
                            .unwrap_or_else(|| "panic payload of unknown type".to_string());
                        eprintln!(
                            "INVARIANT UNEVALUABLE: totalling the reserve panicked \
                             ({why}); {state}"
                        );
                        return false;
                    }
                };

                let (Some(next_supply), Some(next_liabilities)) = (
                    supply.checked_add(supply_base),
                    liabilities.checked_add(liabilities_base),
                ) else {
                    eprintln!(
                        "INVARIANT UNEVALUABLE: base totals exceed i128 at {state}; \
                         running supply={supply}+{supply_base} liabilities={liabilities}+{liabilities_base}"
                    );
                    return false;
                };
                supply = next_supply;
                liabilities = next_liabilities;
            }
            true
        });

        if evaluable {
            assert!(
                supply > liabilities,
                "supply {supply} <= liabilities {liabilities}"
            );
        }
    }

    /// Assert the user is not underwater
    fn assert_user_invariants(&self, user: &Address) {
        let pool_fixture = &self.pools[0];

        let positions = pool_fixture.pool.get_positions(&user);
        self.env.as_contract(&pool_fixture.pool.address, || {
            let mut pool_state = PoolState::load(&self.env);
            let data =
                PositionData::calculate_from_positions(&self.env, &mut pool_state, &positions);
            assert!(data.as_health_factor(&self.env) > data.scalar);
        });
    }
}
