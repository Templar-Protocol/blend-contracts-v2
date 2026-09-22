use cast::i128;
use soroban_sdk::{panic_with_error, Env};

use crate::{
    constants::{SCALAR_12, SCALAR_7, SECONDS_PER_YEAR},
    math::FixedMath,
    storage::ReserveConfig,
    PoolError,
};

/// Calculates the loan accrual ratio for the Reserve based on the current utilization and
/// rate modifier for the reserve.
///
/// ### Arguments
/// * `config` - The Reserve config to calculate an accrual for
/// * `cur_util` - The current utilization rate of the reserve (7 decimals)
/// * `ir_mod` - The current interest rate modifier of the reserve (9 decimals)
/// * `last_block` - The last block an accrual was performed
///
/// ### Returns
/// * (i128, i128) - (accrual amount scaled to 9 decimal places, new interest rate modifier scaled to 9 decimal places)
#[allow(clippy::zero_prefixed_literal)]
pub fn calc_accrual(
    e: &Env,
    config: &ReserveConfig,
    cur_util: i128,
    ir_mod: i128,
    last_time: u64,
) -> (i128, i128) {
    // Preserve the original arithmetic-before-timestamp-error ordering.
    let cur_ir = current_interest(e, config, cur_util, ir_mod);

    // update rate_modifier
    let delta_time = i128(e.ledger().timestamp() - last_time);
    // this should never occur, but require some time to pass
    if delta_time < 1 {
        panic_with_error!(e, PoolError::InternalError);
    }
    finish_accrual(e, config, cur_util, ir_mod, delta_time, cur_ir)
}

#[allow(clippy::zero_prefixed_literal)]
fn current_interest(
    math: &impl FixedMath,
    config: &ReserveConfig,
    cur_util: i128,
    ir_mod: i128,
) -> i128 {
    let target_util = i128(config.util);
    if cur_util <= target_util {
        let util_scalar = math.ceil(cur_util, SCALAR_7, target_util);
        let base_rate = math.ceil(util_scalar, i128(config.r_one), SCALAR_7) + i128(config.r_base);
        math.ceil(base_rate, ir_mod, SCALAR_7)
    } else if cur_util <= 0_9500000 {
        let util_scalar = math.ceil(cur_util - target_util, SCALAR_7, 0_9500000 - target_util);
        let base_rate = math.ceil(util_scalar, i128(config.r_two), SCALAR_7)
            + i128(config.r_one)
            + i128(config.r_base);
        math.ceil(base_rate, ir_mod, SCALAR_7)
    } else {
        let util_scalar = math.ceil(cur_util - 0_9500000, SCALAR_7, 0_0500000);
        let extra_rate = math.ceil(util_scalar, i128(config.r_three), SCALAR_7);
        let intersection = math.ceil(
            ir_mod,
            i128(config.r_two + config.r_one + config.r_base),
            SCALAR_7,
        );
        extra_rate + intersection
    }
}

fn finish_accrual(
    math: &impl FixedMath,
    config: &ReserveConfig,
    cur_util: i128,
    ir_mod: i128,
    delta_time: i128,
    cur_ir: i128,
) -> (i128, i128) {
    let target_util = i128(config.util);
    // util dif 7 decimals
    let util_dif = cur_util - target_util;
    let new_ir_mod: i128;
    if util_dif >= 0 {
        // rate modifier increasing
        let util_error = delta_time * util_dif;
        let rate_dif = math.floor(util_error, i128(config.reactivity), SCALAR_7);
        let next_ir_mod = ir_mod + rate_dif;
        let ir_mod_max = 10 * SCALAR_7;
        if next_ir_mod > ir_mod_max {
            new_ir_mod = ir_mod_max;
        } else {
            new_ir_mod = next_ir_mod;
        }
    } else {
        // rate modifier decreasing
        let util_error = delta_time * util_dif;
        let rate_dif = math.ceil(util_error, i128(config.reactivity), SCALAR_7);
        let next_ir_mod = ir_mod + rate_dif;
        let ir_mod_min = SCALAR_7 / 10;
        if next_ir_mod < ir_mod_min {
            new_ir_mod = ir_mod_min;
        } else {
            new_ir_mod = next_ir_mod;
        }
    }

    // calc accrual amount over blocks
    // scale delta_time to 12 decimals so time_weight is scaled to 12 decimals
    let delta_time_scaled = delta_time * SCALAR_12;
    let time_weight = delta_time_scaled / SECONDS_PER_YEAR;
    (
        // accrual scaled to 12 decimals
        SCALAR_12 + math.ceil(time_weight, cur_ir, SCALAR_7),
        new_ir_mod,
    )
}

/// Metadata-valid reserve configuration from u8 symbolic seeds:
/// - util = k * 100_000 for k in [0, 90]: <= 0_9000000, including the
///   metadata-valid zero target (covered by prove_e1_divisors_clamps_unity;
///   prove_e1_boundary_values pins k >= 1 because its cur_util == target
///   boundary at k == 0 would need cur_util == 0, which the real caller
///   never reaches)
/// - harness seeds: cur_util = j * 100_000 for j in [1, 100],
///   ir_mod = m * 1_000_000 for m in [1, 100], delta_time in [1, 255];
///   a zero target routes cur_util > 0 to the middle branch, whose
///   util_scalar stays <= SCALAR_7 because that branch requires
///   cur_util <= 0_9500000 (cur_util == SCALAR_7 selects the high branch)
/// - r_base in [1000, 1255]: inside [0_0001000, 1_0000000)
/// - r_one <= r_two <= r_three (u8, ordered by caller assumption)
/// - reactivity <= 255 <= 0_0001000
/// The u32 rate sum r_two + r_one + r_base (<= 1765 here) cannot overflow on
/// this domain; overflow-freedom over all metadata-valid u32 tuples is NOT
/// claimed and stays a separately recorded representability premise.
#[cfg(kani)]
mod verification {
    use super::*;

    fn reserve_config(
        util_k: u8,
        r_one: u8,
        r_two: u8,
        r_three: u8,
        r_base: u8,
        reactivity: u8,
    ) -> ReserveConfig {
        ReserveConfig {
            index: 0,
            decimals: 7,
            c_factor: 0_7500000,
            l_factor: 0_7500000,
            util: u32::from(util_k) * 100_000,
            max_util: 0_9500000,
            r_base: 1000 + u32::from(r_base),
            r_one: u32::from(r_one),
            r_two: u32::from(r_two),
            r_three: u32::from(r_three),
            reactivity: u32::from(reactivity),
            supply_cap: 1000000000000000000,
            enabled: true,
        }
    }

