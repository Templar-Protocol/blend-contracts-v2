use cast::{i128, u64};
use sep_41_token::TokenClient;
use soroban_fixed_point_math::FixedPoint;
use soroban_sdk::{panic_with_error, unwrap::UnwrapOptimized, vec, Address, Env, Vec};

use crate::{
    backstop::{is_pool_above_threshold, load_pool_backstop_data},
    constants::{MAX_BACKFILLED_EMISSIONS, MAX_RZ_SIZE, SCALAR_7},
    dependencies::EmitterClient,
    errors::BackstopError,
    storage::{self, BackstopEmissionData, RzEmissions},
    PoolBalance,
};

use super::distributor::update_emission_data;

/// Add a pool to the reward zone. If the reward zone is full, attempt to swap it with the pool to remove.
pub fn add_to_reward_zone(e: &Env, to_add: Address, to_remove: Option<Address>) {
    let mut reward_zone = storage::get_reward_zone(e);

    // ensure an entity in the reward zone cannot be included twice
    if reward_zone.contains(to_add.clone()) {
        panic_with_error!(e, BackstopError::BadRequest);
    }

    // ensure to_add has met the minimum backstop deposit threshold
    // NOTE: "to_add" can only carry a pool balance if it is a deployed pool from the factory
    let pool_data = load_pool_backstop_data(e, &to_add);
    if !is_pool_above_threshold(&pool_data) {
        panic_with_error!(e, BackstopError::InvalidRewardZoneEntry);
    }

    // if updating the rz list, ensure distribute was run recently
    if reward_zone.len() > 0 {
        require_distribute_run_recently(e);
    }

    if MAX_RZ_SIZE > reward_zone.len() {
        // there is room in the reward zone. Add "to_add".
        reward_zone.push_front(to_add.clone());
    } else {
        match to_remove {
            None => panic_with_error!(e, BackstopError::RewardZoneFull),
            Some(to_remove) => {
                // Verify "to_add" has a higher backstop deposit that "to_remove"
                if pool_data.tokens <= storage::get_pool_balance(e, &to_remove).tokens {
                    panic_with_error!(e, BackstopError::InvalidRewardZoneEntry);
                }
                remove_pool(e, &mut reward_zone, &to_remove);
                reward_zone.push_front(to_add.clone());
            }
        }
    }
    storage::set_reward_zone(e, &reward_zone);
}

/// Remove a pool to the reward zone if below the minimum backstop deposit threshold
pub fn remove_from_reward_zone(e: &Env, to_remove: Address) {
    let mut reward_zone = storage::get_reward_zone(e);

    // ensure to_remove has not met the backstop threshold
    let pool_data = load_pool_backstop_data(e, &to_remove);
    if is_pool_above_threshold(&pool_data) {
        panic_with_error!(e, BackstopError::BadRequest);
    } else {
        require_distribute_run_recently(e);
        remove_pool(e, &mut reward_zone, &to_remove);
        storage::set_reward_zone(e, &reward_zone);
    }
}

/// Remove a pool from the reward zone
fn remove_pool(e: &Env, reward_zone: &mut Vec<Address>, to_remove: &Address) {
    let to_remove_index = reward_zone.first_index_of(to_remove.clone());
    match to_remove_index {
        Some(idx) => {
            reward_zone.remove(idx);
        }
        None => panic_with_error!(e, BackstopError::InvalidRewardZoneEntry),
    }
}

/// Require distribute was run recently to prevent rz edits from significantly disrupting emissions
///
/// Note - this will always fail after the emitter stops emitting tokens to the backstop. This is
/// ok as the reward zone is only used to determine the distribution of said emissions.
fn require_distribute_run_recently(e: &Env) {
    let last_distribution = storage::get_last_distribution_time(e);
    if last_distribution < e.ledger().timestamp() - 60 * 60 {
        panic_with_error!(e, BackstopError::BadRequest);
    }
}

