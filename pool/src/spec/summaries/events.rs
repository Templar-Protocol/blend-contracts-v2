#![allow(unused)]
use soroban_sdk::{Address, Env, Symbol, Vec};
use crate::{AuctionData, ReserveConfig};
use crate::events::PoolEvents;

#[cfg(feature = "certora")]
impl PoolEvents {
    pub fn set_admin(e: &Env, admin: Address, new_admin: Address) {
    }

    pub fn update_pool(e: &Env, admin: Address, backstop_take_rate: u32, max_positions: u32) {
    }

    pub fn queue_set_reserve(e: &Env, admin: Address, asset: Address, metadata: ReserveConfig) {
    }

    pub fn cancel_set_reserve(e: &Env, admin: Address, asset: Address) {
    }

    pub fn set_reserve(e: &Env, asset: Address, index: u32) {
    }

    pub fn set_status(e: &Env, new_status: u32) {
    }

    pub fn set_status_admin(e: &Env, admin: Address, pool_status: u32) {
    }

    pub fn reserve_emission_update(e: &Env, res_token_id: u32, eps: u64, expiration: u64) {
    }

    pub fn gulp_emissions(e: &Env, emissions: i128) {
    }

    pub fn claim(e: &Env, from: Address, reserve_token_ids: Vec<u32>, amount_claimed: i128) {
    }

    pub fn bad_debt(e: &Env, user: Address, asset: Address, d_tokens: i128) {
    }

    pub fn defaulted_debt(e: &Env, asset: Address, d_tokens_burnt: i128) {
    }

    pub fn supply(e: &Env, asset: Address, from: Address, tokens_in: i128, b_tokens_minted: i128) {
    }

    pub fn withdraw(
        e: &Env,
        asset: Address,
        from: Address,
        tokens_out: i128,
        b_tokens_burnt: i128,
    ) {
    }

    pub fn supply_collateral(
        e: &Env,
        asset: Address,
        from: Address,
        tokens_in: i128,
        b_tokens_minted: i128,
    ) {
    }

    pub fn withdraw_collateral(
        e: &Env,
        asset: Address,
        from: Address,
        tokens_out: i128,
        b_tokens_burnt: i128,
    ) {
    }

    pub fn borrow(e: &Env, asset: Address, from: Address, tokens_out: i128, d_tokens_minted: i128) {
    }

    pub fn repay(e: &Env, asset: Address, from: Address, tokens_in: i128, d_tokens_burnt: i128) {
    }

    pub fn flash_loan(
        e: &Env,
        asset: Address,
        from: Address,
        contract: Address,
        tokens_out: i128,
        d_tokens_minted: i128,
    ) {
    }

    pub fn gulp(e: &Env, asset: Address, token_delta: i128) {
    }

    pub fn new_auction(
        e: &Env,
        auction_type: u32,
        user: Address,
        percent: u32,
        auction_data: AuctionData,
    ) {
    }

    pub fn fill_auction(
        e: &Env,
        auction_type: u32,
        user: Address,
        filler: Address,
        fill_percent: i128,
        filled_auction_data: AuctionData,
    ) {
    }

    pub fn delete_auction(e: &Env, auction_type: u32, user: Address) {
    }

    pub fn collateral_orphaned(e: &Env, user: Address, asset: Address, b_tokens: i128) {
    }

    pub fn debt_setoff(e: &Env, asset: Address, b_tokens_burned: i128, d_tokens_repaid: i128) {
    }

    pub fn orphan_settled(e: &Env, asset: Address, b_tokens: i128) {
    }
}