    /// E1 kernel checks on an explicitly conditional domain. Premises:
    /// - metadata validity as enforced by require_valid_reserve_metadata
    ///   (config.rs:193-207), restated for the seeded fields, INCLUDING the
    ///   metadata-valid zero target (util == 0);
    /// - cur_util in (0, SCALAR_7]: positive utilization is the real caller
    ///   precondition (Reserve::load returns early at cur_util == 0,
    ///   reserve.rs:57-61); the SCALAR_7 cap is a declared 7-decimal
    ///   utilization-ratio proof domain, not a protocol limit proved here;
    /// - ir_mod in [SCALAR_7/10, 10*SCALAR_7]: the invariant range of stored
    ///   modifiers (initialized to SCALAR_7 at config.rs:155, only written by
    ///   this calculation at reserve.rs:70);
    /// - delta_time in [1, 255] seconds: positive elapsed time premise.
    ///
    /// Divisor validity (the <=-target branch requires cur_util <= target, so
    /// with cur_util > 0 it is entered only when target >= cur_util > 0; a
    /// zero target therefore routes to the middle branch whose divisor
    /// 0_9500000 - 0 > 0, and the middle divisor is positive because
    /// util <= 0_9000000; constant SCALAR_7 elsewhere) and product
    /// representability are checked by the proof itself: the Kani FixedMath
    /// implementation fails on zero divisors and checked-arithmetic overflow.
    #[kani::proof]
    fn prove_e1_divisors_clamps_unity() {
        let util_k: u8 = kani::any();
        let cur_j: u8 = kani::any();
        let ir_m: u8 = kani::any();
        let dt: u8 = kani::any();
        let r1: u8 = kani::any();
        let r2: u8 = kani::any();
        let r3: u8 = kani::any();
        let rb: u8 = kani::any();
        let react: u8 = kani::any();
        kani::assume(util_k <= 90); // zero target is metadata-valid and included
        kani::assume(cur_j >= 1 && cur_j <= 100);
        kani::assume(ir_m >= 1 && ir_m <= 100);
        kani::assume(dt >= 1);
        kani::assume(r1 <= r2 && r2 <= r3);
        let config = reserve_config(util_k, r1, r2, r3, rb, react);
        let cur_util = i128(cur_j) * 100_000;
        let ir_mod = i128(ir_m) * 1_000_000;

        let cur_ir = current_interest(&(), &config, cur_util, ir_mod);
        let (accrual, new_ir_mod) =
            finish_accrual(&(), &config, cur_util, ir_mod, i128(dt), cur_ir);

        // modifier respects the actual clamps
        assert!(new_ir_mod >= SCALAR_7 / 10);
        assert!(new_ir_mod <= 10 * SCALAR_7);
        // loan accrual is at least the unity scalar on nonnegative inputs
        assert!(accrual >= SCALAR_12);
        // zero-target coverage is reachable: cur_util > 0 sends it to branch 2
        kani::cover!(util_k == 0);
    }

    /// Piecewise boundary values against independent u128 oracles (single
    /// rounding: at each pinned boundary the util_scalar stage is exact, so
    /// the whole rate reduces to one ceil of a representable product).
    /// Target here is pinned positive (util_k >= 1): the cur_util == target
    /// boundary at target == 0 would need cur_util == 0, which the real
    /// caller never reaches (reserve.rs:57-61); zero-target behavior is
    /// covered by prove_e1_divisors_clamps_unity instead.
    #[kani::proof]
    fn prove_e1_boundary_values() {
        let util_k: u8 = kani::any();
        let r1: u8 = kani::any();
        let r2: u8 = kani::any();
        let r3: u8 = kani::any();
        let rb: u8 = kani::any();
        let ir_m: u8 = kani::any();
        let react: u8 = kani::any();
        kani::assume(util_k >= 1 && util_k <= 90);
        kani::assume(r1 <= r2 && r2 <= r3);
        kani::assume(ir_m >= 1 && ir_m <= 100);
        let config = reserve_config(util_k, r1, r2, r3, rb, react);
        let ir_mod = i128(ir_m) * 1_000_000;
        let im = u128::from(ir_m) * 1_000_000;
        let s7 = SCALAR_7 as u128;

        // cur_util == target_util: util_scalar == SCALAR_7 exactly
        let target = i128(util_k) * 100_000;
        let expected_a = ((u128::from(config.r_one + config.r_base) * im + s7 - 1) / s7) as i128;
        assert_eq!(current_interest(&(), &config, target, ir_mod), expected_a);

        // cur_util == 0_9500000: middle-branch boundary, util_scalar == SCALAR_7 exactly
        let expected_b =
            ((u128::from(config.r_two + config.r_one + config.r_base) * im + s7 - 1) / s7) as i128;
        assert_eq!(
            current_interest(&(), &config, 0_9500000, ir_mod),
            expected_b
        );

        // cur_util == SCALAR_7: high branch, extra stage exact at r_three
        let expected_c = i128(config.r_three) + expected_b;
        assert_eq!(current_interest(&(), &config, SCALAR_7, ir_mod), expected_c);

        // modifier clamp witnesses (concrete, hand-derived):
        // upper clamp: ir_mod = 10*SCALAR_7, increasing error 255 * 900_000 * 255 / 10^7
        let mut hi = config.clone();
        hi.util = 100_000; // target 0.01
        hi.reactivity = 255;
        assert_eq!(
            finish_accrual(&(), &hi, 1_000_000, 100_000_000, 255, 0).1,
            10 * SCALAR_7
        );
        // lower clamp: ir_mod = SCALAR_7/10, decreasing error 255 * 8_900_000 * 255 / 10^7
        let mut lo = config.clone();
        lo.util = 9_000_000; // target 0.9
        lo.reactivity = 255;
        assert_eq!(
            finish_accrual(&(), &lo, 100_000, 1_000_000, 255, 0).1,
            SCALAR_7 / 10
        );
        // zero reactivity leaves the modifier untouched
        let mut flat = config.clone();
        flat.reactivity = 0;
        assert_eq!(finish_accrual(&(), &flat, target, ir_mod, 200, 0).1, ir_mod);
    }

    // Composition prerequisites are actual dependency arithmetic, not guard results.
    // All three must pass before any composed caller may be consumed.
    #[kani::proof]
    fn astra_e1_numeric_cancel() {
        let a: i128 = kani::any();
        kani::assume(a >= 1 && a <= SCALAR_7);
        assert_eq!(().ceil(a, SCALAR_7, a), SCALAR_7);
        kani::cover!(a == 1);
        kani::cover!(a == SCALAR_7);
    }
    // Cancellation image: the caller kernel only ever invokes stage 0 with
    // a == 100_000 * k for k in [1, 94]; this const helper proves the real
    // dependency cancellation law on exactly that image, one K at a time.
    fn astra_e1_cancel_image<const K: u8>() {
        assert!(K >= 1 && K <= 94);
        let a = 100_000 * i128::from(K);
        assert_eq!(().ceil(a, SCALAR_7, a), SCALAR_7);
        kani::cover!(true);
    }

    #[kani::proof]
    fn astra_e1_numeric_rate_identity() {
        let r: u8 = kani::any();
        assert_eq!(().ceil(SCALAR_7, i128(r), SCALAR_7), i128(r));
        kani::cover!(r == 0);
        kani::cover!(r == 255);
    }