/// Distribute emissions from the emitter to the reward zone and backstop depositors. This also implements
/// backfilling emissions if the emitter has not distributed to this version of the backstop before.
pub fn distribute(e: &Env) -> i128 {
    let is_backfill: bool;
    let mut needs_reset: bool = false;
    let last_backfill_status = storage::get_backfill_status(e);
    let emitter = storage::get_emitter(e);
    let emitter_last_distribution =
        match EmitterClient::new(&e, &emitter).try_get_last_distro(&e.current_contract_address()) {
            Ok(distro) => {
                is_backfill = false;
                if last_backfill_status != Some(false) {
                    // first time the backstop has gotten a distro time from the emitter
                    // reset last distribution time if we were backfilling previously
                    needs_reset = last_backfill_status == Some(true);
                    storage::set_backfill_status(e, &false);
                }
                distro.unwrap_optimized()
            }
            // allows for backfilled emissions
            Err(_) => {
                is_backfill = true;
                if last_backfill_status.is_none() {
                    // first time calling with backfill emissions
                    storage::set_backfill_status(e, &true);
                } else if last_backfill_status == Some(false) {
                    // backfilling has already stopped. Getting an error from the emitter
                    // is unexpected.
                    panic_with_error!(e, BackstopError::BadRequest);
                }
                e.ledger().timestamp()
            }
        };
    let last_distribution = storage::get_last_distribution_time(e);

    // if we have never distributed before, record the emitter's last distribution time and
    // start emissions from that time
    if last_distribution == 0 {
        storage::set_last_distribution_time(e, &emitter_last_distribution);
        return 0;
    }

    // if this is the first distribution after a backstop swap, we need to stop the backfill emissions
    // safely. The only way to do this is to reset the last distribution time to the emitters.
    // This skips all emissions between the last distribution time and the emitter's last distribution time.
    // This is necessary as the backstop cannot determine how much BLND was actually emitted
    // between those two timepoints.
    if needs_reset {
        storage::set_last_distribution_time(e, &emitter_last_distribution);
        return 0;
    }

    // if at least 5 seconds (1 block) has not passed, panic
    if emitter_last_distribution - last_distribution < 5 {
        panic_with_error!(e, BackstopError::BadRequest);
    }

    let reward_zone = storage::get_reward_zone(e);
    let rz_len = reward_zone.len();
    // reward zone must have at least one pool for emissions to start
    if rz_len == 0 {
        panic_with_error!(e, BackstopError::BadRequest);
    }

    // emitter releases 1 token per second
    let mut new_emissions = i128(emitter_last_distribution - last_distribution) * SCALAR_7;

    // if backfilling emissions, ensure we are not over the maximum backfilled emissions allotment.
    // backfilled emissions must fit within the maximum drop amount from the emitter.
    if is_backfill {
        let mut cur_backfill = storage::get_backfill_emissions(e);
        // panic if we already reached the maximum backfilled emissions
        if cur_backfill >= MAX_BACKFILLED_EMISSIONS {
            panic_with_error!(e, BackstopError::MaxBackfillEmissions);
        }
        // cap new emissions to the maximum backfilled emissions
        if new_emissions + cur_backfill > MAX_BACKFILLED_EMISSIONS {
            new_emissions = MAX_BACKFILLED_EMISSIONS - cur_backfill;
        }
        cur_backfill += new_emissions;
        storage::set_backfill_emissions(e, &cur_backfill);
    }
    storage::set_last_distribution_time(e, &emitter_last_distribution);

    let mut rz_balance: Vec<(Address, PoolBalance)> = vec![e];

    // fetch total non-queued backstop tokens in the reward zone
    let mut total_non_queued_tokens: i128 = 0;
    for rz_pool in reward_zone {
        let pool_balance = storage::get_pool_balance(e, &rz_pool);
        total_non_queued_tokens += pool_balance.non_queued_tokens();
        rz_balance.push_back((rz_pool, pool_balance));
    }

    // store emissions due for each reward zone pool
    for (rz_pool, pool_balance) in rz_balance {
        let pool_non_queued_tokens = pool_balance.non_queued_tokens();
        let share = pool_non_queued_tokens
            .fixed_div_floor(total_non_queued_tokens, SCALAR_7)
            .unwrap_optimized();

        let new_pool_emissions = share
            .fixed_mul_floor(new_emissions, SCALAR_7)
            .unwrap_optimized();
        let mut accrued_emissions = storage::get_rz_emis(e, &rz_pool);
        accrued_emissions.accrued += new_pool_emissions;
        storage::set_rz_emis(e, &rz_pool, &accrued_emissions);
    }

    return new_emissions;
}

