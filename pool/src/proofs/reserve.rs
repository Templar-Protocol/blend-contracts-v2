use super::*;

/// Rate in exact hundredths of SCALAR_12 (u8 domain keeps every product
/// far inside i128). `h == 0` is a genuine zero forward rate.
fn hundredths(h: u8) -> i128 {
    (h as i128 * SCALAR_12) / 100
}

fn sym_data(d_rate: i128, b_rate: i128, b_supply: i128, d_supply: i128) -> ReserveData {
    ReserveData {
        d_rate,
        b_rate,
        ir_mod: 0,
        b_supply,
        d_supply,
        backstop_credit: 0,
        last_time: 0,
    }
}

#[kani::proof]
fn prove_b_token_forward_floor() {
    let amount = kani::any::<u8>() as i128;
    let b_rate = hundredths(kani::any::<u8>()); // zero b_rate included
    let data = sym_data(0, b_rate, 0, 0);
    let out = data.to_asset_from_b_token(&(), amount);
    let product = amount * b_rate;
    assert_eq!(out, product / SCALAR_12);
    assert!(out * SCALAR_12 <= product && (out + 1) * SCALAR_12 > product);
    if b_rate == 0 {
        assert_eq!(out, 0); // real zero-forward-rate branch
    }
}

#[kani::proof]
fn prove_d_token_forward_ceil() {
    let amount = kani::any::<u8>() as i128;
    let rate_h = kani::any::<u8>();
    kani::assume(rate_h >= 1); // production d_rate starts at unity and only grows
    let d_rate = hundredths(rate_h);
    let data = sym_data(d_rate, 0, 0, 0);
    let out = data.to_asset_from_d_token(&(), amount);
    let product = amount * d_rate;
    assert_eq!(out, (product + SCALAR_12 - 1) / SCALAR_12);
    assert!((out - 1) * SCALAR_12 < product && product <= out * SCALAR_12);
}

#[kani::proof]
fn prove_b_token_inverse_rounding() {
    let amount = kani::any::<u8>() as i128;
    let rate_h = kani::any::<u8>();
    kani::assume(rate_h >= 1); // inverse conversions divide by b_rate
    let b_rate = hundredths(rate_h);
    let data = sym_data(0, b_rate, 0, 0);
    let down = data.to_b_token_down(&(), amount);
    let up = data.to_b_token_up(&(), amount);
    let product = amount * SCALAR_12;
    assert_eq!(down, product / b_rate);
    assert_eq!(up, (product + b_rate - 1) / b_rate);
    assert!(down * b_rate <= product && product < (down + 1) * b_rate);
    assert!(product <= up * b_rate && (up - 1) * b_rate < product);
    assert!(up >= down && up - down <= 1);
}

#[kani::proof]
fn prove_d_token_inverse_rounding() {
    let amount = kani::any::<u8>() as i128;
    let rate_h = kani::any::<u8>();
    kani::assume(rate_h >= 1); // inverse conversions divide by d_rate
    let d_rate = hundredths(rate_h);
    let data = sym_data(d_rate, 0, 0, 0);
    let down = data.to_d_token_down(&(), amount);
    let up = data.to_d_token_up(&(), amount);
    let product = amount * SCALAR_12;
    assert_eq!(down, product / d_rate);
    assert_eq!(up, (product + d_rate - 1) / d_rate);
    assert!(down * d_rate <= product && product < (down + 1) * d_rate);
    assert!(product <= up * d_rate && (up - 1) * d_rate < product);
    assert!(up >= down && up - down <= 1);
}

#[kani::proof]
fn prove_b_round_trip_down_conservative() {
    let amount = kani::any::<u8>() as i128;
    let rate_h = kani::any::<u8>();
    kani::assume(rate_h >= 1);
    let b_rate = hundredths(rate_h);
    let data = sym_data(0, b_rate, 0, 0);
    let b_tokens = data.to_b_token_down(&(), amount);
    let out = data.to_asset_from_b_token(&(), b_tokens);
    assert!(out <= amount); // no underlying value created
    assert!(out + b_rate / SCALAR_12 + 2 >= amount); // dust bounded by the rate, not a fixed unit
}

#[kani::proof]
fn prove_b_round_trip_up_conservative() {
    let amount = kani::any::<u8>() as i128;
    let rate_h = kani::any::<u8>();
    kani::assume(rate_h >= 1);
    let b_rate = hundredths(rate_h);
    let data = sym_data(0, b_rate, 0, 0);
    let b_tokens = data.to_b_token_up(&(), amount);
    let out = data.to_asset_from_b_token(&(), b_tokens);
    assert!(out >= amount); // burning up never strands the requested amount
    assert!(out <= amount + b_rate / SCALAR_12);
}

#[kani::proof]
fn prove_d_round_trip_up_conservative() {
    let amount = kani::any::<u8>() as i128;
    let rate_h = kani::any::<u8>();
    kani::assume(rate_h >= 1);
    let d_rate = hundredths(rate_h);
    let data = sym_data(d_rate, 0, 0, 0);
    let d_tokens = data.to_d_token_up(&(), amount);
    let out = data.to_asset_from_d_token(&(), d_tokens);
    assert!(out >= amount); // debt is never under-reported by the up conversion
    assert!(out <= amount + d_rate / SCALAR_12 + 1);
}

#[kani::proof]
fn prove_d_round_trip_down_conservative() {
    let amount = kani::any::<u8>() as i128;
    let rate_h = kani::any::<u8>();
    kani::assume(rate_h >= 1);
    let d_rate = hundredths(rate_h);
    let data = sym_data(d_rate, 0, 0, 0);
    let d_tokens = data.to_d_token_down(&(), amount);
    let out = data.to_asset_from_d_token(&(), d_tokens);
    assert!(out <= amount); // debt is never over-reported by the down conversion
    assert!(out + d_rate / SCALAR_12 + 1 >= amount);
}

#[kani::proof]
fn prove_utilization_range_and_branches() {
    let d_rate_h = kani::any::<u8>();
    kani::assume(d_rate_h >= 1);
    let data = sym_data(
        hundredths(d_rate_h),
        hundredths(kani::any::<u8>()), // zero b_rate reaches the real saturation branch
        kani::any::<u8>() as i128,
        kani::any::<u8>() as i128,
    );
    let util = data.utilization(&());
    assert!(util >= 0 && util <= SCALAR_7);
    let liabilities = data.to_asset_from_d_token(&(), data.d_supply);
    let supply = data.to_asset_from_b_token(&(), data.b_supply);
    if liabilities == 0 {
        assert_eq!(util, 0); // zero-liability branch
    } else if liabilities >= supply {
        assert_eq!(util, SCALAR_7); // cap branch, including zero supply
    } else {
        assert_eq!(util, (liabilities * SCALAR_7 + supply - 1) / supply);
        assert!(util > 0);
    }
}

#[kani::proof]
fn prove_utilization_and_action_predicates() {
    let util = kani::any::<i128>();
    let max_util = kani::any::<u32>();
    assert_eq!(
        utilization_within_max(util, max_util),
        util <= max_util as i128
    );
    assert_eq!(utilization_below_100(util), util < SCALAR_7);
    let enabled = kani::any::<bool>();
    let action = kani::any::<u32>();
    let protected = action == RequestType::Supply as u32
        || action == RequestType::SupplyCollateral as u32
        || action == RequestType::Borrow as u32;
    assert_eq!(
        reserve_action_allowed(enabled, action),
        enabled || !protected
    );
}

#[kani::proof]
fn prove_grow_debt_nonnegative_interest() {
    let d_supply = kani::any::<u8>() as i128;
    let rate_h = kani::any::<u8>();
    kani::assume(rate_h >= 1);
    let accrual_h = kani::any::<u8>();
    kani::assume(accrual_h >= 100); // loan accrual at least the unity scalar
    let mut data = sym_data(hundredths(rate_h), 0, 0, d_supply);
    let old_rate = data.d_rate;
    let accrued = data.grow_debt(&(), hundredths(accrual_h));
    assert!(data.d_rate >= old_rate);
    let old_liabilities = (old_rate * d_supply + SCALAR_12 - 1) / SCALAR_12;
    let new_liabilities = (data.d_rate * d_supply + SCALAR_12 - 1) / SCALAR_12;
    assert_eq!(accrued, new_liabilities - old_liabilities);
    assert!(accrued >= 0); // debt interest is nonnegative
}

#[kani::proof]
fn prove_accrue_split_and_zero_take_rate() {
    let b_supply = kani::any::<u8>() as i128;
    kani::assume(b_supply >= 1); // b_rate division divisor
    let mut data = sym_data(0, hundredths(kani::any::<u8>()), b_supply, 0);
    let accrued = kani::any::<u8>() as i128;
    let take_h = kani::any::<u8>();
    kani::assume(take_h < 100); // take rate is a percentage in SCALAR_7 points, < 100%
    let bstop_rate = (take_h as u32 * SCALAR_7 as u32) / 100; // < SCALAR_7, genuine zero included
    let old_credit = kani::any::<u8>() as i128;
    data.backstop_credit = old_credit;
    let pre_supply = data.to_asset_from_b_token(&(), data.b_supply);
    let old_b_rate = data.b_rate;
    data.accrue(&(), bstop_rate, accrued);
    if accrued == 0 {
        assert_eq!(data.b_rate, old_b_rate); // no-accrual branch leaves rates alone
        assert_eq!(data.backstop_credit, old_credit);
    } else {
        let expected_credit = if bstop_rate == 0 {
            0
        } else {
            (accrued * bstop_rate as i128) / SCALAR_7
        };
        assert_eq!(data.backstop_credit, old_credit + expected_credit);
        if bstop_rate == 0 {
            assert_eq!(data.backstop_credit, old_credit); // zero take-rate branch
        }
        assert!(expected_credit <= accrued);
        assert_eq!(
            data.b_rate,
            (pre_supply + accrued - expected_credit) * SCALAR_12 / b_supply
        );
        // outer-floor bound over BOTH nested floors, in asset units: no value minted
        let credit_delta = data.backstop_credit - old_credit;
        let actual_after_supply = data.to_asset_from_b_token(&(), b_supply);
        assert!(credit_delta + actual_after_supply <= pre_supply + accrued);
        let gap = pre_supply + accrued - credit_delta - actual_after_supply;
        assert!(gap >= 0);
        // deficit is the sum of both floor residuals: rate residual < b_supply, asset residual < SCALAR_12
        assert!(gap * SCALAR_12 < b_supply + SCALAR_12);
    }
}

// Independent diagnostics: original u8 amount / positive u8 rate_h domain.
// Keep the original combined harness above unchanged.
#[kani::proof]
fn independent_probe_b_inverse_down_exact_original() {
    let amount = kani::any::<u8>() as i128;
    let rate_h = kani::any::<u8>();
    kani::assume(rate_h >= 1);
    let b_rate = hundredths(rate_h);
    let data = sym_data(0, b_rate, 0, 0);
    let down = data.to_b_token_down(&(), amount);
    let product = amount * SCALAR_12;
    assert_eq!(down, product / b_rate);
}

#[kani::proof]
fn independent_probe_b_inverse_up_exact_original() {
    let amount = kani::any::<u8>() as i128;
    let rate_h = kani::any::<u8>();
    kani::assume(rate_h >= 1);
    let b_rate = hundredths(rate_h);
    let data = sym_data(0, b_rate, 0, 0);
    let up = data.to_b_token_up(&(), amount);
    let product = amount * SCALAR_12;
    assert_eq!(up, (product + b_rate - 1) / b_rate);
}

#[kani::proof]
fn independent_probe_b_inverse_down_floor_bounds() {
    let amount = kani::any::<u8>() as i128;
    let rate_h = kani::any::<u8>();
    kani::assume(rate_h >= 1);
    let b_rate = hundredths(rate_h);
    let data = sym_data(0, b_rate, 0, 0);
    let down = data.to_b_token_down(&(), amount);
    let product = amount * SCALAR_12;
    assert!(down * b_rate <= product && product < (down + 1) * b_rate);
}

#[kani::proof]
fn independent_probe_b_inverse_up_ceiling_bounds() {
    let amount = kani::any::<u8>() as i128;
    let rate_h = kani::any::<u8>();
    kani::assume(rate_h >= 1);
    let b_rate = hundredths(rate_h);
    let data = sym_data(0, b_rate, 0, 0);
    let up = data.to_b_token_up(&(), amount);
    let product = amount * SCALAR_12;
    assert!(product <= up * b_rate && (up - 1) * b_rate < product);
}

#[kani::proof]
fn independent_probe_b_inverse_adjacency() {
    let amount = kani::any::<u8>() as i128;
    let rate_h = kani::any::<u8>();
    kani::assume(rate_h >= 1);
    let data = sym_data(0, hundredths(rate_h), 0, 0);
    let down = data.to_b_token_down(&(), amount);
    let up = data.to_b_token_up(&(), amount);
    assert!(up >= down && up - down <= 1);
}

// Added non-vacuity witnesses, not assertions present in the original.
#[kani::proof]
fn independent_probe_b_inverse_cover_zero_amount() {
    let amount = kani::any::<u8>() as i128;
    let rate_h = kani::any::<u8>();
    kani::assume(rate_h >= 1);
    let data = sym_data(0, hundredths(rate_h), 0, 0);
    let down = data.to_b_token_down(&(), amount);
    let up = data.to_b_token_up(&(), amount);
    kani::cover!(amount == 0 && down == 0 && up == 0, "zero amount");
}

#[kani::proof]
fn independent_probe_b_inverse_cover_exact_quotient() {
    let amount = kani::any::<u8>() as i128;
    let rate_h = kani::any::<u8>();
    kani::assume(rate_h >= 1);
    let b_rate = hundredths(rate_h);
    let data = sym_data(0, b_rate, 0, 0);
    let down = data.to_b_token_down(&(), amount);
    let up = data.to_b_token_up(&(), amount);
    let product = amount * SCALAR_12;
    kani::cover!(
        amount > 0 && product % b_rate == 0 && down == up,
        "positive amount exact quotient"
    );
}

#[kani::proof]
fn independent_probe_b_inverse_cover_nonzero_remainder() {
    let amount = kani::any::<u8>() as i128;
    let rate_h = kani::any::<u8>();
    kani::assume(rate_h >= 1);
    let b_rate = hundredths(rate_h);
    let data = sym_data(0, b_rate, 0, 0);
    let down = data.to_b_token_down(&(), amount);
    let up = data.to_b_token_up(&(), amount);
    let product = amount * SCALAR_12;
    kani::cover!(product % b_rate != 0 && up == down + 1, "nonzero remainder");
}

// Only the independent oracle is cancelled; source arithmetic stays i128.
// SCALAR_12 = 100*k, k > 0, so both rational expressions are identical.
#[kani::proof]
fn independent_probe_b_inverse_down_exact_cancelled() {
    let amount = kani::any::<u8>() as i128;
    let rate_h = kani::any::<u8>();
    kani::assume(rate_h >= 1);
    let data = sym_data(0, hundredths(rate_h), 0, 0);
    let down = data.to_b_token_down(&(), amount);
    assert_eq!(down, amount * 100 / rate_h as i128);
}

#[kani::proof]
fn independent_probe_b_inverse_up_exact_cancelled() {
    let amount = kani::any::<u8>() as i128;
    let rate_h = kani::any::<u8>();
    kani::assume(rate_h >= 1);
    let data = sym_data(0, hundredths(rate_h), 0, 0);
    let up = data.to_b_token_up(&(), amount);
    assert_eq!(up, (amount * 100 + rate_h as i128 - 1) / rate_h as i128);
}

#[kani::proof]
fn independent_probe_b_inverse_oracle_cancellation_link() {
    let amount = kani::any::<u8>() as i128;
    let rate_h = kani::any::<u8>();
    kani::assume(rate_h >= 1);
    let b_rate = hundredths(rate_h);
    let data = sym_data(0, b_rate, 0, 0);
    // Exercise the same kernels for safety; only oracle equivalence is
    // asserted here, so this harness alone proves no conversion result.
    let _down = data.to_b_token_down(&(), amount);
    let _up = data.to_b_token_up(&(), amount);
    assert!(SCALAR_12 > 0 && SCALAR_12 % 100 == 0);
    assert_eq!(b_rate, rate_h as i128 * (SCALAR_12 / 100));
    let product = amount * SCALAR_12;
    assert_eq!(product / b_rate, amount * 100 / rate_h as i128);
    assert_eq!(
        (product + b_rate - 1) / b_rate,
        (amount * 100 + rate_h as i128 - 1) / rate_h as i128
    );
}

// Optional constant-denominator controls: proper subdomains, not a proof
// of the original domain or permission to omit the other 253 rates.
#[kani::proof]
fn independent_probe_b_inverse_const_rate_h_1() {
    let amount = kani::any::<u8>() as i128;
    let b_rate = hundredths(1);
    let data = sym_data(0, b_rate, 0, 0);
    let down = data.to_b_token_down(&(), amount);
    let up = data.to_b_token_up(&(), amount);
    let product = amount * SCALAR_12;
    assert_eq!(down, product / b_rate);
    assert_eq!(up, (product + b_rate - 1) / b_rate);
    assert!(down * b_rate <= product && product < (down + 1) * b_rate);
    assert!(product <= up * b_rate && (up - 1) * b_rate < product);
    assert!(up >= down && up - down <= 1);
}

#[kani::proof]
fn independent_probe_b_inverse_const_rate_h_255() {
    let amount = kani::any::<u8>() as i128;
    let b_rate = hundredths(255);
    let data = sym_data(0, b_rate, 0, 0);
    let down = data.to_b_token_down(&(), amount);
    let up = data.to_b_token_up(&(), amount);
    let product = amount * SCALAR_12;
    assert_eq!(down, product / b_rate);
    assert_eq!(up, (product + b_rate - 1) / b_rate);
    assert!(down * b_rate <= product && product < (down + 1) * b_rate);
    assert!(product <= up * b_rate && (up - 1) * b_rate < product);
    assert!(up >= down && up - down <= 1);
}

#[kani::proof]
fn independent_probe_b_d_inverse_equivalence() {
    let amount = kani::any::<u8>() as i128;
    let rate_h = kani::any::<u8>();
    kani::assume(rate_h >= 1);
    let rate = hundredths(rate_h);
    let b_data = sym_data(0, rate, 0, 0);
    let d_data = sym_data(rate, 0, 0, 0);
    assert_eq!(
        b_data.to_b_token_down(&(), amount),
        d_data.to_d_token_down(&(), amount)
    );
    assert_eq!(
        b_data.to_b_token_up(&(), amount),
        d_data.to_d_token_up(&(), amount)
    );
}

#[kani::proof]
fn astra_utilization_zero_liabilities() {
    let d_rate_h = kani::any::<u8>();
    kani::assume(d_rate_h >= 1);
    let data = sym_data(
        hundredths(d_rate_h),
        hundredths(kani::any::<u8>()), // zero b_rate reaches the real saturation branch
        kani::any::<u8>() as i128,
        kani::any::<u8>() as i128,
    );
    let liabilities = data.to_asset_from_d_token(&(), data.d_supply);
    let supply = data.to_asset_from_b_token(&(), data.b_supply);
    kani::assume(liabilities == 0);
    let util = data.utilization(&());
    assert!(util >= 0 && util <= SCALAR_7);
    assert_eq!(util, 0); // zero-liability branch
    kani::cover!(liabilities == 0, "D3 zero-liability partition reachable");
    kani::cover!(
        liabilities == 0 && supply == 0,
        "D3 zero liabilities and supply reachable"
    );
}

#[kani::proof]
fn astra_utilization_saturated() {
    let d_rate_h = kani::any::<u8>();
    kani::assume(d_rate_h >= 1);
    let data = sym_data(
        hundredths(d_rate_h),
        hundredths(kani::any::<u8>()), // zero b_rate reaches the real saturation branch
        kani::any::<u8>() as i128,
        kani::any::<u8>() as i128,
    );
    let liabilities = data.to_asset_from_d_token(&(), data.d_supply);
    let supply = data.to_asset_from_b_token(&(), data.b_supply);
    kani::assume(liabilities != 0 && liabilities >= supply);
    let util = data.utilization(&());
    assert!(util >= 0 && util <= SCALAR_7);
    assert_eq!(util, SCALAR_7); // cap branch, including zero supply
    kani::cover!(
        liabilities != 0 && liabilities >= supply,
        "D3 saturation partition reachable"
    );
    kani::cover!(liabilities == supply, "D3 positive equality reachable");
    kani::cover!(data.b_rate == 0, "D3 zero b-rate saturation reachable");
    kani::cover!(data.b_supply == 0, "D3 zero b-supply saturation reachable");
}

#[kani::proof]
fn astra_utilization_interior() {
    let d_rate_h = kani::any::<u8>();
    kani::assume(d_rate_h >= 1);
    let data = sym_data(
        hundredths(d_rate_h),
        hundredths(kani::any::<u8>()), // zero b_rate reaches the real saturation branch
        kani::any::<u8>() as i128,
        kani::any::<u8>() as i128,
    );
    let liabilities = data.to_asset_from_d_token(&(), data.d_supply);
    let supply = data.to_asset_from_b_token(&(), data.b_supply);
    kani::assume(liabilities != 0 && liabilities < supply);
    let util = data.utilization(&());
    assert!(util >= 0 && util <= SCALAR_7);
    assert_eq!(util, (liabilities * SCALAR_7 + supply - 1) / supply);
    assert!(util > 0);
    kani::cover!(
        liabilities != 0 && liabilities < supply,
        "D3 interior partition reachable"
    );
}

#[kani::proof]
fn astra_grow_debt_rate_monotonic() {
    let d_supply = kani::any::<u8>() as i128;
    let rate_h = kani::any::<u8>();
    kani::assume(rate_h >= 1);
    let accrual_h = kani::any::<u8>();
    kani::assume(accrual_h >= 100); // loan accrual at least the unity scalar
    let mut data = sym_data(hundredths(rate_h), 0, 0, d_supply);
    let old_rate = data.d_rate;
    let _accrued = data.grow_debt(&(), hundredths(accrual_h));
    assert!(data.d_rate >= old_rate);
    kani::cover!(true, "E2-1 rate domain reachable");
    kani::cover!(accrual_h == 100, "E2-1 rate unity reachable");
    kani::cover!(d_supply == 0, "E2-1 rate zero supply reachable");
    kani::cover!(
        accrual_h > 100 && d_supply > 0,
        "E2-1 rate nontrivial growth reachable"
    );
}

#[kani::proof]
fn astra_grow_debt_exact_delta() {
    let d_supply = kani::any::<u8>() as i128;
    let rate_h = kani::any::<u8>();
    kani::assume(rate_h >= 1);
    let accrual_h = kani::any::<u8>();
    kani::assume(accrual_h >= 100); // loan accrual at least the unity scalar
    let mut data = sym_data(hundredths(rate_h), 0, 0, d_supply);
    let old_rate = data.d_rate;
    let accrued = data.grow_debt(&(), hundredths(accrual_h));
    let old_liabilities = (old_rate * d_supply + SCALAR_12 - 1) / SCALAR_12;
    let new_liabilities = (data.d_rate * d_supply + SCALAR_12 - 1) / SCALAR_12;
    assert_eq!(accrued, new_liabilities - old_liabilities);
    kani::cover!(true, "E2-1 exact delta domain reachable");
    kani::cover!(accrual_h == 100, "E2-1 exact delta unity reachable");
    kani::cover!(d_supply == 0, "E2-1 exact delta zero supply reachable");
    kani::cover!(
        accrual_h > 100 && d_supply > 0,
        "E2-1 exact delta nontrivial growth reachable"
    );
}

#[kani::proof]
fn astra_grow_debt_nonnegative_delta() {
    let d_supply = kani::any::<u8>() as i128;
    let rate_h = kani::any::<u8>();
    kani::assume(rate_h >= 1);
    let accrual_h = kani::any::<u8>();
    kani::assume(accrual_h >= 100); // loan accrual at least the unity scalar
    let mut data = sym_data(hundredths(rate_h), 0, 0, d_supply);
    let accrued = data.grow_debt(&(), hundredths(accrual_h));
    assert!(accrued >= 0); // debt interest is nonnegative
    kani::cover!(true, "E2-1 nonnegative delta domain reachable");
    kani::cover!(accrual_h == 100, "E2-1 nonnegative delta unity reachable");
    kani::cover!(
        d_supply == 0,
        "E2-1 nonnegative delta zero supply reachable"
    );
    kani::cover!(
        accrual_h > 100 && d_supply > 0,
        "E2-1 nonnegative delta nontrivial growth reachable"
    );
}

#[kani::proof]
fn astra_accrue_zero() {
    let b_supply = kani::any::<u8>() as i128;
    kani::assume(b_supply >= 1); // b_rate division divisor
    let mut data = sym_data(0, hundredths(kani::any::<u8>()), b_supply, 0);
    let accrued = kani::any::<u8>() as i128;
    let take_h = kani::any::<u8>();
    kani::assume(take_h < 100); // take rate is a percentage in SCALAR_7 points, < 100%
    let bstop_rate = (take_h as u32 * SCALAR_7 as u32) / 100; // < SCALAR_7, genuine zero included
    let old_credit = kani::any::<u8>() as i128;
    data.backstop_credit = old_credit;
    let old_b_rate = data.b_rate;
    kani::assume(accrued == 0);
    data.accrue(&(), bstop_rate, accrued);
    assert_eq!(data.b_rate, old_b_rate); // no-accrual branch leaves rates alone
    assert_eq!(data.backstop_credit, old_credit);
    kani::cover!(accrued == 0, "E2-2 zero accrual partition reachable");
    kani::cover!(
        old_b_rate == 0,
        "E2-2 zero accrual zero initial rate reachable"
    );
}

#[kani::proof]
fn astra_accrue_positive_credit() {
    let b_supply = kani::any::<u8>() as i128;
    kani::assume(b_supply >= 1); // b_rate division divisor
    let mut data = sym_data(0, hundredths(kani::any::<u8>()), b_supply, 0);
    let accrued = kani::any::<u8>() as i128;
    let take_h = kani::any::<u8>();
    kani::assume(take_h < 100); // take rate is a percentage in SCALAR_7 points, < 100%
    let bstop_rate = (take_h as u32 * SCALAR_7 as u32) / 100; // < SCALAR_7, genuine zero included
    let old_credit = kani::any::<u8>() as i128;
    data.backstop_credit = old_credit;
    let old_b_rate = data.b_rate;
    kani::assume(accrued > 0);
    data.accrue(&(), bstop_rate, accrued);
    let expected_credit = if bstop_rate == 0 {
        0
    } else {
        (accrued * bstop_rate as i128) / SCALAR_7
    };
    assert_eq!(data.backstop_credit, old_credit + expected_credit);
    if bstop_rate == 0 {
        assert_eq!(data.backstop_credit, old_credit); // zero take-rate branch
    }
    assert!(expected_credit <= accrued);
    kani::cover!(accrued > 0, "E2-2 positive credit partition reachable");
    kani::cover!(bstop_rate == 0, "E2-2 positive credit zero take reachable");
    kani::cover!(
        bstop_rate > 0,
        "E2-2 positive credit nonzero take reachable"
    );
    kani::cover!(
        old_b_rate == 0,
        "E2-2 positive credit zero initial rate reachable"
    );
}

#[kani::proof]
fn astra_accrue_positive_rate() {
    let b_supply = kani::any::<u8>() as i128;
    kani::assume(b_supply >= 1); // b_rate division divisor
    let mut data = sym_data(0, hundredths(kani::any::<u8>()), b_supply, 0);
    let accrued = kani::any::<u8>() as i128;
    let take_h = kani::any::<u8>();
    kani::assume(take_h < 100); // take rate is a percentage in SCALAR_7 points, < 100%
    let bstop_rate = (take_h as u32 * SCALAR_7 as u32) / 100; // < SCALAR_7, genuine zero included
    let old_credit = kani::any::<u8>() as i128;
    data.backstop_credit = old_credit;
    let pre_supply = data.to_asset_from_b_token(&(), data.b_supply);
    let old_b_rate = data.b_rate;
    kani::assume(accrued > 0);
    data.accrue(&(), bstop_rate, accrued);
    let expected_credit = if bstop_rate == 0 {
        0
    } else {
        (accrued * bstop_rate as i128) / SCALAR_7
    };
    assert_eq!(
        data.b_rate,
        (pre_supply + accrued - expected_credit) * SCALAR_12 / b_supply
    );
    kani::cover!(accrued > 0, "E2-2 positive rate partition reachable");
    kani::cover!(bstop_rate == 0, "E2-2 positive rate zero take reachable");
    kani::cover!(bstop_rate > 0, "E2-2 positive rate nonzero take reachable");
    kani::cover!(
        old_b_rate == 0,
        "E2-2 positive rate zero initial rate reachable"
    );
}

#[kani::proof]
fn astra_accrue_positive_accounting() {
    let b_supply = kani::any::<u8>() as i128;
    kani::assume(b_supply >= 1); // b_rate division divisor
    let mut data = sym_data(0, hundredths(kani::any::<u8>()), b_supply, 0);
    let accrued = kani::any::<u8>() as i128;
    let take_h = kani::any::<u8>();
    kani::assume(take_h < 100); // take rate is a percentage in SCALAR_7 points, < 100%
    let bstop_rate = (take_h as u32 * SCALAR_7 as u32) / 100; // < SCALAR_7, genuine zero included
    let old_credit = kani::any::<u8>() as i128;
    data.backstop_credit = old_credit;
    let pre_supply = data.to_asset_from_b_token(&(), data.b_supply);
    let old_b_rate = data.b_rate;
    kani::assume(accrued > 0);
    data.accrue(&(), bstop_rate, accrued);
    // outer-floor bound over BOTH nested floors, in asset units: no value minted
    let credit_delta = data.backstop_credit - old_credit;
    let actual_after_supply = data.to_asset_from_b_token(&(), b_supply);
    assert!(credit_delta + actual_after_supply <= pre_supply + accrued);
    let gap = pre_supply + accrued - credit_delta - actual_after_supply;
    assert!(gap >= 0);
    // deficit is the sum of both floor residuals: rate residual < b_supply, asset residual < SCALAR_12
    assert!(gap * SCALAR_12 < b_supply + SCALAR_12);
    kani::cover!(accrued > 0, "E2-2 positive accounting partition reachable");
    kani::cover!(
        bstop_rate == 0,
        "E2-2 positive accounting zero take reachable"
    );
    kani::cover!(
        bstop_rate > 0,
        "E2-2 positive accounting nonzero take reachable"
    );
    kani::cover!(
        old_b_rate == 0,
        "E2-2 positive accounting zero initial rate reachable"
    );
}

#[kani::proof]
fn astra_utilization_ceil_assets() {
    let l = kani::any::<u16>() as i128;
    let p = kani::any::<u16>() as i128;
    kani::assume(1 <= l && l < p && p <= 650);

    // REAL checked-i128 helper: no contract assumptions or replacement math.
    let q = FixedMath::ceil(&(), l, SCALAR_7, p);
    assert!(0 < q && q <= SCALAR_7);
    assert_eq!(q, (l * SCALAR_7 + p - 1) / p);

    kani::cover!(l == 1 && p == 2, "exact quotient");
    kani::cover!(l == 1 && p == 3, "nonzero remainder");
    kani::cover!(l == 649 && p == 650, "helper upper boundary");
}

// Admit only after astra_utilization_ceil_assets has a complete source-bound PASS.
struct AstraUtilInteriorMath {
    liabilities: i128,
    supply: i128,
}

impl FixedMath for AstraUtilInteriorMath {
    fn floor(&self, x: i128, y: i128, denominator: i128) -> i128 {
        FixedMath::floor(&(), x, y, denominator)
    }

    fn ceil(&self, x: i128, y: i128, denominator: i128) -> i128 {
        if denominator == SCALAR_12 {
            // Actual forward debt conversion remains real checked arithmetic.
            return FixedMath::ceil(&(), x, y, denominator);
        }
        // ASSERT the actual call tuple and proved helper domain; never assume them.
        assert_eq!(x, self.liabilities);
        assert_eq!(y, SCALAR_7);
        assert_eq!(denominator, self.supply);
        assert!(1 <= x && x < denominator && denominator <= 650);

        let q = kani::any::<i128>();
        // Exactly the two independently proved helper postconditions.
        kani::assume(0 < q && q <= SCALAR_7);
        kani::assume(q == (x * y + denominator - 1) / denominator);
        q
    }
}

#[kani::proof]
fn astra_utilization_interior_composed() {
    let d_rate_h = kani::any::<u8>();
    kani::assume(d_rate_h >= 1);
    let data = sym_data(
        hundredths(d_rate_h),
        hundredths(kani::any::<u8>()), // zero b_rate reaches the real saturation branch
        kani::any::<u8>() as i128,
        kani::any::<u8>() as i128,
    );
    let liabilities = data.to_asset_from_d_token(&(), data.d_supply);
    let supply = data.to_asset_from_b_token(&(), data.b_supply);

    // Only the two source-bound, already-PASS forward conversion identities.
    kani::assume(liabilities == (data.d_supply * data.d_rate + SCALAR_12 - 1) / SCALAR_12);
    kani::assume(supply == data.b_supply * data.b_rate / SCALAR_12);
    assert!(0 <= liabilities && liabilities <= 651);
    assert!(0 <= supply && supply <= 650);

    // Preserve the original partition, then discharge helper applicability.
    kani::assume(liabilities != 0 && liabilities < supply);
    assert!(1 <= liabilities && liabilities < supply && supply <= 650);

    let math = AstraUtilInteriorMath {
        liabilities,
        supply,
    };
    let util = data.utilization(&math);
    assert!(util >= 0 && util <= SCALAR_7);
    assert_eq!(util, (liabilities * SCALAR_7 + supply - 1) / supply);
    assert!(util > 0);

    kani::cover!(liabilities == 1 && supply == 2, "caller exact quotient");
    kani::cover!(liabilities == 1 && supply == 3, "caller nonzero remainder");
    kani::cover!(liabilities == 648 && supply == 650, "caller high assets");
}

