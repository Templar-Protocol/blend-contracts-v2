use crate::{
    backstop::{
        above_threshold, cap_backfill, consume_queue_entry, threshold_values, withdraw_queue_entry,
        QueueStep, MAX_BACKFILLED_EMISSIONS,
    },
    pool::backstop_threshold,
};

const S7: i128 = 10_000_000;
const THRESHOLD_PRODUCT: u128 = 10_000_000_000_000_000_000_000_000;

fn bounded_balance() -> (u8, i128, i128) {
    let whole_seed: u8 = kani::any();
    let arbitrary_remainder: u8 = kani::any();
    let remainder_kind: u8 = kani::any();
    kani::assume(remainder_kind <= 2);
    let remainder = match remainder_kind {
        0 => arbitrary_remainder as i128,
        1 => S7 / 2,
        _ => S7 - 1,
    };
    (
        whole_seed,
        whole_seed as i128 * 1_000 * S7 + remainder,
        remainder,
    )
}

fn ordinary_product(blnd: i128, usdc: i128) -> u128 {
    let whole_blnd = (blnd / S7) as u128;
    let whole_usdc = (usdc / S7) as u128;
    whole_blnd * whole_blnd * whole_blnd * whole_blnd * whole_usdc
}

fn ordinary_threshold(blnd: i128, usdc: i128) -> i128 {
    ((ordinary_product(blnd, usdc) * S7 as u128) / THRESHOLD_PRODUCT) as i128
}

#[derive(Clone, Copy)]
struct ThresholdCase {
    blnd_seed: u8,
    blnd_remainder: i128,
    usdc_remainder: i128,
    pool_result: i128,
    backstop_result: bool,
}

fn threshold_case(blnd_min: u8, blnd_max: u8) -> ThresholdCase {
    assert!(blnd_min <= blnd_max);
    let (blnd_seed, blnd, blnd_remainder) = bounded_balance();
    let (usdc_seed, usdc, usdc_remainder) = bounded_balance();
    kani::assume(blnd_seed >= blnd_min && blnd_seed <= blnd_max);

    let seed = blnd_seed as u64;
    let seed_product = seed * seed * seed * seed * usdc_seed as u64;
    let (pool_result, backstop_result) = threshold_values(blnd, usdc);
    assert!(pool_result == (seed_product / 1_000) as i128);
    assert!(backstop_result == (seed_product >= 10_000_000_000));
    assert!((pool_result >= S7) == backstop_result);

    ThresholdCase {
        blnd_seed,
        blnd_remainder,
        usdc_remainder,
        pool_result,
        backstop_result,
    }
}

fn cover_threshold_partition(case: ThresholdCase, blnd_min: u8, blnd_max: u8) {
    kani::cover!(case.blnd_seed == blnd_min, "blnd_partition_min");
    kani::cover!(case.blnd_seed == blnd_max, "blnd_partition_max");
    kani::cover!(
        case.pool_result < S7 && !case.backstop_result,
        "below_threshold"
    );
    kani::cover!(
        case.blnd_remainder == 0 || case.usdc_remainder == 0,
        "remainder_0"
    );
    kani::cover!(
        case.blnd_remainder == 1 || case.usdc_remainder == 1,
        "remainder_1"
    );
    kani::cover!(
        case.blnd_remainder == S7 / 2 || case.usdc_remainder == S7 / 2,
        "remainder_half"
    );
    kani::cover!(
        case.blnd_remainder == S7 - 1 || case.usdc_remainder == S7 - 1,
        "remainder_max"
    );
}

/// Scope: BLND whole seed 0..=31; USDC spans u8 and both use the documented
/// remainder cases. The sole ordered-interval assumption is explicit in
/// `threshold_case`; a u64 seed-product oracle checks both production symbols.
/// Unwind 2 suffices because there are no loops. This partition is always below.
#[kani::proof]
#[kani::solver(kissat)]
#[kani::unwind(2)]
fn threshold_agreement_blnd_0_31() {
    let case = threshold_case(0, 31);
    cover_threshold_partition(case, 0, 31);
}

/// Scope and oracle are identical to `threshold_agreement_blnd_0_31`, with BLND
/// whole seed 32..=63. Unwind 2 suffices; this partition is always below.
#[kani::proof]
#[kani::solver(kissat)]
#[kani::unwind(2)]
fn threshold_agreement_blnd_32_63() {
    let case = threshold_case(32, 63);
    cover_threshold_partition(case, 32, 63);
}

