use soroban_sdk::Map;
use soroban_sdk::{contracttype, panic_with_error, Address, Env, Vec};

use crate::events::PoolEvents;
use crate::AuctionType;
use crate::{auctions, errors::PoolError, validator::require_nonnegative};

use super::pool::Pool;
use super::User;

/// A request a user makes against the pool
#[derive(Clone)]
#[contracttype]
pub struct Request {
    pub request_type: u32,
    pub address: Address, // asset address or liquidatee
    pub amount: i128,
}

/// The type of request to be made against the pool
#[derive(Clone, PartialEq)]
#[repr(u32)]
pub enum RequestType {
    Supply = 0,
    Withdraw = 1,
    SupplyCollateral = 2,
    WithdrawCollateral = 3,
    Borrow = 4,
    Repay = 5,
    FillUserLiquidationAuction = 6,
    FillBadDebtAuction = 7,
    FillInterestAuction = 8,
    DeleteLiquidationAuction = 9,
}

impl RequestType {
    /// Convert a u32 to a RequestType
    ///
    /// ### Panics
    /// If the value is not a valid RequestType
    pub fn from_u32(e: &Env, value: u32) -> Self {
        match value {
            0 => RequestType::Supply,
            1 => RequestType::Withdraw,
            2 => RequestType::SupplyCollateral,
            3 => RequestType::WithdrawCollateral,
            4 => RequestType::Borrow,
            5 => RequestType::Repay,
            6 => RequestType::FillUserLiquidationAuction,
            7 => RequestType::FillBadDebtAuction,
            8 => RequestType::FillInterestAuction,
            9 => RequestType::DeleteLiquidationAuction,
            _ => panic_with_error!(e, PoolError::BadRequest),
        }
    }
}

#[contracttype]
pub struct FlashLoan {
    pub contract: Address,
    pub asset: Address,
    pub amount: i128,
}

/// Transfer actions to be taken by the sender and pool
pub struct Actions {
    pub spender_transfer: Map<Address, i128>,
    pub pool_transfer: Map<Address, i128>,
    pub check_health: bool,
    pub check_max_util: Vec<Address>,
}

impl Actions {
    /// Create an empty set of actions
    pub fn new(e: &Env) -> Self {
        Actions {
            spender_transfer: Map::new(e),
            pool_transfer: Map::new(e),
            check_health: false,
            check_max_util: Vec::new(e),
        }
    }

    /// Add tokens the sender needs to transfer to the pool
    pub fn add_for_spender_transfer(&mut self, asset: &Address, amount: i128) {
        self.spender_transfer.set(
            asset.clone(),
            amount + self.spender_transfer.get(asset.clone()).unwrap_or(0),
        );
    }

    // Add tokens the pool needs to transfer to "to"
    pub fn add_for_pool_transfer(&mut self, asset: &Address, amount: i128) {
        self.pool_transfer.set(
            asset.clone(),
            amount + self.pool_transfer.get(asset.clone()).unwrap_or(0),
        );
    }

    // just a simple flag since we won't need
    // to switch it back to false once set to true.
    pub fn do_check_health(&mut self) {
        self.check_health = true
    }

    // Add "reserve" to the list of reserves to check max utilization for
    pub fn do_check_max_util(&mut self, reserve: &Address) {
        if self.check_max_util.contains(reserve) {
            return;
        }
        self.check_max_util.push_back(reserve.clone());
    }
}

