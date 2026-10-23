//! The mutation target and deterministic replay both execute this exact path.
use crate::valuation::{checked_conversion, checked_sum, NumericLimit, Rounding};
use crate::{Borrow, ClaimPool, NatI128, PassTime, PoolReserveToken, Repay, Supply, Withdraw};
use pool::PoolState;
use serde::{Deserialize, Serialize};
use soroban_fixed_point_math::SorobanFixedPoint;
use soroban_sdk::testutils::arbitrary::arbitrary::{self, Arbitrary};
use soroban_sdk::{testutils::Address as _, Address};
use test_suites::{
    create_fixture_with_data,
    test_fixture::{TestFixture, TokenIndex, SCALAR_12},
};

// Arbitrary field/variant order and the ten-command bound are corpus contracts.
#[derive(Arbitrary, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Input {
    pub sam_xlm_balance: NatI128,
    pub sam_weth_balance: NatI128,
    pub sam_stable_balance: NatI128,
    pub merry_xlm_balance: NatI128,
    pub merry_weth_balance: NatI128,
    pub merry_stable_balance: NatI128,
    pub commands: [Command; 10],
}

#[derive(Arbitrary, Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Command {
    PassTime(PassTime),
    SamSupply(Supply),
    SamWithdraw(Withdraw),
    SamBorrow(Borrow),
    SamRepay(Repay),
    SamClaimPool(ClaimPool),
    MerrySupply(Supply),
    MerryWithdraw(Withdraw),
    MerryBorrow(Borrow),
    MerryRepay(Repay),
    MerryClaimPool(ClaimPool),
}

#[derive(Debug, Serialize)]
pub struct StepReport {
    pub accepted: Option<bool>,
    pub positions_changed: bool,
    pub wallet_delta: Option<i128>,
    pub collateral_empty: Option<bool>,
    pub liabilities_empty: Option<bool>,
    pub invariant: InvariantReport,
}

#[derive(Debug, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum InvariantReport {
    Checked {
        supply: i128,
        liabilities: i128,
    },
    Unevaluable {
        reserve: String,
        limit: NumericLimit,
    },
}

pub fn run<const RECORD_STEPS: bool>(input: &Input) -> [StepReport; 10] {
    // Replay observes positions/wallets; mutation mode elides those host calls.
    let mut fixture = create_fixture_with_data(false);
    let sam = Address::generate(&fixture.env);
    fixture.users.push(sam.clone());
    let merry = Address::generate(&fixture.env);
    fixture.users.push(merry.clone());

    let xlm = &fixture.tokens[TokenIndex::XLM];
    let weth = &fixture.tokens[TokenIndex::WETH];
    let stable = &fixture.tokens[TokenIndex::STABLE];
    xlm.mint(&sam, &input.sam_xlm_balance.0);
    xlm.mint(&merry, &input.merry_xlm_balance.0);
    weth.mint(&sam, &input.sam_weth_balance.0);
    weth.mint(&merry, &input.merry_weth_balance.0);
    stable.mint(&sam, &input.sam_stable_balance.0);
    stable.mint(&merry, &input.merry_stable_balance.0);
    let tokens = reserve_tokens(&fixture);

    std::array::from_fn(|index| {
        let command = &input.commands[index];
        let user = if RECORD_STEPS {
            command.user_index().map(|index| &fixture.users[index])
        } else {
            None
        };
        let before = user.map(|user| fixture.pools[0].pool.get_positions(user));
        let token_user = command.token().zip(user);
        let balance_before =
            token_user.map(|(token, user)| fixture.tokens[token as usize].balance(user));
        let accepted = command.run(&fixture);
        let after = user.map(|user| fixture.pools[0].pool.get_positions(user));
        let balance_after =
            token_user.map(|(token, user)| fixture.tokens[token as usize].balance(user));
        let positions_changed = match (&before, &after) {
            (Some(before), Some(after)) => {
                before.collateral != after.collateral
                    || before.liabilities != after.liabilities
                    || before.supply != after.supply
            }
            _ => false,
        };
        StepReport {
            accepted,
            positions_changed,
            wallet_delta: balance_before
                .zip(balance_after)
                .map(|(before, after)| after - before),
            collateral_empty: after.as_ref().map(|p| p.collateral.is_empty()),
            liabilities_empty: after.as_ref().map(|p| p.liabilities.is_empty()),
            invariant: assert_invariants(&fixture, &tokens),
        }
    })
}