/// Scope and oracle are identical to `threshold_agreement_blnd_0_31`, with BLND
/// whole seed 64..=95. Unwind 2 suffices; covers witness both threshold sides.
#[kani::proof]
#[kani::solver(kissat)]
#[kani::unwind(2)]
fn threshold_agreement_blnd_64_95() {
    let case = threshold_case(64, 95);
    cover_threshold_partition(case, 64, 95);
    kani::cover!(
        case.pool_result > S7 && case.backstop_result,
        "above_threshold"
    );
}

/// Scope and oracle are identical to `threshold_agreement_blnd_0_31`, with BLND
/// whole seed 96..=127. Unwind 2 suffices; covers include exact equality.
#[kani::proof]
#[kani::solver(kissat)]
#[kani::unwind(2)]
fn threshold_agreement_blnd_96_127() {
    let case = threshold_case(96, 127);
    cover_threshold_partition(case, 96, 127);
    kani::cover!(
        case.pool_result == S7 && case.backstop_result,
        "exact_threshold"
    );
    kani::cover!(
        case.pool_result > S7 && case.backstop_result,
        "above_threshold"
    );
}

/// Scope and oracle are identical to `threshold_agreement_blnd_0_31`, with BLND
/// whole seed 128..=159. Unwind 2 suffices; covers witness both threshold sides.
#[kani::proof]
#[kani::solver(kissat)]
#[kani::unwind(2)]
fn threshold_agreement_blnd_128_159() {
    let case = threshold_case(128, 159);
    cover_threshold_partition(case, 128, 159);
    kani::cover!(
        case.pool_result > S7 && case.backstop_result,
        "above_threshold"
    );
}

/// Scope and oracle are identical to `threshold_agreement_blnd_0_31`, with BLND
/// whole seed 160..=191. Unwind 2 suffices; covers witness both threshold sides.
#[kani::proof]
#[kani::solver(kissat)]
#[kani::unwind(2)]
fn threshold_agreement_blnd_160_191() {
    let case = threshold_case(160, 191);
    cover_threshold_partition(case, 160, 191);
    kani::cover!(
        case.pool_result > S7 && case.backstop_result,
        "above_threshold"
    );
}

/// Scope and oracle are identical to `threshold_agreement_blnd_0_31`, with BLND
/// whole seed 192..=223. Unwind 2 suffices; covers witness both threshold sides.
#[kani::proof]
#[kani::solver(kissat)]
#[kani::unwind(2)]
fn threshold_agreement_blnd_192_223() {
    let case = threshold_case(192, 223);
    cover_threshold_partition(case, 192, 223);
    kani::cover!(
        case.pool_result > S7 && case.backstop_result,
        "above_threshold"
    );
}

/// Scope and oracle are identical to `threshold_agreement_blnd_0_31`, with BLND
/// whole seed 224..=255. Together the eight explicit partitions cover the original
/// u8 axis without gaps or overlap. Unwind 2 suffices; both sides are covered.
#[kani::proof]
#[kani::solver(kissat)]
#[kani::unwind(2)]
fn threshold_agreement_blnd_224_255() {
    let case = threshold_case(224, 255);
    cover_threshold_partition(case, 224, 255);
    kani::cover!(
        case.pool_result > S7 && case.backstop_result,
        "above_threshold"
    );
}

fn assert_threshold_reference(blnd: i128, usdc: i128) {
    assert!(backstop_threshold(blnd, usdc) == ordinary_threshold(blnd, usdc));
    assert!(above_threshold(blnd, usdc) == (ordinary_product(blnd, usdc) >= THRESHOLD_PRODUCT));
}

#[derive(Clone, Copy)]
struct MonotonicCase {
    blnd_seed: u8,
    increase_whole: u16,
    before_threshold: i128,
    before_met: bool,
}

fn monotonic_inputs(blnd_min: u8, blnd_max: u8) -> (u8, i128, i128, u16, i128) {
    assert!(blnd_min <= blnd_max);
    let (blnd_seed, blnd, _) = bounded_balance();
    let (_, usdc, _) = bounded_balance();
    kani::assume(blnd_seed >= blnd_min && blnd_seed <= blnd_max);
    let increase_whole: u16 = kani::any();
    kani::assume(increase_whole <= 1_000);
    let increase = increase_whole as i128 * S7;
    (blnd_seed, blnd, usdc, increase_whole, increase)
}

