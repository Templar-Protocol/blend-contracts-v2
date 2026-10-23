// Mathematical-model feasibility probe, not a theorem about production Rust.
// Mirrors reserve.rs::{to_asset_from_b_token,to_b_token_down,to_b_token_up}.
// All quantities here are unbounded mathematical integers. In particular, this
// file does not establish i128/I256 dispatch, overflow, host behavior, or callers.
// The pinned local acceptance command verified nine proof functions and detected
// the intended ceil-mint mutation. No numeric grid restricts a lemma.
use vstd::arithmetic::div_mod::*;
use vstd::arithmetic::mul::*;
use vstd::prelude::*;

verus! {

pub open spec fn q() -> int { 1_000_000_000_000 }

pub open spec fn claims(b: int, r: int) -> int { b * r / q() }

pub open spec fn mint_down(a: int, r: int) -> int { a * q() / r }

// For nonnegative n and positive d, Euclidean quotient plus a nonzero-
// remainder adjustment is exactly mathematical ceil(n/d), including n == 0.
pub open spec fn ceil_div(n: int, d: int) -> int {
    n / d + if n % d == 0 { 0int } else { 1int }
}

pub open spec fn burn_up(a: int, r: int) -> int { ceil_div(a * q(), r) }

// Keep each arithmetic context small: quotient/remainder decomposition first,
// then multiplication identities, then ordered division by a positive divisor.
proof fn floor_add_integer(x: int, k: int, d: int)
    requires d > 0,
    ensures (x + k * d) / d == x / d + k,
{
    lemma_fundamental_div_mod(x, d);
    lemma_mod_bound(x, d);
    lemma_mul_is_commutative(d, x / d);
    lemma_mul_is_distributive_add_other_way(d, x / d, k);
    assert(x + k * d == (x / d + k) * d + x % d);
    lemma_fundamental_div_mod_converse_div(x + k * d, d, x / d + k, x % d);
}

proof fn floor_product_bounds(n: int, d: int)
    requires n >= 0, d > 0,
    ensures n / d >= 0, (n / d) * d <= n,
{
    lemma_div_pos_is_pos(n, d);
    lemma_fundamental_div_mod(n, d);
    lemma_mod_bound(n, d);
    lemma_mul_is_commutative(d, n / d);
}

proof fn ceil_product_bounds(n: int, d: int)
    requires n >= 0, d > 0,
    ensures ceil_div(n, d) >= 0, n <= ceil_div(n, d) * d,
{
    floor_product_bounds(n, d);
    lemma_fundamental_div_mod(n, d);
    lemma_mod_bound(n, d);
    lemma_mul_is_commutative(d, n / d);
    if n % d != 0 {
        lemma_mul_is_distributive_add_other_way(d, n / d, 1);
        lemma_mul_basics(d);
        assert(n <= (n / d + 1) * d);
    }
}

// floor(x/d) + floor(y/d) <= floor((x+y)/d), with no bounds on
// the magnitudes (or even signs) of x and y.
proof fn floor_superadditive(x: int, y: int, d: int)
    requires d > 0,
    ensures x / d + y / d <= (x + y) / d,
{
    lemma_fundamental_div_mod(x, d);
    lemma_fundamental_div_mod(y, d);
    lemma_mod_bound(x, d);
    lemma_mod_bound(y, d);
    lemma_mul_is_distributive_add(d, x / d, y / d);
    assert(d * (x / d + y / d) <= x + y);
    lemma_div_is_ordered(d * (x / d + y / d), x + y, d);
    lemma_div_multiples_vanish(x / d + y / d, d);
}

// Supplying A assets mints floor(A*Q/R) shares. Aggregate supplier
// claims grow by a nonnegative amount no greater than the assets received.
pub proof fn supply_delta(b: int, a: int, r: int)
    requires b >= 0, a >= 0, r > 0,
    ensures
        0 <= claims(b + mint_down(a, r), r) - claims(b, r),
        claims(b + mint_down(a, r), r) - claims(b, r) <= a,
{
    lemma_mul_inequality(0, a, q());
    floor_product_bounds(a * q(), r);
    let m = mint_down(a, r);
    assert(m >= 0);
    assert(m * r <= a * q());
    lemma_mul_inequality(b, b + m, r);
    lemma_div_is_ordered(b * r, (b + m) * r, q());
    lemma_mul_is_distributive_add_other_way(r, b, m);
    assert((b + m) * r <= b * r + a * q());
    lemma_div_is_ordered((b + m) * r, b * r + a * q(), q());
    floor_add_integer(b * r, a, q());
}

// An uncapped withdrawal burns ceil(A*Q/R) shares. The sufficient-
// shares premise is the exact uncapped-path premise, not a numeric limit.
pub proof fn uncapped_withdrawal(b: int, a: int, r: int)
    requires b >= 0, a >= 0, r > 0, burn_up(a, r) <= b,
    ensures
        a <= claims(b, r) - claims(b - burn_up(a, r), r),
        claims(b - burn_up(a, r), r) <= claims(b, r),
{
    lemma_mul_inequality(0, a, q());
    ceil_product_bounds(a * q(), r);
    let k = burn_up(a, r);
    assert(k >= 0);
    assert(a * q() <= k * r);
    lemma_mul_is_distributive_add_other_way(r, b - k, k);
    assert((b - k) * r + a * q() <= b * r);
    lemma_div_is_ordered((b - k) * r + a * q(), b * r, q());
    floor_add_integer((b - k) * r, a, q());
}

// In the capped path, burn u <= B and pay floor(u*R/Q).
// Superadditivity ensures that payout is bounded by claims released.
pub proof fn capped_withdrawal(b: int, u: int, r: int)
    requires b >= 0, 0 <= u <= b, r > 0,
    ensures claims(u, r) <= claims(b, r) - claims(b - u, r),
{
    floor_superadditive(u * r, (b - u) * r, q());
    lemma_mul_is_distributive_add_other_way(r, u, b - u);
    assert(u * r + (b - u) * r == b * r);
}

// Concrete inhabitants of each requested domain, alongside—not instead of—
// the arbitrary-int theorems. Nonzero effects exclude a solely zero-path demo.
pub proof fn domain_witnesses() {
    assert(
        0 < q() / 2 < q()
        && claims(8 + mint_down(3, q() / 2), q() / 2) - claims(8, q() / 2) == 3
        && claims(8 + mint_down(3, q()), q()) - claims(8, q()) == 3
        && q() + q() / 2 > q()
        && claims(8 + mint_down(3, q() + q() / 2), q() + q() / 2)
            - claims(8, q() + q() / 2) == 3
        && (q() + 1) % (q() / 100) != 0
        && claims(8 + mint_down(3, q() + 1), q() + 1) - claims(8, q() + 1) == 2
        && mint_down(0, q() + 1) == 0
        && burn_up(0, q() + 1) == 0
        && claims(0, q() + 1) == 0
    ) by (compute);
}

// The mutant ceil mint is one share, taking claims from 1 to 3 despite
// receiving only one asset. The negative-control generator flips the marked
// true assertion to the false supply bound in a sandbox-only copy.
pub proof fn ceil_mint_counterexample()
    ensures
        burn_up(1, 1_500_000_000_000) == 1,
        claims(1, 1_500_000_000_000) == 1,
        claims(1 + burn_up(1, 1_500_000_000_000), 1_500_000_000_000) == 3,
{
    // NEGATIVE_CONTROL_ASSERTION
    assert(claims(1 + burn_up(1, 1_500_000_000_000), 1_500_000_000_000)
        - claims(1, 1_500_000_000_000) > 1);
}

} // verus!

fn main() {}
