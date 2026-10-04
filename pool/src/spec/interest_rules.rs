use soroban_sdk::Env;

use cvlr_soroban_derive::rule;
use cvlr::asserts::{cvlr_assert, cvlr_assume};

use crate::{spec::summaries::interest::calc_ir, ReserveConfig};

/**
 * This shows a counter example demonstrating that
 * `calc_accrual` in pool/src/pool/interest.rs should
 * make sure that before the computation of `cur_ir`, there
 * is a check to make sure that target_util < 0.95.
 * This can be enforced when the ReserveConfig is created perhaps.
 * If cur_util is <= 0.95,
 * then the second branch should be taken, not the first one but right now
 * the control does not reach the second branch because target_util is allowed to be >= 0.95.
*/ 
#[rule]
pub fn target_util_should_be_less_than_0_9500000(
    e: &Env,
    cur_util: i128,
    ir_mod: i128,
    last_time: u64,
) {
    let config: ReserveConfig = cvlr::nondet();
    cvlr_assume!(cur_util <= 0_9500000 && config.util > 0_9500000);
    let cur_ir = calc_ir(e, &config, cur_util, ir_mod, last_time);
    cvlr_assert!(cur_ir == 2);
}