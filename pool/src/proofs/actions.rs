use super::*;
use crate::constants::SCALAR_12;

/// Rate in exact hundredths of SCALAR_12; keeps every product inside i128.
fn hundredths(h: u8) -> i128 {
    (h as i128 * SCALAR_12) / 100
}

fn sym_data(d_rate: i128, b_rate: i128) -> ReserveData {
    ReserveData {
        d_rate,
        b_rate,
        ir_mod: 0,
        b_supply: 0,
        d_supply: 0,
        backstop_credit: 0,
        last_time: 0,
    }
}

#[kani::proof]
fn prove_withdraw_plan_caps() {
    let amount = kani::any::<u8>() as i128;
    let cur_b_tokens = kani::any::<u8>() as i128;
    let rate_h = kani::any::<u8>();
    kani::assume(rate_h >= 1); // inverse conversion divides by b_rate
    let data = sym_data(hundredths(1), hundredths(rate_h));
    let (tokens_out, to_burn) = plan_withdraw(&data, &(), amount, cur_b_tokens);
    // never burns more than the position
    assert!(to_burn <= cur_b_tokens);
    let requested_burn = data.to_b_token_up(&(), amount);
    if requested_burn > cur_b_tokens {
        // capped: burns the whole position and pays its computed claim
        assert_eq!(to_burn, cur_b_tokens);
        assert_eq!(tokens_out, data.to_asset_from_b_token(&(), cur_b_tokens));
    } else {
        // boundary strictness: an exactly-coverable request stays uncapped
        assert_eq!(to_burn, requested_burn);
        assert_eq!(tokens_out, amount);
    }
    // payout never exceeds the asset claim of the burnt shares
    assert!(tokens_out <= data.to_asset_from_b_token(&(), to_burn));
}

#[kani::proof]
fn prove_repay_plan_split() {
    let amount = kani::any::<u8>() as i128;
    let cur_d_tokens = kani::any::<u8>() as i128;
    let rate_h = kani::any::<u8>();
    kani::assume(rate_h >= 1); // inverse conversion divides by d_rate
    let data = sym_data(hundredths(rate_h), hundredths(1));
    let (tokens_in, d_tokens_burnt, refund, capped) = plan_repay(&data, &(), amount, cur_d_tokens);
    // never extinguishes more debt than exists
    assert!(d_tokens_burnt <= cur_d_tokens);
    let requested_burn = data.to_d_token_down(&(), amount);
    if requested_burn > cur_d_tokens {
        assert!(capped);
        let cur_underlying = data.to_asset_from_d_token(&(), cur_d_tokens);
        assert_eq!(d_tokens_burnt, cur_d_tokens);
        assert_eq!(tokens_in, cur_underlying);
        assert_eq!(refund, amount - cur_underlying);
        // on the panic-free domain the input decomposes exactly
        if refund >= 0 {
            assert_eq!(tokens_in + refund, amount);
        }
    } else {
        // exact equality must not take the refund branch: strictness of `>`
        assert!(!capped);
        assert_eq!(d_tokens_burnt, requested_burn);
        assert_eq!(tokens_in, amount);
        assert_eq!(refund, 0);
    }
}

#[kani::proof]
fn prove_supply_cap_strictness() {
    let total_supply = kani::any::<i128>();
    let supply_cap = kani::any::<i128>();
    assert_eq!(
        supply_within_cap(total_supply, supply_cap),
        total_supply <= supply_cap
    );
}

#[kani::proof]
fn independent_probe_repay_floor_total() {
    let amount = kani::any::<u8>() as i128;
    let rate_h = kani::any::<u8>();
    kani::assume(rate_h >= 1);
    let data = sym_data(hundredths(rate_h), hundredths(1));
    assert!(data.to_d_token_down(&(), amount) >= 0);
}

#[kani::proof]
fn independent_probe_repay_ceil_nonnegative() {
    let amount = kani::any::<u8>() as i128;
    let rate_h = kani::any::<u8>();
    kani::assume(rate_h >= 1);
    let data = sym_data(hundredths(rate_h), hundredths(1));
    assert!(data.to_asset_from_d_token(&(), amount) >= 0);
}

struct RepayMath {
    floor_args: (i128, i128, i128),
    ceil_args: (i128, i128, i128),
    floor_result: i128,
    ceil_result: i128,
}