fn assert_threshold_monotonicity(
    blnd_seed: u8,
    increase_whole: u16,
    before_blnd: i128,
    before_usdc: i128,
    after_blnd: i128,
    after_usdc: i128,
) -> MonotonicCase {
    assert!(before_blnd >= 0 && before_usdc >= 0);
    assert!(after_blnd >= before_blnd && after_usdc >= before_usdc);

    let before_threshold = backstop_threshold(before_blnd, before_usdc);
    let after_threshold = backstop_threshold(after_blnd, after_usdc);
    let before_met = above_threshold(before_blnd, before_usdc);
    let after_met = above_threshold(after_blnd, after_usdc);

    assert!(before_threshold >= 0);
    assert!(after_threshold >= 0);
    assert!(after_threshold >= before_threshold);
    assert!(!before_met || after_met);
    assert_eq!(before_met, before_threshold >= S7);
    assert_eq!(after_met, after_threshold >= S7);

    MonotonicCase {
        blnd_seed,
        increase_whole,
        before_threshold,
        before_met,
    }
}

fn cover_monotonic_partition(case: MonotonicCase, blnd_min: u8, blnd_max: u8) {
    kani::cover!(case.blnd_seed == blnd_min, "blnd_partition_min");
    kani::cover!(case.blnd_seed == blnd_max, "blnd_partition_max");
    kani::cover!(case.increase_whole == 0, "increase_zero");
    kani::cover!(case.increase_whole == 1_000, "increase_1000");
    kani::cover!(
        case.before_threshold < S7 && !case.before_met,
        "starts_below_threshold"
    );
}

macro_rules! monotonic_partition {
    ($name:ident, blnd, $min:literal, $max:literal, $threshold_side:ident) => {
        #[doc = concat!(
            "Scope: direct production-wrapper monotonicity with BLND seed ",
            stringify!($min),
            "..=",
            stringify!($max),
            ", increasing BLND by 0..=1000 whole units. USDC spans u8 and both ",
            "balances use the documented remainder cases. No loops; unwind 2."
        )]
        #[kani::proof]
        #[kani::solver(kissat)]
        #[kani::unwind(2)]
        fn $name() {
            let (blnd_seed, blnd, usdc, increase_whole, increase) =
                monotonic_inputs($min, $max);
            let case = assert_threshold_monotonicity(
                blnd_seed,
                increase_whole,
                blnd,
                usdc,
                blnd + increase,
                usdc,
            );
            cover_monotonic_partition(case, $min, $max);
            monotonic_partition!(@threshold_cover case, $threshold_side);
        }
    };
    ($name:ident, usdc, $min:literal, $max:literal, $threshold_side:ident) => {
        #[doc = concat!(
            "Scope: direct production-wrapper monotonicity with BLND seed ",
            stringify!($min),
            "..=",
            stringify!($max),
            ", increasing USDC by 0..=1000 whole units. USDC spans u8 and both ",
            "balances use the documented remainder cases. No loops; unwind 2."
        )]
        #[kani::proof]
        #[kani::solver(kissat)]
        #[kani::unwind(2)]
        fn $name() {
            let (blnd_seed, blnd, usdc, increase_whole, increase) =
                monotonic_inputs($min, $max);
            let case = assert_threshold_monotonicity(
                blnd_seed,
                increase_whole,
                blnd,
                usdc,
                blnd,
                usdc + increase,
            );
            cover_monotonic_partition(case, $min, $max);
            monotonic_partition!(@threshold_cover case, $threshold_side);
        }
    };
    (@threshold_cover $case:ident, below_only) => {};
    (@threshold_cover $case:ident, both_sides) => {
        kani::cover!(
            $case.before_threshold >= S7 && $case.before_met,
            "starts_at_or_above_threshold"
        );
    };
}

