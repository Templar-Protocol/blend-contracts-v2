//! Supply cap and utilization caps.
//!
//! Two separate guards:
//!
//! - `supply_within_cap` is enforced at the end of `apply_supply` and
//!   `apply_supply_collateral`, the two request handlers that increase a
//!   reserve's supply. These rules drive the *unsummarized* handlers, reached
//!   through the module `apply_summary!` leaves behind, the same way the
//!   auditor's transfer soundness rules reach `handle_transfers`.
//! - `require_utilization_below_max` and `require_utilization_below_100` gate
//!   actions on the reserve's utilization.
//!
//! Utilization is a division, but `ReserveData::utilization` clamps it: zero
//! when there are no liabilities, `SCALAR_7` once liabilities reach supply,
//! and `ceil(liabilities * SCALAR_7 / supply)` only on the branch where
//! liabilities are strictly below supply. The range property below rests on
//! that structure rather than on reasoning about the quotient.

use cvlr::asserts::{cvlr_assert, cvlr_assume, cvlr_satisfy};
use cvlr_soroban::nondet_address;
use cvlr_soroban_derive::rule;
use soroban_sdk::Env;

use crate::constants::SCALAR_7;
use crate::pool::{Pool, Request, Reserve, User};
use crate::storage::{self, PoolConfig};

// ---------------------------------------------------------------- supply cap

/// `apply_supply` cannot leave a reserve's total supply above its cap.
#[rule]
pub fn supply_cap_enforced_on_supply(e: Env) {
    use crate::pool::actions::apply_supply as original;

    let pool_config: PoolConfig = cvlr::nondet();
    storage::set_pool_config(&e, &pool_config);

    let mut pool = Pool::load(&e);
    let mut user: User = cvlr::nondet();
    let mut actions = crate::pool::actions::Actions::new(&e);
    let request: Request = cvlr::nondet();

    original::apply_supply(&e, &mut actions, &mut pool, &mut user, &request);

    let reserve = pool.load_reserve(&e, &request.address, false);
    cvlr_assert!(reserve.total_supply(&e) <= reserve.config.supply_cap);
}

/// `apply_supply_collateral` cannot leave a reserve's total supply above its
/// cap.
#[rule]
pub fn supply_cap_enforced_on_supply_collateral(e: Env) {
    use crate::pool::actions::apply_supply_collateral as original;

    let pool_config: PoolConfig = cvlr::nondet();
    storage::set_pool_config(&e, &pool_config);

    let mut pool = Pool::load(&e);
    let mut user: User = cvlr::nondet();
    let mut actions = crate::pool::actions::Actions::new(&e);
    let request: Request = cvlr::nondet();

    original::apply_supply_collateral(&e, &mut actions, &mut pool, &mut user, &request);

    let reserve = pool.load_reserve(&e, &request.address, false);
    cvlr_assert!(reserve.total_supply(&e) <= reserve.config.supply_cap);
}

// --------------------------------------------------------- utilization range

/// Utilization is always a proportion: between zero and `SCALAR_7` inclusive,
/// for any reserve state.
#[rule]
pub fn utilization_in_range(e: Env) {
    let reserve: Reserve = cvlr::nondet();

    let utilization = reserve.utilization(&e);

    cvlr_assert!(utilization >= 0);
    cvlr_assert!(utilization <= SCALAR_7);
}

// ----------------------------------------------------- utilization below max

/// A reserve over its maximum utilization is always rejected.
#[rule]
pub fn utilization_above_max_panics(e: Env) {
    let reserve: Reserve = cvlr::nondet();
    cvlr_assume!(reserve.utilization(&e) > i128::from(reserve.config.max_util));

    reserve.require_utilization_below_max(&e);

    cvlr_assert!(false);
}

/// Surviving the maximum-utilization guard implies being at or below it.
#[rule]
pub fn utilization_below_max_accepted_implies_within(e: Env) {
    let reserve: Reserve = cvlr::nondet();

    reserve.require_utilization_below_max(&e);

    cvlr_assert!(reserve.utilization(&e) <= i128::from(reserve.config.max_util));
}

// ----------------------------------------------------- utilization below 100

/// A reserve at or above full utilization is always rejected.
#[rule]
pub fn utilization_at_100_panics(e: Env) {
    let reserve: Reserve = cvlr::nondet();
    cvlr_assume!(reserve.utilization(&e) >= SCALAR_7);

    reserve.require_utilization_below_100(&e);

    cvlr_assert!(false);
}

/// Surviving the full-utilization guard implies being strictly below it.
#[rule]
pub fn utilization_below_100_accepted_implies_below(e: Env) {
    let reserve: Reserve = cvlr::nondet();

    reserve.require_utilization_below_100(&e);

    cvlr_assert!(reserve.utilization(&e) < SCALAR_7);
}

// ---------------------------------------------------------- vacuity checks
//
// Every `*_panics` rule asserts `false`, so it passes whenever its
// assumptions are unsatisfiable. Every `*_implies_*` rule passes whenever the
// call under it panics on all paths. These confirm neither is the case.

#[rule]
pub fn sanity_supply_can_succeed(e: Env) {
    use crate::pool::actions::apply_supply as original;

    let pool_config: PoolConfig = cvlr::nondet();
    storage::set_pool_config(&e, &pool_config);

    let mut pool = Pool::load(&e);
    let mut user: User = cvlr::nondet();
    let mut actions = crate::pool::actions::Actions::new(&e);
    let request: Request = cvlr::nondet();

    let minted = original::apply_supply(&e, &mut actions, &mut pool, &mut user, &request);

    cvlr_satisfy!(minted > 0);
}

#[rule]
pub fn sanity_supply_collateral_can_succeed(e: Env) {
    use crate::pool::actions::apply_supply_collateral as original;

    let pool_config: PoolConfig = cvlr::nondet();
    storage::set_pool_config(&e, &pool_config);

    let mut pool = Pool::load(&e);
    let mut user: User = cvlr::nondet();
    let mut actions = crate::pool::actions::Actions::new(&e);
    let request: Request = cvlr::nondet();

    let minted = original::apply_supply_collateral(&e, &mut actions, &mut pool, &mut user, &request);

    cvlr_satisfy!(minted > 0);
}

#[rule]
pub fn sanity_utilization_above_max_reachable(e: Env) {
    let reserve: Reserve = cvlr::nondet();
    cvlr_satisfy!(reserve.utilization(&e) > i128::from(reserve.config.max_util));
}

#[rule]
pub fn sanity_utilization_at_100_reachable(e: Env) {
    let reserve: Reserve = cvlr::nondet();
    cvlr_satisfy!(reserve.utilization(&e) >= SCALAR_7);
}

#[rule]
pub fn sanity_utilization_below_max_reachable(e: Env) {
    let reserve: Reserve = cvlr::nondet();
    reserve.require_utilization_below_max(&e);
    cvlr_satisfy!(true);
}

#[rule]
pub fn sanity_utilization_below_100_reachable(e: Env) {
    let reserve: Reserve = cvlr::nondet();
    reserve.require_utilization_below_100(&e);
    cvlr_satisfy!(true);
}

/// Utilization is not pinned to a single value: the interesting middle of the
/// range is reachable, so the range rule is not trivially about `0` alone.
#[rule]
pub fn sanity_utilization_strictly_between(e: Env) {
    let reserve: Reserve = cvlr::nondet();
    let utilization = reserve.utilization(&e);
    let _ = nondet_address();
    cvlr_satisfy!(utilization > 0 && utilization < SCALAR_7);
}
