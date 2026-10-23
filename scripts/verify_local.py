#!/usr/bin/env python3
"""Run the local proof inventory serially in fail-closed cgroup-v2 services."""

import argparse
from contextlib import ExitStack
import fcntl
import json
import os
from pathlib import Path
import re
import signal
import subprocess
import sys
import uuid

from kani_release import parse_log, unique_object

ROOT = Path(__file__).resolve().parent.parent
OUTPUT = ROOT / "target" / "verification-local"
INVENTORY = ROOT / "verification" / "local-proofs.json"
MEMORY = 4 * 1024**3
SWAP = 0
TASKS = 256
CPU_PERCENT = 200
MAX_SECONDS = 3600
# Transient services inherit their manager's environment, not the invoking shell.
# Keep required tool/build settings explicit without importing CI credentials.
WORKER_ENVIRONMENT = (
    "PATH", "CARGO_BUILD_JOBS", "CARGO_HOME", "RUSTUP_HOME",
    "VERUS_HOME", "XDG_CACHE_HOME",
)
# Keep the acceptance contract independent of the editable execution inventory.
REQUIRED_HARNESSES = {
    "pool::reserve::verification::prove_accrual_conserves_value_debt_000",
    "pool::actions::verification::prove_supply_chain_contract",
    "pool::actions::verification::prove_supply_cap_decides_on_post_state",
    "prove_price_boundary_and_rejection",
    "prove_decimals_boundary_and_cache",
}
TARGETS = {
    "pool": ["-p", "pool"],
    "contracts": ["-p", "pool", "--features", "local-contracts",
                  "-Z", "function-contracts", "-Z", "stubbing"],
    "oracle": ["--manifest-path", "verification/local-host/Cargo.toml",
               "--target-dir", str(OUTPUT / "host-target")],
}


def check_limits():
    """Check the actual worker cgroup, not merely systemd command-line intent."""
    entry = Path("/proc/self/cgroup").read_text().strip().splitlines()
    unified = [line[3:] for line in entry if line.startswith("0::")]
    if len(unified) != 1:
        raise ValueError("a unified cgroup-v2 hierarchy is required")
    group = Path("/sys/fs/cgroup") / unified[0].lstrip("/")
    expected = {
        "memory.max": str(MEMORY),
        "memory.swap.max": str(SWAP),
        "pids.max": str(TASKS),
    }
    for name, value in expected.items():
        if (group / name).read_text().strip() != value:
            raise ValueError(f"containment mismatch: {name}")
    quota, period = (group / "cpu.max").read_text().split()
    if quota == "max" or int(quota) * 100 > CPU_PERCENT * int(period):
        raise ValueError(f"CPU quota exceeds {CPU_PERCENT}%")
    if (group / "memory.oom.group").read_text().strip() != "1":
        raise ValueError("group OOM killing is required")


def contained(command, seconds, log):
    if not 1 <= seconds <= MAX_SECONDS:
        raise ValueError(f"per-attempt containment must be between 1 and {MAX_SECONDS} seconds")
    unit = f"blend-verify-{uuid.uuid4().hex}"
    argv = [
        "systemd-run", "--user", "--wait", "--pipe", "--collect",
        f"--unit={unit}",
        f"--working-directory={ROOT}",
        f"--property=MemoryMax={MEMORY}",
        f"--property=MemorySwapMax={SWAP}", f"--property=TasksMax={TASKS}",
        f"--property=CPUQuota={CPU_PERCENT}%", "--property=OOMPolicy=kill",
        "--property=KillMode=control-group", "--property=TimeoutStopSec=5s",
        f"--property=RuntimeMaxSec={seconds}s",
        *(f"--setenv={name}={os.environ[name]}" for name in WORKER_ENVIRONMENT if name in os.environ),
        # A compiler-cache broker may already run outside this unit's cgroup.
        "--setenv=RUSTC_WRAPPER=", "--setenv=RUSTC_WORKSPACE_WRAPPER=",
        sys.executable, str(Path(__file__).resolve()), "_worker", *command,
    ]
    with ExitStack() as cleanup:
        def interrupted(signum, _frame):
            raise InterruptedError(f"interrupted by signal {signum}")

        for signum in (signal.SIGINT, signal.SIGTERM):
            previous = signal.signal(signum, interrupted)
            cleanup.callback(signal.signal, signum, previous)
        try:
            with log.open("wb") as output:
                return subprocess.run(
                    argv, stdout=output, stderr=subprocess.STDOUT, timeout=seconds + 30,
                ).returncode
        finally:
            # Unit shutdown is independent of the local systemd client lifecycle.
            stopped = subprocess.run(
                ["systemctl", "--user", "stop", f"{unit}.service"],
                stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=15,
            )
            state = subprocess.run(
                ["systemctl", "--user", "show", f"{unit}.service",
                 "--property=ActiveState", "--value"],
                capture_output=True, text=True, timeout=15,
            )
            if state.returncode or state.stdout.strip() not in {"inactive", "failed"}:
                raise ValueError(f"cannot establish solver service cleanup: {unit}")
            if stopped.returncode and "not loaded" not in stopped.stderr.decode():
                raise ValueError(f"could not stop solver service: {stopped.stderr.decode().strip()}")