    #[kani::proof]
    fn astra_e1_numeric_scaled_base() {
        let base: u16 = kani::any();
        let m: u8 = kani::any();
        kani::assume(base >= 1000 && base <= 1765);
        kani::assume(m >= 1 && m <= 100);
        let modifier = i128(m) * 1_000_000;
        let im = u128::from(m) * 1_000_000;
        let s7 = SCALAR_7 as u128;
        let expected = ((u128::from(base) * im + s7 - 1) / s7) as i128;
        assert_eq!(().ceil(i128(base), modifier, SCALAR_7), expected);
        assert_eq!(().ceil(modifier, i128(base), SCALAR_7), expected);
        // Admitted concrete witnesses distinguish exact and rounded division.
        kani::cover!(base == 1000 && m == 1);
        kani::cover!(base == 1765 && m == 100);
        kani::cover!(base == 1000 && m == 40 && (u128::from(base) * im) % s7 == 0);
        kani::cover!(base == 1001 && m == 1 && (u128::from(base) * im) % s7 != 0);
    }

    // Only a source-linked composition checker: it cannot replace numeric receipts.
    // N1 supplies stage 0, N2 stage 1, and both N3 orientations supply stage 2.
    struct InterestComposition {
        operands: [(i128, i128, i128); 3],
        scaled_base: i128,
        ir_m: u8,
        reverse: bool,
        max_base: i128,
        stage: core::cell::Cell<usize>,
    }

    impl FixedMath for InterestComposition {
        fn floor(&self, _: i128, _: i128, _: i128) -> i128 {
            panic!("current_interest must not call floor")
        }

        fn ceil(&self, x: i128, y: i128, denominator: i128) -> i128 {
            let stage = self.stage.get();
            assert!(stage < 3);
            assert_eq!((x, y, denominator), self.operands[stage]);
            let result = match stage {
                0 => {
                    assert!(x >= 1 && x <= SCALAR_7);
                    assert_eq!(y, SCALAR_7);
                    assert_eq!(denominator, x);
                    assert!(
                        100_000 <= x && x <= 9_400_000 && x % 100_000 == 0,
                        "stage-0 cancellation operand outside the proved 100_000*k image (k in [1,94])"
                    );
                    SCALAR_7
                }
                1 => {
                    assert_eq!(x, SCALAR_7);
                    assert!(y >= 0 && y <= 255);
                    assert_eq!(denominator, SCALAR_7);
                    y
                }
                2 => {
                    assert_eq!(denominator, SCALAR_7);
                    assert!(self.ir_m >= 1 && self.ir_m <= 100);
                    let modifier = i128(self.ir_m) * 1_000_000;
                    if self.reverse {
                        assert_eq!(x, modifier);
                        assert!(y >= 1000 && y <= 1765);
                    } else {
                        assert!(x >= 1000 && x <= self.max_base);
                        assert_eq!(y, modifier);
                    }
                    self.scaled_base
                }
                _ => unreachable!(),
            };
            self.stage.set(stage + 1);
            result
        }
    }

    #[kani::proof]
    fn astra_e1_composed_target() {
        let util_k: u8 = kani::any();
        let r1: u8 = kani::any();
        let r2: u8 = kani::any();
        let r3: u8 = kani::any();
        let rb: u8 = kani::any();
        let ir_m: u8 = kani::any();
        let react: u8 = kani::any();
        kani::assume(util_k >= 1 && util_k <= 90);
        kani::assume(r1 <= r2 && r2 <= r3);
        kani::assume(ir_m >= 1 && ir_m <= 100);
        let config = reserve_config(util_k, r1, r2, r3, rb, react);
        let ir_mod = i128(ir_m) * 1_000_000;
        let im = u128::from(ir_m) * 1_000_000;
        let s7 = SCALAR_7 as u128;
        let target = i128(util_k) * 100_000;
        let expected_a = ((u128::from(config.r_one + config.r_base) * im + s7 - 1) / s7) as i128;
        let base = i128(config.r_one) + i128(config.r_base);
        assert!(target >= 100_000 && target <= 9_000_000);
        assert!(base >= 1000 && base <= 1510);
        let math = InterestComposition {
            operands: [
                (target, SCALAR_7, target),
                (SCALAR_7, i128(config.r_one), SCALAR_7),
                (base, ir_mod, SCALAR_7),
            ],
            scaled_base: expected_a,
            ir_m,
            reverse: false,
            stage: core::cell::Cell::new(0),
            max_base: 1510,
        };
        let result = current_interest(&math, &config, target, ir_mod);
        assert_eq!(math.stage.get(), 3);
        assert_eq!(result, expected_a);
        kani::cover!(true);
    }

    #[kani::proof]
    fn astra_e1_composed_ninety_five() {
        let util_k: u8 = kani::any();
        let r1: u8 = kani::any();
        let r2: u8 = kani::any();
        let r3: u8 = kani::any();
        let rb: u8 = kani::any();
        let ir_m: u8 = kani::any();
        let react: u8 = kani::any();
        kani::assume(util_k >= 1 && util_k <= 90);
        kani::assume(r1 <= r2 && r2 <= r3);
        kani::assume(ir_m >= 1 && ir_m <= 100);
        let config = reserve_config(util_k, r1, r2, r3, rb, react);
        let ir_mod = i128(ir_m) * 1_000_000;
        let im = u128::from(ir_m) * 1_000_000;
        let s7 = SCALAR_7 as u128;
        let target = i128(util_k) * 100_000;
        let expected_b =
            ((u128::from(config.r_two + config.r_one + config.r_base) * im + s7 - 1) / s7) as i128;
        let cancellation = 0_9500000 - target;
        let base = i128(config.r_two) + i128(config.r_one) + i128(config.r_base);
        assert!(cancellation >= 500_000 && cancellation <= 9_400_000);
        let math = InterestComposition {
            operands: [
                (cancellation, SCALAR_7, cancellation),
                (SCALAR_7, i128(config.r_two), SCALAR_7),
                (base, ir_mod, SCALAR_7),
            ],
            scaled_base: expected_b,
            max_base: 1765,
            ir_m,
            reverse: false,
            stage: core::cell::Cell::new(0),
        };
        let result = current_interest(&math, &config, 0_9500000, ir_mod);
        assert_eq!(math.stage.get(), 3);
        assert_eq!(result, expected_b);
        kani::cover!(true);
    }

    #[kani::proof]
    fn astra_e1_composed_hundred() {
        let util_k: u8 = kani::any();
        let r1: u8 = kani::any();
        let r2: u8 = kani::any();
        let r3: u8 = kani::any();
        let rb: u8 = kani::any();
        let ir_m: u8 = kani::any();
        let react: u8 = kani::any();
        kani::assume(util_k >= 1 && util_k <= 90);
        kani::assume(r1 <= r2 && r2 <= r3);
        kani::assume(ir_m >= 1 && ir_m <= 100);
        let config = reserve_config(util_k, r1, r2, r3, rb, react);
        let ir_mod = i128(ir_m) * 1_000_000;
        let im = u128::from(ir_m) * 1_000_000;
        let s7 = SCALAR_7 as u128;
        let expected_b =
            ((u128::from(config.r_two + config.r_one + config.r_base) * im + s7 - 1) / s7) as i128;
        let expected_c = i128(config.r_three) + expected_b;
        let base = i128(config.r_two) + i128(config.r_one) + i128(config.r_base);
        let math = InterestComposition {
            operands: [
                (500_000, SCALAR_7, 500_000),
                (SCALAR_7, i128(config.r_three), SCALAR_7),
                (ir_mod, base, SCALAR_7),
            ],
            scaled_base: expected_b,
            ir_m,
            max_base: 1765,
            reverse: true,
            stage: core::cell::Cell::new(0),
        };
        let result = current_interest(&math, &config, SCALAR_7, ir_mod);
        assert_eq!(math.stage.get(), 3);
        assert_eq!(result, expected_c);
        kani::cover!(true);
    }

