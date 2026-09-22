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
#[path = "../proofs/interest.rs"]
mod verification;

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