def load_inventory():
    entries = json.loads(INVENTORY.read_text(), object_pairs_hook=unique_object)
    if not isinstance(entries, list) or not entries:
        raise ValueError("proof inventory must be nonempty")
    identifiers = set()
    for entry in entries:
        if not isinstance(entry, dict) or set(entry) != {
            "target", "harness", "expected_covers", "seconds", "depends_on"
        }:
            raise ValueError("invalid Kani proof inventory fields")
        if not isinstance(entry["target"], str) or entry["target"] not in TARGETS:
            raise ValueError("unknown local proof target")
        if not isinstance(entry["harness"], str) or not re.fullmatch(
            r"[A-Za-z_][A-Za-z_0-9]*(?:::[A-Za-z_][A-Za-z_0-9]*)*", entry["harness"]
        ):
            raise ValueError("invalid Rust harness selector")
        dependencies = entry["depends_on"]
        if (not isinstance(dependencies, list)
                or not all(isinstance(name, str) for name in dependencies)
                or len(set(dependencies)) != len(dependencies)
                or not set(dependencies) <= identifiers):
            raise ValueError("proof dependencies must name earlier checked inventory entries")
        if entry["harness"] in identifiers:
            raise ValueError("duplicate proof selector")
        identifiers.add(entry["harness"])
        if type(entry["expected_covers"]) is not int or entry["expected_covers"] < 1:
            raise ValueError("each local proof must require reachable witnesses")
        if type(entry["seconds"]) is not int or not 1 <= entry["seconds"] <= MAX_SECONDS:
            raise ValueError("invalid proof deadline")
    if identifiers != REQUIRED_HARNESSES:
        raise ValueError(
            f"proof inventory differs from the required set: "
            f"missing={sorted(REQUIRED_HARNESSES - identifiers)}, "
            f"unexpected={sorted(identifiers - REQUIRED_HARNESSES)}"
        )
    return entries


def checked_inventory():
    entries = load_inventory()
    version = subprocess.run(["cargo", "kani", "--version"], capture_output=True,
                             text=True, check=True, timeout=15)
    if "Kani Rust Verifier 0.68.0 (cargo plugin)" not in version.stdout:
        raise ValueError("Kani 0.68.0 is required")
    return entries


def codegen_inventory():
    entries = checked_inventory()
    for target in dict.fromkeys(entry["target"] for entry in entries):
        log = OUTPUT / f"{target}-codegen.log"
        command = ["env", "RUSTC_WRAPPER=", "cargo", "kani",
                   *TARGETS[target], "--lib", "--only-codegen", "--exact"]
        for entry in entries:
            if entry["target"] == target:
                command.extend(["--harness", entry["harness"]])
        if contained(command, 1800, log) != 0:
            raise ValueError(f"proof compilation failed: {target}; log={log}")
        print(f"{target}: proof compilation complete; log={log}", flush=True)


def run_inventory():
    entries = checked_inventory()
    for entry in entries:
        harness = entry["harness"]
        log = OUTPUT / f"{entry['target']}_{harness.replace('::', '_')}.log"
        command = [
            "env", "RUSTC_WRAPPER=", "cargo", "kani", *TARGETS[entry["target"]], "--lib", "--exact",
            "--harness", harness, "--solver", "kissat", "-Z", "unstable-options",
            "--harness-timeout", f"{entry['seconds']}s",
        ]
        result = contained(command, entry["seconds"], log)
        record = {"expected_covers": entry["expected_covers"], "signal": None,
                  "execution_error": None, "exit": result}
        verdict, covers, _, _, _ = parse_log(record, log.read_bytes())
        print(f"{harness}: {verdict}; covers={covers}; log={log}", flush=True)
        if verdict != "PASS":
            raise ValueError(f"proof acceptance failed: {harness}")
    from verify_verus import verify
    verify(contained, OUTPUT)


def main():
    # Setup is explicit; neither cargo proxies nor workers may install a toolchain.
    os.environ["RUSTUP_AUTO_INSTALL"] = "0"
    if len(sys.argv) > 1 and sys.argv[1] == "_worker":
        check_limits()
        os.execvp(sys.argv[2], sys.argv[2:])
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--seconds", type=int, default=900)
    parser.add_argument("--codegen", action="store_true",
                        help="compile each proof configuration before the per-harness deadlines")
    parser.add_argument("command", nargs=argparse.REMAINDER,
                        help="optional experimental command after --; default runs the proof inventory")
    args = parser.parse_args()
    if args.codegen and args.command:
        parser.error("--codegen cannot be combined with an experimental command")
    OUTPUT.mkdir(parents=True, exist_ok=True)
    with (OUTPUT / "run.lock").open("w") as lock:
        fcntl.flock(lock, fcntl.LOCK_EX)
        if args.command:
            command = args.command[1:] if args.command[0] == "--" else args.command
            if not command:
                raise ValueError("empty experimental command")
            log = OUTPUT / f"experiment-{uuid.uuid4().hex}.log"
            result = contained(command, args.seconds, log)
            print(f"experiment: exit={result}; log={log}", flush=True)
            return result
        if args.codegen:
            codegen_inventory()
        else:
            run_inventory()
    return 0


if __name__ == "__main__":
    try:
        sys.exit(main())
    except (OSError, ValueError, subprocess.SubprocessError, InterruptedError) as error:
        print(f"verify-local: {error}", file=sys.stderr)
        sys.exit(1)
