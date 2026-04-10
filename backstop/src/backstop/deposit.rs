use crate::{contract::require_nonnegative, emissions, storage, BackstopError};
use sep_41_token::TokenClient;
use soroban_sdk::{panic_with_error, Address, Env};

use super::require_is_from_pool_factory;

/// Perform a deposit into the backstop module
pub fn execute_deposit(e: &Env, from: &Address, pool_address: &Address, amount: i128) -> i128 {
    require_nonnegative(e, amount);
    if from == pool_address || from == &e.current_contract_address() {
        panic_with_error!(e, &BackstopError::BadRequest)
    }
    let mut pool_balance = storage::get_pool_balance(e, pool_address);
    require_is_from_pool_factory(e, pool_address, pool_balance.shares);
    let mut user_balance = storage::get_user_balance(e, pool_address, from);

    emissions::update_emissions(e, pool_address, &pool_balance, from, &user_balance);

    let backstop_token_client = TokenClient::new(e, &storage::get_backstop_token(e));
    backstop_token_client.transfer(from, &e.current_contract_address(), &amount);

    let to_mint = pool_balance.convert_to_shares(amount);
    if to_mint <= 0 {
        panic_with_error!(e, &BackstopError::InvalidShareMintAmount);
    }
    pool_balance.deposit(amount, to_mint);
    user_balance.add_shares(to_mint);

    storage::set_pool_balance(e, pool_address, &pool_balance);
    storage::set_user_balance(e, pool_address, from, &user_balance);

    to_mint
}
