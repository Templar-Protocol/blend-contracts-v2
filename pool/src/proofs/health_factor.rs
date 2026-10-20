use super::*;

/// `collateral_raw` and `liability_raw` are carried alongside the effective
/// figures and read by nothing the health factor computes.
fn position(collateral_base: i128, liability_base: i128, scalar: i128) -> PositionData {
    PositionData {
        collateral_base,
        collateral_raw: collateral_base,
        liability_base,
        liability_raw: liability_base,
        scalar,
    }
}

/// Effective collateral cannot lower the health factor.
///
/// Half of the monotonicity the audit list names. The quotient floors, so the
/// claim is `<=` rather than `<`: two collateral figures a stroop apart can
/// share a health factor.
#[kani::proof]
fn prove_hf_monotonic_in_collateral() {
    let lower = kani::any::<u8>() as i128;
    let higher = kani::any::<u8>() as i128;
    let liability = kani::any::<u8>() as i128;
    kani::assume(liability > 0); // the ratio divides by it
    kani::assume(lower <= higher);

    let before = position(lower, liability, SCALAR_7);
    let after = position(higher, liability, SCALAR_7);

    assert!(before.as_health_factor(&()) <= after.as_health_factor(&()));
}

/// Effective liability cannot raise the health factor.
#[kani::proof]
fn prove_hf_antitone_in_liability() {
    let collateral = kani::any::<u8>() as i128;
    let lower = kani::any::<u8>() as i128;
    let higher = kani::any::<u8>() as i128;
    kani::assume(lower > 0 && lower <= higher);

    let before = position(collateral, lower, SCALAR_7);
    let after = position(collateral, higher, SCALAR_7);

    assert!(before.as_health_factor(&()) >= after.as_health_factor(&()));
}

/// A position with no liability is over every maximum and under no minimum.
///
/// Both guards short-circuit on `liability_base == 0` before dividing, which is
/// what keeps the ratio from dividing by zero on an unlevered position.
#[kani::proof]
fn prove_zero_liability_is_healthy() {
    let collateral = kani::any::<u8>() as i128;
    let bound = kani::any::<u8>() as i128;
    let unlevered = position(collateral, 0, SCALAR_7);

    assert!(unlevered.is_hf_over(&(), bound));
    assert!(!unlevered.is_hf_under(&(), bound));
}

/// The two guards are not complements: at one bound there are positions that
/// are neither over it nor under it.
///
/// `is_hf_over` compares `>` against a `ceil` threshold and `is_hf_under`
/// compares `<` against a `floor` one, so the band is not an artefact of the
/// bound's rounding. Liquidation reads both, at 1.15 and 1.03, and a position
/// inside the band answers no to each.
#[kani::proof]
fn prove_guards_leave_a_band() {
    let collateral = kani::any::<u8>() as i128;
    let liability = kani::any::<u8>() as i128;
    let bound = kani::any::<u8>() as i128 * (SCALAR_7 / 100);
    kani::assume(liability > 0);

    let data = position(collateral, liability, SCALAR_7);
    let over = data.is_hf_over(&(), bound);
    let under = data.is_hf_under(&(), bound);

    assert!(!(over && under)); // never both
    kani::cover!(!over && !under); // but reachably neither
}
