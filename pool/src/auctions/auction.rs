use crate::{
    constants::SCALAR_7,
    errors::PoolError,
    math::FixedMath,
    pool::{Pool, User},
    storage,
};
use cast::i128;
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
    // panics if auction_type parameter is not valid
    let auction_type_enum = AuctionType::from_u32(e, auction_type);
    if auction_type_enum != AuctionType::UserLiquidation {
        panic_with_error!(e, PoolError::BadRequest);
    }
    require_unique_addresses(e, bid);
    require_unique_addresses(e, lot);
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
    let auction_type_enum = AuctionType::from_u32(e, auction_type);
    if auction_type_enum != AuctionType::UserLiquidation {
        panic_with_error!(e, PoolError::BadRequest);
    }
    if user.clone() == filler_state.address {
        panic_with_error!(e, PoolError::InvalidLiquidation);
    }
    let auction_data = storage::get_auction(e, &auction_type, user);
    let (to_fill_auction, remaining_auction) = scale_auction(e, &auction_data, percent_filled);
    let is_full_fill = remaining_auction.is_none();
    match auction_type_enum {
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
    let block_dif = i128(e.ledger().sequence() - auction_data.block);
    let (bid_modifier, lot_modifier) = auction_timing_modifiers(block_dif);

    // scale the auction
    let percent_filled_i128 = i128(percent_filled) * 1_00000; // scale to decimal form in 7 decimals from percentage
    for (asset, amount) in auction_data.bid.iter() {
        // apply percent scalar and store remainder to base auction
        // round up to avoid rounding exploits
        let (to_fill_base, remaining_base) = partition_quote(e, true, amount, percent_filled_i128);
        if remaining_base > 0 {
            remaining_auction.bid.set(asset.clone(), remaining_base);
        }
        // apply block scalar to to_fill auction and don't store if 0
        let to_fill_scaled = discount_quote(e, true, to_fill_base, bid_modifier);
        if to_fill_scaled > 0 {
            to_fill_auction.bid.set(asset, to_fill_scaled);
        }
    }
    for (asset, amount) in auction_data.lot.iter() {
        // apply percent scalar and store remainder to base auction
        // round down to avoid rounding exploits
        let (to_fill_base, remaining_base) = partition_quote(e, false, amount, percent_filled_i128);
        if remaining_base > 0 {
            remaining_auction.lot.set(asset.clone(), remaining_base);
        }
        // apply block scalar to to_fill auction and don't store if 0
        let to_fill_scaled = discount_quote(e, false, to_fill_base, lot_modifier);
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

/// Returns `(bid_modifier, lot_modifier)` in SCALAR_7 scale:
/// - lot scales linearly from 0% to 100% over the first 200 blocks
/// - bid stays at 100% for the first 200 blocks, then scales linearly down to
///   0% over the next 200 blocks
#[allow(clippy::zero_prefixed_literal)]
fn auction_timing_modifiers(block_dif: i128) -> (i128, i128) {
    let per_block_scalar: i128 = 0_0050000; // modifier moves 0.5% every block
    if block_dif > 200 {
        // lot 100%, bid scaling down from 100% to 0%
        let lot_modifier = SCALAR_7;
        if block_dif < 400 {
            let bid_modifier = SCALAR_7 - (block_dif - 200) * per_block_scalar;
            (bid_modifier, lot_modifier)
        } else {
            (0, lot_modifier)
        }
    } else {
        // lot scaling from 0% to 100%, bid 100%
        let lot_modifier = block_dif * per_block_scalar;
        (SCALAR_7, lot_modifier)
    }
}

/// Split one auction quote entry by the fill percentage into
/// `(to_fill_base, remaining_base)`. The kernel owns the rounding direction:
/// bids round up (conservative for the filler), lots round down.
fn partition_quote(
    math: &impl FixedMath,
    round_up: bool,
    amount: i128,
    percent_filled: i128,
) -> (i128, i128) {
    let to_fill_base = if round_up {
        math.ceil(amount, percent_filled, SCALAR_7)
    } else {
        math.floor(amount, percent_filled, SCALAR_7)
    };
    (to_fill_base, amount - to_fill_base)
}

/// Apply the block modifier to an already-partitioned fill amount. The kernel
/// owns the rounding direction, matching the partition stage.
fn discount_quote(
    math: &impl FixedMath,
    round_up: bool,
    to_fill_base: i128,
    modifier: i128,
) -> i128 {
    if round_up {
        math.ceil(to_fill_base, modifier, SCALAR_7)
    } else {
        math.floor(to_fill_base, modifier, SCALAR_7)
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

#[cfg(kani)]
mod verification {
    use super::*;

    /// Block timing modifiers stay in [0, SCALAR_7]: the lot never decreases
    /// and the bid never increases as blocks elapse, with the exact phase and
    /// maturity boundaries at 0/199/200/201/399/400 blocks and at u32::MAX.
    ///
    /// Elapsed counts cover the full u32 ledger-sequence difference production
    /// can reach; `earlier <= later` chronology is a theorem input, not proved.
    #[kani::proof]
    fn prove_auction_timing_modifiers() {
        let earlier: u32 = kani::any();
        let later: u32 = kani::any();
        kani::assume(earlier <= later);
        let (bid_early, lot_early) = auction_timing_modifiers(i128::from(earlier));
        let (bid_late, lot_late) = auction_timing_modifiers(i128::from(later));

        assert!(bid_early >= 0 && bid_early <= SCALAR_7);
        assert!(lot_early >= 0 && lot_early <= SCALAR_7);
        assert!(bid_late >= 0 && bid_late <= SCALAR_7);
        assert!(lot_late >= 0 && lot_late <= SCALAR_7);

        assert!(lot_late >= lot_early);
        assert!(bid_late <= bid_early);

        // phase and maturity boundaries
        assert_eq!(auction_timing_modifiers(0), (SCALAR_7, 0));
        assert_eq!(auction_timing_modifiers(199), (SCALAR_7, 9_950_000));
        assert_eq!(auction_timing_modifiers(200), (SCALAR_7, SCALAR_7));
        assert_eq!(auction_timing_modifiers(201), (9_950_000, SCALAR_7));
        assert_eq!(auction_timing_modifiers(399), (50_000, SCALAR_7));
        assert_eq!(auction_timing_modifiers(400), (0, SCALAR_7));
        assert_eq!(
            auction_timing_modifiers(i128::from(u32::MAX)),
            (0, SCALAR_7)
        );
    }

    /// Undiscounted quote partition: fill plus remainder equals the original
    /// amount in both rounding directions, with the exact scaled rounding
    /// inequalities and the 100% identity. The kernel's own bid/lot direction
    /// selection is exercised; the block-discounted amounts are separate
    /// quantities proved in `prove_auction_discount_bounds`.
    ///
    /// Domain (deliberate solver restriction, NOT a protocol limit): amounts
    /// 0..=255 (u8) and fill percent 1..=100 as enforced by scale_auction.
    /// Largest product 255 * 10_000_000 fits i128, so the dependency fast
    /// path is exercised.
    #[kani::proof]
    fn prove_auction_quote_partition() {
        let amount_u8: u8 = kani::any();
        let percent_u8: u8 = kani::any();
        kani::assume(percent_u8 >= 1 && percent_u8 <= 100);
        let amount = i128::from(amount_u8);
        let percent_scalar = i128::from(percent_u8) * 1_00000; // same scaling as scale_auction

        let (bid_fill, bid_remain) = partition_quote(&(), true, amount, percent_scalar);
        let (lot_fill, lot_remain) = partition_quote(&(), false, amount, percent_scalar);

        assert_eq!(bid_fill + bid_remain, amount);
        assert_eq!(lot_fill + lot_remain, amount);
        assert!(bid_fill >= 0 && bid_fill <= amount);
        assert!(lot_fill >= 0 && lot_fill <= amount);
        assert!(lot_fill <= bid_fill);

        assert!(lot_fill * SCALAR_7 <= amount * percent_scalar);
        assert!(amount * percent_scalar - lot_fill * SCALAR_7 < SCALAR_7);
        assert!(bid_fill * SCALAR_7 >= amount * percent_scalar);
        assert!(bid_fill * SCALAR_7 - amount * percent_scalar < SCALAR_7);

        if percent_u8 == 100 {
            assert_eq!(bid_fill, amount);
            assert_eq!(lot_fill, amount);
        }

        // non-vacuity witnesses: 1 unit at 1% fills as a bid (rounds up) but
        // not as a lot (rounds down)
        assert_eq!(partition_quote(&(), true, 1, 1_00000), (1, 0));
        assert_eq!(partition_quote(&(), false, 1, 1_00000).0, 0);
    }

    /// Time-discounted settlement bounds, separate from the partition above:
    /// the discounted amount never exceeds the undiscounted fill and never
    /// goes negative, in both rounding directions, with the modifier
    /// boundary identities (100% modifier is exact, 0% modifier fills nothing).
    ///
    /// Domain (deliberate solver restriction, NOT a protocol limit): amounts
    /// 0..=255 (u8), fill percent 1..=100, block modifier in [0, SCALAR_7].
    #[kani::proof]
    fn prove_auction_discount_bounds() {
        let amount_u8: u8 = kani::any();
        let percent_u8: u8 = kani::any();
        let modifier: i128 = kani::any();
        kani::assume(percent_u8 >= 1 && percent_u8 <= 100);
        kani::assume(modifier >= 0 && modifier <= SCALAR_7);
        let amount = i128::from(amount_u8);
        let percent_scalar = i128::from(percent_u8) * 1_00000;

        // compose the actual production kernels: partition first, then discount
        let (bid_fill, _) = partition_quote(&(), true, amount, percent_scalar);
        let (lot_fill, _) = partition_quote(&(), false, amount, percent_scalar);
        let bid_scaled = discount_quote(&(), true, bid_fill, modifier);
        let lot_scaled = discount_quote(&(), false, lot_fill, modifier);

        assert!(bid_scaled >= 0 && bid_scaled <= bid_fill);
        assert!(lot_scaled >= 0 && lot_scaled <= lot_fill);

        if modifier == SCALAR_7 {
            assert_eq!(bid_scaled, bid_fill);
            assert_eq!(lot_scaled, lot_fill);
        }
        if modifier == 0 {
            assert_eq!(bid_scaled, 0);
            assert_eq!(lot_scaled, 0);
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        pool::Positions,
        storage::PoolConfig,
        testutils::{self, create_comet_lp_pool, create_pool},
    };

    use super::*;
    use sep_40_oracle::testutils::Asset;
    use soroban_sdk::{
        map,
        testutils::{Address as _, Ledger, LedgerInfo},
        unwrap::UnwrapOptimized,
        vec, Symbol,
    };

    #[test]
    fn test_create_liquidation() {
        let e = Env::default();

        e.mock_all_auths();
        e.ledger().set(LedgerInfo {
            timestamp: 12345,
            protocol_version: 22,
            sequence_number: 50,
            network_id: Default::default(),
            base_reserve: 10,
            min_temp_entry_ttl: 10,
            min_persistent_entry_ttl: 10,
            max_entry_ttl: 3110400,
        });

        let bombadil = Address::generate(&e);
        let samwise = Address::generate(&e);

        let pool_address = create_pool(&e);
        let (oracle_address, oracle_client) = testutils::create_mock_oracle(&e);

        // creating reserves for a pool exhausts the budget
        e.cost_estimate().budget().reset_unlimited();
        let (underlying_0, _) = testutils::create_token_contract(&e, &bombadil);
        let (mut reserve_config_0, mut reserve_data_0) = testutils::default_reserve_meta();
        reserve_data_0.last_time = 12345;
        reserve_data_0.b_rate = 1_100_000_000_000;
        reserve_config_0.c_factor = 0_8500000;
        reserve_config_0.l_factor = 0_9000000;
        reserve_config_0.index = 0;
        testutils::create_reserve(
            &e,
            &pool_address,
            &underlying_0,
            &reserve_config_0,
            &reserve_data_0,
        );

        let (underlying_1, _) = testutils::create_token_contract(&e, &bombadil);
        let (mut reserve_config_1, mut reserve_data_1) = testutils::default_reserve_meta();
        reserve_data_1.b_rate = 1_200_000_000_000;
        reserve_config_1.c_factor = 0_7500000;
        reserve_config_1.l_factor = 0_7500000;
        reserve_data_1.last_time = 12345;
        reserve_config_1.index = 1;
        testutils::create_reserve(
            &e,
            &pool_address,
            &underlying_1,
            &reserve_config_1,
            &reserve_data_1,
        );

        let (underlying_2, _) = testutils::create_token_contract(&e, &bombadil);
        let (mut reserve_config_2, reserve_data_2) = testutils::default_reserve_meta();
        reserve_config_2.c_factor = 0_0000000;
        reserve_config_2.l_factor = 0_7000000;
        reserve_config_2.index = 2;
        testutils::create_reserve(
            &e,
            &pool_address,
            &underlying_2,
            &reserve_config_2,
            &reserve_data_2,
        );

        oracle_client.set_data(
            &bombadil,
            &Asset::Other(Symbol::new(&e, "USD")),
            &vec![
                &e,
                Asset::Stellar(underlying_0.clone()),
                Asset::Stellar(underlying_1.clone()),
                Asset::Stellar(underlying_2.clone()),
            ],
            &7,
            &300,
        );
        oracle_client.set_price_stable(&vec![&e, 2_0000000, 4_0000000, 50_0000000]);

        let liq_pct = 45;
        let positions: Positions = Positions {
            collateral: map![
                &e,
                (reserve_config_0.index, 90_9100000),
                (reserve_config_1.index, 04_5800000),
            ],
            liabilities: map![&e, (reserve_config_2.index, 02_7500000),],
            supply: map![&e],
        };
        let pool_config = PoolConfig {
            oracle: oracle_address,
            min_collateral: 1_0000000,
            bstop_rate: 0_1000000,
            status: 0,
            max_positions: 4,
        };
        e.as_contract(&pool_address, || {
            storage::set_backstop(&e, &Address::generate(&e));
            storage::set_user_positions(&e, &samwise, &positions);
            storage::set_pool_config(&e, &pool_config);

            e.cost_estimate().budget().reset_unlimited();
            create_auction(
                &e,
                0,
                &samwise,
                &vec![&e, underlying_2],
                &vec![&e, underlying_0, underlying_1],
                liq_pct,
            );
            assert!(storage::has_auction(&e, &0, &samwise));
        });
    }

    #[test]
    #[should_panic(expected = "Error(Contract, #1211)")]
    fn test_create_liquidation_for_pool() {
        let e = Env::default();
        e.cost_estimate().budget().reset_unlimited();
        e.mock_all_auths();
        e.ledger().set(LedgerInfo {
            timestamp: 12345,
            protocol_version: 22,
            sequence_number: 50,
            network_id: Default::default(),
            base_reserve: 10,
            min_temp_entry_ttl: 10,
            min_persistent_entry_ttl: 10,
            max_entry_ttl: 3110400,
        });

        let bombadil = Address::generate(&e);

        let pool_address = create_pool(&e);
        let (oracle_address, oracle_client) = testutils::create_mock_oracle(&e);

        let (underlying_0, _) = testutils::create_token_contract(&e, &bombadil);
        let (mut reserve_config_0, mut reserve_data_0) = testutils::default_reserve_meta();
        reserve_data_0.last_time = 12345;
        reserve_data_0.b_rate = 1_100_000_000_000;
        reserve_config_0.c_factor = 0_8500000;
        reserve_config_0.l_factor = 0_9000000;
        reserve_config_0.index = 0;
        testutils::create_reserve(
            &e,
            &pool_address,
            &underlying_0,
            &reserve_config_0,
            &reserve_data_0,
        );

        let (underlying_1, _) = testutils::create_token_contract(&e, &bombadil);
        let (mut reserve_config_1, mut reserve_data_1) = testutils::default_reserve_meta();
        reserve_data_1.b_rate = 1_200_000_000_000;
        reserve_config_1.c_factor = 0_7500000;
        reserve_config_1.l_factor = 0_7500000;
        reserve_data_1.last_time = 12345;
        reserve_config_1.index = 1;
        testutils::create_reserve(
            &e,
            &pool_address,
            &underlying_1,
            &reserve_config_1,
            &reserve_data_1,
        );

        let (underlying_2, _) = testutils::create_token_contract(&e, &bombadil);
        let (mut reserve_config_2, reserve_data_2) = testutils::default_reserve_meta();
        reserve_config_2.c_factor = 0_0000000;
        reserve_config_2.l_factor = 0_7000000;
        reserve_config_2.index = 2;
        testutils::create_reserve(
            &e,
            &pool_address,
            &underlying_2,
            &reserve_config_2,
            &reserve_data_2,
        );

        oracle_client.set_data(
            &bombadil,
            &Asset::Other(Symbol::new(&e, "USD")),
            &vec![
                &e,
                Asset::Stellar(underlying_0.clone()),
                Asset::Stellar(underlying_1.clone()),
                Asset::Stellar(underlying_2.clone()),
            ],
            &7,
            &300,
        );
        oracle_client.set_price_stable(&vec![&e, 2_0000000, 4_0000000, 50_0000000]);

        let liq_pct = 45;
        let positions: Positions = Positions {
            collateral: map![
                &e,
                (reserve_config_0.index, 90_9100000),
                (reserve_config_1.index, 04_5800000),
            ],
            liabilities: map![&e, (reserve_config_2.index, 02_7500000),],
            supply: map![&e],
        };
        let pool_config = PoolConfig {
            oracle: oracle_address,
            min_collateral: 1_0000000,
            bstop_rate: 0_1000000,
            status: 0,
            max_positions: 4,
        };
        e.as_contract(&pool_address, || {
            storage::set_backstop(&e, &Address::generate(&e));
            storage::set_user_positions(&e, &pool_address, &positions);
            storage::set_pool_config(&e, &pool_config);

            create_auction(
                &e,
                0,
                &pool_address,
                &vec![&e, underlying_2],
                &vec![&e, underlying_0, underlying_1],
                liq_pct,
            );
        });
    }

    #[test]
    #[should_panic(expected = "Error(Contract, #1211)")]
    fn test_create_liquidation_for_backstop() {
        let e = Env::default();
        e.cost_estimate().budget().reset_unlimited();
        e.mock_all_auths();
        e.ledger().set(LedgerInfo {
            timestamp: 12345,
            protocol_version: 22,
            sequence_number: 50,
            network_id: Default::default(),
            base_reserve: 10,
            min_temp_entry_ttl: 10,
            min_persistent_entry_ttl: 10,
            max_entry_ttl: 3110400,
        });

        let bombadil = Address::generate(&e);

        let pool_address = create_pool(&e);
        let backstop = Address::generate(&e);
        let (oracle_address, oracle_client) = testutils::create_mock_oracle(&e);

        let (underlying_0, _) = testutils::create_token_contract(&e, &bombadil);
        let (mut reserve_config_0, mut reserve_data_0) = testutils::default_reserve_meta();
        reserve_data_0.last_time = 12345;
        reserve_data_0.b_rate = 1_100_000_000_000;
        reserve_config_0.c_factor = 0_8500000;
        reserve_config_0.l_factor = 0_9000000;
        reserve_config_0.index = 0;
        testutils::create_reserve(
            &e,
            &pool_address,
            &underlying_0,
            &reserve_config_0,
            &reserve_data_0,
        );

        let (underlying_1, _) = testutils::create_token_contract(&e, &bombadil);
        let (mut reserve_config_1, mut reserve_data_1) = testutils::default_reserve_meta();
        reserve_data_1.b_rate = 1_200_000_000_000;
        reserve_config_1.c_factor = 0_7500000;
        reserve_config_1.l_factor = 0_7500000;
        reserve_data_1.last_time = 12345;
        reserve_config_1.index = 1;
        testutils::create_reserve(
            &e,
            &pool_address,
            &underlying_1,
            &reserve_config_1,
            &reserve_data_1,
        );

        let (underlying_2, _) = testutils::create_token_contract(&e, &bombadil);
        let (mut reserve_config_2, reserve_data_2) = testutils::default_reserve_meta();
        reserve_config_2.c_factor = 0_0000000;
        reserve_config_2.l_factor = 0_7000000;
        reserve_config_2.index = 2;
        testutils::create_reserve(
            &e,
            &pool_address,
            &underlying_2,
            &reserve_config_2,
            &reserve_data_2,
        );

        oracle_client.set_data(
            &bombadil,
            &Asset::Other(Symbol::new(&e, "USD")),
            &vec![
                &e,
                Asset::Stellar(underlying_0.clone()),
                Asset::Stellar(underlying_1.clone()),
                Asset::Stellar(underlying_2.clone()),
            ],
            &7,
            &300,
        );
        oracle_client.set_price_stable(&vec![&e, 2_0000000, 4_0000000, 50_0000000]);

        let liq_pct = 45;
        let positions: Positions = Positions {
            collateral: map![
                &e,
                (reserve_config_0.index, 90_9100000),
                (reserve_config_1.index, 04_5800000),
            ],
            liabilities: map![&e, (reserve_config_2.index, 02_7500000),],
            supply: map![&e],
        };
        let pool_config = PoolConfig {
            oracle: oracle_address,
            min_collateral: 1_0000000,
            bstop_rate: 0_1000000,
            status: 0,
            max_positions: 4,
        };
        e.as_contract(&pool_address, || {
            storage::set_backstop(&e, &backstop);
            storage::set_user_positions(&e, &backstop, &positions);
            storage::set_pool_config(&e, &pool_config);

            create_auction(
                &e,
                0,
                &backstop,
                &vec![&e, underlying_2],
                &vec![&e, underlying_0, underlying_1],
                liq_pct,
            );
        });
    }

    #[test]
    #[should_panic(expected = "Error(Contract, #1200)")]
    fn test_create_auction_invalid_type() {
        let e = Env::default();
        e.mock_all_auths();
        e.cost_estimate().budget().reset_unlimited(); // setup exhausts budget

        e.ledger().set(LedgerInfo {
            timestamp: 12345,
            protocol_version: 22,
            sequence_number: 50,
            network_id: Default::default(),
            base_reserve: 10,
            min_temp_entry_ttl: 10,
            min_persistent_entry_ttl: 10,
            max_entry_ttl: 3110400,
        });

        let bombadil = Address::generate(&e);

        let pool_address = create_pool(&e);
        let (usdc_id, _) = testutils::create_token_contract(&e, &bombadil);
        let (blnd_id, _) = testutils::create_blnd_token(&e, &pool_address, &bombadil);

        let (backstop_token_id, _) = create_comet_lp_pool(&e, &bombadil, &blnd_id, &usdc_id);
        let (backstop_address, backstop_client) =
            testutils::create_backstop(&e, &pool_address, &backstop_token_id, &usdc_id, &blnd_id);
        backstop_client.deposit(&bombadil, &pool_address, &(50 * SCALAR_7));
        let (oracle_id, oracle_client) = testutils::create_mock_oracle(&e);

        let (underlying_0, _) = testutils::create_token_contract(&e, &bombadil);
        let (mut reserve_config_0, mut reserve_data_0) = testutils::default_reserve_meta();
        reserve_data_0.last_time = 12345;
        reserve_data_0.backstop_credit = 200_0000000;
        reserve_data_0.b_supply = 1000_0000000;
        reserve_data_0.d_supply = 750_0000000;
        reserve_config_0.index = 0;
        testutils::create_reserve(
            &e,
            &pool_address,
            &underlying_0,
            &reserve_config_0,
            &reserve_data_0,
        );

        oracle_client.set_data(
            &bombadil,
            &Asset::Other(Symbol::new(&e, "USD")),
            &vec![
                &e,
                Asset::Stellar(underlying_0.clone()),
                Asset::Stellar(usdc_id),
            ],
            &7,
            &300,
        );
        oracle_client.set_price_stable(&vec![&e, 2_0000000, 1_0000000]);

        let pool_config = PoolConfig {
            oracle: oracle_id,
            min_collateral: 1_0000000,
            bstop_rate: 0_1000000,
            status: 0,
            max_positions: 4,
        };
        e.as_contract(&pool_address, || {
            storage::set_pool_config(&e, &pool_config);

            create_auction(
                &e,
                3,
                &backstop_address,
                &vec![&e, backstop_token_id],
                &vec![&e, underlying_0],
                100,
            );
        });
    }

    #[test]
    #[should_panic(expected = "Error(Contract, #1200)")]
    fn test_create_auction_duplicate_bid() {
        let e = Env::default();
        e.mock_all_auths_allowing_non_root_auth();
        e.cost_estimate().budget().reset_unlimited(); // setup exhausts budget

        e.ledger().set(LedgerInfo {
            timestamp: 12345,
            protocol_version: 22,
            sequence_number: 50,
            network_id: Default::default(),
            base_reserve: 10,
            min_temp_entry_ttl: 10,
            min_persistent_entry_ttl: 10,
            max_entry_ttl: 3110400,
        });

        let bombadil = Address::generate(&e);
        let samwise = Address::generate(&e);
        let pool_address = create_pool(&e);

        let (blnd, blnd_client) = testutils::create_blnd_token(&e, &pool_address, &bombadil);
        let (usdc, usdc_client) = testutils::create_token_contract(&e, &bombadil);
        let (lp_token, lp_token_client) =
            testutils::create_comet_lp_pool(&e, &bombadil, &blnd, &usdc);
        let (backstop_address, backstop_client) =
            testutils::create_backstop(&e, &pool_address, &lp_token, &usdc, &blnd);
        // mint lp tokens
        blnd_client.mint(&samwise, &500_001_0000000);
        blnd_client.approve(&samwise, &lp_token, &i128::MAX, &99999);
        usdc_client.mint(&samwise, &12_501_0000000);
        usdc_client.approve(&samwise, &lp_token, &i128::MAX, &99999);
        lp_token_client.join_pool(
            &50_000_0000000,
            &vec![&e, 500_001_0000000, 12_501_0000000],
            &samwise,
        );
        backstop_client.deposit(&samwise, &pool_address, &50_000_0000000);

        let (oracle_id, oracle_client) = testutils::create_mock_oracle(&e);

        let (underlying_0, _) = testutils::create_token_contract(&e, &bombadil);
        let (mut reserve_config_0, mut reserve_data_0) = testutils::default_reserve_meta();
        reserve_data_0.d_rate = 1_100_000_000_000;
        reserve_data_0.last_time = 12345;
        reserve_config_0.index = 0;
        testutils::create_reserve(
            &e,
            &pool_address,
            &underlying_0,
            &reserve_config_0,
            &reserve_data_0,
        );

        let (underlying_1, _) = testutils::create_token_contract(&e, &bombadil);
        let (mut reserve_config_1, mut reserve_data_1) = testutils::default_reserve_meta();
        reserve_data_1.d_rate = 1_200_000_000_000;
        reserve_data_1.last_time = 12345;
        reserve_config_1.index = 1;
        testutils::create_reserve(
            &e,
            &pool_address,
            &underlying_1,
            &reserve_config_1,
            &reserve_data_1,
        );

        let (underlying_2, _) = testutils::create_token_contract(&e, &bombadil);
        let (mut reserve_config_2, mut reserve_data_2) = testutils::default_reserve_meta();
        reserve_data_2.b_rate = 1_100_000_000_000;
        reserve_data_2.last_time = 12345;
        reserve_config_2.index = 1;
        testutils::create_reserve(
            &e,
            &pool_address,
            &underlying_2,
            &reserve_config_2,
            &reserve_data_2,
        );
        oracle_client.set_data(
            &bombadil,
            &Asset::Other(Symbol::new(&e, "USD1")),
            &vec![
                &e,
                Asset::Stellar(underlying_0.clone()),
                Asset::Stellar(underlying_1.clone()),
                Asset::Stellar(underlying_2.clone()),
                Asset::Stellar(usdc),
                Asset::Stellar(blnd),
            ],
            &7,
            &300,
        );
        oracle_client.set_price_stable(&vec![
            &e,
            2_0000000,
            4_0000000,
            100_0000000,
            1_0000000,
            0_1000000,
        ]);

        let positions: Positions = Positions {
            collateral: map![&e],
            liabilities: map![
                &e,
                (reserve_config_0.index, 10_0000000),
                (reserve_config_1.index, 2_5000000)
            ],
            supply: map![&e],
        };

        let pool_config = PoolConfig {
            oracle: oracle_id,
            min_collateral: 1_0000000,
            bstop_rate: 0_1000000,
            status: 0,
            max_positions: 4,
        };
        e.as_contract(&pool_address, || {
            storage::set_pool_config(&e, &pool_config);
            storage::set_user_positions(&e, &backstop_address, &positions);

            create_auction(
                &e,
                1,
                &backstop_address,
                &vec![&e, underlying_0.clone(), underlying_1, underlying_0],
                &vec![&e, lp_token],
                100,
            );
        });
    }

    #[test]
    #[should_panic(expected = "Error(Contract, #1200)")]
    fn test_create_auction_duplicate_lot() {
        let e = Env::default();
        e.mock_all_auths();
        e.cost_estimate().budget().reset_unlimited(); // setup exhausts budget

        e.ledger().set(LedgerInfo {
            timestamp: 12345,
            protocol_version: 22,
            sequence_number: 50,
            network_id: Default::default(),
            base_reserve: 10,
            min_temp_entry_ttl: 10,
            min_persistent_entry_ttl: 10,
            max_entry_ttl: 3110400,
        });

        let bombadil = Address::generate(&e);

        let pool_address = create_pool(&e);
        let (usdc_id, _) = testutils::create_token_contract(&e, &bombadil);
        let (blnd_id, _) = testutils::create_blnd_token(&e, &pool_address, &bombadil);

        let (backstop_token_id, _) = create_comet_lp_pool(&e, &bombadil, &blnd_id, &usdc_id);
        let (backstop_address, backstop_client) =
            testutils::create_backstop(&e, &pool_address, &backstop_token_id, &usdc_id, &blnd_id);
        backstop_client.deposit(&bombadil, &pool_address, &(50 * SCALAR_7));
        let (oracle_id, oracle_client) = testutils::create_mock_oracle(&e);

        let (underlying_0, _) = testutils::create_token_contract(&e, &bombadil);
        let (mut reserve_config_0, mut reserve_data_0) = testutils::default_reserve_meta();
        reserve_data_0.last_time = 12345;
        reserve_data_0.backstop_credit = 100_0000000;
        reserve_data_0.b_supply = 1000_0000000;
        reserve_data_0.d_supply = 750_0000000;
        reserve_config_0.index = 0;
        testutils::create_reserve(
            &e,
            &pool_address,
            &underlying_0,
            &reserve_config_0,
            &reserve_data_0,
        );

        let (underlying_1, _) = testutils::create_token_contract(&e, &bombadil);
        let (mut reserve_config_1, mut reserve_data_1) = testutils::default_reserve_meta();
        reserve_data_1.last_time = 12345;
        reserve_data_1.backstop_credit = 25_0000000;
        reserve_data_1.b_supply = 250_0000000;
        reserve_data_1.d_supply = 187_5000000;
        reserve_config_1.index = 1;
        testutils::create_reserve(
            &e,
            &pool_address,
            &underlying_1,
            &reserve_config_1,
            &reserve_data_1,
        );

        let (underlying_2, _) = testutils::create_token_contract(&e, &bombadil);
        let (mut reserve_config_2, mut reserve_data_2) = testutils::default_reserve_meta();
        reserve_data_2.b_rate = 1_100_000_000_000;
        reserve_data_2.last_time = 12345;
        reserve_config_2.index = 1;
        testutils::create_reserve(
            &e,
            &pool_address,
            &underlying_2,
            &reserve_config_2,
            &reserve_data_2,
        );

        oracle_client.set_data(
            &bombadil,
            &Asset::Other(Symbol::new(&e, "USD")),
            &vec![
                &e,
                Asset::Stellar(underlying_0.clone()),
                Asset::Stellar(underlying_1.clone()),
                Asset::Stellar(underlying_2),
                Asset::Stellar(usdc_id),
            ],
            &7,
            &300,
        );
        oracle_client.set_price_stable(&vec![&e, 2_0000000, 4_0000000, 100_0000000, 1_0000000]);

        let pool_config = PoolConfig {
            oracle: oracle_id,
            min_collateral: 1_0000000,
            bstop_rate: 0_1000000,
            status: 0,
            max_positions: 4,
        };
        e.as_contract(&pool_address, || {
            storage::set_pool_config(&e, &pool_config);

            create_auction(
                &e,
                2,
                &backstop_address,
                &vec![&e, backstop_token_id],
                &vec![&e, underlying_0.clone(), underlying_1, underlying_0],
                100,
            );
        });
    }

    #[test]
    fn test_delete_user_liquidation() {
        let e = Env::default();
        e.mock_all_auths();

        let pool_id = create_pool(&e);
        let samwise = Address::generate(&e);

        let auction_data = AuctionData {
            bid: map![&e],
            lot: map![&e],
            block: 100,
        };
        e.as_contract(&pool_id, || {
            storage::set_auction(
                &e,
                &(AuctionType::UserLiquidation as u32),
                &samwise,
                &auction_data,
            );

            delete_liquidation(&e, &samwise);
            assert!(!storage::has_auction(
                &e,
                &(AuctionType::UserLiquidation as u32),
                &samwise
            ));
        });
    }

    #[test]
    #[should_panic(expected = "Error(Contract, #1200)")]
    fn test_delete_user_liquidation_does_not_exist() {
        let e = Env::default();
        e.mock_all_auths();
        let pool_id = create_pool(&e);

        let samwise = Address::generate(&e);

        e.as_contract(&pool_id, || {
            delete_liquidation(&e, &samwise);
        });
    }

    #[test]
    fn test_fill() {
        let e = Env::default();

        e.mock_all_auths();
        e.ledger().set(LedgerInfo {
            timestamp: 12345,
            protocol_version: 22,
            sequence_number: 175,
            network_id: Default::default(),
            base_reserve: 10,
            min_temp_entry_ttl: 172800,
            min_persistent_entry_ttl: 172800,
            max_entry_ttl: 9999999,
        });

        let bombadil = Address::generate(&e);
        let samwise = Address::generate(&e);
        let frodo = Address::generate(&e);

        let pool_address = create_pool(&e);

        let (oracle_address, oracle_client) = testutils::create_mock_oracle(&e);

        // creating reserves for a pool exhausts the budget
        e.cost_estimate().budget().reset_unlimited();
        let (underlying_0, _) = testutils::create_token_contract(&e, &bombadil);
        let (mut reserve_config_0, reserve_data_0) = testutils::default_reserve_meta();
        reserve_config_0.index = 0;
        testutils::create_reserve(
            &e,
            &pool_address,
            &underlying_0,
            &reserve_config_0,
            &reserve_data_0,
        );

        let (underlying_1, _) = testutils::create_token_contract(&e, &bombadil);
        let (mut reserve_config_1, reserve_data_1) = testutils::default_reserve_meta();
        reserve_config_1.index = 1;
        testutils::create_reserve(
            &e,
            &pool_address,
            &underlying_1,
            &reserve_config_1,
            &reserve_data_1,
        );

        let (underlying_2, _) = testutils::create_token_contract(&e, &bombadil);
        let (mut reserve_config_2, reserve_data_2) = testutils::default_reserve_meta();
        reserve_config_2.index = 2;
        testutils::create_reserve(
            &e,
            &pool_address,
            &underlying_2,
            &reserve_config_2,
            &reserve_data_2,
        );

        // Full-fill paths evaluate residual-position valuation through the
        // pool oracle; register real 7-decimal prices for all three reserves.
        oracle_client.set_data(
            &bombadil,
            &Asset::Other(Symbol::new(&e, "USD")),
            &vec![
                &e,
                Asset::Stellar(underlying_0.clone()),
                Asset::Stellar(underlying_1.clone()),
                Asset::Stellar(underlying_2.clone()),
            ],
            &7,
            &300,
        );
        oracle_client.set_price_stable(&vec![&e, 2_0000000, 4_0000000, 50_0000000]);
        e.cost_estimate().budget().reset_unlimited();

        let auction_data = AuctionData {
            bid: map![&e, (underlying_2.clone(), 1_2375000)],
            lot: map![
                &e,
                (underlying_0.clone(), 30_5595329),
                (underlying_1.clone(), 1_5395739)
            ],
            block: 176,
        };
        let pool_config = PoolConfig {
            oracle: oracle_address,
            min_collateral: 1_0000000,
            bstop_rate: 0_1000000,
            status: 0,
            max_positions: 4,
        };
        let positions: Positions = Positions {
            collateral: map![
                &e,
                (reserve_config_0.index, 90_9100000),
                (reserve_config_1.index, 04_5800000),
            ],
            liabilities: map![&e, (reserve_config_2.index, 02_7500000),],
            supply: map![&e],
        };
        e.as_contract(&pool_address, || {
            storage::set_user_positions(&e, &samwise, &positions);
            storage::set_pool_config(&e, &pool_config);
            storage::set_auction(&e, &0, &samwise, &auction_data);

            e.ledger().set(LedgerInfo {
                timestamp: 12345 + 200 * 5,
                protocol_version: 22,
                sequence_number: 176 + 200,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 172800,
                min_persistent_entry_ttl: 172800,
                max_entry_ttl: 9999999,
            });
            e.cost_estimate().budget().reset_unlimited();
            let mut pool = Pool::load(&e);
            let mut frodo_state = User::load(&e, &frodo);
            fill(&e, &mut pool, 0, &samwise, &mut frodo_state, 100);
            let has_auction = storage::has_auction(&e, &0, &samwise);
            assert_eq!(has_auction, false);
        });
    }

    #[test]
    fn test_partial_fill() {
        let e = Env::default();

        e.mock_all_auths();
        e.ledger().set(LedgerInfo {
            timestamp: 12345,
            protocol_version: 22,
            sequence_number: 175,
            network_id: Default::default(),
            base_reserve: 10,
            min_temp_entry_ttl: 172800,
            min_persistent_entry_ttl: 172800,
            max_entry_ttl: 9999999,
        });

        let bombadil = Address::generate(&e);
        let samwise = Address::generate(&e);
        let frodo = Address::generate(&e);

        let pool_address = create_pool(&e);

        let (oracle_address, _) = testutils::create_mock_oracle(&e);

        // creating reserves for a pool exhausts the budget
        e.cost_estimate().budget().reset_unlimited();
        let (underlying_0, _) = testutils::create_token_contract(&e, &bombadil);
        let (mut reserve_config_0, reserve_data_0) = testutils::default_reserve_meta();
        reserve_config_0.index = 0;
        testutils::create_reserve(
            &e,
            &pool_address,
            &underlying_0,
            &reserve_config_0,
            &reserve_data_0,
        );

        let (underlying_1, _) = testutils::create_token_contract(&e, &bombadil);
        let (mut reserve_config_1, reserve_data_1) = testutils::default_reserve_meta();
        reserve_config_1.index = 1;
        testutils::create_reserve(
            &e,
            &pool_address,
            &underlying_1,
            &reserve_config_1,
            &reserve_data_1,
        );

        let (underlying_2, _) = testutils::create_token_contract(&e, &bombadil);
        let (mut reserve_config_2, reserve_data_2) = testutils::default_reserve_meta();
        reserve_config_2.index = 2;
        testutils::create_reserve(
            &e,
            &pool_address,
            &underlying_2,
            &reserve_config_2,
            &reserve_data_2,
        );
        e.cost_estimate().budget().reset_unlimited();

        let auction_data = AuctionData {
            bid: map![&e, (underlying_2.clone(), 1_2375000)],
            lot: map![
                &e,
                (underlying_0.clone(), 30_5595329),
                (underlying_1.clone(), 1_5395739)
            ],
            block: 176,
        };
        let pool_config = PoolConfig {
            oracle: oracle_address,
            min_collateral: 1_0000000,
            bstop_rate: 0_1000000,
            status: 0,
            max_positions: 4,
        };
        let positions: Positions = Positions {
            collateral: map![
                &e,
                (reserve_config_0.index, 90_9100000),
                (reserve_config_1.index, 04_5800000),
            ],
            liabilities: map![&e, (reserve_config_2.index, 02_7500000),],
            supply: map![&e],
        };
        e.as_contract(&pool_address, || {
            storage::set_user_positions(&e, &samwise, &positions);
            storage::set_pool_config(&e, &pool_config);
            storage::set_auction(&e, &0, &samwise, &auction_data);

            e.ledger().set(LedgerInfo {
                timestamp: 12345 + 200 * 5,
                protocol_version: 22,
                sequence_number: 176 + 200,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 172800,
                min_persistent_entry_ttl: 172800,
                max_entry_ttl: 9999999,
            });
            e.cost_estimate().budget().reset_unlimited();
            let mut pool = Pool::load(&e);
            let mut frodo_state = User::load(&e, &frodo);
            fill(&e, &mut pool, 0, &samwise, &mut frodo_state, 25);

            let expected_new_auction_data = AuctionData {
                bid: map![&e, (underlying_2.clone(), 9281250)],
                lot: map![
                    &e,
                    (underlying_0.clone(), 22_9196497),
                    (underlying_1.clone(), 1_1546805)
                ],
                block: 176,
            };
            let new_auction = storage::get_auction(&e, &0, &samwise);
            assert_eq!(new_auction.bid, expected_new_auction_data.bid);
            assert_eq!(new_auction.lot, expected_new_auction_data.lot);
            assert_eq!(new_auction.block, expected_new_auction_data.block);
        });
    }

    #[test]
    fn test_partial_partial_full_fill() {
        let e = Env::default();
        e.cost_estimate().budget().reset_unlimited();
        e.mock_all_auths();

        e.ledger().set(LedgerInfo {
            timestamp: 12345,
            protocol_version: 22,
            sequence_number: 175,
            network_id: Default::default(),
            base_reserve: 10,
            min_temp_entry_ttl: 172800,
            min_persistent_entry_ttl: 172800,
            max_entry_ttl: 9999999,
        });

        let bombadil = Address::generate(&e);
        let samwise = Address::generate(&e);
        let frodo = Address::generate(&e);

        let pool_address = create_pool(&e);

        let (oracle_address, oracle_client) = testutils::create_mock_oracle(&e);

        let (underlying_0, _) = testutils::create_token_contract(&e, &bombadil);
        let (mut reserve_config_0, reserve_data_0) = testutils::default_reserve_meta();

        reserve_config_0.index = 0;
        testutils::create_reserve(
            &e,
            &pool_address,
            &underlying_0,
            &reserve_config_0,
            &reserve_data_0,
        );

        let (underlying_1, _) = testutils::create_token_contract(&e, &bombadil);
        let (mut reserve_config_1, reserve_data_1) = testutils::default_reserve_meta();

        reserve_config_1.index = 1;
        testutils::create_reserve(
            &e,
            &pool_address,
            &underlying_1,
            &reserve_config_1,
            &reserve_data_1,
        );

        let (underlying_2, _) = testutils::create_token_contract(&e, &bombadil);
        let (mut reserve_config_2, reserve_data_2) = testutils::default_reserve_meta();

        reserve_config_2.index = 2;
        testutils::create_reserve(
            &e,
            &pool_address,
            &underlying_2,
            &reserve_config_2,
            &reserve_data_2,
        );

        // The final full fill evaluates residual positions through the pool
        // oracle; register real 7-decimal prices for all three reserves.
        oracle_client.set_data(
            &bombadil,
            &Asset::Other(Symbol::new(&e, "USD")),
            &vec![
                &e,
                Asset::Stellar(underlying_0.clone()),
                Asset::Stellar(underlying_1.clone()),
                Asset::Stellar(underlying_2.clone()),
            ],
            &7,
            &300,
        );
        oracle_client.set_price_stable(&vec![&e, 2_0000000, 4_0000000, 50_0000000]);

        let auction_data = AuctionData {
            bid: map![&e, (underlying_2.clone(), 100_000_0000)],
            lot: map![
                &e,
                (underlying_0.clone(), 10_000_0000),
                (underlying_1.clone(), 1_000_0000)
            ],
            block: 176,
        };
        let pool_config = PoolConfig {
            oracle: oracle_address,
            min_collateral: 1_0000000,
            bstop_rate: 0_1000000,
            status: 0,
            max_positions: 4,
        };
        let positions: Positions = Positions {
            collateral: map![
                &e,
                (reserve_config_0.index, 30_000_0000),
                (reserve_config_1.index, 3_000_0000),
            ],
            liabilities: map![&e, (reserve_config_2.index, 200_000_0000),],
            supply: map![&e],
        };
        e.as_contract(&pool_address, || {
            storage::set_user_positions(&e, &samwise, &positions);
            storage::set_pool_config(&e, &pool_config);
            storage::set_auction(&e, &0, &samwise, &auction_data);

            // Partial fill 1 - 25% @ 50% lot mod
            e.ledger().set(LedgerInfo {
                timestamp: 12345 + 100 * 5,
                protocol_version: 22,
                sequence_number: 176 + 100,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 172800,
                min_persistent_entry_ttl: 172800,
                max_entry_ttl: 9999999,
            });
            let mut pool = Pool::load(&e);
            let mut frodo_state = User::load(&e, &frodo);
            fill(&e, &mut pool, 0, &samwise, &mut frodo_state, 25);

            let expected_new_auction_data = AuctionData {
                bid: map![&e, (underlying_2.clone(), 75_000_0000)],
                lot: map![
                    &e,
                    (underlying_0.clone(), 7_500_0000),
                    (underlying_1.clone(), 750_0000)
                ],
                block: 176,
            };

            // Partial fill 2 - 66% @ 100% mods
            let new_auction = storage::get_auction(&e, &0, &samwise);
            assert_eq!(new_auction.bid, expected_new_auction_data.bid);
            assert_eq!(new_auction.lot, expected_new_auction_data.lot);
            assert_eq!(new_auction.block, expected_new_auction_data.block);

            e.ledger().set(LedgerInfo {
                timestamp: 12345 + 200 * 5,
                protocol_version: 22,
                sequence_number: 176 + 200,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 172800,
                min_persistent_entry_ttl: 172800,
                max_entry_ttl: 9999999,
            });
            let mut pool = Pool::load(&e);
            let mut frodo_state = User::load(&e, &frodo);
            fill(&e, &mut pool, 0, &samwise, &mut frodo_state, 67);

            let expected_new_auction_data = AuctionData {
                bid: map![&e, (underlying_2.clone(), 24_7500000)],
                lot: map![
                    &e,
                    (underlying_0.clone(), 2_4750000),
                    (underlying_1.clone(), 0_2475000)
                ],
                block: 176,
            };
            let new_auction = storage::get_auction(&e, &0, &samwise);
            assert_eq!(new_auction.bid, expected_new_auction_data.bid);
            assert_eq!(new_auction.lot, expected_new_auction_data.lot);
            assert_eq!(new_auction.block, expected_new_auction_data.block);

            // full fill at 50% bid mod
            e.ledger().set(LedgerInfo {
                timestamp: 12345 + 300 * 5,
                protocol_version: 22,
                sequence_number: 176 + 300,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 172800,
                min_persistent_entry_ttl: 172800,
                max_entry_ttl: 9999999,
            });
            let mut pool = Pool::load(&e);
            let mut frodo_state = User::load(&e, &frodo);
            fill(&e, &mut pool, 0, &samwise, &mut frodo_state, 100);
            let new_auction = storage::has_auction(&e, &0, &samwise);
            assert_eq!(new_auction, false);
            let samwise_positions = storage::get_user_positions(&e, &samwise);
            assert_eq!(
                samwise_positions
                    .collateral
                    .get(reserve_config_0.index)
                    .unwrap_optimized(),
                30_000_0000 - 1_250_0000 - 5_000_0002 - 2_499_9998
            );
            assert_eq!(
                samwise_positions
                    .collateral
                    .get(reserve_config_1.index)
                    .unwrap_optimized(),
                3_000_0000 - 125_0000 - 500_0000 - 250_0000
            );
            assert_eq!(
                samwise_positions
                    .liabilities
                    .get(reserve_config_2.index)
                    .unwrap_optimized(),
                200_000_0000 - 25_000_0000 - 50_000_0025 - 12_6249975
            );
        });
    }

    #[test]
    #[should_panic(expected = "Error(Contract, #1200)")]
    fn test_fill_fails_pct_too_large() {
        let e = Env::default();

        e.mock_all_auths();
        e.ledger().set(LedgerInfo {
            timestamp: 12345,
            protocol_version: 22,
            sequence_number: 175,
            network_id: Default::default(),
            base_reserve: 10,
            min_temp_entry_ttl: 172800,
            min_persistent_entry_ttl: 172800,
            max_entry_ttl: 9999999,
        });

        let bombadil = Address::generate(&e);
        let samwise = Address::generate(&e);
        let frodo = Address::generate(&e);

        let pool_address = create_pool(&e);

        let (oracle_address, _) = testutils::create_mock_oracle(&e);

        // creating reserves for a pool exhausts the budget
        e.cost_estimate().budget().reset_unlimited();
        let (underlying_0, _) = testutils::create_token_contract(&e, &bombadil);
        let (mut reserve_config_0, reserve_data_0) = testutils::default_reserve_meta();
        reserve_config_0.index = 0;
        testutils::create_reserve(
            &e,
            &pool_address,
            &underlying_0,
            &reserve_config_0,
            &reserve_data_0,
        );

        let (underlying_1, _) = testutils::create_token_contract(&e, &bombadil);
        let (mut reserve_config_1, reserve_data_1) = testutils::default_reserve_meta();
        reserve_config_1.index = 1;
        testutils::create_reserve(
            &e,
            &pool_address,
            &underlying_1,
            &reserve_config_1,
            &reserve_data_1,
        );

        let (underlying_2, _) = testutils::create_token_contract(&e, &bombadil);
        let (mut reserve_config_2, reserve_data_2) = testutils::default_reserve_meta();
        reserve_config_2.index = 2;
        testutils::create_reserve(
            &e,
            &pool_address,
            &underlying_2,
            &reserve_config_2,
            &reserve_data_2,
        );

        let auction_data = AuctionData {
            bid: map![&e, (underlying_2.clone(), 1_2375000)],
            lot: map![
                &e,
                (underlying_0.clone(), 30_5595329),
                (underlying_1.clone(), 1_5395739)
            ],
            block: 176,
        };
        let pool_config = PoolConfig {
            oracle: oracle_address,
            min_collateral: 1_0000000,
            bstop_rate: 0_1000000,
            status: 0,
            max_positions: 4,
        };
        let positions: Positions = Positions {
            collateral: map![
                &e,
                (reserve_config_0.index, 90_9100000),
                (reserve_config_1.index, 04_5800000),
            ],
            liabilities: map![&e, (reserve_config_2.index, 02_7500000),],
            supply: map![&e],
        };
        e.as_contract(&pool_address, || {
            storage::set_user_positions(&e, &samwise, &positions);
            storage::set_pool_config(&e, &pool_config);
            storage::set_auction(&e, &0, &samwise, &auction_data);

            e.ledger().set(LedgerInfo {
                timestamp: 12345 + 200 * 5,
                protocol_version: 22,
                sequence_number: 176 + 200,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 172800,
                min_persistent_entry_ttl: 172800,
                max_entry_ttl: 9999999,
            });
            e.cost_estimate().budget().reset_unlimited();
            let mut pool = Pool::load(&e);
            let mut frodo_state = User::load(&e, &frodo);
            fill(&e, &mut pool, 0, &samwise, &mut frodo_state, 101);

            let expected_new_auction_data = AuctionData {
                bid: map![&e, (underlying_2.clone(), 9281250)],
                lot: map![
                    &e,
                    (underlying_0.clone(), 22_9196497),
                    (underlying_1.clone(), 1_1546805)
                ],
                block: 176,
            };
            let new_auction = storage::get_auction(&e, &0, &samwise);
            assert_eq!(new_auction.bid, expected_new_auction_data.bid);
            assert_eq!(new_auction.lot, expected_new_auction_data.lot);
            assert_eq!(new_auction.block, expected_new_auction_data.block);
        });
    }

    #[test]
    #[should_panic(expected = "Error(Contract, #1200)")]
    fn test_fill_fails_pct_too_small() {
        let e = Env::default();

        e.mock_all_auths();
        e.ledger().set(LedgerInfo {
            timestamp: 12345,
            protocol_version: 22,
            sequence_number: 175,
            network_id: Default::default(),
            base_reserve: 10,
            min_temp_entry_ttl: 172800,
            min_persistent_entry_ttl: 172800,
            max_entry_ttl: 9999999,
        });

        let bombadil = Address::generate(&e);
        let samwise = Address::generate(&e);
        let frodo = Address::generate(&e);

        let pool_address = create_pool(&e);

        let (oracle_address, _) = testutils::create_mock_oracle(&e);

        // creating reserves for a pool exhausts the budget
        e.cost_estimate().budget().reset_unlimited();
        let (underlying_0, _) = testutils::create_token_contract(&e, &bombadil);
        let (mut reserve_config_0, reserve_data_0) = testutils::default_reserve_meta();
        reserve_config_0.index = 0;
        testutils::create_reserve(
            &e,
            &pool_address,
            &underlying_0,
            &reserve_config_0,
            &reserve_data_0,
        );

        let (underlying_1, _) = testutils::create_token_contract(&e, &bombadil);
        let (mut reserve_config_1, reserve_data_1) = testutils::default_reserve_meta();

        reserve_config_1.index = 1;
        testutils::create_reserve(
            &e,
            &pool_address,
            &underlying_1,
            &reserve_config_1,
            &reserve_data_1,
        );

        let (underlying_2, _) = testutils::create_token_contract(&e, &bombadil);
        let (mut reserve_config_2, reserve_data_2) = testutils::default_reserve_meta();

        reserve_config_2.index = 2;
        testutils::create_reserve(
            &e,
            &pool_address,
            &underlying_2,
            &reserve_config_2,
            &reserve_data_2,
        );
        e.cost_estimate().budget().reset_unlimited();
        let auction_data = AuctionData {
            bid: map![&e, (underlying_2.clone(), 1_2375000)],
            lot: map![
                &e,
                (underlying_0.clone(), 30_5595329),
                (underlying_1.clone(), 1_5395739)
            ],
            block: 176,
        };
        let pool_config = PoolConfig {
            oracle: oracle_address,
            min_collateral: 1_0000000,
            bstop_rate: 0_1000000,
            status: 0,
            max_positions: 4,
        };
        let positions: Positions = Positions {
            collateral: map![
                &e,
                (reserve_config_0.index, 90_9100000),
                (reserve_config_1.index, 04_5800000),
            ],
            liabilities: map![&e, (reserve_config_2.index, 02_7500000),],
            supply: map![&e],
        };
        e.as_contract(&pool_address, || {
            storage::set_user_positions(&e, &samwise, &positions);
            storage::set_pool_config(&e, &pool_config);
            storage::set_auction(&e, &0, &samwise, &auction_data);

            e.ledger().set(LedgerInfo {
                timestamp: 12345 + 200 * 5,
                protocol_version: 22,
                sequence_number: 176 + 200,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 172800,
                min_persistent_entry_ttl: 172800,
                max_entry_ttl: 9999999,
            });
            e.cost_estimate().budget().reset_unlimited();
            let mut pool = Pool::load(&e);
            let mut frodo_state = User::load(&e, &frodo);
            fill(&e, &mut pool, 0, &samwise, &mut frodo_state, 0);

            let expected_new_auction_data = AuctionData {
                bid: map![&e, (underlying_2.clone(), 9281250)],
                lot: map![
                    &e,
                    (underlying_0.clone(), 22_9196497),
                    (underlying_1.clone(), 1_1546805)
                ],
                block: 176,
            };
            let new_auction = storage::get_auction(&e, &0, &samwise);
            assert_eq!(new_auction.bid, expected_new_auction_data.bid);
            assert_eq!(new_auction.lot, expected_new_auction_data.lot);
            assert_eq!(new_auction.block, expected_new_auction_data.block);
        });
    }

    #[test]
    #[should_panic(expected = "Error(Contract, #1211)")]
    fn test_fill_liquidation_same_address() {
        let e = Env::default();

        e.mock_all_auths();
        e.ledger().set(LedgerInfo {
            timestamp: 12345,
            protocol_version: 22,
            sequence_number: 175,
            network_id: Default::default(),
            base_reserve: 10,
            min_temp_entry_ttl: 172800,
            min_persistent_entry_ttl: 172800,
            max_entry_ttl: 9999999,
        });

        let bombadil = Address::generate(&e);
        let samwise = Address::generate(&e);

        let pool_address = create_pool(&e);

        let (oracle_address, _) = testutils::create_mock_oracle(&e);

        // creating reserves for a pool exhausts the budget
        e.cost_estimate().budget().reset_unlimited();
        let (underlying_0, _) = testutils::create_token_contract(&e, &bombadil);
        let (mut reserve_config_0, reserve_data_0) = testutils::default_reserve_meta();
        reserve_config_0.index = 0;
        testutils::create_reserve(
            &e,
            &pool_address,
            &underlying_0,
            &reserve_config_0,
            &reserve_data_0,
        );

        let (underlying_1, _) = testutils::create_token_contract(&e, &bombadil);
        let (mut reserve_config_1, reserve_data_1) = testutils::default_reserve_meta();
        reserve_config_1.index = 1;
        testutils::create_reserve(
            &e,
            &pool_address,
            &underlying_1,
            &reserve_config_1,
            &reserve_data_1,
        );

        let (underlying_2, _) = testutils::create_token_contract(&e, &bombadil);
        let (mut reserve_config_2, reserve_data_2) = testutils::default_reserve_meta();
        reserve_config_2.index = 2;
        testutils::create_reserve(
            &e,
            &pool_address,
            &underlying_2,
            &reserve_config_2,
            &reserve_data_2,
        );
        e.cost_estimate().budget().reset_unlimited();

        let auction_data = AuctionData {
            bid: map![&e, (underlying_2.clone(), 1_2375000)],
            lot: map![
                &e,
                (underlying_0.clone(), 30_5595329),
                (underlying_1.clone(), 1_5395739)
            ],
            block: 176,
        };
        let pool_config = PoolConfig {
            oracle: oracle_address,
            min_collateral: 1_0000000,
            bstop_rate: 0_1000000,
            status: 0,
            max_positions: 4,
        };
        let positions: Positions = Positions {
            collateral: map![
                &e,
                (reserve_config_0.index, 90_9100000),
                (reserve_config_1.index, 04_5800000),
            ],
            liabilities: map![&e, (reserve_config_2.index, 02_7500000),],
            supply: map![&e],
        };
        e.as_contract(&pool_address, || {
            storage::set_user_positions(&e, &samwise, &positions);
            storage::set_pool_config(&e, &pool_config);
            storage::set_auction(&e, &0, &samwise, &auction_data);

            e.ledger().set(LedgerInfo {
                timestamp: 12345 + 200 * 5,
                protocol_version: 22,
                sequence_number: 176 + 200,
                network_id: Default::default(),
                base_reserve: 10,
                min_temp_entry_ttl: 172800,
                min_persistent_entry_ttl: 172800,
                max_entry_ttl: 9999999,
            });
            e.cost_estimate().budget().reset_unlimited();
            let mut pool = Pool::load(&e);
            let mut samwise_state = User::load(&e, &samwise);
            fill(&e, &mut pool, 0, &samwise, &mut samwise_state, 100);
        });
    }

    #[test]
    fn test_delete_stale_auction() {
        let e = Env::default();
        e.mock_all_auths();

        e.ledger().set(LedgerInfo {
            timestamp: 12345,
            protocol_version: 22,
            sequence_number: 1500,
            network_id: Default::default(),
            base_reserve: 10,
            min_temp_entry_ttl: 172800,
            min_persistent_entry_ttl: 172800,
            max_entry_ttl: 9999999,
        });

        let pool_address = create_pool(&e);
        let auction_type: u32 = 2;
        let user = Address::generate(&e);
        let underlying_0 = Address::generate(&e);
        let underlying_1 = Address::generate(&e);

        let auction_data = AuctionData {
            bid: map![&e, (underlying_0.clone(), 100_0000000)],
            lot: map![&e, (underlying_1.clone(), 100_0000000)],
            block: 1000,
        };
        e.as_contract(&pool_address, || {
            storage::set_auction(&e, &auction_type, &user, &auction_data);
            let has_auction = storage::has_auction(&e, &auction_type, &user);
            assert_eq!(has_auction, true);

            delete_stale_auction(&e, auction_type, &user);
            let has_auction = storage::has_auction(&e, &auction_type, &user);
            assert_eq!(has_auction, false);
        });
    }

    // #[test]
    // fn test_delete_stale_auction_bad_debt() {
    //     let e = Env::default();
    //     e.mock_all_auths();

    //     e.ledger().set(LedgerInfo {
    //         timestamp: 12345,
    //         protocol_version: 22,
    //         sequence_number: 1500,
    //         network_id: Default::default(),
    //         base_reserve: 10,
    //         min_temp_entry_ttl: 172800,
    //         min_persistent_entry_ttl: 172800,
    //         max_entry_ttl: 9999999,
    //     });

    //     let pool_address = create_pool(&e);
    //     let bombadil = Address::generate(&e);
    //     let frodo = Address::generate(&e);

    //     let (blnd, blnd_client) = create_blnd_token(&e, &pool_address, &bombadil);
    //     let (usdc, usdc_client) = create_token_contract(&e, &bombadil);
    //     let (lp_token, lp_token_client) = create_comet_lp_pool(&e, &bombadil, &blnd, &usdc);
    //     let (backstop_address, backstop_client) =
    //         create_backstop(&e, &pool_address, &lp_token, &usdc, &blnd);

    //     // mint lp tokens and deposit them into the pool's backstop
    //     let backstop_tokens = 1_500_0000000; // over 5% of threshold
    //     blnd_client.mint(&frodo, &500_001_0000000);
    //     blnd_client.approve(&frodo, &lp_token, &i128::MAX, &99999);
    //     usdc_client.mint(&frodo, &12_501_0000000);
    //     usdc_client.approve(&frodo, &lp_token, &i128::MAX, &99999);
    //     lp_token_client.join_pool(
    //         &backstop_tokens,
    //         &vec![&e, 500_001_0000000, 12_501_0000000],
    //         &frodo,
    //     );
    //     backstop_client.deposit(&frodo, &pool_address, &backstop_tokens);

    //     let (underlying_0, _) = testutils::create_token_contract(&e, &bombadil);
    //     let (reserve_config, reserve_data_0) = testutils::default_reserve_meta();
    //     testutils::create_reserve(
    //         &e,
    //         &pool_address,
    //         &underlying_0,
    //         &reserve_config,
    //         &reserve_data_0,
    //     );

    //     let (underlying_1, _) = testutils::create_token_contract(&e, &bombadil);
    //     let (reserve_config, reserve_data_1) = testutils::default_reserve_meta();
    //     testutils::create_reserve(
    //         &e,
    //         &pool_address,
    //         &underlying_1,
    //         &reserve_config,
    //         &reserve_data_1,
    //     );

    //     let auction_type: u32 = 1;
    //     let auction_data = AuctionData {
    //         bid: map![&e, (underlying_0.clone(), 100_0000000)],
    //         lot: map![&e, (underlying_1.clone(), 100_0000000)],
    //         block: 1000,
    //     };

    //     let backstop_positions = Positions {
    //         collateral: map![&e],
    //         liabilities: map![&e, (0, 100_0000000)],
    //         supply: map![&e,],
    //     };
    //     let pool_config = PoolConfig {
    //         oracle: Address::generate(&e),
    //         min_collateral: 1_0000000,
    //         bstop_rate: 0_1000000,
    //         status: 1,
    //         max_positions: 5,
    //     };
    //     e.as_contract(&pool_address, || {
    //         storage::set_pool_config(&e, &pool_config);
    //         storage::set_user_positions(&e, &backstop_address, &backstop_positions);
    //         storage::set_auction(&e, &auction_type, &backstop_address, &auction_data);
    //         let has_auction = storage::has_auction(&e, &auction_type, &backstop_address);
    //         assert_eq!(has_auction, true);

    //         delete_stale_auction(&e, auction_type, &backstop_address);
    //         let has_auction = storage::has_auction(&e, &auction_type, &backstop_address);
    //         assert_eq!(has_auction, false);

    //         // validate no other state changed
    //         let post_backstop_positions = storage::get_user_positions(&e, &backstop_address);
    //         assert_eq!(post_backstop_positions.collateral.len(), 0);
    //         assert_eq!(
    //             post_backstop_positions.liabilities,
    //             backstop_positions.liabilities
    //         );
    //         assert_eq!(post_backstop_positions.supply.len(), 0);

    //         let post_reserve_data_0 = storage::get_res_data(&e, &underlying_0);
    //         assert_eq!(post_reserve_data_0.last_time, 0);
    //         assert_eq!(post_reserve_data_0.d_supply, reserve_data_0.d_supply);
    //         let post_reserve_data_1 = storage::get_res_data(&e, &underlying_1);
    //         assert_eq!(post_reserve_data_1.last_time, 0);
    //         assert_eq!(post_reserve_data_1.d_supply, reserve_data_1.d_supply);
    //     });
    // }

    // #[test]
    // fn test_delete_stale_auction_bad_debt_needs_default() {
    //     let e = Env::default();
    //     e.mock_all_auths();

    //     e.ledger().set(LedgerInfo {
    //         timestamp: 12345,
    //         protocol_version: 22,
    //         sequence_number: 1500,
    //         network_id: Default::default(),
    //         base_reserve: 10,
    //         min_temp_entry_ttl: 172800,
    //         min_persistent_entry_ttl: 172800,
    //         max_entry_ttl: 9999999,
    //     });

    //     let pool_address = create_pool(&e);
    //     let bombadil = Address::generate(&e);
    //     let frodo = Address::generate(&e);

    //     let (blnd, blnd_client) = create_blnd_token(&e, &pool_address, &bombadil);
    //     let (usdc, usdc_client) = create_token_contract(&e, &bombadil);
    //     let (lp_token, lp_token_client) = create_comet_lp_pool(&e, &bombadil, &blnd, &usdc);
    //     let (backstop_address, backstop_client) =
    //         create_backstop(&e, &pool_address, &lp_token, &usdc, &blnd);

    //     // mint lp tokens and deposit them into the pool's backstop
    //     let backstop_tokens = 1_000_0000000; // under 5% of threshold
    //     blnd_client.mint(&frodo, &500_001_0000000);
    //     blnd_client.approve(&frodo, &lp_token, &i128::MAX, &99999);
    //     usdc_client.mint(&frodo, &12_501_0000000);
    //     usdc_client.approve(&frodo, &lp_token, &i128::MAX, &99999);
    //     lp_token_client.join_pool(
    //         &backstop_tokens,
    //         &vec![&e, 500_001_0000000, 12_501_0000000],
    //         &frodo,
    //     );
    //     backstop_client.deposit(&frodo, &pool_address, &backstop_tokens);

    //     let (underlying_0, _) = testutils::create_token_contract(&e, &bombadil);
    //     let (reserve_config, reserve_data_0) = testutils::default_reserve_meta();
    //     testutils::create_reserve(
    //         &e,
    //         &pool_address,
    //         &underlying_0,
    //         &reserve_config,
    //         &reserve_data_0,
    //     );

    //     let (underlying_1, _) = testutils::create_token_contract(&e, &bombadil);
    //     let (reserve_config, reserve_data_1) = testutils::default_reserve_meta();
    //     testutils::create_reserve(
    //         &e,
    //         &pool_address,
    //         &underlying_1,
    //         &reserve_config,
    //         &reserve_data_1,
    //     );

    //     let auction_type: u32 = 1;
    //     let auction_data = AuctionData {
    //         bid: map![&e, (underlying_0.clone(), 100_0000000)],
    //         lot: map![&e, (underlying_1.clone(), 100_0000000)],
    //         block: 1000,
    //     };

    //     let backstop_positions = Positions {
    //         collateral: map![&e],
    //         liabilities: map![&e, (0, 100_0000000)],
    //         supply: map![&e,],
    //     };
    //     let pool_config = PoolConfig {
    //         oracle: Address::generate(&e),
    //         min_collateral: 1_0000000,
    //         bstop_rate: 0_1000000,
    //         status: 1,
    //         max_positions: 5,
    //     };
    //     e.as_contract(&pool_address, || {
    //         storage::set_pool_config(&e, &pool_config);
    //         storage::set_user_positions(&e, &backstop_address, &backstop_positions);
    //         storage::set_auction(&e, &auction_type, &backstop_address, &auction_data);
    //         let has_auction = storage::has_auction(&e, &auction_type, &backstop_address);
    //         assert_eq!(has_auction, true);

    //         delete_stale_auction(&e, auction_type, &backstop_address);
    //         let has_auction = storage::has_auction(&e, &auction_type, &backstop_address);
    //         assert_eq!(has_auction, false);

    //         // validate backstop positions defaulted
    //         let post_backstop_positions = storage::get_user_positions(&e, &backstop_address);
    //         assert_eq!(post_backstop_positions.collateral.len(), 0);
    //         assert_eq!(post_backstop_positions.liabilities.len(), 0);
    //         assert_eq!(post_backstop_positions.supply.len(), 0);

    //         let post_reserve_data_0 = storage::get_res_data(&e, &underlying_0);
    //         assert_eq!(post_reserve_data_0.last_time, 12345);
    //         assert!(post_reserve_data_0.d_supply < reserve_data_0.d_supply);
    //         assert!(post_reserve_data_0.d_rate > reserve_data_0.d_rate);
    //         assert_eq!(post_reserve_data_0.b_supply, reserve_data_0.b_supply);
    //         assert!(post_reserve_data_0.b_rate < reserve_data_0.b_rate);
    //         // non-affected reserve not changed
    //         let post_reserve_data_1 = storage::get_res_data(&e, &underlying_1);
    //         assert_eq!(post_reserve_data_1.last_time, 0);
    //         assert_eq!(post_reserve_data_1.d_supply, reserve_data_1.d_supply);
    //     });
    // }

    // #[test]
    // fn test_delete_stale_auction_user_liquidation() {
    //     let e = Env::default();
    //     e.mock_all_auths();

    //     e.ledger().set(LedgerInfo {
    //         timestamp: 12345,
    //         protocol_version: 22,
    //         sequence_number: 1500,
    //         network_id: Default::default(),
    //         base_reserve: 10,
    //         min_temp_entry_ttl: 172800,
    //         min_persistent_entry_ttl: 172800,
    //         max_entry_ttl: 9999999,
    //     });

    //     let pool_address = create_pool(&e);
    //     let bombadil = Address::generate(&e);
    //     let frodo = Address::generate(&e);
    //     let samwise = Address::generate(&e);

    //     let (blnd, blnd_client) = create_blnd_token(&e, &pool_address, &bombadil);
    //     let (usdc, usdc_client) = create_token_contract(&e, &bombadil);
    //     let (lp_token, lp_token_client) = create_comet_lp_pool(&e, &bombadil, &blnd, &usdc);
    //     let (_, backstop_client) = create_backstop(&e, &pool_address, &lp_token, &usdc, &blnd);

    //     // mint lp tokens and deposit them into the pool's backstop
    //     let backstop_tokens = 1_500_0000000; // over 5% of threshold
    //     blnd_client.mint(&frodo, &500_001_0000000);
    //     blnd_client.approve(&frodo, &lp_token, &i128::MAX, &99999);
    //     usdc_client.mint(&frodo, &12_501_0000000);
    //     usdc_client.approve(&frodo, &lp_token, &i128::MAX, &99999);
    //     lp_token_client.join_pool(
    //         &backstop_tokens,
    //         &vec![&e, 500_001_0000000, 12_501_0000000],
    //         &frodo,
    //     );
    //     backstop_client.deposit(&frodo, &pool_address, &backstop_tokens);

    //     let (underlying_0, _) = testutils::create_token_contract(&e, &bombadil);
    //     let (reserve_config, reserve_data_0) = testutils::default_reserve_meta();
    //     testutils::create_reserve(
    //         &e,
    //         &pool_address,
    //         &underlying_0,
    //         &reserve_config,
    //         &reserve_data_0,
    //     );

    //     let (underlying_1, _) = testutils::create_token_contract(&e, &bombadil);
    //     let (reserve_config, reserve_data_1) = testutils::default_reserve_meta();
    //     testutils::create_reserve(
    //         &e,
    //         &pool_address,
    //         &underlying_1,
    //         &reserve_config,
    //         &reserve_data_1,
    //     );

    //     let auction_type: u32 = 0;
    //     let auction_data = AuctionData {
    //         bid: map![&e, (underlying_0.clone(), 100_0000000)],
    //         lot: map![&e, (underlying_1.clone(), 100_0000000)],
    //         block: 1000,
    //     };

    //     let positions = Positions {
    //         collateral: map![&e, (1, 100_0000000)],
    //         liabilities: map![&e, (0, 100_0000000)],
    //         supply: map![&e,],
    //     };
    //     let pool_config = PoolConfig {
    //         oracle: Address::generate(&e),
    //         min_collateral: 1_0000000,
    //         bstop_rate: 0_1000000,
    //         status: 1,
    //         max_positions: 5,
    //     };
    //     e.as_contract(&pool_address, || {
    //         storage::set_pool_config(&e, &pool_config);
    //         storage::set_user_positions(&e, &samwise, &positions);
    //         storage::set_auction(&e, &auction_type, &samwise, &auction_data);
    //         let has_auction = storage::has_auction(&e, &auction_type, &samwise);
    //         assert_eq!(has_auction, true);

    //         delete_stale_auction(&e, auction_type, &samwise);
    //         let has_auction = storage::has_auction(&e, &auction_type, &samwise);
    //         assert_eq!(has_auction, false);

    //         // validate no other state changed
    //         let post_positions = storage::get_user_positions(&e, &samwise);
    //         assert_eq!(post_positions.collateral, positions.collateral);
    //         assert_eq!(post_positions.liabilities, positions.liabilities);
    //         assert_eq!(post_positions.supply, positions.supply);

    //         let post_reserve_data_0 = storage::get_res_data(&e, &underlying_0);
    //         assert_eq!(post_reserve_data_0.last_time, 0);
    //         assert_eq!(post_reserve_data_0.d_supply, reserve_data_0.d_supply);
    //         let post_reserve_data_1 = storage::get_res_data(&e, &underlying_1);
    //         assert_eq!(post_reserve_data_1.last_time, 0);
    //         assert_eq!(post_reserve_data_1.d_supply, reserve_data_1.d_supply);
    //     });
    // }

    // #[test]
    // fn test_delete_stale_auction_user_liquidation_bad_debt() {
    //     let e = Env::default();
    //     e.mock_all_auths();

    //     e.ledger().set(LedgerInfo {
    //         timestamp: 12345,
    //         protocol_version: 22,
    //         sequence_number: 1500,
    //         network_id: Default::default(),
    //         base_reserve: 10,
    //         min_temp_entry_ttl: 172800,
    //         min_persistent_entry_ttl: 172800,
    //         max_entry_ttl: 9999999,
    //     });

    //     let pool_address = create_pool(&e);
    //     let bombadil = Address::generate(&e);
    //     let frodo = Address::generate(&e);
    //     let samwise = Address::generate(&e);

    //     let (blnd, blnd_client) = create_blnd_token(&e, &pool_address, &bombadil);
    //     let (usdc, usdc_client) = create_token_contract(&e, &bombadil);
    //     let (lp_token, lp_token_client) = create_comet_lp_pool(&e, &bombadil, &blnd, &usdc);
    //     let (backstop_address, backstop_client) =
    //         create_backstop(&e, &pool_address, &lp_token, &usdc, &blnd);

    //     // mint lp tokens and deposit them into the pool's backstop
    //     let backstop_tokens = 1_500_0000000; // over 5% of threshold
    //     blnd_client.mint(&frodo, &500_001_0000000);
    //     blnd_client.approve(&frodo, &lp_token, &i128::MAX, &99999);
    //     usdc_client.mint(&frodo, &12_501_0000000);
    //     usdc_client.approve(&frodo, &lp_token, &i128::MAX, &99999);
    //     lp_token_client.join_pool(
    //         &backstop_tokens,
    //         &vec![&e, 500_001_0000000, 12_501_0000000],
    //         &frodo,
    //     );
    //     backstop_client.deposit(&frodo, &pool_address, &backstop_tokens);

    //     let (underlying_0, _) = testutils::create_token_contract(&e, &bombadil);
    //     let (reserve_config, reserve_data_0) = testutils::default_reserve_meta();
    //     testutils::create_reserve(
    //         &e,
    //         &pool_address,
    //         &underlying_0,
    //         &reserve_config,
    //         &reserve_data_0,
    //     );

    //     let (underlying_1, _) = testutils::create_token_contract(&e, &bombadil);
    //     let (reserve_config, reserve_data_1) = testutils::default_reserve_meta();
    //     testutils::create_reserve(
    //         &e,
    //         &pool_address,
    //         &underlying_1,
    //         &reserve_config,
    //         &reserve_data_1,
    //     );

    //     let auction_type: u32 = 0;
    //     let auction_data = AuctionData {
    //         bid: map![&e, (underlying_0.clone(), 100_0000000)],
    //         lot: map![&e, (underlying_1.clone(), 100_0000000)],
    //         block: 1000,
    //     };

    //     let positions = Positions {
    //         collateral: map![&e],
    //         liabilities: map![&e, (0, 100_0000000)],
    //         supply: map![&e,],
    //     };
    //     let pool_config = PoolConfig {
    //         oracle: Address::generate(&e),
    //         min_collateral: 1_0000000,
    //         bstop_rate: 0_1000000,
    //         status: 1,
    //         max_positions: 5,
    //     };
    //     e.as_contract(&pool_address, || {
    //         storage::set_pool_config(&e, &pool_config);
    //         storage::set_user_positions(&e, &samwise, &positions);
    //         storage::set_auction(&e, &auction_type, &samwise, &auction_data);
    //         let has_auction = storage::has_auction(&e, &auction_type, &samwise);
    //         assert_eq!(has_auction, true);

    //         delete_stale_auction(&e, auction_type, &samwise);
    //         let has_auction = storage::has_auction(&e, &auction_type, &samwise);
    //         assert_eq!(has_auction, false);

    //         // validate bad debt assigned to backstop
    //         let post_positions = storage::get_user_positions(&e, &samwise);
    //         assert_eq!(post_positions.collateral.len(), 0);
    //         assert_eq!(post_positions.liabilities.len(), 0);
    //         assert_eq!(post_positions.supply.len(), 0);

    //         let backstop_positions = storage::get_user_positions(&e, &backstop_address);
    //         assert_eq!(backstop_positions.collateral.len(), 0);
    //         assert_eq!(backstop_positions.liabilities, positions.liabilities);
    //         assert_eq!(backstop_positions.supply.len(), 0);

    //         let post_reserve_data_0 = storage::get_res_data(&e, &underlying_0);
    //         assert_eq!(post_reserve_data_0.last_time, 12345);
    //         assert_eq!(post_reserve_data_0.d_supply, reserve_data_0.d_supply);
    //         let post_reserve_data_1 = storage::get_res_data(&e, &underlying_1);
    //         assert_eq!(post_reserve_data_1.last_time, 0);
    //         assert_eq!(post_reserve_data_1.d_supply, reserve_data_1.d_supply);
    //     });
    // }

    #[test]
    #[should_panic(expected = "Error(Contract, #1200)")]
    fn test_delete_stale_auction_not_stale() {
        let e = Env::default();
        e.mock_all_auths();

        e.ledger().set(LedgerInfo {
            timestamp: 12345,
            protocol_version: 22,
            sequence_number: 1500,
            network_id: Default::default(),
            base_reserve: 10,
            min_temp_entry_ttl: 172800,
            min_persistent_entry_ttl: 172800,
            max_entry_ttl: 9999999,
        });

        let pool_address = create_pool(&e);
        let user = Address::generate(&e);
        let underlying_0 = Address::generate(&e);
        let underlying_1 = Address::generate(&e);

        let auction_type: u32 = 2;
        let auction_data = AuctionData {
            bid: map![&e, (underlying_0.clone(), 100_0000000)],
            lot: map![&e, (underlying_1.clone(), 100_0000000)],
            block: 1001,
        };

        e.as_contract(&pool_address, || {
            storage::set_auction(&e, &auction_type, &user, &auction_data);
            let has_auction = storage::has_auction(&e, &auction_type, &user);
            assert_eq!(has_auction, true);

            delete_stale_auction(&e, auction_type, &user);
        });
    }

    #[test]
    #[should_panic(expected = "Error(Contract, #1200)")]
    fn test_delete_stale_auction_does_not_exist() {
        let e = Env::default();
        e.mock_all_auths();

        e.ledger().set(LedgerInfo {
            timestamp: 12345,
            protocol_version: 22,
            sequence_number: 1500,
            network_id: Default::default(),
            base_reserve: 10,
            min_temp_entry_ttl: 172800,
            min_persistent_entry_ttl: 172800,
            max_entry_ttl: 9999999,
        });

        let pool_address = create_pool(&e);
        let auction_type: u32 = 2;
        let user = Address::generate(&e);
        let underlying_0 = Address::generate(&e);
        let underlying_1 = Address::generate(&e);

        let auction_data = AuctionData {
            bid: map![&e, (underlying_0.clone(), 100_0000000)],
            lot: map![&e, (underlying_1.clone(), 100_0000000)],
            block: 1001,
        };

        e.as_contract(&pool_address, || {
            storage::set_auction(&e, &auction_type, &user, &auction_data);
            let has_auction = storage::has_auction(&e, &auction_type, &user);
            assert_eq!(has_auction, true);

            delete_stale_auction(&e, 0, &user);
        });
    }

    #[test]
    fn test_scale_auction_100_fill_pct() {
        // 0 blocks
        let e = Env::default();
        let underlying_0 = Address::generate(&e);
        let underlying_1 = Address::generate(&e);

        let base_auction_data = AuctionData {
            bid: map![&e, (underlying_0.clone(), 100_0000000)],
            lot: map![&e, (underlying_1.clone(), 100_0000000)],
            block: 1000,
        };

        // 0 blocks
        e.ledger().set(LedgerInfo {
            timestamp: 12345,
            protocol_version: 22,
            sequence_number: 1000,
            network_id: Default::default(),
            base_reserve: 10,
            min_temp_entry_ttl: 172800,
            min_persistent_entry_ttl: 172800,
            max_entry_ttl: 9999999,
        });
        let (scaled_auction, remaining_auction) = scale_auction(&e, &base_auction_data, 100);
        assert_eq!(
            scaled_auction.bid.get_unchecked(underlying_0.clone()),
            100_0000000
        );
        assert_eq!(scaled_auction.lot.len(), 0);
        assert!(remaining_auction.is_none());

        // 100 blocks
        e.ledger().set(LedgerInfo {
            timestamp: 12345,
            protocol_version: 22,
            sequence_number: 1100,
            network_id: Default::default(),
            base_reserve: 10,
            min_temp_entry_ttl: 172800,
            min_persistent_entry_ttl: 172800,
            max_entry_ttl: 9999999,
        });
        let (scaled_auction, remaining_auction) = scale_auction(&e, &base_auction_data, 100);
        assert_eq!(
            scaled_auction.bid.get_unchecked(underlying_0.clone()),
            100_0000000
        );
        assert_eq!(
            scaled_auction.lot.get_unchecked(underlying_1.clone()),
            50_0000000
        );
        assert!(remaining_auction.is_none());

        // 200 blocks
        e.ledger().set(LedgerInfo {
            timestamp: 12345,
            protocol_version: 22,
            sequence_number: 1200,
            network_id: Default::default(),
            base_reserve: 10,
            min_temp_entry_ttl: 172800,
            min_persistent_entry_ttl: 172800,
            max_entry_ttl: 9999999,
        });
        let (scaled_auction, remaining_auction) = scale_auction(&e, &base_auction_data, 100);
        assert_eq!(
            scaled_auction.bid.get_unchecked(underlying_0.clone()),
            100_0000000
        );
        assert_eq!(
            scaled_auction.lot.get_unchecked(underlying_1.clone()),
            100_0000000
        );
        assert!(remaining_auction.is_none());

        // 300 blocks
        e.ledger().set(LedgerInfo {
            timestamp: 12345,
            protocol_version: 22,
            sequence_number: 1300,
            network_id: Default::default(),
            base_reserve: 10,
            min_temp_entry_ttl: 172800,
            min_persistent_entry_ttl: 172800,
            max_entry_ttl: 9999999,
        });
        let (scaled_auction, remaining_auction) = scale_auction(&e, &base_auction_data, 100);
        assert_eq!(
            scaled_auction.bid.get_unchecked(underlying_0.clone()),
            50_0000000
        );
        assert_eq!(
            scaled_auction.lot.get_unchecked(underlying_1.clone()),
            100_0000000
        );
        assert!(remaining_auction.is_none());

        // 400 blocks
        e.ledger().set(LedgerInfo {
            timestamp: 12345,
            protocol_version: 22,
            sequence_number: 1400,
            network_id: Default::default(),
            base_reserve: 10,
            min_temp_entry_ttl: 172800,
            min_persistent_entry_ttl: 172800,
            max_entry_ttl: 9999999,
        });
        let (scaled_auction, remaining_auction) = scale_auction(&e, &base_auction_data, 100);
        assert_eq!(scaled_auction.bid.len(), 0);
        assert_eq!(
            scaled_auction.lot.get_unchecked(underlying_1.clone()),
            100_0000000
        );
        assert!(remaining_auction.is_none());
    }

    #[test]
    fn test_scale_auction_not_100_fill_pct() {
        // @dev: bids always round up, lots always round down
        //       the remaining is exact based on scaled auction
        let e = Env::default();
        let underlying_0 = Address::generate(&e);
        let underlying_1 = Address::generate(&e);

        let base_auction_data = AuctionData {
            bid: map![&e, (underlying_0.clone(), 25_0000005)],
            lot: map![&e, (underlying_1.clone(), 25_0000005)],
            block: 1000,
        };

        // 0 blocks
        e.ledger().set(LedgerInfo {
            timestamp: 12345,
            protocol_version: 22,
            sequence_number: 1000,
            network_id: Default::default(),
            base_reserve: 10,
            min_temp_entry_ttl: 172800,
            min_persistent_entry_ttl: 172800,
            max_entry_ttl: 9999999,
        });
        let (scaled_auction, remaining_auction_option) = scale_auction(&e, &base_auction_data, 50);
        let remaining_auction = remaining_auction_option.unwrap();
        assert_eq!(
            scaled_auction.bid.get_unchecked(underlying_0.clone()),
            12_5000003 // fill pct rounds up
        );
        assert_eq!(scaled_auction.lot.len(), 0);
        assert_eq!(
            remaining_auction.bid.get_unchecked(underlying_0.clone()),
            12_5000002
        );
        assert_eq!(
            remaining_auction.lot.get_unchecked(underlying_1.clone()),
            12_5000003
        );

        // 100 blocks
        e.ledger().set(LedgerInfo {
            timestamp: 12345,
            protocol_version: 22,
            sequence_number: 1100,
            network_id: Default::default(),
            base_reserve: 10,
            min_temp_entry_ttl: 172800,
            min_persistent_entry_ttl: 172800,
            max_entry_ttl: 9999999,
        });

        let (scaled_auction, remaining_auction_option) = scale_auction(&e, &base_auction_data, 60);
        let remaining_auction = remaining_auction_option.unwrap();
        assert_eq!(
            scaled_auction.bid.get_unchecked(underlying_0.clone()),
            15_0000003
        );
        assert_eq!(
            scaled_auction.lot.get_unchecked(underlying_1.clone()),
            7_5000001 // modifier rounds down
        );
        assert_eq!(
            remaining_auction.bid.get_unchecked(underlying_0.clone()),
            10_0000002
        );
        assert_eq!(
            remaining_auction.lot.get_unchecked(underlying_1.clone()),
            10_0000002
        );

        // 300 blocks
        e.ledger().set(LedgerInfo {
            timestamp: 12345,
            protocol_version: 22,
            sequence_number: 1300,
            network_id: Default::default(),
            base_reserve: 10,
            min_temp_entry_ttl: 172800,
            min_persistent_entry_ttl: 172800,
            max_entry_ttl: 9999999,
        });

        let (scaled_auction, remaining_auction_option) = scale_auction(&e, &base_auction_data, 60);
        let remaining_auction = remaining_auction_option.unwrap();
        assert_eq!(
            scaled_auction.bid.get_unchecked(underlying_0.clone()),
            7_5000002 // modifier rounds up
        );
        assert_eq!(
            scaled_auction.lot.get_unchecked(underlying_1.clone()),
            15_0000003
        );
        assert_eq!(
            remaining_auction.bid.get_unchecked(underlying_0.clone()),
            10_0000002
        );
        assert_eq!(
            remaining_auction.lot.get_unchecked(underlying_1.clone()),
            10_0000002
        );

        // 400 blocks
        e.ledger().set(LedgerInfo {
            timestamp: 12345,
            protocol_version: 22,
            sequence_number: 1400,
            network_id: Default::default(),
            base_reserve: 10,
            min_temp_entry_ttl: 172800,
            min_persistent_entry_ttl: 172800,
            max_entry_ttl: 9999999,
        });
        let (scaled_auction, remaining_auction_option) = scale_auction(&e, &base_auction_data, 50);
        let remaining_auction = remaining_auction_option.unwrap();
        assert_eq!(scaled_auction.bid.len(), 0);
        assert_eq!(
            scaled_auction.lot.get_unchecked(underlying_1.clone()),
            12_5000002 // fill pct rounds down
        );
        assert_eq!(
            remaining_auction.bid.get_unchecked(underlying_0.clone()),
            12_5000002
        );
        assert_eq!(
            remaining_auction.lot.get_unchecked(underlying_1.clone()),
            12_5000003
        );
    }

    #[test]
    #[should_panic(expected = "Error(Contract, #1200)")]
    fn test_scale_auction_fill_percentage_zero() {
        let e = Env::default();
        let underlying_0 = Address::generate(&e);
        let underlying_1 = Address::generate(&e);

        let base_auction_data = AuctionData {
            bid: map![&e, (underlying_0.clone(), 25_0000005)],
            lot: map![&e, (underlying_1.clone(), 25_0000005)],
            block: 1000,
        };

        // 0 blocks
        e.ledger().set(LedgerInfo {
            timestamp: 12345,
            protocol_version: 22,
            sequence_number: 1000,
            network_id: Default::default(),
            base_reserve: 10,
            min_temp_entry_ttl: 172800,
            min_persistent_entry_ttl: 172800,
            max_entry_ttl: 9999999,
        });

        let (_, _) = scale_auction(&e, &base_auction_data, 0);
    }

    #[test]
    #[should_panic(expected = "Error(Contract, #1200)")]
    fn test_scale_auction_fill_percentage_over_100() {
        let e = Env::default();
        let underlying_0 = Address::generate(&e);
        let underlying_1 = Address::generate(&e);

        let base_auction_data = AuctionData {
            bid: map![&e, (underlying_0.clone(), 25_0000005)],
            lot: map![&e, (underlying_1.clone(), 25_0000005)],
            block: 1000,
        };

        // 0 blocks
        e.ledger().set(LedgerInfo {
            timestamp: 12345,
            protocol_version: 22,
            sequence_number: 1000,
            network_id: Default::default(),
            base_reserve: 10,
            min_temp_entry_ttl: 172800,
            min_persistent_entry_ttl: 172800,
            max_entry_ttl: 9999999,
        });

        let (_, _) = scale_auction(&e, &base_auction_data, 101);
    }

    #[test]
    fn test_scale_auction_dust() {
        // @dev: bids always round up, lots always round down
        //       the remaining is exact based on scaled auction
        let e = Env::default();
        let underlying_0 = Address::generate(&e);
        let underlying_1 = Address::generate(&e);

        let base_auction_data = AuctionData {
            bid: map![&e, (underlying_0.clone(), 0_0000001)],
            lot: map![&e, (underlying_1.clone(), 0_0000001)],
            block: 1000,
        };

        // 0 blocks
        e.ledger().set(LedgerInfo {
            timestamp: 12345,
            protocol_version: 22,
            sequence_number: 1000,
            network_id: Default::default(),
            base_reserve: 10,
            min_temp_entry_ttl: 172800,
            min_persistent_entry_ttl: 172800,
            max_entry_ttl: 9999999,
        });
        let (scaled_auction, remaining_auction_option) = scale_auction(&e, &base_auction_data, 99);
        assert_eq!(scaled_auction.bid.get_unchecked(underlying_0.clone()), 1);
        assert_eq!(scaled_auction.lot.len(), 0);
        let remaining_auction = remaining_auction_option.unwrap();
        assert_eq!(remaining_auction.bid.len(), 0);
        assert_eq!(remaining_auction.lot.get_unchecked(underlying_1.clone()), 1);

        // 100 blocks
        e.ledger().set(LedgerInfo {
            timestamp: 12345,
            protocol_version: 22,
            sequence_number: 1100,
            network_id: Default::default(),
            base_reserve: 10,
            min_temp_entry_ttl: 172800,
            min_persistent_entry_ttl: 172800,
            max_entry_ttl: 9999999,
        });

        let (scaled_auction, remaining_auction_option) = scale_auction(&e, &base_auction_data, 100);
        assert_eq!(scaled_auction.bid.get_unchecked(underlying_0.clone()), 1);
        assert_eq!(scaled_auction.lot.len(), 0);
        assert!(remaining_auction_option.is_none());

        let (scaled_auction, remaining_auction_option) = scale_auction(&e, &base_auction_data, 99);
        assert_eq!(scaled_auction.bid.get_unchecked(underlying_0.clone()), 1);
        assert_eq!(scaled_auction.lot.len(), 0);
        let remaining_auction = remaining_auction_option.unwrap();
        assert_eq!(remaining_auction.bid.len(), 0);
        assert_eq!(remaining_auction.lot.get_unchecked(underlying_1.clone()), 1);

        // 200 blocks
        e.ledger().set(LedgerInfo {
            timestamp: 12345,
            protocol_version: 22,
            sequence_number: 1200,
            network_id: Default::default(),
            base_reserve: 10,
            min_temp_entry_ttl: 172800,
            min_persistent_entry_ttl: 172800,
            max_entry_ttl: 9999999,
        });

        let (scaled_auction, remaining_auction_option) = scale_auction(&e, &base_auction_data, 99);
        assert_eq!(scaled_auction.bid.get_unchecked(underlying_0.clone()), 1);
        assert_eq!(scaled_auction.lot.len(), 0);
        let remaining_auction = remaining_auction_option.unwrap();
        assert_eq!(remaining_auction.bid.len(), 0);
        assert_eq!(remaining_auction.lot.get_unchecked(underlying_1.clone()), 1);

        // 300 blocks
        e.ledger().set(LedgerInfo {
            timestamp: 12345,
            protocol_version: 22,
            sequence_number: 1300,
            network_id: Default::default(),
            base_reserve: 10,
            min_temp_entry_ttl: 172800,
            min_persistent_entry_ttl: 172800,
            max_entry_ttl: 9999999,
        });

        let (scaled_auction, remaining_auction_option) = scale_auction(&e, &base_auction_data, 100);
        assert_eq!(scaled_auction.bid.get_unchecked(underlying_0.clone()), 1);
        assert_eq!(scaled_auction.lot.get_unchecked(underlying_1.clone()), 1);
        assert!(remaining_auction_option.is_none());

        let (scaled_auction, remaining_auction_option) = scale_auction(&e, &base_auction_data, 99);
        assert_eq!(scaled_auction.bid.get_unchecked(underlying_0.clone()), 1);
        assert_eq!(scaled_auction.lot.len(), 0);
        let remaining_auction = remaining_auction_option.unwrap();
        assert_eq!(remaining_auction.bid.len(), 0);
        assert_eq!(remaining_auction.lot.get_unchecked(underlying_1.clone()), 1);

        // 399 blocks
        e.ledger().set(LedgerInfo {
            timestamp: 12345,
            protocol_version: 22,
            sequence_number: 1399,
            network_id: Default::default(),
            base_reserve: 10,
            min_temp_entry_ttl: 172800,
            min_persistent_entry_ttl: 172800,
            max_entry_ttl: 9999999,
        });
        let (scaled_auction, remaining_auction_option) = scale_auction(&e, &base_auction_data, 99);
        assert_eq!(scaled_auction.bid.get_unchecked(underlying_0.clone()), 1);
        assert_eq!(scaled_auction.lot.len(), 0);
        let remaining_auction = remaining_auction_option.unwrap();
        assert_eq!(remaining_auction.bid.len(), 0);
        assert_eq!(remaining_auction.lot.get_unchecked(underlying_1.clone()), 1);

        // 400 blocks
        e.ledger().set(LedgerInfo {
            timestamp: 12345,
            protocol_version: 22,
            sequence_number: 1400,
            network_id: Default::default(),
            base_reserve: 10,
            min_temp_entry_ttl: 172800,
            min_persistent_entry_ttl: 172800,
            max_entry_ttl: 9999999,
        });
        let (scaled_auction, remaining_auction_option) = scale_auction(&e, &base_auction_data, 99);
        assert_eq!(scaled_auction.bid.len(), 0);
        assert_eq!(scaled_auction.lot.len(), 0);
        let remaining_auction = remaining_auction_option.unwrap();
        assert_eq!(remaining_auction.bid.len(), 0);
        assert_eq!(remaining_auction.lot.get_unchecked(underlying_1.clone()), 1);

        // with 100 fill pct
        let (scaled_auction, remaining_auction_option) = scale_auction(&e, &base_auction_data, 100);
        assert_eq!(scaled_auction.bid.len(), 0);
        assert_eq!(scaled_auction.lot.get_unchecked(underlying_1.clone()), 1);
        assert!(remaining_auction_option.is_none());
    }

    #[test]
    #[should_panic(expected = "Error(Contract, #1200)")]
    fn test_fill_rejects_existing_bad_debt_auction() {
        let e = Env::default();
        e.mock_all_auths();

        let bombadil = Address::generate(&e);
        let samwise = Address::generate(&e);
        let frodo = Address::generate(&e);
        let pool_address = create_pool(&e);
        let oracle_id = Address::generate(&e);
        let (underlying_0, _) = testutils::create_token_contract(&e, &bombadil);
        let (underlying_1, _) = testutils::create_token_contract(&e, &bombadil);

        // seed a bad-debt auction directly so only the new fill guard can reject the call
        let auction_data = AuctionData {
            bid: map![&e, (underlying_0.clone(), 1)],
            lot: map![&e, (underlying_1.clone(), 1)],
            block: 10,
        };
        let pool_config = PoolConfig {
            oracle: oracle_id,
            min_collateral: 1_0000000,
            bstop_rate: 0,
            status: 0,
            max_positions: 4,
        };
        e.as_contract(&pool_address, || {
            storage::set_pool_config(&e, &pool_config);
            storage::set_auction(&e, &1, &samwise, &auction_data);
            assert!(storage::has_auction(&e, &1, &samwise));
            let mut pool = Pool::load(&e);
            let mut frodo_state = User::load(&e, &frodo);

            fill(&e, &mut pool, 1, &samwise, &mut frodo_state, 100);
        });
    }

    #[test]
    #[should_panic(expected = "Error(Contract, #1200)")]
    fn test_fill_rejects_existing_interest_auction() {
        let e = Env::default();
        e.mock_all_auths();

        let bombadil = Address::generate(&e);
        let samwise = Address::generate(&e);
        let frodo = Address::generate(&e);
        let pool_address = create_pool(&e);
        let oracle_id = Address::generate(&e);
        let (underlying_0, _) = testutils::create_token_contract(&e, &bombadil);
        let (underlying_1, _) = testutils::create_token_contract(&e, &bombadil);

        // seed an interest auction directly so only the new fill guard can reject the call
        let auction_data = AuctionData {
            bid: map![&e, (underlying_0.clone(), 1)],
            lot: map![&e, (underlying_1.clone(), 1)],
            block: 10,
        };
        let pool_config = PoolConfig {
            oracle: oracle_id,
            min_collateral: 1_0000000,
            bstop_rate: 0,
            status: 0,
            max_positions: 4,
        };
        e.as_contract(&pool_address, || {
            storage::set_pool_config(&e, &pool_config);
            storage::set_auction(&e, &2, &samwise, &auction_data);
            assert!(storage::has_auction(&e, &2, &samwise));
            let mut pool = Pool::load(&e);
            let mut frodo_state = User::load(&e, &frodo);

            fill(&e, &mut pool, 2, &samwise, &mut frodo_state, 100);
        });
    }
}
