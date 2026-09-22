use crate::pool::{
    action_allowed, admin_status as production_admin_status, auction_modifiers, next_status,
    reserve_action_allowed, StatusError,
};

fn automatic_reference(
    current: u32,
    q4w_pct: i128,
    met_threshold: bool,
) -> Result<u32, StatusError> {
    match current {
        4 | 6 => Err(StatusError::StatusNotAllowed),
        2 if q4w_pct >= 7_500_000 => Ok(5),
        2 => Ok(2),
        0 if !met_threshold || q4w_pct >= 5_000_000 => Ok(3),
        0 => Ok(0),
        _ if q4w_pct >= 6_000_000 => Ok(5),
        _ if q4w_pct >= 3_000_000 || !met_threshold => Ok(3),
        _ => Ok(1),
    }
}

/// Scope: proves production `next_status` over full-width u32/i128/bool inputs.
/// There are no assumptions. A separate ordered match is the truth-table oracle;
/// unwind 2 is sufficient because there are no loops. Covers include both forbidden
/// states, default states, and equality at all automatic/admin thresholds.
#[kani::proof]
#[kani::solver(kissat)]
#[kani::unwind(2)]
fn automatic_status() {
    let current: u32 = kani::any();
    let q4w_pct: i128 = kani::any();
    let met_threshold: bool = kani::any();

    let actual = next_status(current, q4w_pct, met_threshold);
    assert_eq!(actual, automatic_reference(current, q4w_pct, met_threshold));

    kani::cover!(
        current == 4 && actual == Err(StatusError::StatusNotAllowed),
        "status_4_rejected"
    );
    kani::cover!(
        current == 6 && actual == Err(StatusError::StatusNotAllowed),
        "status_6_rejected"
    );
    kani::cover!(
        current == 2 && q4w_pct == 7_500_000 && actual == Ok(5),
        "q4w_75_equality"
    );
    kani::cover!(
        current == 0 && q4w_pct == 5_000_000 && actual == Ok(3),
        "q4w_50_equality"
    );
    kani::cover!(
        current == 1 && q4w_pct == 6_000_000 && actual == Ok(5),
        "q4w_60_equality"
    );
    kani::cover!(
        current == 1 && q4w_pct == 3_000_000 && actual == Ok(3),
        "q4w_30_equality"
    );
    kani::cover!(
        current == u32::MAX && q4w_pct < 3_000_000 && met_threshold && actual == Ok(1),
        "default_current"
    );
    kani::cover!(
        !met_threshold && current == 0 && q4w_pct < 5_000_000 && actual == Ok(3),
        "threshold_not_met"
    );
}

fn admin_reference(requested: u32, q4w_pct: i128, met_threshold: bool) -> Result<u32, StatusError> {
    match requested {
        0 if met_threshold && q4w_pct < 5_000_000 => Ok(0),
        0 => Err(StatusError::StatusNotAllowed),
        2 | 3 if q4w_pct < 7_500_000 => Ok(requested),
        2 | 3 => Err(StatusError::StatusNotAllowed),
        4 => Ok(4),
        _ => Err(StatusError::BadRequest),
    }
}

/// Scope: proves production `admin_status` over full-width u32/i128/bool inputs.
/// There are no assumptions. The oracle is a separate ordered table; unwind 2 is
/// sufficient for this loop-free policy. Covers witness status 3, invalid requests,
/// unconditional freeze, and strict rejection at both equality boundaries.
#[kani::proof]
#[kani::solver(kissat)]
#[kani::unwind(2)]
fn admin_status() {
    let requested: u32 = kani::any();
    let q4w_pct: i128 = kani::any();
    let met_threshold: bool = kani::any();

    let actual = production_admin_status(requested, q4w_pct, met_threshold);
    assert_eq!(actual, admin_reference(requested, q4w_pct, met_threshold));

    kani::cover!(
        requested == 0 && met_threshold && q4w_pct < 5_000_000 && actual == Ok(0),
        "active_allowed"
    );
    kani::cover!(
        requested == 0 && q4w_pct == 5_000_000 && actual == Err(StatusError::StatusNotAllowed),
        "active_equality_rejected"
    );
    kani::cover!(
        requested == 2 && q4w_pct == 7_500_000 && actual == Err(StatusError::StatusNotAllowed),
        "admin_ice_equality_rejected"
    );
    kani::cover!(
        requested == 3 && q4w_pct < 7_500_000 && actual == Ok(3),
        "status_3_allowed"
    );
    kani::cover!(requested == 4 && actual == Ok(4), "freeze_allowed");
    kani::cover!(
        requested == u32::MAX && actual == Err(StatusError::BadRequest),
        "invalid_request"
    );
}