/// Assign backstop and pool emissions to `pool` based on the reward zone and the backstop emissions index
/// Returns the amount of backstop and pool emissions assigned to the pool
#[allow(clippy::zero_prefixed_literal)]
pub fn gulp_emissions(e: &Env, pool: &Address) -> (i128, i128) {
    let pool_balance = storage::get_pool_balance(e, pool);
    let new_emissions = storage::get_rz_emis(e, pool);

    // Only allow pools to accrue once per day
    if new_emissions.last_time > e.ledger().timestamp() - 24 * 60 * 60 {
        panic_with_error!(e, BackstopError::BadRequest);
    }

    if new_emissions.accrued > 0 {
        let new_backstop_emissions = new_emissions
            .accrued
            .fixed_mul_floor(0_7000000, SCALAR_7)
            .unwrap_optimized();
        let new_pool_emissions = new_emissions
            .accrued
            .fixed_mul_floor(0_3000000, SCALAR_7)
            .unwrap_optimized();

        // distribute pool emissions via allowance to pools
        let blnd_token_client = TokenClient::new(e, &storage::get_blnd_token(e));
        let current_allowance = blnd_token_client.allowance(&e.current_contract_address(), pool);
        let new_seq = e.ledger().sequence() + storage::LEDGER_BUMP_USER; // ~120 days
        blnd_token_client.approve(
            &e.current_contract_address(),
            pool,
            &(current_allowance + new_pool_emissions),
            &new_seq,
        );
        storage::set_rz_emis(
            e,
            pool,
            &RzEmissions {
                accrued: 0,
                last_time: e.ledger().timestamp(),
            },
        );
        set_backstop_emission_eps(e, pool, &pool_balance, new_backstop_emissions);
        return (new_backstop_emissions, new_pool_emissions);
    }
    return (0, 0);
}

/// Set a new EPS for the backstop
pub fn set_backstop_emission_eps(
    e: &Env,
    pool_id: &Address,
    pool_balance: &PoolBalance,
    new_tokens: i128,
) {
    let mut tokens_left_to_emit = new_tokens;
    let expiration = e.ledger().timestamp() + 7 * 24 * 60 * 60;

    if let Some(mut emission_data) = update_emission_data(e, pool_id, &pool_balance) {
        // a previous data exists - update with old data before setting new EPS
        if emission_data.last_time != e.ledger().timestamp() {
            // force the emission data to be updated to the current timestamp
            emission_data.last_time = e.ledger().timestamp();
        }
        // determine the amount of tokens not emitted from the last config
        if emission_data.expiration > e.ledger().timestamp() {
            let time_since_last_emission = emission_data.expiration - e.ledger().timestamp();

            // Eps is scaled by 14 decimal places
            let tokens_since_last_emission = i128(emission_data.eps)
                .fixed_mul_floor(i128(time_since_last_emission), SCALAR_7)
                .unwrap_optimized();
            tokens_left_to_emit += tokens_since_last_emission;
        }
        // Scale eps by 14 decimal places to reduce rounding errors
        let eps = u64(tokens_left_to_emit * SCALAR_7 / (7 * 24 * 60 * 60)).unwrap_optimized();
        emission_data.eps = eps;
        emission_data.expiration = expiration;
        storage::set_backstop_emis_data(e, pool_id, &emission_data);
    } else {
        // first time the pool's backstop is receiving emissions - ensure data is written
        let eps = u64(tokens_left_to_emit * SCALAR_7 / (7 * 24 * 60 * 60)).unwrap_optimized();
        storage::set_backstop_emis_data(
            e,
            pool_id,
            &BackstopEmissionData {
                eps,
                expiration,
                index: 0,
                last_time: e.ledger().timestamp(),
            },
        );
    }
}
