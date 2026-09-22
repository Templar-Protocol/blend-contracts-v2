use super::*;

impl FixedMath for () {
    fn floor(&self, x: i128, y: i128, denominator: i128) -> i128 {
        soroban_fixed_point_math::FixedPoint::fixed_mul_floor(x, y, denominator).unwrap()
    }

    fn ceil(&self, x: i128, y: i128, denominator: i128) -> i128 {
        soroban_fixed_point_math::FixedPoint::fixed_mul_ceil(x, y, denominator).unwrap()
    }
}