/// Build a set of pool actions and the new positions from the supplied requests. Validates that the requests
/// are valid based on the status and supported reserves in the pool.
///
/// ### Arguments
/// * pool - The pool
/// * from - The sender of the requests
/// * requests - The requests to be processed
///
/// ### Returns
/// A tuple of (actions, positions, check_health) where:
/// * actions - A actions to be taken by the pool
/// * user - The state of the "from" user after the requests have been processed
/// * check_health - A bool indicating if a health factor check should be performed
///
/// ### Panics
/// If the request is invalid, or if the pool is in an invalid state.
pub fn build_actions_from_request(
    e: &Env,
    pool: &mut Pool,
    from_state: &mut User,
    requests: Vec<Request>,
) -> Actions {
    let mut actions = Actions::new(e);
    for request in requests.iter() {
        // verify the request is allowed
        require_nonnegative(e, &request.amount);
        pool.require_action_allowed(e, request.request_type);
        match RequestType::from_u32(e, request.request_type) {
            RequestType::Supply => {
                let b_tokens_minted = apply_supply(e, &mut actions, pool, from_state, &request);
                PoolEvents::supply(
                    e,
                    request.address.clone(),
                    from_state.address.clone(),
                    request.amount,
                    b_tokens_minted,
                );
            }
            RequestType::Withdraw => {
                let (tokens_out, b_tokens_burnt) =
                    apply_withdraw(e, &mut actions, pool, from_state, &request);
                PoolEvents::withdraw(
                    e,
                    request.address.clone(),
                    from_state.address.clone(),
                    tokens_out,
                    b_tokens_burnt,
                );
            }
            RequestType::SupplyCollateral => {
                let b_tokens_minted =
                    apply_supply_collateral(e, &mut actions, pool, from_state, &request);
                PoolEvents::supply_collateral(
                    e,
                    request.address.clone(),
                    from_state.address.clone(),
                    request.amount,
                    b_tokens_minted,
                );
            }
            RequestType::WithdrawCollateral => {
                let (tokens_out, b_tokens_burnt) =
                    apply_withdraw_collateral(e, &mut actions, pool, from_state, &request);
                PoolEvents::withdraw_collateral(
                    e,
                    request.address.clone(),
                    from_state.address.clone(),
                    tokens_out,
                    b_tokens_burnt,
                );
            }
            RequestType::Borrow => {
                let d_tokens_minted = apply_borrow(e, &mut actions, pool, from_state, &request);
                PoolEvents::borrow(
                    e,
                    request.address.clone(),
                    from_state.address.clone(),
                    request.amount,
                    d_tokens_minted,
                );
            }
            RequestType::Repay => {
                let (tokens_in, d_tokens_burnt) =
                    apply_repay(e, &mut actions, pool, from_state, &request);
                PoolEvents::repay(
                    e,
                    request.address.clone(),
                    from_state.address.clone(),
                    tokens_in,
                    d_tokens_burnt,
                );
            }
            RequestType::FillUserLiquidationAuction => {
                let filled_auction = auctions::fill(
                    e,
                    pool,
                    0,
                    &request.address,
                    from_state,
                    request.amount as u64,
                );
                actions.do_check_health();

                PoolEvents::fill_auction(
                    e,
                    0u32,
                    request.address.clone(),
                    from_state.address.clone(),
                    request.amount,
                    filled_auction,
                );
            }
            RequestType::FillBadDebtAuction => {
                // Note: will fail if input address is not the backstop since there cannot be a bad debt auction for a different address in storage
                let filled_auction = auctions::fill(
                    e,
                    pool,
                    1,
                    &request.address,
                    from_state,
                    request.amount as u64,
                );
                actions.do_check_health();

                PoolEvents::fill_auction(
                    e,
                    1u32,
                    request.address.clone(),
                    from_state.address.clone(),
                    request.amount,
                    filled_auction,
                );
            }
            RequestType::FillInterestAuction => {
                // Note: will fail if input address is not the backstop since there cannot be an interest auction for a different address in storage
                let filled_auction = auctions::fill(
                    e,
                    pool,
                    2,
                    &request.address,
                    from_state,
                    request.amount as u64,
                );
                PoolEvents::fill_auction(
                    e,
                    2u32,
                    request.address.clone(),
                    from_state.address.clone(),
                    request.amount,
                    filled_auction,
                );
            }
            RequestType::DeleteLiquidationAuction => {
                // Note: request object is ignored besides type
                auctions::delete_liquidation(e, &from_state.address);
                actions.do_check_health();
                PoolEvents::delete_auction(
                    e,
                    AuctionType::UserLiquidation as u32,
                    from_state.address.clone(),
                );
            }
        }
    }

    actions
}

/// Apply a "supply" request to the pool
///
/// Appends any necessary actions to the actions list, updates the user and pool's state
///
/// Returns the amount of b_tokens minted
fn apply_supply(
    e: &Env,
    actions: &mut Actions,
    pool: &mut Pool,
    user: &mut User,
    request: &Request,
) -> i128 {
    let mut reserve = pool.load_reserve(e, &request.address, true);
    reserve.require_action_allowed(e, request.request_type);
    let b_tokens_minted = reserve.to_b_token_down(e, request.amount);
    user.add_supply(e, &mut reserve, b_tokens_minted);
    actions.add_for_spender_transfer(&reserve.asset, request.amount);
    if reserve.total_supply(e) > reserve.config.supply_cap {
        panic_with_error!(e, PoolError::ExceededSupplyCap);
    }
    pool.cache_reserve(reserve);
    b_tokens_minted
}

/// Apply a "withdraw" request to the pool
///
/// Appends any necessary actions to the actions list, updates the user and pool's state
///
/// Returns the amount of tokens withdrawn and b_tokens burnt
fn apply_withdraw(
    e: &Env,
    actions: &mut Actions,
    pool: &mut Pool,
    user: &mut User,
    request: &Request,
) -> (i128, i128) {
    let mut reserve = pool.load_reserve(e, &request.address, true);
    let cur_b_tokens = user.get_supply(reserve.config.index);
    let mut to_burn = reserve.to_b_token_up(e, request.amount);
    let mut tokens_out = request.amount;
    if to_burn > cur_b_tokens {
        to_burn = cur_b_tokens;
        tokens_out = reserve.to_asset_from_b_token(e, cur_b_tokens);
    }
    user.remove_supply(e, &mut reserve, to_burn);
    reserve.require_utilization_below_100(e);
    actions.add_for_pool_transfer(&reserve.asset, tokens_out);
    pool.cache_reserve(reserve);
    (tokens_out, to_burn)
}

