use super::*;

/// Block timing modifiers stay in [0, SCALAR_7]: the lot never decreases
/// and the bid never increases as blocks elapse, with the exact phase and
/// maturity boundaries at 0/199/200/201/399/400 blocks and at u32::MAX.
///
/// Elapsed counts cover the full u32 ledger-sequence difference production
/// can reach; `earlier <= later` chronology is a theorem input, not proved.
#[kani::proof]
fn prove_auction_timing_modifiers() {
    let earlier: u32 = kani::any();
    let later: u32 = kani::any();
    kani::assume(earlier <= later);
    let (bid_early, lot_early) = auction_timing_modifiers(i128::from(earlier));
    let (bid_late, lot_late) = auction_timing_modifiers(i128::from(later));

    assert!(bid_early >= 0 && bid_early <= SCALAR_7);
    assert!(lot_early >= 0 && lot_early <= SCALAR_7);
    assert!(bid_late >= 0 && bid_late <= SCALAR_7);
    assert!(lot_late >= 0 && lot_late <= SCALAR_7);

    assert!(lot_late >= lot_early);
    assert!(bid_late <= bid_early);

    // phase and maturity boundaries
    assert_eq!(auction_timing_modifiers(0), (SCALAR_7, 0));
    assert_eq!(auction_timing_modifiers(199), (SCALAR_7, 9_950_000));
    assert_eq!(auction_timing_modifiers(200), (SCALAR_7, SCALAR_7));
    assert_eq!(auction_timing_modifiers(201), (9_950_000, SCALAR_7));
    assert_eq!(auction_timing_modifiers(399), (50_000, SCALAR_7));
    assert_eq!(auction_timing_modifiers(400), (0, SCALAR_7));
    assert_eq!(
        auction_timing_modifiers(i128::from(u32::MAX)),
        (0, SCALAR_7)
    );
}

/// Undiscounted quote partition: fill plus remainder equals the original
/// amount in both rounding directions, with the exact scaled rounding
/// inequalities and the 100% identity. The kernel's own bid/lot direction
/// selection is exercised; the block-discounted amounts are separate
/// quantities proved in `prove_auction_discount_bounds`.
///
/// Domain (deliberate solver restriction, NOT a protocol limit): amounts
/// 0..=255 (u8) and fill percent 1..=100 as enforced by scale_auction.
/// Largest product 255 * 10_000_000 fits i128, so the dependency fast
/// path is exercised.
#[kani::proof]
fn prove_auction_quote_partition() {
    let amount_u8: u8 = kani::any();
    let percent_u8: u8 = kani::any();
    kani::assume(percent_u8 >= 1 && percent_u8 <= 100);
    let amount = i128::from(amount_u8);
    let percent_scalar = i128::from(percent_u8) * 1_00000; // same scaling as scale_auction

    let (bid_fill, bid_remain) = partition_quote(&(), true, amount, percent_scalar);
    let (lot_fill, lot_remain) = partition_quote(&(), false, amount, percent_scalar);

    assert_eq!(bid_fill + bid_remain, amount);
    assert_eq!(lot_fill + lot_remain, amount);
    assert!(bid_fill >= 0 && bid_fill <= amount);
    assert!(lot_fill >= 0 && lot_fill <= amount);
    assert!(lot_fill <= bid_fill);

    assert!(lot_fill * SCALAR_7 <= amount * percent_scalar);
    assert!(amount * percent_scalar - lot_fill * SCALAR_7 < SCALAR_7);
    assert!(bid_fill * SCALAR_7 >= amount * percent_scalar);
    assert!(bid_fill * SCALAR_7 - amount * percent_scalar < SCALAR_7);

    if percent_u8 == 100 {
        assert_eq!(bid_fill, amount);
        assert_eq!(lot_fill, amount);
    }

    // non-vacuity witnesses: 1 unit at 1% fills as a bid (rounds up) but
    // not as a lot (rounds down)
    assert_eq!(partition_quote(&(), true, 1, 1_00000), (1, 0));
    assert_eq!(partition_quote(&(), false, 1, 1_00000).0, 0);
}

/// Time-discounted settlement bounds, separate from the partition above:
/// the discounted amount never exceeds the undiscounted fill and never
/// goes negative, in both rounding directions, with the modifier
/// boundary identities (100% modifier is exact, 0% modifier fills nothing).
///
/// Domain (deliberate solver restriction, NOT a protocol limit): amounts
/// 0..=255 (u8), fill percent 1..=100, block modifier in [0, SCALAR_7].
#[kani::proof]
fn prove_auction_discount_bounds() {
    let amount_u8: u8 = kani::any();
    let percent_u8: u8 = kani::any();
    let modifier: i128 = kani::any();
    kani::assume(percent_u8 >= 1 && percent_u8 <= 100);
    kani::assume(modifier >= 0 && modifier <= SCALAR_7);
    let amount = i128::from(amount_u8);
    let percent_scalar = i128::from(percent_u8) * 1_00000;

    // compose the actual production kernels: partition first, then discount
    let (bid_fill, _) = partition_quote(&(), true, amount, percent_scalar);
    let (lot_fill, _) = partition_quote(&(), false, amount, percent_scalar);
    let bid_scaled = discount_quote(&(), true, bid_fill, modifier);
    let lot_scaled = discount_quote(&(), false, lot_fill, modifier);

    assert!(bid_scaled >= 0 && bid_scaled <= bid_fill);
    assert!(lot_scaled >= 0 && lot_scaled <= lot_fill);

    if modifier == SCALAR_7 {
        assert_eq!(bid_scaled, bid_fill);
        assert_eq!(lot_scaled, lot_fill);
    }
    if modifier == 0 {
        assert_eq!(bid_scaled, 0);
        assert_eq!(lot_scaled, 0);
    }
}