impl Command {
    fn user_index(&self) -> Option<usize> {
        use Command::*;
        match self {
            PassTime(_) => None,
            SamSupply(_) | SamWithdraw(_) | SamBorrow(_) | SamRepay(_) | SamClaimPool(_) => Some(1),
            MerrySupply(_) | MerryWithdraw(_) | MerryBorrow(_) | MerryRepay(_)
            | MerryClaimPool(_) => Some(2),
        }
    }

    fn token(&self) -> Option<PoolReserveToken> {
        use Command::*;
        match self {
            SamSupply(cmd) | MerrySupply(cmd) => Some(cmd.token),
            SamWithdraw(cmd) | MerryWithdraw(cmd) => Some(cmd.token),
            SamBorrow(cmd) | MerryBorrow(cmd) => Some(cmd.token),
            SamRepay(cmd) | MerryRepay(cmd) => Some(cmd.token),
            PassTime(_) | SamClaimPool(_) | MerryClaimPool(_) => None,
        }
    }

    fn run(&self, fixture: &TestFixture<'_>) -> Option<bool> {
        use Command::*;
        Some(match self {
            PassTime(cmd) => {
                cmd.run(fixture);
                return None;
            }
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
        })
    }
}

fn reserve_tokens(fixture: &TestFixture<'_>) -> Vec<TokenIndex> {
    let mut tokens: Vec<_> = fixture.pools[0].reserves.keys().copied().collect();
    tokens.sort_by_key(|token| *token as usize);
    tokens
}