/// Apply a "supply_collateral" request to the pool
///
/// Appends any necessary actions to the actions list, updates the user and pool's state
///
/// Returns the amount of b_tokens minted
fn apply_supply_collateral(
    e: &Env,
    actions: &mut Actions,
    pool: &mut Pool,
    user: &mut User,
    request: &Request,
) -> i128 {
    let mut reserve = pool.load_reserve(e, &request.address, true);
    reserve.require_action_allowed(e, request.request_type);
    let b_tokens_minted = reserve.to_b_token_down(e, request.amount);
    user.add_collateral(e, &mut reserve, b_tokens_minted);
    actions.add_for_spender_transfer(&reserve.asset, request.amount);
    if reserve.total_supply(e) > reserve.config.supply_cap {
        panic_with_error!(e, PoolError::ExceededSupplyCap);
    }
    pool.cache_reserve(reserve);
    b_tokens_minted
}

/// Apply a "withdraw_collateral" request to the pool
///
/// Appends any necessary actions to the actions list, updates the user and pool's state
///
/// Returns the amount of tokens withdrawn and b_tokens burnt
fn apply_withdraw_collateral(
    e: &Env,
    actions: &mut Actions,
    pool: &mut Pool,
    user: &mut User,
    request: &Request,
) -> (i128, i128) {
    let mut reserve = pool.load_reserve(e, &request.address, true);
    let cur_b_tokens = user.get_collateral(reserve.config.index);
    let mut to_burn = reserve.to_b_token_up(e, request.amount);
    let mut tokens_out = request.amount;
    if to_burn > cur_b_tokens {
        to_burn = cur_b_tokens;
        tokens_out = reserve.to_asset_from_b_token(e, cur_b_tokens);
    }
    user.remove_collateral(e, &mut reserve, to_burn);
    reserve.require_utilization_below_100(e);
    actions.add_for_pool_transfer(&reserve.asset, tokens_out);
    actions.do_check_health();
    pool.cache_reserve(reserve);
    (tokens_out, to_burn)
}

/// Apply a "borrow" request to the pool
///
/// Appends any necessary actions to the actions list, updates the user and pool's state
///
/// Returns the amount of d_tokens minted
fn apply_borrow(
    e: &Env,
    actions: &mut Actions,
    pool: &mut Pool,
    user: &mut User,
    request: &Request,
) -> i128 {
    let mut reserve = pool.load_reserve(e, &request.address, true);
    reserve.require_action_allowed(e, request.request_type);
    let d_tokens_minted = reserve.to_d_token_up(e, request.amount);
    user.add_liabilities(e, &mut reserve, d_tokens_minted);
    reserve.require_utilization_below_100(e);
    actions.do_check_max_util(&reserve.asset);
    actions.add_for_pool_transfer(&reserve.asset, request.amount);
    actions.do_check_health();
    pool.cache_reserve(reserve);
    d_tokens_minted
}

/// Apply a "repay" request to the pool
///
/// Appends any necessary actions to the actions list, updates the user and pool's state
///
/// Returns the repayment amount and d_tokens_burnt
fn apply_repay(
    e: &Env,
    actions: &mut Actions,
    pool: &mut Pool,
    user: &mut User,
    request: &Request,
) -> (i128, i128) {
    let mut reserve = pool.load_reserve(e, &request.address, true);
    let cur_d_tokens = user.get_liabilities(reserve.config.index);
    let d_tokens_burnt = reserve.to_d_token_down(e, request.amount);
    let repayment_amount = request.amount;
    if d_tokens_burnt > cur_d_tokens {
        let cur_underlying_borrowed = reserve.to_asset_from_d_token(e, cur_d_tokens);
        let amount_to_refund = request.amount - cur_underlying_borrowed;
        require_nonnegative(e, &amount_to_refund);
        actions.add_for_spender_transfer(&reserve.asset, request.amount);
        actions.add_for_pool_transfer(&reserve.asset, amount_to_refund);
        user.remove_liabilities(e, &mut reserve, cur_d_tokens);
        pool.cache_reserve(reserve);
        (cur_underlying_borrowed, cur_d_tokens)
    } else {
        actions.add_for_spender_transfer(&reserve.asset, request.amount);
        user.remove_liabilities(e, &mut reserve, d_tokens_burnt);
        pool.cache_reserve(reserve);
        (repayment_amount, d_tokens_burnt)
    }
}
