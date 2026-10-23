//! Unproved bounded arithmetic prerequisites: both Kani runs timed out at 600s.
//! Real conversions/repayment planning are exercised, not token settlement or
//! handler/global-position induction. These are excluded from acceptance.
use super::*;

#[kani::proof]
fn prove_borrow_aggregate_debt_delta() {
    let debt = kani::any::<u8>() as i128;
    let amount = kani::any::<u8>() as i128;
    let rate = kani::any::<u8>();
    kani::assume(rate >= 100);
    let data = sym_data(hundredths(rate), SCALAR_12);
    let before = data.to_asset_from_d_token(&(), debt);
    let minted = data.to_d_token_up(&(), amount);
    let after = data.to_asset_from_d_token(&(), debt + minted);
    assert!(after - before >= amount);
    assert!(after >= before);
    kani::cover!(amount == 0);
    kani::cover!(amount > 0 && rate == 100);
    kani::cover!(amount > 0 && rate > 100);
}

#[kani::proof]
fn prove_repay_aggregate_delta_and_refund() {
    let debt = kani::any::<u8>() as i128;
    let user_debt = kani::any::<u8>() as i128;
    kani::assume(user_debt > 0 && user_debt <= debt);
    let amount = kani::any::<u8>() as i128;
    let rate = kani::any::<u8>();
    kani::assume(rate >= 100);
    let data = sym_data(hundredths(rate), SCALAR_12);
    let (net_in, burnt, refund, capped) = plan_repay(&data, &(), amount, user_debt);
    let before = data.to_asset_from_d_token(&(), debt);
    let after = data.to_asset_from_d_token(&(), debt - burnt);
    assert!(0 <= burnt && burnt <= user_debt);
    assert!(0 <= before - after && before - after <= net_in);
    assert!(refund >= 0);
    assert_eq!(net_in, amount - refund);
    kani::cover!(capped && burnt > 0 && refund > 0);
    kani::cover!(!capped && burnt > 0);
    kani::cover!(
        debt == 2
            && user_debt == 1
            && amount == 3
            && rate == 150
            && net_in == 2
            && refund == 1
            && before == 3
            && after == 2
    );
}