/// Priced supplier claims plus credit must strictly exceed priced debt.
/// This is not a token-cash solvency assertion. Only an exact out-of-i128
/// intermediate/result can skip it; loading, host and logic failures propagate.
fn assert_invariants(fixture: &TestFixture<'_>, tokens: &[TokenIndex]) -> InvariantReport {
    let pool_fixture = &fixture.pools[0];
    fixture.env.as_contract(&pool_fixture.pool.address, || {
        let mut supply = 0;
        let mut liabilities = 0;
        let mut pool_state = PoolState::load(&fixture.env);
        for &token in tokens {
            let asset = &fixture.tokens[token];
            let reserve = pool_state.load_reserve(&fixture.env, &asset.address, false);
            let price = pool_state.load_price(&fixture.env, &reserve.asset);
            let totals = (|| -> Result<(i128, i128), NumericLimit> {
                let claims = checked_conversion(
                    "reserve.total_supply",
                    reserve.data.b_supply,
                    reserve.data.b_rate,
                    SCALAR_12,
                    Rounding::Floor,
                    || reserve.total_supply(&fixture.env),
                )?;
                let held = checked_sum("supply_plus_credit", claims, reserve.data.backstop_credit)?;
                let owed = checked_conversion(
                    "reserve.total_liabilities",
                    reserve.data.d_supply,
                    reserve.data.d_rate,
                    SCALAR_12,
                    Rounding::Ceil,
                    || reserve.total_liabilities(&fixture.env),
                )?;
                let supply_base = checked_conversion(
                    "priced_supply",
                    price,
                    held,
                    reserve.scalar,
                    Rounding::Floor,
                    || price.fixed_mul_floor(&fixture.env, &held, &reserve.scalar),
                )?;
                let liabilities_base = checked_conversion(
                    "priced_liabilities",
                    price,
                    owed,
                    reserve.scalar,
                    Rounding::Ceil,
                    || price.fixed_mul_ceil(&fixture.env, &owed, &reserve.scalar),
                )?;
                Ok((
                    checked_sum("aggregate_supply", supply, supply_base)?,
                    checked_sum("aggregate_liabilities", liabilities, liabilities_base)?,
                ))
            })();
            match totals {
                Ok((next_supply, next_liabilities)) => {
                    supply = next_supply;
                    liabilities = next_liabilities;
                }
                Err(limit) => {
                    eprintln!(
                        "INVARIANT UNEVALUABLE: {token:?}: {limit:?}; price={price} scalar={} \
                         b_rate={} d_rate={} b_supply={} d_supply={} credit={}",
                        reserve.scalar,
                        reserve.data.b_rate,
                        reserve.data.d_rate,
                        reserve.data.b_supply,
                        reserve.data.d_supply,
                        reserve.data.backstop_credit,
                    );
                    return InvariantReport::Unevaluable {
                        reserve: format!("{token:?}"),
                        limit,
                    };
                }
            }
        }
        assert!(
            supply > liabilities,
            "supply {supply} <= liabilities {liabilities}"
        );
        InvariantReport::Checked {
            supply,
            liabilities,
        }
    })
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RegressionSeed {
    pub schema_version: u32,
    pub description: String,
    pub input: Input,
    pub expected: [ExpectedStep; 10],
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExpectedStep {
    pub accepted: Option<bool>,
    pub positions_changed: bool,
    pub wallet_delta: Option<i128>,
    pub collateral_empty: Option<bool>,
    pub liabilities_empty: Option<bool>,
}

pub fn replay(seed: &RegressionSeed) -> Result<[StepReport; 10], String> {
    if seed.schema_version != 1 || seed.description.trim().is_empty() {
        return Err("expected schema_version=1 and a nonempty description".to_string());
    }
    for balance in [
        &seed.input.sam_xlm_balance,
        &seed.input.sam_weth_balance,
        &seed.input.sam_stable_balance,
        &seed.input.merry_xlm_balance,
        &seed.input.merry_weth_balance,
        &seed.input.merry_stable_balance,
    ] {
        if !(0..=i64::MAX as i128).contains(&balance.0) {
            return Err("seed balance is outside the existing Arbitrary input domain".to_string());
        }
    }
    let reports = run::<true>(&seed.input);
    for (index, (actual, expected)) in reports.iter().zip(&seed.expected).enumerate() {
        if actual.accepted != expected.accepted
            || actual.positions_changed != expected.positions_changed
            || expected
                .wallet_delta
                .is_some_and(|v| actual.wallet_delta != Some(v))
            || expected
                .collateral_empty
                .is_some_and(|v| actual.collateral_empty != Some(v))
            || expected
                .liabilities_empty
                .is_some_and(|v| actual.liabilities_empty != Some(v))
            || !matches!(&actual.invariant, InvariantReport::Checked { .. })
        {
            return Err(format!(
                "command {index} {:?}: expected {expected:?}, observed {actual:?}",
                seed.input.commands[index],
            ));
        }
    }
    if !reports
        .iter()
        .any(|report| report.accepted == Some(true) && report.positions_changed)
    {
        return Err("seed exercised no accepted financial position transition".to_string());
    }
    Ok(reports)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn committed_seeds_execute_real_financial_transitions() {
        for (name, data) in [
            (
                "sam_debt_cycle",
                include_str!("regression-seeds/sam_debt_cycle.json"),
            ),
            (
                "merry_debt_cycle",
                include_str!("regression-seeds/merry_debt_cycle.json"),
            ),
            (
                "rounding_and_rejections",
                include_str!("regression-seeds/rounding_and_rejections.json"),
            ),
        ] {
            let seed: RegressionSeed = serde_json::from_str(data).unwrap();
            replay(&seed).unwrap_or_else(|error| panic!("{name}: {error}"));
        }
    }

    #[test]
    fn real_out_of_range_reserve_is_reported_as_unevaluable() {
        let fixture = create_fixture_with_data(false);
        let mut data = fixture.read_reserve_data(0, TokenIndex::WETH);
        data.b_supply = i128::MAX;
        data.b_rate = i128::MAX;
        data.last_time = fixture.env.ledger().timestamp();
        fixture.env.as_contract(&fixture.pools[0].pool.address, || {
            fixture.env.storage().persistent().set(
                &pool::PoolDataKey::ResData(fixture.tokens[TokenIndex::WETH].address.clone()),
                &data,
            );
        });
        match assert_invariants(&fixture, &reserve_tokens(&fixture)) {
            InvariantReport::Unevaluable { reserve, limit } => {
                assert_eq!(reserve, "WETH");
                assert_eq!(limit.stage, "reserve.total_supply");
            }
            other => panic!("expected explicit numeric limit, got {other:?}"),
        }
    }

    #[test]
    fn invalid_oracle_quote_is_a_failure_not_a_skip() {
        let fixture = create_fixture_with_data(false);
        fixture.oracle.set_price_stable(&soroban_sdk::vec![
            &fixture.env,
            0,
            1_0000000,
            1_000_000,
            1_0000000
        ]);
        assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            assert_invariants(&fixture, &reserve_tokens(&fixture))
        }))
        .is_err());
    }
}