// Conditional composition: source-bound real forward identities/ranges transfer
// the original u8 ReserveData domain into L in 0..=651 and P in 0..=650.
// Independent L/P overapproximate those outputs; no config/output relationship
// is assumed here. Original zero/cap branches remain separate obligations.
// The interior q contract requires ALL 649 real-helper divisor proofs
// astra_utilization_ceil_supply_002..650, with
// matching source and arithmetic dependency. Canaries alone do not suffice.
struct AstraUtilParametricMath {
    debt: (i128, i128, i128),
    supplier: (i128, i128, i128),
    liabilities: i128,
    supply: i128,
    quotient: i128,
    calls: core::cell::Cell<u8>,
}

impl FixedMath for AstraUtilParametricMath {
    fn floor(&self, x: i128, y: i128, denominator: i128) -> i128 {
        assert_eq!(self.calls.get(), 1);
        assert_eq!((x, y, denominator), self.supplier);
        self.calls.set(2);
        self.supply
    }

    fn ceil(&self, x: i128, y: i128, denominator: i128) -> i128 {
        match self.calls.get() {
            0 => {
                assert_eq!((x, y, denominator), self.debt);
                self.calls.set(1);
                self.liabilities
            }
            2 => {
                assert_eq!(
                    (x, y, denominator),
                    (self.liabilities, SCALAR_7, self.supply)
                );
                assert!(
                    1 <= self.liabilities && self.liabilities < self.supply && self.supply <= 650
                );
                let q = self.quotient;
                // Only the complete, source-bound helper contract above.
                kani::assume(0 < q && q <= SCALAR_7);
                self.calls.set(3);
                q
            }
            _ => panic!("unexpected utilization ceil call"),
        }
    }
}

#[kani::proof]
fn astra_utilization_interior_parametric() {
    let d_rate_h = kani::any::<u8>();
    kani::assume(d_rate_h >= 1);
    let data = sym_data(
        hundredths(d_rate_h),
        hundredths(kani::any::<u8>()),
        kani::any::<u8>() as i128,
        kani::any::<u8>() as i128,
    );
    let liabilities = kani::any::<u16>() as i128;
    let supply = kani::any::<u16>() as i128;
    kani::assume(1 <= liabilities && liabilities <= 651);
    kani::assume(supply <= 650);
    kani::assume(0 < liabilities && liabilities < supply);

    let math = AstraUtilParametricMath {
        debt: (data.d_supply, data.d_rate, SCALAR_12),
        supplier: (data.b_supply, data.b_rate, SCALAR_12),
        liabilities,
        supply,
        quotient: kani::any::<i128>(),
        calls: core::cell::Cell::new(0),
    };
    let util = data.utilization(&math);
    assert_eq!(math.calls.get(), 3);
    assert!(util >= 0 && util <= SCALAR_7);
    // This proves output composition only, not a standalone original-oracle PASS.
    // Original literal equality requires all 649 source-bound real helper proofs
    // astra_utilization_ceil_supply_002..650.
    assert_eq!(util, math.quotient);
    assert!(util > 0);

    // Parameter-pair covers, NOT original-input reachability evidence.
    kani::cover!(liabilities == 1 && supply == 2, "parametric exact quotient");
    kani::cover!(
        liabilities == 1 && supply == 3,
        "parametric nonzero remainder"
    );
    kani::cover!(
        liabilities == 648 && supply == 650,
        "parametric high assets"
    );
}

// Concrete original-domain witnesses use only the real checked arithmetic.
#[kani::proof]
fn astra_utilization_real_witness_half() {
    let data = sym_data(hundredths(100), hundredths(100), 2, 1);
    let liabilities = data.to_asset_from_d_token(&(), data.d_supply);
    let supply = data.to_asset_from_b_token(&(), data.b_supply);
    assert_eq!(liabilities, 1);
    assert_eq!(supply, 2);
    assert!(1 <= liabilities && liabilities < supply && supply <= 650);
    let util = data.utilization(&());
    assert!(util >= 0 && util <= SCALAR_7);
    assert_eq!(util, (liabilities * SCALAR_7 + supply - 1) / supply);
    assert!(util > 0);
    kani::cover!(liabilities == 1 && supply == 2, "real exact quotient");
}

#[kani::proof]
fn astra_utilization_real_witness_third() {
    let data = sym_data(hundredths(100), hundredths(100), 3, 1);
    let liabilities = data.to_asset_from_d_token(&(), data.d_supply);
    let supply = data.to_asset_from_b_token(&(), data.b_supply);
    assert_eq!(liabilities, 1);
    assert_eq!(supply, 3);
    assert!(1 <= liabilities && liabilities < supply && supply <= 650);
    let util = data.utilization(&());
    assert!(util >= 0 && util <= SCALAR_7);
    assert_eq!(util, (liabilities * SCALAR_7 + supply - 1) / supply);
    assert!(util > 0);
    kani::cover!(liabilities == 1 && supply == 3, "real nonzero remainder");
}

#[kani::proof]
fn astra_utilization_real_witness_high() {
    let data = sym_data(hundredths(255), hundredths(255), 255, 254);
    let liabilities = data.to_asset_from_d_token(&(), data.d_supply);
    let supply = data.to_asset_from_b_token(&(), data.b_supply);
    assert_eq!(liabilities, 648);
    assert_eq!(supply, 650);
    assert!(1 <= liabilities && liabilities < supply && supply <= 650);
    let util = data.utilization(&());
    assert!(util >= 0 && util <= SCALAR_7);
    assert_eq!(util, (liabilities * SCALAR_7 + supply - 1) / supply);
    assert!(util > 0);
    kani::cover!(liabilities == 648 && supply == 650, "real high assets");
}

// Full helper coverage requires every P in 2..=650; canaries are not closure.
fn astra_utilization_ceil_fixed_supply<const P: u16>() {
    let l = kani::any::<u16>() as i128;
    let p = P as i128;
    assert!(2 <= P && P <= 650);
    kani::assume(1 <= l && l < p && p <= 650);
    let q = FixedMath::ceil(&(), l, SCALAR_7, p);
    assert!(0 < q && q <= SCALAR_7);
    assert_eq!(q, (l * SCALAR_7 + p - 1) / p);
    kani::cover!(l == 1, "local lower liability endpoint");
    kani::cover!(l == p - 1, "local upper liability endpoint");
}

macro_rules! proof_case {
    ($name:ident, $helper:ident, $value:literal) => {
        #[kani::proof]
        fn $name() {
            $helper::<$value>();
        }
    };
}

proof_case!(
    astra_utilization_ceil_supply_002,
    astra_utilization_ceil_fixed_supply,
    2
);

proof_case!(
    astra_utilization_ceil_supply_003,
    astra_utilization_ceil_fixed_supply,
    3
);

proof_case!(
    astra_utilization_ceil_supply_650,
    astra_utilization_ceil_fixed_supply,
    650
);

proof_case!(
    astra_utilization_ceil_supply_004,
    astra_utilization_ceil_fixed_supply,
    4
);

proof_case!(
    astra_utilization_ceil_supply_005,
    astra_utilization_ceil_fixed_supply,
    5
);

proof_case!(
    astra_utilization_ceil_supply_006,
    astra_utilization_ceil_fixed_supply,
    6
);

proof_case!(
    astra_utilization_ceil_supply_007,
    astra_utilization_ceil_fixed_supply,
    7
);

proof_case!(
    astra_utilization_ceil_supply_008,
    astra_utilization_ceil_fixed_supply,
    8
);

proof_case!(
    astra_utilization_ceil_supply_009,
    astra_utilization_ceil_fixed_supply,
    9
);

proof_case!(
    astra_utilization_ceil_supply_010,
    astra_utilization_ceil_fixed_supply,
    10
);

proof_case!(
    astra_utilization_ceil_supply_011,
    astra_utilization_ceil_fixed_supply,
    11
);

proof_case!(
    astra_utilization_ceil_supply_012,
    astra_utilization_ceil_fixed_supply,
    12
);

proof_case!(
    astra_utilization_ceil_supply_013,
    astra_utilization_ceil_fixed_supply,
    13
);

proof_case!(
    astra_utilization_ceil_supply_014,
    astra_utilization_ceil_fixed_supply,
    14
);

proof_case!(
    astra_utilization_ceil_supply_015,
    astra_utilization_ceil_fixed_supply,
    15
);

proof_case!(
    astra_utilization_ceil_supply_016,
    astra_utilization_ceil_fixed_supply,
    16
);

proof_case!(
    astra_utilization_ceil_supply_017,
    astra_utilization_ceil_fixed_supply,
    17
);

proof_case!(
    astra_utilization_ceil_supply_018,
    astra_utilization_ceil_fixed_supply,
    18
);

proof_case!(
    astra_utilization_ceil_supply_019,
    astra_utilization_ceil_fixed_supply,
    19
);

proof_case!(
    astra_utilization_ceil_supply_020,
    astra_utilization_ceil_fixed_supply,
    20
);

proof_case!(
    astra_utilization_ceil_supply_021,
    astra_utilization_ceil_fixed_supply,
    21
);

proof_case!(
    astra_utilization_ceil_supply_022,
    astra_utilization_ceil_fixed_supply,
    22
);

proof_case!(
    astra_utilization_ceil_supply_023,
    astra_utilization_ceil_fixed_supply,
    23
);

proof_case!(
    astra_utilization_ceil_supply_024,
    astra_utilization_ceil_fixed_supply,
    24
);

proof_case!(
    astra_utilization_ceil_supply_025,
    astra_utilization_ceil_fixed_supply,
    25
);

proof_case!(
    astra_utilization_ceil_supply_026,
    astra_utilization_ceil_fixed_supply,
    26
);

proof_case!(
    astra_utilization_ceil_supply_027,
    astra_utilization_ceil_fixed_supply,
    27
);

proof_case!(
    astra_utilization_ceil_supply_028,
    astra_utilization_ceil_fixed_supply,
    28
);

proof_case!(
    astra_utilization_ceil_supply_029,
    astra_utilization_ceil_fixed_supply,
    29
);

proof_case!(
    astra_utilization_ceil_supply_030,
    astra_utilization_ceil_fixed_supply,
    30
);

proof_case!(
    astra_utilization_ceil_supply_031,
    astra_utilization_ceil_fixed_supply,
    31
);

proof_case!(
    astra_utilization_ceil_supply_032,
    astra_utilization_ceil_fixed_supply,
    32
);

proof_case!(
    astra_utilization_ceil_supply_033,
    astra_utilization_ceil_fixed_supply,
    33
);

proof_case!(
    astra_utilization_ceil_supply_034,
    astra_utilization_ceil_fixed_supply,
    34
);

proof_case!(
    astra_utilization_ceil_supply_035,
    astra_utilization_ceil_fixed_supply,
    35
);

proof_case!(
    astra_utilization_ceil_supply_036,
    astra_utilization_ceil_fixed_supply,
    36
);

proof_case!(
    astra_utilization_ceil_supply_037,
    astra_utilization_ceil_fixed_supply,
    37
);

proof_case!(
    astra_utilization_ceil_supply_038,
    astra_utilization_ceil_fixed_supply,
    38
);

proof_case!(
    astra_utilization_ceil_supply_039,
    astra_utilization_ceil_fixed_supply,
    39
);

proof_case!(
    astra_utilization_ceil_supply_040,
    astra_utilization_ceil_fixed_supply,
    40
);

proof_case!(
    astra_utilization_ceil_supply_041,
    astra_utilization_ceil_fixed_supply,
    41
);

proof_case!(
    astra_utilization_ceil_supply_042,
    astra_utilization_ceil_fixed_supply,
    42
);

proof_case!(
    astra_utilization_ceil_supply_043,
    astra_utilization_ceil_fixed_supply,
    43
);

proof_case!(
    astra_utilization_ceil_supply_044,
    astra_utilization_ceil_fixed_supply,
    44
);

proof_case!(
    astra_utilization_ceil_supply_045,
    astra_utilization_ceil_fixed_supply,
    45
);

proof_case!(
    astra_utilization_ceil_supply_046,
    astra_utilization_ceil_fixed_supply,
    46
);

proof_case!(
    astra_utilization_ceil_supply_047,
    astra_utilization_ceil_fixed_supply,
    47
);

proof_case!(
    astra_utilization_ceil_supply_048,
    astra_utilization_ceil_fixed_supply,
    48
);

proof_case!(
    astra_utilization_ceil_supply_049,
    astra_utilization_ceil_fixed_supply,
    49
);

proof_case!(
    astra_utilization_ceil_supply_050,
    astra_utilization_ceil_fixed_supply,
    50
);

proof_case!(
    astra_utilization_ceil_supply_051,
    astra_utilization_ceil_fixed_supply,
    51
);

proof_case!(
    astra_utilization_ceil_supply_052,
    astra_utilization_ceil_fixed_supply,
    52
);

proof_case!(
    astra_utilization_ceil_supply_053,
    astra_utilization_ceil_fixed_supply,
    53
);

proof_case!(
    astra_utilization_ceil_supply_054,
    astra_utilization_ceil_fixed_supply,
    54
);

proof_case!(
    astra_utilization_ceil_supply_055,
    astra_utilization_ceil_fixed_supply,
    55
);

proof_case!(
    astra_utilization_ceil_supply_056,
    astra_utilization_ceil_fixed_supply,
    56
);

proof_case!(
    astra_utilization_ceil_supply_057,
    astra_utilization_ceil_fixed_supply,
    57
);

proof_case!(
    astra_utilization_ceil_supply_058,
    astra_utilization_ceil_fixed_supply,
    58
);

proof_case!(
    astra_utilization_ceil_supply_059,
    astra_utilization_ceil_fixed_supply,
    59
);

proof_case!(
    astra_utilization_ceil_supply_060,
    astra_utilization_ceil_fixed_supply,
    60
);

proof_case!(
    astra_utilization_ceil_supply_061,
    astra_utilization_ceil_fixed_supply,
    61
);

proof_case!(
    astra_utilization_ceil_supply_062,
    astra_utilization_ceil_fixed_supply,
    62
);

proof_case!(
    astra_utilization_ceil_supply_063,
    astra_utilization_ceil_fixed_supply,
    63
);

proof_case!(
    astra_utilization_ceil_supply_064,
    astra_utilization_ceil_fixed_supply,
    64
);

proof_case!(
    astra_utilization_ceil_supply_065,
    astra_utilization_ceil_fixed_supply,
    65
);

proof_case!(
    astra_utilization_ceil_supply_066,
    astra_utilization_ceil_fixed_supply,
    66
);

proof_case!(
    astra_utilization_ceil_supply_067,
    astra_utilization_ceil_fixed_supply,
    67
);

proof_case!(
    astra_utilization_ceil_supply_068,
    astra_utilization_ceil_fixed_supply,
    68
);

proof_case!(
    astra_utilization_ceil_supply_069,
    astra_utilization_ceil_fixed_supply,
    69
);

proof_case!(
    astra_utilization_ceil_supply_070,
    astra_utilization_ceil_fixed_supply,
    70
);

proof_case!(
    astra_utilization_ceil_supply_071,
    astra_utilization_ceil_fixed_supply,
    71
);

proof_case!(
    astra_utilization_ceil_supply_072,
    astra_utilization_ceil_fixed_supply,
    72
);

proof_case!(
    astra_utilization_ceil_supply_073,
    astra_utilization_ceil_fixed_supply,
    73
);

proof_case!(
    astra_utilization_ceil_supply_074,
    astra_utilization_ceil_fixed_supply,
    74
);

proof_case!(
    astra_utilization_ceil_supply_075,
    astra_utilization_ceil_fixed_supply,
    75
);

proof_case!(
    astra_utilization_ceil_supply_076,
    astra_utilization_ceil_fixed_supply,
    76
);

proof_case!(
    astra_utilization_ceil_supply_077,
    astra_utilization_ceil_fixed_supply,
    77
);

proof_case!(
    astra_utilization_ceil_supply_078,
    astra_utilization_ceil_fixed_supply,
    78
);

proof_case!(
    astra_utilization_ceil_supply_079,
    astra_utilization_ceil_fixed_supply,
    79
);

proof_case!(
    astra_utilization_ceil_supply_080,
    astra_utilization_ceil_fixed_supply,
    80
);

proof_case!(
    astra_utilization_ceil_supply_081,
    astra_utilization_ceil_fixed_supply,
    81
);

proof_case!(
    astra_utilization_ceil_supply_082,
    astra_utilization_ceil_fixed_supply,
    82
);

proof_case!(
    astra_utilization_ceil_supply_083,
    astra_utilization_ceil_fixed_supply,
    83
);

proof_case!(
    astra_utilization_ceil_supply_084,
    astra_utilization_ceil_fixed_supply,
    84
);

proof_case!(
    astra_utilization_ceil_supply_085,
    astra_utilization_ceil_fixed_supply,
    85
);

proof_case!(
    astra_utilization_ceil_supply_086,
    astra_utilization_ceil_fixed_supply,
    86
);

proof_case!(
    astra_utilization_ceil_supply_087,
    astra_utilization_ceil_fixed_supply,
    87
);

proof_case!(
    astra_utilization_ceil_supply_088,
    astra_utilization_ceil_fixed_supply,
    88
);

proof_case!(
    astra_utilization_ceil_supply_089,
    astra_utilization_ceil_fixed_supply,
    89
);

proof_case!(
    astra_utilization_ceil_supply_090,
    astra_utilization_ceil_fixed_supply,
    90
);

proof_case!(
    astra_utilization_ceil_supply_091,
    astra_utilization_ceil_fixed_supply,
    91
);

proof_case!(
    astra_utilization_ceil_supply_092,
    astra_utilization_ceil_fixed_supply,
    92
);

proof_case!(
    astra_utilization_ceil_supply_093,
    astra_utilization_ceil_fixed_supply,
    93
);

proof_case!(
    astra_utilization_ceil_supply_094,
    astra_utilization_ceil_fixed_supply,
    94
);

proof_case!(
    astra_utilization_ceil_supply_095,
    astra_utilization_ceil_fixed_supply,
    95
);

proof_case!(
    astra_utilization_ceil_supply_096,
    astra_utilization_ceil_fixed_supply,
    96
);

proof_case!(
    astra_utilization_ceil_supply_097,
    astra_utilization_ceil_fixed_supply,
    97
);

proof_case!(
    astra_utilization_ceil_supply_098,
    astra_utilization_ceil_fixed_supply,
    98
);

proof_case!(
    astra_utilization_ceil_supply_099,
    astra_utilization_ceil_fixed_supply,
    99
);

proof_case!(
    astra_utilization_ceil_supply_100,
    astra_utilization_ceil_fixed_supply,
    100
);

proof_case!(
    astra_utilization_ceil_supply_101,
    astra_utilization_ceil_fixed_supply,
    101
);

proof_case!(
    astra_utilization_ceil_supply_102,
    astra_utilization_ceil_fixed_supply,
    102
);

proof_case!(
    astra_utilization_ceil_supply_103,
    astra_utilization_ceil_fixed_supply,
    103
);

proof_case!(
    astra_utilization_ceil_supply_104,
    astra_utilization_ceil_fixed_supply,
    104
);

proof_case!(
    astra_utilization_ceil_supply_105,
    astra_utilization_ceil_fixed_supply,
    105
);

proof_case!(
    astra_utilization_ceil_supply_106,
    astra_utilization_ceil_fixed_supply,
    106
);

proof_case!(
    astra_utilization_ceil_supply_107,
    astra_utilization_ceil_fixed_supply,
    107
);

proof_case!(
    astra_utilization_ceil_supply_108,
    astra_utilization_ceil_fixed_supply,
    108
);

proof_case!(
    astra_utilization_ceil_supply_109,
    astra_utilization_ceil_fixed_supply,
    109
);

proof_case!(
    astra_utilization_ceil_supply_110,
    astra_utilization_ceil_fixed_supply,
    110
);

proof_case!(
    astra_utilization_ceil_supply_111,
    astra_utilization_ceil_fixed_supply,
    111
);

proof_case!(
    astra_utilization_ceil_supply_112,
    astra_utilization_ceil_fixed_supply,
    112
);

proof_case!(
    astra_utilization_ceil_supply_113,
    astra_utilization_ceil_fixed_supply,
    113
);

proof_case!(
    astra_utilization_ceil_supply_114,
    astra_utilization_ceil_fixed_supply,
    114
);

proof_case!(
    astra_utilization_ceil_supply_115,
    astra_utilization_ceil_fixed_supply,
    115
);

proof_case!(
    astra_utilization_ceil_supply_116,
    astra_utilization_ceil_fixed_supply,
    116
);

proof_case!(
    astra_utilization_ceil_supply_117,
    astra_utilization_ceil_fixed_supply,
    117
);

proof_case!(
    astra_utilization_ceil_supply_118,
    astra_utilization_ceil_fixed_supply,
    118
);

proof_case!(
    astra_utilization_ceil_supply_119,
    astra_utilization_ceil_fixed_supply,
    119
);

proof_case!(
    astra_utilization_ceil_supply_120,
    astra_utilization_ceil_fixed_supply,
    120
);

proof_case!(
    astra_utilization_ceil_supply_121,
    astra_utilization_ceil_fixed_supply,
    121
);

proof_case!(
    astra_utilization_ceil_supply_122,
    astra_utilization_ceil_fixed_supply,
    122
);

proof_case!(
    astra_utilization_ceil_supply_123,
    astra_utilization_ceil_fixed_supply,
    123
);

proof_case!(
    astra_utilization_ceil_supply_124,
    astra_utilization_ceil_fixed_supply,
    124
);

proof_case!(
    astra_utilization_ceil_supply_125,
    astra_utilization_ceil_fixed_supply,
    125
);

proof_case!(
    astra_utilization_ceil_supply_126,
    astra_utilization_ceil_fixed_supply,
    126
);

proof_case!(
    astra_utilization_ceil_supply_127,
    astra_utilization_ceil_fixed_supply,
    127
);

proof_case!(
    astra_utilization_ceil_supply_128,
    astra_utilization_ceil_fixed_supply,
    128
);

proof_case!(
    astra_utilization_ceil_supply_129,
    astra_utilization_ceil_fixed_supply,
    129
);

proof_case!(
    astra_utilization_ceil_supply_130,
    astra_utilization_ceil_fixed_supply,
    130
);

proof_case!(
    astra_utilization_ceil_supply_131,
    astra_utilization_ceil_fixed_supply,
    131
);

proof_case!(
    astra_utilization_ceil_supply_132,
    astra_utilization_ceil_fixed_supply,
    132
);

proof_case!(
    astra_utilization_ceil_supply_133,
    astra_utilization_ceil_fixed_supply,
    133
);

proof_case!(
    astra_utilization_ceil_supply_134,
    astra_utilization_ceil_fixed_supply,
    134
);

proof_case!(
    astra_utilization_ceil_supply_135,
    astra_utilization_ceil_fixed_supply,
    135
);

proof_case!(
    astra_utilization_ceil_supply_136,
    astra_utilization_ceil_fixed_supply,
    136
);

proof_case!(
    astra_utilization_ceil_supply_137,
    astra_utilization_ceil_fixed_supply,
    137
);

proof_case!(
    astra_utilization_ceil_supply_138,
    astra_utilization_ceil_fixed_supply,
    138
);

proof_case!(
    astra_utilization_ceil_supply_139,
    astra_utilization_ceil_fixed_supply,
    139
);

proof_case!(
    astra_utilization_ceil_supply_140,
    astra_utilization_ceil_fixed_supply,
    140
);

proof_case!(
    astra_utilization_ceil_supply_141,
    astra_utilization_ceil_fixed_supply,
    141
);

proof_case!(
    astra_utilization_ceil_supply_142,
    astra_utilization_ceil_fixed_supply,
    142
);

proof_case!(
    astra_utilization_ceil_supply_143,
    astra_utilization_ceil_fixed_supply,
    143
);

proof_case!(
    astra_utilization_ceil_supply_144,
    astra_utilization_ceil_fixed_supply,
    144
);

proof_case!(
    astra_utilization_ceil_supply_145,
    astra_utilization_ceil_fixed_supply,
    145
);

proof_case!(
    astra_utilization_ceil_supply_146,
    astra_utilization_ceil_fixed_supply,
    146
);

proof_case!(
    astra_utilization_ceil_supply_147,
    astra_utilization_ceil_fixed_supply,
    147
);

proof_case!(
    astra_utilization_ceil_supply_148,
    astra_utilization_ceil_fixed_supply,
    148
);

proof_case!(
    astra_utilization_ceil_supply_149,
    astra_utilization_ceil_fixed_supply,
    149
);

proof_case!(
    astra_utilization_ceil_supply_150,
    astra_utilization_ceil_fixed_supply,
    150
);

proof_case!(
    astra_utilization_ceil_supply_151,
    astra_utilization_ceil_fixed_supply,
    151
);

proof_case!(
    astra_utilization_ceil_supply_152,
    astra_utilization_ceil_fixed_supply,
    152
);

proof_case!(
    astra_utilization_ceil_supply_153,
    astra_utilization_ceil_fixed_supply,
    153
);

proof_case!(
    astra_utilization_ceil_supply_154,
    astra_utilization_ceil_fixed_supply,
    154
);

proof_case!(
    astra_utilization_ceil_supply_155,
    astra_utilization_ceil_fixed_supply,
    155
);

proof_case!(
    astra_utilization_ceil_supply_156,
    astra_utilization_ceil_fixed_supply,
    156
);

proof_case!(
    astra_utilization_ceil_supply_157,
    astra_utilization_ceil_fixed_supply,
    157
);

proof_case!(
    astra_utilization_ceil_supply_158,
    astra_utilization_ceil_fixed_supply,
    158
);

proof_case!(
    astra_utilization_ceil_supply_159,
    astra_utilization_ceil_fixed_supply,
    159
);

proof_case!(
    astra_utilization_ceil_supply_160,
    astra_utilization_ceil_fixed_supply,
    160
);

proof_case!(
    astra_utilization_ceil_supply_161,
    astra_utilization_ceil_fixed_supply,
    161
);

proof_case!(
    astra_utilization_ceil_supply_162,
    astra_utilization_ceil_fixed_supply,
    162
);

proof_case!(
    astra_utilization_ceil_supply_163,
    astra_utilization_ceil_fixed_supply,
    163
);

proof_case!(
    astra_utilization_ceil_supply_164,
    astra_utilization_ceil_fixed_supply,
    164
);

proof_case!(
    astra_utilization_ceil_supply_165,
    astra_utilization_ceil_fixed_supply,
    165
);

proof_case!(
    astra_utilization_ceil_supply_166,
    astra_utilization_ceil_fixed_supply,
    166
);

proof_case!(
    astra_utilization_ceil_supply_167,
    astra_utilization_ceil_fixed_supply,
    167
);

proof_case!(
    astra_utilization_ceil_supply_168,
    astra_utilization_ceil_fixed_supply,
    168
);

proof_case!(
    astra_utilization_ceil_supply_169,
    astra_utilization_ceil_fixed_supply,
    169
);

proof_case!(
    astra_utilization_ceil_supply_170,
    astra_utilization_ceil_fixed_supply,
    170
);

proof_case!(
    astra_utilization_ceil_supply_171,
    astra_utilization_ceil_fixed_supply,
    171
);

proof_case!(
    astra_utilization_ceil_supply_172,
    astra_utilization_ceil_fixed_supply,
    172
);

proof_case!(
    astra_utilization_ceil_supply_173,
    astra_utilization_ceil_fixed_supply,
    173
);

proof_case!(
    astra_utilization_ceil_supply_174,
    astra_utilization_ceil_fixed_supply,
    174
);

proof_case!(
    astra_utilization_ceil_supply_175,
    astra_utilization_ceil_fixed_supply,
    175
);

proof_case!(
    astra_utilization_ceil_supply_176,
    astra_utilization_ceil_fixed_supply,
    176
);

proof_case!(
    astra_utilization_ceil_supply_177,
    astra_utilization_ceil_fixed_supply,
    177
);

proof_case!(
    astra_utilization_ceil_supply_178,
    astra_utilization_ceil_fixed_supply,
    178
);

proof_case!(
    astra_utilization_ceil_supply_179,
    astra_utilization_ceil_fixed_supply,
    179
);

proof_case!(
    astra_utilization_ceil_supply_180,
    astra_utilization_ceil_fixed_supply,
    180
);

proof_case!(
    astra_utilization_ceil_supply_181,
    astra_utilization_ceil_fixed_supply,
    181
);

proof_case!(
    astra_utilization_ceil_supply_182,
    astra_utilization_ceil_fixed_supply,
    182
);

proof_case!(
    astra_utilization_ceil_supply_183,
    astra_utilization_ceil_fixed_supply,
    183
);

proof_case!(
    astra_utilization_ceil_supply_184,
    astra_utilization_ceil_fixed_supply,
    184
);

proof_case!(
    astra_utilization_ceil_supply_185,
    astra_utilization_ceil_fixed_supply,
    185
);

proof_case!(
    astra_utilization_ceil_supply_186,
    astra_utilization_ceil_fixed_supply,
    186
);

proof_case!(
    astra_utilization_ceil_supply_187,
    astra_utilization_ceil_fixed_supply,
    187
);

proof_case!(
    astra_utilization_ceil_supply_188,
    astra_utilization_ceil_fixed_supply,
    188
);

proof_case!(
    astra_utilization_ceil_supply_189,
    astra_utilization_ceil_fixed_supply,
    189
);

proof_case!(
    astra_utilization_ceil_supply_190,
    astra_utilization_ceil_fixed_supply,
    190
);

proof_case!(
    astra_utilization_ceil_supply_191,
    astra_utilization_ceil_fixed_supply,
    191
);

proof_case!(
    astra_utilization_ceil_supply_192,
    astra_utilization_ceil_fixed_supply,
    192
);

proof_case!(
    astra_utilization_ceil_supply_193,
    astra_utilization_ceil_fixed_supply,
    193
);

proof_case!(
    astra_utilization_ceil_supply_194,
    astra_utilization_ceil_fixed_supply,
    194
);

proof_case!(
    astra_utilization_ceil_supply_195,
    astra_utilization_ceil_fixed_supply,
    195
);

proof_case!(
    astra_utilization_ceil_supply_196,
    astra_utilization_ceil_fixed_supply,
    196
);

proof_case!(
    astra_utilization_ceil_supply_197,
    astra_utilization_ceil_fixed_supply,
    197
);

proof_case!(
    astra_utilization_ceil_supply_198,
    astra_utilization_ceil_fixed_supply,
    198
);

proof_case!(
    astra_utilization_ceil_supply_199,
    astra_utilization_ceil_fixed_supply,
    199
);

proof_case!(
    astra_utilization_ceil_supply_200,
    astra_utilization_ceil_fixed_supply,
    200
);

proof_case!(
    astra_utilization_ceil_supply_201,
    astra_utilization_ceil_fixed_supply,
    201
);

proof_case!(
    astra_utilization_ceil_supply_202,
    astra_utilization_ceil_fixed_supply,
    202
);

proof_case!(
    astra_utilization_ceil_supply_203,
    astra_utilization_ceil_fixed_supply,
    203
);

proof_case!(
    astra_utilization_ceil_supply_204,
    astra_utilization_ceil_fixed_supply,
    204
);

proof_case!(
    astra_utilization_ceil_supply_205,
    astra_utilization_ceil_fixed_supply,
    205
);

proof_case!(
    astra_utilization_ceil_supply_206,
    astra_utilization_ceil_fixed_supply,
    206
);

proof_case!(
    astra_utilization_ceil_supply_207,
    astra_utilization_ceil_fixed_supply,
    207
);

proof_case!(
    astra_utilization_ceil_supply_208,
    astra_utilization_ceil_fixed_supply,
    208
);

proof_case!(
    astra_utilization_ceil_supply_209,
    astra_utilization_ceil_fixed_supply,
    209
);

proof_case!(
    astra_utilization_ceil_supply_210,
    astra_utilization_ceil_fixed_supply,
    210
);

proof_case!(
    astra_utilization_ceil_supply_211,
    astra_utilization_ceil_fixed_supply,
    211
);

proof_case!(
    astra_utilization_ceil_supply_212,
    astra_utilization_ceil_fixed_supply,
    212
);

proof_case!(
    astra_utilization_ceil_supply_213,
    astra_utilization_ceil_fixed_supply,
    213
);

proof_case!(
    astra_utilization_ceil_supply_214,
    astra_utilization_ceil_fixed_supply,
    214
);

proof_case!(
    astra_utilization_ceil_supply_215,
    astra_utilization_ceil_fixed_supply,
    215
);

proof_case!(
    astra_utilization_ceil_supply_216,
    astra_utilization_ceil_fixed_supply,
    216
);

proof_case!(
    astra_utilization_ceil_supply_217,
    astra_utilization_ceil_fixed_supply,
    217
);

proof_case!(
    astra_utilization_ceil_supply_218,
    astra_utilization_ceil_fixed_supply,
    218
);

proof_case!(
    astra_utilization_ceil_supply_219,
    astra_utilization_ceil_fixed_supply,
    219
);

