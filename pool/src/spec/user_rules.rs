use soroban_sdk::Env;

use cvlr_soroban_derive::rule;
use cvlr::asserts::{cvlr_assert, cvlr_assume};

use crate::pool::{Reserve, User};

// adding liabilities increases liabilities
#[rule]
pub fn add_liabilities_increases_liabilities(env: &Env) {
    let mut user: User = cvlr::nondet();
    let mut reserve: Reserve = cvlr::nondet();
    let amount: i128 = cvlr::nondet();
    cvlr_assume!(amount > 0);

    let pool_liabilities_before = user.get_liabilities(reserve.config.index);

    user.add_liabilities(env, &mut reserve, amount);

    let pool_liabilities_after = user.get_liabilities(reserve.config.index);

    cvlr_assert!(pool_liabilities_after >= pool_liabilities_before + amount);
}

// adding liabilities increases dsupply on reserve, unchanged bsupply
#[rule]
pub fn add_liabilities_increases_dsupply(env: &Env) {
    let mut user: User = cvlr::nondet();
    let mut reserve: Reserve = cvlr::nondet();
    let amount: i128 = cvlr::nondet();

    cvlr_assume!(amount > 0);

    let reserve_d_supply_before = reserve.data.d_supply;
    let reserve_b_supply_before = reserve.data.b_supply;

    user.add_liabilities(env, &mut reserve, amount);

    let reserve_d_supply_after = reserve.data.d_supply;
    let reserve_b_supply_after = reserve.data.b_supply;

    cvlr_assert!(reserve_d_supply_after == reserve_d_supply_before + amount);
    cvlr_assert!(reserve_b_supply_after == reserve_b_supply_before);
}

// remove liabilities decreases liabilities
#[rule]
pub fn remove_liabilities_decreases_liabilities(env: &Env) {
    let mut user: User = cvlr::nondet();
    let mut reserve: Reserve = cvlr::nondet();
    let amount: i128 = cvlr::nondet();

    cvlr_assume!(amount > 0);

    let pool_liabilities_before = user.get_liabilities(reserve.config.index);

    user.remove_liabilities(env, &mut reserve, amount);

    let pool_liabilities_after = user.get_liabilities(reserve.config.index);

    // removed the entire balance of liabilities for the token
    if pool_liabilities_after == 0 {
        cvlr_assert!(pool_liabilities_before == amount);
    } else {
        cvlr_assert!(pool_liabilities_after == pool_liabilities_before - amount);
    }
}

// removing liabilities decreases dsupply on reserve, unchanged bsupply
#[rule]
pub fn remove_liabilities_decreases_dsupply(env: &Env) {
    let mut user: User = cvlr::nondet();
    let mut reserve: Reserve = cvlr::nondet();
    let amount: i128 = cvlr::nondet();

    cvlr_assume!(amount > 0);

    let reserve_d_supply_before = reserve.data.d_supply;
    let reserve_b_supply_before = reserve.data.b_supply;

    user.remove_liabilities(env, &mut reserve, amount);

    let reserve_d_supply_after = reserve.data.d_supply;
    let reserve_b_supply_after = reserve.data.b_supply;

    cvlr_assert!(reserve_d_supply_after == reserve_d_supply_before - amount);
    cvlr_assert!(reserve_b_supply_after == reserve_b_supply_before);
}

// adding collateral increases collateral
#[rule]
pub fn add_collateral_increases_position_collateral(env: &Env) {
    let mut user: User = cvlr::nondet();
    let mut reserve: Reserve = cvlr::nondet();
    let amount: i128 = cvlr::nondet();

    cvlr_assume!(amount > 0);

    let pool_collateral_before = user.get_collateral(reserve.config.index);

    user.add_collateral(env, &mut reserve, amount);

    let pool_collateral_after = user.get_collateral(reserve.config.index);

    cvlr_assert!(pool_collateral_after == pool_collateral_before + amount);
}

// adding collateral increases bsupply, unchanged d_supply
#[rule]
pub fn add_collateral_increases_b_supply(env: &Env) {
    let mut user: User = cvlr::nondet();
    let mut reserve: Reserve = cvlr::nondet();
    let amount: i128 = cvlr::nondet();

    cvlr_assume!(amount > 0);

    let reserve_b_supply_before = reserve.data.b_supply;
    let reserve_d_supply_before = reserve.data.d_supply;

    user.add_collateral(env, &mut reserve, amount);

    let reserve_b_supply_after = reserve.data.b_supply;
    let reserve_d_supply_after = reserve.data.d_supply;

    // b balance should increase
    cvlr_assert!(reserve_b_supply_after == reserve_b_supply_before + amount);
    // d balance should not change
    cvlr_assert!(reserve_d_supply_after == reserve_d_supply_before);
}

