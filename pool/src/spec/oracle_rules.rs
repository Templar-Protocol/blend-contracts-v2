//! Oracle circuit breaker.
//!
//! `Pool::load_price` must reject a quote that is absent, non-positive,
//! future-dated, or older than `MAX_PRICE_AGE`, and `load_price_decimals`
//! must reject a feed whose decimals are not `REQUIRED_DECIMALS`. These rules
//! establish both directions: a rejected quote always panics, and a quote the
//! guard accepts was necessarily acceptable.
//!
//! The "always panics" shape (`cvlr_assert!(false)` after the call) is the
//! auditor's, as used by `verify_update_status_6`.

use cvlr::asserts::{cvlr_assert, cvlr_assume, cvlr_satisfy};
use cvlr_soroban::nondet_address;
use cvlr_soroban_derive::rule;
use soroban_sdk::Env;

use crate::pool::Pool;
use crate::spec::oracle::{
    feed_decimals, feed_has_price, feed_price, feed_timestamp, havoc_feed, quote_acceptable,
    MAX_PRICE_AGE, REQUIRED_DECIMALS,
};
use crate::storage::{self, PoolConfig};

/// Install an arbitrary pool config and an arbitrary oracle quote, then hand
/// back a `Pool` with an empty price cache so `load_price` must consult the
/// feed rather than return a cached value.
fn setup(e: &Env) -> Pool {
    let pool_config: PoolConfig = cvlr::nondet();
    storage::set_pool_config(e, &pool_config);
    havoc_feed();
    Pool::load(e)
}

/// A quote the guard accepts is present, positive, not future-dated and
/// within the staleness window. This is the circuit breaker stated as a
/// postcondition, and it covers all four rejection cases at once.
#[rule]
pub fn price_accepted_implies_quote_acceptable(e: Env) {
    let asset = nondet_address();
    let mut pool = setup(&e);

    let now = e.ledger().timestamp();
    let price = pool.load_price(&e, &asset);

    cvlr_assert!(quote_acceptable(now));
    // The value handed back is the one the feed reported.
    cvlr_assert!(price == feed_price());
    cvlr_assert!(price > 0);
}

/// An absent quote always panics.
#[rule]
pub fn missing_price_panics(e: Env) {
    let asset = nondet_address();
    let mut pool = setup(&e);
    cvlr_assume!(!feed_has_price());

    pool.load_price(&e, &asset);

    cvlr_assert!(false);
}

/// A non-positive quote always panics.
#[rule]
pub fn non_positive_price_panics(e: Env) {
    let asset = nondet_address();
    let mut pool = setup(&e);
    cvlr_assume!(feed_has_price() && feed_price() <= 0);

    pool.load_price(&e, &asset);

    cvlr_assert!(false);
}

/// A future-dated quote always panics.
#[rule]
pub fn future_dated_price_panics(e: Env) {
    let asset = nondet_address();
    let mut pool = setup(&e);
    let now = e.ledger().timestamp();
    cvlr_assume!(feed_has_price() && feed_timestamp() > now);

    pool.load_price(&e, &asset);

    cvlr_assert!(false);
}

/// A quote older than the staleness window always panics.
#[rule]
pub fn stale_price_panics(e: Env) {
    let asset = nondet_address();
    let mut pool = setup(&e);
    let now = e.ledger().timestamp();
    cvlr_assume!(feed_has_price() && feed_timestamp() <= now);
    cvlr_assume!(now - feed_timestamp() > MAX_PRICE_AGE);

    pool.load_price(&e, &asset);

    cvlr_assert!(false);
}

/// A feed reporting decimals other than the required count always panics.
#[rule]
pub fn wrong_decimals_panics(e: Env) {
    let mut pool = setup(&e);
    cvlr_assume!(feed_decimals() != REQUIRED_DECIMALS);

    pool.load_price_decimals(&e);

    cvlr_assert!(false);
}

// Vacuity checks. Each `*_panics` rule above asserts `false`, so it passes
// whenever its assumptions are unsatisfiable. These confirm the corresponding
// preconditions are reachable, which is what makes those rules meaningful.

#[rule]
pub fn sanity_acceptable_quote_reachable(e: Env) {
    let _pool = setup(&e);
    let now = e.ledger().timestamp();
    cvlr_satisfy!(quote_acceptable(now));
}

#[rule]
pub fn sanity_missing_price_reachable(e: Env) {
    let _pool = setup(&e);
    cvlr_satisfy!(!feed_has_price());
}

#[rule]
pub fn sanity_non_positive_price_reachable(e: Env) {
    let _pool = setup(&e);
    cvlr_satisfy!(feed_has_price() && feed_price() <= 0);
}

#[rule]
pub fn sanity_future_dated_reachable(e: Env) {
    let _pool = setup(&e);
    let now = e.ledger().timestamp();
    cvlr_satisfy!(feed_has_price() && feed_timestamp() > now);
}

#[rule]
pub fn sanity_stale_price_reachable(e: Env) {
    let _pool = setup(&e);
    let now = e.ledger().timestamp();
    cvlr_satisfy!(feed_has_price() && feed_timestamp() <= now && now - feed_timestamp() > MAX_PRICE_AGE);
}

#[rule]
pub fn sanity_wrong_decimals_reachable(e: Env) {
    let _pool = setup(&e);
    cvlr_satisfy!(feed_decimals() != REQUIRED_DECIMALS);
}

/// `load_price` can also return normally, so the postcondition rule is not
/// passing merely because every path panics.
#[rule]
pub fn sanity_price_load_can_succeed(e: Env) {
    let asset = nondet_address();
    let mut pool = setup(&e);
    let price = pool.load_price(&e, &asset);
    cvlr_satisfy!(price > 0);
}
