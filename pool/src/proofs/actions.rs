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