impl FixedMath for RepayMath {
    fn floor(&self, x: i128, y: i128, denominator: i128) -> i128 {
        assert_eq!((x, y, denominator), self.floor_args);
        self.floor_result
    }

    fn ceil(&self, x: i128, y: i128, denominator: i128) -> i128 {
        assert_eq!((x, y, denominator), self.ceil_args);
        self.ceil_result
    }
}

#[kani::proof]
fn independent_probe_repay_parametric() {
    let amount = kani::any::<u8>() as i128;
    let cur_d_tokens = kani::any::<u8>() as i128;
    let rate_h = kani::any::<u8>();
    kani::assume(rate_h >= 1);
    let data = sym_data(hundredths(rate_h), hundredths(1));
    let math = RepayMath {
        floor_args: (amount, SCALAR_12, data.d_rate),
        ceil_args: (cur_d_tokens, data.d_rate, SCALAR_12),
        floor_result: kani::any(),
        ceil_result: kani::any(),
    };
    // Established separately for the real () implementation on all caller inputs.
    kani::assume(math.ceil_result >= 0);
    let (tokens_in, d_tokens_burnt, refund, capped) =
        plan_repay(&data, &math, amount, cur_d_tokens);
    assert!(d_tokens_burnt <= cur_d_tokens);
    let requested_burn = data.to_d_token_down(&math, amount);
    if requested_burn > cur_d_tokens {
        assert!(capped);
        let cur_underlying = data.to_asset_from_d_token(&math, cur_d_tokens);
        assert_eq!(d_tokens_burnt, cur_d_tokens);
        assert_eq!(tokens_in, cur_underlying);
        assert_eq!(refund, amount - cur_underlying);
        if refund >= 0 {
            assert_eq!(tokens_in + refund, amount);
        }
    } else {
        assert!(!capped);
        assert_eq!(d_tokens_burnt, requested_burn);
        assert_eq!(tokens_in, amount);
        assert_eq!(refund, 0);
    }
    kani::cover!(capped);
    kani::cover!(!capped);
    kani::cover!(math.floor_result == cur_d_tokens && !capped);
}

#[kani::proof]
fn independent_probe_withdraw_floor_total() {
    let amount = kani::any::<u8>() as i128;
    let rate_h = kani::any::<u8>();
    kani::assume(rate_h >= 1);
    let data = sym_data(hundredths(1), hundredths(rate_h));
    assert!(data.to_asset_from_b_token(&(), amount) >= 0);
}

#[kani::proof]
fn independent_probe_withdraw_up_total() {
    let amount = kani::any::<u8>() as i128;
    let rate_h = kani::any::<u8>();
    kani::assume(rate_h >= 1);
    let data = sym_data(hundredths(1), hundredths(rate_h));
    assert!(data.to_b_token_up(&(), amount) >= 0);
}

struct WithdrawMath {
    ceil_args: (i128, i128, i128),
    floor_args: (i128, i128, i128),
    ceil_result: i128,
    floor_result: i128,
}

impl FixedMath for WithdrawMath {
    fn floor(&self, x: i128, y: i128, denominator: i128) -> i128 {
        assert_eq!((x, y, denominator), self.floor_args);
        self.floor_result
    }

    fn ceil(&self, x: i128, y: i128, denominator: i128) -> i128 {
        assert_eq!((x, y, denominator), self.ceil_args);
        self.ceil_result
    }
}

#[kani::proof]
fn independent_probe_withdraw_parametric() {
    let amount = kani::any::<u8>() as i128;
    let cur_b_tokens = kani::any::<u8>() as i128;
    let rate_h = kani::any::<u8>();
    kani::assume(rate_h >= 1); // inverse conversion divides by b_rate
    let data = sym_data(hundredths(1), hundredths(rate_h));
    let q: i128 = kani::any(); // stable requested_burn
    let y: i128 = kani::any(); // stable asset claim
    let math = WithdrawMath {
        ceil_args: (amount, SCALAR_12, data.b_rate),
        floor_args: (q.min(cur_b_tokens), data.b_rate, SCALAR_12),
        ceil_result: q,
        floor_result: y,
    };
    // Established separately for the real () implementation:
    kani::assume(q >= 0 && y >= 0);
    // Source-linked round-trip theorem (prove_b_round_trip_up_conservative):
    // on the uncapped branch only, burning up never strands the request.
    kani::assume(q > cur_b_tokens || y >= amount);
    let (tokens_out, to_burn) = plan_withdraw(&data, &math, amount, cur_b_tokens);
    // never burns more than the position
    assert!(to_burn <= cur_b_tokens);
    let requested_burn = data.to_b_token_up(&math, amount);
    if requested_burn > cur_b_tokens {
        // capped: burns the whole position and pays its computed claim
        assert_eq!(to_burn, cur_b_tokens);
        assert_eq!(tokens_out, data.to_asset_from_b_token(&math, cur_b_tokens));
    } else {
        // boundary strictness: an exactly-coverable request stays uncapped
        assert_eq!(to_burn, requested_burn);
        assert_eq!(tokens_out, amount);
    }
    // payout never exceeds the asset claim of the burnt shares
    assert!(tokens_out <= data.to_asset_from_b_token(&math, to_burn));
    kani::cover!(requested_burn > cur_b_tokens);
    kani::cover!(requested_burn <= cur_b_tokens);
}

