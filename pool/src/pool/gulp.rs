use sep_41_token::TokenClient;
use soroban_sdk::{Address, Env};

use super::{Pool, RequestType, Reserve};

/// Gulps the excess tokens in the pool, determined by the difference between the pool token balance
/// and the reserve total supply, backstop credit, and liabiltiies.
///
/// ### Arguments
/// * `asset` - The address of the asset to gulp
///
/// ### Returns
/// * The gulped token delta accrued to the backstop credit
///
/// ### Panics
/// * If borrowing is not enabled on the pool. This ensures that the backstop can safely process
/// interest auctions.
pub fn execute_gulp(e: &Env, asset: &Address) -> i128 {
    let pool = Pool::load(e);

    // ensure the backstop can safely accept new interest
    pool.require_action_allowed(e, RequestType::Borrow as u32);

    let mut reserve = Reserve::load(e, &pool.config, asset);
    let pool_token_balance = TokenClient::new(e, asset).balance(&e.current_contract_address());
    let reserve_token_balance =
        reserve.total_supply(e) + reserve.data.backstop_credit - reserve.total_liabilities(e);
    let token_balance_delta = pool_token_balance - reserve_token_balance;
    if token_balance_delta <= 0 {
        return 0;
    }

    reserve.data.backstop_credit += token_balance_delta;
    reserve.store(e);

    return token_balance_delta;
}
