#!/usr/bin/env python3
"""Run exact inventory-selected Kani harnesses and retain source-bound receipts."""

import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import signal
import subprocess
import sys
import time


def unique_object(pairs):
    result = {}
    for key, value in pairs:
        if key in result:
            raise ValueError(f"duplicate JSON key: {key}")
        result[key] = value
    return result


def rust_digests(source, target):
    result = {}
    for directory, dirs, files in os.walk(source, onerror=lambda error: (_ for _ in ()).throw(error)):
        base = Path(directory)
        dirs[:] = sorted(d for d in dirs if d != "target" and (base / d).resolve() != target)
        for name in sorted(files):
            path = base / name
            if name.endswith(".rs") or name in ("Cargo.toml", "Cargo.lock", "rust-toolchain.toml"):
                result[str(path.relative_to(source))] = hashlib.sha256(path.read_bytes()).hexdigest()
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", required=True, type=Path)
    parser.add_argument("--inventory", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path, help="New results directory; must not exist")
    parser.add_argument("--target-dir", type=Path)
    parser.add_argument("--solver", default="kissat")
    parser.add_argument("--timeout", default="120s", help="Positive integer duration with s, m or h suffix")
    parser.add_argument("--harness", action="append", help="Exact fully qualified inventory selector; repeatable")
    parser.add_argument("--force-build", action="store_true", help="Force rebuilding on the first invocation only")
    args = parser.parse_args()
    source = args.source.resolve()
    target = (args.target_dir or source / "target" / "kani").resolve()
    output = args.output.absolute()
    try:
        if not source.is_dir() or not (source / "Cargo.toml").is_file():
            raise ValueError("--source must be a Cargo project root")
        if source == target or source.is_relative_to(target):
            raise ValueError("--target-dir cannot contain the source root")
        if output.exists() or output.is_symlink():
            raise ValueError("--output already exists")
        if not re.fullmatch(r"[1-9][0-9]*[smh]", args.timeout):
            raise ValueError("--timeout must be a positive integer followed by s, m or h")
        if not args.solver or args.solver.startswith("-"):
            raise ValueError("--solver must name a solver")
        inventory_bytes = args.inventory.read_bytes()
        inventory = json.loads(inventory_bytes, object_pairs_hook=unique_object)
        if not isinstance(inventory, list) or not inventory:
            raise ValueError("inventory must be a nonempty array")
        ids, selectors = set(), set()
        required = {"id", "package", "harness", "expected_covers"}
        for entry in inventory:
            if not isinstance(entry, dict) or not required <= entry.keys() or entry.keys() - required - {"tests"}:
                raise ValueError("each inventory entry requires id, package, harness, expected_covers; only tests is optional")
            identifier = entry["id"]
            harness = entry["harness"]
            if not isinstance(identifier, str) or not identifier.strip() or identifier != identifier.strip():
                raise ValueError("inventory id must be a nonempty trimmed string")
            if entry["package"] not in ("pool", "backstop"):
                raise ValueError(f"{identifier}: package must be pool or backstop")
            if not isinstance(harness, str) or not re.fullmatch(r"(?:[A-Za-z_][A-Za-z_0-9]*::)+[A-Za-z_][A-Za-z_0-9]*", harness):
                raise ValueError(f"{identifier}: harness must be fully qualified")
            if type(entry["expected_covers"]) is not int or entry["expected_covers"] < 0:
                raise ValueError(f"{identifier}: expected_covers must be a nonnegative integer")
            if "tests" in entry and type(entry["tests"]) is not bool:
                raise ValueError(f"{identifier}: tests must be boolean")
            if identifier in ids or harness in selectors:
                raise ValueError(f"duplicate inventory id or selector: {identifier}, {harness}")
            ids.add(identifier)
            selectors.add(harness)
        if args.harness is not None:
            if len(set(args.harness)) != len(args.harness) or any(h not in selectors for h in args.harness):
                raise ValueError("--harness selections must be unique exact inventory selectors")
            selected = set(args.harness)
            inventory = [entry for entry in inventory if entry["harness"] in selected]
        if not inventory:
            raise ValueError("no harnesses selected")
        cargo_kani = shutil.which("cargo-kani")
        if cargo_kani is None:
            raise ValueError("cargo-kani not found on PATH")
        if os.name != "posix":
            raise ValueError("POSIX process groups are required for solver cleanup")
        output.mkdir(parents=True, exist_ok=False)
    except (OSError, ValueError) as error:
        parser.error(str(error))

    records = []
    interrupted = 0

    def interrupt(signum, _frame):
        nonlocal interrupted
        interrupted = signum
        raise KeyboardInterrupt(signum)

    inventory_sha256 = hashlib.sha256(inventory_bytes).hexdigest()

    signals = (signal.SIGINT, signal.SIGTERM, signal.SIGHUP)
    old_handlers = {sig: signal.signal(sig, interrupt) for sig in signals}
    try:
        for index, entry in enumerate(inventory):
            argv = [cargo_kani, "-p", entry["package"], "--lib", "--harness", entry["harness"],
                    "--exact", "--target-dir", str(target), "-Z", "unstable-options",
                    "--harness-timeout", args.timeout, "--solver", args.solver]
            if entry.get("tests", False):
                argv.append("--tests")
            if args.force_build and index == 0:
                argv.append("--force-build")
            log_path = output / f"{index + 1:04d}.log"
            before = rust_digests(source, target)
            start_time = time.time()
            start = time.monotonic()
            process = None
            execution_error = None
            print("START", entry["harness"], flush=True)
            try:
                with log_path.open("xb") as log:
                    # Block signals until Popen returns its group ID, closing the spawn/cleanup race.
                    previous_mask = signal.pthread_sigmask(signal.SIG_BLOCK, signals)
                    try:
                        process = subprocess.Popen(argv, cwd=source, stdout=log,
                                                   stderr=subprocess.STDOUT, start_new_session=True)
                    finally:
                        signal.pthread_sigmask(signal.SIG_SETMASK, previous_mask)
                    process.wait()
            except KeyboardInterrupt as caught:
                execution_error = "interrupted"
                interrupted = caught.args[0] if caught.args else signal.SIGINT
            except OSError as error:
                execution_error = str(error)
            finally:
                # Kani may exit on its harness cap without stopping the solver child.
                for sig in signals:
                    signal.signal(sig, signal.SIG_IGN)
                try:
                    if process is not None:
                        try:
                            os.killpg(process.pid, signal.SIGKILL)
                        except ProcessLookupError:
                            pass
                        process.wait()
                finally:
                    for sig in signals:
                        signal.signal(sig, interrupt)
            elapsed = time.monotonic() - start
            after = rust_digests(source, target)
            log_bytes = log_path.read_bytes()
            text = log_bytes.decode("utf-8", errors="replace")
            covers = [(int(a), int(b)) for a, b in re.findall(r"\*\* (\d+) of (\d+) cover properties satisfied", text)]
            expected = entry["expected_covers"]
            covers_passed = covers == [(expected, expected)] or (expected == 0 and not covers)
            terminals = re.findall(r"^Complete - (\d+) successfully verified harnesses, (\d+) failures, (\d+) total\.$", text, re.MULTILINE)
            verification = re.findall(r"^VERIFICATION:- (SUCCESSFUL|FAILED)\s*$", text, re.MULTILINE)
            exit_code = process.returncode if process is not None else None
            timeout = bool(re.search(r"timed? out|timeout|time limit", text, re.IGNORECASE))
            compile_failure = bool(re.search(r"could not compile|compilation errors?|failed to compile|error\[E\d+\]", text, re.IGNORECASE))
            if interrupted:
                verdict = "INTERRUPTED"
            elif execution_error:
                verdict = "EXECUTION_FAILURE"
            elif before != after:
                verdict = "VALIDATION_FAILURE"
            elif exit_code == 0 and terminals == [("1", "0", "1")] and verification == ["SUCCESSFUL"] and covers_passed:
                verdict = "PASS"
            elif compile_failure:
                verdict = "COMPILE_FAILURE"
            elif timeout:
                verdict = "TIMEOUT"
            elif "FAILED" in verification:
                verdict = "SEMANTIC_FAILURE"
            elif exit_code != 0:
                verdict = "EXECUTION_FAILURE"
            else:
                verdict = "VALIDATION_FAILURE"
            record = dict(entry, argv=argv, cwd=str(source), exit=exit_code, started_at=start_time,
                          source_sha256_before=before, source_sha256_after=after, source_unchanged=before == after,
                          manifest_sha256_before={key: value for key, value in before.items() if not key.endswith(".rs")},
                          elapsed=elapsed, log=log_path.name, log_sha256=hashlib.sha256(log_bytes).hexdigest(),
                          cover_summaries=covers, covers_passed=covers_passed, terminal_summaries=terminals,
                          verification_summaries=verification, verdict=verdict, passed=verdict == "PASS",
                          execution_error=execution_error, signal=interrupted or None, terminal=text[-2500:],
                          inventory_sha256=inventory_sha256)
            records.append(record)
            temporary = output / "results.json.tmp"
            temporary.write_text(json.dumps(records, indent=2) + "\n")
            temporary.replace(output / "results.json")
            print(entry["harness"], verdict, "exit", exit_code, "seconds", elapsed, flush=True)
            if interrupted:
                return 128 + interrupted
            if verdict != "PASS":
                return exit_code if exit_code is not None and 0 < exit_code < 256 else 1
    except KeyboardInterrupt:
        return 128 + (interrupted or signal.SIGINT)
    except OSError as error:
        print(f"evidence error: {error}", file=sys.stderr)
        return 1
    finally:
        for sig, handler in old_handlers.items():
            signal.signal(sig, handler)
    return 0


if __name__ == "__main__":
    sys.exit(main())
