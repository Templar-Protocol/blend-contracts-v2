use crate::{
    constants::SCALAR_7, dependencies::BackstopClient, errors::PoolError, pool::Pool, storage,
};
use cast::i128;
use sep_41_token::TokenClient;
use soroban_fixed_point_math::SorobanFixedPoint;
use soroban_sdk::{map, panic_with_error, Address, Env, Vec};

use super::{AuctionData, AuctionType};

pub fn create_interest_auction_data(
    e: &Env,
    user: &Address,
    bid: &Vec<Address>,
    lot: &Vec<Address>,
    percent: u32,
) -> AuctionData {
    let backstop = storage::get_backstop(e);
    if user != &backstop {
        panic_with_error!(e, PoolError::BadRequest);
    }
    if percent != 100 {
        panic_with_error!(e, PoolError::BadRequest);
    }
    if storage::has_auction(e, &(AuctionType::InterestAuction as u32), &backstop) {
        panic_with_error!(e, PoolError::AuctionInProgress);
    }

    let mut pool = Pool::load(e);
    // bid is required to have 1 entry, so require lot to have less than max_positions entries
    if pool.config.max_positions <= lot.len() {
        panic_with_error!(e, PoolError::MaxPositionsExceeded);
    }
    let oracle_scalar = 10i128.pow(pool.load_price_decimals(e));
    let mut auction_data = AuctionData {
        lot: map![e],
        bid: map![e],
        block: e.ledger().sequence() + 1,
    };

    // validate and create lot auction data
    let mut interest_value = 0; // expressed in the oracle's decimals
    for lot_asset in lot {
        // don't store updated reserve data back to ledger. This will occur on the the auction's fill.
        // `load_reserve` will panic if the reserve does not exist
        let reserve = pool.load_reserve(e, &lot_asset, false);
        if reserve.data.backstop_credit > 0 {
            let asset_to_base = pool.load_price(e, &reserve.asset);
            interest_value += i128(asset_to_base).fixed_mul_floor(
                e,
                &reserve.data.backstop_credit,
                &reserve.scalar,
            );
            auction_data
                .lot
                .set(reserve.asset, reserve.data.backstop_credit);
        }
    }

    if auction_data.lot.is_empty() {
        panic_with_error!(e, PoolError::InvalidLot);
    }

    // Ensure that the interest value is at least 200 USDC
    if interest_value < 200 * oracle_scalar {
        panic_with_error!(e, PoolError::InterestTooSmall);
    }

    // validate and create bid auction data
    let backstop_client = BackstopClient::new(e, &backstop);
    let backstop_token = backstop_client.backstop_token();
    if bid.len() != 1 || bid.get_unchecked(0) != backstop_token {
        panic_with_error!(e, PoolError::InvalidBid);
    }

    let pool_backstop_data = backstop_client.pool_data(&e.current_contract_address());
    // backstop tokens use 7 decimals
    let bid_amount = interest_value // oracle_scalar
        .fixed_mul_floor(e, &1_2000000, &oracle_scalar) // denom of oracle_scalar means result is SCALAR_7
        .fixed_div_floor(e, &pool_backstop_data.token_spot_price, &SCALAR_7); // token_spot_price is SCALAR_7
    auction_data.bid.set(backstop_token, bid_amount);

    auction_data
}

pub fn fill_interest_auction(
    e: &Env,
    pool: &mut Pool,
    auction_data: &AuctionData,
    filler: &Address,
) {
    // bid only contains the Backstop token
    let backstop = storage::get_backstop(e);
    if filler.clone() == backstop {
        panic_with_error!(e, PoolError::BadRequest);
    }
    let backstop_client = BackstopClient::new(&e, &backstop);
    let backstop_token: Address = backstop_client.backstop_token();
    let backstop_token_bid_amount = auction_data.bid.get(backstop_token).unwrap_or(0);
    if backstop_token_bid_amount > 0 {
        backstop_client.donate(
            &filler,
            &e.current_contract_address(),
            &backstop_token_bid_amount,
        );
    }

    // lot contains underlying tokens, but the backstop credit must be updated on the reserve
    for (res_asset_address, lot_amount) in auction_data.lot.iter() {
        let mut reserve = pool.load_reserve(e, &res_asset_address, true);
        reserve.data.backstop_credit -= lot_amount;
        pool.cache_reserve(reserve);
        TokenClient::new(e, &res_asset_address).transfer(
            &e.current_contract_address(),
            filler,
            &lot_amount,
        );
    }
}
