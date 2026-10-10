use crate::{dependencies::PoolBackstopData, spec::GHOST_POOL_BACKSTOP_DATA};

/// Replaces the cross-contract `BackstopClient::pool_data` read with an
/// unconstrained value, recording it so the status rules can refer to it.
pub fn pool_data_summary() -> PoolBackstopData {
    let pool_data = PoolBackstopData {
        tokens: cvlr::nondet(),
        shares: cvlr::nondet(),
        q4w_pct: cvlr::nondet(),
        blnd: cvlr::nondet(),
        usdc: cvlr::nondet(),
        token_spot_price: cvlr::nondet(),
    };
    unsafe { GHOST_POOL_BACKSTOP_DATA = pool_data.clone() };
    pool_data
}
