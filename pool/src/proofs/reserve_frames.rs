use super::*;

/// Adding a field to `ReserveData` fails to compile here rather than dropping
/// silently out of the frames below. Indices follow declaration order.
fn fields(data: &ReserveData) -> (i128, i128, i128, i128, i128, i128, u64) {
    let ReserveData {
        d_rate,
        b_rate,
        ir_mod,
        b_supply,
        d_supply,
        backstop_credit,
        last_time,
    } = data;
    (
        *d_rate,
        *b_rate,
        *ir_mod,
        *b_supply,
        *d_supply,
        *backstop_credit,
        *last_time,
    )
}

/// `sym_data` pins `ir_mod`, `backstop_credit` and `last_time` to zero, which
/// would prove the frames only at those constants.
fn any_data() -> ReserveData {
    ReserveData {
        d_rate: hundredths(kani::any::<u8>()),
        b_rate: hundredths(kani::any::<u8>()),
        ir_mod: kani::any::<u8>() as i128,
        b_supply: kani::any::<u8>() as i128,
        d_supply: kani::any::<u8>() as i128,
        backstop_credit: kani::any::<u8>() as i128,
        last_time: kani::any::<u8>() as u64,
    }
}

/// `mint_supply` adds exactly its argument to `b_supply` and moves no other
/// field.
///
/// `add_supply` and `add_collateral` reach it only through an `Env`, a Soroban
/// `Map` and `update_b_emissions`, which Kani cannot drive, so this proves the
/// mutator. They contribute the `amount > 0` check, which `mint_supply` omits.
/// The Certora rules `add_supply_increases_b_supply` and
/// `add_collateral_increases_b_supply` cover the handlers.
#[kani::proof]
fn prove_mint_supply_frame() {
    let mut data = any_data();
    let amount = kani::any::<u8>() as i128;
    let before = fields(&data);

    data.mint_supply(amount);

    let mut expected = before;
    expected.3 += amount; // b_supply
    assert_eq!(fields(&data), expected);
}

/// `burn_supply` subtracts exactly its argument from `b_supply` and moves no
/// other field.
///
/// `amount` is not bounded by `b_supply`: the method checks nothing against the
/// reserve total, and `remove_collateral` and `remove_supply` validate the
/// user's position instead.
#[kani::proof]
fn prove_burn_supply_frame() {
    let mut data = any_data();
    let amount = kani::any::<u8>() as i128;
    let before = fields(&data);

    data.burn_supply(amount);

    let mut expected = before;
    expected.3 -= amount; // b_supply
    assert_eq!(fields(&data), expected);
}

/// Accrual moves `b_rate` and `backstop_credit` and no other field.
///
/// The supply cap rests on this: accrual raises
/// `to_asset_from_b_token(b_supply)` by recomputing the rate rather than
/// issuing shares, which is why it is excluded from the cap guard instead of
/// counted as a writer of `b_supply`.
///
/// `accrue` divides by `b_supply` to recompute `b_rate` without checking it is
/// non-zero.
#[kani::proof]
fn prove_accrue_moves_only_rate_and_credit() {
    let mut data = any_data();
    let accrued = kani::any::<u8>() as i128;
    let take_h = kani::any::<u8>();
    kani::assume(take_h < 100); // take rate is a percentage in SCALAR_7 points, < 100%
    let bstop_rate = (take_h as u32 * SCALAR_7 as u32) / 100; // < SCALAR_7, genuine zero included
    kani::assume(data.b_supply > 0);
    let before = fields(&data);

    data.accrue(&(), bstop_rate, accrued);

    let mut expected = before;
    expected.1 = data.b_rate; // recomputed from the accrued underlying
    expected.5 = data.backstop_credit; // the take rate's share of it
    assert_eq!(fields(&data), expected);
    // a u8 bstop_rate would floor every credit to zero, leaving the field the
    // frame names unexercised
    kani::cover!(data.backstop_credit > before.5);
    kani::cover!(data.b_rate != before.1);
}

/// Debt growth moves `d_rate` and no other field, so it cannot disturb the
/// supply side of the reserve that the cap guard reads.
#[kani::proof]
fn prove_grow_debt_moves_only_d_rate() {
    let mut data = any_data();
    let loan_accrual = hundredths(kani::any::<u8>());
    let before = fields(&data);

    data.grow_debt(&(), loan_accrual);

    let mut expected = before;
    expected.0 = data.d_rate; // grown by the loan accrual
    assert_eq!(fields(&data), expected);
}