proof_case!(
    astra_utilization_ceil_supply_220,
    astra_utilization_ceil_fixed_supply,
    220
);

proof_case!(
    astra_utilization_ceil_supply_221,
    astra_utilization_ceil_fixed_supply,
    221
);

proof_case!(
    astra_utilization_ceil_supply_222,
    astra_utilization_ceil_fixed_supply,
    222
);

proof_case!(
    astra_utilization_ceil_supply_223,
    astra_utilization_ceil_fixed_supply,
    223
);

proof_case!(
    astra_utilization_ceil_supply_224,
    astra_utilization_ceil_fixed_supply,
    224
);

proof_case!(
    astra_utilization_ceil_supply_225,
    astra_utilization_ceil_fixed_supply,
    225
);

proof_case!(
    astra_utilization_ceil_supply_226,
    astra_utilization_ceil_fixed_supply,
    226
);

proof_case!(
    astra_utilization_ceil_supply_227,
    astra_utilization_ceil_fixed_supply,
    227
);

proof_case!(
    astra_utilization_ceil_supply_228,
    astra_utilization_ceil_fixed_supply,
    228
);

proof_case!(
    astra_utilization_ceil_supply_229,
    astra_utilization_ceil_fixed_supply,
    229
);

proof_case!(
    astra_utilization_ceil_supply_230,
    astra_utilization_ceil_fixed_supply,
    230
);

proof_case!(
    astra_utilization_ceil_supply_231,
    astra_utilization_ceil_fixed_supply,
    231
);

proof_case!(
    astra_utilization_ceil_supply_232,
    astra_utilization_ceil_fixed_supply,
    232
);

proof_case!(
    astra_utilization_ceil_supply_233,
    astra_utilization_ceil_fixed_supply,
    233
);

proof_case!(
    astra_utilization_ceil_supply_234,
    astra_utilization_ceil_fixed_supply,
    234
);

proof_case!(
    astra_utilization_ceil_supply_235,
    astra_utilization_ceil_fixed_supply,
    235
);

proof_case!(
    astra_utilization_ceil_supply_236,
    astra_utilization_ceil_fixed_supply,
    236
);

proof_case!(
    astra_utilization_ceil_supply_237,
    astra_utilization_ceil_fixed_supply,
    237
);

proof_case!(
    astra_utilization_ceil_supply_238,
    astra_utilization_ceil_fixed_supply,
    238
);

proof_case!(
    astra_utilization_ceil_supply_239,
    astra_utilization_ceil_fixed_supply,
    239
);

proof_case!(
    astra_utilization_ceil_supply_240,
    astra_utilization_ceil_fixed_supply,
    240
);

proof_case!(
    astra_utilization_ceil_supply_241,
    astra_utilization_ceil_fixed_supply,
    241
);

proof_case!(
    astra_utilization_ceil_supply_242,
    astra_utilization_ceil_fixed_supply,
    242
);

proof_case!(
    astra_utilization_ceil_supply_243,
    astra_utilization_ceil_fixed_supply,
    243
);

proof_case!(
    astra_utilization_ceil_supply_244,
    astra_utilization_ceil_fixed_supply,
    244
);

proof_case!(
    astra_utilization_ceil_supply_245,
    astra_utilization_ceil_fixed_supply,
    245
);

proof_case!(
    astra_utilization_ceil_supply_246,
    astra_utilization_ceil_fixed_supply,
    246
);

proof_case!(
    astra_utilization_ceil_supply_247,
    astra_utilization_ceil_fixed_supply,
    247
);

proof_case!(
    astra_utilization_ceil_supply_248,
    astra_utilization_ceil_fixed_supply,
    248
);

proof_case!(
    astra_utilization_ceil_supply_249,
    astra_utilization_ceil_fixed_supply,
    249
);

proof_case!(
    astra_utilization_ceil_supply_250,
    astra_utilization_ceil_fixed_supply,
    250
);

proof_case!(
    astra_utilization_ceil_supply_251,
    astra_utilization_ceil_fixed_supply,
    251
);

proof_case!(
    astra_utilization_ceil_supply_252,
    astra_utilization_ceil_fixed_supply,
    252
);

proof_case!(
    astra_utilization_ceil_supply_253,
    astra_utilization_ceil_fixed_supply,
    253
);

proof_case!(
    astra_utilization_ceil_supply_254,
    astra_utilization_ceil_fixed_supply,
    254
);

proof_case!(
    astra_utilization_ceil_supply_255,
    astra_utilization_ceil_fixed_supply,
    255
);

proof_case!(
    astra_utilization_ceil_supply_256,
    astra_utilization_ceil_fixed_supply,
    256
);

proof_case!(
    astra_utilization_ceil_supply_257,
    astra_utilization_ceil_fixed_supply,
    257
);

proof_case!(
    astra_utilization_ceil_supply_258,
    astra_utilization_ceil_fixed_supply,
    258
);

proof_case!(
    astra_utilization_ceil_supply_259,
    astra_utilization_ceil_fixed_supply,
    259
);

proof_case!(
    astra_utilization_ceil_supply_260,
    astra_utilization_ceil_fixed_supply,
    260
);

proof_case!(
    astra_utilization_ceil_supply_261,
    astra_utilization_ceil_fixed_supply,
    261
);

proof_case!(
    astra_utilization_ceil_supply_262,
    astra_utilization_ceil_fixed_supply,
    262
);

proof_case!(
    astra_utilization_ceil_supply_263,
    astra_utilization_ceil_fixed_supply,
    263
);

proof_case!(
    astra_utilization_ceil_supply_264,
    astra_utilization_ceil_fixed_supply,
    264
);

proof_case!(
    astra_utilization_ceil_supply_265,
    astra_utilization_ceil_fixed_supply,
    265
);

proof_case!(
    astra_utilization_ceil_supply_266,
    astra_utilization_ceil_fixed_supply,
    266
);

proof_case!(
    astra_utilization_ceil_supply_267,
    astra_utilization_ceil_fixed_supply,
    267
);

proof_case!(
    astra_utilization_ceil_supply_268,
    astra_utilization_ceil_fixed_supply,
    268
);

proof_case!(
    astra_utilization_ceil_supply_269,
    astra_utilization_ceil_fixed_supply,
    269
);

proof_case!(
    astra_utilization_ceil_supply_270,
    astra_utilization_ceil_fixed_supply,
    270
);

proof_case!(
    astra_utilization_ceil_supply_271,
    astra_utilization_ceil_fixed_supply,
    271
);

proof_case!(
    astra_utilization_ceil_supply_272,
    astra_utilization_ceil_fixed_supply,
    272
);

proof_case!(
    astra_utilization_ceil_supply_273,
    astra_utilization_ceil_fixed_supply,
    273
);

proof_case!(
    astra_utilization_ceil_supply_274,
    astra_utilization_ceil_fixed_supply,
    274
);

proof_case!(
    astra_utilization_ceil_supply_275,
    astra_utilization_ceil_fixed_supply,
    275
);

proof_case!(
    astra_utilization_ceil_supply_276,
    astra_utilization_ceil_fixed_supply,
    276
);

proof_case!(
    astra_utilization_ceil_supply_277,
    astra_utilization_ceil_fixed_supply,
    277
);

proof_case!(
    astra_utilization_ceil_supply_278,
    astra_utilization_ceil_fixed_supply,
    278
);

proof_case!(
    astra_utilization_ceil_supply_279,
    astra_utilization_ceil_fixed_supply,
    279
);

proof_case!(
    astra_utilization_ceil_supply_280,
    astra_utilization_ceil_fixed_supply,
    280
);

proof_case!(
    astra_utilization_ceil_supply_281,
    astra_utilization_ceil_fixed_supply,
    281
);

proof_case!(
    astra_utilization_ceil_supply_282,
    astra_utilization_ceil_fixed_supply,
    282
);

proof_case!(
    astra_utilization_ceil_supply_283,
    astra_utilization_ceil_fixed_supply,
    283
);

proof_case!(
    astra_utilization_ceil_supply_284,
    astra_utilization_ceil_fixed_supply,
    284
);

proof_case!(
    astra_utilization_ceil_supply_285,
    astra_utilization_ceil_fixed_supply,
    285
);

proof_case!(
    astra_utilization_ceil_supply_286,
    astra_utilization_ceil_fixed_supply,
    286
);

proof_case!(
    astra_utilization_ceil_supply_287,
    astra_utilization_ceil_fixed_supply,
    287
);

proof_case!(
    astra_utilization_ceil_supply_288,
    astra_utilization_ceil_fixed_supply,
    288
);

proof_case!(
    astra_utilization_ceil_supply_289,
    astra_utilization_ceil_fixed_supply,
    289
);

proof_case!(
    astra_utilization_ceil_supply_290,
    astra_utilization_ceil_fixed_supply,
    290
);

proof_case!(
    astra_utilization_ceil_supply_291,
    astra_utilization_ceil_fixed_supply,
    291
);

proof_case!(
    astra_utilization_ceil_supply_292,
    astra_utilization_ceil_fixed_supply,
    292
);

proof_case!(
    astra_utilization_ceil_supply_293,
    astra_utilization_ceil_fixed_supply,
    293
);

proof_case!(
    astra_utilization_ceil_supply_294,
    astra_utilization_ceil_fixed_supply,
    294
);

proof_case!(
    astra_utilization_ceil_supply_295,
    astra_utilization_ceil_fixed_supply,
    295
);

proof_case!(
    astra_utilization_ceil_supply_296,
    astra_utilization_ceil_fixed_supply,
    296
);

proof_case!(
    astra_utilization_ceil_supply_297,
    astra_utilization_ceil_fixed_supply,
    297
);

proof_case!(
    astra_utilization_ceil_supply_298,
    astra_utilization_ceil_fixed_supply,
    298
);

proof_case!(
    astra_utilization_ceil_supply_299,
    astra_utilization_ceil_fixed_supply,
    299
);

proof_case!(
    astra_utilization_ceil_supply_300,
    astra_utilization_ceil_fixed_supply,
    300
);

proof_case!(
    astra_utilization_ceil_supply_301,
    astra_utilization_ceil_fixed_supply,
    301
);

proof_case!(
    astra_utilization_ceil_supply_302,
    astra_utilization_ceil_fixed_supply,
    302
);

proof_case!(
    astra_utilization_ceil_supply_303,
    astra_utilization_ceil_fixed_supply,
    303
);

proof_case!(
    astra_utilization_ceil_supply_304,
    astra_utilization_ceil_fixed_supply,
    304
);

proof_case!(
    astra_utilization_ceil_supply_305,
    astra_utilization_ceil_fixed_supply,
    305
);

proof_case!(
    astra_utilization_ceil_supply_306,
    astra_utilization_ceil_fixed_supply,
    306
);

proof_case!(
    astra_utilization_ceil_supply_307,
    astra_utilization_ceil_fixed_supply,
    307
);

proof_case!(
    astra_utilization_ceil_supply_308,
    astra_utilization_ceil_fixed_supply,
    308
);

proof_case!(
    astra_utilization_ceil_supply_309,
    astra_utilization_ceil_fixed_supply,
    309
);

proof_case!(
    astra_utilization_ceil_supply_310,
    astra_utilization_ceil_fixed_supply,
    310
);

proof_case!(
    astra_utilization_ceil_supply_311,
    astra_utilization_ceil_fixed_supply,
    311
);

proof_case!(
    astra_utilization_ceil_supply_312,
    astra_utilization_ceil_fixed_supply,
    312
);

proof_case!(
    astra_utilization_ceil_supply_313,
    astra_utilization_ceil_fixed_supply,
    313
);

proof_case!(
    astra_utilization_ceil_supply_314,
    astra_utilization_ceil_fixed_supply,
    314
);

proof_case!(
    astra_utilization_ceil_supply_315,
    astra_utilization_ceil_fixed_supply,
    315
);

proof_case!(
    astra_utilization_ceil_supply_316,
    astra_utilization_ceil_fixed_supply,
    316
);

proof_case!(
    astra_utilization_ceil_supply_317,
    astra_utilization_ceil_fixed_supply,
    317
);

proof_case!(
    astra_utilization_ceil_supply_318,
    astra_utilization_ceil_fixed_supply,
    318
);

proof_case!(
    astra_utilization_ceil_supply_319,
    astra_utilization_ceil_fixed_supply,
    319
);

proof_case!(
    astra_utilization_ceil_supply_320,
    astra_utilization_ceil_fixed_supply,
    320
);

proof_case!(
    astra_utilization_ceil_supply_321,
    astra_utilization_ceil_fixed_supply,
    321
);

proof_case!(
    astra_utilization_ceil_supply_322,
    astra_utilization_ceil_fixed_supply,
    322
);

proof_case!(
    astra_utilization_ceil_supply_323,
    astra_utilization_ceil_fixed_supply,
    323
);

proof_case!(
    astra_utilization_ceil_supply_324,
    astra_utilization_ceil_fixed_supply,
    324
);

proof_case!(
    astra_utilization_ceil_supply_325,
    astra_utilization_ceil_fixed_supply,
    325
);

proof_case!(
    astra_utilization_ceil_supply_326,
    astra_utilization_ceil_fixed_supply,
    326
);

proof_case!(
    astra_utilization_ceil_supply_327,
    astra_utilization_ceil_fixed_supply,
    327
);

proof_case!(
    astra_utilization_ceil_supply_328,
    astra_utilization_ceil_fixed_supply,
    328
);

proof_case!(
    astra_utilization_ceil_supply_329,
    astra_utilization_ceil_fixed_supply,
    329
);

proof_case!(
    astra_utilization_ceil_supply_330,
    astra_utilization_ceil_fixed_supply,
    330
);

proof_case!(
    astra_utilization_ceil_supply_331,
    astra_utilization_ceil_fixed_supply,
    331
);

proof_case!(
    astra_utilization_ceil_supply_332,
    astra_utilization_ceil_fixed_supply,
    332
);

proof_case!(
    astra_utilization_ceil_supply_333,
    astra_utilization_ceil_fixed_supply,
    333
);

proof_case!(
    astra_utilization_ceil_supply_334,
    astra_utilization_ceil_fixed_supply,
    334
);

proof_case!(
    astra_utilization_ceil_supply_335,
    astra_utilization_ceil_fixed_supply,
    335
);

proof_case!(
    astra_utilization_ceil_supply_336,
    astra_utilization_ceil_fixed_supply,
    336
);

proof_case!(
    astra_utilization_ceil_supply_337,
    astra_utilization_ceil_fixed_supply,
    337
);

proof_case!(
    astra_utilization_ceil_supply_338,
    astra_utilization_ceil_fixed_supply,
    338
);

proof_case!(
    astra_utilization_ceil_supply_339,
    astra_utilization_ceil_fixed_supply,
    339
);

proof_case!(
    astra_utilization_ceil_supply_340,
    astra_utilization_ceil_fixed_supply,
    340
);

proof_case!(
    astra_utilization_ceil_supply_341,
    astra_utilization_ceil_fixed_supply,
    341
);

proof_case!(
    astra_utilization_ceil_supply_342,
    astra_utilization_ceil_fixed_supply,
    342
);

proof_case!(
    astra_utilization_ceil_supply_343,
    astra_utilization_ceil_fixed_supply,
    343
);

proof_case!(
    astra_utilization_ceil_supply_344,
    astra_utilization_ceil_fixed_supply,
    344
);

proof_case!(
    astra_utilization_ceil_supply_345,
    astra_utilization_ceil_fixed_supply,
    345
);

proof_case!(
    astra_utilization_ceil_supply_346,
    astra_utilization_ceil_fixed_supply,
    346
);

proof_case!(
    astra_utilization_ceil_supply_347,
    astra_utilization_ceil_fixed_supply,
    347
);

proof_case!(
    astra_utilization_ceil_supply_348,
    astra_utilization_ceil_fixed_supply,
    348
);

proof_case!(
    astra_utilization_ceil_supply_349,
    astra_utilization_ceil_fixed_supply,
    349
);

proof_case!(
    astra_utilization_ceil_supply_350,
    astra_utilization_ceil_fixed_supply,
    350
);

proof_case!(
    astra_utilization_ceil_supply_351,
    astra_utilization_ceil_fixed_supply,
    351
);

proof_case!(
    astra_utilization_ceil_supply_352,
    astra_utilization_ceil_fixed_supply,
    352
);

proof_case!(
    astra_utilization_ceil_supply_353,
    astra_utilization_ceil_fixed_supply,
    353
);

proof_case!(
    astra_utilization_ceil_supply_354,
    astra_utilization_ceil_fixed_supply,
    354
);

proof_case!(
    astra_utilization_ceil_supply_355,
    astra_utilization_ceil_fixed_supply,
    355
);

proof_case!(
    astra_utilization_ceil_supply_356,
    astra_utilization_ceil_fixed_supply,
    356
);

proof_case!(
    astra_utilization_ceil_supply_357,
    astra_utilization_ceil_fixed_supply,
    357
);

proof_case!(
    astra_utilization_ceil_supply_358,
    astra_utilization_ceil_fixed_supply,
    358
);

proof_case!(
    astra_utilization_ceil_supply_359,
    astra_utilization_ceil_fixed_supply,
    359
);

proof_case!(
    astra_utilization_ceil_supply_360,
    astra_utilization_ceil_fixed_supply,
    360
);

proof_case!(
    astra_utilization_ceil_supply_361,
    astra_utilization_ceil_fixed_supply,
    361
);

proof_case!(
    astra_utilization_ceil_supply_362,
    astra_utilization_ceil_fixed_supply,
    362
);

proof_case!(
    astra_utilization_ceil_supply_363,
    astra_utilization_ceil_fixed_supply,
    363
);

proof_case!(
    astra_utilization_ceil_supply_364,
    astra_utilization_ceil_fixed_supply,
    364
);

proof_case!(
    astra_utilization_ceil_supply_365,
    astra_utilization_ceil_fixed_supply,
    365
);

proof_case!(
    astra_utilization_ceil_supply_366,
    astra_utilization_ceil_fixed_supply,
    366
);

proof_case!(
    astra_utilization_ceil_supply_367,
    astra_utilization_ceil_fixed_supply,
    367
);

proof_case!(
    astra_utilization_ceil_supply_368,
    astra_utilization_ceil_fixed_supply,
    368
);

proof_case!(
    astra_utilization_ceil_supply_369,
    astra_utilization_ceil_fixed_supply,
    369
);

proof_case!(
    astra_utilization_ceil_supply_370,
    astra_utilization_ceil_fixed_supply,
    370
);

proof_case!(
    astra_utilization_ceil_supply_371,
    astra_utilization_ceil_fixed_supply,
    371
);

proof_case!(
    astra_utilization_ceil_supply_372,
    astra_utilization_ceil_fixed_supply,
    372
);

proof_case!(
    astra_utilization_ceil_supply_373,
    astra_utilization_ceil_fixed_supply,
    373
);

proof_case!(
    astra_utilization_ceil_supply_374,
    astra_utilization_ceil_fixed_supply,
    374
);

proof_case!(
    astra_utilization_ceil_supply_375,
    astra_utilization_ceil_fixed_supply,
    375
);

proof_case!(
    astra_utilization_ceil_supply_376,
    astra_utilization_ceil_fixed_supply,
    376
);

proof_case!(
    astra_utilization_ceil_supply_377,
    astra_utilization_ceil_fixed_supply,
    377
);

proof_case!(
    astra_utilization_ceil_supply_378,
    astra_utilization_ceil_fixed_supply,
    378
);

proof_case!(
    astra_utilization_ceil_supply_379,
    astra_utilization_ceil_fixed_supply,
    379
);

proof_case!(
    astra_utilization_ceil_supply_380,
    astra_utilization_ceil_fixed_supply,
    380
);

proof_case!(
    astra_utilization_ceil_supply_381,
    astra_utilization_ceil_fixed_supply,
    381
);

proof_case!(
    astra_utilization_ceil_supply_382,
    astra_utilization_ceil_fixed_supply,
    382
);

proof_case!(
    astra_utilization_ceil_supply_383,
    astra_utilization_ceil_fixed_supply,
    383
);

proof_case!(
    astra_utilization_ceil_supply_384,
    astra_utilization_ceil_fixed_supply,
    384
);

proof_case!(
    astra_utilization_ceil_supply_385,
    astra_utilization_ceil_fixed_supply,
    385
);

proof_case!(
    astra_utilization_ceil_supply_386,
    astra_utilization_ceil_fixed_supply,
    386
);

proof_case!(
    astra_utilization_ceil_supply_387,
    astra_utilization_ceil_fixed_supply,
    387
);

proof_case!(
    astra_utilization_ceil_supply_388,
    astra_utilization_ceil_fixed_supply,
    388
);

proof_case!(
    astra_utilization_ceil_supply_389,
    astra_utilization_ceil_fixed_supply,
    389
);

proof_case!(
    astra_utilization_ceil_supply_390,
    astra_utilization_ceil_fixed_supply,
    390
);

proof_case!(
    astra_utilization_ceil_supply_391,
    astra_utilization_ceil_fixed_supply,
    391
);

proof_case!(
    astra_utilization_ceil_supply_392,
    astra_utilization_ceil_fixed_supply,
    392
);

proof_case!(
    astra_utilization_ceil_supply_393,
    astra_utilization_ceil_fixed_supply,
    393
);

proof_case!(
    astra_utilization_ceil_supply_394,
    astra_utilization_ceil_fixed_supply,
    394
);

proof_case!(
    astra_utilization_ceil_supply_395,
    astra_utilization_ceil_fixed_supply,
    395
);

proof_case!(
    astra_utilization_ceil_supply_396,
    astra_utilization_ceil_fixed_supply,
    396
);

proof_case!(
    astra_utilization_ceil_supply_397,
    astra_utilization_ceil_fixed_supply,
    397
);

proof_case!(
    astra_utilization_ceil_supply_398,
    astra_utilization_ceil_fixed_supply,
    398
);

proof_case!(
    astra_utilization_ceil_supply_399,
    astra_utilization_ceil_fixed_supply,
    399
);

proof_case!(
    astra_utilization_ceil_supply_400,
    astra_utilization_ceil_fixed_supply,
    400
);

proof_case!(
    astra_utilization_ceil_supply_401,
    astra_utilization_ceil_fixed_supply,
    401
);

proof_case!(
    astra_utilization_ceil_supply_402,
    astra_utilization_ceil_fixed_supply,
    402
);

proof_case!(
    astra_utilization_ceil_supply_403,
    astra_utilization_ceil_fixed_supply,
    403
);

proof_case!(
    astra_utilization_ceil_supply_404,
    astra_utilization_ceil_fixed_supply,
    404
);

proof_case!(
    astra_utilization_ceil_supply_405,
    astra_utilization_ceil_fixed_supply,
    405
);

proof_case!(
    astra_utilization_ceil_supply_406,
    astra_utilization_ceil_fixed_supply,
    406
);

proof_case!(
    astra_utilization_ceil_supply_407,
    astra_utilization_ceil_fixed_supply,
    407
);

proof_case!(
    astra_utilization_ceil_supply_408,
    astra_utilization_ceil_fixed_supply,
    408
);

proof_case!(
    astra_utilization_ceil_supply_409,
    astra_utilization_ceil_fixed_supply,
    409
);

proof_case!(
    astra_utilization_ceil_supply_410,
    astra_utilization_ceil_fixed_supply,
    410
);

proof_case!(
    astra_utilization_ceil_supply_411,
    astra_utilization_ceil_fixed_supply,
    411
);

proof_case!(
    astra_utilization_ceil_supply_412,
    astra_utilization_ceil_fixed_supply,
    412
);

proof_case!(
    astra_utilization_ceil_supply_413,
    astra_utilization_ceil_fixed_supply,
    413
);

proof_case!(
    astra_utilization_ceil_supply_414,
    astra_utilization_ceil_fixed_supply,
    414
);

proof_case!(
    astra_utilization_ceil_supply_415,
    astra_utilization_ceil_fixed_supply,
    415
);

proof_case!(
    astra_utilization_ceil_supply_416,
    astra_utilization_ceil_fixed_supply,
    416
);

proof_case!(
    astra_utilization_ceil_supply_417,
    astra_utilization_ceil_fixed_supply,
    417
);

proof_case!(
    astra_utilization_ceil_supply_418,
    astra_utilization_ceil_fixed_supply,
    418
);

proof_case!(
    astra_utilization_ceil_supply_419,
    astra_utilization_ceil_fixed_supply,
    419
);

proof_case!(
    astra_utilization_ceil_supply_420,
    astra_utilization_ceil_fixed_supply,
    420
);

proof_case!(
    astra_utilization_ceil_supply_421,
    astra_utilization_ceil_fixed_supply,
    421
);

proof_case!(
    astra_utilization_ceil_supply_422,
    astra_utilization_ceil_fixed_supply,
    422
);

proof_case!(
    astra_utilization_ceil_supply_423,
    astra_utilization_ceil_fixed_supply,
    423
);

proof_case!(
    astra_utilization_ceil_supply_424,
    astra_utilization_ceil_fixed_supply,
    424
);

proof_case!(
    astra_utilization_ceil_supply_425,
    astra_utilization_ceil_fixed_supply,
    425
);

proof_case!(
    astra_utilization_ceil_supply_426,
    astra_utilization_ceil_fixed_supply,
    426
);

proof_case!(
    astra_utilization_ceil_supply_427,
    astra_utilization_ceil_fixed_supply,
    427
);

proof_case!(
    astra_utilization_ceil_supply_428,
    astra_utilization_ceil_fixed_supply,
    428
);

proof_case!(
    astra_utilization_ceil_supply_429,
    astra_utilization_ceil_fixed_supply,
    429
);

proof_case!(
    astra_utilization_ceil_supply_430,
    astra_utilization_ceil_fixed_supply,
    430
);

proof_case!(
    astra_utilization_ceil_supply_431,
    astra_utilization_ceil_fixed_supply,
    431
);

proof_case!(
    astra_utilization_ceil_supply_432,
    astra_utilization_ceil_fixed_supply,
    432
);

proof_case!(
    astra_utilization_ceil_supply_433,
    astra_utilization_ceil_fixed_supply,
    433
);

proof_case!(
    astra_utilization_ceil_supply_434,
    astra_utilization_ceil_fixed_supply,
    434
);

proof_case!(
    astra_utilization_ceil_supply_435,
    astra_utilization_ceil_fixed_supply,
    435
);

proof_case!(
    astra_utilization_ceil_supply_436,
    astra_utilization_ceil_fixed_supply,
    436
);

proof_case!(
    astra_utilization_ceil_supply_437,
    astra_utilization_ceil_fixed_supply,
    437
);

proof_case!(
    astra_utilization_ceil_supply_438,
    astra_utilization_ceil_fixed_supply,
    438
);

proof_case!(
    astra_utilization_ceil_supply_439,
    astra_utilization_ceil_fixed_supply,
    439
);

proof_case!(
    astra_utilization_ceil_supply_440,
    astra_utilization_ceil_fixed_supply,
    440
);

proof_case!(
    astra_utilization_ceil_supply_441,
    astra_utilization_ceil_fixed_supply,
    441
);

proof_case!(
    astra_utilization_ceil_supply_442,
    astra_utilization_ceil_fixed_supply,
    442
);

proof_case!(
    astra_utilization_ceil_supply_443,
    astra_utilization_ceil_fixed_supply,
    443
);

proof_case!(
    astra_utilization_ceil_supply_444,
    astra_utilization_ceil_fixed_supply,
    444
);

proof_case!(
    astra_utilization_ceil_supply_445,
    astra_utilization_ceil_fixed_supply,
    445
);

proof_case!(
    astra_utilization_ceil_supply_446,
    astra_utilization_ceil_fixed_supply,
    446
);

proof_case!(
    astra_utilization_ceil_supply_447,
    astra_utilization_ceil_fixed_supply,
    447
);

proof_case!(
    astra_utilization_ceil_supply_448,
    astra_utilization_ceil_fixed_supply,
    448
);

proof_case!(
    astra_utilization_ceil_supply_449,
    astra_utilization_ceil_fixed_supply,
    449
);

proof_case!(
    astra_utilization_ceil_supply_450,
    astra_utilization_ceil_fixed_supply,
    450
);

proof_case!(
    astra_utilization_ceil_supply_451,
    astra_utilization_ceil_fixed_supply,
    451
);

proof_case!(
    astra_utilization_ceil_supply_452,
    astra_utilization_ceil_fixed_supply,
    452
);

proof_case!(
    astra_utilization_ceil_supply_453,
    astra_utilization_ceil_fixed_supply,
    453
);

proof_case!(
    astra_utilization_ceil_supply_454,
    astra_utilization_ceil_fixed_supply,
    454
);

proof_case!(
    astra_utilization_ceil_supply_455,
    astra_utilization_ceil_fixed_supply,
    455
);

proof_case!(
    astra_utilization_ceil_supply_456,
    astra_utilization_ceil_fixed_supply,
    456
);

proof_case!(
    astra_utilization_ceil_supply_457,
    astra_utilization_ceil_fixed_supply,
    457
);

proof_case!(
    astra_utilization_ceil_supply_458,
    astra_utilization_ceil_fixed_supply,
    458
);

proof_case!(
    astra_utilization_ceil_supply_459,
    astra_utilization_ceil_fixed_supply,
    459
);

proof_case!(
    astra_utilization_ceil_supply_460,
    astra_utilization_ceil_fixed_supply,
    460
);

proof_case!(
    astra_utilization_ceil_supply_461,
    astra_utilization_ceil_fixed_supply,
    461
);

proof_case!(
    astra_utilization_ceil_supply_462,
    astra_utilization_ceil_fixed_supply,
    462
);

proof_case!(
    astra_utilization_ceil_supply_463,
    astra_utilization_ceil_fixed_supply,
    463
);

proof_case!(
    astra_utilization_ceil_supply_464,
    astra_utilization_ceil_fixed_supply,
    464
);

proof_case!(
    astra_utilization_ceil_supply_465,
    astra_utilization_ceil_fixed_supply,
    465
);

proof_case!(
    astra_utilization_ceil_supply_466,
    astra_utilization_ceil_fixed_supply,
    466
);

proof_case!(
    astra_utilization_ceil_supply_467,
    astra_utilization_ceil_fixed_supply,
    467
);

proof_case!(
    astra_utilization_ceil_supply_468,
    astra_utilization_ceil_fixed_supply,
    468
);

proof_case!(
    astra_utilization_ceil_supply_469,
    astra_utilization_ceil_fixed_supply,
    469
);

proof_case!(
    astra_utilization_ceil_supply_470,
    astra_utilization_ceil_fixed_supply,
    470
);

proof_case!(
    astra_utilization_ceil_supply_471,
    astra_utilization_ceil_fixed_supply,
    471
);

proof_case!(
    astra_utilization_ceil_supply_472,
    astra_utilization_ceil_fixed_supply,
    472
);

proof_case!(
    astra_utilization_ceil_supply_473,
    astra_utilization_ceil_fixed_supply,
    473
);

proof_case!(
    astra_utilization_ceil_supply_474,
    astra_utilization_ceil_fixed_supply,
    474
);

proof_case!(
    astra_utilization_ceil_supply_475,
    astra_utilization_ceil_fixed_supply,
    475
);

proof_case!(
    astra_utilization_ceil_supply_476,
    astra_utilization_ceil_fixed_supply,
    476
);

proof_case!(
    astra_utilization_ceil_supply_477,
    astra_utilization_ceil_fixed_supply,
    477
);

proof_case!(
    astra_utilization_ceil_supply_478,
    astra_utilization_ceil_fixed_supply,
    478
);

proof_case!(
    astra_utilization_ceil_supply_479,
    astra_utilization_ceil_fixed_supply,
    479
);

proof_case!(
    astra_utilization_ceil_supply_480,
    astra_utilization_ceil_fixed_supply,
    480
);

proof_case!(
    astra_utilization_ceil_supply_481,
    astra_utilization_ceil_fixed_supply,
    481
);

proof_case!(
    astra_utilization_ceil_supply_482,
    astra_utilization_ceil_fixed_supply,
    482
);

proof_case!(
    astra_utilization_ceil_supply_483,
    astra_utilization_ceil_fixed_supply,
    483
);

proof_case!(
    astra_utilization_ceil_supply_484,
    astra_utilization_ceil_fixed_supply,
    484
);

proof_case!(
    astra_utilization_ceil_supply_485,
    astra_utilization_ceil_fixed_supply,
    485
);

proof_case!(
    astra_utilization_ceil_supply_486,
    astra_utilization_ceil_fixed_supply,
    486
);

proof_case!(
    astra_utilization_ceil_supply_487,
    astra_utilization_ceil_fixed_supply,
    487
);

proof_case!(
    astra_utilization_ceil_supply_488,
    astra_utilization_ceil_fixed_supply,
    488
);

proof_case!(
    astra_utilization_ceil_supply_489,
    astra_utilization_ceil_fixed_supply,
    489
);

proof_case!(
    astra_utilization_ceil_supply_490,
    astra_utilization_ceil_fixed_supply,
    490
);

proof_case!(
    astra_utilization_ceil_supply_491,
    astra_utilization_ceil_fixed_supply,
    491
);

proof_case!(
    astra_utilization_ceil_supply_492,
    astra_utilization_ceil_fixed_supply,
    492
);

proof_case!(
    astra_utilization_ceil_supply_493,
    astra_utilization_ceil_fixed_supply,
    493
);

proof_case!(
    astra_utilization_ceil_supply_494,
    astra_utilization_ceil_fixed_supply,
    494
);

proof_case!(
    astra_utilization_ceil_supply_495,
    astra_utilization_ceil_fixed_supply,
    495
);