    macro_rules! proof_case {
        ($name:ident, $helper:ident, $value:literal) => {
            #[kani::proof]
            fn $name() {
                $helper::<$value>();
            }
        };
    }

    proof_case!(astra_e1_cancel_image_k01, astra_e1_cancel_image, 1);

    proof_case!(astra_e1_cancel_image_k02, astra_e1_cancel_image, 2);

    proof_case!(astra_e1_cancel_image_k03, astra_e1_cancel_image, 3);

    proof_case!(astra_e1_cancel_image_k04, astra_e1_cancel_image, 4);

    proof_case!(astra_e1_cancel_image_k05, astra_e1_cancel_image, 5);

    proof_case!(astra_e1_cancel_image_k06, astra_e1_cancel_image, 6);

    proof_case!(astra_e1_cancel_image_k07, astra_e1_cancel_image, 7);

    proof_case!(astra_e1_cancel_image_k08, astra_e1_cancel_image, 8);

    proof_case!(astra_e1_cancel_image_k09, astra_e1_cancel_image, 9);

    proof_case!(astra_e1_cancel_image_k10, astra_e1_cancel_image, 10);

    proof_case!(astra_e1_cancel_image_k11, astra_e1_cancel_image, 11);

    proof_case!(astra_e1_cancel_image_k12, astra_e1_cancel_image, 12);

    proof_case!(astra_e1_cancel_image_k13, astra_e1_cancel_image, 13);

    proof_case!(astra_e1_cancel_image_k14, astra_e1_cancel_image, 14);

    proof_case!(astra_e1_cancel_image_k15, astra_e1_cancel_image, 15);

    proof_case!(astra_e1_cancel_image_k16, astra_e1_cancel_image, 16);

    proof_case!(astra_e1_cancel_image_k17, astra_e1_cancel_image, 17);

    proof_case!(astra_e1_cancel_image_k18, astra_e1_cancel_image, 18);

    proof_case!(astra_e1_cancel_image_k19, astra_e1_cancel_image, 19);

    proof_case!(astra_e1_cancel_image_k20, astra_e1_cancel_image, 20);

    proof_case!(astra_e1_cancel_image_k21, astra_e1_cancel_image, 21);

    proof_case!(astra_e1_cancel_image_k22, astra_e1_cancel_image, 22);

    proof_case!(astra_e1_cancel_image_k23, astra_e1_cancel_image, 23);

    proof_case!(astra_e1_cancel_image_k24, astra_e1_cancel_image, 24);

    proof_case!(astra_e1_cancel_image_k25, astra_e1_cancel_image, 25);

    proof_case!(astra_e1_cancel_image_k26, astra_e1_cancel_image, 26);

    proof_case!(astra_e1_cancel_image_k27, astra_e1_cancel_image, 27);

    proof_case!(astra_e1_cancel_image_k28, astra_e1_cancel_image, 28);

    proof_case!(astra_e1_cancel_image_k29, astra_e1_cancel_image, 29);

    proof_case!(astra_e1_cancel_image_k30, astra_e1_cancel_image, 30);

    proof_case!(astra_e1_cancel_image_k31, astra_e1_cancel_image, 31);

    proof_case!(astra_e1_cancel_image_k32, astra_e1_cancel_image, 32);

    proof_case!(astra_e1_cancel_image_k33, astra_e1_cancel_image, 33);

    proof_case!(astra_e1_cancel_image_k34, astra_e1_cancel_image, 34);

    proof_case!(astra_e1_cancel_image_k35, astra_e1_cancel_image, 35);

    proof_case!(astra_e1_cancel_image_k36, astra_e1_cancel_image, 36);

    proof_case!(astra_e1_cancel_image_k37, astra_e1_cancel_image, 37);

    proof_case!(astra_e1_cancel_image_k38, astra_e1_cancel_image, 38);

    proof_case!(astra_e1_cancel_image_k39, astra_e1_cancel_image, 39);

    proof_case!(astra_e1_cancel_image_k40, astra_e1_cancel_image, 40);

    proof_case!(astra_e1_cancel_image_k41, astra_e1_cancel_image, 41);

    proof_case!(astra_e1_cancel_image_k42, astra_e1_cancel_image, 42);

    proof_case!(astra_e1_cancel_image_k43, astra_e1_cancel_image, 43);

    proof_case!(astra_e1_cancel_image_k44, astra_e1_cancel_image, 44);

    proof_case!(astra_e1_cancel_image_k45, astra_e1_cancel_image, 45);

    proof_case!(astra_e1_cancel_image_k46, astra_e1_cancel_image, 46);

    proof_case!(astra_e1_cancel_image_k47, astra_e1_cancel_image, 47);

    proof_case!(astra_e1_cancel_image_k48, astra_e1_cancel_image, 48);

    proof_case!(astra_e1_cancel_image_k49, astra_e1_cancel_image, 49);

    proof_case!(astra_e1_cancel_image_k50, astra_e1_cancel_image, 50);

    proof_case!(astra_e1_cancel_image_k51, astra_e1_cancel_image, 51);

    proof_case!(astra_e1_cancel_image_k52, astra_e1_cancel_image, 52);

    proof_case!(astra_e1_cancel_image_k53, astra_e1_cancel_image, 53);

    proof_case!(astra_e1_cancel_image_k54, astra_e1_cancel_image, 54);

    proof_case!(astra_e1_cancel_image_k55, astra_e1_cancel_image, 55);

    proof_case!(astra_e1_cancel_image_k56, astra_e1_cancel_image, 56);

    proof_case!(astra_e1_cancel_image_k57, astra_e1_cancel_image, 57);

    proof_case!(astra_e1_cancel_image_k58, astra_e1_cancel_image, 58);

    proof_case!(astra_e1_cancel_image_k59, astra_e1_cancel_image, 59);

    proof_case!(astra_e1_cancel_image_k60, astra_e1_cancel_image, 60);

    proof_case!(astra_e1_cancel_image_k61, astra_e1_cancel_image, 61);

    proof_case!(astra_e1_cancel_image_k62, astra_e1_cancel_image, 62);

    proof_case!(astra_e1_cancel_image_k63, astra_e1_cancel_image, 63);

    proof_case!(astra_e1_cancel_image_k64, astra_e1_cancel_image, 64);

    proof_case!(astra_e1_cancel_image_k65, astra_e1_cancel_image, 65);

    proof_case!(astra_e1_cancel_image_k66, astra_e1_cancel_image, 66);

    proof_case!(astra_e1_cancel_image_k67, astra_e1_cancel_image, 67);

    proof_case!(astra_e1_cancel_image_k68, astra_e1_cancel_image, 68);

    proof_case!(astra_e1_cancel_image_k69, astra_e1_cancel_image, 69);

    proof_case!(astra_e1_cancel_image_k70, astra_e1_cancel_image, 70);

