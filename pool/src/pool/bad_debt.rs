use soroban_sdk::{panic_with_error, Address, Env};

use crate::{dependencies::BackstopClient, events::PoolEvents, storage, AuctionType, PoolError};

use super::{calc_pool_backstop_threshold, Pool, User};

/// Handles any bad debt that exists for "user"
pub fn bad_debt(e: &Env, user: &Address) {
    let mut pool = Pool::load(e);
    let mut user_state = User::load(e, user);

    let backstop = storage::get_backstop(e);

    let had_bad_debt = if user == &backstop {
        if storage::has_auction(e, &(AuctionType::BadDebtAuction as u32), &backstop) {
            panic_with_error!(e, PoolError::AuctionInProgress);
        }
        check_and_handle_backstop_bad_debt(e, &mut pool, user, &mut user_state)
    } else {
        if storage::has_auction(e, &(AuctionType::UserLiquidation as u32), &user) {
            panic_with_error!(e, PoolError::AuctionInProgress);
        }
        check_and_handle_user_bad_debt(e, &mut pool, user, &mut user_state)
    };

    if had_bad_debt {
        user_state.store(e);
        pool.store_cached_reserves(e);
    } else {
        panic_with_error!(e, PoolError::BadRequest);
    }
}

/// Check if a user has bad debt.
///
/// If they do, pass the bad debt off to the backstop.
///
/// If not, this function does nothing.
///
/// `user_state` is modified in place, and is not stored to chain. If this function
/// is invoked, `user_state` must be written to chain afterwards.
///
/// `pool` is modified in place, and reserve updates are not stored to chain. If this function
/// is invoked, `pool.store_cached_reserves()` must be called afterwards.
///
/// ### Arguments
/// * pool - The pool
/// * user - The user's address
/// * user_state - The user's state
///
/// ### Returns
/// * `true` if the user's bad debt was handled, `false` otherwise
pub fn check_and_handle_user_bad_debt(
    e: &Env,
    pool: &mut Pool,
    user: &Address,
    user_state: &mut User,
) -> bool {
    if user_state.has_liabilities() && !user_state.has_collateral() {
        // no more collateral left to liquidate for this user
        // pass the rest of the debt to the backstop as bad debt
        let reserve_list = storage::get_res_list(e);
        let backstop_address = storage::get_backstop(e);
        let mut backstop_state = User::load(e, &backstop_address);
        for (reserve_index, liability_balance) in user_state.positions.liabilities.iter() {
            let asset = reserve_list.get_unchecked(reserve_index);
            let mut reserve = pool.load_reserve(e, &asset, true);
            backstop_state.add_liabilities(e, &mut reserve, liability_balance);
            user_state.remove_liabilities(e, &mut reserve, liability_balance);
            pool.cache_reserve(reserve);

            PoolEvents::bad_debt(e, user.clone(), asset, liability_balance);
        }
        backstop_state.store(e);
        return true;
    }
    return false;
}

/// Check if the backstop's bad debt needs to be defaulted. This occurs when the backstop has less than
/// 5% of the backstop threshold in tokens, as this implies there likely isn't enough backstop tokens
/// to reasonalby auction off bad debt.
///
/// If the backstop has less than 5% of the threshold, default the bad debt.
///
/// If not, this function does nothing.
///
/// `backstop_state` is modified in place, and is not stored to chain. If this function
/// is invoked, `backstop_state` must be written to chain afterwards.
///
/// `pool` is modified in place, and reserve updates are not stored to chain. If this function
/// is invoked, `pool.store_cached_reserves()` must be called afterwards.
///
/// ### Arguments
/// * pool - The pool
/// * backstop_state - The backstop's state
///
/// ### Returns
/// * `true` if the backstop's bad debt was defaulted, `false` otherwise
pub fn check_and_handle_backstop_bad_debt(
    e: &Env,
    pool: &mut Pool,
    backstop_address: &Address,
    backstop_state: &mut User,
) -> bool {
    if backstop_state.has_liabilities() {
        let backstop_client = BackstopClient::new(e, backstop_address);
        let pool_backstop_data = backstop_client.pool_data(&e.current_contract_address());
        let threshold = calc_pool_backstop_threshold(&pool_backstop_data);
        if threshold < 0_0000003 {
            // ~5% of threshold
            let reserve_list = storage::get_res_list(e);
            for (reserve_index, liability_balance) in backstop_state.positions.liabilities.iter() {
                let res_asset_address = reserve_list.get_unchecked(reserve_index);
                let mut reserve = pool.load_reserve(e, &res_asset_address, true);
                backstop_state.default_liabilities(e, &mut reserve, liability_balance);
                pool.cache_reserve(reserve);

                PoolEvents::defaulted_debt(e, res_asset_address, liability_balance);
            }
            return true;
        }
    }
    return false;
}
