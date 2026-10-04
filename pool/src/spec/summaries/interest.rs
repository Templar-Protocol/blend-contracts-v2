use cast::i128;
use soroban_sdk::Env;

use crate::storage::ReserveConfig;

/** A dummy version of calc_accrual from pool/src/pool/interest.rs made
 * by separating the cur_ir computation out to a separate function
 * which is then called within the calc_accural_summary function below.
 * Note that this is NOT sound but here we don't care about the actual value of cur_ir,
 * we just want to know if the second branch will ever be taken.
*/
pub fn calc_ir(
    _e: &Env,
    config: &ReserveConfig,
    cur_util: i128,
    _ir_mod: i128,
    _last_time: u64,
) -> i128 {
    let cur_ir: i128;
    let target_util: i128 = i128(config.util);
    if cur_util <= target_util {
        cur_ir = 1;
    } else if cur_util <= 0_9500000 {
        cur_ir = 2;
    } else {
        cur_ir = 3;
    }
    return cur_ir
}