monotonic_partition!(threshold_monotonicity_blnd_0_31, blnd, 0, 31, below_only);
monotonic_partition!(threshold_monotonicity_blnd_32_47, blnd, 32, 47, below_only);
monotonic_partition!(threshold_monotonicity_blnd_48_63, blnd, 48, 63, below_only);
monotonic_partition!(threshold_monotonicity_blnd_64_71, blnd, 64, 71, below_only);
monotonic_partition!(threshold_monotonicity_blnd_72_79, blnd, 72, 79, below_only);
monotonic_partition!(threshold_monotonicity_blnd_80_87, blnd, 80, 87, both_sides);
monotonic_partition!(threshold_monotonicity_blnd_88_91, blnd, 88, 91, both_sides);
monotonic_partition!(threshold_monotonicity_blnd_92_95, blnd, 92, 95, both_sides);
monotonic_partition!(
    threshold_monotonicity_blnd_96_103,
    blnd,
    96,
    103,
    both_sides
);
monotonic_partition!(
    threshold_monotonicity_blnd_104_111,
    blnd,
    104,
    111,
    both_sides
);
monotonic_partition!(
    threshold_monotonicity_blnd_112_119,
    blnd,
    112,
    119,
    both_sides
);
monotonic_partition!(
    threshold_monotonicity_blnd_120_127,
    blnd,
    120,
    127,
    both_sides
);
monotonic_partition!(
    threshold_monotonicity_blnd_128_135,
    blnd,
    128,
    135,
    both_sides
);
monotonic_partition!(
    threshold_monotonicity_blnd_136_143,
    blnd,
    136,
    143,
    both_sides
);
monotonic_partition!(
    threshold_monotonicity_blnd_144_147,
    blnd,
    144,
    147,
    both_sides
);
monotonic_partition!(
    threshold_monotonicity_blnd_148_151,
    blnd,
    148,
    151,
    both_sides
);
monotonic_partition!(
    threshold_monotonicity_blnd_152_159,
    blnd,
    152,
    159,
    both_sides
);
monotonic_partition!(
    threshold_monotonicity_blnd_160_167,
    blnd,
    160,
    167,
    both_sides
);
monotonic_partition!(
    threshold_monotonicity_blnd_168_175,
    blnd,
    168,
    175,
    both_sides
);
monotonic_partition!(
    threshold_monotonicity_blnd_176_179,
    blnd,
    176,
    179,
    both_sides
);
monotonic_partition!(
    threshold_monotonicity_blnd_180_183,
    blnd,
    180,
    183,
    both_sides
);
monotonic_partition!(
    threshold_monotonicity_blnd_184_191,
    blnd,
    184,
    191,
    both_sides
);
monotonic_partition!(
    threshold_monotonicity_blnd_192_195,
    blnd,
    192,
    195,
    both_sides
);
monotonic_partition!(
    threshold_monotonicity_blnd_196_199,
    blnd,
    196,
    199,
    both_sides
);
monotonic_partition!(
    threshold_monotonicity_blnd_200_203,
    blnd,
    200,
    203,
    both_sides
);
monotonic_partition!(
    threshold_monotonicity_blnd_204_207,
    blnd,
    204,
    207,
    both_sides
);
monotonic_partition!(
    threshold_monotonicity_blnd_208_223,
    blnd,
    208,
    223,
    both_sides
);
monotonic_partition!(
    threshold_monotonicity_blnd_224_255,
    blnd,
    224,
    255,
    both_sides
);
monotonic_partition!(threshold_monotonicity_usdc_0_31, usdc, 0, 31, below_only);
monotonic_partition!(threshold_monotonicity_usdc_32_63, usdc, 32, 63, below_only);
monotonic_partition!(threshold_monotonicity_usdc_64_95, usdc, 64, 95, both_sides);
monotonic_partition!(
    threshold_monotonicity_usdc_96_103,
    usdc,
    96,
    103,
    both_sides
);
monotonic_partition!(
    threshold_monotonicity_usdc_104_111,
    usdc,
    104,
    111,
    both_sides
);
monotonic_partition!(
    threshold_monotonicity_usdc_112_127,
    usdc,
    112,
    127,
    both_sides
);
monotonic_partition!(
    threshold_monotonicity_usdc_128_159,
    usdc,
    128,
    159,
    both_sides
);
monotonic_partition!(
    threshold_monotonicity_usdc_160_191,
    usdc,
    160,
    191,
    both_sides
);
monotonic_partition!(
    threshold_monotonicity_usdc_192_223,
    usdc,
    192,
    223,
    both_sides
);
monotonic_partition!(
    threshold_monotonicity_usdc_224_255,
    usdc,
    224,
    255,
    both_sides
);

