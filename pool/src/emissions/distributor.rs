use cast::i128;
use sep_41_token::TokenClient;
use soroban_fixed_point_math::SorobanFixedPoint;
use soroban_sdk::{panic_with_error, Address, Env, Vec};

use crate::{
    constants::SCALAR_7,
    errors::PoolError,
    pool::User,
    storage::{self, ReserveEmissionData, UserEmissionData},
    validator::require_nonnegative,
};

/// Performs a claim against the given "reserve_token_ids" for "from"
pub fn execute_claim(e: &Env, from: &Address, reserve_token_ids: &Vec<u32>, to: &Address) -> i128 {
    let from_state = User::load(e, from);
    let reserve_list = storage::get_res_list(e);
    let mut to_claim = 0;
    for reserve_token_id in reserve_token_ids.clone() {
        let reserve_index = reserve_token_id / 2;
        let reserve_addr = reserve_list.get(reserve_index);
        match reserve_addr {
            Some(res_address) => {
                let reserve_config = storage::get_res_config(e, &res_address);
                let reserve_data = storage::get_res_data(e, &res_address);
                let (user_balance, supply) = match reserve_token_id % 2 {
                    0 => (
                        from_state.get_liabilities(reserve_index),
                        reserve_data.d_supply,
                    ),
                    1 => (
                        from_state.get_total_supply(reserve_index),
                        reserve_data.b_supply,
                    ),
                    _ => panic_with_error!(e, PoolError::BadRequest),
                };
                to_claim += claim_emissions(
                    e,
                    reserve_token_id,
                    supply,
                    10i128.pow(reserve_config.decimals),
                    from,
                    user_balance,
                );
            }
            None => {
                panic_with_error!(e, PoolError::BadRequest)
            }
        }
    }

    if to_claim > 0 {
        let backstop = storage::get_backstop(e);
        let blnd_token = storage::get_blnd_token(e);
        TokenClient::new(e, &blnd_token).transfer_from(
            &e.current_contract_address(),
            &backstop,
            to,
            &to_claim,
        );
    }
    to_claim
}

/// Update the emissions information about a reserve token. Must be called before any update
/// is made to the supply of debtTokens or blendTokens.
///
/// A reserve token id is a unique identifier for a position in a pool.
/// - For a reserve's dTokens (liabilities), reserve_token_id = reserve_index * 2
/// - For a reserve's bTokens (supply/collateral), reserve_token_id = reserve_index * 2 + 1
///
/// Returns the amount of tokens to claim, or zero if 'claim' is false
///
/// ### Arguments
/// * `res_token_id` - The reserve token id being acted against
/// * `supply` - The current supply of the reserve token
/// * `supply_scalar` - The scalar of the reserve token
/// * `user` - The user performing an action against the reserve
/// * `balance` - The current balance of the user
///
/// ### Panics
/// If the reserve update failed
pub fn update_emissions(
    e: &Env,
    res_token_id: u32,
    supply: i128,
    supply_scalar: i128,
    user: &Address,
    balance: i128,
) {
    if let Some(res_emis_data) = update_emission_data(e, res_token_id, supply, supply_scalar) {
        update_user_emissions(
            e,
            &res_emis_data,
            res_token_id,
            supply_scalar,
            user,
            balance,
            false,
        );
    }
}

/// Update and claim the emissions for a reserve token.
///
/// Returns the amount of tokens to claim.
///
/// ### Arguments
/// * `res_token_id` - The reserve token being acted against => (reserve index * 2 + (0 for debtToken or 1 for blendToken))
/// * `supply` - The current supply of the reserve token
/// * `supply_scalar` - The scalar of the reserve token
/// * `user` - The user claiming for the reserve
/// * `balance` - The current balance of the user
///
/// ### Panics
/// If the reserve update failed
pub(crate) fn claim_emissions(
    e: &Env,
    res_token_id: u32,
    supply: i128,
    supply_scalar: i128,
    user: &Address,
    balance: i128,
) -> i128 {
    if let Some(res_emis_data) = update_emission_data(e, res_token_id, supply, supply_scalar) {
        update_user_emissions(
            e,
            &res_emis_data,
            res_token_id,
            supply_scalar,
            user,
            balance,
            true,
        )
    } else {
        0
    }
}

