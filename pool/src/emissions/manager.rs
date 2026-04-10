use crate::{
    constants::SCALAR_7,
    dependencies::BackstopClient,
    errors::PoolError,
    events::PoolEvents,
    storage::{self, ReserveConfig, ReserveEmissionData},
};
use cast::{i128, u64};
use soroban_fixed_point_math::SorobanFixedPoint;
use soroban_sdk::{
    contracttype, map, panic_with_error, unwrap::UnwrapOptimized, Address, Env, Map, Vec,
};

use super::distributor;

// Types

/// Metadata for a pool's reserve emission configuration
#[contracttype]
pub struct ReserveEmissionMetadata {
    pub res_index: u32,
    pub res_type: u32,
    pub share: u64,
}

/// Set the pool emissions
///
/// These will not be applied until the next `update_emissions` is run
///
/// ### Arguments
/// * `res_emission_metadata` - A vector of `ReserveEmissionMetadata` that details each reserve token's share
///                             if the total pool eps
///
/// ### Panics
/// If any res_emission_metadata is included where share is 0, the reserve index is invalid,
/// or the reserve type is invalid
pub fn set_pool_emissions(e: &Env, res_emission_metadata: Vec<ReserveEmissionMetadata>) {
    let mut pool_emissions: Map<u32, u64> = map![e];

    let reserve_list = storage::get_res_list(e);
    for metadata in res_emission_metadata {
        let key = metadata.res_index * 2 + metadata.res_type;
        if metadata.res_type > 1
            || reserve_list.get(metadata.res_index).is_none()
            || metadata.share == 0
        {
            panic_with_error!(e, PoolError::BadRequest);
        }
        pool_emissions.set(key, metadata.share);
    }

    storage::set_pool_emissions(e, &pool_emissions);
}

/// Consume emitted tokens from the backstop and distribute them to reserves
///
/// Returns the number of new tokens distributed for emissions
///
/// ### Panics
/// If the pool is not in the backstop reward zone
pub fn gulp_emissions(e: &Env) -> i128 {
    let backstop = storage::get_backstop(e);
    let new_emissions =
        BackstopClient::new(e, &backstop).gulp_emissions(&e.current_contract_address());
    do_gulp_emissions(e, new_emissions);
    new_emissions
}

pub(crate) fn do_gulp_emissions(e: &Env, new_emissions: i128) {
    // ensure enough tokens are being emitted to avoid rounding issues
    if new_emissions < SCALAR_7 {
        panic_with_error!(e, PoolError::BadRequest)
    }
    let pool_emissions = storage::get_pool_emissions(e);
    let reserve_list = storage::get_res_list(e);
    let mut pool_emis_enabled: Vec<(ReserveConfig, Address, u32, u64)> = Vec::new(e);

    let mut total_share: i128 = 0;
    for (res_token_id, res_eps_share) in pool_emissions.iter() {
        let reserve_index = res_token_id / 2;
        let res_asset_address = reserve_list.get_unchecked(reserve_index);
        let res_config = storage::get_res_config(e, &res_asset_address);

        if res_config.enabled {
            pool_emis_enabled.push_back((
                res_config,
                res_asset_address,
                res_token_id,
                res_eps_share,
            ));
            total_share += i128(res_eps_share);
        }
    }
    for (res_config, res_asset_address, res_token_id, res_eps_share) in pool_emis_enabled {
        let new_reserve_emissions = i128(res_eps_share)
            .fixed_div_floor(e, &total_share, &SCALAR_7)
            .fixed_mul_floor(e, &new_emissions, &SCALAR_7);

        update_reserve_emission_eps(
            e,
            &res_config,
            &res_asset_address,
            res_token_id,
            new_reserve_emissions,
        );
    }
}

fn update_reserve_emission_eps(
    e: &Env,
    reserve_config: &ReserveConfig,
    asset: &Address,
    res_token_id: u32,
    new_reserve_emissions: i128,
) {
    let mut tokens_left_to_emit = new_reserve_emissions;
    let reserve_data = storage::get_res_data(e, asset);
    let supply = match res_token_id % 2 {
        0 => reserve_data.d_supply,
        1 => reserve_data.b_supply,
        _ => panic_with_error!(e, PoolError::BadRequest),
    };
    let expiration: u64 = e.ledger().timestamp() + 7 * 24 * 60 * 60;

    if let Some(mut emission_data) = distributor::update_emission_data(
        e,
        res_token_id,
        supply,
        10i128.pow(reserve_config.decimals),
    ) {
        // data exists - update it with old config

        if emission_data.last_time != e.ledger().timestamp() {
            // force the emission data to be updated to the current timestamp
            emission_data.last_time = e.ledger().timestamp();
        }
        // determine the amount of tokens not emitted from the last config
        if emission_data.expiration > e.ledger().timestamp() {
            let time_left_till_exp = emission_data.expiration - e.ledger().timestamp();

            // Eps is scaled by 14 decimals
            let tokens_to_emit_till_exp =
                i128(emission_data.eps).fixed_mul_floor(e, &i128(time_left_till_exp), &SCALAR_7);
            tokens_left_to_emit += tokens_to_emit_till_exp;
        }

        let eps = u64(tokens_left_to_emit * SCALAR_7 / (7 * 24 * 60 * 60)).unwrap_optimized();

        emission_data.expiration = expiration;
        emission_data.eps = eps;
        storage::set_res_emis_data(e, &res_token_id, &emission_data);
        PoolEvents::reserve_emission_update(e, res_token_id, eps, expiration);
    } else {
        // no config or data exists yet - first time this reserve token will get emission
        let eps = u64(tokens_left_to_emit * SCALAR_7 / (7 * 24 * 60 * 60)).unwrap_optimized();
        storage::set_res_emis_data(
            e,
            &res_token_id,
            &ReserveEmissionData {
                expiration,
                eps,
                index: 0,
                last_time: e.ledger().timestamp(),
            },
        );
        PoolEvents::reserve_emission_update(e, res_token_id, eps, expiration);
    }
}
