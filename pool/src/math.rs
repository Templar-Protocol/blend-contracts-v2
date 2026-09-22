//! Arithmetic boundary for shared lending kernels, not a replacement arithmetic model.
//!
//! Production keeps SorobanFixedPoint, including its I256 fallback. Kani uses the
//! same dependency's checked i128 operations only on each harness's product-fit
//! domain. The calling kernels, including rounding direction, are shared.
use soroban_fixed_point_math::SorobanFixedPoint;
use soroban_sdk::Env;

pub(crate) trait FixedMath {
    fn floor(&self, x: i128, y: i128, denominator: i128) -> i128;
    fn ceil(&self, x: i128, y: i128, denominator: i128) -> i128;
}

impl FixedMath for Env {
    fn floor(&self, x: i128, y: i128, denominator: i128) -> i128 {
        SorobanFixedPoint::fixed_mul_floor(&x, self, &y, &denominator)
    }

    fn ceil(&self, x: i128, y: i128, denominator: i128) -> i128 {
        SorobanFixedPoint::fixed_mul_ceil(&x, self, &y, &denominator)
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rounding_keeps_i256_fallback() {
        let e = Env::default();
        // The product overflows i128, but both rounded results are representable.
        assert_eq!(FixedMath::floor(&e, i128::MAX, 2, i128::MAX - 1), 2);
        assert_eq!(FixedMath::ceil(&e, i128::MAX, 2, i128::MAX - 1), 3);
    }
}