/// Update the reserve token emission data
///
/// Returns the new ReserveEmissionData, if None if no data exists
///
/// ### Arguments
/// * `res_token_id` - The reserve token being acted against => (reserve index * 2 + (0 for debtToken or 1 for blendToken))
/// * `supply` - The current supply of the reserve token
/// * `supply_scalar` - The scalar of the reserve token
/// * `emis_config` - The reserve token emission configuration
///
/// ### Panics
/// If the reserve update failed
pub(super) fn update_emission_data(
    e: &Env,
    res_token_id: u32,
    supply: i128,
    supply_scalar: i128,
) -> Option<ReserveEmissionData> {
    match storage::get_res_emis_data(e, &res_token_id) {
        Some(mut res_emission_data) => {
            if res_emission_data.last_time >= res_emission_data.expiration
                || e.ledger().timestamp() == res_emission_data.last_time
                || res_emission_data.eps == 0
                || supply == 0
            {
                return Some(res_emission_data);
            }

            let ledger_timestamp = if e.ledger().timestamp() > res_emission_data.expiration {
                res_emission_data.expiration
            } else {
                e.ledger().timestamp()
            };

            let additional_idx = (i128(ledger_timestamp - res_emission_data.last_time)
                * i128(res_emission_data.eps))
            .fixed_div_floor(&e, &supply, &supply_scalar);

            res_emission_data.index += additional_idx;
            res_emission_data.last_time = ledger_timestamp;
            storage::set_res_emis_data(e, &res_token_id, &res_emission_data);
            Some(res_emission_data)
        }
        None => return None, // no emission exist, no update is required
    }
}

pub(crate) fn update_user_emissions(
    e: &Env,
    res_emis_data: &ReserveEmissionData,
    res_token_id: u32,
    supply_scalar: i128,
    user: &Address,
    balance: i128,
    claim: bool,
) -> i128 {
    if let Some(user_data) = storage::get_user_emissions(e, user, &res_token_id) {
        if user_data.index != res_emis_data.index || claim {
            let mut accrual = user_data.accrued;
            if balance != 0 {
                let delta_index = res_emis_data.index - user_data.index;
                require_nonnegative(e, &delta_index);
                let to_accrue = balance.fixed_mul_floor(
                    e,
                    &(res_emis_data.index - user_data.index),
                    &(supply_scalar * SCALAR_7),
                );
                accrual += to_accrue;
            }
            return set_user_emissions(e, user, res_token_id, res_emis_data.index, accrual, claim);
        }
        0
    } else if balance == 0 {
        // first time the user registered an action with the asset since emissions were added
        return set_user_emissions(e, user, res_token_id, res_emis_data.index, 0, claim);
    } else {
        // user had tokens before emissions began, they are due any historical emissions
        let to_accrue =
            balance.fixed_mul_floor(e, &res_emis_data.index, &(supply_scalar * SCALAR_7));
        return set_user_emissions(e, user, res_token_id, res_emis_data.index, to_accrue, claim);
    }
}

pub(crate) fn set_user_emissions(
    e: &Env,
    user: &Address,
    res_token_id: u32,
    index: i128,
    accrued: i128,
    claim: bool,
) -> i128 {
    if claim {
        storage::set_user_emissions(
            e,
            user,
            &res_token_id,
            &UserEmissionData { index, accrued: 0 },
        );
        accrued
    } else {
        storage::set_user_emissions(e, user, &res_token_id, &UserEmissionData { index, accrued });
        0
    }
}
