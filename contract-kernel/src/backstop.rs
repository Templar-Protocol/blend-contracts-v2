const SCALAR_7: i128 = 10_000_000;
const THRESHOLD_PRODUCT: i128 = 10_000_000_000_000_000_000_000_000;

pub const MAX_BACKFILLED_EMISSIONS: i128 = 10_000_000 * SCALAR_7;

// Kani 0.68.0 panics in rvalue.rs:1009 when decoding the niche-encoded
// `Result<QueueStep, NotExpired>` discriminant. This representation is proof-only;
// production and Wasm keep Rust's normal representation.
#[cfg_attr(kani, repr(C))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum QueueStep {
    Partial { entry_remaining: i128 },
    Exact,
    Continue { requested_remaining: i128 },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NotExpired;

pub const fn above_threshold(blnd: i128, usdc: i128) -> bool {
    threshold_values(blnd, usdc).1
}

pub(crate) const fn threshold_values(blnd: i128, usdc: i128) -> (i128, bool) {
    let whole_blnd = blnd / SCALAR_7;
    let whole_usdc = usdc / SCALAR_7;
    let product = whole_blnd
        .saturating_mul(whole_blnd)
        .saturating_mul(whole_blnd)
        .saturating_mul(whole_blnd)
        .saturating_mul(whole_usdc);
    (
        product.saturating_mul(SCALAR_7) / THRESHOLD_PRODUCT,
        product >= THRESHOLD_PRODUCT,
    )
}

pub const fn consume_queue_entry(entry_amount: i128, requested: i128) -> QueueStep {
    if entry_amount > requested {
        QueueStep::Partial {
            entry_remaining: entry_amount - requested,
        }
    } else if entry_amount == requested {
        QueueStep::Exact
    } else {
        QueueStep::Continue {
            requested_remaining: requested - entry_amount,
        }
    }
}

pub const fn withdraw_queue_entry(
    entry_amount: i128,
    expires: u64,
    now: u64,
    requested: i128,
) -> Result<QueueStep, NotExpired> {
    if expires > now {
        Err(NotExpired)
    } else {
        Ok(consume_queue_entry(entry_amount, requested))
    }
}

pub const fn cap_backfill(current: i128, requested: i128) -> Option<(i128, i128)> {
    if current >= MAX_BACKFILLED_EMISSIONS {
        None
    } else {
        let allocated = if requested + current > MAX_BACKFILLED_EMISSIONS {
            MAX_BACKFILLED_EMISSIONS - current
        } else {
            requested
        };
        Some((allocated, current + allocated))
    }
}

