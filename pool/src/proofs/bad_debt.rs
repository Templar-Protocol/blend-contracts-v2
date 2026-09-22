use super::*;
use crate::constants::SCALAR_12;

/// Rates are symbolic u8 hundredths of SCALAR_12 (0..=2.55 x unity): a
/// deliberate proof-domain restriction, not a protocol rate bound.
fn rate_hundredths(u: u8) -> i128 {
    (u as i128) * SCALAR_12 / 100
}

fn setoff_data(d_rate: i128, b_rate: i128) -> ReserveData {
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

/// C1: the same-reserve supply-setoff kernel from
/// check_and_handle_user_bad_debt. Establishes, independently of the
/// kernel's own arithmetic, the exact accepted burn policy and rounding:
/// the committed burn is exactly the supply claim capped by the
/// ceil-rounded debt-covering b-tokens, repayment is exactly the
/// floor-rounded d-tokens covered by those b-tokens capped by the debt,
/// and the returned residual is the kernel-computed remainder the
/// production consumer defaults on. A setoff is committed if and only if
/// the claim and b_rate are positive and that repayment rounds above zero.
/// Returning an arbitrarily smaller positive burn, or a residual computed
/// anywhere but inside the kernel, fails these assertions.
///
/// Domain: amount (d-token debt) positive u8 cast, claim (b-token supply)
/// u8 cast including zero, d_rate positive u8 hundredths of unity, b_rate
/// u8 hundredths of unity with zero explicitly included. Product fit: the
/// largest intermediate numerator is 66,705 * SCALAR_12 (~6.7e16), far
/// inside i128, so the dependency FixedPoint conversions equal the
/// production SorobanFixedPoint fast path; the host I256 fallback is never
/// taken on this domain and is not claimed. Oracle arithmetic is u64:
/// every operand and product stays below 2^63 on this domain, so Kani's
/// default overflow checks discharge representability and all casts are
/// lossless. No one-unit dust bound is asserted anywhere.
#[kani::proof]
fn prove_setoff_exact_policy_split_and_conditions() {
    let amount_u: u8 = kani::any();
    let claim_u: u8 = kani::any();
    let d_rate_u: u8 = kani::any();
    let b_rate_u: u8 = kani::any();
    // Liabilities map entries are positive; a zero d-rate cannot divide.
    kani::assume(amount_u > 0 && d_rate_u > 0);
    let amount = amount_u as i128;
    let claim = claim_u as i128;
    let data = setoff_data(rate_hundredths(d_rate_u), rate_hundredths(b_rate_u));

    let setoff = supply_setoff(&(), &data, amount, claim);

    // Independent u64 oracle over the same inputs: mirrors only the
    // ReserveData rounding contract (ceil/floor of x*y/z with these exact
    // operands), not the kernel's implementation.
    let scale = SCALAR_12 as u64;
    let d_rate_o = d_rate_u as u64 * scale / 100;
    let b_rate_o = b_rate_u as u64 * scale / 100;
    let (b_exp, repaid_exp) = if claim_u > 0 && b_rate_o > 0 {
        let debt_assets = (amount_u as u64 * d_rate_o).div_ceil(scale);
        let b_target = (debt_assets * scale).div_ceil(b_rate_o);
        let b_cap = (claim_u as u64).min(b_target);
        let covered = b_cap * b_rate_o / scale;
        let repaid = (amount_u as u64).min(covered * scale / d_rate_o);
        (b_cap, repaid)
    } else {
        (0, 0)
    };

    // Commit happens iff the real conditions hold: positive claim, positive
    // b_rate, and repayment rounding above zero.
    assert!((setoff.repaid > 0) == (repaid_exp > 0));

    if repaid_exp > 0 {
        // Exact accepted burn policy and rounding (caps follow: burn <=
        // claim and repaid <= amount by the oracle's own mins).
        assert!(setoff.burn == b_exp as i128);
        assert!(setoff.repaid == repaid_exp as i128);
        assert!(setoff.burn > 0 && setoff.burn <= claim);
        assert!(setoff.repaid > 0 && setoff.repaid <= amount);
        // True kernel debt split: the residual is the kernel's own
        // subtraction, consumed by the production default branch.
        assert!(setoff.residual >= 0 && setoff.repaid + setoff.residual == amount);
    } else {
        // No commit: zero claim, zero b_rate, or repayment rounding to
        // zero leaves the claim and the whole debt untouched.
        assert!(setoff.repaid == 0 && setoff.burn == 0 && setoff.residual == amount);
    }

    // C3 default link: with positive debt,
    // no committed setoff means a residual default, not absence of
    // default; only full repayment avoids the default branch.
    assert!(setoff.has_default() == (setoff.residual > 0));
    if setoff.repaid == 0 {
        assert!(setoff.residual == amount && setoff.has_default());
    }

    kani::cover!(setoff.repaid > 0 && setoff.residual == 0);
    kani::cover!(setoff.repaid > 0 && setoff.residual > 0);
    kani::cover!(setoff.repaid == 0 && claim == 0);
    kani::cover!(setoff.repaid == 0 && claim > 0 && data.b_rate == 0);
    kani::cover!(setoff.repaid == 0 && claim > 0 && data.b_rate > 0);
}

/// C3: the residual-collateral decision from
/// check_and_handle_user_bad_debt. Collateral is orphaned into pool
/// custody only when a default actually happened and collateral exists;
/// no residual default retains the user's collateral. The storage, map,
/// emission and event effects behind this decision remain host boundaries
/// and are not claimed here.
#[kani::proof]
fn prove_orphan_collateral_decision() {
    let had_default: bool = kani::any();
    let has_collateral: bool = kani::any();
    let orphan = should_orphan_collateral(had_default, has_collateral);
    assert!(orphan == (had_default && has_collateral));
    if !had_default {
        assert!(!orphan);
    }
    if !has_collateral {
        assert!(!orphan);
    }
    kani::cover!(had_default && has_collateral);
    kani::cover!(had_default && !has_collateral);
    kani::cover!(!had_default && has_collateral);
    kani::cover!(!had_default && !has_collateral);
}

// C1 S2: parametric caller over the four original inputs only (the C1
// domain: amount,d_rate positive u8; claim,b_rate u8 incl zero). The two
// verified helper families bind every intermediate to the original u64
// oracle before this caller may be counted toward C1; caller PASS alone is
// structural, not numeric closure.
struct SetoffMath {
    ceil_args: [(i128, i128, i128); 2],
    ceil_results: [i128; 2],
    floor_args: [(i128, i128, i128); 2],
    floor_results: [i128; 2],
    ceil_calls: core::cell::Cell<u32>,
    floor_calls: core::cell::Cell<u32>,
}

impl FixedMath for SetoffMath {
    fn floor(&self, x: i128, y: i128, denominator: i128) -> i128 {
        let slot = self.floor_calls.get() as usize;
        assert!(slot < 2);
        assert_eq!((x, y, denominator), self.floor_args[slot]);
        self.floor_calls.set(self.floor_calls.get() + 1);
        self.floor_results[slot]
    }

    fn ceil(&self, x: i128, y: i128, denominator: i128) -> i128 {
        let slot = self.ceil_calls.get() as usize;
        assert!(slot < 2);
        assert_eq!((x, y, denominator), self.ceil_args[slot]);
        self.ceil_calls.set(self.ceil_calls.get() + 1);
        self.ceil_results[slot]
    }
}

#[kani::proof]
fn prove_setoff_guarded_parametric() {
    let amount_u: u8 = kani::any();
    let claim_u: u8 = kani::any();
    let d_rate_h: u8 = kani::any();
    let b_rate_h: u8 = kani::any();
    // Liabilities map entries are positive; a zero d-rate cannot divide.
    kani::assume(amount_u > 0 && d_rate_h > 0);
    let amount = amount_u as i128;
    let claim = claim_u as i128;
    let data = setoff_data(rate_hundredths(d_rate_h), rate_hundredths(b_rate_h));

    // Numeric identity with the original u64 oracle is composed only
    // after all S0/S1 prerequisites pass; no numeric oracle is assumed here.

    // Helper-derived premises for the committed branch, per S0/S1:
    //   d_forward:  debt_assets == ceil(amount_u*d_rate_o/scale)
    //               in 1..=651
    //   b_inverse_up: b_target == ceil(debt_assets*scale/b_rate_o)
    //               in 1..=65100
    // Outside the guard, both conversions bypass arithmetic.
    let setoff;
    let mut pay_burn: i128 = 0;
    let mut pay_q: i128 = 0;
    if claim_u > 0 && data.b_rate > 0 {
        // Arbitrary helper outputs over supersets of the real images.
        // S0/S1 establish the original u64 identities and these bounds.
        let debt_assets: i128 = kani::any();
        kani::assume(debt_assets >= 1 && debt_assets <= 651);
        let b_target: i128 = kani::any();
        kani::assume(b_target >= 1 && b_target <= 65100);
        let burn_cap = claim.min(b_target);
        assert!(burn_cap > 0 && burn_cap <= 255);
        let covered: i128 = kani::any();
        kani::assume(covered >= 0 && covered <= 650);
        let repaid_target: i128 = kani::any();
        kani::assume(repaid_target >= 0 && repaid_target <= 65100);
        // Zero input is preserved by the real d-inverse helper.
        // The converse is false: a positive asset amount can round to zero.
        kani::assume(covered != 0 || repaid_target == 0);

        let math = SetoffMath {
            ceil_args: [
                (amount, data.d_rate, SCALAR_12),
                (debt_assets, SCALAR_12, data.b_rate),
            ],
            ceil_results: [debt_assets, b_target],
            floor_args: [
                (burn_cap, data.b_rate, SCALAR_12),
                (covered, SCALAR_12, data.d_rate),
            ],
            floor_results: [covered, repaid_target],
            ceil_calls: core::cell::Cell::new(0),
            floor_calls: core::cell::Cell::new(0),
        };
        setoff = supply_setoff(&math, &data, amount, claim);
        assert_eq!(math.ceil_calls.get(), 2);
        assert_eq!(math.floor_calls.get(), 2);
        pay_burn = burn_cap;
        pay_q = repaid_target;
    } else {
        // Zero claim/rate bypass: no arithmetic call fires.
        let math = SetoffMath {
            ceil_args: [(0, 0, 0); 2],
            ceil_results: [0; 2],
            floor_args: [(0, 0, 0); 2],
            floor_results: [0; 2],
            ceil_calls: core::cell::Cell::new(0),
            floor_calls: core::cell::Cell::new(0),
        };
        setoff = supply_setoff(&math, &data, amount, claim);
        assert_eq!(math.ceil_calls.get() + math.floor_calls.get(), 0);
    }

    // Exact mirrored policy (original assertions bad_debt.rs:1573-1598).
    let repaid_exp = amount.min(pay_q);
    if repaid_exp > 0 {
        assert!(setoff.burn == pay_burn);
        assert!(setoff.repaid == repaid_exp);
        assert!(setoff.burn > 0 && setoff.burn <= claim);
        assert!(setoff.repaid > 0 && setoff.repaid <= amount);
        assert!(setoff.residual >= 0 && setoff.repaid + setoff.residual == amount);
    } else {
        assert!(setoff.repaid == 0 && setoff.burn == 0 && setoff.residual == amount);
    }
    assert!(setoff.has_default() == (setoff.residual > 0));

    kani::cover!(setoff.repaid > 0 && setoff.residual == 0);
    kani::cover!(setoff.repaid > 0 && setoff.residual > 0);
    kani::cover!(setoff.repaid == 0 && claim == 0);
    kani::cover!(setoff.repaid == 0 && claim > 0 && data.b_rate == 0);
    kani::cover!(setoff.repaid == 0 && claim > 0 && data.b_rate > 0);
}

/// Witness harness for the guarded caller: actual () arithmetic on the
/// five approved tuples (SettlementProofDesign S2 covers). Each tuple must
/// exercise its named policy branch under the real kernel with the real
/// dependency implementation; no abstraction, no symbolic value.
#[kani::proof]
fn prove_setoff_guarded_witnesses() {
    let scale = SCALAR_12 as u64;
    // (amount_u, claim_u, d_h, b_h, label)
    let full = (1u8, 1u8, 100u8, 100u8);
    let partial = (2u8, 1u8, 100u8, 100u8);
    let zero_claim = (1u8, 0u8, 100u8, 100u8);
    let zero_b_rate = (1u8, 1u8, 100u8, 0u8);
    let rounded_zero = (1u8, 1u8, 100u8, 1u8);

    for params in [full, partial, zero_claim, zero_b_rate, rounded_zero] {
        let (a_u, c_u, d_h, b_h) = params;
        let amount = a_u as i128;
        let claim = c_u as i128;
        let data = setoff_data(rate_hundredths(d_h), rate_hundredths(b_h));
        let setoff = supply_setoff(&(), &data, amount, claim);

        // Independent oracle for the same tuple.
        let d_rate_o = d_h as u64 * scale / 100;
        let b_rate_o = b_h as u64 * scale / 100;
        let (b_exp, repaid_exp) = if c_u > 0 && b_rate_o > 0 {
            let debt_assets = (a_u as u64 * d_rate_o).div_ceil(scale);
            let b_target = (debt_assets * scale).div_ceil(b_rate_o);
            let b_cap = (c_u as u64).min(b_target);
            let covered = b_cap * b_rate_o / scale;
            let repaid = (a_u as u64).min(covered * scale / d_rate_o);
            (b_cap, repaid)
        } else {
            (0, 0)
        };

        if repaid_exp > 0 {
            assert!(setoff.burn == b_exp as i128);
            assert!(setoff.repaid == repaid_exp as i128);
            assert!(setoff.burn > 0 && setoff.burn <= claim);
            assert!(setoff.repaid > 0 && setoff.repaid <= amount);
            assert!(setoff.residual >= 0 && setoff.repaid + setoff.residual == amount);
        } else {
            assert!(setoff.repaid == 0 && setoff.burn == 0 && setoff.residual == amount);
        }
        assert!(setoff.has_default() == (setoff.residual > 0));
    }

    assert!(
        supply_setoff(
            &(),
            &setoff_data(rate_hundredths(full.2), rate_hundredths(full.3)),
            full.0 as i128,
            full.1 as i128
        )
        .residual
            == 0
    ); // full witness: whole debt settled
    assert!(
        supply_setoff(
            &(),
            &setoff_data(rate_hundredths(partial.2), rate_hundredths(partial.3)),
            partial.0 as i128,
            partial.1 as i128
        )
        .repaid
            < partial.0 as i128
    ); // partial witness: strictly partial repayment
    assert_eq!(
        supply_setoff(
            &(),
            &setoff_data(rate_hundredths(zero_claim.2), rate_hundredths(zero_claim.3)),
            zero_claim.0 as i128,
            0
        )
        .repaid,
        0
    ); // zero-claim witness
    assert_eq!(
        supply_setoff(
            &(),
            &setoff_data(
                rate_hundredths(zero_b_rate.2),
                rate_hundredths(zero_b_rate.3)
            ),
            zero_b_rate.0 as i128,
            zero_b_rate.1 as i128
        )
        .repaid,
        0
    ); // zero-b-rate witness
    assert_eq!(
        supply_setoff(
            &(),
            &setoff_data(
                rate_hundredths(rounded_zero.2),
                rate_hundredths(rounded_zero.3)
            ),
            rounded_zero.0 as i128,
            rounded_zero.1 as i128
        )
        .repaid,
        0
    ); // rounded-zero no-commit witness
}