// removing collateral decreases collateral
#[rule]
pub fn remove_collateral_decreases_position_collateral(env: &Env) {
    let mut user: User = cvlr::nondet();
    let mut reserve: Reserve = cvlr::nondet();
    let amount: i128 = cvlr::nondet();

    cvlr_assume!(amount > 0);

    let pool_collateral_before = user.get_collateral(reserve.config.index);

    user.remove_collateral(env, &mut reserve, amount);

    let pool_collateral_after = user.get_collateral(reserve.config.index);

    cvlr_assert!(pool_collateral_after == pool_collateral_before - amount);
}

// removing collateral decreases bsupply, unchanged d_supply
#[rule]
pub fn remove_collateral_decreases_b_supply(env: &Env) {
    let mut user: User = cvlr::nondet();
    let mut reserve: Reserve = cvlr::nondet();
    let amount: i128 = cvlr::nondet();

    cvlr_assume!(amount > 0);

    let reserve_b_supply_before = reserve.data.b_supply;
    let reserve_d_supply_before = reserve.data.d_supply;

    user.remove_collateral(env, &mut reserve, amount);

    let reserve_b_supply_after = reserve.data.b_supply;
    let reserve_d_supply_after = reserve.data.d_supply;

    // b balance should increase
    cvlr_assert!(reserve_b_supply_after == reserve_b_supply_before - amount);
    // d balance should not change
    cvlr_assert!(reserve_d_supply_after == reserve_d_supply_before);
}

// adding supply increases positional supply
#[rule]
pub fn add_supply_increases_position_supply(env: &Env) {
    let mut user: User = cvlr::nondet();
    let mut reserve: Reserve = cvlr::nondet();
    let amount: i128 = cvlr::nondet();

    cvlr_assume!(amount > 0);

    let supply_before = user.get_supply(reserve.config.index);

    user.add_supply(env, &mut reserve, amount);

    let supply_after = user.get_supply(reserve.config.index);

    cvlr_assert!(supply_after == supply_before + amount);
}

// adding supply increases bsupply, unchanged d_supply
#[rule]
pub fn add_supply_increases_b_supply(env: &Env) {
    let mut user: User = cvlr::nondet();
    let mut reserve: Reserve = cvlr::nondet();
    let amount: i128 = cvlr::nondet();

    cvlr_assume!(amount > 0);

    let reserve_b_supply_before = reserve.data.b_supply;
    let reserve_d_supply_before = reserve.data.d_supply;

    user.add_supply(env, &mut reserve, amount);

    let reserve_b_supply_after = reserve.data.b_supply;
    let reserve_d_supply_after = reserve.data.d_supply;

    // b balance should increase
    cvlr_assert!(reserve_b_supply_after == reserve_b_supply_before + amount);
    // d balance should not change
    cvlr_assert!(reserve_d_supply_after == reserve_d_supply_before);
}

// removing supply decreases positional supply
#[rule]
pub fn remove_supply_decreases_position_collateral(env: &Env) {
    let mut user: User = cvlr::nondet();
    let mut reserve: Reserve = cvlr::nondet();
    let amount: i128 = cvlr::nondet();

    cvlr_assume!(amount > 0);

    let supply_before = user.get_supply(reserve.config.index);

    user.remove_supply(env, &mut reserve, amount);

    let supply_after = user.get_supply(reserve.config.index);

    cvlr_assert!(supply_after == supply_before - amount);
}

// removing collateral decreases bsupply, unchanged d_supply
#[rule]
pub fn remove_supply_decreases_b_supply(env: &Env) {
    let mut user: User = cvlr::nondet();
    let mut reserve: Reserve = cvlr::nondet();
    let amount: i128 = cvlr::nondet();

    cvlr_assume!(amount > 0);

    let reserve_b_supply_before = reserve.data.b_supply;
    let reserve_d_supply_before = reserve.data.d_supply;

    user.remove_collateral(env, &mut reserve, amount);

    let reserve_b_supply_after = reserve.data.b_supply;
    let reserve_d_supply_after = reserve.data.d_supply;

    // b balance should increase
    cvlr_assert!(reserve_b_supply_after == reserve_b_supply_before - amount);
    // d balance should not change
    cvlr_assert!(reserve_d_supply_after == reserve_d_supply_before);
}