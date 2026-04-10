use crate::{contract::require_nonnegative, storage, BackstopError};
use sep_41_token::TokenClient;
use soroban_sdk::{panic_with_error, Address, Env};

use super::require_is_from_pool_factory;

/// Perform a draw from a pool's backstop
///
/// `pool_address` MUST be authenticated before calling
pub fn execute_draw(e: &Env, pool_address: &Address, amount: i128, to: &Address) {
    require_nonnegative(e, amount);

    let mut pool_balance = storage::get_pool_balance(e, pool_address);

    pool_balance.withdraw(e, amount, 0);
    storage::set_pool_balance(e, pool_address, &pool_balance);

    let backstop_token = TokenClient::new(e, &storage::get_backstop_token(e));
    backstop_token.transfer(&e.current_contract_address(), to, &amount);
}

/// Perform a donation to a pool's backstop
pub fn execute_donate(e: &Env, from: &Address, pool_address: &Address, amount: i128) {
    require_nonnegative(e, amount);
    if from == pool_address || from == &e.current_contract_address() {
        panic_with_error!(e, &BackstopError::BadRequest)
    }

    let mut pool_balance = storage::get_pool_balance(e, pool_address);
    require_is_from_pool_factory(e, pool_address, pool_balance.shares);

    let backstop_token = TokenClient::new(e, &storage::get_backstop_token(e));
    backstop_token.transfer_from(
        &e.current_contract_address(),
        from,
        &e.current_contract_address(),
        &amount,
    );

    pool_balance.deposit(amount, 0);
    storage::set_pool_balance(e, pool_address, &pool_balance);
}