    proof_case!(astra_e1_cancel_image_k71, astra_e1_cancel_image, 71);

    proof_case!(astra_e1_cancel_image_k72, astra_e1_cancel_image, 72);

    proof_case!(astra_e1_cancel_image_k73, astra_e1_cancel_image, 73);

    proof_case!(astra_e1_cancel_image_k74, astra_e1_cancel_image, 74);

    proof_case!(astra_e1_cancel_image_k75, astra_e1_cancel_image, 75);

    proof_case!(astra_e1_cancel_image_k76, astra_e1_cancel_image, 76);

    proof_case!(astra_e1_cancel_image_k77, astra_e1_cancel_image, 77);

    proof_case!(astra_e1_cancel_image_k78, astra_e1_cancel_image, 78);

    proof_case!(astra_e1_cancel_image_k79, astra_e1_cancel_image, 79);

    proof_case!(astra_e1_cancel_image_k80, astra_e1_cancel_image, 80);

    proof_case!(astra_e1_cancel_image_k81, astra_e1_cancel_image, 81);

    proof_case!(astra_e1_cancel_image_k82, astra_e1_cancel_image, 82);

    proof_case!(astra_e1_cancel_image_k83, astra_e1_cancel_image, 83);

    proof_case!(astra_e1_cancel_image_k84, astra_e1_cancel_image, 84);

    proof_case!(astra_e1_cancel_image_k85, astra_e1_cancel_image, 85);

    proof_case!(astra_e1_cancel_image_k86, astra_e1_cancel_image, 86);

    proof_case!(astra_e1_cancel_image_k87, astra_e1_cancel_image, 87);

    proof_case!(astra_e1_cancel_image_k88, astra_e1_cancel_image, 88);

    proof_case!(astra_e1_cancel_image_k89, astra_e1_cancel_image, 89);

    proof_case!(astra_e1_cancel_image_k90, astra_e1_cancel_image, 90);

    proof_case!(astra_e1_cancel_image_k91, astra_e1_cancel_image, 91);

    proof_case!(astra_e1_cancel_image_k92, astra_e1_cancel_image, 92);

    proof_case!(astra_e1_cancel_image_k93, astra_e1_cancel_image, 93);

    proof_case!(astra_e1_cancel_image_k94, astra_e1_cancel_image, 94);

    fn astra_e1_numeric_scaled_modifier<const M: u8>() {
        let base: u16 = kani::any();
        let m: u8 = M;
        kani::assume(base >= 1000 && base <= 1765);
        assert!(M >= 1 && M <= 100);
        let modifier = i128(m) * 1_000_000;
        let im = u128::from(m) * 1_000_000;
        let s7 = SCALAR_7 as u128;
        let expected = ((u128::from(base) * im + s7 - 1) / s7) as i128;
        assert_eq!(().ceil(i128(base), modifier, SCALAR_7), expected);
        assert_eq!(().ceil(modifier, i128(base), SCALAR_7), expected);
        kani::cover!(base == 1000);
        kani::cover!(base == 1765);
    }

    proof_case!(
        astra_e1_numeric_scaled_m001,
        astra_e1_numeric_scaled_modifier,
        1
    );

    proof_case!(
        astra_e1_numeric_scaled_m040,
        astra_e1_numeric_scaled_modifier,
        40
    );

    proof_case!(
        astra_e1_numeric_scaled_m100,
        astra_e1_numeric_scaled_modifier,
        100
    );

    #[kani::proof]
    fn astra_e1_numeric_scaled_nonexact_witness() {
        let base: u16 = 1001;
        let m: u8 = 1;
        let modifier = i128(m) * 1_000_000;
        let im = u128::from(m) * 1_000_000;
        let s7 = SCALAR_7 as u128;
        let expected = ((u128::from(base) * im + s7 - 1) / s7) as i128;
        assert_eq!(().ceil(i128(base), modifier, SCALAR_7), expected);
        assert_eq!(().ceil(modifier, i128(base), SCALAR_7), expected);
        kani::cover!(base == 1001 && m == 1 && (u128::from(base) * im) % s7 != 0);
    }

    proof_case!(
        astra_e1_numeric_scaled_m002,
        astra_e1_numeric_scaled_modifier,
        2
    );

    proof_case!(
        astra_e1_numeric_scaled_m003,
        astra_e1_numeric_scaled_modifier,
        3
    );

    proof_case!(
        astra_e1_numeric_scaled_m004,
        astra_e1_numeric_scaled_modifier,
        4
    );

    proof_case!(
        astra_e1_numeric_scaled_m005,
        astra_e1_numeric_scaled_modifier,
        5
    );

    proof_case!(
        astra_e1_numeric_scaled_m006,
        astra_e1_numeric_scaled_modifier,
        6
    );

    proof_case!(
        astra_e1_numeric_scaled_m007,
        astra_e1_numeric_scaled_modifier,
        7
    );

    proof_case!(
        astra_e1_numeric_scaled_m008,
        astra_e1_numeric_scaled_modifier,
        8
    );

    proof_case!(
        astra_e1_numeric_scaled_m009,
        astra_e1_numeric_scaled_modifier,
        9
    );

    proof_case!(
        astra_e1_numeric_scaled_m010,
        astra_e1_numeric_scaled_modifier,
        10
    );

    proof_case!(
        astra_e1_numeric_scaled_m011,
        astra_e1_numeric_scaled_modifier,
        11
    );

    proof_case!(
        astra_e1_numeric_scaled_m012,
        astra_e1_numeric_scaled_modifier,
        12
    );

    proof_case!(
        astra_e1_numeric_scaled_m013,
        astra_e1_numeric_scaled_modifier,
        13
    );

    proof_case!(
        astra_e1_numeric_scaled_m014,
        astra_e1_numeric_scaled_modifier,
        14
    );

    proof_case!(
        astra_e1_numeric_scaled_m015,
        astra_e1_numeric_scaled_modifier,
        15
    );

    proof_case!(
        astra_e1_numeric_scaled_m016,
        astra_e1_numeric_scaled_modifier,
        16
    );

    proof_case!(
        astra_e1_numeric_scaled_m017,
        astra_e1_numeric_scaled_modifier,
        17
    );

    proof_case!(
        astra_e1_numeric_scaled_m018,
        astra_e1_numeric_scaled_modifier,
        18
    );

    proof_case!(
        astra_e1_numeric_scaled_m019,
        astra_e1_numeric_scaled_modifier,
        19
    );

    proof_case!(
        astra_e1_numeric_scaled_m020,
        astra_e1_numeric_scaled_modifier,
        20
    );

    proof_case!(
        astra_e1_numeric_scaled_m021,
        astra_e1_numeric_scaled_modifier,
        21
    );

    proof_case!(
        astra_e1_numeric_scaled_m022,
        astra_e1_numeric_scaled_modifier,
        22
    );

    proof_case!(
        astra_e1_numeric_scaled_m023,
        astra_e1_numeric_scaled_modifier,
        23
    );

    proof_case!(
        astra_e1_numeric_scaled_m024,
        astra_e1_numeric_scaled_modifier,
        24
    );

