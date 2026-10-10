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
//!
//! # Domain
//!
//! `utilization` performs `ceil(d_supply, d_rate, SCALAR_12)`,
//! `floor(b_supply, b_rate, SCALAR_12)` and then
//! `ceil(liabilities, SCALAR_7, supply)`, the last with a symbolic
//! denominator. Over fully unconstrained `i128` fields that is two 128-bit
//! multiplications and a division by an unconstrained 128-bit value, and the
//! prover exhausted its heap on it (job `ed0b99fc`, FAILED with no output).
//!
//! The rules therefore run over `bounded_reserve`, which restricts the
//! reserve to the representable, economically meaningful region:
//! non-negative supplies up to `MAX_SUPPLY`, rates from `SCALAR_12` (the
//! value a reserve is initialized with) up to `MAX_RATE`, and a
//! metadata-valid `max_util`. The largest product is then
//! `MAX_SUPPLY * MAX_RATE = 1e33`, inside `i128`.
//!
//! This is a premise, not a theorem. Outside it the products overflow, and
//! production — built with `overflow-checks = true` — panics rather than
//! returning a wrong utilization. What these rules do not cover is whether
//! the reserve fields can leave this region in the first place.

use cvlr::asserts::{cvlr_assert, cvlr_assume, cvlr_satisfy};
use cvlr_soroban::nondet_address;
use cvlr_soroban_derive::rule;
use soroban_sdk::{Address, Env};

use crate::constants::{SCALAR_12, SCALAR_7};
use crate::pool::{Pool, Request, Reserve, User};
use crate::storage::{self, PoolConfig};

/// Largest supply either side of a reserve may hold: 1e11 tokens at 7
/// decimals.
const MAX_SUPPLY: i128 = 1_000_000_000_000_000_000;

/// Largest b_rate or d_rate: a thousand times the initial `SCALAR_12`.
const MAX_RATE: i128 = 1_000_000_000_000_000;

/// A reserve inside the representable, economically meaningful region. See
/// the module documentation: this is a premise of every rule below.
fn bounded_reserve() -> Reserve {
    let reserve: Reserve = cvlr::nondet();
    cvlr_assume!(reserve.data.b_supply >= 0 && reserve.data.b_supply <= MAX_SUPPLY);
    cvlr_assume!(reserve.data.d_supply >= 0 && reserve.data.d_supply <= MAX_SUPPLY);
    cvlr_assume!(reserve.data.b_rate >= SCALAR_12 && reserve.data.b_rate <= MAX_RATE);
    cvlr_assume!(reserve.data.d_rate >= SCALAR_12 && reserve.data.d_rate <= MAX_RATE);
    // Metadata validation keeps max_util strictly below 100%.
    cvlr_assume!(i128::from(reserve.config.max_util) < SCALAR_7);
    reserve
}

/// Seed storage with a bounded reserve for `asset`, so `Pool::load_reserve`
/// returns rates and supplies inside the domain `bounded_reserve` describes
/// rather than whatever unconstrained values symbolic storage holds.
///
/// Without this the supply-cap rules hand the solver the same unbounded
/// `mul_div` chain that crashed the utilization job: `apply_supply` calls
/// `to_b_token_down` and `total_supply`, both of which multiply a supply by a
/// rate.
fn seed_bounded_reserve(e: &Env, asset: &Address) {
    let mut config: crate::storage::ReserveConfig = cvlr::nondet();
    let mut data: crate::storage::ReserveData = cvlr::nondet();

    cvlr_assume!(data.b_supply >= 0 && data.b_supply <= MAX_SUPPLY);
    cvlr_assume!(data.d_supply >= 0 && data.d_supply <= MAX_SUPPLY);
    cvlr_assume!(data.b_rate >= SCALAR_12 && data.b_rate <= MAX_RATE);
    cvlr_assume!(data.d_rate >= SCALAR_12 && data.d_rate <= MAX_RATE);
    cvlr_assume!(data.backstop_credit >= 0 && data.backstop_credit <= MAX_SUPPLY);
    cvlr_assume!(config.supply_cap >= 0 && config.supply_cap <= MAX_SUPPLY);
    cvlr_assume!(config.decimals <= 18);
    cvlr_assume!(config.enabled);
    config.index = 0;
    data.last_time = 0;

    storage::set_res_config(e, asset, &config);
    storage::set_res_data(e, asset, &data);
}

// ---------------------------------------------------------------- supply cap

