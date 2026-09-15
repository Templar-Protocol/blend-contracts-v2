use crate::constants::SCALAR_7;

// Both crate instantiations must use the same raw-balance scale.
const _: () = assert!(SCALAR_7 == 1_0000000);

pub(crate) fn saturating_backstop_product(blnd: i128, usdc: i128) -> i128 {
    // Floor balances before the original saturating product, in the original order.
    let bal_blnd = blnd / SCALAR_7;
    let bal_usdc = usdc / SCALAR_7;
    bal_blnd
        .saturating_mul(bal_blnd)
        .saturating_mul(bal_blnd)
        .saturating_mul(bal_blnd)
        .saturating_mul(bal_usdc)
}