/// Scope: deterministic signed, extreme, exact-threshold and ±1 raw-unit
/// regression points for both production wrappers. No assumptions or loops;
/// unwind 2 suffices. A u128 oracle is used only at concrete nonnegative points.
#[kani::proof]
#[kani::solver(kissat)]
#[kani::unwind(2)]
fn threshold_boundary_points() {
    assert!(backstop_threshold(0, i128::MAX) == 0);
    assert!(!above_threshold(0, i128::MAX));
    assert!(backstop_threshold(i128::MAX, 0) == 0);
    assert!(!above_threshold(i128::MAX, 0));
    assert!(backstop_threshold(i128::MAX, i128::MAX) == i128::MAX / THRESHOLD_PRODUCT as i128);
    assert!(above_threshold(i128::MAX, i128::MAX));
    assert_eq!(backstop_threshold(-2 * S7, 3 * S7), 0);
    assert!(!above_threshold(-2 * S7, 3 * S7));
    assert_eq!(backstop_threshold(2 * S7, -3 * S7), 0);
    assert!(!above_threshold(2 * S7, -3 * S7));

    let exact_blnd = 200_000 * S7;
    let exact_usdc = 6_250 * S7;
    assert_threshold_reference(exact_blnd, exact_usdc);
    assert!(backstop_threshold(exact_blnd, exact_usdc) == S7);
    assert!(above_threshold(exact_blnd, exact_usdc));

    assert_threshold_reference(exact_blnd - 1, exact_usdc);
    assert_threshold_reference(exact_blnd + 1, exact_usdc);
    assert_threshold_reference(exact_blnd, exact_usdc - 1);
    assert_threshold_reference(exact_blnd, exact_usdc + 1);
    assert!(!above_threshold(exact_blnd - 1, exact_usdc));
    assert!(!above_threshold(exact_blnd, exact_usdc - 1));
    assert!(
        backstop_threshold(exact_blnd + 1, exact_usdc)
            == backstop_threshold(exact_blnd, exact_usdc)
    );
    assert!(
        backstop_threshold(exact_blnd, exact_usdc + 1)
            == backstop_threshold(exact_blnd, exact_usdc)
    );
    assert!(
        backstop_threshold(exact_blnd + S7 - 1, exact_usdc)
            == backstop_threshold(exact_blnd, exact_usdc)
    );
    assert!(
        backstop_threshold(exact_blnd, exact_usdc + S7 - 1)
            == backstop_threshold(exact_blnd, exact_usdc)
    );

    kani::cover!(backstop_threshold(0, i128::MAX) == 0, "zero_axis_extreme");
    kani::cover!(above_threshold(i128::MAX, i128::MAX), "saturated_extreme");
    kani::cover!(
        backstop_threshold(exact_blnd, exact_usdc) == S7,
        "exact_threshold"
    );
    kani::cover!(
        !above_threshold(exact_blnd - 1, exact_usdc)
            && !above_threshold(exact_blnd, exact_usdc - 1),
        "subunit_neighbors"
    );
}

fn assert_queue_conservation(entry: i128, requested: i128, step: QueueStep) {
    let (consumed, entry_remaining, requested_remaining) = match step {
        QueueStep::Partial { entry_remaining } => (requested, entry_remaining, 0),
        QueueStep::Exact => (entry, 0, 0),
        QueueStep::Continue {
            requested_remaining,
        } => (entry, 0, requested_remaining),
    };
    let minimum = if entry < requested { entry } else { requested };
    assert!(consumed >= 0);
    assert!(entry_remaining >= 0);
    assert!(requested_remaining >= 0);
    assert_eq!(consumed, minimum);
    assert_eq!(consumed + entry_remaining, entry);
    assert_eq!(consumed + requested_remaining, requested);
}

/// Scope: proves production `consume_queue_entry` over full-width nonnegative i128
/// entry/request amounts. Nonnegativity is the contract API precondition; no magnitude
/// bound is imposed. Assertions cover exact comparison variants and conservation;
/// unwind 2 is sufficient without loops. Covers witness zero, partial, exact and continue.
#[kani::proof]
#[kani::solver(kissat)]
#[kani::unwind(2)]
fn queue_conservation() {
    let entry: i128 = kani::any();
    let requested: i128 = kani::any();
    kani::assume(entry >= 0 && requested >= 0);

    let step = consume_queue_entry(entry, requested);
    match step {
        QueueStep::Partial { entry_remaining } => {
            assert!(entry > requested);
            assert_eq!(entry_remaining, entry - requested);
        }
        QueueStep::Exact => assert_eq!(entry, requested),
        QueueStep::Continue {
            requested_remaining,
        } => {
            assert!(entry < requested);
            assert_eq!(requested_remaining, requested - entry);
        }
    }
    assert_queue_conservation(entry, requested, step);

    kani::cover!(
        entry == 0 && requested == 0 && step == QueueStep::Exact,
        "zero_exact"
    );
    kani::cover!(
        entry > requested && matches!(step, QueueStep::Partial { .. }),
        "partial"
    );
    kani::cover!(
        entry == requested && entry > 0 && step == QueueStep::Exact,
        "nonzero_exact"
    );
    kani::cover!(
        entry < requested && matches!(step, QueueStep::Continue { .. }),
        "continue"
    );
}