    proof_case!(
        astra_e1_numeric_scaled_m025,
        astra_e1_numeric_scaled_modifier,
        25
    );

    proof_case!(
        astra_e1_numeric_scaled_m026,
        astra_e1_numeric_scaled_modifier,
        26
    );

    proof_case!(
        astra_e1_numeric_scaled_m027,
        astra_e1_numeric_scaled_modifier,
        27
    );

    proof_case!(
        astra_e1_numeric_scaled_m028,
        astra_e1_numeric_scaled_modifier,
        28
    );

    proof_case!(
        astra_e1_numeric_scaled_m029,
        astra_e1_numeric_scaled_modifier,
        29
    );

    proof_case!(
        astra_e1_numeric_scaled_m030,
        astra_e1_numeric_scaled_modifier,
        30
    );

    proof_case!(
        astra_e1_numeric_scaled_m031,
        astra_e1_numeric_scaled_modifier,
        31
    );

    proof_case!(
        astra_e1_numeric_scaled_m032,
        astra_e1_numeric_scaled_modifier,
        32
    );

    proof_case!(
        astra_e1_numeric_scaled_m033,
        astra_e1_numeric_scaled_modifier,
        33
    );

    proof_case!(
        astra_e1_numeric_scaled_m034,
        astra_e1_numeric_scaled_modifier,
        34
    );

    proof_case!(
        astra_e1_numeric_scaled_m035,
        astra_e1_numeric_scaled_modifier,
        35
    );

    proof_case!(
        astra_e1_numeric_scaled_m036,
        astra_e1_numeric_scaled_modifier,
        36
    );

    proof_case!(
        astra_e1_numeric_scaled_m037,
        astra_e1_numeric_scaled_modifier,
        37
    );

    proof_case!(
        astra_e1_numeric_scaled_m038,
        astra_e1_numeric_scaled_modifier,
        38
    );

    proof_case!(
        astra_e1_numeric_scaled_m039,
        astra_e1_numeric_scaled_modifier,
        39
    );

    proof_case!(
        astra_e1_numeric_scaled_m041,
        astra_e1_numeric_scaled_modifier,
        41
    );

    proof_case!(
        astra_e1_numeric_scaled_m042,
        astra_e1_numeric_scaled_modifier,
        42
    );

    proof_case!(
        astra_e1_numeric_scaled_m043,
        astra_e1_numeric_scaled_modifier,
        43
    );

    proof_case!(
        astra_e1_numeric_scaled_m044,
        astra_e1_numeric_scaled_modifier,
        44
    );

    proof_case!(
        astra_e1_numeric_scaled_m045,
        astra_e1_numeric_scaled_modifier,
        45
    );

    proof_case!(
        astra_e1_numeric_scaled_m046,
        astra_e1_numeric_scaled_modifier,
        46
    );

    proof_case!(
        astra_e1_numeric_scaled_m047,
        astra_e1_numeric_scaled_modifier,
        47
    );

    proof_case!(
        astra_e1_numeric_scaled_m048,
        astra_e1_numeric_scaled_modifier,
        48
    );

    proof_case!(
        astra_e1_numeric_scaled_m049,
        astra_e1_numeric_scaled_modifier,
        49
    );

    proof_case!(
        astra_e1_numeric_scaled_m050,
        astra_e1_numeric_scaled_modifier,
        50
    );

    proof_case!(
        astra_e1_numeric_scaled_m051,
        astra_e1_numeric_scaled_modifier,
        51
    );

    proof_case!(
        astra_e1_numeric_scaled_m052,
        astra_e1_numeric_scaled_modifier,
        52
    );

    proof_case!(
        astra_e1_numeric_scaled_m053,
        astra_e1_numeric_scaled_modifier,
        53
    );

    proof_case!(
        astra_e1_numeric_scaled_m054,
        astra_e1_numeric_scaled_modifier,
        54
    );

    proof_case!(
        astra_e1_numeric_scaled_m055,
        astra_e1_numeric_scaled_modifier,
        55
    );

    proof_case!(
        astra_e1_numeric_scaled_m056,
        astra_e1_numeric_scaled_modifier,
        56
    );

    proof_case!(
        astra_e1_numeric_scaled_m057,
        astra_e1_numeric_scaled_modifier,
        57
    );

    proof_case!(
        astra_e1_numeric_scaled_m058,
        astra_e1_numeric_scaled_modifier,
        58
    );

    proof_case!(
        astra_e1_numeric_scaled_m059,
        astra_e1_numeric_scaled_modifier,
        59
    );

    proof_case!(
        astra_e1_numeric_scaled_m060,
        astra_e1_numeric_scaled_modifier,
        60
    );

    proof_case!(
        astra_e1_numeric_scaled_m061,
        astra_e1_numeric_scaled_modifier,
        61
    );

    proof_case!(
        astra_e1_numeric_scaled_m062,
        astra_e1_numeric_scaled_modifier,
        62
    );

    proof_case!(
        astra_e1_numeric_scaled_m063,
        astra_e1_numeric_scaled_modifier,
        63
    );

    proof_case!(
        astra_e1_numeric_scaled_m064,
        astra_e1_numeric_scaled_modifier,
        64
    );

    proof_case!(
        astra_e1_numeric_scaled_m065,
        astra_e1_numeric_scaled_modifier,
        65
    );

    proof_case!(
        astra_e1_numeric_scaled_m066,
        astra_e1_numeric_scaled_modifier,
        66
    );

    proof_case!(
        astra_e1_numeric_scaled_m067,
        astra_e1_numeric_scaled_modifier,
        67
    );

    proof_case!(
        astra_e1_numeric_scaled_m068,
        astra_e1_numeric_scaled_modifier,
        68
    );

    proof_case!(
        astra_e1_numeric_scaled_m069,
        astra_e1_numeric_scaled_modifier,
        69
    );

    proof_case!(
        astra_e1_numeric_scaled_m070,
        astra_e1_numeric_scaled_modifier,
        70
    );

    proof_case!(
        astra_e1_numeric_scaled_m071,
        astra_e1_numeric_scaled_modifier,
        71
    );

    proof_case!(
        astra_e1_numeric_scaled_m072,
        astra_e1_numeric_scaled_modifier,
        72
    );

    proof_case!(
        astra_e1_numeric_scaled_m073,
        astra_e1_numeric_scaled_modifier,
        73
    );

    proof_case!(
        astra_e1_numeric_scaled_m074,
        astra_e1_numeric_scaled_modifier,
        74
    );

    proof_case!(
        astra_e1_numeric_scaled_m075,
        astra_e1_numeric_scaled_modifier,
        75
    );

    proof_case!(
        astra_e1_numeric_scaled_m076,
        astra_e1_numeric_scaled_modifier,
        76
    );

    proof_case!(
        astra_e1_numeric_scaled_m077,
        astra_e1_numeric_scaled_modifier,
        77
    );

    proof_case!(
        astra_e1_numeric_scaled_m078,
        astra_e1_numeric_scaled_modifier,
        78
    );

    proof_case!(
        astra_e1_numeric_scaled_m079,
        astra_e1_numeric_scaled_modifier,
        79
    );

    proof_case!(
        astra_e1_numeric_scaled_m080,
        astra_e1_numeric_scaled_modifier,
        80
    );