proof_case!(
    astra_utilization_ceil_supply_496,
    astra_utilization_ceil_fixed_supply,
    496
);

proof_case!(
    astra_utilization_ceil_supply_497,
    astra_utilization_ceil_fixed_supply,
    497
);

proof_case!(
    astra_utilization_ceil_supply_498,
    astra_utilization_ceil_fixed_supply,
    498
);

proof_case!(
    astra_utilization_ceil_supply_499,
    astra_utilization_ceil_fixed_supply,
    499
);

proof_case!(
    astra_utilization_ceil_supply_500,
    astra_utilization_ceil_fixed_supply,
    500
);

proof_case!(
    astra_utilization_ceil_supply_501,
    astra_utilization_ceil_fixed_supply,
    501
);

proof_case!(
    astra_utilization_ceil_supply_502,
    astra_utilization_ceil_fixed_supply,
    502
);

proof_case!(
    astra_utilization_ceil_supply_503,
    astra_utilization_ceil_fixed_supply,
    503
);

proof_case!(
    astra_utilization_ceil_supply_504,
    astra_utilization_ceil_fixed_supply,
    504
);

proof_case!(
    astra_utilization_ceil_supply_505,
    astra_utilization_ceil_fixed_supply,
    505
);

proof_case!(
    astra_utilization_ceil_supply_506,
    astra_utilization_ceil_fixed_supply,
    506
);

proof_case!(
    astra_utilization_ceil_supply_507,
    astra_utilization_ceil_fixed_supply,
    507
);

proof_case!(
    astra_utilization_ceil_supply_508,
    astra_utilization_ceil_fixed_supply,
    508
);

proof_case!(
    astra_utilization_ceil_supply_509,
    astra_utilization_ceil_fixed_supply,
    509
);

proof_case!(
    astra_utilization_ceil_supply_510,
    astra_utilization_ceil_fixed_supply,
    510
);

proof_case!(
    astra_utilization_ceil_supply_511,
    astra_utilization_ceil_fixed_supply,
    511
);

proof_case!(
    astra_utilization_ceil_supply_512,
    astra_utilization_ceil_fixed_supply,
    512
);

proof_case!(
    astra_utilization_ceil_supply_513,
    astra_utilization_ceil_fixed_supply,
    513
);

proof_case!(
    astra_utilization_ceil_supply_514,
    astra_utilization_ceil_fixed_supply,
    514
);

proof_case!(
    astra_utilization_ceil_supply_515,
    astra_utilization_ceil_fixed_supply,
    515
);

proof_case!(
    astra_utilization_ceil_supply_516,
    astra_utilization_ceil_fixed_supply,
    516
);

proof_case!(
    astra_utilization_ceil_supply_517,
    astra_utilization_ceil_fixed_supply,
    517
);

proof_case!(
    astra_utilization_ceil_supply_518,
    astra_utilization_ceil_fixed_supply,
    518
);

proof_case!(
    astra_utilization_ceil_supply_519,
    astra_utilization_ceil_fixed_supply,
    519
);

proof_case!(
    astra_utilization_ceil_supply_520,
    astra_utilization_ceil_fixed_supply,
    520
);

proof_case!(
    astra_utilization_ceil_supply_521,
    astra_utilization_ceil_fixed_supply,
    521
);

proof_case!(
    astra_utilization_ceil_supply_522,
    astra_utilization_ceil_fixed_supply,
    522
);

proof_case!(
    astra_utilization_ceil_supply_523,
    astra_utilization_ceil_fixed_supply,
    523
);

proof_case!(
    astra_utilization_ceil_supply_524,
    astra_utilization_ceil_fixed_supply,
    524
);

proof_case!(
    astra_utilization_ceil_supply_525,
    astra_utilization_ceil_fixed_supply,
    525
);

proof_case!(
    astra_utilization_ceil_supply_526,
    astra_utilization_ceil_fixed_supply,
    526
);

proof_case!(
    astra_utilization_ceil_supply_527,
    astra_utilization_ceil_fixed_supply,
    527
);

proof_case!(
    astra_utilization_ceil_supply_528,
    astra_utilization_ceil_fixed_supply,
    528
);

proof_case!(
    astra_utilization_ceil_supply_529,
    astra_utilization_ceil_fixed_supply,
    529
);

proof_case!(
    astra_utilization_ceil_supply_530,
    astra_utilization_ceil_fixed_supply,
    530
);

proof_case!(
    astra_utilization_ceil_supply_531,
    astra_utilization_ceil_fixed_supply,
    531
);

proof_case!(
    astra_utilization_ceil_supply_532,
    astra_utilization_ceil_fixed_supply,
    532
);

proof_case!(
    astra_utilization_ceil_supply_533,
    astra_utilization_ceil_fixed_supply,
    533
);

proof_case!(
    astra_utilization_ceil_supply_534,
    astra_utilization_ceil_fixed_supply,
    534
);

proof_case!(
    astra_utilization_ceil_supply_535,
    astra_utilization_ceil_fixed_supply,
    535
);

proof_case!(
    astra_utilization_ceil_supply_536,
    astra_utilization_ceil_fixed_supply,
    536
);

proof_case!(
    astra_utilization_ceil_supply_537,
    astra_utilization_ceil_fixed_supply,
    537
);

proof_case!(
    astra_utilization_ceil_supply_538,
    astra_utilization_ceil_fixed_supply,
    538
);

proof_case!(
    astra_utilization_ceil_supply_539,
    astra_utilization_ceil_fixed_supply,
    539
);

proof_case!(
    astra_utilization_ceil_supply_540,
    astra_utilization_ceil_fixed_supply,
    540
);

proof_case!(
    astra_utilization_ceil_supply_541,
    astra_utilization_ceil_fixed_supply,
    541
);

proof_case!(
    astra_utilization_ceil_supply_542,
    astra_utilization_ceil_fixed_supply,
    542
);

proof_case!(
    astra_utilization_ceil_supply_543,
    astra_utilization_ceil_fixed_supply,
    543
);

proof_case!(
    astra_utilization_ceil_supply_544,
    astra_utilization_ceil_fixed_supply,
    544
);

proof_case!(
    astra_utilization_ceil_supply_545,
    astra_utilization_ceil_fixed_supply,
    545
);

proof_case!(
    astra_utilization_ceil_supply_546,
    astra_utilization_ceil_fixed_supply,
    546
);

proof_case!(
    astra_utilization_ceil_supply_547,
    astra_utilization_ceil_fixed_supply,
    547
);

proof_case!(
    astra_utilization_ceil_supply_548,
    astra_utilization_ceil_fixed_supply,
    548
);

proof_case!(
    astra_utilization_ceil_supply_549,
    astra_utilization_ceil_fixed_supply,
    549
);

proof_case!(
    astra_utilization_ceil_supply_550,
    astra_utilization_ceil_fixed_supply,
    550
);

proof_case!(
    astra_utilization_ceil_supply_551,
    astra_utilization_ceil_fixed_supply,
    551
);

proof_case!(
    astra_utilization_ceil_supply_552,
    astra_utilization_ceil_fixed_supply,
    552
);

proof_case!(
    astra_utilization_ceil_supply_553,
    astra_utilization_ceil_fixed_supply,
    553
);

proof_case!(
    astra_utilization_ceil_supply_554,
    astra_utilization_ceil_fixed_supply,
    554
);

proof_case!(
    astra_utilization_ceil_supply_555,
    astra_utilization_ceil_fixed_supply,
    555
);

proof_case!(
    astra_utilization_ceil_supply_556,
    astra_utilization_ceil_fixed_supply,
    556
);

proof_case!(
    astra_utilization_ceil_supply_557,
    astra_utilization_ceil_fixed_supply,
    557
);

proof_case!(
    astra_utilization_ceil_supply_558,
    astra_utilization_ceil_fixed_supply,
    558
);

proof_case!(
    astra_utilization_ceil_supply_559,
    astra_utilization_ceil_fixed_supply,
    559
);

proof_case!(
    astra_utilization_ceil_supply_560,
    astra_utilization_ceil_fixed_supply,
    560
);

proof_case!(
    astra_utilization_ceil_supply_561,
    astra_utilization_ceil_fixed_supply,
    561
);

proof_case!(
    astra_utilization_ceil_supply_562,
    astra_utilization_ceil_fixed_supply,
    562
);

proof_case!(
    astra_utilization_ceil_supply_563,
    astra_utilization_ceil_fixed_supply,
    563
);

proof_case!(
    astra_utilization_ceil_supply_564,
    astra_utilization_ceil_fixed_supply,
    564
);

proof_case!(
    astra_utilization_ceil_supply_565,
    astra_utilization_ceil_fixed_supply,
    565
);

proof_case!(
    astra_utilization_ceil_supply_566,
    astra_utilization_ceil_fixed_supply,
    566
);

proof_case!(
    astra_utilization_ceil_supply_567,
    astra_utilization_ceil_fixed_supply,
    567
);

proof_case!(
    astra_utilization_ceil_supply_568,
    astra_utilization_ceil_fixed_supply,
    568
);

proof_case!(
    astra_utilization_ceil_supply_569,
    astra_utilization_ceil_fixed_supply,
    569
);

proof_case!(
    astra_utilization_ceil_supply_570,
    astra_utilization_ceil_fixed_supply,
    570
);

proof_case!(
    astra_utilization_ceil_supply_571,
    astra_utilization_ceil_fixed_supply,
    571
);

proof_case!(
    astra_utilization_ceil_supply_572,
    astra_utilization_ceil_fixed_supply,
    572
);

proof_case!(
    astra_utilization_ceil_supply_573,
    astra_utilization_ceil_fixed_supply,
    573
);

proof_case!(
    astra_utilization_ceil_supply_574,
    astra_utilization_ceil_fixed_supply,
    574
);

proof_case!(
    astra_utilization_ceil_supply_575,
    astra_utilization_ceil_fixed_supply,
    575
);

proof_case!(
    astra_utilization_ceil_supply_576,
    astra_utilization_ceil_fixed_supply,
    576
);

proof_case!(
    astra_utilization_ceil_supply_577,
    astra_utilization_ceil_fixed_supply,
    577
);

proof_case!(
    astra_utilization_ceil_supply_578,
    astra_utilization_ceil_fixed_supply,
    578
);

proof_case!(
    astra_utilization_ceil_supply_579,
    astra_utilization_ceil_fixed_supply,
    579
);

proof_case!(
    astra_utilization_ceil_supply_580,
    astra_utilization_ceil_fixed_supply,
    580
);

proof_case!(
    astra_utilization_ceil_supply_581,
    astra_utilization_ceil_fixed_supply,
    581
);

proof_case!(
    astra_utilization_ceil_supply_582,
    astra_utilization_ceil_fixed_supply,
    582
);

proof_case!(
    astra_utilization_ceil_supply_583,
    astra_utilization_ceil_fixed_supply,
    583
);

proof_case!(
    astra_utilization_ceil_supply_584,
    astra_utilization_ceil_fixed_supply,
    584
);

proof_case!(
    astra_utilization_ceil_supply_585,
    astra_utilization_ceil_fixed_supply,
    585
);

proof_case!(
    astra_utilization_ceil_supply_586,
    astra_utilization_ceil_fixed_supply,
    586
);

proof_case!(
    astra_utilization_ceil_supply_587,
    astra_utilization_ceil_fixed_supply,
    587
);

proof_case!(
    astra_utilization_ceil_supply_588,
    astra_utilization_ceil_fixed_supply,
    588
);

proof_case!(
    astra_utilization_ceil_supply_589,
    astra_utilization_ceil_fixed_supply,
    589
);

proof_case!(
    astra_utilization_ceil_supply_590,
    astra_utilization_ceil_fixed_supply,
    590
);

proof_case!(
    astra_utilization_ceil_supply_591,
    astra_utilization_ceil_fixed_supply,
    591
);

proof_case!(
    astra_utilization_ceil_supply_592,
    astra_utilization_ceil_fixed_supply,
    592
);

proof_case!(
    astra_utilization_ceil_supply_593,
    astra_utilization_ceil_fixed_supply,
    593
);

proof_case!(
    astra_utilization_ceil_supply_594,
    astra_utilization_ceil_fixed_supply,
    594
);

proof_case!(
    astra_utilization_ceil_supply_595,
    astra_utilization_ceil_fixed_supply,
    595
);

proof_case!(
    astra_utilization_ceil_supply_596,
    astra_utilization_ceil_fixed_supply,
    596
);

proof_case!(
    astra_utilization_ceil_supply_597,
    astra_utilization_ceil_fixed_supply,
    597
);

proof_case!(
    astra_utilization_ceil_supply_598,
    astra_utilization_ceil_fixed_supply,
    598
);

proof_case!(
    astra_utilization_ceil_supply_599,
    astra_utilization_ceil_fixed_supply,
    599
);

proof_case!(
    astra_utilization_ceil_supply_600,
    astra_utilization_ceil_fixed_supply,
    600
);

proof_case!(
    astra_utilization_ceil_supply_601,
    astra_utilization_ceil_fixed_supply,
    601
);

proof_case!(
    astra_utilization_ceil_supply_602,
    astra_utilization_ceil_fixed_supply,
    602
);

proof_case!(
    astra_utilization_ceil_supply_603,
    astra_utilization_ceil_fixed_supply,
    603
);

proof_case!(
    astra_utilization_ceil_supply_604,
    astra_utilization_ceil_fixed_supply,
    604
);

proof_case!(
    astra_utilization_ceil_supply_605,
    astra_utilization_ceil_fixed_supply,
    605
);

proof_case!(
    astra_utilization_ceil_supply_606,
    astra_utilization_ceil_fixed_supply,
    606
);

proof_case!(
    astra_utilization_ceil_supply_607,
    astra_utilization_ceil_fixed_supply,
    607
);

proof_case!(
    astra_utilization_ceil_supply_608,
    astra_utilization_ceil_fixed_supply,
    608
);

proof_case!(
    astra_utilization_ceil_supply_609,
    astra_utilization_ceil_fixed_supply,
    609
);

proof_case!(
    astra_utilization_ceil_supply_610,
    astra_utilization_ceil_fixed_supply,
    610
);

proof_case!(
    astra_utilization_ceil_supply_611,
    astra_utilization_ceil_fixed_supply,
    611
);

proof_case!(
    astra_utilization_ceil_supply_612,
    astra_utilization_ceil_fixed_supply,
    612
);

proof_case!(
    astra_utilization_ceil_supply_613,
    astra_utilization_ceil_fixed_supply,
    613
);

proof_case!(
    astra_utilization_ceil_supply_614,
    astra_utilization_ceil_fixed_supply,
    614
);

proof_case!(
    astra_utilization_ceil_supply_615,
    astra_utilization_ceil_fixed_supply,
    615
);

proof_case!(
    astra_utilization_ceil_supply_616,
    astra_utilization_ceil_fixed_supply,
    616
);

proof_case!(
    astra_utilization_ceil_supply_617,
    astra_utilization_ceil_fixed_supply,
    617
);

proof_case!(
    astra_utilization_ceil_supply_618,
    astra_utilization_ceil_fixed_supply,
    618
);

proof_case!(
    astra_utilization_ceil_supply_619,
    astra_utilization_ceil_fixed_supply,
    619
);

proof_case!(
    astra_utilization_ceil_supply_620,
    astra_utilization_ceil_fixed_supply,
    620
);

proof_case!(
    astra_utilization_ceil_supply_621,
    astra_utilization_ceil_fixed_supply,
    621
);

proof_case!(
    astra_utilization_ceil_supply_622,
    astra_utilization_ceil_fixed_supply,
    622
);

proof_case!(
    astra_utilization_ceil_supply_623,
    astra_utilization_ceil_fixed_supply,
    623
);

proof_case!(
    astra_utilization_ceil_supply_624,
    astra_utilization_ceil_fixed_supply,
    624
);

proof_case!(
    astra_utilization_ceil_supply_625,
    astra_utilization_ceil_fixed_supply,
    625
);

proof_case!(
    astra_utilization_ceil_supply_626,
    astra_utilization_ceil_fixed_supply,
    626
);

proof_case!(
    astra_utilization_ceil_supply_627,
    astra_utilization_ceil_fixed_supply,
    627
);

proof_case!(
    astra_utilization_ceil_supply_628,
    astra_utilization_ceil_fixed_supply,
    628
);

proof_case!(
    astra_utilization_ceil_supply_629,
    astra_utilization_ceil_fixed_supply,
    629
);

proof_case!(
    astra_utilization_ceil_supply_630,
    astra_utilization_ceil_fixed_supply,
    630
);

proof_case!(
    astra_utilization_ceil_supply_631,
    astra_utilization_ceil_fixed_supply,
    631
);

proof_case!(
    astra_utilization_ceil_supply_632,
    astra_utilization_ceil_fixed_supply,
    632
);

proof_case!(
    astra_utilization_ceil_supply_633,
    astra_utilization_ceil_fixed_supply,
    633
);

proof_case!(
    astra_utilization_ceil_supply_634,
    astra_utilization_ceil_fixed_supply,
    634
);

proof_case!(
    astra_utilization_ceil_supply_635,
    astra_utilization_ceil_fixed_supply,
    635
);

proof_case!(
    astra_utilization_ceil_supply_636,
    astra_utilization_ceil_fixed_supply,
    636
);

proof_case!(
    astra_utilization_ceil_supply_637,
    astra_utilization_ceil_fixed_supply,
    637
);

proof_case!(
    astra_utilization_ceil_supply_638,
    astra_utilization_ceil_fixed_supply,
    638
);

proof_case!(
    astra_utilization_ceil_supply_639,
    astra_utilization_ceil_fixed_supply,
    639
);

proof_case!(
    astra_utilization_ceil_supply_640,
    astra_utilization_ceil_fixed_supply,
    640
);

proof_case!(
    astra_utilization_ceil_supply_641,
    astra_utilization_ceil_fixed_supply,
    641
);

proof_case!(
    astra_utilization_ceil_supply_642,
    astra_utilization_ceil_fixed_supply,
    642
);

proof_case!(
    astra_utilization_ceil_supply_643,
    astra_utilization_ceil_fixed_supply,
    643
);

proof_case!(
    astra_utilization_ceil_supply_644,
    astra_utilization_ceil_fixed_supply,
    644
);

proof_case!(
    astra_utilization_ceil_supply_645,
    astra_utilization_ceil_fixed_supply,
    645
);

proof_case!(
    astra_utilization_ceil_supply_646,
    astra_utilization_ceil_fixed_supply,
    646
);

proof_case!(
    astra_utilization_ceil_supply_647,
    astra_utilization_ceil_fixed_supply,
    647
);

proof_case!(
    astra_utilization_ceil_supply_648,
    astra_utilization_ceil_fixed_supply,
    648
);

proof_case!(
    astra_utilization_ceil_supply_649,
    astra_utilization_ceil_fixed_supply,
    649
);

fn astra_grow_debt_exact_delta_fixed_supply<const SUPPLY: u8>() {
    let d_supply = SUPPLY as i128;
    let rate_h = kani::any::<u8>();
    kani::assume(rate_h >= 1);
    let accrual_h = kani::any::<u8>();
    kani::assume(accrual_h >= 100); // loan accrual at least the unity scalar
    let mut data = sym_data(hundredths(rate_h), 0, 0, d_supply);
    let old_rate = data.d_rate;
    let accrued = data.grow_debt(&(), hundredths(accrual_h));
    let old_liabilities = (old_rate * d_supply + SCALAR_12 - 1) / SCALAR_12;
    let new_liabilities = (data.d_rate * d_supply + SCALAR_12 - 1) / SCALAR_12;
    assert_eq!(accrued, new_liabilities - old_liabilities);
    kani::cover!(true);
}

proof_case!(
    astra_grow_debt_exact_delta_supply_000,
    astra_grow_debt_exact_delta_fixed_supply,
    0
);

proof_case!(
    astra_grow_debt_exact_delta_supply_001,
    astra_grow_debt_exact_delta_fixed_supply,
    1
);

proof_case!(
    astra_grow_debt_exact_delta_supply_255,
    astra_grow_debt_exact_delta_fixed_supply,
    255
);

fn astra_grow_debt_nonnegative_delta_fixed_supply<const SUPPLY: u8>() {
    let d_supply = SUPPLY as i128;
    let rate_h = kani::any::<u8>();
    kani::assume(rate_h >= 1);
    let accrual_h = kani::any::<u8>();
    kani::assume(accrual_h >= 100); // loan accrual at least the unity scalar
    let mut data = sym_data(hundredths(rate_h), 0, 0, d_supply);
    let accrued = data.grow_debt(&(), hundredths(accrual_h));
    assert!(accrued >= 0); // debt interest is nonnegative
    kani::cover!(true);
}

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_000,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    0
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_001,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    1
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_255,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    255
);

fn astra_accrue_positive_rate_fixed_supply<const SUPPLY: u8>() {
    let b_supply = SUPPLY as i128;
    kani::assume(b_supply >= 1); // b_rate division divisor
    let mut data = sym_data(0, hundredths(kani::any::<u8>()), b_supply, 0);
    let accrued = kani::any::<u8>() as i128;
    let take_h = kani::any::<u8>();
    kani::assume(take_h < 100); // take rate is a percentage in SCALAR_7 points, < 100%
    let bstop_rate = (take_h as u32 * SCALAR_7 as u32) / 100; // < SCALAR_7, genuine zero included
    let old_credit = kani::any::<u8>() as i128;
    data.backstop_credit = old_credit;
    let pre_supply = data.to_asset_from_b_token(&(), data.b_supply);
    let old_b_rate = data.b_rate;
    kani::assume(accrued > 0);
    data.accrue(&(), bstop_rate, accrued);
    let expected_credit = if bstop_rate == 0 {
        0
    } else {
        (accrued * bstop_rate as i128) / SCALAR_7
    };
    assert_eq!(
        data.b_rate,
        (pre_supply + accrued - expected_credit) * SCALAR_12 / b_supply
    );
    kani::cover!(true);
}

proof_case!(
    astra_accrue_positive_rate_supply_001,
    astra_accrue_positive_rate_fixed_supply,
    1
);

proof_case!(
    astra_accrue_positive_rate_supply_003,
    astra_accrue_positive_rate_fixed_supply,
    3
);

proof_case!(
    astra_accrue_positive_rate_supply_255,
    astra_accrue_positive_rate_fixed_supply,
    255
);

fn astra_accrue_positive_accounting_fixed_supply<const SUPPLY: u8>() {
    let b_supply = SUPPLY as i128;
    kani::assume(b_supply >= 1); // b_rate division divisor
    let mut data = sym_data(0, hundredths(kani::any::<u8>()), b_supply, 0);
    let accrued = kani::any::<u8>() as i128;
    let take_h = kani::any::<u8>();
    kani::assume(take_h < 100); // take rate is a percentage in SCALAR_7 points, < 100%
    let bstop_rate = (take_h as u32 * SCALAR_7 as u32) / 100; // < SCALAR_7, genuine zero included
    let old_credit = kani::any::<u8>() as i128;
    data.backstop_credit = old_credit;
    let pre_supply = data.to_asset_from_b_token(&(), data.b_supply);
    let old_b_rate = data.b_rate;
    kani::assume(accrued > 0);
    data.accrue(&(), bstop_rate, accrued);
    // outer-floor bound over BOTH nested floors, in asset units: no value minted
    let credit_delta = data.backstop_credit - old_credit;
    let actual_after_supply = data.to_asset_from_b_token(&(), b_supply);
    assert!(credit_delta + actual_after_supply <= pre_supply + accrued);
    let gap = pre_supply + accrued - credit_delta - actual_after_supply;
    assert!(gap >= 0);
    // deficit is the sum of both floor residuals: rate residual < b_supply, asset residual < SCALAR_12
    assert!(gap * SCALAR_12 < b_supply + SCALAR_12);
    kani::cover!(true);
}

proof_case!(
    astra_accrue_positive_accounting_supply_001,
    astra_accrue_positive_accounting_fixed_supply,
    1
);

proof_case!(
    astra_accrue_positive_accounting_supply_003,
    astra_accrue_positive_accounting_fixed_supply,
    3
);

proof_case!(
    astra_accrue_positive_accounting_supply_255,
    astra_accrue_positive_accounting_fixed_supply,
    255
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_002,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    2
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_003,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    3
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_004,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    4
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_005,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    5
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_006,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    6
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_007,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    7
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_008,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    8
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_009,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    9
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_010,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    10
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_011,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    11
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_012,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    12
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_013,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    13
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_014,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    14
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_015,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    15
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_016,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    16
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_017,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    17
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_018,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    18
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_019,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    19
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_020,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    20
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_021,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    21
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_022,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    22
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_023,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    23
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_024,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    24
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_025,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    25
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_026,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    26
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_027,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    27
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_028,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    28
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_029,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    29
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_030,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    30
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_031,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    31
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_032,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    32
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_033,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    33
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_034,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    34
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_035,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    35
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_036,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    36
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_037,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    37
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_038,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    38
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_039,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    39
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_040,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    40
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_041,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    41
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_042,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    42
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_043,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    43
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_044,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    44
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_045,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    45
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_046,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    46
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_047,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    47
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_048,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    48
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_049,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    49
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_050,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    50
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_051,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    51
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_052,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    52
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_053,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    53
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_054,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    54
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_055,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    55
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_056,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    56
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_057,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    57
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_058,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    58
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_059,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    59
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_060,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    60
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_061,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    61
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_062,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    62
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_063,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    63
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_064,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    64
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_065,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    65
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_066,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    66
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_067,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    67
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_068,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    68
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_069,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    69
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_070,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    70
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_071,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    71
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_072,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    72
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_073,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    73
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_074,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    74
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_075,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    75
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_076,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    76
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_077,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    77
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_078,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    78
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_079,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    79
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_080,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    80
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_081,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    81
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_082,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    82
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_083,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    83
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_084,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    84
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_085,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    85
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_086,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    86
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_087,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    87
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_088,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    88
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_089,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    89
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_090,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    90
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_091,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    91
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_092,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    92
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_093,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    93
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_094,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    94
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_095,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    95
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_096,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    96
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_097,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    97
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_098,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    98
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_099,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    99
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_100,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    100
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_101,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    101
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_102,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    102
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_103,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    103
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_104,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    104
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_105,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    105
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_106,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    106
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_107,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    107
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_108,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    108
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_109,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    109
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_110,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    110
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_111,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    111
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_112,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    112
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_113,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    113
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_114,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    114
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_115,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    115
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_116,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    116
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_117,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    117
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_118,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    118
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_119,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    119
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_120,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    120
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_121,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    121
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_122,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    122
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_123,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    123
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_124,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    124
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_125,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    125
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_126,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    126
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_127,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    127
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_128,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    128
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_129,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    129
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_130,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    130
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_131,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    131
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_132,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    132
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_133,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    133
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_134,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    134
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_135,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    135
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_136,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    136
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_137,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    137
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_138,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    138
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_139,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    139
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_140,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    140
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_141,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    141
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_142,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    142
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_143,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    143
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_144,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    144
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_145,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    145
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_146,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    146
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_147,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    147
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_148,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    148
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_149,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    149
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_150,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    150
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_151,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    151
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_152,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    152
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_153,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    153
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_154,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    154
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_155,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    155
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_156,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    156
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_157,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    157
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_158,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    158
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_159,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    159
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_160,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    160
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_161,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    161
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_162,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    162
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_163,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    163
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_164,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    164
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_165,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    165
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_166,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    166
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_167,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    167
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_168,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    168
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_169,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    169
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_170,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    170
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_171,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    171
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_172,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    172
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_173,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    173
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_174,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    174
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_175,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    175
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_176,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    176
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_177,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    177
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_178,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    178
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_179,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    179
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_180,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    180
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_181,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    181
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_182,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    182
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_183,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    183
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_184,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    184
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_185,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    185
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_186,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    186
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_187,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    187
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_188,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    188
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_189,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    189
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_190,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    190
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_191,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    191
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_192,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    192
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_193,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    193
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_194,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    194
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_195,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    195
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_196,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    196
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_197,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    197
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_198,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    198
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_199,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    199
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_200,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    200
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_201,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    201
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_202,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    202
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_203,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    203
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_204,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    204
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_205,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    205
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_206,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    206
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_207,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    207
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_208,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    208
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_209,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    209
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_210,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    210
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_211,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    211
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_212,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    212
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_213,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    213
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_214,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    214
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_215,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    215
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_216,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    216
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_217,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    217
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_218,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    218
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_219,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    219
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_220,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    220
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_221,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    221
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_222,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    222
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_223,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    223
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_224,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    224
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_225,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    225
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_226,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    226
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_227,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    227
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_228,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    228
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_229,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    229
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_230,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    230
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_231,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    231
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_232,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    232
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_233,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    233
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_234,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    234
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_235,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    235
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_236,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    236
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_237,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    237
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_238,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    238
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_239,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    239
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_240,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    240
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_241,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    241
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_242,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    242
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_243,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    243
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_244,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    244
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_245,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    245
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_246,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    246
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_247,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    247
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_248,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    248
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_249,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    249
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_250,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    250
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_251,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    251
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_252,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    252
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_253,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    253
);

proof_case!(
    astra_grow_debt_nonnegative_delta_supply_254,
    astra_grow_debt_nonnegative_delta_fixed_supply,
    254
);

proof_case!(
    astra_accrue_positive_rate_supply_002,
    astra_accrue_positive_rate_fixed_supply,
    2
);

proof_case!(
    astra_accrue_positive_rate_supply_004,
    astra_accrue_positive_rate_fixed_supply,
    4
);

proof_case!(
    astra_accrue_positive_rate_supply_005,
    astra_accrue_positive_rate_fixed_supply,
    5
);

proof_case!(
    astra_accrue_positive_rate_supply_006,
    astra_accrue_positive_rate_fixed_supply,
    6
);

proof_case!(
    astra_accrue_positive_rate_supply_007,
    astra_accrue_positive_rate_fixed_supply,
    7
);

proof_case!(
    astra_accrue_positive_rate_supply_008,
    astra_accrue_positive_rate_fixed_supply,
    8
);

proof_case!(
    astra_accrue_positive_rate_supply_009,
    astra_accrue_positive_rate_fixed_supply,
    9
);

proof_case!(
    astra_accrue_positive_rate_supply_010,
    astra_accrue_positive_rate_fixed_supply,
    10
);

proof_case!(
    astra_accrue_positive_rate_supply_011,
    astra_accrue_positive_rate_fixed_supply,
    11
);

proof_case!(
    astra_accrue_positive_rate_supply_012,
    astra_accrue_positive_rate_fixed_supply,
    12
);

proof_case!(
    astra_accrue_positive_rate_supply_013,
    astra_accrue_positive_rate_fixed_supply,
    13
);

proof_case!(
    astra_accrue_positive_rate_supply_014,
    astra_accrue_positive_rate_fixed_supply,
    14
);

proof_case!(
    astra_accrue_positive_rate_supply_015,
    astra_accrue_positive_rate_fixed_supply,
    15
);

proof_case!(
    astra_accrue_positive_rate_supply_016,
    astra_accrue_positive_rate_fixed_supply,
    16
);

proof_case!(
    astra_accrue_positive_rate_supply_017,
    astra_accrue_positive_rate_fixed_supply,
    17
);

proof_case!(
    astra_accrue_positive_rate_supply_018,
    astra_accrue_positive_rate_fixed_supply,
    18
);

proof_case!(
    astra_accrue_positive_rate_supply_019,
    astra_accrue_positive_rate_fixed_supply,
    19
);

proof_case!(
    astra_accrue_positive_rate_supply_020,
    astra_accrue_positive_rate_fixed_supply,
    20
);

proof_case!(
    astra_accrue_positive_rate_supply_021,
    astra_accrue_positive_rate_fixed_supply,
    21
);

proof_case!(
    astra_accrue_positive_rate_supply_022,
    astra_accrue_positive_rate_fixed_supply,
    22
);

proof_case!(
    astra_accrue_positive_rate_supply_023,
    astra_accrue_positive_rate_fixed_supply,
    23
);

proof_case!(
    astra_accrue_positive_rate_supply_024,
    astra_accrue_positive_rate_fixed_supply,
    24
);

proof_case!(
    astra_accrue_positive_rate_supply_025,
    astra_accrue_positive_rate_fixed_supply,
    25
);

proof_case!(
    astra_accrue_positive_rate_supply_026,
    astra_accrue_positive_rate_fixed_supply,
    26
);

proof_case!(
    astra_accrue_positive_rate_supply_027,
    astra_accrue_positive_rate_fixed_supply,
    27
);

proof_case!(
    astra_accrue_positive_rate_supply_028,
    astra_accrue_positive_rate_fixed_supply,
    28
);

proof_case!(
    astra_accrue_positive_rate_supply_029,
    astra_accrue_positive_rate_fixed_supply,
    29
);

proof_case!(
    astra_accrue_positive_rate_supply_030,
    astra_accrue_positive_rate_fixed_supply,
    30
);

proof_case!(
    astra_accrue_positive_rate_supply_031,
    astra_accrue_positive_rate_fixed_supply,
    31
);

proof_case!(
    astra_accrue_positive_rate_supply_032,
    astra_accrue_positive_rate_fixed_supply,
    32
);

proof_case!(
    astra_accrue_positive_rate_supply_033,
    astra_accrue_positive_rate_fixed_supply,
    33
);

proof_case!(
    astra_accrue_positive_rate_supply_034,
    astra_accrue_positive_rate_fixed_supply,
    34
);

proof_case!(
    astra_accrue_positive_rate_supply_035,
    astra_accrue_positive_rate_fixed_supply,
    35
);

