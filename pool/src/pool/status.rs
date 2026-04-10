use crate::{
    constants::SCALAR_7,
    dependencies::{BackstopClient, PoolBackstopData},
    storage, PoolError,
};
use soroban_sdk::{panic_with_error, Env};

/// Update the pool status based on the backstop module
#[allow(clippy::zero_prefixed_literal)]
#[allow(clippy::inconsistent_digit_grouping)]
pub fn execute_update_pool_status(e: &Env) -> u32 {
    let mut pool_config = storage::get_pool_config(e);

    // check the pool has met minimum backstop deposits
    let backstop_id = storage::get_backstop(e);
    let backstop_client = BackstopClient::new(e, &backstop_id);

    let pool_backstop_data = backstop_client.pool_data(&e.current_contract_address());
    let threshold = calc_pool_backstop_threshold(&pool_backstop_data);
    let mut met_threshold = true;
    if threshold < SCALAR_7 {
        met_threshold = false;
    }

    match pool_config.status {
        // Setup
        6 => {
            // Setup supersedes all other statuses
            panic_with_error!(e, PoolError::StatusNotAllowed);
        }
        // Admin frozen
        4 => {
            // Admin frozen supersedes all other statuses
            panic_with_error!(e, PoolError::StatusNotAllowed);
        }
        // Admin on-ice
        2 => {
            if pool_backstop_data.q4w_pct >= 0_7500000 {
                // Q4W over 75% freezes the pool
                pool_config.status = 5;
            }
        }
        // Admin active
        0 => {
            if !met_threshold || pool_backstop_data.q4w_pct >= 0_5000000 {
                // Q4w over 50% or being under threshold puts the pool on-ice
                pool_config.status = 3;
            }
        }
        // Admin status isn't set
        _ => {
            if pool_backstop_data.q4w_pct >= 0_6000000 {
                // Q4w over 60% sets pool to Frozen
                pool_config.status = 5;
            } else if pool_backstop_data.q4w_pct >= 0_3000000 || !met_threshold {
                // Q4w over 30% sets pool to On-Ice
                pool_config.status = 3;
            } else {
                // Backstop is healthy and the pool is set to Active
                pool_config.status = 1;
            }
        }
    }
    storage::set_pool_config(e, &pool_config);
    pool_config.status
}

/// Admin set the pool status
#[allow(clippy::zero_prefixed_literal)]
#[allow(clippy::inconsistent_digit_grouping)]
pub fn execute_set_pool_status(e: &Env, pool_status: u32) {
    let mut pool_config = storage::get_pool_config(e);

    // check the pool has met minimum backstop deposits
    let backstop_id = storage::get_backstop(e);
    let backstop_client = BackstopClient::new(e, &backstop_id);

    let pool_backstop_data = backstop_client.pool_data(&e.current_contract_address());

    match pool_status {
        0 => {
            // Threshold must be met and q4w must be under 50% for the admin to set Active
            if calc_pool_backstop_threshold(&pool_backstop_data) < SCALAR_7
                || pool_backstop_data.q4w_pct >= 0_5000000
            {
                panic_with_error!(e, PoolError::StatusNotAllowed);
            }
            // Admin Active
            pool_config.status = 0;
        }
        2 => {
            // Q4w must be under 75% for admin to set On-Ice
            if pool_backstop_data.q4w_pct >= 0_7500000 {
                panic_with_error!(e, PoolError::StatusNotAllowed);
            }
            // Admin On-Ice
            pool_config.status = 2;
        }
        3 => {
            // Q4w must be under 75% for admin to set permissionless On-Ice
            if pool_backstop_data.q4w_pct >= 0_7500000 {
                panic_with_error!(e, PoolError::StatusNotAllowed);
            }
            // On-Ice
            pool_config.status = 3;
        }
        4 => {
            // Admin can always freeze the pool
            // Admin Frozen
            pool_config.status = 4;
        }
        _ => {
            panic_with_error!(e, PoolError::BadRequest);
        }
    }
    storage::set_pool_config(e, &pool_config);
}

/// Calculate the threshold for the pool's backstop balance
///
/// Returns the threshold as a percentage^5 in SCALAR_7 points such that SCALAR_7 = 100%
/// NOTE: The result is the percentage^5 to simplify the calculation of the pools product constant.
///       Some useful results:
///         - greater than 1 = 100+%
///         - 1_0000000 = 100%
///         - 0_0000100 = ~10%
///         - 0_0000003 = ~5%
///         - 0_0000000 = ~0-4%
pub fn calc_pool_backstop_threshold(pool_backstop_data: &PoolBackstopData) -> i128 {
    // @dev: Calculation for pools product constant of underlying will often overflow i128
    //       so saturating mul is used. This is safe because the threshold is below i128::MAX and the
    //       protocol does not need to differentiate between pools over the threshold product constant.
    //       The calculation is:
    //        - Threshold % = (bal_blnd^4 * bal_usdc) / PC^5 such that PC is 100k
    let threshold_pc = 10_000_000_000_000_000_000_000_000i128; // 1e25 (100k^5)

    // floor balances to nearest full unit and calculate saturated pool product constant
    // and scale to SCALAR_7 to get final division result in SCALAR_7 points
    let bal_blnd = pool_backstop_data.blnd / SCALAR_7;
    let bal_usdc = pool_backstop_data.usdc / SCALAR_7;
    let saturating_pool_pc = bal_blnd
        .saturating_mul(bal_blnd)
        .saturating_mul(bal_blnd)
        .saturating_mul(bal_blnd)
        .saturating_mul(bal_usdc)
        .saturating_mul(SCALAR_7); // 10^7 * 10^7
    saturating_pool_pc / threshold_pc
}
