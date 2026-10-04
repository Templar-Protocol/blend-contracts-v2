//! Certora Sunbeam specification: rules, summaries, and the ghost state they read.
//!
//! Ported from the auditor's harness at Certora/blend-contracts-v2@certora, which
//! verified the June 2025 Formal Verification report against upstream Blend v2 at
//! `996e09e`. Everything that depends on `cvlr` is gated behind the `certora`
//! feature so a production build resolves the upstream dependency graph.

#[cfg(feature = "certora")]
pub mod token;
#[cfg(feature = "certora")]
pub(crate) mod summaries;
#[cfg(feature = "certora")]
pub(crate) mod model;
#[cfg(feature = "certora")]
pub(crate) mod health_rules;
#[cfg(feature = "certora")]
pub(crate) mod user_rules;
#[cfg(feature = "certora")]
pub(crate) mod interest_rules;
#[cfg(feature = "certora")]
pub(crate) mod pool_status_rules;

/// Replace an expression the prover cannot reason about with an unconstrained
/// value, optionally havocking the places it would have written.
///
/// Without the `certora` feature this expands to the original expression, so
/// production behaviour and the production dependency graph are unchanged.
#[macro_export]
macro_rules! nondet_expr {
    ( $m:expr ) => {{
        #[cfg(feature = "certora")]
        let nondet_value = ::cvlr::nondet::nondet();
        #[cfg(not(feature = "certora"))]
        let nondet_value = $m;
        nondet_value
    }};
    ( $($e:expr ),* ; $m:expr ) => {{
        #[cfg(feature = "certora")]
        let nondet_value = {
            $( $e = ::cvlr::nondet::nondet(); )*
            ::cvlr::nondet::nondet()
        };
        #[cfg(not(feature = "certora"))]
        let nondet_value = $m;
        nondet_value
    }};
}

/// Stand-in for `cvlr_soroban_macros::apply_summary` when the `certora` feature
/// is off and that optional crate is absent. The upstream macro emits the
/// unmodified function in exactly this case, so the two agree on production
/// builds and the summarized bodies are never duplicated here.
#[cfg(not(feature = "certora"))]
#[macro_export]
macro_rules! apply_summary {
    (
        $new:path,
        $( #[$meta:meta] )*
        $vis:vis fn $id:ident ($($arg:ident : $arg_ty:ty),* $(,)?) $( -> $ret:ty )?
        $body:block
    ) => {
        $( #[$meta] )*
        $vis fn $id($($arg : $arg_ty),*) $( -> $ret )? $body
    };
}

#[cfg(feature = "certora")]
use crate::dependencies::PoolBackstopData;

#[cfg(feature = "certora")]
pub(crate) static mut GHOST_POOL_BACKSTOP_DATA: PoolBackstopData = PoolBackstopData {
    tokens: 0,
    shares: 0,
    q4w_pct: 0,
    blnd: 0,
    usdc: 0,
    token_spot_price: 0,
};

#[cfg(feature = "certora")]
pub(crate) static mut GHOST_MET_THRESHOLD: bool = false;
