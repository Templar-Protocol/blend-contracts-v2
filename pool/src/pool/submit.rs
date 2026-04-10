use moderc3156::FlashLoanClient;
use sep_41_token::TokenClient;
use soroban_sdk::{panic_with_error, Address, Env, Map, Vec};

use crate::{events::PoolEvents, storage, AuctionType, PoolError};

use super::{
    actions::{build_actions_from_request, Actions, Request},
    health_factor::PositionData,
    pool::Pool,
    FlashLoan, Positions, RequestType, User,
};

/// Execute a set of updates for a user against the pool.
///
/// ### Arguments
/// * from - The address of the user whose positions are being modified
/// * spender - The address of the user who is sending tokens to the pool
/// * to - The address of the user who is receiving tokens from the pool
/// * requests - A vec of requests to be processed
/// * use_allowance - A bool indicating if transfer_from is to be used
///
/// ### Panics
/// If the request is unable to be fully executed
pub fn execute_submit(
    e: &Env,
    from: &Address,
    spender: &Address,
    to: &Address,
    requests: Vec<Request>,
    use_allowance: bool,
) -> Positions {
    if from == &e.current_contract_address()
        || spender == &e.current_contract_address()
        || to == &e.current_contract_address()
    {
        panic_with_error!(e, &PoolError::BadRequest);
    }
    let mut pool = Pool::load(e);
    let mut from_state = User::load(e, from);

    let prev_positions_count = from_state.positions.effective_count();

    let actions = build_actions_from_request(e, &mut pool, &mut from_state, requests);

    validate_submit(
        e,
        &mut pool,
        &from_state,
        prev_positions_count,
        actions.check_health,
        &actions.check_max_util,
    );

    if use_allowance {
        handle_transfer_with_allowance(e, &actions, spender, to);
    } else {
        handle_transfers(e, &actions, spender, to);
    }

    // store updated info to ledger
    pool.store_cached_reserves(e);
    from_state.store(e);

    from_state.positions
}

/// Same as `execute_submit` but specifically made for performing a flash loan borrow before
/// the other submitted requests.
pub fn execute_submit_with_flash_loan(
    e: &Env,
    from: &Address,
    flash_loan: FlashLoan,
    requests: Vec<Request>,
) -> Positions {
    if from == &e.current_contract_address() {
        panic_with_error!(e, &PoolError::BadRequest);
    }
    let mut pool = Pool::load(e);
    let mut from_state = User::load(e, from);

    let prev_positions_count = from_state.positions.effective_count();

    // note: we add the flash loan liabilities before processing the other
    // requests.
    {
        pool.require_action_allowed(e, RequestType::Borrow as u32);
        let mut reserve = pool.load_reserve(e, &flash_loan.asset, true);
        let d_tokens_minted = reserve.to_d_token_up(e, flash_loan.amount);
        from_state.add_liabilities(e, &mut reserve, d_tokens_minted);
        reserve.require_action_allowed(e, RequestType::Borrow as u32);
        reserve.require_utilization_below_100(e);

        pool.cache_reserve(reserve);

        PoolEvents::flash_loan(
            e,
            flash_loan.asset.clone(),
            from.clone(),
            flash_loan.contract.clone(),
            flash_loan.amount,
            d_tokens_minted,
        );
    }

    let mut actions = build_actions_from_request(e, &mut pool, &mut from_state, requests);

    // require flash loaned asset is added to check_max_util
    if !actions.check_max_util.contains(&flash_loan.asset) {
        actions.check_max_util.push_back(flash_loan.asset.clone());
    }

    // always check health since flash_borrow requires it
    validate_submit(
        e,
        &mut pool,
        &from_state,
        prev_positions_count,
        true,
        &actions.check_max_util,
    );

    // we deal with the flashloan transfer before the others to allow the flash
    // loan to yield the repaid or supplied amount in the transfers.
    TokenClient::new(e, &flash_loan.asset).transfer(
        &e.current_contract_address(),
        &flash_loan.contract,
        &flash_loan.amount,
    );
    // calls the receiver contract with "from" as the caller
    FlashLoanClient::new(&e, &flash_loan.contract).exec_op(
        &from,
        &flash_loan.asset,
        &flash_loan.amount,
        &0,
    );

    // note: at this point, the pool has sum_by_asset(actions.flash_borrow.1) for each involved asset, but the user also has
    // increased liabilities. These will have to be either fully repaid by now in the requests following the flash borrow
    // or the user needs to have some previously added collateral to cover the borrow, i.e user is already healthy at this point,
    // we just have to make sure that they have the balances they are claiming to have through the transfers.

    handle_transfer_with_allowance(e, &actions, from, from);

    // store updated info to ledger
    pool.store_cached_reserves(e);
    from_state.store(e);

    from_state.positions
}

