const SCALAR_7: i128 = 10_000_000;
const Q4W_ON_ICE: i128 = 3_000_000;
const Q4W_ADMIN_ON_ICE: i128 = 5_000_000;
const Q4W_FROZEN: i128 = 6_000_000;
const Q4W_ADMIN_FROZEN: i128 = 7_500_000;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StatusError {
    BadRequest,
    StatusNotAllowed,
}

pub const fn next_status(
    current: u32,
    q4w_pct: i128,
    met_threshold: bool,
) -> Result<u32, StatusError> {
    match current {
        4 | 6 => Err(StatusError::StatusNotAllowed),
        2 => {
            if q4w_pct >= Q4W_ADMIN_FROZEN {
                Ok(5)
            } else {
                Ok(2)
            }
        }
        0 => {
            if !met_threshold || q4w_pct >= Q4W_ADMIN_ON_ICE {
                Ok(3)
            } else {
                Ok(0)
            }
        }
        _ => {
            if q4w_pct >= Q4W_FROZEN {
                Ok(5)
            } else if q4w_pct >= Q4W_ON_ICE || !met_threshold {
                Ok(3)
            } else {
                Ok(1)
            }
        }
    }
}

pub const fn admin_status(
    requested: u32,
    q4w_pct: i128,
    met_threshold: bool,
) -> Result<u32, StatusError> {
    match requested {
        0 => {
            if met_threshold && q4w_pct < Q4W_ADMIN_ON_ICE {
                Ok(0)
            } else {
                Err(StatusError::StatusNotAllowed)
            }
        }
        2 | 3 => {
            if q4w_pct < Q4W_ADMIN_FROZEN {
                Ok(requested)
            } else {
                Err(StatusError::StatusNotAllowed)
            }
        }
        4 => Ok(4),
        _ => Err(StatusError::BadRequest),
    }
}

pub const fn action_allowed(status: u32, request_type: u32) -> bool {
    !((status > 1 && (request_type == 4 || request_type == 9))
        || (status > 3 && (request_type == 0 || request_type == 2)))
}

pub const fn reserve_action_allowed(enabled: bool, request_type: u32) -> bool {
    enabled || !(request_type == 0 || request_type == 2 || request_type == 4)
}

pub const fn auction_modifiers(elapsed_blocks: u32) -> (i128, i128) {
    if elapsed_blocks <= 200 {
        (SCALAR_7, elapsed_blocks as i128 * 50_000)
    } else if elapsed_blocks < 400 {
        (SCALAR_7 - (elapsed_blocks as i128 - 200) * 50_000, SCALAR_7)
    } else {
        (0, SCALAR_7)
    }
}

pub const fn backstop_threshold(blnd: i128, usdc: i128) -> i128 {
    crate::backstop::threshold_values(blnd, usdc).0
}