proof_case!(
    astra_accrue_positive_rate_supply_036,
    astra_accrue_positive_rate_fixed_supply,
    36
);

proof_case!(
    astra_accrue_positive_rate_supply_037,
    astra_accrue_positive_rate_fixed_supply,
    37
);

proof_case!(
    astra_accrue_positive_rate_supply_038,
    astra_accrue_positive_rate_fixed_supply,
    38
);

proof_case!(
    astra_accrue_positive_rate_supply_039,
    astra_accrue_positive_rate_fixed_supply,
    39
);

proof_case!(
    astra_accrue_positive_rate_supply_040,
    astra_accrue_positive_rate_fixed_supply,
    40
);

proof_case!(
    astra_accrue_positive_rate_supply_041,
    astra_accrue_positive_rate_fixed_supply,
    41
);

proof_case!(
    astra_accrue_positive_rate_supply_042,
    astra_accrue_positive_rate_fixed_supply,
    42
);

proof_case!(
    astra_accrue_positive_rate_supply_043,
    astra_accrue_positive_rate_fixed_supply,
    43
);

proof_case!(
    astra_accrue_positive_rate_supply_044,
    astra_accrue_positive_rate_fixed_supply,
    44
);

proof_case!(
    astra_accrue_positive_rate_supply_045,
    astra_accrue_positive_rate_fixed_supply,
    45
);

proof_case!(
    astra_accrue_positive_rate_supply_046,
    astra_accrue_positive_rate_fixed_supply,
    46
);

proof_case!(
    astra_accrue_positive_rate_supply_047,
    astra_accrue_positive_rate_fixed_supply,
    47
);

proof_case!(
    astra_accrue_positive_rate_supply_048,
    astra_accrue_positive_rate_fixed_supply,
    48
);

proof_case!(
    astra_accrue_positive_rate_supply_049,
    astra_accrue_positive_rate_fixed_supply,
    49
);

proof_case!(
    astra_accrue_positive_rate_supply_050,
    astra_accrue_positive_rate_fixed_supply,
    50
);

proof_case!(
    astra_accrue_positive_rate_supply_051,
    astra_accrue_positive_rate_fixed_supply,
    51
);

proof_case!(
    astra_accrue_positive_rate_supply_052,
    astra_accrue_positive_rate_fixed_supply,
    52
);

proof_case!(
    astra_accrue_positive_rate_supply_053,
    astra_accrue_positive_rate_fixed_supply,
    53
);

proof_case!(
    astra_accrue_positive_rate_supply_054,
    astra_accrue_positive_rate_fixed_supply,
    54
);

proof_case!(
    astra_accrue_positive_rate_supply_055,
    astra_accrue_positive_rate_fixed_supply,
    55
);

proof_case!(
    astra_accrue_positive_rate_supply_056,
    astra_accrue_positive_rate_fixed_supply,
    56
);

proof_case!(
    astra_accrue_positive_rate_supply_057,
    astra_accrue_positive_rate_fixed_supply,
    57
);

proof_case!(
    astra_accrue_positive_rate_supply_058,
    astra_accrue_positive_rate_fixed_supply,
    58
);

proof_case!(
    astra_accrue_positive_rate_supply_059,
    astra_accrue_positive_rate_fixed_supply,
    59
);

proof_case!(
    astra_accrue_positive_rate_supply_060,
    astra_accrue_positive_rate_fixed_supply,
    60
);

proof_case!(
    astra_accrue_positive_rate_supply_061,
    astra_accrue_positive_rate_fixed_supply,
    61
);

proof_case!(
    astra_accrue_positive_rate_supply_062,
    astra_accrue_positive_rate_fixed_supply,
    62
);

proof_case!(
    astra_accrue_positive_rate_supply_063,
    astra_accrue_positive_rate_fixed_supply,
    63
);

proof_case!(
    astra_accrue_positive_rate_supply_064,
    astra_accrue_positive_rate_fixed_supply,
    64
);

proof_case!(
    astra_accrue_positive_rate_supply_065,
    astra_accrue_positive_rate_fixed_supply,
    65
);

proof_case!(
    astra_accrue_positive_rate_supply_066,
    astra_accrue_positive_rate_fixed_supply,
    66
);

proof_case!(
    astra_accrue_positive_rate_supply_067,
    astra_accrue_positive_rate_fixed_supply,
    67
);

proof_case!(
    astra_accrue_positive_rate_supply_068,
    astra_accrue_positive_rate_fixed_supply,
    68
);

proof_case!(
    astra_accrue_positive_rate_supply_069,
    astra_accrue_positive_rate_fixed_supply,
    69
);

proof_case!(
    astra_accrue_positive_rate_supply_070,
    astra_accrue_positive_rate_fixed_supply,
    70
);

proof_case!(
    astra_accrue_positive_rate_supply_071,
    astra_accrue_positive_rate_fixed_supply,
    71
);

proof_case!(
    astra_accrue_positive_rate_supply_072,
    astra_accrue_positive_rate_fixed_supply,
    72
);

proof_case!(
    astra_accrue_positive_rate_supply_073,
    astra_accrue_positive_rate_fixed_supply,
    73
);

proof_case!(
    astra_accrue_positive_rate_supply_074,
    astra_accrue_positive_rate_fixed_supply,
    74
);

proof_case!(
    astra_accrue_positive_rate_supply_075,
    astra_accrue_positive_rate_fixed_supply,
    75
);

proof_case!(
    astra_accrue_positive_rate_supply_076,
    astra_accrue_positive_rate_fixed_supply,
    76
);

proof_case!(
    astra_accrue_positive_rate_supply_077,
    astra_accrue_positive_rate_fixed_supply,
    77
);

proof_case!(
    astra_accrue_positive_rate_supply_078,
    astra_accrue_positive_rate_fixed_supply,
    78
);

proof_case!(
    astra_accrue_positive_rate_supply_079,
    astra_accrue_positive_rate_fixed_supply,
    79
);

proof_case!(
    astra_accrue_positive_rate_supply_080,
    astra_accrue_positive_rate_fixed_supply,
    80
);

proof_case!(
    astra_accrue_positive_rate_supply_081,
    astra_accrue_positive_rate_fixed_supply,
    81
);

proof_case!(
    astra_accrue_positive_rate_supply_082,
    astra_accrue_positive_rate_fixed_supply,
    82
);

proof_case!(
    astra_accrue_positive_rate_supply_083,
    astra_accrue_positive_rate_fixed_supply,
    83
);

proof_case!(
    astra_accrue_positive_rate_supply_084,
    astra_accrue_positive_rate_fixed_supply,
    84
);

proof_case!(
    astra_accrue_positive_rate_supply_085,
    astra_accrue_positive_rate_fixed_supply,
    85
);

proof_case!(
    astra_accrue_positive_rate_supply_086,
    astra_accrue_positive_rate_fixed_supply,
    86
);

proof_case!(
    astra_accrue_positive_rate_supply_087,
    astra_accrue_positive_rate_fixed_supply,
    87
);

proof_case!(
    astra_accrue_positive_rate_supply_088,
    astra_accrue_positive_rate_fixed_supply,
    88
);

proof_case!(
    astra_accrue_positive_rate_supply_089,
    astra_accrue_positive_rate_fixed_supply,
    89
);

proof_case!(
    astra_accrue_positive_rate_supply_090,
    astra_accrue_positive_rate_fixed_supply,
    90
);

proof_case!(
    astra_accrue_positive_rate_supply_091,
    astra_accrue_positive_rate_fixed_supply,
    91
);

proof_case!(
    astra_accrue_positive_rate_supply_092,
    astra_accrue_positive_rate_fixed_supply,
    92
);

proof_case!(
    astra_accrue_positive_rate_supply_093,
    astra_accrue_positive_rate_fixed_supply,
    93
);

proof_case!(
    astra_accrue_positive_rate_supply_094,
    astra_accrue_positive_rate_fixed_supply,
    94
);

proof_case!(
    astra_accrue_positive_rate_supply_095,
    astra_accrue_positive_rate_fixed_supply,
    95
);

proof_case!(
    astra_accrue_positive_rate_supply_096,
    astra_accrue_positive_rate_fixed_supply,
    96
);

proof_case!(
    astra_accrue_positive_rate_supply_097,
    astra_accrue_positive_rate_fixed_supply,
    97
);

proof_case!(
    astra_accrue_positive_rate_supply_098,
    astra_accrue_positive_rate_fixed_supply,
    98
);

proof_case!(
    astra_accrue_positive_rate_supply_099,
    astra_accrue_positive_rate_fixed_supply,
    99
);

proof_case!(
    astra_accrue_positive_rate_supply_100,
    astra_accrue_positive_rate_fixed_supply,
    100
);

proof_case!(
    astra_accrue_positive_rate_supply_101,
    astra_accrue_positive_rate_fixed_supply,
    101
);

proof_case!(
    astra_accrue_positive_rate_supply_102,
    astra_accrue_positive_rate_fixed_supply,
    102
);

proof_case!(
    astra_accrue_positive_rate_supply_103,
    astra_accrue_positive_rate_fixed_supply,
    103
);

proof_case!(
    astra_accrue_positive_rate_supply_104,
    astra_accrue_positive_rate_fixed_supply,
    104
);

proof_case!(
    astra_accrue_positive_rate_supply_105,
    astra_accrue_positive_rate_fixed_supply,
    105
);

proof_case!(
    astra_accrue_positive_rate_supply_106,
    astra_accrue_positive_rate_fixed_supply,
    106
);

proof_case!(
    astra_accrue_positive_rate_supply_107,
    astra_accrue_positive_rate_fixed_supply,
    107
);

proof_case!(
    astra_accrue_positive_rate_supply_108,
    astra_accrue_positive_rate_fixed_supply,
    108
);

proof_case!(
    astra_accrue_positive_rate_supply_109,
    astra_accrue_positive_rate_fixed_supply,
    109
);

proof_case!(
    astra_accrue_positive_rate_supply_110,
    astra_accrue_positive_rate_fixed_supply,
    110
);

proof_case!(
    astra_accrue_positive_rate_supply_111,
    astra_accrue_positive_rate_fixed_supply,
    111
);

proof_case!(
    astra_accrue_positive_rate_supply_112,
    astra_accrue_positive_rate_fixed_supply,
    112
);

proof_case!(
    astra_accrue_positive_rate_supply_113,
    astra_accrue_positive_rate_fixed_supply,
    113
);

proof_case!(
    astra_accrue_positive_rate_supply_114,
    astra_accrue_positive_rate_fixed_supply,
    114
);

proof_case!(
    astra_accrue_positive_rate_supply_115,
    astra_accrue_positive_rate_fixed_supply,
    115
);

proof_case!(
    astra_accrue_positive_rate_supply_116,
    astra_accrue_positive_rate_fixed_supply,
    116
);

proof_case!(
    astra_accrue_positive_rate_supply_117,
    astra_accrue_positive_rate_fixed_supply,
    117
);

proof_case!(
    astra_accrue_positive_rate_supply_118,
    astra_accrue_positive_rate_fixed_supply,
    118
);

proof_case!(
    astra_accrue_positive_rate_supply_119,
    astra_accrue_positive_rate_fixed_supply,
    119
);

proof_case!(
    astra_accrue_positive_rate_supply_120,
    astra_accrue_positive_rate_fixed_supply,
    120
);

proof_case!(
    astra_accrue_positive_rate_supply_121,
    astra_accrue_positive_rate_fixed_supply,
    121
);

proof_case!(
    astra_accrue_positive_rate_supply_122,
    astra_accrue_positive_rate_fixed_supply,
    122
);

proof_case!(
    astra_accrue_positive_rate_supply_123,
    astra_accrue_positive_rate_fixed_supply,
    123
);

proof_case!(
    astra_accrue_positive_rate_supply_124,
    astra_accrue_positive_rate_fixed_supply,
    124
);

proof_case!(
    astra_accrue_positive_rate_supply_125,
    astra_accrue_positive_rate_fixed_supply,
    125
);

proof_case!(
    astra_accrue_positive_rate_supply_126,
    astra_accrue_positive_rate_fixed_supply,
    126
);

proof_case!(
    astra_accrue_positive_rate_supply_127,
    astra_accrue_positive_rate_fixed_supply,
    127
);

proof_case!(
    astra_accrue_positive_rate_supply_128,
    astra_accrue_positive_rate_fixed_supply,
    128
);

proof_case!(
    astra_accrue_positive_rate_supply_129,
    astra_accrue_positive_rate_fixed_supply,
    129
);

proof_case!(
    astra_accrue_positive_rate_supply_130,
    astra_accrue_positive_rate_fixed_supply,
    130
);

proof_case!(
    astra_accrue_positive_rate_supply_131,
    astra_accrue_positive_rate_fixed_supply,
    131
);

proof_case!(
    astra_accrue_positive_rate_supply_132,
    astra_accrue_positive_rate_fixed_supply,
    132
);

proof_case!(
    astra_accrue_positive_rate_supply_133,
    astra_accrue_positive_rate_fixed_supply,
    133
);

proof_case!(
    astra_accrue_positive_rate_supply_134,
    astra_accrue_positive_rate_fixed_supply,
    134
);

proof_case!(
    astra_accrue_positive_rate_supply_135,
    astra_accrue_positive_rate_fixed_supply,
    135
);

proof_case!(
    astra_accrue_positive_rate_supply_136,
    astra_accrue_positive_rate_fixed_supply,
    136
);

proof_case!(
    astra_accrue_positive_rate_supply_137,
    astra_accrue_positive_rate_fixed_supply,
    137
);

proof_case!(
    astra_accrue_positive_rate_supply_138,
    astra_accrue_positive_rate_fixed_supply,
    138
);

proof_case!(
    astra_accrue_positive_rate_supply_139,
    astra_accrue_positive_rate_fixed_supply,
    139
);

proof_case!(
    astra_accrue_positive_rate_supply_140,
    astra_accrue_positive_rate_fixed_supply,
    140
);

proof_case!(
    astra_accrue_positive_rate_supply_141,
    astra_accrue_positive_rate_fixed_supply,
    141
);

proof_case!(
    astra_accrue_positive_rate_supply_142,
    astra_accrue_positive_rate_fixed_supply,
    142
);

proof_case!(
    astra_accrue_positive_rate_supply_143,
    astra_accrue_positive_rate_fixed_supply,
    143
);

proof_case!(
    astra_accrue_positive_rate_supply_144,
    astra_accrue_positive_rate_fixed_supply,
    144
);

proof_case!(
    astra_accrue_positive_rate_supply_145,
    astra_accrue_positive_rate_fixed_supply,
    145
);

proof_case!(
    astra_accrue_positive_rate_supply_146,
    astra_accrue_positive_rate_fixed_supply,
    146
);

proof_case!(
    astra_accrue_positive_rate_supply_147,
    astra_accrue_positive_rate_fixed_supply,
    147
);

proof_case!(
    astra_accrue_positive_rate_supply_148,
    astra_accrue_positive_rate_fixed_supply,
    148
);

proof_case!(
    astra_accrue_positive_rate_supply_149,
    astra_accrue_positive_rate_fixed_supply,
    149
);

proof_case!(
    astra_accrue_positive_rate_supply_150,
    astra_accrue_positive_rate_fixed_supply,
    150
);

proof_case!(
    astra_accrue_positive_rate_supply_151,
    astra_accrue_positive_rate_fixed_supply,
    151
);

proof_case!(
    astra_accrue_positive_rate_supply_152,
    astra_accrue_positive_rate_fixed_supply,
    152
);

proof_case!(
    astra_accrue_positive_rate_supply_153,
    astra_accrue_positive_rate_fixed_supply,
    153
);

proof_case!(
    astra_accrue_positive_rate_supply_154,
    astra_accrue_positive_rate_fixed_supply,
    154
);

proof_case!(
    astra_accrue_positive_rate_supply_155,
    astra_accrue_positive_rate_fixed_supply,
    155
);

proof_case!(
    astra_accrue_positive_rate_supply_156,
    astra_accrue_positive_rate_fixed_supply,
    156
);

proof_case!(
    astra_accrue_positive_rate_supply_157,
    astra_accrue_positive_rate_fixed_supply,
    157
);

proof_case!(
    astra_accrue_positive_rate_supply_158,
    astra_accrue_positive_rate_fixed_supply,
    158
);

proof_case!(
    astra_accrue_positive_rate_supply_159,
    astra_accrue_positive_rate_fixed_supply,
    159
);

proof_case!(
    astra_accrue_positive_rate_supply_160,
    astra_accrue_positive_rate_fixed_supply,
    160
);

proof_case!(
    astra_accrue_positive_rate_supply_161,
    astra_accrue_positive_rate_fixed_supply,
    161
);

proof_case!(
    astra_accrue_positive_rate_supply_162,
    astra_accrue_positive_rate_fixed_supply,
    162
);

proof_case!(
    astra_accrue_positive_rate_supply_163,
    astra_accrue_positive_rate_fixed_supply,
    163
);

proof_case!(
    astra_accrue_positive_rate_supply_164,
    astra_accrue_positive_rate_fixed_supply,
    164
);

proof_case!(
    astra_accrue_positive_rate_supply_165,
    astra_accrue_positive_rate_fixed_supply,
    165
);

proof_case!(
    astra_accrue_positive_rate_supply_166,
    astra_accrue_positive_rate_fixed_supply,
    166
);

proof_case!(
    astra_accrue_positive_rate_supply_167,
    astra_accrue_positive_rate_fixed_supply,
    167
);

proof_case!(
    astra_accrue_positive_rate_supply_168,
    astra_accrue_positive_rate_fixed_supply,
    168
);

proof_case!(
    astra_accrue_positive_rate_supply_169,
    astra_accrue_positive_rate_fixed_supply,
    169
);

proof_case!(
    astra_accrue_positive_rate_supply_170,
    astra_accrue_positive_rate_fixed_supply,
    170
);

proof_case!(
    astra_accrue_positive_rate_supply_171,
    astra_accrue_positive_rate_fixed_supply,
    171
);

proof_case!(
    astra_accrue_positive_rate_supply_172,
    astra_accrue_positive_rate_fixed_supply,
    172
);

proof_case!(
    astra_accrue_positive_rate_supply_173,
    astra_accrue_positive_rate_fixed_supply,
    173
);

proof_case!(
    astra_accrue_positive_rate_supply_174,
    astra_accrue_positive_rate_fixed_supply,
    174
);

proof_case!(
    astra_accrue_positive_rate_supply_175,
    astra_accrue_positive_rate_fixed_supply,
    175
);

proof_case!(
    astra_accrue_positive_rate_supply_176,
    astra_accrue_positive_rate_fixed_supply,
    176
);

proof_case!(
    astra_accrue_positive_rate_supply_177,
    astra_accrue_positive_rate_fixed_supply,
    177
);

proof_case!(
    astra_accrue_positive_rate_supply_178,
    astra_accrue_positive_rate_fixed_supply,
    178
);

proof_case!(
    astra_accrue_positive_rate_supply_179,
    astra_accrue_positive_rate_fixed_supply,
    179
);

proof_case!(
    astra_accrue_positive_rate_supply_180,
    astra_accrue_positive_rate_fixed_supply,
    180
);

proof_case!(
    astra_accrue_positive_rate_supply_181,
    astra_accrue_positive_rate_fixed_supply,
    181
);

proof_case!(
    astra_accrue_positive_rate_supply_182,
    astra_accrue_positive_rate_fixed_supply,
    182
);

proof_case!(
    astra_accrue_positive_rate_supply_183,
    astra_accrue_positive_rate_fixed_supply,
    183
);

proof_case!(
    astra_accrue_positive_rate_supply_184,
    astra_accrue_positive_rate_fixed_supply,
    184
);

proof_case!(
    astra_accrue_positive_rate_supply_185,
    astra_accrue_positive_rate_fixed_supply,
    185
);

proof_case!(
    astra_accrue_positive_rate_supply_186,
    astra_accrue_positive_rate_fixed_supply,
    186
);

proof_case!(
    astra_accrue_positive_rate_supply_187,
    astra_accrue_positive_rate_fixed_supply,
    187
);

proof_case!(
    astra_accrue_positive_rate_supply_188,
    astra_accrue_positive_rate_fixed_supply,
    188
);

proof_case!(
    astra_accrue_positive_rate_supply_189,
    astra_accrue_positive_rate_fixed_supply,
    189
);

proof_case!(
    astra_accrue_positive_rate_supply_190,
    astra_accrue_positive_rate_fixed_supply,
    190
);

proof_case!(
    astra_accrue_positive_rate_supply_191,
    astra_accrue_positive_rate_fixed_supply,
    191
);

proof_case!(
    astra_accrue_positive_rate_supply_192,
    astra_accrue_positive_rate_fixed_supply,
    192
);

proof_case!(
    astra_accrue_positive_rate_supply_193,
    astra_accrue_positive_rate_fixed_supply,
    193
);

proof_case!(
    astra_accrue_positive_rate_supply_194,
    astra_accrue_positive_rate_fixed_supply,
    194
);

proof_case!(
    astra_accrue_positive_rate_supply_195,
    astra_accrue_positive_rate_fixed_supply,
    195
);

proof_case!(
    astra_accrue_positive_rate_supply_196,
    astra_accrue_positive_rate_fixed_supply,
    196
);

proof_case!(
    astra_accrue_positive_rate_supply_197,
    astra_accrue_positive_rate_fixed_supply,
    197
);

proof_case!(
    astra_accrue_positive_rate_supply_198,
    astra_accrue_positive_rate_fixed_supply,
    198
);

proof_case!(
    astra_accrue_positive_rate_supply_199,
    astra_accrue_positive_rate_fixed_supply,
    199
);

proof_case!(
    astra_accrue_positive_rate_supply_200,
    astra_accrue_positive_rate_fixed_supply,
    200
);

proof_case!(
    astra_accrue_positive_rate_supply_201,
    astra_accrue_positive_rate_fixed_supply,
    201
);

proof_case!(
    astra_accrue_positive_rate_supply_202,
    astra_accrue_positive_rate_fixed_supply,
    202
);

proof_case!(
    astra_accrue_positive_rate_supply_203,
    astra_accrue_positive_rate_fixed_supply,
    203
);

proof_case!(
    astra_accrue_positive_rate_supply_204,
    astra_accrue_positive_rate_fixed_supply,
    204
);

proof_case!(
    astra_accrue_positive_rate_supply_205,
    astra_accrue_positive_rate_fixed_supply,
    205
);

proof_case!(
    astra_accrue_positive_rate_supply_206,
    astra_accrue_positive_rate_fixed_supply,
    206
);

proof_case!(
    astra_accrue_positive_rate_supply_207,
    astra_accrue_positive_rate_fixed_supply,
    207
);

proof_case!(
    astra_accrue_positive_rate_supply_208,
    astra_accrue_positive_rate_fixed_supply,
    208
);

proof_case!(
    astra_accrue_positive_rate_supply_209,
    astra_accrue_positive_rate_fixed_supply,
    209
);

proof_case!(
    astra_accrue_positive_rate_supply_210,
    astra_accrue_positive_rate_fixed_supply,
    210
);

proof_case!(
    astra_accrue_positive_rate_supply_211,
    astra_accrue_positive_rate_fixed_supply,
    211
);

proof_case!(
    astra_accrue_positive_rate_supply_212,
    astra_accrue_positive_rate_fixed_supply,
    212
);

proof_case!(
    astra_accrue_positive_rate_supply_213,
    astra_accrue_positive_rate_fixed_supply,
    213
);

proof_case!(
    astra_accrue_positive_rate_supply_214,
    astra_accrue_positive_rate_fixed_supply,
    214
);

proof_case!(
    astra_accrue_positive_rate_supply_215,
    astra_accrue_positive_rate_fixed_supply,
    215
);

proof_case!(
    astra_accrue_positive_rate_supply_216,
    astra_accrue_positive_rate_fixed_supply,
    216
);

proof_case!(
    astra_accrue_positive_rate_supply_217,
    astra_accrue_positive_rate_fixed_supply,
    217
);

proof_case!(
    astra_accrue_positive_rate_supply_218,
    astra_accrue_positive_rate_fixed_supply,
    218
);

proof_case!(
    astra_accrue_positive_rate_supply_219,
    astra_accrue_positive_rate_fixed_supply,
    219
);

proof_case!(
    astra_accrue_positive_rate_supply_220,
    astra_accrue_positive_rate_fixed_supply,
    220
);

proof_case!(
    astra_accrue_positive_rate_supply_221,
    astra_accrue_positive_rate_fixed_supply,
    221
);

proof_case!(
    astra_accrue_positive_rate_supply_222,
    astra_accrue_positive_rate_fixed_supply,
    222
);

proof_case!(
    astra_accrue_positive_rate_supply_223,
    astra_accrue_positive_rate_fixed_supply,
    223
);

proof_case!(
    astra_accrue_positive_rate_supply_224,
    astra_accrue_positive_rate_fixed_supply,
    224
);

proof_case!(
    astra_accrue_positive_rate_supply_225,
    astra_accrue_positive_rate_fixed_supply,
    225
);

proof_case!(
    astra_accrue_positive_rate_supply_226,
    astra_accrue_positive_rate_fixed_supply,
    226
);

proof_case!(
    astra_accrue_positive_rate_supply_227,
    astra_accrue_positive_rate_fixed_supply,
    227
);

proof_case!(
    astra_accrue_positive_rate_supply_228,
    astra_accrue_positive_rate_fixed_supply,
    228
);

proof_case!(
    astra_accrue_positive_rate_supply_229,
    astra_accrue_positive_rate_fixed_supply,
    229
);

proof_case!(
    astra_accrue_positive_rate_supply_230,
    astra_accrue_positive_rate_fixed_supply,
    230
);

proof_case!(
    astra_accrue_positive_rate_supply_231,
    astra_accrue_positive_rate_fixed_supply,
    231
);

proof_case!(
    astra_accrue_positive_rate_supply_232,
    astra_accrue_positive_rate_fixed_supply,
    232
);

proof_case!(
    astra_accrue_positive_rate_supply_233,
    astra_accrue_positive_rate_fixed_supply,
    233
);

proof_case!(
    astra_accrue_positive_rate_supply_234,
    astra_accrue_positive_rate_fixed_supply,
    234
);

proof_case!(
    astra_accrue_positive_rate_supply_235,
    astra_accrue_positive_rate_fixed_supply,
    235
);

proof_case!(
    astra_accrue_positive_rate_supply_236,
    astra_accrue_positive_rate_fixed_supply,
    236
);

proof_case!(
    astra_accrue_positive_rate_supply_237,
    astra_accrue_positive_rate_fixed_supply,
    237
);

proof_case!(
    astra_accrue_positive_rate_supply_238,
    astra_accrue_positive_rate_fixed_supply,
    238
);

proof_case!(
    astra_accrue_positive_rate_supply_239,
    astra_accrue_positive_rate_fixed_supply,
    239
);

proof_case!(
    astra_accrue_positive_rate_supply_240,
    astra_accrue_positive_rate_fixed_supply,
    240
);

proof_case!(
    astra_accrue_positive_rate_supply_241,
    astra_accrue_positive_rate_fixed_supply,
    241
);

proof_case!(
    astra_accrue_positive_rate_supply_242,
    astra_accrue_positive_rate_fixed_supply,
    242
);

proof_case!(
    astra_accrue_positive_rate_supply_243,
    astra_accrue_positive_rate_fixed_supply,
    243
);

proof_case!(
    astra_accrue_positive_rate_supply_244,
    astra_accrue_positive_rate_fixed_supply,
    244
);

proof_case!(
    astra_accrue_positive_rate_supply_245,
    astra_accrue_positive_rate_fixed_supply,
    245
);

proof_case!(
    astra_accrue_positive_rate_supply_246,
    astra_accrue_positive_rate_fixed_supply,
    246
);

proof_case!(
    astra_accrue_positive_rate_supply_247,
    astra_accrue_positive_rate_fixed_supply,
    247
);

proof_case!(
    astra_accrue_positive_rate_supply_248,
    astra_accrue_positive_rate_fixed_supply,
    248
);

proof_case!(
    astra_accrue_positive_rate_supply_249,
    astra_accrue_positive_rate_fixed_supply,
    249
);

proof_case!(
    astra_accrue_positive_rate_supply_250,
    astra_accrue_positive_rate_fixed_supply,
    250
);

proof_case!(
    astra_accrue_positive_rate_supply_251,
    astra_accrue_positive_rate_fixed_supply,
    251
);

proof_case!(
    astra_accrue_positive_rate_supply_252,
    astra_accrue_positive_rate_fixed_supply,
    252
);

proof_case!(
    astra_accrue_positive_rate_supply_253,
    astra_accrue_positive_rate_fixed_supply,
    253
);

proof_case!(
    astra_accrue_positive_rate_supply_254,
    astra_accrue_positive_rate_fixed_supply,
    254
);

proof_case!(
    astra_accrue_positive_accounting_supply_002,
    astra_accrue_positive_accounting_fixed_supply,
    2
);

proof_case!(
    astra_accrue_positive_accounting_supply_004,
    astra_accrue_positive_accounting_fixed_supply,
    4
);

proof_case!(
    astra_accrue_positive_accounting_supply_005,
    astra_accrue_positive_accounting_fixed_supply,
    5
);

proof_case!(
    astra_accrue_positive_accounting_supply_006,
    astra_accrue_positive_accounting_fixed_supply,
    6
);

proof_case!(
    astra_accrue_positive_accounting_supply_007,
    astra_accrue_positive_accounting_fixed_supply,
    7
);

proof_case!(
    astra_accrue_positive_accounting_supply_008,
    astra_accrue_positive_accounting_fixed_supply,
    8
);

proof_case!(
    astra_accrue_positive_accounting_supply_009,
    astra_accrue_positive_accounting_fixed_supply,
    9
);

proof_case!(
    astra_accrue_positive_accounting_supply_010,
    astra_accrue_positive_accounting_fixed_supply,
    10
);

proof_case!(
    astra_accrue_positive_accounting_supply_011,
    astra_accrue_positive_accounting_fixed_supply,
    11
);

proof_case!(
    astra_accrue_positive_accounting_supply_012,
    astra_accrue_positive_accounting_fixed_supply,
    12
);

proof_case!(
    astra_accrue_positive_accounting_supply_013,
    astra_accrue_positive_accounting_fixed_supply,
    13
);

proof_case!(
    astra_accrue_positive_accounting_supply_014,
    astra_accrue_positive_accounting_fixed_supply,
    14
);

proof_case!(
    astra_accrue_positive_accounting_supply_015,
    astra_accrue_positive_accounting_fixed_supply,
    15
);

proof_case!(
    astra_accrue_positive_accounting_supply_016,
    astra_accrue_positive_accounting_fixed_supply,
    16
);

proof_case!(
    astra_accrue_positive_accounting_supply_017,
    astra_accrue_positive_accounting_fixed_supply,
    17
);

proof_case!(
    astra_accrue_positive_accounting_supply_018,
    astra_accrue_positive_accounting_fixed_supply,
    18
);

proof_case!(
    astra_accrue_positive_accounting_supply_019,
    astra_accrue_positive_accounting_fixed_supply,
    19
);

proof_case!(
    astra_accrue_positive_accounting_supply_020,
    astra_accrue_positive_accounting_fixed_supply,
    20
);

proof_case!(
    astra_accrue_positive_accounting_supply_021,
    astra_accrue_positive_accounting_fixed_supply,
    21
);

proof_case!(
    astra_accrue_positive_accounting_supply_022,
    astra_accrue_positive_accounting_fixed_supply,
    22
);

proof_case!(
    astra_accrue_positive_accounting_supply_023,
    astra_accrue_positive_accounting_fixed_supply,
    23
);

proof_case!(
    astra_accrue_positive_accounting_supply_024,
    astra_accrue_positive_accounting_fixed_supply,
    24
);

proof_case!(
    astra_accrue_positive_accounting_supply_025,
    astra_accrue_positive_accounting_fixed_supply,
    25
);

proof_case!(
    astra_accrue_positive_accounting_supply_026,
    astra_accrue_positive_accounting_fixed_supply,
    26
);

proof_case!(
    astra_accrue_positive_accounting_supply_027,
    astra_accrue_positive_accounting_fixed_supply,
    27
);

proof_case!(
    astra_accrue_positive_accounting_supply_028,
    astra_accrue_positive_accounting_fixed_supply,
    28
);

proof_case!(
    astra_accrue_positive_accounting_supply_029,
    astra_accrue_positive_accounting_fixed_supply,
    29
);

proof_case!(
    astra_accrue_positive_accounting_supply_030,
    astra_accrue_positive_accounting_fixed_supply,
    30
);

proof_case!(
    astra_accrue_positive_accounting_supply_031,
    astra_accrue_positive_accounting_fixed_supply,
    31
);

proof_case!(
    astra_accrue_positive_accounting_supply_032,
    astra_accrue_positive_accounting_fixed_supply,
    32
);

proof_case!(
    astra_accrue_positive_accounting_supply_033,
    astra_accrue_positive_accounting_fixed_supply,
    33
);

proof_case!(
    astra_accrue_positive_accounting_supply_034,
    astra_accrue_positive_accounting_fixed_supply,
    34
);

proof_case!(
    astra_accrue_positive_accounting_supply_035,
    astra_accrue_positive_accounting_fixed_supply,
    35
);

proof_case!(
    astra_accrue_positive_accounting_supply_036,
    astra_accrue_positive_accounting_fixed_supply,
    36
);

proof_case!(
    astra_accrue_positive_accounting_supply_037,
    astra_accrue_positive_accounting_fixed_supply,
    37
);