/// Validate submit results in a valid state for the pool and user.
///
/// ### Arguments
/// * pool - The pool state. Writes the oracle cache if oracle data is fetched.
/// * from_state - The user state for "from"
/// * prev_positions_count - The initial number of positions for "from"
/// * check_health - A bool indicating if the health factor should be checked
fn validate_submit(
    e: &Env,
    pool: &mut Pool,
    from_state: &User,
    prev_positions_count: u32,
    check_health: bool,
    check_max_util: &Vec<Address>,
) {
    // Verify max positions haven't been exceeded
    pool.require_under_max(e, &from_state.positions, prev_positions_count);

    // Verify "from" does not have an active liquidation post requests
    if storage::has_auction(
        e,
        &(AuctionType::UserLiquidation as u32),
        &from_state.address,
    ) {
        panic_with_error!(e, PoolError::AuctionInProgress);
    }

    // Verify all requested reserve's end utilization is below the max utilization
    for address in check_max_util {
        // these will all be cached already
        let reserve = pool.load_reserve(e, &address, false);
        reserve.require_utilization_below_max(e);
    }

    // panics if the new positions set does not meet the health factor requirement
    // min is 1.0000100 to prevent rounding errors
    if check_health && from_state.has_liabilities() {
        let position_data = PositionData::calculate_from_positions(e, pool, &from_state.positions);
        if position_data.is_hf_under(e, 1_0000100) {
            panic_with_error!(e, PoolError::InvalidHf);
        } else if position_data.collateral_base < pool.config.min_collateral {
            panic_with_error!(e, PoolError::MinCollateralNotMet);
        }
    }
}

fn handle_transfer_with_allowance(e: &Env, actions: &Actions, spender: &Address, to: &Address) {
    // map of token -> amount
    // amount can be negative:
    // pool owes when amount > 0
    // spender owes when amount < 0
    let mut net_balances: Map<Address, i128> = Map::new(e);

    for (token, amount) in actions.spender_transfer.iter() {
        net_balances.set(
            token.clone(),
            net_balances.get(token).unwrap_or_default() - amount,
        );
    }
    for (token, amount) in actions.pool_transfer.iter() {
        net_balances.set(
            token.clone(),
            net_balances.get(token).unwrap_or_default() + amount,
        );
    }

    for (address, amount) in net_balances {
        let token = TokenClient::new(e, &address);
        if amount < 0 {
            // transfer tokens from sender to pool
            token.transfer_from(
                &e.current_contract_address(),
                spender,
                &e.current_contract_address(),
                &amount.abs(),
            );
        } else if amount > 0 {
            // transfer tokens from pool to "to"
            token.transfer(&e.current_contract_address(), to, &amount);
        }
    }
}

fn handle_transfers(e: &Env, actions: &Actions, spender: &Address, to: &Address) {
    // transfer tokens from sender to pool
    for (address, amount) in actions.spender_transfer.iter() {
        TokenClient::new(e, &address).transfer(spender, &e.current_contract_address(), &amount);
    }

    // transfer tokens from pool to "to"
    for (address, amount) in actions.pool_transfer.iter() {
        TokenClient::new(e, &address).transfer(&e.current_contract_address(), to, &amount);
    }
}
