#!/usr/bin/env python3
"""Accept one selected Kani harness, including its exact reachable-cover contract.

The coverage inventory must match the union of the PR and weekly selections.
CI requires exact cover counts, including zero; historical release receipt
parsing retains its existing acceptance contract.
"""

import argparse
from pathlib import Path
import re
import sys

from kani_release import parse_inventory, parse_log

ROOT = Path(__file__).resolve().parent.parent
SELECTIONS = (ROOT / "verification/kani-ci-harnesses.txt",
              ROOT / "verification/kani-full-harnesses.txt")
INVENTORY = ROOT / "verification/kani-expected-covers.json"
HARNESS = r"[A-Za-z_][A-Za-z_0-9]*(?:::[A-Za-z_][A-Za-z_0-9]*)+"


def selectors(path):
    names = [line.strip() for line in path.read_text().splitlines()
             if line.strip() and not line.lstrip().startswith("#")]
    if (not names or len(set(names)) != len(names)
            or any(not re.fullmatch(HARNESS, name) for name in names)):
        raise ValueError(f"empty, duplicate or malformed harness selection: {path}")
    return set(names)


def selected_entry(selection, harness, inventory=INVENTORY):
    _, entries = parse_inventory(inventory)
    union = set().union(*(selectors(path) for path in SELECTIONS))
    if set(entries) != union:
        raise ValueError(f"cover inventory differs from selector union: "
                         f"missing={sorted(union - set(entries))}, "
                         f"unexpected={sorted(set(entries) - union)}")
    if selection.resolve() not in {path.resolve() for path in SELECTIONS}:
        raise ValueError("selection must be the PR or weekly harness file")
    if harness not in selectors(selection):
        raise ValueError(f"harness is not in the requested selection: {harness}")
    return entries[harness]


def check(entry, log_bytes, exit_code):
    text = log_bytes.decode("utf-8", errors="strict")
    # Strip only terminal color sequences, not arbitrary text around summaries.
    text = re.sub(r"\x1b\[[0-9;]*m", "", text)
    lines = [line.strip() for line in text.splitlines()]
    checks = [line for line in lines if "Checking harness" in line]
    if checks != [f"Checking harness {entry['harness']}..."]:
        raise ValueError("log must identify exactly the requested harness")
    verdicts = [line for line in lines if line.startswith("VERIFICATION")]
    terminals = [line for line in lines if line.startswith("Complete")]
    if verdicts != ["VERIFICATION:- SUCCESSFUL"] or terminals != [
        "Complete - 1 successfully verified harnesses, 0 failures, 1 total."
    ]:
        raise ValueError("missing, malformed, failed or multiple Kani verdicts")
    summaries = [line for line in lines if "cover properties" in line]
    expected = entry["expected_covers"]
    required = f"** {expected} of {expected} cover properties satisfied"
    if summaries != [required] and not (expected == 0 and summaries == []):
        raise ValueError(f"expected exactly {expected} satisfied covers; got {summaries}")
    record = {"expected_covers": expected, "signal": None,
              "execution_error": None, "exit": exit_code}
    verdict, covers, _, _, _ = parse_log(record, text.encode())
    if verdict != "PASS":
        raise ValueError(f"Kani acceptance failed: {verdict}; process exit={exit_code}")
    return covers


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--selection", type=Path, required=True)
    parser.add_argument("--harness", required=True)
    parser.add_argument("--log", type=Path, required=True)
    parser.add_argument("--exit-code", type=int, required=True)
    args = parser.parse_args()
    entry = selected_entry(args.selection, args.harness)
    covers = check(entry, args.log.read_bytes(), args.exit_code)
    print(f"{args.harness}: PASS; covers={covers}; log={args.log}")
    return 0


if __name__ == "__main__":
    try:
        sys.exit(main())
    except (OSError, ValueError) as error:
        print(f"check-kani-ci: {error}", file=sys.stderr)
        sys.exit(1)
