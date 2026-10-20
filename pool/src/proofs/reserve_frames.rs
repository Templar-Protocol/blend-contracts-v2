use super::*;

/// Every field of `ReserveData` except `b_supply`, so a frame condition is one
/// comparison rather than six assertions repeated per harness.
fn rest_of(data: &ReserveData) -> (i128, i128, i128, i128, i128, u64) {
    (
        data.d_rate,
        data.b_rate,
        data.ir_mod,
        data.d_supply,
        data.backstop_credit,
        data.last_time,
    )
}

/// `mint_supply` adds exactly its argument and moves nothing else.
///
/// `add_supply` and `add_collateral` reach it only through an `Env`, a Soroban
/// `Map` and `update_b_emissions`, none of which are model-checkable, so what
/// is proved is the mutator, not the handlers that call it. They contribute the
/// `amount > 0` check, which `mint_supply` does not make.
#[kani::proof]
fn prove_mint_supply_frame() {
    let b_rate = hundredths(kani::any::<u8>());
    let b_supply = kani::any::<u8>() as i128;
    let amount = kani::any::<u8>() as i128;
    let mut data = sym_data(SCALAR_12, b_rate, b_supply, 0);
    let frame = rest_of(&data);

    data.mint_supply(amount);

    assert_eq!(data.b_supply, b_supply + amount);
    assert_eq!(rest_of(&data), frame);
}

/// `burn_supply` subtracts exactly its argument and moves nothing else.
///
/// No assumption bounds `amount` by `b_supply`, because the method makes no
/// such check: `remove_collateral` and `remove_supply` validate the user's
/// position, not the reserve's total. Keeping the domain open states that.
#[kani::proof]
fn prove_burn_supply_frame() {
    let b_rate = hundredths(kani::any::<u8>());
    let b_supply = kani::any::<u8>() as i128;
    let amount = kani::any::<u8>() as i128;
    let mut data = sym_data(SCALAR_12, b_rate, b_supply, 0);
    let frame = rest_of(&data);

    data.burn_supply(amount);

    assert_eq!(data.b_supply, b_supply - amount);
    assert_eq!(rest_of(&data), frame);
}

/// Accrual moves `b_rate` and `backstop_credit` and nothing else.
///
/// The supply-cap argument rests on this: accrual raises
/// `to_asset_from_b_token(b_supply)` by recomputing the rate, not by issuing
/// shares, which is why it is excluded from the cap guard rather than counted
/// as a writer of `b_supply`. Until now that was a reading of `accrue`.
///
/// `b_supply > 0` is assumed because `accrue` divides by it to recompute
/// `b_rate` without checking first.
#[kani::proof]
fn prove_accrue_moves_only_rate_and_credit() {
    let b_rate = hundredths(kani::any::<u8>());
    let b_supply = kani::any::<u8>() as i128;
    let d_supply = kani::any::<u8>() as i128;
    let bstop_rate = kani::any::<u8>() as u32;
    let accrued = kani::any::<u8>() as i128;
    kani::assume(b_rate > 0 && b_supply > 0);

    let mut data = sym_data(SCALAR_12, b_rate, b_supply, d_supply);
    let (d_rate_0, ir_mod_0, last_time_0) = (data.d_rate, data.ir_mod, data.last_time);

    data.accrue(&(), bstop_rate, accrued);

    assert_eq!(data.b_supply, b_supply);
    assert_eq!(data.d_supply, d_supply);
    assert_eq!(data.d_rate, d_rate_0);
    assert_eq!(data.ir_mod, ir_mod_0);
    assert_eq!(data.last_time, last_time_0);
}

/// Debt growth moves `d_rate` and nothing else, so it cannot disturb the
/// supply side of the reserve that the cap guard reads.
#[kani::proof]
fn prove_grow_debt_moves_only_d_rate() {
    let d_rate = hundredths(kani::any::<u8>());
    let b_rate = hundredths(kani::any::<u8>());
    let b_supply = kani::any::<u8>() as i128;
    let d_supply = kani::any::<u8>() as i128;
    let loan_accrual = hundredths(kani::any::<u8>());
    kani::assume(d_rate > 0 && b_rate > 0);

    let mut data = sym_data(d_rate, b_rate, b_supply, d_supply);
    let (ir_mod_0, credit_0, last_time_0) = (data.ir_mod, data.backstop_credit, data.last_time);

    data.grow_debt(&(), loan_accrual);

    assert_eq!(data.b_supply, b_supply);
    assert_eq!(data.b_rate, b_rate);
    assert_eq!(data.d_supply, d_supply);
    assert_eq!(data.ir_mod, ir_mod_0);
    assert_eq!(data.backstop_credit, credit_0);
    assert_eq!(data.last_time, last_time_0);
}