/// `apply_supply` cannot leave a reserve's total supply above its cap.
#[rule]
pub fn supply_cap_enforced_on_supply(e: Env) {
    use crate::pool::actions::apply_supply as original;

    let pool_config: PoolConfig = cvlr::nondet();
    storage::set_pool_config(&e, &pool_config);

    let request: Request = cvlr::nondet();
    cvlr_assume!(request.amount > 0 && request.amount <= MAX_SUPPLY);
    seed_bounded_reserve(&e, &request.address);

    let mut pool = Pool::load(&e);
    let mut user: User = cvlr::nondet();
    let mut actions = crate::pool::actions::Actions::new(&e);

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

    let request: Request = cvlr::nondet();
    cvlr_assume!(request.amount > 0 && request.amount <= MAX_SUPPLY);
    seed_bounded_reserve(&e, &request.address);

    let mut pool = Pool::load(&e);
    let mut user: User = cvlr::nondet();
    let mut actions = crate::pool::actions::Actions::new(&e);

    original::apply_supply_collateral(&e, &mut actions, &mut pool, &mut user, &request);

    let reserve = pool.load_reserve(&e, &request.address, false);
    cvlr_assert!(reserve.total_supply(&e) <= reserve.config.supply_cap);
}

// --------------------------------------------------------- utilization range

/// Utilization is always a proportion: between zero and `SCALAR_7` inclusive,
/// for any reserve state.
#[rule]
pub fn utilization_in_range(e: Env) {
    let reserve = bounded_reserve();

    let utilization = reserve.utilization(&e);

    cvlr_assert!(utilization >= 0);
    cvlr_assert!(utilization <= SCALAR_7);
}

// ----------------------------------------------------- utilization below max

/// A reserve over its maximum utilization is always rejected.
#[rule]
pub fn utilization_above_max_panics(e: Env) {
    let reserve = bounded_reserve();
    cvlr_assume!(reserve.utilization(&e) > i128::from(reserve.config.max_util));

    reserve.require_utilization_below_max(&e);

    cvlr_assert!(false);
}

/// Surviving the maximum-utilization guard implies being at or below it.
#[rule]
pub fn utilization_below_max_accepted_implies_within(e: Env) {
    let reserve = bounded_reserve();

    reserve.require_utilization_below_max(&e);

    cvlr_assert!(reserve.utilization(&e) <= i128::from(reserve.config.max_util));
}

// ----------------------------------------------------- utilization below 100

/// A reserve at or above full utilization is always rejected.
#[rule]
pub fn utilization_at_100_panics(e: Env) {
    let reserve = bounded_reserve();
    cvlr_assume!(reserve.utilization(&e) >= SCALAR_7);

    reserve.require_utilization_below_100(&e);

    cvlr_assert!(false);
}

/// Surviving the full-utilization guard implies being strictly below it.
#[rule]
pub fn utilization_below_100_accepted_implies_below(e: Env) {
    let reserve = bounded_reserve();

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

    let request: Request = cvlr::nondet();
    cvlr_assume!(request.amount > 0 && request.amount <= MAX_SUPPLY);
    seed_bounded_reserve(&e, &request.address);

    let mut pool = Pool::load(&e);
    let mut user: User = cvlr::nondet();
    let mut actions = crate::pool::actions::Actions::new(&e);

    let minted = original::apply_supply(&e, &mut actions, &mut pool, &mut user, &request);

    cvlr_satisfy!(minted > 0);
}

#[rule]
pub fn sanity_supply_collateral_can_succeed(e: Env) {
    use crate::pool::actions::apply_supply_collateral as original;

    let pool_config: PoolConfig = cvlr::nondet();
    storage::set_pool_config(&e, &pool_config);

    let request: Request = cvlr::nondet();
    cvlr_assume!(request.amount > 0 && request.amount <= MAX_SUPPLY);
    seed_bounded_reserve(&e, &request.address);

    let mut pool = Pool::load(&e);
    let mut user: User = cvlr::nondet();
    let mut actions = crate::pool::actions::Actions::new(&e);

    let minted = original::apply_supply_collateral(&e, &mut actions, &mut pool, &mut user, &request);

    cvlr_satisfy!(minted > 0);
}

#[rule]
pub fn sanity_utilization_above_max_reachable(e: Env) {
    let reserve = bounded_reserve();
    cvlr_satisfy!(reserve.utilization(&e) > i128::from(reserve.config.max_util));
}

#[rule]
pub fn sanity_utilization_at_100_reachable(e: Env) {
    let reserve = bounded_reserve();
    cvlr_satisfy!(reserve.utilization(&e) >= SCALAR_7);
}

#[rule]
pub fn sanity_utilization_below_max_reachable(e: Env) {
    let reserve = bounded_reserve();
    reserve.require_utilization_below_max(&e);
    cvlr_satisfy!(true);
}

#[rule]
pub fn sanity_utilization_below_100_reachable(e: Env) {
    let reserve = bounded_reserve();
    reserve.require_utilization_below_100(&e);
    cvlr_satisfy!(true);
}

/// Utilization is not pinned to a single value: the interesting middle of the
/// range is reachable, so the range rule is not trivially about `0` alone.
#[rule]
pub fn sanity_utilization_strictly_between(e: Env) {
    let reserve = bounded_reserve();
    let utilization = reserve.utilization(&e);
    let _ = nondet_address();
    cvlr_satisfy!(utilization > 0 && utilization < SCALAR_7);
}