    proof_case!(
        astra_e1_numeric_scaled_m081,
        astra_e1_numeric_scaled_modifier,
        81
    );

    proof_case!(
        astra_e1_numeric_scaled_m082,
        astra_e1_numeric_scaled_modifier,
        82
    );

    proof_case!(
        astra_e1_numeric_scaled_m083,
        astra_e1_numeric_scaled_modifier,
        83
    );

    proof_case!(
        astra_e1_numeric_scaled_m084,
        astra_e1_numeric_scaled_modifier,
        84
    );

    proof_case!(
        astra_e1_numeric_scaled_m085,
        astra_e1_numeric_scaled_modifier,
        85
    );

    proof_case!(
        astra_e1_numeric_scaled_m086,
        astra_e1_numeric_scaled_modifier,
        86
    );

    proof_case!(
        astra_e1_numeric_scaled_m087,
        astra_e1_numeric_scaled_modifier,
        87
    );

    proof_case!(
        astra_e1_numeric_scaled_m088,
        astra_e1_numeric_scaled_modifier,
        88
    );

    proof_case!(
        astra_e1_numeric_scaled_m089,
        astra_e1_numeric_scaled_modifier,
        89
    );

    proof_case!(
        astra_e1_numeric_scaled_m090,
        astra_e1_numeric_scaled_modifier,
        90
    );

    proof_case!(
        astra_e1_numeric_scaled_m091,
        astra_e1_numeric_scaled_modifier,
        91
    );

    proof_case!(
        astra_e1_numeric_scaled_m092,
        astra_e1_numeric_scaled_modifier,
        92
    );

    proof_case!(
        astra_e1_numeric_scaled_m093,
        astra_e1_numeric_scaled_modifier,
        93
    );

    proof_case!(
        astra_e1_numeric_scaled_m094,
        astra_e1_numeric_scaled_modifier,
        94
    );

    proof_case!(
        astra_e1_numeric_scaled_m095,
        astra_e1_numeric_scaled_modifier,
        95
    );

    proof_case!(
        astra_e1_numeric_scaled_m096,
        astra_e1_numeric_scaled_modifier,
        96
    );

    proof_case!(
        astra_e1_numeric_scaled_m097,
        astra_e1_numeric_scaled_modifier,
        97
    );

    proof_case!(
        astra_e1_numeric_scaled_m098,
        astra_e1_numeric_scaled_modifier,
        98
    );

    proof_case!(
        astra_e1_numeric_scaled_m099,
        astra_e1_numeric_scaled_modifier,
        99
    );

