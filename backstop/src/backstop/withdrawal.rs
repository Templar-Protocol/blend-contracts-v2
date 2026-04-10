use crate::{
    contract::require_nonnegative, dependencies::PoolClient, emissions, storage, BackstopError,
};
use sep_41_token::TokenClient;
use soroban_sdk::{panic_with_error, unwrap::UnwrapOptimized, Address, Env};

use super::Q4W;

/// Perform a queue for withdraw from the backstop module
pub fn execute_queue_withdrawal(
    e: &Env,
    from: &Address,
    pool_address: &Address,
    amount: i128,
) -> Q4W {
    require_nonnegative(e, amount);

    let mut pool_balance = storage::get_pool_balance(e, pool_address);
    let mut user_balance = storage::get_user_balance(e, pool_address, from);

    // update emissions
    emissions::update_emissions(e, pool_address, &pool_balance, from, &user_balance);

    user_balance.queue_shares_for_withdrawal(e, amount);
    pool_balance.queue_for_withdraw(amount);

    storage::set_user_balance(e, pool_address, from, &user_balance);
    storage::set_pool_balance(e, pool_address, &pool_balance);

    user_balance.q4w.last().unwrap_optimized()
}

/// Perform a dequeue of queued for withdraw deposits from the backstop module
pub fn execute_dequeue_withdrawal(e: &Env, from: &Address, pool_address: &Address, amount: i128) {
    require_nonnegative(e, amount);

    let mut pool_balance = storage::get_pool_balance(e, pool_address);
    let mut user_balance = storage::get_user_balance(e, pool_address, from);

    // update emissions
    emissions::update_emissions(e, pool_address, &pool_balance, from, &user_balance);

    user_balance.dequeue_shares(e, amount);
    user_balance.add_shares(amount);
    pool_balance.dequeue_q4w(e, amount);

    storage::set_user_balance(e, pool_address, from, &user_balance);
    storage::set_pool_balance(e, pool_address, &pool_balance);
}

/// Perform a withdraw from the backstop module
pub fn execute_withdraw(e: &Env, from: &Address, pool_address: &Address, amount: i128) -> i128 {
    require_nonnegative(e, amount);

    let pool_client = PoolClient::new(e, pool_address);
    let backstop_positions = pool_client.get_positions(&e.current_contract_address());
    if backstop_positions.liabilities.len() > 0 {
        panic_with_error!(e, &BackstopError::BadDebtExists);
    }

    let mut pool_balance = storage::get_pool_balance(e, pool_address);
    let mut user_balance = storage::get_user_balance(e, pool_address, from);

    user_balance.withdraw_shares(e, amount);

    let to_return = pool_balance.convert_to_tokens(amount);
    if to_return == 0 {
        panic_with_error!(e, &BackstopError::InvalidTokenWithdrawAmount);
    }
    pool_balance.withdraw(e, to_return, amount);

    storage::set_user_balance(e, pool_address, from, &user_balance);
    storage::set_pool_balance(e, pool_address, &pool_balance);

    let backstop_token_client = TokenClient::new(e, &storage::get_backstop_token(e));
    backstop_token_client.transfer(&e.current_contract_address(), from, &to_return);

    to_return
}
