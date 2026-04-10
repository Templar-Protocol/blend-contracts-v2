use crate::{
    constants::SCALAR_7,
    errors::PoolError,
    pool::{Pool, User},
    storage,
};
use cast::i128;
use soroban_fixed_point_math::SorobanFixedPoint;
use soroban_sdk::{contracttype, map, panic_with_error, Address, Env, Map, Vec};

use super::{
    backstop_interest_auction::{create_interest_auction_data, fill_interest_auction},
    bad_debt_auction::{create_bad_debt_auction_data, fill_bad_debt_auction},
    user_liquidation_auction::{create_user_liq_auction_data, fill_user_liq_auction},
};

#[derive(Clone, PartialEq)]
#[repr(u32)]
pub enum AuctionType {
    UserLiquidation = 0,
    BadDebtAuction = 1,
    InterestAuction = 2,
}

impl AuctionType {
    pub fn from_u32(e: &Env, value: u32) -> Self {
        match value {
            0 => AuctionType::UserLiquidation,
            1 => AuctionType::BadDebtAuction,
            2 => AuctionType::InterestAuction,
            _ => panic_with_error!(e, PoolError::BadRequest),
        }
    }
}

#[derive(Clone)]
#[contracttype]
pub struct AuctionData {
    /// A map of the assets being bid on and the amount being bid. These are tokens spent
    /// by the filler of the auction.
    ///
    /// The bid is different based on each auction type:
    /// - UserLiquidation: dTokens
    /// - BadDebtAuction: dTokens
    /// - InterestAuction: Underlying assets (backstop token)
    pub bid: Map<Address, i128>,
    /// A map of the assets being auctioned off and the amount being auctioned. These are tokens
    /// received by the filler of the auction.
    ///
    /// The lot is different based on each auction type:
    /// - UserLiquidation: bTokens
    /// - BadDebtAuction: Underlying assets (backstop token)
    /// - InterestAuction: Underlying assets
    pub lot: Map<Address, i128>,
    /// The block the auction begins on. This is used to determine how the auction
    /// should be scaled based on the number of blocks that have passed since the auction began.
    pub block: u32,
}

/// Create a new auction. Stores the resulting auction to the ledger to begin on the next block.
///
/// Returns the AuctionData object created
///
/// ### Arguments
/// * `auction_type` - The type of auction being created
/// * `user` - The user involved in the auction
/// * `bid` - The assets being bid on
/// * `lot` - The assets being auctioned off
/// * `percent` - The percentage of the user's positions being liquidated
///
/// ### Panics
/// * If the max positions are exceeded
/// * If the user and percent are invalid for the auction type
/// * If the auction is unable to be created
pub fn create_auction(
    e: &Env,
    auction_type: u32,
    user: &Address,
    bid: &Vec<Address>,
    lot: &Vec<Address>,
    percent: u32,
) -> AuctionData {
    require_unique_addresses(e, bid);
    require_unique_addresses(e, lot);
    // panics if auction_type parameter is not valid
    let auction_type_enum = AuctionType::from_u32(e, auction_type);
    let auction_data = match auction_type_enum {
        AuctionType::UserLiquidation => create_user_liq_auction_data(e, user, bid, lot, percent),
        AuctionType::BadDebtAuction => create_bad_debt_auction_data(e, user, bid, lot, percent),
        AuctionType::InterestAuction => create_interest_auction_data(e, user, bid, lot, percent),
    };
    storage::set_auction(e, &auction_type, user, &auction_data);
    auction_data
}

/// Delete an auction if it is stale
pub fn delete_stale_auction(e: &Env, auction_type: u32, user: &Address) {
    if !storage::has_auction(e, &auction_type, user) {
        panic_with_error!(e, PoolError::BadRequest);
    }

    let auction = storage::get_auction(e, &auction_type, user);
    // require auction is stale (older than 500 blocks)
    if auction.block + 500 > e.ledger().sequence() {
        panic_with_error!(e, PoolError::BadRequest);
    }

    storage::del_auction(e, &auction_type, user);
}

/// Delete a liquidation auction if the user being liquidated
///
/// NOTE: Does not verify if the user's positions are healthy. This must be done
/// before the contract call is completed.
///
/// ### Arguments
/// * `auction_type` - The type of auction being created
///
/// ### Panics
/// If no auction exists for the user
pub fn delete_liquidation(e: &Env, user: &Address) {
    if !storage::has_auction(e, &(AuctionType::UserLiquidation as u32), user) {
        panic_with_error!(e, PoolError::BadRequest);
    }
    storage::del_auction(e, &(AuctionType::UserLiquidation as u32), user);
}

