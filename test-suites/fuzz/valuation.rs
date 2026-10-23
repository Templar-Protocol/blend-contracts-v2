//! Exact arithmetic is only a representability preflight. Evaluable inputs still
//! execute the production conversions; no panic is caught or classified here.
use num_bigint::BigInt;
use num_integer::Integer;
use num_traits::ToPrimitive;
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct NumericLimit {
    pub stage: &'static str,
    pub exact_result: String,
}

#[derive(Clone, Copy)]
pub enum Rounding {
    Floor,
    Ceil,
}

pub fn checked_conversion(
    stage: &'static str,
    x: i128,
    y: i128,
    denominator: i128,
    rounding: Rounding,
    production: impl FnOnce() -> i128,
) -> Result<i128, NumericLimit> {
    // Zero divisors are logic/configuration errors, never numeric-range skips.
    assert_ne!(denominator, 0, "zero divisor in {stage}");
    let product = BigInt::from(x) * BigInt::from(y);
    let denominator = BigInt::from(denominator);
    let exact = match rounding {
        Rounding::Floor => product.div_floor(&denominator),
        Rounding::Ceil => product.div_ceil(&denominator),
    };
    let expected = exact.to_i128().ok_or_else(|| NumericLimit {
        stage,
        exact_result: exact.to_string(),
    })?;
    let actual = production();
    assert_eq!(actual, expected, "production valuation mismatch in {stage}");
    Ok(actual)
}

pub fn checked_sum(stage: &'static str, x: i128, y: i128) -> Result<i128, NumericLimit> {
    x.checked_add(y).ok_or_else(|| NumericLimit {
        stage,
        exact_result: (BigInt::from(x) + BigInt::from(y)).to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use soroban_fixed_point_math::SorobanFixedPoint;
    use soroban_sdk::Env;

    fn convert(x: i128, y: i128, d: i128, rounding: Rounding) -> Result<i128, NumericLimit> {
        let env = Env::default();
        checked_conversion("test", x, y, d, rounding, || match rounding {
            Rounding::Floor => x.fixed_mul_floor(&env, &y, &d),
            Rounding::Ceil => x.fixed_mul_ceil(&env, &y, &d),
        })
    }

    #[test]
    fn wide_product_with_representable_result_executes_production() {
        assert_eq!(
            convert(i128::MAX, 2, i128::MAX - 1, Rounding::Floor).unwrap(),
            2
        );
        assert_eq!(
            convert(i128::MAX, 2, i128::MAX - 1, Rounding::Ceil).unwrap(),
            3
        );
        assert_eq!(
            convert(-i128::MAX, 2, i128::MAX - 1, Rounding::Floor).unwrap(),
            -3
        );
        assert_eq!(
            convert(-i128::MAX, 2, i128::MAX - 1, Rounding::Ceil).unwrap(),
            -2
        );
        assert_eq!(
            convert(i128::MIN, 1, 1, Rounding::Floor).unwrap(),
            i128::MIN
        );
        assert_eq!(convert(i128::MAX, 1, 1, Rounding::Ceil).unwrap(), i128::MAX);
    }

    #[test]
    fn only_exact_out_of_range_results_skip_production() {
        let limit = checked_conversion("overflow", i128::MAX, 2, 1, Rounding::Floor, || {
            panic!("unrepresentable conversion must not be invoked")
        })
        .unwrap_err();
        assert_eq!(
            limit.exact_result,
            (BigInt::from(i128::MAX) * BigInt::from(2)).to_string()
        );
        assert!(convert(i128::MIN, 1, -1, Rounding::Ceil).is_err());
        assert!(checked_sum("sum", i128::MAX, 1).is_err());
        assert!(checked_sum("sum", i128::MIN, -1).is_err());
    }

    #[test]
    fn signed_rounding_and_zero_match_production() {
        for (x, y, d, floor, ceil) in [
            (3, 1, 2, 1, 2),
            (-3, 1, 2, -2, -1),
            (3, 1, -2, -2, -1),
            (-3, 1, -2, 1, 2),
            (0, i128::MAX, 7, 0, 0),
            (4, -1, 2, -2, -2),
        ] {
            assert_eq!(convert(x, y, d, Rounding::Floor).unwrap(), floor);
            assert_eq!(convert(x, y, d, Rounding::Ceil).unwrap(), ceil);
        }
    }

    #[test]
    fn unexpected_panics_and_zero_divisors_are_not_skips() {
        assert!(std::panic::catch_unwind(|| {
            checked_conversion("unexpected host failure", 1, 1, 1, Rounding::Floor, || {
                panic!("host or logic error")
            })
        })
        .is_err());
        assert!(std::panic::catch_unwind(|| convert(1, 1, 0, Rounding::Floor)).is_err());
        assert!(std::panic::catch_unwind(|| {
            checked_conversion("wrong rounding", 3, 1, 2, Rounding::Floor, || 2)
        })
        .is_err());
    }
}
