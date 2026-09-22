use super::*;

fn arbitrary_config() -> ReserveConfig {
    ReserveConfig {
        index: kani::any(),
        decimals: kani::any(),
        c_factor: kani::any(),
        l_factor: kani::any(),
        util: kani::any(),
        max_util: kani::any(),
        r_base: kani::any(),
        r_one: kani::any(),
        r_two: kani::any(),
        r_three: kani::any(),
        reactivity: kani::any(),
        supply_cap: kani::any(),
        enabled: kani::any(),
    }
}

#[kani::proof]
fn prove_disable_only_transition() {
    let current = arbitrary_config();
    let candidate = arbitrary_config();
    let permitted = current.enabled
        && !candidate.enabled
        && (
            current.index,
            current.decimals,
            current.c_factor,
            current.l_factor,
            current.util,
            current.max_util,
            current.r_base,
            current.r_one,
            current.r_two,
            current.r_three,
            current.reactivity,
            current.supply_cap,
        ) == (
            candidate.index,
            candidate.decimals,
            candidate.c_factor,
            candidate.l_factor,
            candidate.util,
            candidate.max_util,
            candidate.r_base,
            candidate.r_one,
            candidate.r_two,
            candidate.r_three,
            candidate.reactivity,
            candidate.supply_cap,
        );
    assert_eq!(is_disable_only(&current, &candidate), permitted);

    // Construct a permitted transition without assuming the predicate under proof.
    let mut enabled = current.clone();
    enabled.enabled = true;
    let mut disabled = enabled.clone();
    disabled.enabled = false;
    assert!(is_disable_only(&enabled, &disabled));
    assert!(!is_disable_only(&disabled, &enabled));
}