fn supply_data(b_rate: i128, b_supply: i128) -> ReserveData {
    ReserveData {
        d_rate: SCALAR_12,
        b_rate,
        ir_mod: 0,
        b_supply,
        d_supply: 0,
        backstop_credit: 0,
        last_time: 0,
    }
}

/// Drive the arithmetic `apply_supply` runs between loading the reserve and
/// testing the cap: mint at the floor rate, add to `b_supply`, re-read the
/// total. Returns `(total_before, total_after, minted)`.
///
/// `add_supply` adds exactly the minted amount to `b_supply` (`user.rs`), and
/// `apply_supply` passes exactly `to_b_token_down(amount)`, so the sequence
/// here matches the handler's. It is still a transcription: nothing makes this
/// follow the handler if the handler's order changes.
fn supply_chain(b_supply: i128, amount: i128, b_rate: i128) -> (i128, i128, i128) {
    let before = supply_data(b_rate, b_supply);
    let total_before = before.to_asset_from_b_token(&(), b_supply);
    let minted = before.to_b_token_down(&(), amount);
    let after = supply_data(b_rate, b_supply + minted);
    let total_after = after.to_asset_from_b_token(&(), b_supply + minted);
    (total_before, total_after, minted)
}

/// Supplying never lowers the reserve's accounted supply, at any rate.
///
/// This is the half the cap decision rests on: with it, a reserve already over
/// its cap stays over after any supply, so `prove_supply_cap_decides_on_post_state`
/// can reject it. The matching upper bound needs the rate fixed — see
/// `supply_credit_no_inflation`.
#[kani::proof]
fn prove_supply_credit_monotonic() {
    let rate_h = kani::any::<u8>();
    kani::assume(rate_h >= 1); // the inverse conversion divides by b_rate
    let b_supply = kani::any::<u8>() as i128;
    let amount = kani::any::<u8>() as i128;
    let (total_before, total_after, minted) = supply_chain(b_supply, amount, hundredths(rate_h));
    assert!(minted >= 0);
    assert!(total_after >= total_before);
}

/// Supplying `amount` credits at most `amount` of underlying, so the cap
/// cannot be overshot by conversion rounding.
///
/// `prove_b_round_trip_down_conservative` establishes this for one deposit into
/// an *empty* reserve. This is the same bound once the reserve already holds
/// supply, which is the state the cap check runs against; flooring is not
/// additive, so it does not follow from the single-deposit result.
///
/// `RATE_H` fixes `b_rate` at `RATE_H` hundredths of `SCALAR_12`. The bound
/// puts a quotient inside the third conversion's dividend, and with a symbolic
/// divisor there the solver does not finish — `prove_supply_credit_monotonic`
/// keeps the symbolic rate because it needs no such nesting.
fn supply_credit_no_inflation<const RATE_H: u8>() {
    let b_supply = kani::any::<u8>() as i128;
    let amount = kani::any::<u8>() as i128;
    let (total_before, total_after, _) = supply_chain(b_supply, amount, hundredths(RATE_H));
    assert!(total_after <= total_before + amount);
}

macro_rules! proof_case {
    ($name:ident, $helper:ident, $value:literal) => {
        #[kani::proof]
        fn $name() {
            $helper::<$value>();
        }
    };
}