proof_case!(
    astra_accrue_positive_accounting_supply_038,
    astra_accrue_positive_accounting_fixed_supply,
    38
);

proof_case!(
    astra_accrue_positive_accounting_supply_039,
    astra_accrue_positive_accounting_fixed_supply,
    39
);

proof_case!(
    astra_accrue_positive_accounting_supply_040,
    astra_accrue_positive_accounting_fixed_supply,
    40
);

proof_case!(
    astra_accrue_positive_accounting_supply_041,
    astra_accrue_positive_accounting_fixed_supply,
    41
);

proof_case!(
    astra_accrue_positive_accounting_supply_042,
    astra_accrue_positive_accounting_fixed_supply,
    42
);

proof_case!(
    astra_accrue_positive_accounting_supply_043,
    astra_accrue_positive_accounting_fixed_supply,
    43
);

proof_case!(
    astra_accrue_positive_accounting_supply_044,
    astra_accrue_positive_accounting_fixed_supply,
    44
);

proof_case!(
    astra_accrue_positive_accounting_supply_045,
    astra_accrue_positive_accounting_fixed_supply,
    45
);

proof_case!(
    astra_accrue_positive_accounting_supply_046,
    astra_accrue_positive_accounting_fixed_supply,
    46
);

proof_case!(
    astra_accrue_positive_accounting_supply_047,
    astra_accrue_positive_accounting_fixed_supply,
    47
);

proof_case!(
    astra_accrue_positive_accounting_supply_048,
    astra_accrue_positive_accounting_fixed_supply,
    48
);

proof_case!(
    astra_accrue_positive_accounting_supply_049,
    astra_accrue_positive_accounting_fixed_supply,
    49
);

proof_case!(
    astra_accrue_positive_accounting_supply_050,
    astra_accrue_positive_accounting_fixed_supply,
    50
);

proof_case!(
    astra_accrue_positive_accounting_supply_051,
    astra_accrue_positive_accounting_fixed_supply,
    51
);

proof_case!(
    astra_accrue_positive_accounting_supply_052,
    astra_accrue_positive_accounting_fixed_supply,
    52
);

proof_case!(
    astra_accrue_positive_accounting_supply_053,
    astra_accrue_positive_accounting_fixed_supply,
    53
);

proof_case!(
    astra_accrue_positive_accounting_supply_054,
    astra_accrue_positive_accounting_fixed_supply,
    54
);

proof_case!(
    astra_accrue_positive_accounting_supply_055,
    astra_accrue_positive_accounting_fixed_supply,
    55
);

proof_case!(
    astra_accrue_positive_accounting_supply_056,
    astra_accrue_positive_accounting_fixed_supply,
    56
);

proof_case!(
    astra_accrue_positive_accounting_supply_057,
    astra_accrue_positive_accounting_fixed_supply,
    57
);

proof_case!(
    astra_accrue_positive_accounting_supply_058,
    astra_accrue_positive_accounting_fixed_supply,
    58
);

proof_case!(
    astra_accrue_positive_accounting_supply_059,
    astra_accrue_positive_accounting_fixed_supply,
    59
);

proof_case!(
    astra_accrue_positive_accounting_supply_060,
    astra_accrue_positive_accounting_fixed_supply,
    60
);

proof_case!(
    astra_accrue_positive_accounting_supply_061,
    astra_accrue_positive_accounting_fixed_supply,
    61
);

proof_case!(
    astra_accrue_positive_accounting_supply_062,
    astra_accrue_positive_accounting_fixed_supply,
    62
);

proof_case!(
    astra_accrue_positive_accounting_supply_063,
    astra_accrue_positive_accounting_fixed_supply,
    63
);

proof_case!(
    astra_accrue_positive_accounting_supply_064,
    astra_accrue_positive_accounting_fixed_supply,
    64
);

proof_case!(
    astra_accrue_positive_accounting_supply_065,
    astra_accrue_positive_accounting_fixed_supply,
    65
);

proof_case!(
    astra_accrue_positive_accounting_supply_066,
    astra_accrue_positive_accounting_fixed_supply,
    66
);

proof_case!(
    astra_accrue_positive_accounting_supply_067,
    astra_accrue_positive_accounting_fixed_supply,
    67
);

proof_case!(
    astra_accrue_positive_accounting_supply_068,
    astra_accrue_positive_accounting_fixed_supply,
    68
);

proof_case!(
    astra_accrue_positive_accounting_supply_069,
    astra_accrue_positive_accounting_fixed_supply,
    69
);

proof_case!(
    astra_accrue_positive_accounting_supply_070,
    astra_accrue_positive_accounting_fixed_supply,
    70
);

proof_case!(
    astra_accrue_positive_accounting_supply_071,
    astra_accrue_positive_accounting_fixed_supply,
    71
);

proof_case!(
    astra_accrue_positive_accounting_supply_072,
    astra_accrue_positive_accounting_fixed_supply,
    72
);

proof_case!(
    astra_accrue_positive_accounting_supply_073,
    astra_accrue_positive_accounting_fixed_supply,
    73
);

proof_case!(
    astra_accrue_positive_accounting_supply_074,
    astra_accrue_positive_accounting_fixed_supply,
    74
);

proof_case!(
    astra_accrue_positive_accounting_supply_075,
    astra_accrue_positive_accounting_fixed_supply,
    75
);

proof_case!(
    astra_accrue_positive_accounting_supply_076,
    astra_accrue_positive_accounting_fixed_supply,
    76
);

proof_case!(
    astra_accrue_positive_accounting_supply_077,
    astra_accrue_positive_accounting_fixed_supply,
    77
);

proof_case!(
    astra_accrue_positive_accounting_supply_078,
    astra_accrue_positive_accounting_fixed_supply,
    78
);

proof_case!(
    astra_accrue_positive_accounting_supply_079,
    astra_accrue_positive_accounting_fixed_supply,
    79
);

proof_case!(
    astra_accrue_positive_accounting_supply_080,
    astra_accrue_positive_accounting_fixed_supply,
    80
);

proof_case!(
    astra_accrue_positive_accounting_supply_081,
    astra_accrue_positive_accounting_fixed_supply,
    81
);

proof_case!(
    astra_accrue_positive_accounting_supply_082,
    astra_accrue_positive_accounting_fixed_supply,
    82
);

proof_case!(
    astra_accrue_positive_accounting_supply_083,
    astra_accrue_positive_accounting_fixed_supply,
    83
);

proof_case!(
    astra_accrue_positive_accounting_supply_084,
    astra_accrue_positive_accounting_fixed_supply,
    84
);

proof_case!(
    astra_accrue_positive_accounting_supply_085,
    astra_accrue_positive_accounting_fixed_supply,
    85
);

proof_case!(
    astra_accrue_positive_accounting_supply_086,
    astra_accrue_positive_accounting_fixed_supply,
    86
);

proof_case!(
    astra_accrue_positive_accounting_supply_087,
    astra_accrue_positive_accounting_fixed_supply,
    87
);

proof_case!(
    astra_accrue_positive_accounting_supply_088,
    astra_accrue_positive_accounting_fixed_supply,
    88
);

proof_case!(
    astra_accrue_positive_accounting_supply_089,
    astra_accrue_positive_accounting_fixed_supply,
    89
);

proof_case!(
    astra_accrue_positive_accounting_supply_090,
    astra_accrue_positive_accounting_fixed_supply,
    90
);

proof_case!(
    astra_accrue_positive_accounting_supply_091,
    astra_accrue_positive_accounting_fixed_supply,
    91
);

proof_case!(
    astra_accrue_positive_accounting_supply_092,
    astra_accrue_positive_accounting_fixed_supply,
    92
);

proof_case!(
    astra_accrue_positive_accounting_supply_093,
    astra_accrue_positive_accounting_fixed_supply,
    93
);

proof_case!(
    astra_accrue_positive_accounting_supply_094,
    astra_accrue_positive_accounting_fixed_supply,
    94
);

proof_case!(
    astra_accrue_positive_accounting_supply_095,
    astra_accrue_positive_accounting_fixed_supply,
    95
);

proof_case!(
    astra_accrue_positive_accounting_supply_096,
    astra_accrue_positive_accounting_fixed_supply,
    96
);

proof_case!(
    astra_accrue_positive_accounting_supply_097,
    astra_accrue_positive_accounting_fixed_supply,
    97
);

proof_case!(
    astra_accrue_positive_accounting_supply_098,
    astra_accrue_positive_accounting_fixed_supply,
    98
);

proof_case!(
    astra_accrue_positive_accounting_supply_099,
    astra_accrue_positive_accounting_fixed_supply,
    99
);

proof_case!(
    astra_accrue_positive_accounting_supply_100,
    astra_accrue_positive_accounting_fixed_supply,
    100
);

proof_case!(
    astra_accrue_positive_accounting_supply_101,
    astra_accrue_positive_accounting_fixed_supply,
    101
);

proof_case!(
    astra_accrue_positive_accounting_supply_102,
    astra_accrue_positive_accounting_fixed_supply,
    102
);

proof_case!(
    astra_accrue_positive_accounting_supply_103,
    astra_accrue_positive_accounting_fixed_supply,
    103
);

proof_case!(
    astra_accrue_positive_accounting_supply_104,
    astra_accrue_positive_accounting_fixed_supply,
    104
);

proof_case!(
    astra_accrue_positive_accounting_supply_105,
    astra_accrue_positive_accounting_fixed_supply,
    105
);

proof_case!(
    astra_accrue_positive_accounting_supply_106,
    astra_accrue_positive_accounting_fixed_supply,
    106
);

proof_case!(
    astra_accrue_positive_accounting_supply_107,
    astra_accrue_positive_accounting_fixed_supply,
    107
);

proof_case!(
    astra_accrue_positive_accounting_supply_108,
    astra_accrue_positive_accounting_fixed_supply,
    108
);

proof_case!(
    astra_accrue_positive_accounting_supply_109,
    astra_accrue_positive_accounting_fixed_supply,
    109
);

proof_case!(
    astra_accrue_positive_accounting_supply_110,
    astra_accrue_positive_accounting_fixed_supply,
    110
);

proof_case!(
    astra_accrue_positive_accounting_supply_111,
    astra_accrue_positive_accounting_fixed_supply,
    111
);

proof_case!(
    astra_accrue_positive_accounting_supply_112,
    astra_accrue_positive_accounting_fixed_supply,
    112
);

proof_case!(
    astra_accrue_positive_accounting_supply_113,
    astra_accrue_positive_accounting_fixed_supply,
    113
);

proof_case!(
    astra_accrue_positive_accounting_supply_114,
    astra_accrue_positive_accounting_fixed_supply,
    114
);

proof_case!(
    astra_accrue_positive_accounting_supply_115,
    astra_accrue_positive_accounting_fixed_supply,
    115
);

proof_case!(
    astra_accrue_positive_accounting_supply_116,
    astra_accrue_positive_accounting_fixed_supply,
    116
);

proof_case!(
    astra_accrue_positive_accounting_supply_117,
    astra_accrue_positive_accounting_fixed_supply,
    117
);

proof_case!(
    astra_accrue_positive_accounting_supply_118,
    astra_accrue_positive_accounting_fixed_supply,
    118
);

proof_case!(
    astra_accrue_positive_accounting_supply_119,
    astra_accrue_positive_accounting_fixed_supply,
    119
);

proof_case!(
    astra_accrue_positive_accounting_supply_120,
    astra_accrue_positive_accounting_fixed_supply,
    120
);

proof_case!(
    astra_accrue_positive_accounting_supply_121,
    astra_accrue_positive_accounting_fixed_supply,
    121
);

proof_case!(
    astra_accrue_positive_accounting_supply_122,
    astra_accrue_positive_accounting_fixed_supply,
    122
);

proof_case!(
    astra_accrue_positive_accounting_supply_123,
    astra_accrue_positive_accounting_fixed_supply,
    123
);

proof_case!(
    astra_accrue_positive_accounting_supply_124,
    astra_accrue_positive_accounting_fixed_supply,
    124
);

proof_case!(
    astra_accrue_positive_accounting_supply_125,
    astra_accrue_positive_accounting_fixed_supply,
    125
);

proof_case!(
    astra_accrue_positive_accounting_supply_126,
    astra_accrue_positive_accounting_fixed_supply,
    126
);

proof_case!(
    astra_accrue_positive_accounting_supply_127,
    astra_accrue_positive_accounting_fixed_supply,
    127
);

proof_case!(
    astra_accrue_positive_accounting_supply_128,
    astra_accrue_positive_accounting_fixed_supply,
    128
);

proof_case!(
    astra_accrue_positive_accounting_supply_129,
    astra_accrue_positive_accounting_fixed_supply,
    129
);

proof_case!(
    astra_accrue_positive_accounting_supply_130,
    astra_accrue_positive_accounting_fixed_supply,
    130
);

proof_case!(
    astra_accrue_positive_accounting_supply_131,
    astra_accrue_positive_accounting_fixed_supply,
    131
);

proof_case!(
    astra_accrue_positive_accounting_supply_132,
    astra_accrue_positive_accounting_fixed_supply,
    132
);

proof_case!(
    astra_accrue_positive_accounting_supply_133,
    astra_accrue_positive_accounting_fixed_supply,
    133
);

proof_case!(
    astra_accrue_positive_accounting_supply_134,
    astra_accrue_positive_accounting_fixed_supply,
    134
);

proof_case!(
    astra_accrue_positive_accounting_supply_135,
    astra_accrue_positive_accounting_fixed_supply,
    135
);

proof_case!(
    astra_accrue_positive_accounting_supply_136,
    astra_accrue_positive_accounting_fixed_supply,
    136
);

proof_case!(
    astra_accrue_positive_accounting_supply_137,
    astra_accrue_positive_accounting_fixed_supply,
    137
);

proof_case!(
    astra_accrue_positive_accounting_supply_138,
    astra_accrue_positive_accounting_fixed_supply,
    138
);

proof_case!(
    astra_accrue_positive_accounting_supply_139,
    astra_accrue_positive_accounting_fixed_supply,
    139
);

proof_case!(
    astra_accrue_positive_accounting_supply_140,
    astra_accrue_positive_accounting_fixed_supply,
    140
);

proof_case!(
    astra_accrue_positive_accounting_supply_141,
    astra_accrue_positive_accounting_fixed_supply,
    141
);

proof_case!(
    astra_accrue_positive_accounting_supply_142,
    astra_accrue_positive_accounting_fixed_supply,
    142
);

proof_case!(
    astra_accrue_positive_accounting_supply_143,
    astra_accrue_positive_accounting_fixed_supply,
    143
);

proof_case!(
    astra_accrue_positive_accounting_supply_144,
    astra_accrue_positive_accounting_fixed_supply,
    144
);

proof_case!(
    astra_accrue_positive_accounting_supply_145,
    astra_accrue_positive_accounting_fixed_supply,
    145
);

proof_case!(
    astra_accrue_positive_accounting_supply_146,
    astra_accrue_positive_accounting_fixed_supply,
    146
);

proof_case!(
    astra_accrue_positive_accounting_supply_147,
    astra_accrue_positive_accounting_fixed_supply,
    147
);

proof_case!(
    astra_accrue_positive_accounting_supply_148,
    astra_accrue_positive_accounting_fixed_supply,
    148
);

proof_case!(
    astra_accrue_positive_accounting_supply_149,
    astra_accrue_positive_accounting_fixed_supply,
    149
);

proof_case!(
    astra_accrue_positive_accounting_supply_150,
    astra_accrue_positive_accounting_fixed_supply,
    150
);

proof_case!(
    astra_accrue_positive_accounting_supply_151,
    astra_accrue_positive_accounting_fixed_supply,
    151
);

proof_case!(
    astra_accrue_positive_accounting_supply_152,
    astra_accrue_positive_accounting_fixed_supply,
    152
);

proof_case!(
    astra_accrue_positive_accounting_supply_153,
    astra_accrue_positive_accounting_fixed_supply,
    153
);

proof_case!(
    astra_accrue_positive_accounting_supply_154,
    astra_accrue_positive_accounting_fixed_supply,
    154
);

proof_case!(
    astra_accrue_positive_accounting_supply_155,
    astra_accrue_positive_accounting_fixed_supply,
    155
);

proof_case!(
    astra_accrue_positive_accounting_supply_156,
    astra_accrue_positive_accounting_fixed_supply,
    156
);

proof_case!(
    astra_accrue_positive_accounting_supply_157,
    astra_accrue_positive_accounting_fixed_supply,
    157
);

proof_case!(
    astra_accrue_positive_accounting_supply_158,
    astra_accrue_positive_accounting_fixed_supply,
    158
);

proof_case!(
    astra_accrue_positive_accounting_supply_159,
    astra_accrue_positive_accounting_fixed_supply,
    159
);

proof_case!(
    astra_accrue_positive_accounting_supply_160,
    astra_accrue_positive_accounting_fixed_supply,
    160
);

proof_case!(
    astra_accrue_positive_accounting_supply_161,
    astra_accrue_positive_accounting_fixed_supply,
    161
);

proof_case!(
    astra_accrue_positive_accounting_supply_162,
    astra_accrue_positive_accounting_fixed_supply,
    162
);

proof_case!(
    astra_accrue_positive_accounting_supply_163,
    astra_accrue_positive_accounting_fixed_supply,
    163
);

proof_case!(
    astra_accrue_positive_accounting_supply_164,
    astra_accrue_positive_accounting_fixed_supply,
    164
);

proof_case!(
    astra_accrue_positive_accounting_supply_165,
    astra_accrue_positive_accounting_fixed_supply,
    165
);

proof_case!(
    astra_accrue_positive_accounting_supply_166,
    astra_accrue_positive_accounting_fixed_supply,
    166
);

proof_case!(
    astra_accrue_positive_accounting_supply_167,
    astra_accrue_positive_accounting_fixed_supply,
    167
);

proof_case!(
    astra_accrue_positive_accounting_supply_168,
    astra_accrue_positive_accounting_fixed_supply,
    168
);

proof_case!(
    astra_accrue_positive_accounting_supply_169,
    astra_accrue_positive_accounting_fixed_supply,
    169
);

proof_case!(
    astra_accrue_positive_accounting_supply_170,
    astra_accrue_positive_accounting_fixed_supply,
    170
);

proof_case!(
    astra_accrue_positive_accounting_supply_171,
    astra_accrue_positive_accounting_fixed_supply,
    171
);

proof_case!(
    astra_accrue_positive_accounting_supply_172,
    astra_accrue_positive_accounting_fixed_supply,
    172
);

proof_case!(
    astra_accrue_positive_accounting_supply_173,
    astra_accrue_positive_accounting_fixed_supply,
    173
);

proof_case!(
    astra_accrue_positive_accounting_supply_174,
    astra_accrue_positive_accounting_fixed_supply,
    174
);

proof_case!(
    astra_accrue_positive_accounting_supply_175,
    astra_accrue_positive_accounting_fixed_supply,
    175
);

proof_case!(
    astra_accrue_positive_accounting_supply_176,
    astra_accrue_positive_accounting_fixed_supply,
    176
);

proof_case!(
    astra_accrue_positive_accounting_supply_177,
    astra_accrue_positive_accounting_fixed_supply,
    177
);

proof_case!(
    astra_accrue_positive_accounting_supply_178,
    astra_accrue_positive_accounting_fixed_supply,
    178
);

proof_case!(
    astra_accrue_positive_accounting_supply_179,
    astra_accrue_positive_accounting_fixed_supply,
    179
);

proof_case!(
    astra_accrue_positive_accounting_supply_180,
    astra_accrue_positive_accounting_fixed_supply,
    180
);

proof_case!(
    astra_accrue_positive_accounting_supply_181,
    astra_accrue_positive_accounting_fixed_supply,
    181
);

proof_case!(
    astra_accrue_positive_accounting_supply_182,
    astra_accrue_positive_accounting_fixed_supply,
    182
);

proof_case!(
    astra_accrue_positive_accounting_supply_183,
    astra_accrue_positive_accounting_fixed_supply,
    183
);

proof_case!(
    astra_accrue_positive_accounting_supply_184,
    astra_accrue_positive_accounting_fixed_supply,
    184
);

proof_case!(
    astra_accrue_positive_accounting_supply_185,
    astra_accrue_positive_accounting_fixed_supply,
    185
);

proof_case!(
    astra_accrue_positive_accounting_supply_186,
    astra_accrue_positive_accounting_fixed_supply,
    186
);

proof_case!(
    astra_accrue_positive_accounting_supply_187,
    astra_accrue_positive_accounting_fixed_supply,
    187
);

proof_case!(
    astra_accrue_positive_accounting_supply_188,
    astra_accrue_positive_accounting_fixed_supply,
    188
);

proof_case!(
    astra_accrue_positive_accounting_supply_189,
    astra_accrue_positive_accounting_fixed_supply,
    189
);

proof_case!(
    astra_accrue_positive_accounting_supply_190,
    astra_accrue_positive_accounting_fixed_supply,
    190
);

proof_case!(
    astra_accrue_positive_accounting_supply_191,
    astra_accrue_positive_accounting_fixed_supply,
    191
);

proof_case!(
    astra_accrue_positive_accounting_supply_192,
    astra_accrue_positive_accounting_fixed_supply,
    192
);

proof_case!(
    astra_accrue_positive_accounting_supply_193,
    astra_accrue_positive_accounting_fixed_supply,
    193
);

proof_case!(
    astra_accrue_positive_accounting_supply_194,
    astra_accrue_positive_accounting_fixed_supply,
    194
);

proof_case!(
    astra_accrue_positive_accounting_supply_195,
    astra_accrue_positive_accounting_fixed_supply,
    195
);

proof_case!(
    astra_accrue_positive_accounting_supply_196,
    astra_accrue_positive_accounting_fixed_supply,
    196
);

proof_case!(
    astra_accrue_positive_accounting_supply_197,
    astra_accrue_positive_accounting_fixed_supply,
    197
);

proof_case!(
    astra_accrue_positive_accounting_supply_198,
    astra_accrue_positive_accounting_fixed_supply,
    198
);

proof_case!(
    astra_accrue_positive_accounting_supply_199,
    astra_accrue_positive_accounting_fixed_supply,
    199
);

proof_case!(
    astra_accrue_positive_accounting_supply_200,
    astra_accrue_positive_accounting_fixed_supply,
    200
);

proof_case!(
    astra_accrue_positive_accounting_supply_201,
    astra_accrue_positive_accounting_fixed_supply,
    201
);

proof_case!(
    astra_accrue_positive_accounting_supply_202,
    astra_accrue_positive_accounting_fixed_supply,
    202
);

proof_case!(
    astra_accrue_positive_accounting_supply_203,
    astra_accrue_positive_accounting_fixed_supply,
    203
);

proof_case!(
    astra_accrue_positive_accounting_supply_204,
    astra_accrue_positive_accounting_fixed_supply,
    204
);

proof_case!(
    astra_accrue_positive_accounting_supply_205,
    astra_accrue_positive_accounting_fixed_supply,
    205
);

proof_case!(
    astra_accrue_positive_accounting_supply_206,
    astra_accrue_positive_accounting_fixed_supply,
    206
);

proof_case!(
    astra_accrue_positive_accounting_supply_207,
    astra_accrue_positive_accounting_fixed_supply,
    207
);

proof_case!(
    astra_accrue_positive_accounting_supply_208,
    astra_accrue_positive_accounting_fixed_supply,
    208
);

proof_case!(
    astra_accrue_positive_accounting_supply_209,
    astra_accrue_positive_accounting_fixed_supply,
    209
);

proof_case!(
    astra_accrue_positive_accounting_supply_210,
    astra_accrue_positive_accounting_fixed_supply,
    210
);

proof_case!(
    astra_accrue_positive_accounting_supply_211,
    astra_accrue_positive_accounting_fixed_supply,
    211
);

proof_case!(
    astra_accrue_positive_accounting_supply_212,
    astra_accrue_positive_accounting_fixed_supply,
    212
);

proof_case!(
    astra_accrue_positive_accounting_supply_213,
    astra_accrue_positive_accounting_fixed_supply,
    213
);

proof_case!(
    astra_accrue_positive_accounting_supply_214,
    astra_accrue_positive_accounting_fixed_supply,
    214
);

proof_case!(
    astra_accrue_positive_accounting_supply_215,
    astra_accrue_positive_accounting_fixed_supply,
    215
);

proof_case!(
    astra_accrue_positive_accounting_supply_216,
    astra_accrue_positive_accounting_fixed_supply,
    216
);

proof_case!(
    astra_accrue_positive_accounting_supply_217,
    astra_accrue_positive_accounting_fixed_supply,
    217
);

proof_case!(
    astra_accrue_positive_accounting_supply_218,
    astra_accrue_positive_accounting_fixed_supply,
    218
);

proof_case!(
    astra_accrue_positive_accounting_supply_219,
    astra_accrue_positive_accounting_fixed_supply,
    219
);

proof_case!(
    astra_accrue_positive_accounting_supply_220,
    astra_accrue_positive_accounting_fixed_supply,
    220
);

proof_case!(
    astra_accrue_positive_accounting_supply_221,
    astra_accrue_positive_accounting_fixed_supply,
    221
);

proof_case!(
    astra_accrue_positive_accounting_supply_222,
    astra_accrue_positive_accounting_fixed_supply,
    222
);

proof_case!(
    astra_accrue_positive_accounting_supply_223,
    astra_accrue_positive_accounting_fixed_supply,
    223
);

proof_case!(
    astra_accrue_positive_accounting_supply_224,
    astra_accrue_positive_accounting_fixed_supply,
    224
);

proof_case!(
    astra_accrue_positive_accounting_supply_225,
    astra_accrue_positive_accounting_fixed_supply,
    225
);

proof_case!(
    astra_accrue_positive_accounting_supply_226,
    astra_accrue_positive_accounting_fixed_supply,
    226
);

proof_case!(
    astra_accrue_positive_accounting_supply_227,
    astra_accrue_positive_accounting_fixed_supply,
    227
);

proof_case!(
    astra_accrue_positive_accounting_supply_228,
    astra_accrue_positive_accounting_fixed_supply,
    228
);

proof_case!(
    astra_accrue_positive_accounting_supply_229,
    astra_accrue_positive_accounting_fixed_supply,
    229
);

proof_case!(
    astra_accrue_positive_accounting_supply_230,
    astra_accrue_positive_accounting_fixed_supply,
    230
);

proof_case!(
    astra_accrue_positive_accounting_supply_231,
    astra_accrue_positive_accounting_fixed_supply,
    231
);

proof_case!(
    astra_accrue_positive_accounting_supply_232,
    astra_accrue_positive_accounting_fixed_supply,
    232
);

proof_case!(
    astra_accrue_positive_accounting_supply_233,
    astra_accrue_positive_accounting_fixed_supply,
    233
);

proof_case!(
    astra_accrue_positive_accounting_supply_234,
    astra_accrue_positive_accounting_fixed_supply,
    234
);

proof_case!(
    astra_accrue_positive_accounting_supply_235,
    astra_accrue_positive_accounting_fixed_supply,
    235
);

proof_case!(
    astra_accrue_positive_accounting_supply_236,
    astra_accrue_positive_accounting_fixed_supply,
    236
);

proof_case!(
    astra_accrue_positive_accounting_supply_237,
    astra_accrue_positive_accounting_fixed_supply,
    237
);

proof_case!(
    astra_accrue_positive_accounting_supply_238,
    astra_accrue_positive_accounting_fixed_supply,
    238
);

proof_case!(
    astra_accrue_positive_accounting_supply_239,
    astra_accrue_positive_accounting_fixed_supply,
    239
);

proof_case!(
    astra_accrue_positive_accounting_supply_240,
    astra_accrue_positive_accounting_fixed_supply,
    240
);

proof_case!(
    astra_accrue_positive_accounting_supply_241,
    astra_accrue_positive_accounting_fixed_supply,
    241
);

proof_case!(
    astra_accrue_positive_accounting_supply_242,
    astra_accrue_positive_accounting_fixed_supply,
    242
);

proof_case!(
    astra_accrue_positive_accounting_supply_243,
    astra_accrue_positive_accounting_fixed_supply,
    243
);

proof_case!(
    astra_accrue_positive_accounting_supply_244,
    astra_accrue_positive_accounting_fixed_supply,
    244
);

proof_case!(
    astra_accrue_positive_accounting_supply_245,
    astra_accrue_positive_accounting_fixed_supply,
    245
);

proof_case!(
    astra_accrue_positive_accounting_supply_246,
    astra_accrue_positive_accounting_fixed_supply,
    246
);

proof_case!(
    astra_accrue_positive_accounting_supply_247,
    astra_accrue_positive_accounting_fixed_supply,
    247
);

proof_case!(
    astra_accrue_positive_accounting_supply_248,
    astra_accrue_positive_accounting_fixed_supply,
    248
);

proof_case!(
    astra_accrue_positive_accounting_supply_249,
    astra_accrue_positive_accounting_fixed_supply,
    249
);

proof_case!(
    astra_accrue_positive_accounting_supply_250,
    astra_accrue_positive_accounting_fixed_supply,
    250
);

proof_case!(
    astra_accrue_positive_accounting_supply_251,
    astra_accrue_positive_accounting_fixed_supply,
    251
);

proof_case!(
    astra_accrue_positive_accounting_supply_252,
    astra_accrue_positive_accounting_fixed_supply,
    252
);

proof_case!(
    astra_accrue_positive_accounting_supply_253,
    astra_accrue_positive_accounting_fixed_supply,
    253
);

proof_case!(
    astra_accrue_positive_accounting_supply_254,
    astra_accrue_positive_accounting_fixed_supply,
    254
);

// Liability conversion products only; not the larger rate-growth product.
#[kani::proof]
fn astra_debt_ceil_numerator_literal() {
    let n = kani::any::<u64>() as i128;
    kani::assume(n <= 1_658_137_500_000_000);
    assert_eq!(n.checked_mul(1), Some(n));
    let rounded_n = n.checked_add(SCALAR_12 - 1).unwrap();
    let q = FixedMath::ceil(&(), n, 1, SCALAR_12);
    assert!(q >= 0);
    assert_eq!(q, rounded_n / SCALAR_12);
    kani::cover!(n == 0, "zero numerator");
    kani::cover!(n == SCALAR_12, "positive exact quotient");
    kani::cover!(n == SCALAR_12 + 1, "positive nonzero remainder");
    kani::cover!(n == 1_658_137_500_000_000, "maximum liability product");
}

#[kani::proof]
fn astra_debt_ceil_product_bridge() {
    let x = kani::any::<u8>() as i128;
    let y = kani::any::<u64>() as i128;
    kani::assume(y <= 6_502_500_000_000);
    let n = x.checked_mul(y).unwrap();
    assert!(0 <= n && n <= 1_658_137_500_000_000);
    assert_eq!(n.checked_mul(1), Some(n));
    assert_eq!(y.checked_mul(x), Some(n));
    assert_eq!(
        FixedMath::ceil(&(), x, y, SCALAR_12),
        FixedMath::ceil(&(), n, 1, SCALAR_12)
    );
    kani::cover!(x == 0, "zero supply");
    kani::cover!(x == 1 && y == SCALAR_12 + 1, "nonexact positive product");
    kani::cover!(x == 255 && y == 6_502_500_000_000, "maximum product");
}

// Exact fixed-x cell of astra_debt_ceil_product_bridge, not rate growth.
// Full-domain closure requires all 256 X cells; these wrappers are canaries.
fn astra_debt_ceil_product_bridge_fixed_x<const X: u8>() {
    let x = X as i128;
    let y = kani::any::<u64>() as i128;
    kani::assume(y <= 6_502_500_000_000);
    let n = x.checked_mul(y).unwrap();
    assert!(0 <= n && n <= 1_658_137_500_000_000);
    assert_eq!(n.checked_mul(1), Some(n));
    assert_eq!(y.checked_mul(x), Some(n));
    assert_eq!(
        FixedMath::ceil(&(), x, y, SCALAR_12),
        FixedMath::ceil(&(), n, 1, SCALAR_12)
    );
    kani::cover!(y == 0, "zero y in fixed-x cell");
    kani::cover!(y == 6_502_500_000_000, "maximum y in fixed-x cell");
}

proof_case!(
    astra_debt_ceil_product_bridge_x0,
    astra_debt_ceil_product_bridge_fixed_x,
    0
);

proof_case!(
    astra_debt_ceil_product_bridge_x1,
    astra_debt_ceil_product_bridge_fixed_x,
    1
);

proof_case!(
    astra_debt_ceil_product_bridge_x255,
    astra_debt_ceil_product_bridge_fixed_x,
    255
);

// X2..X254 wrappers: one proof per remaining u8 cell, byte-identical helper.
proof_case!(
    astra_debt_ceil_product_bridge_x2,
    astra_debt_ceil_product_bridge_fixed_x,
    2
);

proof_case!(
    astra_debt_ceil_product_bridge_x3,
    astra_debt_ceil_product_bridge_fixed_x,
    3
);

proof_case!(
    astra_debt_ceil_product_bridge_x4,
    astra_debt_ceil_product_bridge_fixed_x,
    4
);

proof_case!(
    astra_debt_ceil_product_bridge_x5,
    astra_debt_ceil_product_bridge_fixed_x,
    5
);

proof_case!(
    astra_debt_ceil_product_bridge_x6,
    astra_debt_ceil_product_bridge_fixed_x,
    6
);

proof_case!(
    astra_debt_ceil_product_bridge_x7,
    astra_debt_ceil_product_bridge_fixed_x,
    7
);

proof_case!(
    astra_debt_ceil_product_bridge_x8,
    astra_debt_ceil_product_bridge_fixed_x,
    8
);

