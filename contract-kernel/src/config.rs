pub const MAX_POSITIONS: u32 = 60;

pub const fn valid_pool_config(take_rate: u32, max_positions: u32, min_collateral: i128) -> bool {
    take_rate < 10_000_000
        && max_positions >= 2
        && max_positions <= MAX_POSITIONS
        && min_collateral >= 0
}
