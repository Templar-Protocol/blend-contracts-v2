use super::*;

/// Safety of the one shared production prefix over the full nonnegative i128 raw-balance
/// domain: the single saturating product cannot panic on overflow or division and stays
/// nonnegative. Covers witness the zero-floor and saturation boundaries. Cross-crate prefix
/// identity rests on the shared source definition and literal caller forwarding, not on a
/// two-copy equality miter.
#[kani::proof]
fn prove_shared_threshold_prefix_safety() {
    let blnd: i128 = kani::any();
    let usdc: i128 = kani::any();
    kani::assume(blnd >= 0 && usdc >= 0);
    let product = saturating_backstop_product(blnd, usdc);
    assert!(product >= 0);
    // Zero boundary: sub-scalar raw balances floor to zero units and collapse the product.
    kani::cover!(blnd < SCALAR_7 && product == 0);
    kani::cover!(usdc < SCALAR_7 && product == 0);
    // Unit boundary: the smallest raw balances yielding a nonzero product.
    kani::cover!(blnd == SCALAR_7 && usdc == SCALAR_7 && product == 1);
    // Saturation boundary: raw balances beyond representability pin the product at i128::MAX.
    kani::cover!(blnd == i128::MAX && usdc == SCALAR_7 && product == i128::MAX);
}

#[kani::proof]
fn prove_threshold_scale_agreement() {
    let product: i128 = kani::any();
    assert_eq!(
        threshold_from_product(product) >= SCALAR_7,
        backstop::threshold_from_product(product)
    );
    kani::cover!(product == 0);
    kani::cover!(product == 10_000_000_000_000_000_000_000_000i128 - 1);
    kani::cover!(product == 10_000_000_000_000_000_000_000_000i128);
    kani::cover!(product == i128::MAX);
    kani::cover!(product == -1);
    kani::cover!(product == i128::MIN);
}
