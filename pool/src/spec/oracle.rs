//! Model of the SEP-40 price feed reads in `Pool::load_price` and
//! `Pool::load_price_decimals`.
//!
//! The feed is replaced at the call site rather than by a mock client, so a
//! rule can both choose what the oracle reports and inspect it afterwards.
//! Same shape as `summaries::backstop_pooldata`, which replaces the backstop
//! client read in `execute_update_pool_status`.

use sep_40_oracle::PriceData;

/// The staleness window `load_price` enforces, in seconds.
pub(crate) const MAX_PRICE_AGE: u64 = 86_400;

/// The decimal count `load_price_decimals` requires of the feed.
pub(crate) const REQUIRED_DECIMALS: u32 = 7;

/// What the modelled feed reports. A rule assigns these before driving the
/// pool, so the oracle's answer is under the rule's control rather than
/// resampled at each call.
pub(crate) static mut GHOST_PRICE: i128 = 0;
pub(crate) static mut GHOST_TIMESTAMP: u64 = 0;
pub(crate) static mut GHOST_HAS_PRICE: bool = false;
pub(crate) static mut GHOST_DECIMALS: u32 = REQUIRED_DECIMALS;

/// Set the feed to report an arbitrary, unconstrained quote.
pub(crate) fn havoc_feed() {
    unsafe {
        GHOST_PRICE = cvlr::nondet();
        GHOST_TIMESTAMP = cvlr::nondet();
        GHOST_HAS_PRICE = cvlr::nondet();
        GHOST_DECIMALS = cvlr::nondet();
    }
}

pub(crate) fn feed_price() -> i128 {
    unsafe { GHOST_PRICE }
}

pub(crate) fn feed_timestamp() -> u64 {
    unsafe { GHOST_TIMESTAMP }
}

pub(crate) fn feed_has_price() -> bool {
    unsafe { GHOST_HAS_PRICE }
}

pub(crate) fn feed_decimals() -> u32 {
    unsafe { GHOST_DECIMALS }
}

/// Replaces `PriceFeedClient::lastprice`.
pub(crate) fn lastprice_summary() -> Option<PriceData> {
    if feed_has_price() {
        Some(PriceData {
            price: feed_price(),
            timestamp: feed_timestamp(),
        })
    } else {
        None
    }
}

/// Replaces `PriceFeedClient::decimals`.
pub(crate) fn decimals_summary() -> u32 {
    feed_decimals()
}

/// The quote the guard in `load_price` is required to accept: present,
/// strictly positive, not future-dated and within `MAX_PRICE_AGE` of `now`.
pub(crate) fn quote_acceptable(now: u64) -> bool {
    feed_has_price()
        && feed_price() > 0
        && feed_timestamp() <= now
        && now - feed_timestamp() <= MAX_PRICE_AGE
}
