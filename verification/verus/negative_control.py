#!/usr/bin/env python3
"""Generate a rejected ceil-mint variant outside the acceptance source tree.

Usage: python3 verification/verus/negative_control.py \
    /tmp/blend-verification-tools/verus/negative-control/supply_delta.rs

This only writes source; it never invokes Verus. The parent runner must first
establish the unmodified file passes, then require an assertion/postcondition
failure (not a syntax, dependency, timeout, or process failure) for this mutant.
"""

import argparse
from pathlib import Path


MINT = "pub open spec fn mint_down(a: int, r: int) -> int { a * q() / r }"
MUTANT_MINT = (
    "pub open spec fn mint_down(a: int, r: int) -> int { ceil_div(a * q(), r) }"
)
COUNTEREXAMPLE = """    // NEGATIVE_CONTROL_ASSERTION
    assert(claims(1 + burn_up(1, 1_500_000_000_000), 1_500_000_000_000)
        - claims(1, 1_500_000_000_000) > 1);"""
MUTANT_ASSERTION = """    // NEGATIVE_CONTROL_ASSERTION: false for B=1, R=1.5Q, A=1.
    assert(claims(1 + mint_down(1, 1_500_000_000_000), 1_500_000_000_000)
        - claims(1, 1_500_000_000_000) <= 1);"""


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("output", type=Path)
    output = parser.parse_args().output.resolve()
    roots = (
        Path("/tmp/blend-verification-tools/verus").resolve(),
        Path(__file__).resolve().parents[2] / "target" / "verification-local",
    )
    if not any(output.is_relative_to(root) for root in roots) or output.suffix != ".rs":
        parser.error("output must be an .rs file inside a verification build sandbox")
    source = Path(__file__).with_name("supply_delta.rs").read_text()
    if source.count(MINT) != 1 or source.count(COUNTEREXAMPLE) != 1:
        parser.error("source changed: expected mutation anchors exactly once")
    mutant = source.replace(MINT, MUTANT_MINT).replace(
        COUNTEREXAMPLE, MUTANT_ASSERTION
    )
    # Skip changed concrete examples so the checker reaches the false theorem.
    begin = mutant.index("pub proof fn domain_witnesses()")
    end = mutant.index("// The mutant ceil mint", begin)
    mutant = mutant[:begin] + mutant[end:]
    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_text(mutant)
    print(output)


if __name__ == "__main__":
    main()
