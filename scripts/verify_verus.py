"""Check the universal mathematical model and its required ceil-mint mutation."""

import json
import os
from pathlib import Path
import subprocess
import sys

from kani_release import hash_file, unique_object

ROOT = Path(__file__).resolve().parent.parent
REQUIRED = {
    "supply_delta::floor_add_integer",
    "supply_delta::floor_product_bounds",
    "supply_delta::ceil_product_bounds",
    "supply_delta::floor_superadditive",
    "supply_delta::supply_delta",
    "supply_delta::uncapped_withdrawal",
    "supply_delta::capped_withdrawal",
    "supply_delta::domain_witnesses",
    "supply_delta::ceil_mint_counterexample",
}
MUTANT_FAILURES = {"supply_delta::supply_delta", "supply_delta::ceil_mint_counterexample"}


def report(path):
    text = path.read_text()
    start = text.find("\n{\n")
    if start < 0:
        raise ValueError(f"Verus did not produce its JSON verdict: {path}")
    return json.JSONDecoder(object_pairs_hook=unique_object).raw_decode(text[start + 1:])[0]


def verify(contained, output):
    pin = json.loads((ROOT / "verification/verus/toolchain.json").read_text())
    home = Path(os.environ.get("VERUS_HOME", "/tmp/blend-verification-tools/verus"))
    bundle = home / "verus-x86-linux"
    hashes = {
        bundle / "verus": pin["verus"]["executable_sha256"],
        bundle / "rust_verify": pin["verus"]["rust_verify_sha256"],
        bundle / "z3": pin["solver"]["sha256"],
        bundle / "vstd.vir": pin["libraries"]["bundled_vstd_vir_sha256"],
        bundle / "libverus_builtin_macros.so": pin["libraries"]["bundled_builtin_macros_sha256"],
        bundle / "libverus_state_machines_macros.so":
            pin["libraries"]["bundled_state_machines_macros_sha256"],
        bundle / "libverus_builtin.rlib": pin["libraries"]["bundled_builtin_rlib_sha256"],
        bundle / "libvstd.rlib": pin["libraries"]["bundled_libvstd_rlib_sha256"],
        home / "rustup/toolchains" / pin["rust"]["toolchain"] / "bin/rustc":
            pin["rust"]["rustc_sha256"],
    }
    for path, digest in hashes.items():
        if hash_file(path) != digest:
            raise ValueError(f"pinned verifier/library mismatch: {path}")
    compiler = subprocess.run(
        ["env", "RUSTUP_AUTO_INSTALL=0", f"RUSTUP_HOME={home / 'rustup'}", f"CARGO_HOME={home / 'cargo'}",
         "rustup", "run", pin["rust"]["toolchain"], "rustc", "--version", "--verbose"],
        capture_output=True, text=True, timeout=15, check=True,
    )
    if f"commit-hash: {pin['rust']['commit']}" not in compiler.stdout:
        raise ValueError("pinned Verus compiler identity mismatch")
    command = [
        "env", "RUSTUP_AUTO_INSTALL=0", f"RUSTUP_HOME={home / 'rustup'}", f"CARGO_HOME={home / 'cargo'}",
        f"VERUS_Z3_PATH={bundle / 'z3'}", str(bundle / "verus"),
    ]
    options = ["--no-cheating", "--num-threads", "1", "--time", "--output-json"]
    positive = output / "verus-positive.log"
    exit_code = contained(command + [str(ROOT / "verification/verus/supply_delta.rs")] + options,
                          300, positive)
    data = report(positive)
    result = data["verification-results"]
    functions = data["times-ms"]["smt"]["smt-run-module-times"][0]["function-breakdown"]
    verified = {entry["function"] for entry in functions if entry["success"]}
    if (exit_code != 0 or result.get("success") is not True
            or result.get("errors") != 0 or result.get("verified") != len(REQUIRED)
            or result.get("is-verifying-entire-crate") is not True
            or verified != REQUIRED or data["verus"]["commit"] != pin["verus"]["commit"]):
        raise ValueError(f"universal arithmetic acceptance failed: {positive}")
    print(f"Verus mathematical model: {len(verified)} checked proofs; log={positive}", flush=True)

    mutant = output / "verus-negative" / "supply_delta.rs"
    subprocess.run([sys.executable, str(ROOT / "verification/verus/negative_control.py"), str(mutant)],
                   check=True, timeout=15, stdout=subprocess.PIPE)
    negative = output / "verus-negative.log"
    exit_code = contained(command + [str(mutant)] + options, 300, negative)
    data = report(negative)
    result = data["verification-results"]
    functions = data["times-ms"]["smt"]["smt-run-module-times"][0]["function-breakdown"]
    failures = {entry["function"] for entry in functions if not entry["success"]}
    if (exit_code == 0 or result.get("success") is not False
            or result.get("encountered-vir-error") is not False
            or result.get("is-verifying-entire-crate") is not True
            or result.get("errors") != len(MUTANT_FAILURES)
            or failures != MUTANT_FAILURES or "error: assertion failed" not in negative.read_text()):
        raise ValueError(f"ceil-mint mutation did not produce the required proof failures: {negative}")
    print(f"Verus ceil-mint mutation: required assertion failures detected; log={negative}", flush=True)
