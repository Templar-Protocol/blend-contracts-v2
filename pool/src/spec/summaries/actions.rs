#![allow(unused)]
use soroban_sdk::{Env, Address, Vec};
use crate::spec::model;
use crate::{Request, Positions, AuctionType, AuctionData};
use crate::pool::{
    User,
    Pool,
    actions::Actions,
};

/// N.B. these summaries do not model storage outside of positions,
/// so they are unsound when considering any properties that depend on
/// other storage
///
macro_rules! arb_positions {
    ($positions:expr, $map:ident, $amount:ident, $orig:ident, $new:expr) => {
        let reserve: u32 = cvlr::nondet();
        let $amount: i128  = cvlr::nondet();
        cvlr::cvlr_assume!($amount > 0);
        let b = &mut $positions;
        let $orig = b.$map.get(reserve).unwrap_or(0);
        b.$map.set(reserve, $new);
    };

    ($positions:expr, $map:ident, $amount:ident, $new:expr) => {
        let reserve: u32 = cvlr::nondet();
        let $amount: i128  = cvlr::nondet();
        cvlr::cvlr_assume!($amount > 0);
        let b = &mut $positions;
        b.$map.set(reserve, $new);
    };
}

pub(crate) fn apply_supply(
    e: &Env,
    _actions: &mut Actions,
    pool: &mut Pool,
    user: &mut User,
    _request: &Request,
) -> i128 {
    arb_positions!(user.positions, supply, amount, amount);
    pool.reserves = cvlr_soroban::nondet_map();
    cvlr::nondet()
}

pub(crate) fn apply_withdraw(
    e: &Env,
    _actions: &mut Actions,
    pool: &mut Pool,
    user: &mut User,
    _request: &Request,
) -> (i128, i128) {
    arb_positions!(user.positions, supply, amount, amount);
    pool.reserves = cvlr_soroban::nondet_map();
    (cvlr::nondet(), cvlr::nondet())
}

pub(crate) fn apply_supply_collateral(
    e: &Env,
    _actions: &mut Actions,
    pool: &mut Pool,
    user: &mut User,
    _request: &Request,
) -> i128 {
    arb_positions!(user.positions, collateral, amount, orig, orig + amount);
    cvlr::cvlr_assume!(amount > 0);
    pool.reserves = cvlr_soroban::nondet_map();
    cvlr::nondet()
}

pub(crate) fn apply_withdraw_collateral(
    e: &Env,
    actions: &mut Actions,
    pool: &mut Pool,
    user: &mut User,
    _request: &Request,
) -> (i128, i128) {
    arb_positions!(user.positions, collateral, amount, orig, orig - amount);
    cvlr::cvlr_assume!(amount > 0);
    cvlr::cvlr_assume!(amount <= orig);
    actions.do_check_health();
    pool.reserves = cvlr_soroban::nondet_map();
    (cvlr::nondet(), cvlr::nondet())
}

pub(crate) fn apply_borrow(
    _e: &Env,
    actions: &mut Actions,
    pool: &mut Pool,
    user: &mut User,
    _request: &Request,
) -> i128 {
    arb_positions!(user.positions, liabilities, amount, orig, orig + amount);
    cvlr::cvlr_assume!(amount > 0);
    actions.do_check_health();
    pool.reserves = cvlr_soroban::nondet_map();
    cvlr::nondet()
}

pub(crate) fn apply_repay(
    _e: &Env,
    _actions: &mut Actions,
    pool: &mut Pool,
    user: &mut User,
    _request: &Request,
) -> (i128, i128) {
    arb_positions!(user.positions, liabilities, amount, orig, orig - amount);
    cvlr::cvlr_assume!(amount > 0);
    cvlr::cvlr_assume!(amount <= orig);
    pool.reserves = cvlr_soroban::nondet_map();
    (cvlr::nondet(), cvlr::nondet())
}

pub fn build_actions_from_request_postcondition(
    check_health: bool,
    pre_user: &User,
    post_user: &User,
) -> bool {
    check_health || !crate::spec::health_rules::should_check(&pre_user, &post_user)
}

/// This spec is proved sound (modulo notes above) in spec::build_actions_from_request
pub fn build_actions_from_request(
    e: &Env,
    _pool: &mut Pool,
    from_state: &mut User,
    _requests: Vec<Request>,
) -> Actions {
    let actions: Actions = cvlr::nondet::nondet();
    let new_state: User = cvlr::nondet::nondet();

    // Auctions write positions
    if cvlr::nondet() {
        new_state.store(e);
    }

    // Assume the postcondition
    cvlr::cvlr_assume!(
        build_actions_from_request_postcondition(actions.check_health, &from_state, &new_state)
    );

    actions
}