proof_case!(
    astra_debt_ceil_product_bridge_x9,
    astra_debt_ceil_product_bridge_fixed_x,
    9
);

proof_case!(
    astra_debt_ceil_product_bridge_x10,
    astra_debt_ceil_product_bridge_fixed_x,
    10
);

proof_case!(
    astra_debt_ceil_product_bridge_x11,
    astra_debt_ceil_product_bridge_fixed_x,
    11
);

proof_case!(
    astra_debt_ceil_product_bridge_x12,
    astra_debt_ceil_product_bridge_fixed_x,
    12
);

proof_case!(
    astra_debt_ceil_product_bridge_x13,
    astra_debt_ceil_product_bridge_fixed_x,
    13
);

proof_case!(
    astra_debt_ceil_product_bridge_x14,
    astra_debt_ceil_product_bridge_fixed_x,
    14
);

proof_case!(
    astra_debt_ceil_product_bridge_x15,
    astra_debt_ceil_product_bridge_fixed_x,
    15
);

proof_case!(
    astra_debt_ceil_product_bridge_x16,
    astra_debt_ceil_product_bridge_fixed_x,
    16
);

proof_case!(
    astra_debt_ceil_product_bridge_x17,
    astra_debt_ceil_product_bridge_fixed_x,
    17
);

proof_case!(
    astra_debt_ceil_product_bridge_x18,
    astra_debt_ceil_product_bridge_fixed_x,
    18
);

proof_case!(
    astra_debt_ceil_product_bridge_x19,
    astra_debt_ceil_product_bridge_fixed_x,
    19
);

proof_case!(
    astra_debt_ceil_product_bridge_x20,
    astra_debt_ceil_product_bridge_fixed_x,
    20
);

proof_case!(
    astra_debt_ceil_product_bridge_x21,
    astra_debt_ceil_product_bridge_fixed_x,
    21
);

proof_case!(
    astra_debt_ceil_product_bridge_x22,
    astra_debt_ceil_product_bridge_fixed_x,
    22
);

proof_case!(
    astra_debt_ceil_product_bridge_x23,
    astra_debt_ceil_product_bridge_fixed_x,
    23
);

proof_case!(
    astra_debt_ceil_product_bridge_x24,
    astra_debt_ceil_product_bridge_fixed_x,
    24
);

proof_case!(
    astra_debt_ceil_product_bridge_x25,
    astra_debt_ceil_product_bridge_fixed_x,
    25
);

proof_case!(
    astra_debt_ceil_product_bridge_x26,
    astra_debt_ceil_product_bridge_fixed_x,
    26
);

proof_case!(
    astra_debt_ceil_product_bridge_x27,
    astra_debt_ceil_product_bridge_fixed_x,
    27
);

proof_case!(
    astra_debt_ceil_product_bridge_x28,
    astra_debt_ceil_product_bridge_fixed_x,
    28
);

proof_case!(
    astra_debt_ceil_product_bridge_x29,
    astra_debt_ceil_product_bridge_fixed_x,
    29
);

proof_case!(
    astra_debt_ceil_product_bridge_x30,
    astra_debt_ceil_product_bridge_fixed_x,
    30
);

proof_case!(
    astra_debt_ceil_product_bridge_x31,
    astra_debt_ceil_product_bridge_fixed_x,
    31
);

proof_case!(
    astra_debt_ceil_product_bridge_x32,
    astra_debt_ceil_product_bridge_fixed_x,
    32
);

proof_case!(
    astra_debt_ceil_product_bridge_x33,
    astra_debt_ceil_product_bridge_fixed_x,
    33
);

proof_case!(
    astra_debt_ceil_product_bridge_x34,
    astra_debt_ceil_product_bridge_fixed_x,
    34
);

proof_case!(
    astra_debt_ceil_product_bridge_x35,
    astra_debt_ceil_product_bridge_fixed_x,
    35
);

proof_case!(
    astra_debt_ceil_product_bridge_x36,
    astra_debt_ceil_product_bridge_fixed_x,
    36
);

proof_case!(
    astra_debt_ceil_product_bridge_x37,
    astra_debt_ceil_product_bridge_fixed_x,
    37
);

proof_case!(
    astra_debt_ceil_product_bridge_x38,
    astra_debt_ceil_product_bridge_fixed_x,
    38
);

proof_case!(
    astra_debt_ceil_product_bridge_x39,
    astra_debt_ceil_product_bridge_fixed_x,
    39
);

proof_case!(
    astra_debt_ceil_product_bridge_x40,
    astra_debt_ceil_product_bridge_fixed_x,
    40
);

proof_case!(
    astra_debt_ceil_product_bridge_x41,
    astra_debt_ceil_product_bridge_fixed_x,
    41
);

proof_case!(
    astra_debt_ceil_product_bridge_x42,
    astra_debt_ceil_product_bridge_fixed_x,
    42
);

proof_case!(
    astra_debt_ceil_product_bridge_x43,
    astra_debt_ceil_product_bridge_fixed_x,
    43
);

proof_case!(
    astra_debt_ceil_product_bridge_x44,
    astra_debt_ceil_product_bridge_fixed_x,
    44
);

proof_case!(
    astra_debt_ceil_product_bridge_x45,
    astra_debt_ceil_product_bridge_fixed_x,
    45
);

proof_case!(
    astra_debt_ceil_product_bridge_x46,
    astra_debt_ceil_product_bridge_fixed_x,
    46
);

proof_case!(
    astra_debt_ceil_product_bridge_x47,
    astra_debt_ceil_product_bridge_fixed_x,
    47
);

proof_case!(
    astra_debt_ceil_product_bridge_x48,
    astra_debt_ceil_product_bridge_fixed_x,
    48
);

proof_case!(
    astra_debt_ceil_product_bridge_x49,
    astra_debt_ceil_product_bridge_fixed_x,
    49
);

proof_case!(
    astra_debt_ceil_product_bridge_x50,
    astra_debt_ceil_product_bridge_fixed_x,
    50
);

proof_case!(
    astra_debt_ceil_product_bridge_x51,
    astra_debt_ceil_product_bridge_fixed_x,
    51
);

proof_case!(
    astra_debt_ceil_product_bridge_x52,
    astra_debt_ceil_product_bridge_fixed_x,
    52
);

proof_case!(
    astra_debt_ceil_product_bridge_x53,
    astra_debt_ceil_product_bridge_fixed_x,
    53
);

proof_case!(
    astra_debt_ceil_product_bridge_x54,
    astra_debt_ceil_product_bridge_fixed_x,
    54
);

proof_case!(
    astra_debt_ceil_product_bridge_x55,
    astra_debt_ceil_product_bridge_fixed_x,
    55
);

proof_case!(
    astra_debt_ceil_product_bridge_x56,
    astra_debt_ceil_product_bridge_fixed_x,
    56
);

proof_case!(
    astra_debt_ceil_product_bridge_x57,
    astra_debt_ceil_product_bridge_fixed_x,
    57
);

proof_case!(
    astra_debt_ceil_product_bridge_x58,
    astra_debt_ceil_product_bridge_fixed_x,
    58
);

proof_case!(
    astra_debt_ceil_product_bridge_x59,
    astra_debt_ceil_product_bridge_fixed_x,
    59
);

proof_case!(
    astra_debt_ceil_product_bridge_x60,
    astra_debt_ceil_product_bridge_fixed_x,
    60
);

proof_case!(
    astra_debt_ceil_product_bridge_x61,
    astra_debt_ceil_product_bridge_fixed_x,
    61
);

proof_case!(
    astra_debt_ceil_product_bridge_x62,
    astra_debt_ceil_product_bridge_fixed_x,
    62
);

proof_case!(
    astra_debt_ceil_product_bridge_x63,
    astra_debt_ceil_product_bridge_fixed_x,
    63
);

proof_case!(
    astra_debt_ceil_product_bridge_x64,
    astra_debt_ceil_product_bridge_fixed_x,
    64
);

proof_case!(
    astra_debt_ceil_product_bridge_x65,
    astra_debt_ceil_product_bridge_fixed_x,
    65
);

proof_case!(
    astra_debt_ceil_product_bridge_x66,
    astra_debt_ceil_product_bridge_fixed_x,
    66
);

proof_case!(
    astra_debt_ceil_product_bridge_x67,
    astra_debt_ceil_product_bridge_fixed_x,
    67
);

proof_case!(
    astra_debt_ceil_product_bridge_x68,
    astra_debt_ceil_product_bridge_fixed_x,
    68
);

proof_case!(
    astra_debt_ceil_product_bridge_x69,
    astra_debt_ceil_product_bridge_fixed_x,
    69
);

proof_case!(
    astra_debt_ceil_product_bridge_x70,
    astra_debt_ceil_product_bridge_fixed_x,
    70
);

proof_case!(
    astra_debt_ceil_product_bridge_x71,
    astra_debt_ceil_product_bridge_fixed_x,
    71
);

proof_case!(
    astra_debt_ceil_product_bridge_x72,
    astra_debt_ceil_product_bridge_fixed_x,
    72
);

proof_case!(
    astra_debt_ceil_product_bridge_x73,
    astra_debt_ceil_product_bridge_fixed_x,
    73
);

proof_case!(
    astra_debt_ceil_product_bridge_x74,
    astra_debt_ceil_product_bridge_fixed_x,
    74
);

proof_case!(
    astra_debt_ceil_product_bridge_x75,
    astra_debt_ceil_product_bridge_fixed_x,
    75
);

proof_case!(
    astra_debt_ceil_product_bridge_x76,
    astra_debt_ceil_product_bridge_fixed_x,
    76
);

proof_case!(
    astra_debt_ceil_product_bridge_x77,
    astra_debt_ceil_product_bridge_fixed_x,
    77
);

proof_case!(
    astra_debt_ceil_product_bridge_x78,
    astra_debt_ceil_product_bridge_fixed_x,
    78
);

proof_case!(
    astra_debt_ceil_product_bridge_x79,
    astra_debt_ceil_product_bridge_fixed_x,
    79
);

proof_case!(
    astra_debt_ceil_product_bridge_x80,
    astra_debt_ceil_product_bridge_fixed_x,
    80
);

proof_case!(
    astra_debt_ceil_product_bridge_x81,
    astra_debt_ceil_product_bridge_fixed_x,
    81
);

proof_case!(
    astra_debt_ceil_product_bridge_x82,
    astra_debt_ceil_product_bridge_fixed_x,
    82
);

proof_case!(
    astra_debt_ceil_product_bridge_x83,
    astra_debt_ceil_product_bridge_fixed_x,
    83
);

proof_case!(
    astra_debt_ceil_product_bridge_x84,
    astra_debt_ceil_product_bridge_fixed_x,
    84
);

proof_case!(
    astra_debt_ceil_product_bridge_x85,
    astra_debt_ceil_product_bridge_fixed_x,
    85
);

proof_case!(
    astra_debt_ceil_product_bridge_x86,
    astra_debt_ceil_product_bridge_fixed_x,
    86
);

proof_case!(
    astra_debt_ceil_product_bridge_x87,
    astra_debt_ceil_product_bridge_fixed_x,
    87
);

proof_case!(
    astra_debt_ceil_product_bridge_x88,
    astra_debt_ceil_product_bridge_fixed_x,
    88
);

proof_case!(
    astra_debt_ceil_product_bridge_x89,
    astra_debt_ceil_product_bridge_fixed_x,
    89
);

proof_case!(
    astra_debt_ceil_product_bridge_x90,
    astra_debt_ceil_product_bridge_fixed_x,
    90
);

proof_case!(
    astra_debt_ceil_product_bridge_x91,
    astra_debt_ceil_product_bridge_fixed_x,
    91
);

proof_case!(
    astra_debt_ceil_product_bridge_x92,
    astra_debt_ceil_product_bridge_fixed_x,
    92
);

proof_case!(
    astra_debt_ceil_product_bridge_x93,
    astra_debt_ceil_product_bridge_fixed_x,
    93
);

proof_case!(
    astra_debt_ceil_product_bridge_x94,
    astra_debt_ceil_product_bridge_fixed_x,
    94
);

proof_case!(
    astra_debt_ceil_product_bridge_x95,
    astra_debt_ceil_product_bridge_fixed_x,
    95
);

proof_case!(
    astra_debt_ceil_product_bridge_x96,
    astra_debt_ceil_product_bridge_fixed_x,
    96
);

proof_case!(
    astra_debt_ceil_product_bridge_x97,
    astra_debt_ceil_product_bridge_fixed_x,
    97
);

proof_case!(
    astra_debt_ceil_product_bridge_x98,
    astra_debt_ceil_product_bridge_fixed_x,
    98
);

proof_case!(
    astra_debt_ceil_product_bridge_x99,
    astra_debt_ceil_product_bridge_fixed_x,
    99
);

proof_case!(
    astra_debt_ceil_product_bridge_x100,
    astra_debt_ceil_product_bridge_fixed_x,
    100
);

proof_case!(
    astra_debt_ceil_product_bridge_x101,
    astra_debt_ceil_product_bridge_fixed_x,
    101
);

proof_case!(
    astra_debt_ceil_product_bridge_x102,
    astra_debt_ceil_product_bridge_fixed_x,
    102
);

proof_case!(
    astra_debt_ceil_product_bridge_x103,
    astra_debt_ceil_product_bridge_fixed_x,
    103
);

proof_case!(
    astra_debt_ceil_product_bridge_x104,
    astra_debt_ceil_product_bridge_fixed_x,
    104
);

proof_case!(
    astra_debt_ceil_product_bridge_x105,
    astra_debt_ceil_product_bridge_fixed_x,
    105
);

proof_case!(
    astra_debt_ceil_product_bridge_x106,
    astra_debt_ceil_product_bridge_fixed_x,
    106
);

proof_case!(
    astra_debt_ceil_product_bridge_x107,
    astra_debt_ceil_product_bridge_fixed_x,
    107
);

proof_case!(
    astra_debt_ceil_product_bridge_x108,
    astra_debt_ceil_product_bridge_fixed_x,
    108
);

proof_case!(
    astra_debt_ceil_product_bridge_x109,
    astra_debt_ceil_product_bridge_fixed_x,
    109
);

proof_case!(
    astra_debt_ceil_product_bridge_x110,
    astra_debt_ceil_product_bridge_fixed_x,
    110
);

proof_case!(
    astra_debt_ceil_product_bridge_x111,
    astra_debt_ceil_product_bridge_fixed_x,
    111
);

proof_case!(
    astra_debt_ceil_product_bridge_x112,
    astra_debt_ceil_product_bridge_fixed_x,
    112
);

proof_case!(
    astra_debt_ceil_product_bridge_x113,
    astra_debt_ceil_product_bridge_fixed_x,
    113
);

proof_case!(
    astra_debt_ceil_product_bridge_x114,
    astra_debt_ceil_product_bridge_fixed_x,
    114
);

proof_case!(
    astra_debt_ceil_product_bridge_x115,
    astra_debt_ceil_product_bridge_fixed_x,
    115
);

proof_case!(
    astra_debt_ceil_product_bridge_x116,
    astra_debt_ceil_product_bridge_fixed_x,
    116
);

proof_case!(
    astra_debt_ceil_product_bridge_x117,
    astra_debt_ceil_product_bridge_fixed_x,
    117
);

proof_case!(
    astra_debt_ceil_product_bridge_x118,
    astra_debt_ceil_product_bridge_fixed_x,
    118
);

proof_case!(
    astra_debt_ceil_product_bridge_x119,
    astra_debt_ceil_product_bridge_fixed_x,
    119
);

proof_case!(
    astra_debt_ceil_product_bridge_x120,
    astra_debt_ceil_product_bridge_fixed_x,
    120
);

proof_case!(
    astra_debt_ceil_product_bridge_x121,
    astra_debt_ceil_product_bridge_fixed_x,
    121
);

proof_case!(
    astra_debt_ceil_product_bridge_x122,
    astra_debt_ceil_product_bridge_fixed_x,
    122
);

proof_case!(
    astra_debt_ceil_product_bridge_x123,
    astra_debt_ceil_product_bridge_fixed_x,
    123
);

proof_case!(
    astra_debt_ceil_product_bridge_x124,
    astra_debt_ceil_product_bridge_fixed_x,
    124
);

proof_case!(
    astra_debt_ceil_product_bridge_x125,
    astra_debt_ceil_product_bridge_fixed_x,
    125
);

proof_case!(
    astra_debt_ceil_product_bridge_x126,
    astra_debt_ceil_product_bridge_fixed_x,
    126
);

proof_case!(
    astra_debt_ceil_product_bridge_x127,
    astra_debt_ceil_product_bridge_fixed_x,
    127
);

proof_case!(
    astra_debt_ceil_product_bridge_x128,
    astra_debt_ceil_product_bridge_fixed_x,
    128
);

proof_case!(
    astra_debt_ceil_product_bridge_x129,
    astra_debt_ceil_product_bridge_fixed_x,
    129
);

proof_case!(
    astra_debt_ceil_product_bridge_x130,
    astra_debt_ceil_product_bridge_fixed_x,
    130
);

proof_case!(
    astra_debt_ceil_product_bridge_x131,
    astra_debt_ceil_product_bridge_fixed_x,
    131
);

proof_case!(
    astra_debt_ceil_product_bridge_x132,
    astra_debt_ceil_product_bridge_fixed_x,
    132
);

proof_case!(
    astra_debt_ceil_product_bridge_x133,
    astra_debt_ceil_product_bridge_fixed_x,
    133
);

proof_case!(
    astra_debt_ceil_product_bridge_x134,
    astra_debt_ceil_product_bridge_fixed_x,
    134
);

proof_case!(
    astra_debt_ceil_product_bridge_x135,
    astra_debt_ceil_product_bridge_fixed_x,
    135
);

proof_case!(
    astra_debt_ceil_product_bridge_x136,
    astra_debt_ceil_product_bridge_fixed_x,
    136
);

proof_case!(
    astra_debt_ceil_product_bridge_x137,
    astra_debt_ceil_product_bridge_fixed_x,
    137
);

proof_case!(
    astra_debt_ceil_product_bridge_x138,
    astra_debt_ceil_product_bridge_fixed_x,
    138
);

proof_case!(
    astra_debt_ceil_product_bridge_x139,
    astra_debt_ceil_product_bridge_fixed_x,
    139
);

proof_case!(
    astra_debt_ceil_product_bridge_x140,
    astra_debt_ceil_product_bridge_fixed_x,
    140
);

proof_case!(
    astra_debt_ceil_product_bridge_x141,
    astra_debt_ceil_product_bridge_fixed_x,
    141
);

proof_case!(
    astra_debt_ceil_product_bridge_x142,
    astra_debt_ceil_product_bridge_fixed_x,
    142
);

proof_case!(
    astra_debt_ceil_product_bridge_x143,
    astra_debt_ceil_product_bridge_fixed_x,
    143
);

proof_case!(
    astra_debt_ceil_product_bridge_x144,
    astra_debt_ceil_product_bridge_fixed_x,
    144
);

proof_case!(
    astra_debt_ceil_product_bridge_x145,
    astra_debt_ceil_product_bridge_fixed_x,
    145
);

proof_case!(
    astra_debt_ceil_product_bridge_x146,
    astra_debt_ceil_product_bridge_fixed_x,
    146
);

proof_case!(
    astra_debt_ceil_product_bridge_x147,
    astra_debt_ceil_product_bridge_fixed_x,
    147
);

proof_case!(
    astra_debt_ceil_product_bridge_x148,
    astra_debt_ceil_product_bridge_fixed_x,
    148
);

proof_case!(
    astra_debt_ceil_product_bridge_x149,
    astra_debt_ceil_product_bridge_fixed_x,
    149
);

proof_case!(
    astra_debt_ceil_product_bridge_x150,
    astra_debt_ceil_product_bridge_fixed_x,
    150
);

proof_case!(
    astra_debt_ceil_product_bridge_x151,
    astra_debt_ceil_product_bridge_fixed_x,
    151
);

proof_case!(
    astra_debt_ceil_product_bridge_x152,
    astra_debt_ceil_product_bridge_fixed_x,
    152
);

proof_case!(
    astra_debt_ceil_product_bridge_x153,
    astra_debt_ceil_product_bridge_fixed_x,
    153
);

proof_case!(
    astra_debt_ceil_product_bridge_x154,
    astra_debt_ceil_product_bridge_fixed_x,
    154
);

proof_case!(
    astra_debt_ceil_product_bridge_x155,
    astra_debt_ceil_product_bridge_fixed_x,
    155
);

proof_case!(
    astra_debt_ceil_product_bridge_x156,
    astra_debt_ceil_product_bridge_fixed_x,
    156
);

proof_case!(
    astra_debt_ceil_product_bridge_x157,
    astra_debt_ceil_product_bridge_fixed_x,
    157
);

proof_case!(
    astra_debt_ceil_product_bridge_x158,
    astra_debt_ceil_product_bridge_fixed_x,
    158
);

proof_case!(
    astra_debt_ceil_product_bridge_x159,
    astra_debt_ceil_product_bridge_fixed_x,
    159
);

proof_case!(
    astra_debt_ceil_product_bridge_x160,
    astra_debt_ceil_product_bridge_fixed_x,
    160
);

proof_case!(
    astra_debt_ceil_product_bridge_x161,
    astra_debt_ceil_product_bridge_fixed_x,
    161
);

proof_case!(
    astra_debt_ceil_product_bridge_x162,
    astra_debt_ceil_product_bridge_fixed_x,
    162
);

proof_case!(
    astra_debt_ceil_product_bridge_x163,
    astra_debt_ceil_product_bridge_fixed_x,
    163
);

proof_case!(
    astra_debt_ceil_product_bridge_x164,
    astra_debt_ceil_product_bridge_fixed_x,
    164
);

proof_case!(
    astra_debt_ceil_product_bridge_x165,
    astra_debt_ceil_product_bridge_fixed_x,
    165
);

proof_case!(
    astra_debt_ceil_product_bridge_x166,
    astra_debt_ceil_product_bridge_fixed_x,
    166
);

proof_case!(
    astra_debt_ceil_product_bridge_x167,
    astra_debt_ceil_product_bridge_fixed_x,
    167
);

proof_case!(
    astra_debt_ceil_product_bridge_x168,
    astra_debt_ceil_product_bridge_fixed_x,
    168
);

proof_case!(
    astra_debt_ceil_product_bridge_x169,
    astra_debt_ceil_product_bridge_fixed_x,
    169
);

proof_case!(
    astra_debt_ceil_product_bridge_x170,
    astra_debt_ceil_product_bridge_fixed_x,
    170
);

proof_case!(
    astra_debt_ceil_product_bridge_x171,
    astra_debt_ceil_product_bridge_fixed_x,
    171
);

proof_case!(
    astra_debt_ceil_product_bridge_x172,
    astra_debt_ceil_product_bridge_fixed_x,
    172
);

proof_case!(
    astra_debt_ceil_product_bridge_x173,
    astra_debt_ceil_product_bridge_fixed_x,
    173
);

proof_case!(
    astra_debt_ceil_product_bridge_x174,
    astra_debt_ceil_product_bridge_fixed_x,
    174
);

proof_case!(
    astra_debt_ceil_product_bridge_x175,
    astra_debt_ceil_product_bridge_fixed_x,
    175
);

proof_case!(
    astra_debt_ceil_product_bridge_x176,
    astra_debt_ceil_product_bridge_fixed_x,
    176
);

proof_case!(
    astra_debt_ceil_product_bridge_x177,
    astra_debt_ceil_product_bridge_fixed_x,
    177
);

proof_case!(
    astra_debt_ceil_product_bridge_x178,
    astra_debt_ceil_product_bridge_fixed_x,
    178
);

proof_case!(
    astra_debt_ceil_product_bridge_x179,
    astra_debt_ceil_product_bridge_fixed_x,
    179
);

proof_case!(
    astra_debt_ceil_product_bridge_x180,
    astra_debt_ceil_product_bridge_fixed_x,
    180
);

proof_case!(
    astra_debt_ceil_product_bridge_x181,
    astra_debt_ceil_product_bridge_fixed_x,
    181
);

proof_case!(
    astra_debt_ceil_product_bridge_x182,
    astra_debt_ceil_product_bridge_fixed_x,
    182
);

proof_case!(
    astra_debt_ceil_product_bridge_x183,
    astra_debt_ceil_product_bridge_fixed_x,
    183
);

proof_case!(
    astra_debt_ceil_product_bridge_x184,
    astra_debt_ceil_product_bridge_fixed_x,
    184
);

proof_case!(
    astra_debt_ceil_product_bridge_x185,
    astra_debt_ceil_product_bridge_fixed_x,
    185
);

proof_case!(
    astra_debt_ceil_product_bridge_x186,
    astra_debt_ceil_product_bridge_fixed_x,
    186
);

proof_case!(
    astra_debt_ceil_product_bridge_x187,
    astra_debt_ceil_product_bridge_fixed_x,
    187
);

proof_case!(
    astra_debt_ceil_product_bridge_x188,
    astra_debt_ceil_product_bridge_fixed_x,
    188
);

proof_case!(
    astra_debt_ceil_product_bridge_x189,
    astra_debt_ceil_product_bridge_fixed_x,
    189
);

proof_case!(
    astra_debt_ceil_product_bridge_x190,
    astra_debt_ceil_product_bridge_fixed_x,
    190
);

proof_case!(
    astra_debt_ceil_product_bridge_x191,
    astra_debt_ceil_product_bridge_fixed_x,
    191
);

proof_case!(
    astra_debt_ceil_product_bridge_x192,
    astra_debt_ceil_product_bridge_fixed_x,
    192
);

proof_case!(
    astra_debt_ceil_product_bridge_x193,
    astra_debt_ceil_product_bridge_fixed_x,
    193
);

proof_case!(
    astra_debt_ceil_product_bridge_x194,
    astra_debt_ceil_product_bridge_fixed_x,
    194
);

proof_case!(
    astra_debt_ceil_product_bridge_x195,
    astra_debt_ceil_product_bridge_fixed_x,
    195
);

proof_case!(
    astra_debt_ceil_product_bridge_x196,
    astra_debt_ceil_product_bridge_fixed_x,
    196
);

proof_case!(
    astra_debt_ceil_product_bridge_x197,
    astra_debt_ceil_product_bridge_fixed_x,
    197
);

proof_case!(
    astra_debt_ceil_product_bridge_x198,
    astra_debt_ceil_product_bridge_fixed_x,
    198
);

proof_case!(
    astra_debt_ceil_product_bridge_x199,
    astra_debt_ceil_product_bridge_fixed_x,
    199
);

proof_case!(
    astra_debt_ceil_product_bridge_x200,
    astra_debt_ceil_product_bridge_fixed_x,
    200
);

proof_case!(
    astra_debt_ceil_product_bridge_x201,
    astra_debt_ceil_product_bridge_fixed_x,
    201
);

proof_case!(
    astra_debt_ceil_product_bridge_x202,
    astra_debt_ceil_product_bridge_fixed_x,
    202
);

proof_case!(
    astra_debt_ceil_product_bridge_x203,
    astra_debt_ceil_product_bridge_fixed_x,
    203
);

proof_case!(
    astra_debt_ceil_product_bridge_x204,
    astra_debt_ceil_product_bridge_fixed_x,
    204
);

proof_case!(
    astra_debt_ceil_product_bridge_x205,
    astra_debt_ceil_product_bridge_fixed_x,
    205
);

proof_case!(
    astra_debt_ceil_product_bridge_x206,
    astra_debt_ceil_product_bridge_fixed_x,
    206
);

proof_case!(
    astra_debt_ceil_product_bridge_x207,
    astra_debt_ceil_product_bridge_fixed_x,
    207
);

proof_case!(
    astra_debt_ceil_product_bridge_x208,
    astra_debt_ceil_product_bridge_fixed_x,
    208
);

proof_case!(
    astra_debt_ceil_product_bridge_x209,
    astra_debt_ceil_product_bridge_fixed_x,
    209
);

proof_case!(
    astra_debt_ceil_product_bridge_x210,
    astra_debt_ceil_product_bridge_fixed_x,
    210
);

proof_case!(
    astra_debt_ceil_product_bridge_x211,
    astra_debt_ceil_product_bridge_fixed_x,
    211
);

proof_case!(
    astra_debt_ceil_product_bridge_x212,
    astra_debt_ceil_product_bridge_fixed_x,
    212
);

proof_case!(
    astra_debt_ceil_product_bridge_x213,
    astra_debt_ceil_product_bridge_fixed_x,
    213
);

proof_case!(
    astra_debt_ceil_product_bridge_x214,
    astra_debt_ceil_product_bridge_fixed_x,
    214
);

proof_case!(
    astra_debt_ceil_product_bridge_x215,
    astra_debt_ceil_product_bridge_fixed_x,
    215
);

proof_case!(
    astra_debt_ceil_product_bridge_x216,
    astra_debt_ceil_product_bridge_fixed_x,
    216
);

proof_case!(
    astra_debt_ceil_product_bridge_x217,
    astra_debt_ceil_product_bridge_fixed_x,
    217
);

proof_case!(
    astra_debt_ceil_product_bridge_x218,
    astra_debt_ceil_product_bridge_fixed_x,
    218
);

proof_case!(
    astra_debt_ceil_product_bridge_x219,
    astra_debt_ceil_product_bridge_fixed_x,
    219
);

proof_case!(
    astra_debt_ceil_product_bridge_x220,
    astra_debt_ceil_product_bridge_fixed_x,
    220
);

proof_case!(
    astra_debt_ceil_product_bridge_x221,
    astra_debt_ceil_product_bridge_fixed_x,
    221
);

proof_case!(
    astra_debt_ceil_product_bridge_x222,
    astra_debt_ceil_product_bridge_fixed_x,
    222
);

proof_case!(
    astra_debt_ceil_product_bridge_x223,
    astra_debt_ceil_product_bridge_fixed_x,
    223
);

proof_case!(
    astra_debt_ceil_product_bridge_x224,
    astra_debt_ceil_product_bridge_fixed_x,
    224
);

proof_case!(
    astra_debt_ceil_product_bridge_x225,
    astra_debt_ceil_product_bridge_fixed_x,
    225
);

proof_case!(
    astra_debt_ceil_product_bridge_x226,
    astra_debt_ceil_product_bridge_fixed_x,
    226
);

proof_case!(
    astra_debt_ceil_product_bridge_x227,
    astra_debt_ceil_product_bridge_fixed_x,
    227
);

proof_case!(
    astra_debt_ceil_product_bridge_x228,
    astra_debt_ceil_product_bridge_fixed_x,
    228
);

proof_case!(
    astra_debt_ceil_product_bridge_x229,
    astra_debt_ceil_product_bridge_fixed_x,
    229
);

proof_case!(
    astra_debt_ceil_product_bridge_x230,
    astra_debt_ceil_product_bridge_fixed_x,
    230
);

proof_case!(
    astra_debt_ceil_product_bridge_x231,
    astra_debt_ceil_product_bridge_fixed_x,
    231
);

proof_case!(
    astra_debt_ceil_product_bridge_x232,
    astra_debt_ceil_product_bridge_fixed_x,
    232
);

proof_case!(
    astra_debt_ceil_product_bridge_x233,
    astra_debt_ceil_product_bridge_fixed_x,
    233
);

proof_case!(
    astra_debt_ceil_product_bridge_x234,
    astra_debt_ceil_product_bridge_fixed_x,
    234
);

proof_case!(
    astra_debt_ceil_product_bridge_x235,
    astra_debt_ceil_product_bridge_fixed_x,
    235
);

proof_case!(
    astra_debt_ceil_product_bridge_x236,
    astra_debt_ceil_product_bridge_fixed_x,
    236
);

proof_case!(
    astra_debt_ceil_product_bridge_x237,
    astra_debt_ceil_product_bridge_fixed_x,
    237
);

proof_case!(
    astra_debt_ceil_product_bridge_x238,
    astra_debt_ceil_product_bridge_fixed_x,
    238
);

proof_case!(
    astra_debt_ceil_product_bridge_x239,
    astra_debt_ceil_product_bridge_fixed_x,
    239
);

proof_case!(
    astra_debt_ceil_product_bridge_x240,
    astra_debt_ceil_product_bridge_fixed_x,
    240
);

proof_case!(
    astra_debt_ceil_product_bridge_x241,
    astra_debt_ceil_product_bridge_fixed_x,
    241
);

proof_case!(
    astra_debt_ceil_product_bridge_x242,
    astra_debt_ceil_product_bridge_fixed_x,
    242
);

proof_case!(
    astra_debt_ceil_product_bridge_x243,
    astra_debt_ceil_product_bridge_fixed_x,
    243
);

proof_case!(
    astra_debt_ceil_product_bridge_x244,
    astra_debt_ceil_product_bridge_fixed_x,
    244
);

proof_case!(
    astra_debt_ceil_product_bridge_x245,
    astra_debt_ceil_product_bridge_fixed_x,
    245
);

proof_case!(
    astra_debt_ceil_product_bridge_x246,
    astra_debt_ceil_product_bridge_fixed_x,
    246
);

proof_case!(
    astra_debt_ceil_product_bridge_x247,
    astra_debt_ceil_product_bridge_fixed_x,
    247
);

proof_case!(
    astra_debt_ceil_product_bridge_x248,
    astra_debt_ceil_product_bridge_fixed_x,
    248
);

proof_case!(
    astra_debt_ceil_product_bridge_x249,
    astra_debt_ceil_product_bridge_fixed_x,
    249
);

proof_case!(
    astra_debt_ceil_product_bridge_x250,
    astra_debt_ceil_product_bridge_fixed_x,
    250
);

