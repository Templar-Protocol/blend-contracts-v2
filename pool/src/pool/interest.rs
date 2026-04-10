use cast::i128;
use soroban_fixed_point_math::SorobanFixedPoint;
use soroban_sdk::{panic_with_error, Env};

use crate::{
    constants::{SCALAR_12, SCALAR_7, SECONDS_PER_YEAR},
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
    let cur_ir: i128;
    let target_util: i128 = i128(config.util);
    if cur_util <= target_util {
        let util_scalar = cur_util.fixed_div_ceil(e, &target_util, &SCALAR_7);
        let base_rate =
            util_scalar.fixed_mul_ceil(e, &i128(config.r_one), &SCALAR_7) + i128(config.r_base);

        cur_ir = base_rate.fixed_mul_ceil(e, &ir_mod, &SCALAR_7);
    } else if cur_util <= 0_9500000 {
        let util_scalar =
            (cur_util - target_util).fixed_div_ceil(e, &(0_9500000 - target_util), &SCALAR_7);
        let base_rate = util_scalar.fixed_mul_ceil(e, &i128(config.r_two), &SCALAR_7)
            + i128(config.r_one)
            + i128(config.r_base);

        cur_ir = base_rate.fixed_mul_ceil(e, &ir_mod, &SCALAR_7);
    } else {
        let util_scalar = (cur_util - 0_9500000).fixed_div_ceil(e, &0_0500000, &SCALAR_7);
        let extra_rate = util_scalar.fixed_mul_ceil(e, &i128(config.r_three), &SCALAR_7);

        let intersection = ir_mod.fixed_mul_ceil(
            e,
            &i128(config.r_two + config.r_one + config.r_base),
            &SCALAR_7,
        );
        cur_ir = extra_rate + intersection;
    }

    // update rate_modifier
    let delta_time = i128(e.ledger().timestamp() - last_time);
    // this should never occur, but require some time to pass
    if delta_time < 1 {
        panic_with_error!(e, PoolError::InternalError);
    }
    // util dif 7 decimals
    let util_dif = cur_util - target_util;
    let new_ir_mod: i128;
    if util_dif >= 0 {
        // rate modifier increasing
        let util_error = delta_time * util_dif;
        let rate_dif = util_error.fixed_mul_floor(e, &i128(config.reactivity), &SCALAR_7);
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
        let rate_dif = util_error.fixed_mul_ceil(e, &i128(config.reactivity), &SCALAR_7);
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
        SCALAR_12 + time_weight.fixed_mul_ceil(e, &cur_ir, &SCALAR_7),
        new_ir_mod,
    )
}
