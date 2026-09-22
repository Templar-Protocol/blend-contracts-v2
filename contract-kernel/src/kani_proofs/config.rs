use crate::config::valid_pool_config;

/// Scope: proves the production-used pool/factory config predicate over full-width
/// u32/u32/i128 inputs. There are no assumptions. The assertion is the declarative
/// three-bound contract; unwind 2 is sufficient because this scalar policy has no loops.
/// Covers witness every requested boundary plus accepted and rejected configurations.
#[kani::proof]
#[kani::solver(kissat)]
#[kani::unwind(2)]
fn pool_config_bounds() {
    let take_rate: u32 = kani::any();
    let max_positions: u32 = kani::any();
    let min_collateral: i128 = kani::any();

    let expected =
        take_rate < 10_000_000 && (2..=60).contains(&max_positions) && min_collateral >= 0;
    let actual = valid_pool_config(take_rate, max_positions, min_collateral);
    assert_eq!(actual, expected);

    kani::cover!(
        take_rate == 9_999_999 && max_positions == 2 && min_collateral == 0,
        "rate_9_999_999"
    );
    kani::cover!(
        take_rate == 10_000_000 && max_positions == 2 && min_collateral == 0,
        "rate_10_000_000"
    );
    kani::cover!(
        take_rate == 0 && max_positions == 2 && min_collateral == 0,
        "positions_2"
    );
    kani::cover!(
        take_rate == 0 && max_positions == 60 && min_collateral == 0,
        "positions_60"
    );
    kani::cover!(
        take_rate == 0 && max_positions == 61 && min_collateral == 0,
        "positions_61"
    );
    kani::cover!(
        take_rate == 0 && max_positions == 2 && min_collateral == -1,
        "collateral_minus_1"
    );
    kani::cover!(
        take_rate == 0 && max_positions == 2 && min_collateral == 0,
        "collateral_zero"
    );
    kani::cover!(actual, "accepted");
    kani::cover!(!actual, "rejected");
}