/// Scope: proves production `withdraw_queue_entry` for full-width u64 times and
/// full-width nonnegative i128 amounts. Amount nonnegativity is the API precondition;
/// maturity itself is unbounded. Success reuses conservation, rejection is exactly
/// `expires > now`, and unwind 2 suffices. Covers include u64::MAX equality and variants.
#[kani::proof]
#[kani::solver(kissat)]
#[kani::unwind(2)]
fn withdrawal_maturity() {
    let entry: i128 = kani::any();
    let requested: i128 = kani::any();
    let expires: u64 = kani::any();
    let now: u64 = kani::any();
    kani::assume(entry >= 0 && requested >= 0);

    let result = withdraw_queue_entry(entry, expires, now, requested);
    let rejected = matches!(result, Err(_));
    let accepted = matches!(result, Ok(_));
    assert_eq!(rejected, expires > now);
    if let Ok(step) = result {
        assert_eq!(step, consume_queue_entry(entry, requested));
        assert_queue_conservation(entry, requested, step);
    }

    kani::cover!(expires > now && rejected, "not_expired");
    kani::cover!(expires == now && accepted, "exact_maturity");
    kani::cover!(
        expires == u64::MAX && now == u64::MAX && accepted,
        "max_exact_maturity"
    );
    kani::cover!(expires < now && accepted, "past_maturity");
    kani::cover!(
        expires <= now && entry > requested && matches!(result, Ok(QueueStep::Partial { .. })),
        "mature_partial"
    );
    kani::cover!(
        expires <= now && entry == requested && matches!(result, Ok(QueueStep::Exact)),
        "mature_exact"
    );
    kani::cover!(
        expires <= now && entry < requested && matches!(result, Ok(QueueStep::Continue { .. })),
        "mature_continue"
    );
}

/// Scope: proves production `cap_backfill` for current 0..=limit+1 and elapsed-
/// emission requests `u64 * S7`. The ordered interval is asserted before its stated
/// domain assumption. The independent min oracle proves the cap and total; unwind 2
/// suffices without loops. Covers witness no room, exact fill, capping, zero and reject.
#[kani::proof]
#[kani::solver(kissat)]
#[kani::unwind(2)]
fn backfill_limit() {
    let current: i128 = kani::any();
    const CURRENT_MIN: i128 = 0;
    const CURRENT_MAX: i128 = MAX_BACKFILLED_EMISSIONS + 1;
    assert!(CURRENT_MIN <= CURRENT_MAX);
    kani::assume(current >= CURRENT_MIN && current <= CURRENT_MAX);
    let elapsed: u64 = kani::any();
    let requested = elapsed as i128 * S7;

    let actual = cap_backfill(current, requested);
    if current >= MAX_BACKFILLED_EMISSIONS {
        assert_eq!(actual, None);
    } else {
        let room = MAX_BACKFILLED_EMISSIONS - current;
        let allocated = if requested < room { requested } else { room };
        assert_eq!(actual, Some((allocated, current + allocated)));
        if let Some((_, new_total)) = actual {
            assert!(new_total <= MAX_BACKFILLED_EMISSIONS);
        }
    }

    kani::cover!(
        current == MAX_BACKFILLED_EMISSIONS && matches!(actual, None),
        "no_room"
    );
    kani::cover!(
        current == MAX_BACKFILLED_EMISSIONS + 1 && matches!(actual, None),
        "rejected_current"
    );
    kani::cover!(
        current < MAX_BACKFILLED_EMISSIONS
            && requested == MAX_BACKFILLED_EMISSIONS - current
            && actual == Some((requested, MAX_BACKFILLED_EMISSIONS)),
        "exact_fill"
    );
    kani::cover!(
        current < MAX_BACKFILLED_EMISSIONS
            && requested > MAX_BACKFILLED_EMISSIONS - current
            && matches!(actual, Some((_, MAX_BACKFILLED_EMISSIONS))),
        "partial_capped_fill"
    );
    kani::cover!(
        current < MAX_BACKFILLED_EMISSIONS && requested == 0 && actual == Some((0, current)),
        "zero_request"
    );
}