// 1 hundredth is far below the initial SCALAR_12, the region a reserve enters
// after default_b_rate subtracts a loss; 100 is the initial rate exactly; 255
// is the top of the hundredths range.
proof_case!(
    prove_supply_credit_no_inflation_h001,
    supply_credit_no_inflation,
    1
);
proof_case!(
    prove_supply_credit_no_inflation_h100,
    supply_credit_no_inflation,
    100
);
proof_case!(
    prove_supply_credit_no_inflation_h255,
    supply_credit_no_inflation,
    255
);

/// The cap decision is taken on the post-supply total, so a reserve already
/// over its cap cannot be supplied into.
///
/// `prove_supply_cap_strictness` shows `supply_within_cap` equals `<=`; it says
/// nothing about which total the handler hands it. Driving the chain shows an
/// over-cap reserve stays rejected for every amount, rather than being admitted
/// on a stale pre-state reading.
#[kani::proof]
fn prove_supply_cap_decides_on_post_state() {
    let rate_h = kani::any::<u8>();
    kani::assume(rate_h >= 1);
    let b_supply = kani::any::<u8>() as i128;
    let amount = kani::any::<u8>() as i128;
    let supply_cap = kani::any::<u8>() as i128;
    let (total_before, total_after, _) = supply_chain(b_supply, amount, hundredths(rate_h));

    assert_eq!(
        supply_within_cap(total_after, supply_cap),
        total_after <= supply_cap
    );
    if total_before > supply_cap {
        assert!(!supply_within_cap(total_after, supply_cap));
    }
    kani::cover!(supply_within_cap(total_after, supply_cap));
    kani::cover!(!supply_within_cap(total_after, supply_cap));
}

/// `burn_supply` and the two add paths are exact inverses on `b_supply`.
///
/// The orphan-collateral path in `bad_debt.rs` and the liquidation fill in
/// `user_liquidation_auction.rs` both increase `b_supply` without a cap check,
/// and both are safe only because an equal decrease precedes them on the same
/// reserve. This pins that cancellation, so neither path can drift into a net
/// increase.
#[kani::proof]
fn prove_burn_then_add_is_supply_neutral() {
    let b_supply = kani::any::<u8>() as i128;
    let amount = kani::any::<u8>() as i128;
    kani::assume(amount > 0 && amount <= b_supply); // both paths move an existing position

    let mut data = supply_data(SCALAR_12, b_supply);
    data.burn_supply(amount);
    assert_eq!(data.b_supply, b_supply - amount);
    data.b_supply += amount; // what add_supply/add_collateral do
    assert_eq!(data.b_supply, b_supply);
}

/// The no-inflation bound and monotonicity over the representable range,
/// stated in product space so no multiplication appears.
///
/// `apply_supply` forms `b_supply * b_rate`, `minted * b_rate` and
/// `amount * SCALAR_12`. Taking those as the symbolic quantities, under the
/// only relations integer division guarantees between them, leaves division by
/// the constant `SCALAR_12` and comparisons.
///
/// Faithfulness is by Rust semantics rather than a further harness: `minted`
/// is `amount * SCALAR_12 / b_rate` and `/` on non-negative `i128` is floor
/// division, so `minted * b_rate <= amount * SCALAR_12 < (minted + 1) *
/// b_rate`, which is assumed below; and `(b_supply + minted) * b_rate` is
/// `b_supply * b_rate + minted * b_rate` by distributivity.
#[kani::proof]
fn prove_supply_credit_bound_product_space() {
    let p_before = kani::any::<i128>(); // b_supply * b_rate
    let q_minted = kani::any::<i128>(); // minted   * b_rate
    let p_amount = kani::any::<i128>(); // amount   * SCALAR_12
    let b_rate = kani::any::<i128>();

    kani::assume(b_rate > 0);
    kani::assume(p_before >= 0 && q_minted >= 0 && p_amount >= 0);
    kani::assume(p_before <= i128::MAX / 4);
    kani::assume(q_minted <= i128::MAX / 4);
    kani::assume(p_amount <= i128::MAX / 4);

    kani::assume(q_minted <= p_amount);
    kani::assume(p_amount - q_minted < b_rate); // q_minted + b_rate would overflow
    kani::assume(p_amount % SCALAR_12 == 0);
    let amount = p_amount / SCALAR_12;

    let total_before = p_before / SCALAR_12;
    let total_after = (p_before + q_minted) / SCALAR_12;

    assert!(total_after <= total_before + amount);
    assert!(total_after >= total_before);
}