    #[kani::proof]
    fn astra_e1_boundary_modifiers() {
        let util_k: u8 = kani::any();
        let r1: u8 = kani::any();
        let r2: u8 = kani::any();
        let r3: u8 = kani::any();
        let rb: u8 = kani::any();
        let ir_m: u8 = kani::any();
        let react: u8 = kani::any();
        kani::assume(util_k >= 1 && util_k <= 90);
        kani::assume(r1 <= r2 && r2 <= r3);
        kani::assume(ir_m >= 1 && ir_m <= 100);
        let config = reserve_config(util_k, r1, r2, r3, rb, react);
        let ir_mod = i128(ir_m) * 1_000_000;
        let target = i128(util_k) * 100_000;

        // modifier clamp witnesses (concrete, hand-derived):
        // upper clamp: ir_mod = 10*SCALAR_7, increasing error 255 * 900_000 * 255 / 10^7
        let mut hi = config.clone();
        hi.util = 100_000; // target 0.01
        hi.reactivity = 255;
        assert_eq!(
            finish_accrual(&(), &hi, 1_000_000, 100_000_000, 255, 0).1,
            10 * SCALAR_7
        );
        // lower clamp: ir_mod = SCALAR_7/10, decreasing error 255 * 8_900_000 * 255 / 10^7
        let mut lo = config.clone();
        lo.util = 9_000_000; // target 0.9
        lo.reactivity = 255;
        assert_eq!(
            finish_accrual(&(), &lo, 100_000, 1_000_000, 255, 0).1,
            SCALAR_7 / 10
        );
        // zero reactivity leaves the modifier untouched
        let mut flat = config.clone();
        flat.reactivity = 0;
        assert_eq!(finish_accrual(&(), &flat, target, ir_mod, 200, 0).1, ir_mod);
        kani::cover!(
            util_k == 1 && r1 == 0 && r2 == 0 && r3 == 0 && rb == 0 && ir_m == 1 && react == 0
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use soroban_sdk::testutils::{Ledger, LedgerInfo};

    #[test]
    fn test_calc_accrual_util_under_target() {
        let e = Env::default();

        let reserve_config = ReserveConfig {
            decimals: 7,
            c_factor: 0_7500000,
            l_factor: 0_7500000,
            util: 0_7500000,
            max_util: 0_9500000,
            r_base: 0_0100000,
            r_one: 0_0500000,
            r_two: 0_5000000,
            r_three: 1_5000000,
            reactivity: 0_0000020,
            supply_cap: 1000000000000000000,
            index: 0,
            enabled: true,
        };
        let ir_mod: i128 = 1_0000000;

        e.ledger().set(LedgerInfo {
            timestamp: 500,
            protocol_version: 22,
            sequence_number: 100,
            network_id: Default::default(),
            base_reserve: 10,
            min_temp_entry_ttl: 10,
            min_persistent_entry_ttl: 10,
            max_entry_ttl: 3110400,
        });

        let (accrual, ir_mod) = calc_accrual(&e, &reserve_config, 0_6565656, ir_mod, 0);

        assert_eq!(accrual, 1_000_000_852_536);
        assert_eq!(ir_mod, 0_9999066);
    }

    #[test]
    fn test_calc_accrual_util_over_target() {
        let e = Env::default();

        let reserve_config = ReserveConfig {
            decimals: 7,
            c_factor: 0_7500000,
            l_factor: 0_7500000,
            util: 0_7500000,
            max_util: 0_9500000,
            r_base: 0_0100000,
            r_one: 0_0500000,
            r_two: 0_5000000,
            r_three: 1_5000000,
            reactivity: 0_0000020,
            supply_cap: 1000000000000000000,
            index: 0,
            enabled: true,
        };
        let ir_mod: i128 = 1_0000000;

        e.ledger().set(LedgerInfo {
            timestamp: 500,
            protocol_version: 22,
            sequence_number: 100,
            network_id: Default::default(),
            base_reserve: 10,
            min_temp_entry_ttl: 10,
            min_persistent_entry_ttl: 10,
            max_entry_ttl: 3110400,
        });

        let (accrual, ir_mod) = calc_accrual(&e, &reserve_config, 0_7979797, ir_mod, 0);

        assert_eq!(accrual, 1_000_002_853_078);
        assert_eq!(ir_mod, 1_0000479);
    }

    #[test]
    fn test_calc_accrual_util_over_95() {
        let e = Env::default();

        let reserve_config = ReserveConfig {
            decimals: 7,
            c_factor: 0_7500000,
            l_factor: 0_7500000,
            util: 0_7500000,
            max_util: 0_9500000,
            r_base: 0_0100000,
            r_one: 0_0500000,
            r_two: 0_5000000,
            r_three: 1_5000000,
            reactivity: 0_0000020,
            supply_cap: 1000000000000000000,
            index: 0,
            enabled: true,
        };
        let ir_mod: i128 = 1_0000000;

        e.ledger().set(LedgerInfo {
            timestamp: 500,
            protocol_version: 22,
            sequence_number: 100,
            network_id: Default::default(),
            base_reserve: 10,
            min_temp_entry_ttl: 10,
            min_persistent_entry_ttl: 10,
            max_entry_ttl: 3110400,
        });

        let (accrual, ir_mod) = calc_accrual(&e, &reserve_config, 0_9696969, ir_mod, 0);

        assert_eq!(accrual, 1_000_018_247_510);
        assert_eq!(ir_mod, 1_0002196);
    }

    #[test]
    fn test_calc_ir_mod_over_limit() {
        let e = Env::default();

        let reserve_config = ReserveConfig {
            decimals: 7,
            c_factor: 0_7500000,
            l_factor: 0_7500000,
            util: 0_7500000,
            max_util: 0_9500000,
            r_base: 0_0100000,
            r_one: 0_0500000,
            r_two: 0_5000000,
            r_three: 1_5000000,
            reactivity: 0_0000020,
            supply_cap: 1000000000000000000,
            index: 0,
            enabled: true,
        };
        let ir_mod: i128 = 9_9970000;

        e.ledger().set(LedgerInfo {
            timestamp: 12345,
            protocol_version: 22,
            sequence_number: 10000,
            network_id: Default::default(),
            base_reserve: 10,
            min_temp_entry_ttl: 10,
            min_persistent_entry_ttl: 10,
            max_entry_ttl: 3110400,
        });

        let (_accrual, ir_mod) = calc_accrual(&e, &reserve_config, 0_9696969, ir_mod, 0);

        assert_eq!(ir_mod, 10_0000000);
    }

    #[test]
    fn test_calc_ir_mod_under_limit() {
        let e = Env::default();

        let reserve_config = ReserveConfig {
            decimals: 7,
            c_factor: 0_7500000,
            l_factor: 0_7500000,
            util: 0_7500000,
            max_util: 0_9500000,
            r_base: 0_0100000,
            r_one: 0_0500000,
            r_two: 0_5000000,
            r_three: 1_5000000,
            reactivity: 0_0000020,
            supply_cap: 1000000000000000000,
            index: 0,
            enabled: true,
        };
        let ir_mod: i128 = 0_1500000;

        e.ledger().set(LedgerInfo {
            timestamp: 10000 * 5,
            protocol_version: 22,
            sequence_number: 10000,
            network_id: Default::default(),
            base_reserve: 10,
            min_temp_entry_ttl: 10,
            min_persistent_entry_ttl: 10,
            max_entry_ttl: 3110400,
        });

        let (_accrual, ir_mod) = calc_accrual(&e, &reserve_config, 0_2020202, ir_mod, 0);

        assert_eq!(ir_mod, 0_1000000);
    }

    #[test]
    fn test_calc_ir_mod_reactivity_0() {
        let e = Env::default();

        let reserve_config = ReserveConfig {
            decimals: 7,
            c_factor: 0_7500000,
            l_factor: 0_7500000,
            util: 0_7500000,
            max_util: 0_9500000,
            r_base: 0_0100000,
            r_one: 0_0500000,
            r_two: 0_5000000,
            r_three: 1_5000000,
            reactivity: 0,
            supply_cap: 1000000000000000000,
            index: 0,
            enabled: true,
        };
        let ir_mod: i128 = 1_0000000;

        e.ledger().set(LedgerInfo {
            timestamp: 500,
            protocol_version: 22,
            sequence_number: 100,
            network_id: Default::default(),
            base_reserve: 10,
            min_temp_entry_ttl: 10,
            min_persistent_entry_ttl: 10,
            max_entry_ttl: 3110400,
        });

        let (accrual, ir_mod) = calc_accrual(&e, &reserve_config, 0_6565656, ir_mod, 0);

        assert_eq!(accrual, 1_000_000_852_536);
        assert_eq!(ir_mod, 1_0000000);
    }

    #[test]
    fn test_calc_accrual_rounds_up() {
        let e = Env::default();

        let reserve_config = ReserveConfig {
            decimals: 7,
            c_factor: 0_7500000,
            l_factor: 0_7500000,
            util: 0_7500000,
            max_util: 0_9500000,
            r_base: 0_0001000,
            r_one: 0_0500000,
            r_two: 0_5000000,
            r_three: 1_5000000,
            reactivity: 0_0000020,
            supply_cap: 1000000000000000000,
            index: 0,
            enabled: true,
        };
        let ir_mod: i128 = 0_1000000;

        e.ledger().set(LedgerInfo {
            timestamp: 501,
            protocol_version: 22,
            sequence_number: 100,
            network_id: Default::default(),
            base_reserve: 10,
            min_temp_entry_ttl: 10,
            min_persistent_entry_ttl: 10,
            max_entry_ttl: 3110400,
        });

        let (accrual, ir_mod) = calc_accrual(&e, &reserve_config, 0_0000005, ir_mod, 500);

        assert_eq!(accrual, 1_000_000_000_001);
        assert_eq!(ir_mod, 0_1000000);
    }

    #[test]
    fn test_calc_accrual_fixed_rate() {
        let e = Env::default();

        let reserve_config = ReserveConfig {
            decimals: 7,
            c_factor: 0_7500000,
            l_factor: 0_7500000,
            util: 0_7500000,
            max_util: 0_9500000,
            r_base: 0_2500000,
            r_one: 0,
            r_two: 0,
            r_three: 0,
            reactivity: 0_0000020,
            supply_cap: 1000000000000000000,
            index: 0,
            enabled: true,
        };
        let ir_mod: i128 = 1_0000000;

        e.ledger().set(LedgerInfo {
            timestamp: 500,
            protocol_version: 22,
            sequence_number: 100,
            network_id: Default::default(),
            base_reserve: 10,
            min_temp_entry_ttl: 10,
            min_persistent_entry_ttl: 10,
            max_entry_ttl: 3110400,
        });

        let (accrual_0, ir_mod_0) = calc_accrual(&e, &reserve_config, 0, ir_mod, 0);
        let (accrual_1, ir_mod_1) = calc_accrual(&e, &reserve_config, 0_6565656, ir_mod, 0);
        let (accrual_2, ir_mod_2) = calc_accrual(&e, &reserve_config, 0_7565656, ir_mod, 0);
        let (accrual_3, ir_mod_3) = calc_accrual(&e, &reserve_config, 0_9565656, ir_mod, 0);

        assert_eq!(accrual_0, 1_000_003_963_724);
        assert_eq!(ir_mod_0, 0_9992500);
        assert_eq!(accrual_1, 1_000_003_963_724);
        assert_eq!(ir_mod_1, 0_9999066);
        assert_eq!(accrual_2, 1_000_003_963_724);
        assert_eq!(ir_mod_2, 1_0000065);
        assert_eq!(accrual_3, 1_000_003_963_724);
        assert_eq!(ir_mod_3, 1_0002065);
    }
}