/// Scope: proves both production action predicates over full-width status/tag and
/// bool inputs. There are no assumptions. Explicit tag matches are the independent
/// forbidden-set oracle; unwind 2 is sufficient without loops. Covers witness every
/// forbidden family, enabled bypass, and retained behavior for an invalid tag.
#[kani::proof]
#[kani::solver(kissat)]
#[kani::unwind(2)]
fn action_permissions() {
    let status: u32 = kani::any();
    let request_type: u32 = kani::any();
    let enabled: bool = kani::any();

    let expected_pool = match request_type {
        4 | 9 => status <= 1,
        0 | 2 => status <= 3,
        _ => true,
    };
    let expected_reserve = match request_type {
        0 | 2 | 4 => enabled,
        _ => true,
    };

    assert_eq!(action_allowed(status, request_type), expected_pool);
    assert_eq!(
        reserve_action_allowed(enabled, request_type),
        expected_reserve
    );

    kani::cover!(
        status == 2 && request_type == 4 && !expected_pool,
        "pool_borrow_forbidden"
    );
    kani::cover!(
        status == 2 && request_type == 9 && !expected_pool,
        "pool_cancel_forbidden"
    );
    kani::cover!(
        status == 4 && request_type == 0 && !expected_pool,
        "pool_supply_forbidden"
    );
    kani::cover!(
        status == 4 && request_type == 2 && !expected_pool,
        "pool_collateral_forbidden"
    );
    kani::cover!(
        !enabled && request_type == 0 && !expected_reserve,
        "reserve_supply_forbidden"
    );
    kani::cover!(
        !enabled && request_type == 2 && !expected_reserve,
        "reserve_collateral_forbidden"
    );
    kani::cover!(
        !enabled && request_type == 4 && !expected_reserve,
        "reserve_borrow_forbidden"
    );
    kani::cover!(
        enabled && request_type == 4 && expected_reserve,
        "enabled_reserve"
    );
    kani::cover!(
        request_type == u32::MAX && expected_pool && expected_reserve,
        "invalid_tag_predicate_behavior"
    );
}

fn schedule_reference(elapsed: u32) -> (i128, i128) {
    match elapsed {
        0..=200 => (10_000_000, elapsed as i128 * 50_000),
        201..=399 => (10_000_000 - (elapsed as i128 - 200) * 50_000, 10_000_000),
        _ => (0, 10_000_000),
    }
}

/// Scope: proves production `auction_modifiers` for every u32 elapsed block.
/// There are no assumptions. A declarative piecewise schedule is the oracle; unwind
/// 2 is sufficient without loops. Covers witness 0, both sides of 200 and 400, and
/// both endpoints; adjacency is checked whenever `elapsed + 1` is representable.
#[kani::proof]
#[kani::solver(kissat)]
#[kani::unwind(2)]
fn auction_schedule() {
    let elapsed: u32 = kani::any();
    let actual = auction_modifiers(elapsed);
    assert_eq!(actual, schedule_reference(elapsed));
    assert!((0..=10_000_000).contains(&actual.0));
    assert!((0..=10_000_000).contains(&actual.1));

    if elapsed < u32::MAX {
        let next = auction_modifiers(elapsed + 1);
        assert!(next.0 <= actual.0);
        assert!(next.1 >= actual.1);
    }

    assert_eq!(auction_modifiers(0), (10_000_000, 0));
    assert_eq!(auction_modifiers(200), (10_000_000, 10_000_000));
    assert_eq!(auction_modifiers(400), (0, 10_000_000));

    kani::cover!(elapsed == 0, "elapsed_0");
    kani::cover!(elapsed == 199, "elapsed_199");
    kani::cover!(elapsed == 200, "elapsed_200");
    kani::cover!(elapsed == 201, "elapsed_201");
    kani::cover!(elapsed == 399, "elapsed_399");
    kani::cover!(elapsed == 400, "elapsed_400");
    kani::cover!(elapsed == 401, "elapsed_401");
}
