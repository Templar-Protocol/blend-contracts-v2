use crate::{
    pool::{self, actions::Actions, User},
    spec::{model, summaries::actions::build_actions_from_request_postcondition},
    FlashLoan,
    Request
};
use soroban_sdk::{Address, Env};

use cvlr_soroban_derive::rule;
use cvlr::asserts::{cvlr_assert, cvlr_satisfy};
use cvlr::nondet::Nondet;
use cvlr_soroban::nondet_address;

/// Return `true` iff `state` has liabilities and
/// either a collateral at `model::skolem_i()` decreased relative to `orig_state`
/// or a liability at `model::skolem_i()` increased relative to `orig_state`
pub fn should_check(orig_state: &User, state: &User) -> bool {
    let idx = model::skolem_i();
    let collateral_decrease_idx =
        state.positions.collateral.get(idx).unwrap_or(0) < orig_state.positions.collateral.get(idx).unwrap_or(0);

    let liabilities_increase_idx =
        state.positions.liabilities.get(idx).unwrap_or(0) > orig_state.positions.liabilities.get(idx).unwrap_or(0);

    state.has_liabilities() && (collateral_decrease_idx || liabilities_increase_idx)
}

/// Check the soundness of the summary for `build_actions_from_request`
#[rule]
pub fn build_actions_from_request(env: &Env, from: Address, requests: soroban_sdk::Vec<Request>) {
    model::init();

    let mut pool = pool::Pool::load(env);
    let orig_state = User::load(env, &from);
    let mut state = orig_state.clone();

    let actions =
        pool::actions::build_actions_from_request::build_actions_from_request(
            env,
            &mut pool,
            &mut state,
            requests,
        );

    // Assume the postcondition we _assume_ in the summary is in fact true
    cvlr_assert!(
        build_actions_from_request_postcondition(actions.check_health, &orig_state, &state)
    );
}

/// The main user health property:
/// if we compare the User state of `user` before & after calling `execute_submit`,
/// then either the user's positions changed in an obviously healthy direction OR
/// we explicitly checked that they are healthy
#[rule]
pub fn user_health_execute_submit(env: &Env, req: soroban_sdk::Vec<Request>) {
    use pool::submit::execute_submit;
    model::init();

    let user = nondet_address();
    let spender = nondet_address();
    let to = nondet_address();
    let allowance: bool = cvlr::nondet();

    let orig_state = User::load(env, &user);
    execute_submit(
        env,
        &user,
        &spender,
        &to,
        req,
        allowance,
    );
    let new_state = User::load(env, &user);

    cvlr_assert!(model::get_checked() || !should_check(&orig_state, &new_state));
}

/// The main user health property:
/// if we compare the User state of `user` before & after calling `execute_submit`,
/// then either the user's positions changed in an obviously healthy direction OR
/// we explicitly checked that they are healthy
#[rule]
pub fn user_health_execute_submit_with_flash_loan(env: &Env, req: soroban_sdk::Vec<Request>) {
    use pool::submit::execute_submit_with_flash_loan;
    model::init();

    let user = nondet_address();
    let flash_loan = FlashLoan::nondet();

    let orig_state = User::load(env, &user);
    execute_submit_with_flash_loan(
        env,
        &user,
        flash_loan,
        req,
    );
    let new_state = User::load(env, &user);

    cvlr_assert!(model::get_checked() || !should_check(&orig_state, &new_state));
}


/// Check that the rule `build_actions_from_request` is not vacuously true
#[rule]
pub fn build_actions_from_request_sanity_1(env: &Env, from: Address, requests: soroban_sdk::Vec<Request>) {
    model::init();

    let mut pool = pool::Pool::load(env);
    let mut state = User::load(env, &from);

    let actions =
        // once this has a summary we need to swap this out for the original fn...
        pool::actions::build_actions_from_request(
            env,
            &mut pool,
            &mut state,
            requests,
        );

    cvlr_satisfy!(actions.check_health);
}


/// Check that the rule `build_actions_from_request` is not vacuously true
#[rule]
pub fn build_actions_from_request_sanity_2(env: &Env, from: Address, requests: soroban_sdk::Vec<Request>) {
    model::init();

    let mut pool = pool::Pool::load(env);
    let mut state = User::load(env, &from);

    let actions =
        // once this has a summary we need to swap this out for the original fn...
        pool::actions::build_actions_from_request(
            env,
            &mut pool,
            &mut state,
            requests,
        );

    cvlr_satisfy!(!actions.check_health);
}

#[rule]
pub fn handle_transfer_with_allowance_summary_ok(e: &Env) {
    use pool::submit::handle_transfer_with_allowance as original;
    model::init();

    let a = Actions::nondet();
    let spender = nondet_address();
    let to = nondet_address();
    let user = nondet_address();
    let key = crate::PoolDataKey::Positions(user.clone());

    let pre: pool::Positions = e.storage().persistent().get(&key).unwrap();
    original::handle_transfer_with_allowance(e, &a, &spender, &to);
    let post: pool::Positions = e.storage().persistent().get(&key).unwrap();

    cvlr_assert!(
        pre.collateral.get(model::skolem_i())
        == post.collateral.get(model::skolem_i())
    );

    cvlr_assert!(
        pre.supply.get(model::skolem_i())
        == post.supply.get(model::skolem_i())
    );

    cvlr_assert!(
        pre.liabilities.get(model::skolem_i())
        == post.liabilities.get(model::skolem_i())
    );
}

#[rule]
pub fn handle_transfers_summary_ok(e: &Env) {
    use pool::submit::handle_transfers as original;
    model::init();

    let a = Actions::nondet();
    let spender = nondet_address();
    let to = nondet_address();
    let user = nondet_address();
    let key = crate::PoolDataKey::Positions(user.clone());


    let pre: pool::Positions = e.storage().persistent().get(&key).unwrap();
    original::handle_transfers(e, &a, &spender, &to);
    let post: pool::Positions = e.storage().persistent().get(&key).unwrap();

    cvlr_assert!(
        pre.collateral.get(model::skolem_i())
        == post.collateral.get(model::skolem_i())
    );

    cvlr_assert!(
        pre.supply.get(model::skolem_i())
        == post.supply.get(model::skolem_i())
    );

    cvlr_assert!(
        pre.liabilities.get(model::skolem_i())
        == post.liabilities.get(model::skolem_i())
    );

}

/// The main user health property:
/// if we compare the User state of `user` before & after calling `execute_submit`,
/// then either the user's positions changed in an obviously healthy direction OR
/// we explicitly checked that they are healthy
#[rule]
pub fn user_health_sanity(env: &Env, req: soroban_sdk::Vec<Request>) {
    use pool::submit::execute_submit;
    model::init();

    let user = nondet_address();
    let spender = nondet_address();
    let to = nondet_address();
    let allowance: bool = cvlr::nondet();

    execute_submit(
        env,
        &user,
        &spender,
        &to,
        req,
        allowance,
    );

    cvlr_satisfy!(true);
}

/// The main user health property:
/// if we compare the User state of `user` before & after calling `execute_submit`,
/// then either the user's positions changed in an obviously healthy direction OR
/// we explicitly checked that they are healthy
#[rule]
pub fn user_health_flash_loan_sanity(env: &Env, req: soroban_sdk::Vec<Request>) {
    use pool::submit::execute_submit_with_flash_loan;
    model::init();

    let user = nondet_address();
    let flash_loan = FlashLoan::nondet();

    execute_submit_with_flash_loan(
        env,
        &user,
        flash_loan,
        req,
    );

    cvlr_satisfy!(true);
}