/// Fills the auction from the invoker.
///
/// ### Arguments
/// * `pool` - The pool
/// * `auction_type` - The type of auction to fill
/// * `user` - The user involved in the auction
/// * `filler_state` - The Address filling the auction
/// * `percent_filled` - The percentage being filled as a number (i.e. 15 => 15%)
///
/// ### Panics
/// If the auction does not exist, or if the pool is unable to fulfill either side
/// of the auction quote
pub fn fill(
    e: &Env,
    pool: &mut Pool,
    auction_type: u32,
    user: &Address,
    filler_state: &mut User,
    percent_filled: u64,
) -> AuctionData {
    if user.clone() == filler_state.address {
        panic_with_error!(e, PoolError::InvalidLiquidation);
    }
    let auction_data = storage::get_auction(e, &auction_type, user);
    let (to_fill_auction, remaining_auction) = scale_auction(e, &auction_data, percent_filled);
    let is_full_fill = remaining_auction.is_none();
    match AuctionType::from_u32(e, auction_type) {
        AuctionType::UserLiquidation => {
            fill_user_liq_auction(e, pool, &to_fill_auction, user, filler_state, is_full_fill)
        }
        AuctionType::BadDebtAuction => {
            fill_bad_debt_auction(e, pool, &to_fill_auction, filler_state, is_full_fill);
        }
        AuctionType::InterestAuction => {
            fill_interest_auction(e, pool, &to_fill_auction, &filler_state.address)
        }
    };

    if let Some(auction_to_store) = remaining_auction {
        storage::set_auction(e, &auction_type, user, &auction_to_store);
    } else {
        storage::del_auction(e, &auction_type, user);
    }

    to_fill_auction
}

/// Scale the auction based on the percent being filled and the amount of blocks that have passed
/// since the auction began.
///
/// ### Arguments
/// * `auction_data` - The auction data to scale
/// * `percent_filled` - The percentage being filled as a number (i.e. 15 => 15%)
///
/// Returns the (Scaled Auction, Remaining Auction) such that:
/// - Scaled Auction is the auction data scaled
/// - Remaining Auction is the leftover auction data that will be stored in the ledger, or deleted if None
///
/// ### Panics
/// If the percent filled is greater than 100 or less than 0
#[allow(clippy::zero_prefixed_literal)]
fn scale_auction(
    e: &Env,
    auction_data: &AuctionData,
    percent_filled: u64,
) -> (AuctionData, Option<AuctionData>) {
    if percent_filled > 100 || percent_filled == 0 {
        panic_with_error!(e, PoolError::BadRequest);
    }

    let mut to_fill_auction = AuctionData {
        bid: map![e],
        lot: map![e],
        block: auction_data.block,
    };
    let mut remaining_auction = AuctionData {
        bid: map![e],
        lot: map![e],
        block: auction_data.block,
    };

    // determine block based auction modifiers
    let bid_modifier: i128;
    let lot_modifier: i128;
    let per_block_scalar: i128 = 0_0050000; // modifier moves 0.5% every block
    let block_dif = i128(e.ledger().sequence() - auction_data.block);
    if block_dif > 200 {
        // lot 100%, bid scaling down from 100% to 0%
        lot_modifier = SCALAR_7;
        if block_dif < 400 {
            bid_modifier = SCALAR_7 - (block_dif - 200) * per_block_scalar;
        } else {
            bid_modifier = 0;
        }
    } else {
        // lot scaling from 0% to 100%, bid 100%
        lot_modifier = block_dif * per_block_scalar;
        bid_modifier = SCALAR_7;
    }

    // scale the auction
    let percent_filled_i128 = i128(percent_filled) * 1_00000; // scale to decimal form in 7 decimals from percentage
    for (asset, amount) in auction_data.bid.iter() {
        // apply percent scalar and store remainder to base auction
        // round up to avoid rounding exploits
        let to_fill_base = amount.fixed_mul_ceil(e, &percent_filled_i128, &SCALAR_7);
        let remaining_base = amount - to_fill_base;
        if remaining_base > 0 {
            remaining_auction.bid.set(asset.clone(), remaining_base);
        }
        // apply block scalar to to_fill auction and don't store if 0
        let to_fill_scaled = to_fill_base.fixed_mul_ceil(e, &bid_modifier, &SCALAR_7);
        if to_fill_scaled > 0 {
            to_fill_auction.bid.set(asset, to_fill_scaled);
        }
    }
    for (asset, amount) in auction_data.lot.iter() {
        // apply percent scalar and store remainder to base auction
        // round down to avoid rounding exploits
        let to_fill_base = amount.fixed_mul_floor(e, &percent_filled_i128, &SCALAR_7);
        let remaining_base = amount - to_fill_base;
        if remaining_base > 0 {
            remaining_auction.lot.set(asset.clone(), remaining_base);
        }
        // apply block scalar to to_fill auction and don't store if 0
        let to_fill_scaled = to_fill_base.fixed_mul_floor(e, &lot_modifier, &SCALAR_7);
        if to_fill_scaled > 0 {
            to_fill_auction.lot.set(asset, to_fill_scaled);
        }
    }

    if remaining_auction.lot.is_empty() && remaining_auction.bid.is_empty() {
        (to_fill_auction, None)
    } else {
        (to_fill_auction, Some(remaining_auction))
    }
}

/// Require that all addresses in the list are unique
///
/// ### Panics
/// If any duplicate addresses are found
fn require_unique_addresses(e: &Env, list: &Vec<Address>) {
    let mut temp_map = Map::<Address, bool>::new(e);
    for address in list {
        if temp_map.contains_key(address.clone()) {
            panic_with_error!(e, PoolError::BadRequest);
        }
        temp_map.set(address.clone(), true);
    }
}