proof_case!(
    astra_debt_ceil_product_bridge_x251,
    astra_debt_ceil_product_bridge_fixed_x,
    251
);

proof_case!(
    astra_debt_ceil_product_bridge_x252,
    astra_debt_ceil_product_bridge_fixed_x,
    252
);

proof_case!(
    astra_debt_ceil_product_bridge_x253,
    astra_debt_ceil_product_bridge_fixed_x,
    253
);

proof_case!(
    astra_debt_ceil_product_bridge_x254,
    astra_debt_ceil_product_bridge_fixed_x,
    254
);

// Explicit composition: only liability conversions use proved literal laws.
// The rate-growth ceiling remains the real checked-i128 implementation.
struct AstraDebtDeltaMath {
    stage: core::cell::Cell<u8>,
    new_rate: core::cell::Cell<i128>,
    supply: i128,
    old_rate: i128,
    accrual: i128,
}

impl FixedMath for AstraDebtDeltaMath {
    fn floor(&self, _: i128, _: i128, _: i128) -> i128 {
        panic!("debt growth must not floor")
    }

    fn ceil(&self, x: i128, y: i128, denominator: i128) -> i128 {
        let stage = self.stage.get();
        assert!(stage < 3);
        self.stage.set(stage + 1);
        if stage == 1 {
            assert_eq!(
                (x, y, denominator),
                (self.accrual, self.old_rate, SCALAR_12)
            );
            let rate = FixedMath::ceil(&(), x, y, denominator);
            assert!(0 <= rate && rate <= 6_502_500_000_000);
            self.new_rate.set(rate);
            return rate;
        }
        let expected_rate = if stage == 0 {
            self.old_rate
        } else {
            self.new_rate.get()
        };
        assert_eq!((x, y, denominator), (self.supply, expected_rate, SCALAR_12));
        assert!(0 <= x && x <= 255);
        assert!(0 <= y && y <= 6_502_500_000_000);
        let n = y.checked_mul(x).unwrap();
        assert!(0 <= n && n <= 1_658_137_500_000_000);
        let rounded_n = n.checked_add(SCALAR_12 - 1).unwrap();
        // Prerequisites: full fixed-x product bridge + numerator literal.
        rounded_n / SCALAR_12
    }
}

#[kani::proof]
fn astra_grow_debt_exact_delta_composed() {
    let d_supply = kani::any::<u8>() as i128;
    let rate_h = kani::any::<u8>();
    kani::assume(rate_h >= 1);
    let accrual_h = kani::any::<u8>();
    kani::assume(accrual_h >= 100);
    let mut data = sym_data(hundredths(rate_h), 0, 0, d_supply);
    let old_rate = data.d_rate;
    let math = AstraDebtDeltaMath {
        stage: core::cell::Cell::new(0),
        new_rate: core::cell::Cell::new(0),
        supply: d_supply,
        old_rate,
        accrual: hundredths(accrual_h),
    };
    let accrued = data.grow_debt(&math, hundredths(accrual_h));
    assert_eq!(math.stage.get(), 3);
    assert_eq!(data.d_rate, math.new_rate.get());
    let old_liabilities = (old_rate * d_supply + SCALAR_12 - 1) / SCALAR_12;
    let new_liabilities = (data.d_rate * d_supply + SCALAR_12 - 1) / SCALAR_12;
    assert_eq!(accrued, new_liabilities - old_liabilities);
    kani::cover!(true, "E2-1 exact delta domain reachable");
    kani::cover!(accrual_h == 100, "E2-1 exact delta unity reachable");
    kani::cover!(d_supply == 0, "E2-1 exact delta zero supply reachable");
    kani::cover!(
        accrual_h > 100 && d_supply > 0,
        "E2-1 exact delta nontrivial growth reachable"
    );
}

// C1 S0: exact original-u64-oracle bridge over the actual ()
// implementation. Rate hundredths: every nonzero u8 h casts losslessly
// (h <= 255 so h*S/100 < 2^63; product fits i128 comfortably). These are
// numeric prerequisite receipts, not assumptions about ().
#[kani::proof]
fn c1_s0_d_forward_exact_u64() {
    let amount_u: u8 = kani::any();
    let rate_h: u8 = kani::any();
    kani::assume(rate_h > 0);
    let amount = amount_u as i128;
    let rate = hundredths(rate_h);
    let scale = SCALAR_12 as u64;
    let rate_o = rate_h as u64 * scale / 100;
    assert_eq!(scale as i128, SCALAR_12); // u64 cast lossless
    assert_eq!(rate_o as i128, rate); // oracle-rate bridge
    let product_o = amount_u as u64 * rate_o;
    assert_eq!(product_o as i128, amount * rate); // product bridge
    let out = sym_data(rate, 0, 0, 0).to_asset_from_d_token(&(), amount);
    let expected = product_o.div_ceil(scale);
    assert!(expected <= 651);
    assert_eq!(out, expected as i128);
    assert!(out >= 0 && out <= 651); // totality and range
    if amount == 0 {
        assert_eq!(out, 0); // zero-input preservation
    }
    if amount > 0 {
        assert!(out >= 1); // caller D strict positivity premise
    }
    kani::cover!(amount_u == 0);
    kani::cover!(amount_u == 255 && rate_h == 255); // domain-max reachability
}

/// C2 S0 prerequisite: the real checked-i128 forward conversion equals
/// the unchanged C2 u64 oracle on every positive original operand.
/// No supplier-loss caller is accepted from this prerequisite alone.
#[kani::proof]
fn c2_s0_d_forward_exact_u64() {
    let default_u: u8 = kani::any();
    let d_rate_u: u8 = kani::any();
    kani::assume(d_rate_u > 0 && default_u > 0);
    let d_rate = hundredths(d_rate_u);
    let data = sym_data(d_rate, 0, 0, 0);
    // This type-checked binding and call select the actual unit helper,
    // not a proof-local conversion or a Soroban host arithmetic stub.
    let _: fn(&ReserveData, &(), i128) -> i128 = ReserveData::to_asset_from_d_token;
    let out = data.to_asset_from_d_token(&(), default_u as i128);

    let d_rate_o = d_rate_u as u64 * (SCALAR_12 as u64) / 100;
    let debt_assets = (default_u as u64 * d_rate_o).div_ceil(SCALAR_12 as u64);
    assert_eq!(SCALAR_12 as u64 as i128, SCALAR_12);
    assert_eq!(d_rate_o as i128, d_rate);
    assert_eq!(
        (default_u as u64 * d_rate_o) as i128,
        default_u as i128 * d_rate
    );
    assert!(out >= 1 && out <= 651);
    assert_eq!(out, debt_assets as i128);
    assert_eq!(out as u64, debt_assets);
    assert_eq!(debt_assets as i128 as u64, debt_assets);
    kani::cover!(default_u == 1 && d_rate_u == 100);
    kani::cover!(default_u == 255 && d_rate_u == 255);
}
#[kani::proof]
fn c1_s0_b_forward_exact_u64() {
    let amount_u: u8 = kani::any();
    let rate_h: u8 = kani::any(); // zero forward rate included
    let amount = amount_u as i128;
    let rate = hundredths(rate_h);
    let scale = SCALAR_12 as u64;
    let rate_o = rate_h as u64 * scale / 100;
    assert_eq!(scale as i128, SCALAR_12); // u64 cast lossless
    assert_eq!(rate_o as i128, rate); // oracle-rate bridge
    let product_o = amount_u as u64 * rate_o;
    assert_eq!(product_o as i128, amount * rate); // product bridge
    let out = sym_data(0, rate, 0, 0).to_asset_from_b_token(&(), amount);
    let expected = product_o / scale;
    assert!(expected <= 650);
    assert_eq!(out, expected as i128);
    assert!(out >= 0 && out <= 650); // totality and range
    if amount == 0 || rate == 0 {
        assert_eq!(out, 0); // real zero branch preserved
    }
    kani::cover!(rate_h == 0); // genuine zero forward rate reachable
    kani::cover!(amount_u == 255 && rate_h == 255);
}

// C1 S1: symbolic trials on the actual () implementation. The two disjoint
// partitions below cover the full debt-assets domain 0..=651 (the u8 low
// slice mirrors the accepted 0..=255 receipts; the high slice 256..=651 is
// genuinely new and includes every possible committed C1 inverse operand,
// including T values up to 65100 through ceil-of-D*S/b_rate at D<=651 with
// b_rate >= S/100).
fn c1_s1_inverse_exact_u64(amount_u: u16, rate_h: u8, round_up: bool) {
    let amount = amount_u as i128;
    let rate = hundredths(rate_h);
    let scale = SCALAR_12 as u64;
    let rate_o = rate_h as u64 * scale / 100;
    assert!(amount <= 651 && rate_h > 0);
    assert_eq!(scale as i128, SCALAR_12); // u64 cast lossless
    assert_eq!(rate_o as i128, rate); // oracle-rate bridge
    assert!(rate_o > 0); // divisor premise from nonzero u8 hundredths
    let product_o = amount_u as u64 * scale;
    assert_eq!(product_o as i128, amount * SCALAR_12); // product bridge
    let data = sym_data(rate, rate, 0, 0); // both rates positive: safe floor divisor
    let (out, expected) = if round_up {
        (data.to_b_token_up(&(), amount), product_o.div_ceil(rate_o))
    } else {
        (data.to_d_token_down(&(), amount), product_o / rate_o)
    };
    assert!(expected <= 65100);
    assert_eq!(out, expected as i128);
    assert!(out >= 0 && out <= 65100); // totality, range, Q bound
    if round_up && amount > 0 {
        assert!(out >= 1); // caller T strict positivity premise (ceil)
    }
    if amount == 0 {
        assert_eq!(out, 0); // zero-input preservation
    }
}

#[kani::proof]
fn c1_s1_b_inverse_low_exact_u64() {
    let amount_u: u8 = kani::any();
    let rate_h: u8 = kani::any();
    kani::assume(rate_h > 0);
    c1_s1_inverse_exact_u64(amount_u as u16, rate_h, true);
    kani::cover!(amount_u == 0); // zero-input reachability
    kani::cover!(amount_u == 255 && rate_h == 255); // low-domain max
}

#[kani::proof]
fn c1_s1_b_inverse_high_exact_u64() {
    let amount_u: u16 = kani::any();
    let rate_h: u8 = kani::any();
    kani::assume(amount_u >= 256 && amount_u <= 651 && rate_h > 0);
    c1_s1_inverse_exact_u64(amount_u, rate_h, true);
    kani::cover!(amount_u == 256 && rate_h == 1); // high-domain floor edge
    kani::cover!(amount_u == 651 && rate_h == 255); // high-domain max
}

#[kani::proof]
fn c1_s1_d_inverse_low_exact_u64() {
    let amount_u: u8 = kani::any();
    let rate_h: u8 = kani::any();
    kani::assume(rate_h > 0);
    c1_s1_inverse_exact_u64(amount_u as u16, rate_h, false);
    kani::cover!(amount_u == 0); // zero-input reachability
    kani::cover!(amount_u == 255 && rate_h == 255); // low-domain max
}

#[kani::proof]
fn c1_s1_d_inverse_high_exact_u64() {
    let amount_u: u16 = kani::any();
    let rate_h: u8 = kani::any();
    kani::assume(amount_u >= 256 && amount_u <= 651 && rate_h > 0);
    c1_s1_inverse_exact_u64(amount_u, rate_h, false);
    kani::cover!(amount_u == 256 && rate_h == 1); // high-domain floor edge
    kani::cover!(amount_u == 651 && rate_h == 255); // high-domain max
}

// Fixed-rate calibration of the corrected C1 S1 u64 bridge above.
// For each direction/slice, closure requires every H in 1..=255.
// H=1,3,255 canaries alone do not close the symbolic-rate domain.
fn c1_s1_inverse_exact_u64_fixed_rate<const H: u8>(amount_u: u16, round_up: bool) {
    let amount = amount_u as i128;
    let rate = hundredths(H);
    let scale = SCALAR_12 as u64;
    let rate_o = H as u64 * scale / 100;
    assert!(amount <= 651 && H >= 1);
    assert_eq!(scale as i128, SCALAR_12); // u64 cast lossless
    assert_eq!(rate_o as i128, rate); // oracle-rate bridge
    assert!(rate_o > 0); // divisor premise from nonzero u8 hundredths
    let product_o = amount_u as u64 * scale;
    assert_eq!(product_o as i128, amount * SCALAR_12); // product bridge
    let data = sym_data(rate, rate, 0, 0); // both rates positive: safe floor divisor
    let (out, expected) = if round_up {
        (data.to_b_token_up(&(), amount), product_o.div_ceil(rate_o))
    } else {
        (data.to_d_token_down(&(), amount), product_o / rate_o)
    };
    assert!(expected <= 65100);
    assert_eq!(out, expected as i128);
    assert!(out >= 0 && out <= 65100); // totality, range, Q bound
    if round_up && amount > 0 {
        assert!(out >= 1); // caller T strict positivity premise (ceil)
    }
    if amount == 0 {
        assert_eq!(out, 0); // zero-input preservation
    }
}

macro_rules! c1_inverse_case {
    (low, $name:ident, $rate:literal, $round_up:literal) => {
        #[kani::proof]
        fn $name() {
            let amount_u: u8 = kani::any();
            c1_s1_inverse_exact_u64_fixed_rate::<$rate>(amount_u as u16, $round_up);
            kani::cover!(amount_u == 0);
            kani::cover!(amount_u == 255);
        }
    };
    (high, $name:ident, $rate:literal, $round_up:literal) => {
        #[kani::proof]
        fn $name() {
            let amount_u: u16 = kani::any();
            kani::assume(amount_u >= 256 && amount_u <= 651);
            c1_s1_inverse_exact_u64_fixed_rate::<$rate>(amount_u, $round_up);
            kani::cover!(amount_u == 256);
            kani::cover!(amount_u == 651);
        }
    };
}

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_001, 1, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_003, 3, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_255, 255, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_001, 1, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_003, 3, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_255, 255, true);

c1_inverse_case!(low, c1_s1_d_inverse_low_exact_u64_rate_001, 1, false);

c1_inverse_case!(low, c1_s1_d_inverse_low_exact_u64_rate_003, 3, false);

c1_inverse_case!(low, c1_s1_d_inverse_low_exact_u64_rate_255, 255, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_001, 1, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_003, 3, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_255, 255, false);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_002, 2, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_004, 4, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_005, 5, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_006, 6, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_007, 7, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_008, 8, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_009, 9, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_010, 10, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_011, 11, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_012, 12, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_013, 13, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_014, 14, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_015, 15, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_016, 16, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_017, 17, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_018, 18, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_019, 19, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_020, 20, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_021, 21, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_022, 22, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_023, 23, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_024, 24, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_025, 25, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_026, 26, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_027, 27, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_028, 28, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_029, 29, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_030, 30, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_031, 31, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_032, 32, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_033, 33, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_034, 34, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_035, 35, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_036, 36, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_037, 37, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_038, 38, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_039, 39, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_040, 40, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_041, 41, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_042, 42, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_043, 43, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_044, 44, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_045, 45, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_046, 46, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_047, 47, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_048, 48, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_049, 49, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_050, 50, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_051, 51, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_052, 52, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_053, 53, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_054, 54, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_055, 55, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_056, 56, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_057, 57, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_058, 58, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_059, 59, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_060, 60, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_061, 61, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_062, 62, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_063, 63, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_064, 64, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_065, 65, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_066, 66, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_067, 67, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_068, 68, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_069, 69, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_070, 70, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_071, 71, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_072, 72, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_073, 73, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_074, 74, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_075, 75, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_076, 76, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_077, 77, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_078, 78, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_079, 79, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_080, 80, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_081, 81, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_082, 82, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_083, 83, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_084, 84, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_085, 85, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_086, 86, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_087, 87, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_088, 88, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_089, 89, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_090, 90, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_091, 91, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_092, 92, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_093, 93, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_094, 94, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_095, 95, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_096, 96, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_097, 97, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_098, 98, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_099, 99, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_100, 100, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_101, 101, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_102, 102, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_103, 103, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_104, 104, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_105, 105, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_106, 106, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_107, 107, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_108, 108, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_109, 109, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_110, 110, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_111, 111, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_112, 112, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_113, 113, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_114, 114, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_115, 115, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_116, 116, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_117, 117, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_118, 118, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_119, 119, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_120, 120, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_121, 121, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_122, 122, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_123, 123, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_124, 124, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_125, 125, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_126, 126, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_127, 127, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_128, 128, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_129, 129, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_130, 130, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_131, 131, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_132, 132, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_133, 133, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_134, 134, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_135, 135, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_136, 136, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_137, 137, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_138, 138, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_139, 139, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_140, 140, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_141, 141, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_142, 142, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_143, 143, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_144, 144, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_145, 145, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_146, 146, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_147, 147, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_148, 148, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_149, 149, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_150, 150, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_151, 151, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_152, 152, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_153, 153, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_154, 154, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_155, 155, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_156, 156, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_157, 157, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_158, 158, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_159, 159, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_160, 160, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_161, 161, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_162, 162, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_163, 163, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_164, 164, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_165, 165, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_166, 166, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_167, 167, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_168, 168, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_169, 169, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_170, 170, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_171, 171, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_172, 172, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_173, 173, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_174, 174, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_175, 175, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_176, 176, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_177, 177, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_178, 178, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_179, 179, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_180, 180, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_181, 181, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_182, 182, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_183, 183, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_184, 184, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_185, 185, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_186, 186, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_187, 187, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_188, 188, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_189, 189, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_190, 190, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_191, 191, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_192, 192, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_193, 193, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_194, 194, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_195, 195, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_196, 196, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_197, 197, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_198, 198, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_199, 199, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_200, 200, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_201, 201, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_202, 202, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_203, 203, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_204, 204, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_205, 205, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_206, 206, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_207, 207, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_208, 208, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_209, 209, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_210, 210, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_211, 211, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_212, 212, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_213, 213, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_214, 214, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_215, 215, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_216, 216, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_217, 217, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_218, 218, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_219, 219, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_220, 220, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_221, 221, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_222, 222, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_223, 223, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_224, 224, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_225, 225, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_226, 226, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_227, 227, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_228, 228, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_229, 229, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_230, 230, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_231, 231, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_232, 232, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_233, 233, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_234, 234, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_235, 235, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_236, 236, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_237, 237, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_238, 238, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_239, 239, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_240, 240, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_241, 241, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_242, 242, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_243, 243, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_244, 244, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_245, 245, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_246, 246, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_247, 247, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_248, 248, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_249, 249, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_250, 250, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_251, 251, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_252, 252, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_253, 253, true);

c1_inverse_case!(low, c1_s1_b_inverse_low_exact_u64_rate_254, 254, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_002, 2, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_004, 4, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_005, 5, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_006, 6, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_007, 7, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_008, 8, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_009, 9, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_010, 10, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_011, 11, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_012, 12, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_013, 13, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_014, 14, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_015, 15, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_016, 16, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_017, 17, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_018, 18, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_019, 19, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_020, 20, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_021, 21, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_022, 22, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_023, 23, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_024, 24, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_025, 25, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_026, 26, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_027, 27, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_028, 28, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_029, 29, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_030, 30, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_031, 31, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_032, 32, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_033, 33, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_034, 34, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_035, 35, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_036, 36, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_037, 37, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_038, 38, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_039, 39, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_040, 40, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_041, 41, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_042, 42, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_043, 43, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_044, 44, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_045, 45, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_046, 46, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_047, 47, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_048, 48, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_049, 49, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_050, 50, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_051, 51, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_052, 52, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_053, 53, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_054, 54, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_055, 55, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_056, 56, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_057, 57, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_058, 58, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_059, 59, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_060, 60, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_061, 61, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_062, 62, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_063, 63, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_064, 64, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_065, 65, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_066, 66, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_067, 67, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_068, 68, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_069, 69, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_070, 70, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_071, 71, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_072, 72, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_073, 73, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_074, 74, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_075, 75, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_076, 76, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_077, 77, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_078, 78, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_079, 79, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_080, 80, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_081, 81, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_082, 82, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_083, 83, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_084, 84, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_085, 85, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_086, 86, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_087, 87, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_088, 88, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_089, 89, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_090, 90, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_091, 91, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_092, 92, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_093, 93, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_094, 94, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_095, 95, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_096, 96, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_097, 97, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_098, 98, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_099, 99, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_100, 100, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_101, 101, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_102, 102, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_103, 103, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_104, 104, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_105, 105, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_106, 106, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_107, 107, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_108, 108, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_109, 109, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_110, 110, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_111, 111, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_112, 112, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_113, 113, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_114, 114, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_115, 115, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_116, 116, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_117, 117, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_118, 118, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_119, 119, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_120, 120, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_121, 121, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_122, 122, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_123, 123, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_124, 124, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_125, 125, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_126, 126, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_127, 127, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_128, 128, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_129, 129, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_130, 130, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_131, 131, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_132, 132, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_133, 133, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_134, 134, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_135, 135, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_136, 136, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_137, 137, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_138, 138, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_139, 139, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_140, 140, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_141, 141, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_142, 142, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_143, 143, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_144, 144, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_145, 145, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_146, 146, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_147, 147, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_148, 148, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_149, 149, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_150, 150, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_151, 151, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_152, 152, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_153, 153, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_154, 154, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_155, 155, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_156, 156, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_157, 157, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_158, 158, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_159, 159, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_160, 160, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_161, 161, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_162, 162, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_163, 163, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_164, 164, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_165, 165, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_166, 166, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_167, 167, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_168, 168, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_169, 169, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_170, 170, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_171, 171, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_172, 172, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_173, 173, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_174, 174, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_175, 175, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_176, 176, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_177, 177, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_178, 178, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_179, 179, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_180, 180, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_181, 181, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_182, 182, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_183, 183, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_184, 184, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_185, 185, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_186, 186, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_187, 187, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_188, 188, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_189, 189, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_190, 190, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_191, 191, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_192, 192, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_193, 193, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_194, 194, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_195, 195, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_196, 196, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_197, 197, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_198, 198, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_199, 199, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_200, 200, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_201, 201, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_202, 202, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_203, 203, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_204, 204, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_205, 205, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_206, 206, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_207, 207, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_208, 208, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_209, 209, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_210, 210, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_211, 211, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_212, 212, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_213, 213, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_214, 214, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_215, 215, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_216, 216, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_217, 217, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_218, 218, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_219, 219, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_220, 220, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_221, 221, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_222, 222, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_223, 223, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_224, 224, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_225, 225, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_226, 226, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_227, 227, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_228, 228, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_229, 229, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_230, 230, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_231, 231, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_232, 232, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_233, 233, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_234, 234, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_235, 235, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_236, 236, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_237, 237, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_238, 238, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_239, 239, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_240, 240, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_241, 241, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_242, 242, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_243, 243, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_244, 244, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_245, 245, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_246, 246, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_247, 247, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_248, 248, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_249, 249, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_250, 250, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_251, 251, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_252, 252, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_253, 253, true);

c1_inverse_case!(high, c1_s1_b_inverse_high_exact_u64_rate_254, 254, true);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_002, 2, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_004, 4, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_005, 5, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_006, 6, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_007, 7, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_008, 8, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_009, 9, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_010, 10, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_011, 11, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_012, 12, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_013, 13, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_014, 14, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_015, 15, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_016, 16, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_017, 17, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_018, 18, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_019, 19, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_020, 20, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_021, 21, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_022, 22, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_023, 23, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_024, 24, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_025, 25, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_026, 26, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_027, 27, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_028, 28, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_029, 29, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_030, 30, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_031, 31, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_032, 32, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_033, 33, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_034, 34, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_035, 35, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_036, 36, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_037, 37, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_038, 38, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_039, 39, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_040, 40, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_041, 41, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_042, 42, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_043, 43, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_044, 44, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_045, 45, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_046, 46, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_047, 47, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_048, 48, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_049, 49, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_050, 50, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_051, 51, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_052, 52, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_053, 53, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_054, 54, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_055, 55, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_056, 56, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_057, 57, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_058, 58, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_059, 59, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_060, 60, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_061, 61, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_062, 62, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_063, 63, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_064, 64, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_065, 65, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_066, 66, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_067, 67, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_068, 68, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_069, 69, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_070, 70, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_071, 71, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_072, 72, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_073, 73, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_074, 74, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_075, 75, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_076, 76, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_077, 77, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_078, 78, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_079, 79, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_080, 80, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_081, 81, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_082, 82, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_083, 83, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_084, 84, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_085, 85, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_086, 86, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_087, 87, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_088, 88, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_089, 89, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_090, 90, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_091, 91, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_092, 92, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_093, 93, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_094, 94, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_095, 95, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_096, 96, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_097, 97, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_098, 98, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_099, 99, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_100, 100, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_101, 101, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_102, 102, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_103, 103, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_104, 104, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_105, 105, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_106, 106, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_107, 107, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_108, 108, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_109, 109, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_110, 110, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_111, 111, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_112, 112, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_113, 113, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_114, 114, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_115, 115, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_116, 116, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_117, 117, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_118, 118, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_119, 119, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_120, 120, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_121, 121, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_122, 122, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_123, 123, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_124, 124, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_125, 125, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_126, 126, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_127, 127, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_128, 128, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_129, 129, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_130, 130, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_131, 131, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_132, 132, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_133, 133, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_134, 134, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_135, 135, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_136, 136, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_137, 137, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_138, 138, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_139, 139, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_140, 140, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_141, 141, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_142, 142, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_143, 143, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_144, 144, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_145, 145, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_146, 146, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_147, 147, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_148, 148, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_149, 149, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_150, 150, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_151, 151, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_152, 152, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_153, 153, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_154, 154, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_155, 155, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_156, 156, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_157, 157, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_158, 158, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_159, 159, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_160, 160, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_161, 161, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_162, 162, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_163, 163, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_164, 164, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_165, 165, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_166, 166, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_167, 167, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_168, 168, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_169, 169, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_170, 170, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_171, 171, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_172, 172, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_173, 173, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_174, 174, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_175, 175, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_176, 176, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_177, 177, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_178, 178, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_179, 179, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_180, 180, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_181, 181, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_182, 182, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_183, 183, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_184, 184, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_185, 185, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_186, 186, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_187, 187, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_188, 188, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_189, 189, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_190, 190, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_191, 191, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_192, 192, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_193, 193, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_194, 194, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_195, 195, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_196, 196, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_197, 197, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_198, 198, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_199, 199, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_200, 200, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_201, 201, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_202, 202, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_203, 203, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_204, 204, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_205, 205, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_206, 206, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_207, 207, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_208, 208, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_209, 209, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_210, 210, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_211, 211, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_212, 212, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_213, 213, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_214, 214, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_215, 215, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_216, 216, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_217, 217, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_218, 218, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_219, 219, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_220, 220, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_221, 221, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_222, 222, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_223, 223, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_224, 224, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_225, 225, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_226, 226, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_227, 227, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_228, 228, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_229, 229, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_230, 230, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_231, 231, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_232, 232, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_233, 233, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_234, 234, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_235, 235, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_236, 236, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_237, 237, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_238, 238, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_239, 239, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_240, 240, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_241, 241, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_242, 242, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_243, 243, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_244, 244, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_245, 245, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_246, 246, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_247, 247, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_248, 248, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_249, 249, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_250, 250, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_251, 251, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_252, 252, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_253, 253, false);

c1_inverse_case!(high, c1_s1_d_inverse_high_exact_u64_rate_254, 254, false);

// Two-stage output-parametric composition: the full 256-cell product bridge
// plus numerator numeric closure binds the liability outputs separately;
// this caller theorem checks their guarded use, not the original numeric
// oracle standalone. Rate growth remains a REAL checked-i128 ceil call.
// Independent nonnegative outputs deliberately need not be ordered.
struct AstraDebtOutputParametricMath {
    stage: core::cell::Cell<u8>,
    new_rate: core::cell::Cell<i128>,
    supply: i128,
    old_rate: i128,
    accrual: i128,
    old_liabilities: i128,
    new_liabilities: i128,
}

impl FixedMath for AstraDebtOutputParametricMath {
    fn floor(&self, _: i128, _: i128, _: i128) -> i128 {
        panic!("debt growth must not floor")
    }

    fn ceil(&self, x: i128, y: i128, denominator: i128) -> i128 {
        let stage = self.stage.get();
        assert!(stage < 3);
        self.stage.set(stage + 1);
        if stage == 1 {
            assert_eq!(
                (x, y, denominator),
                (self.accrual, self.old_rate, SCALAR_12)
            );
            let rate = FixedMath::ceil(&(), x, y, denominator);
            assert!(0 <= rate && rate <= 6_502_500_000_000);
            self.new_rate.set(rate);
            return rate;
        }
        let expected_rate = if stage == 0 {
            self.old_rate
        } else {
            self.new_rate.get()
        };
        assert_eq!((x, y, denominator), (self.supply, expected_rate, SCALAR_12));
        assert!(0 <= x && x <= 255);
        assert!(0 <= y && y <= 6_502_500_000_000);
        let n = y.checked_mul(x).unwrap();
        assert!(0 <= n && n <= 1_658_137_500_000_000);
        let _rounded_n = n.checked_add(SCALAR_12 - 1).unwrap();
        if stage == 0 {
            self.old_liabilities
        } else {
            self.new_liabilities
        }
    }
}

#[kani::proof]
fn astra_grow_debt_output_parametric() {
    let d_supply = kani::any::<u8>() as i128;
    let rate_h = kani::any::<u8>();
    kani::assume(rate_h >= 1);
    let accrual_h = kani::any::<u8>();
    kani::assume(accrual_h >= 100);
    let old_liabilities = kani::any::<i128>();
    kani::assume(old_liabilities >= 0);
    let new_liabilities = kani::any::<i128>();
    kani::assume(new_liabilities >= 0);
    let mut data = sym_data(hundredths(rate_h), 0, 0, d_supply);
    let old_rate = data.d_rate;
    let math = AstraDebtOutputParametricMath {
        stage: core::cell::Cell::new(0),
        new_rate: core::cell::Cell::new(0),
        supply: d_supply,
        old_rate,
        accrual: hundredths(accrual_h),
        old_liabilities,
        new_liabilities,
    };
    let accrued = data.grow_debt(&math, hundredths(accrual_h));
    assert_eq!(math.stage.get(), 3);
    assert_eq!(data.d_rate, math.new_rate.get());
    assert_eq!(accrued, new_liabilities - old_liabilities);
    kani::cover!(true, "E2-1 exact delta domain reachable");
    kani::cover!(accrual_h == 100, "E2-1 exact delta unity reachable");
    kani::cover!(d_supply == 0, "E2-1 exact delta zero supply reachable");
    kani::cover!(
        accrual_h > 100 && d_supply > 0,
        "E2-1 exact delta nontrivial growth reachable"
    );
}

#[kani::proof]
fn astra_grow_debt_output_witness_zero_supply() {
    let mut data = sym_data(hundredths(255), 0, 0, 0);
    let old_liabilities = data.to_asset_from_d_token(&(), data.d_supply);
    let accrued = data.grow_debt(&(), hundredths(255));
    let new_liabilities = data.to_asset_from_d_token(&(), data.d_supply);
    assert_eq!(data.d_rate, 6_502_500_000_000);
    assert_eq!((old_liabilities, new_liabilities, accrued), (0, 0, 0));
    kani::cover!(
        data.d_supply == 0 && old_liabilities == 0 && new_liabilities == 0 && accrued == 0,
        "actual zero-supply growth"
    );
}

#[kani::proof]
fn astra_grow_debt_output_witness_unity() {
    let mut data = sym_data(hundredths(255), 0, 0, 255);
    let old_liabilities = data.to_asset_from_d_token(&(), data.d_supply);
    let accrued = data.grow_debt(&(), hundredths(100));
    let new_liabilities = data.to_asset_from_d_token(&(), data.d_supply);
    assert_eq!(data.d_rate, 2_550_000_000_000);
    assert_eq!((old_liabilities, new_liabilities, accrued), (651, 651, 0));
    kani::cover!(
        data.d_supply > 0 && old_liabilities == 651 && new_liabilities == 651 && accrued == 0,
        "actual positive-supply unity"
    );
}

#[kani::proof]
fn astra_grow_debt_output_witness_growth() {
    let mut data = sym_data(hundredths(100), 0, 0, 100);
    let old_liabilities = data.to_asset_from_d_token(&(), data.d_supply);
    let accrued = data.grow_debt(&(), hundredths(200));
    let new_liabilities = data.to_asset_from_d_token(&(), data.d_supply);
    assert_eq!(data.d_rate, 2_000_000_000_000);
    assert_eq!((old_liabilities, new_liabilities, accrued), (100, 200, 100));
    kani::cover!(
        old_liabilities == 100 && new_liabilities == 200 && accrued == 100,
        "actual nontrivial growth"
    );
}

#[kani::proof]
fn astra_grow_debt_output_witness_maximum() {
    let mut data = sym_data(hundredths(255), 0, 0, 255);
    let old_liabilities = data.to_asset_from_d_token(&(), data.d_supply);
    let accrued = data.grow_debt(&(), hundredths(255));
    let new_liabilities = data.to_asset_from_d_token(&(), data.d_supply);
    assert_eq!(data.d_rate, 6_502_500_000_000);
    assert_eq!(
        (old_liabilities, new_liabilities, accrued),
        (651, 1659, 1008)
    );
    kani::cover!(
        data.d_supply == 255
            && old_liabilities == 651
            && new_liabilities == 1659
            && accrued == 1008,
        "actual maximal 255/255/255 growth"
    );
}
