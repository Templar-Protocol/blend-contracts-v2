//! Methods for distributing backstop emissions to depositors

use cast::i128;
use soroban_fixed_point_math::FixedPoint;
use soroban_sdk::{panic_with_error, unwrap::UnwrapOptimized, Address, Env};

use crate::{
    backstop::{PoolBalance, UserBalance},
    constants::{SCALAR_14, SCALAR_7},
    require_nonnegative,
    storage::{self, BackstopEmissionData, UserEmissionData},
    BackstopError,
};

/// Update the backstop emissions index for the user and pool
pub fn update_emissions(
    e: &Env,
    pool_id: &Address,
    pool_balance: &PoolBalance,
    user_id: &Address,
    user_balance: &UserBalance,
) {
    if let Some(emis_data) = update_emission_data(e, pool_id, pool_balance) {
        update_user_emissions(e, pool_id, user_id, &emis_data, user_balance, false);
    }
}

/// Update for claiming emissions for a user and pool
///
/// DOES NOT SEND CLAIMED TOKENS TO THE USER. The caller
/// is expected to handle sending the tokens once all claimed pools
/// have been processed.
///
/// Returns the number of tokens that need to be transferred to `user`
///
/// Panics if the pool's backstop never had emissions configured
pub(crate) fn claim_emissions(
    e: &Env,
    pool_id: &Address,
    pool_balance: &PoolBalance,
    user_id: &Address,
    user_balance: &UserBalance,
) -> i128 {
    if let Some(emis_data) = update_emission_data(e, pool_id, pool_balance) {
        update_user_emissions(e, pool_id, user_id, &emis_data, user_balance, true)
    } else {
        panic_with_error!(e, BackstopError::BadRequest)
    }
}

/// Update the backstop emissions index for deposits
pub fn update_emission_data(
    e: &Env,
    pool_id: &Address,
    pool_balance: &PoolBalance,
) -> Option<BackstopEmissionData> {
    match storage::get_backstop_emis_data(e, pool_id) {
        Some(emis_data) => {
            if emis_data.last_time >= emis_data.expiration
                || e.ledger().timestamp() == emis_data.last_time
                || emis_data.eps == 0
                || pool_balance.shares == 0
            {
                // emis_data already updated or expired
                return Some(emis_data);
            }

            let max_timestamp = if e.ledger().timestamp() > emis_data.expiration {
                emis_data.expiration
            } else {
                e.ledger().timestamp()
            };

            let unqueued_shares = pool_balance.shares - pool_balance.q4w;
            require_nonnegative(e, unqueued_shares);
            let additional_idx: i128;
            if unqueued_shares == 0 {
                // all shares q4w, omit emissions
                additional_idx = 0;
            } else {
                // Eps is in 14 decimals and needs to be converted to 7 decimals to match emission token decimals
                additional_idx = (i128(max_timestamp - emis_data.last_time) * i128(emis_data.eps))
                    .fixed_div_floor(unqueued_shares, SCALAR_7)
                    .unwrap_optimized();
            }
            let new_data = BackstopEmissionData {
                eps: emis_data.eps,
                expiration: emis_data.expiration,
                index: additional_idx + emis_data.index,
                last_time: e.ledger().timestamp(),
            };

            storage::set_backstop_emis_data(e, pool_id, &new_data);
            Some(new_data)
        }
        None => return None, // no emission exist, no update is required
    }
}

/// Update the user's emissions. If `to_claim` is true, the user's accrued emissions will be returned and
/// a value of zero will be stored to the ledger.
///
/// ### Returns
/// The number of emitted tokens the caller needs to send to the user
fn update_user_emissions(
    e: &Env,
    pool: &Address,
    user: &Address,
    emis_data: &BackstopEmissionData,
    user_balance: &UserBalance,
    to_claim: bool,
) -> i128 {
    if let Some(user_data) = storage::get_user_emis_data(e, pool, user) {
        if user_data.index != emis_data.index || to_claim {
            let mut accrual = user_data.accrued;
            if user_balance.shares != 0 {
                let delta_index = emis_data.index - user_data.index;
                require_nonnegative(e, delta_index);
                let to_accrue = (user_balance.shares)
                    .fixed_mul_floor(delta_index, SCALAR_14)
                    .unwrap_optimized();
                accrual += to_accrue;
            }
            return set_user_emissions(e, pool, user, emis_data.index, accrual, to_claim);
        }
        // no accrual occured and no claim requested
        return 0;
    } else if user_balance.shares == 0 {
        // first time the user registered an action with the asset since emissions were added
        return set_user_emissions(e, pool, user, emis_data.index, 0, to_claim);
    } else {
        // user had tokens before emissions began, they are due any historical emissions
        let to_accrue = user_balance
            .shares
            .fixed_mul_floor(emis_data.index, SCALAR_14)
            .unwrap_optimized();
        return set_user_emissions(e, pool, user, emis_data.index, to_accrue, to_claim);
    }
}

fn set_user_emissions(
    e: &Env,
    pool_id: &Address,
    user: &Address,
    index: i128,
    accrued: i128,
    to_claim: bool,
) -> i128 {
    if to_claim {
        storage::set_user_emis_data(e, pool_id, user, &UserEmissionData { index, accrued: 0 });
        accrued
    } else {
        storage::set_user_emis_data(e, pool_id, user, &UserEmissionData { index, accrued });
        0
    }
}
