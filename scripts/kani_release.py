#!/usr/bin/env python3
"""Package, verify, and publish a source-bound Kani run.

Inputs must be custody-owned by the authenticated local operator. This tool independently
reconciles receipts and logs, but it does not create a cryptographic execution attestation.
"""

import argparse
import datetime as dt
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import stat
import tempfile
import time


SOURCE_FILES = {"Cargo.toml", "Cargo.lock", "rust-toolchain.toml"}
REQUIRED_PACKAGE_FILES = {
    "kani.py",
    "inventory.json",
    "original-obligations.json",
    "integrated-execution-receipt.json",
}
ALLOWED_SUPERSEDED_VERDICTS = {"TIMEOUT"}
COMMAND_TIMEOUTS = {"git": 30 * 60, "tar": 30 * 60, "gh": 60 * 60}
ARCHIVE_TAR_TIMEOUT = 60 * 60
ARCHIVE_ZSTD_TIMEOUT = 2 * 60 * 60
PROCESS_STOP_TIMEOUT = 5


def unique_object(pairs):
    result = {}
    for key, value in pairs:
        if key in result:
            raise ValueError(f"duplicate JSON key: {key}")
        result[key] = value
    return result


def load_json(path):
    try:
        return json.loads(path.read_bytes(), object_pairs_hook=unique_object)
    except (OSError, UnicodeDecodeError, json.JSONDecodeError, ValueError) as error:
        raise ValueError(f"cannot parse {path}: {error}") from error


def hash_file(path):
    digest = hashlib.sha256()
    with path.open("rb") as source:
        for chunk in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def command(argv, *, cwd=None, capture=False, check=True):
    try:
        return subprocess.run(
            argv,
            cwd=cwd,
            check=check,
            text=True,
            stdout=subprocess.PIPE if capture else None,
            stderr=subprocess.PIPE if capture else None,
            timeout=COMMAND_TIMEOUTS.get(Path(argv[0]).name),
        )
    except subprocess.TimeoutExpired as error:
        raise ValueError(
            f"command timed out after {error.timeout}s ({' '.join(argv)})"
        ) from error
    except OSError as error:
        raise ValueError(f"cannot execute {argv[0]}: {error}") from error
    except subprocess.CalledProcessError as error:
        detail = (error.stderr or error.stdout or "").strip()
        raise ValueError(f"command failed ({' '.join(argv)}): {detail}") from error


def require_directory(path, label):
    path = path.expanduser()
    if path.is_symlink():
        raise ValueError(f"{label} must not be a symlink: {path}")
    try:
        path = path.resolve(strict=True)
    except OSError as error:
        raise ValueError(f"cannot resolve {label}: {error}") from error
    if not path.is_dir():
        raise ValueError(f"{label} is not a directory: {path}")
    for entry in (path, *path.rglob("*")):
        metadata = entry.lstat()
        if stat.S_ISLNK(metadata.st_mode):
            raise ValueError(f"{label} contains a symlink: {entry}")
        if not (stat.S_ISDIR(metadata.st_mode) or stat.S_ISREG(metadata.st_mode)):
            raise ValueError(f"{label} contains a special file: {entry}")
        if metadata.st_uid != os.geteuid() or metadata.st_mode & (
            stat.S_IWGRP | stat.S_IWOTH
        ):
            raise ValueError(
                f"{label} is not exclusively writable by the current user: {entry}"
            )
    return path


def snapshot_directory(source, destination, label):
    shutil.copytree(source, destination, symlinks=True)
    return require_directory(destination, f"{label} snapshot")


def prepare_output_directory(path):
    path = path.expanduser()
    if path.is_symlink():
        raise ValueError(f"output directory must not be a symlink: {path}")
    path.mkdir(parents=True, exist_ok=True)
    path = path.resolve(strict=True)
    metadata = path.lstat()
    if (
        not stat.S_ISDIR(metadata.st_mode)
        or metadata.st_uid != os.geteuid()
        or metadata.st_mode & (stat.S_IWGRP | stat.S_IWOTH)
    ):
        raise ValueError(f"output directory is not private to the current user: {path}")
    return path


def install_exclusive(source, destination):
    flags = os.O_WRONLY | os.O_CREAT | os.O_EXCL
    if hasattr(os, "O_NOFOLLOW"):
        flags |= os.O_NOFOLLOW
    try:
        descriptor = os.open(destination, flags, 0o644)
    except OSError as error:
        raise ValueError(
            f"refusing to overwrite release output: {destination}"
        ) from error
    try:
        with os.fdopen(descriptor, "wb") as target, source.open("rb") as payload:
            shutil.copyfileobj(payload, target, 1024 * 1024)
    except BaseException:
        destination.unlink(missing_ok=True)
        raise
    if hash_file(source) != hash_file(destination):
        destination.unlink(missing_ok=True)
        raise ValueError(f"installed output digest mismatch: {destination}")


def resolve_commit(repo_root, revision):
    result = command(
        ["git", "rev-parse", "--verify", f"{revision}^{{commit}}"],
        cwd=repo_root,
        capture=True,
    )
    return result.stdout.strip()


def source_digests(repo_root, commit):
    with tempfile.TemporaryDirectory(prefix="kani-release-source-") as temporary:
        temporary = Path(temporary)
        archive = temporary / "source.tar"
        source = temporary / "source"
        source.mkdir()
        command(
            ["git", "archive", "--format=tar", f"--output={archive}", commit],
            cwd=repo_root,
        )
        command(["tar", "-xf", str(archive), "-C", str(source)])
        source = require_directory(source, "commit source")
        result = {}
        for directory, dirs, files in os.walk(source):
            base = Path(directory)
            dirs[:] = sorted(name for name in dirs if name != "target")
            for name in sorted(files):
                if name.endswith(".rs") or name in SOURCE_FILES:
                    path = base / name
                    result[str(path.relative_to(source))] = hash_file(path)
        return result


def parse_inventory(path):
    inventory = load_json(path)
    if not isinstance(inventory, list) or not inventory:
        raise ValueError("inventory must be a nonempty array")
    by_harness = {}
    for entry in inventory:
        if not isinstance(entry, dict):
            raise ValueError("inventory entries must be objects")
        allowed = {"id", "package", "harness", "expected_covers", "tests"}
        required = allowed - {"tests"}
        if set(entry) - allowed or not required <= set(entry):
            raise ValueError(
                "inventory entries require id, package, harness, expected_covers; only tests is optional"
            )
        harness = entry["harness"]
        if not isinstance(harness, str) or not re.fullmatch(
            r"(?:[A-Za-z_][A-Za-z_0-9]*::)+[A-Za-z_][A-Za-z_0-9]*", harness
        ):
            raise ValueError(f"invalid inventory harness: {harness!r}")
        if entry["id"] != harness:
            raise ValueError(f"{harness}: inventory id must equal harness")
        if harness in by_harness:
            raise ValueError(f"duplicate inventory harness: {harness}")
        if entry["package"] not in {"pool", "backstop"}:
            raise ValueError(f"{harness}: unsupported package")
        if type(entry["expected_covers"]) is not int or entry["expected_covers"] < 0:
            raise ValueError(
                f"{harness}: expected_covers must be a nonnegative integer"
            )
        if "tests" in entry and type(entry["tests"]) is not bool:
            raise ValueError(f"{harness}: tests must be boolean")
        by_harness[harness] = entry
    return inventory, by_harness


def parse_log(record, log_bytes):
    text = log_bytes.decode("utf-8", errors="replace")
    covers = [
        [int(match.group(1)), int(match.group(2))]
        for match in re.finditer(
            r"([0-9]+) of ([0-9]+) cover properties satisfied", text
        )
    ]
    terminals = [
        list(match.groups())
        for match in re.finditer(
            r"Complete - ([0-9]+) successfully verified harnesses, "
            r"([0-9]+) failures, ([0-9]+) total",
            text,
        )
    ]
    verification = re.findall(r"VERIFICATION:- (SUCCESSFUL|FAILED)", text)
    if record["expected_covers"] == 0:
        covers_passed = all(passed == total for passed, total in covers)
    else:
        covers_passed = (
            len(covers) == 1
            and covers[0][0] == record["expected_covers"]
            and covers[0][1] == record["expected_covers"]
        )
    signal = record["signal"]
    execution_error = record["execution_error"]
    exit_code = record["exit"]
    if signal:
        verdict = "INTERRUPTED"
    elif execution_error:
        verdict = "EXECUTION_FAILURE"
    elif (
        exit_code == 0
        and terminals == [["1", "0", "1"]]
        and verification == ["SUCCESSFUL"]
        and covers_passed
    ):
        verdict = "PASS"
    elif re.search(
        r"failed to compile|could not compile|rustc .* exited with|error\[E[0-9]+\]",
        text,
        re.IGNORECASE,
    ):
        verdict = "COMPILE_FAILURE"
    elif re.search(r"timed out|timeout|time limit", text, re.IGNORECASE):
        verdict = "TIMEOUT"
    elif "VERIFICATION:- FAILED" in text or "failures, 1 total" in text:
        verdict = "SEMANTIC_FAILURE"
    elif exit_code != 0:
        verdict = "EXECUTION_FAILURE"
    else:
        verdict = "VALIDATION_FAILURE"
    return verdict, covers, covers_passed, terminals, verification


def validate_results(run_dir, inventory, commit_sources):
    result_files = sorted(run_dir.rglob("results.json"))
    if not result_files:
        raise ValueError(f"no results.json files below {run_dir}")

    passes = set()
    verdicts = {}
    inventory_digests = set()
    referenced_logs = set()
    start_times = []
    records = 0
    for path in result_files:
        batch = load_json(path)
        if not isinstance(batch, list):
            raise ValueError(f"{path} must contain an array")
        batch_inventory_digests = set()
        for record in batch:
            records += 1
            if not isinstance(record, dict):
                raise ValueError(f"{path}: result records must be objects")
            required = {
                "harness",
                "package",
                "expected_covers",
                "verdict",
                "covers_passed",
                "cover_summaries",
                "terminal_summaries",
                "verification_summaries",
                "exit",
                "signal",
                "execution_error",
                "source_unchanged",
                "source_sha256_before",
                "source_sha256_after",
                "inventory_sha256",
                "log",
                "log_sha256",
                "started_at",
            }
            if not required <= set(record):
                raise ValueError(f"{path}: result record is missing required fields")
            harness = record["harness"]
            entry = inventory.get(harness)
            if entry is None:
                raise ValueError(f"result harness is absent from inventory: {harness}")
            if record["package"] != entry["package"]:
                raise ValueError(f"{harness}: result package differs from inventory")
            if record["expected_covers"] != entry["expected_covers"]:
                raise ValueError(f"{harness}: expected covers differ from inventory")
            if record.get("tests", False) != entry.get("tests", False):
                raise ValueError(f"{harness}: test mode differs from inventory")
            log_name = record["log"]
            log_sha256 = record["log_sha256"]
            if not isinstance(log_name, str) or not re.fullmatch(
                r"[A-Za-z0-9][A-Za-z0-9._-]*", log_name
            ):
                raise ValueError(f"{harness}: invalid log filename")
            if not isinstance(log_sha256, str) or not re.fullmatch(
                r"[0-9a-f]{64}", log_sha256
            ):
                raise ValueError(f"{harness}: invalid log digest")
            log_path = path.parent / log_name
            relative_log = log_path.relative_to(run_dir)
            if (
                relative_log in referenced_logs
                or not log_path.is_file()
                or log_path.is_symlink()
            ):
                raise ValueError(f"{harness}: missing or duplicate retained log")
            if hash_file(log_path) != log_sha256:
                raise ValueError(f"{harness}: retained log digest mismatch")
            parsed_verdict, covers, covers_passed, terminals, verification = parse_log(
                record, log_path.read_bytes()
            )
            if (
                record["verdict"] != parsed_verdict
                or record["cover_summaries"] != covers
                or record["covers_passed"] is not covers_passed
                or record["terminal_summaries"] != terminals
                or record["verification_summaries"] != verification
            ):
                raise ValueError(f"{harness}: result fields do not match retained log")
            referenced_logs.add(relative_log)
            record_inventory = record["inventory_sha256"]
            if not isinstance(record_inventory, str) or not re.fullmatch(
                r"[0-9a-f]{64}", record_inventory
            ):
                raise ValueError(f"{harness}: invalid retained inventory digest")
            batch_inventory_digests.add(record_inventory)
            inventory_digests.add(record_inventory)
            if record["source_unchanged"] is not True:
                raise ValueError(f"{harness}: source changed during execution")
            before = record["source_sha256_before"]
            after = record["source_sha256_after"]
            if not isinstance(before, dict) or before != after:
                raise ValueError(f"{harness}: before/after source digests differ")
            if before != commit_sources:
                raise ValueError(
                    f"{harness}: source digests do not match the release commit"
                )
            verdict = record["verdict"]
            if verdict == "PASS":
                passes.add(harness)
            elif verdict not in ALLOWED_SUPERSEDED_VERDICTS:
                raise ValueError(
                    f"{harness}: unsupersedable verdict retained: {verdict}"
                )
            verdicts[verdict] = verdicts.get(verdict, 0) + 1
            started_at = record["started_at"]
            if not isinstance(started_at, (int, float)) or isinstance(started_at, bool):
                raise ValueError(f"{harness}: started_at must be a timestamp")
            start_times.append(float(started_at))
        if len(batch_inventory_digests) != 1:
            raise ValueError(
                f"{path}: result records must share one retained inventory digest"
            )

    actual_logs = {
        log.relative_to(run_dir)
        for log in run_dir.rglob("*.log")
        if log.is_file() and not log.is_symlink()
    }
    if referenced_logs != actual_logs:
        missing = sorted(str(path) for path in referenced_logs - actual_logs)
        extra = sorted(str(path) for path in actual_logs - referenced_logs)
        raise ValueError(
            f"retained log set differs from results; missing={missing[:3]}, extra={extra[:3]}"
        )

    expected = set(inventory)
    if passes != expected:
        missing = sorted(expected - passes)
        raise ValueError(f"not every inventory harness has a PASS: {missing[:3]}")
    return {
        "records": records,
        "unique_targets": len(expected),
        "verdicts": dict(sorted(verdicts.items())),
        "started_at": min(start_times),
        "result_files": len(result_files),
        "retained_inventory_digests": sorted(inventory_digests),
    }


def validate_campaign_receipt(package_dir, inventory_sha256, summary):
    path = package_dir / "integrated-execution-receipt.json"
    receipt = load_json(path)
    if not isinstance(receipt, dict):
        raise ValueError(f"{path}: campaign receipt must be an object")
    expected = {
        "status": "COMPLETE",
        "inventory_sha256": inventory_sha256,
        "targets": summary["unique_targets"],
        "passed": summary["unique_targets"],
        "failed": 0,
    }
    for field, value in expected.items():
        if receipt.get(field) != value:
            raise ValueError(f"{path}: {field} does not match validated execution")
    return receipt


def write_member_manifest(root):
    files = sorted(
        (
            path
            for path in root.rglob("*")
            if path.is_file() and path.name != "SHA256SUMS"
        ),
        key=lambda path: str(path.relative_to(root)).encode(),
    )
    content = "".join(
        f"{hash_file(path)}  {path.relative_to(root)}\n" for path in files
    )
    (root / "SHA256SUMS").write_text(content)
    return len(files)


def parse_member_manifest(root):
    manifest = root / "SHA256SUMS"
    expected = set()
    for line in manifest.read_text().splitlines():
        match = re.fullmatch(r"([0-9a-f]{64})  (.+)", line)
        if match is None:
            raise ValueError("invalid SHA256SUMS line")
        relative = Path(match.group(2))
        if (
            relative.is_absolute()
            or ".." in relative.parts
            or relative.name == "SHA256SUMS"
        ):
            raise ValueError(f"unsafe SHA256SUMS path: {relative}")
        path = root / relative
        if relative in expected or not path.is_file() or path.is_symlink():
            raise ValueError(f"invalid SHA256SUMS member: {relative}")
        if hash_file(path) != match.group(1):
            raise ValueError(f"member checksum mismatch: {relative}")
        expected.add(relative)
    actual = {
        path.relative_to(root)
        for path in root.rglob("*")
        if path.is_file() and path.name != "SHA256SUMS"
    }
    if expected != actual:
        raise ValueError("SHA256SUMS does not cover the exact archive payload")


def stop_process(process):
    if process is None or process.poll() is not None:
        return
    process.terminate()
    try:
        process.wait(timeout=PROCESS_STOP_TIMEOUT)
    except subprocess.TimeoutExpired:
        process.kill()
        try:
            process.wait(timeout=PROCESS_STOP_TIMEOUT)
        except subprocess.TimeoutExpired:
            pass


def build_archive(stage_parent, release_name, run_date, destination):

    tar = None
    zstd = None
    try:
        tar = subprocess.Popen(
            [
                "tar",
                "--sort=name",
                f"--mtime={run_date} 00:00:00Z",
                "--owner=0",
                "--group=0",
                "--numeric-owner",
                "--mode=a=r,u+w,a+X",
                "--format=gnu",
                "-cf",
                "-",
                release_name,
            ],
            cwd=stage_parent,
            stdout=subprocess.PIPE,
        )
        zstd = subprocess.Popen(
            ["zstd", "-19", "-T0", "-q", "-o", str(destination)],
            stdin=tar.stdout,
        )
        if tar.stdout is not None:
            tar.stdout.close()
        started = time.monotonic()
        while True:
            tar_status = tar.poll()
            zstd_status = zstd.poll()
            if tar_status is not None and zstd_status is not None:
                break
            elapsed = time.monotonic() - started
            if tar_status is None and elapsed >= ARCHIVE_TAR_TIMEOUT:
                raise subprocess.TimeoutExpired("tar", ARCHIVE_TAR_TIMEOUT)
            if zstd_status is None and elapsed >= ARCHIVE_ZSTD_TIMEOUT:
                raise subprocess.TimeoutExpired("zstd", ARCHIVE_ZSTD_TIMEOUT)
            time.sleep(0.1)
    except (OSError, subprocess.TimeoutExpired) as error:
        stop_process(zstd)
        stop_process(tar)
        destination.unlink(missing_ok=True)
        if isinstance(error, subprocess.TimeoutExpired):
            raise ValueError(
                f"archive build timed out after {error.timeout}s"
            ) from error
        raise ValueError(f"cannot execute archive tool: {error}") from error
    finally:
        if tar is not None and tar.stdout is not None:
            tar.stdout.close()

    if tar_status != 0 or zstd_status != 0:
        destination.unlink(missing_ok=True)
        raise ValueError(f"archive build failed: tar={tar_status}, zstd={zstd_status}")


def verify_archive(archive, release_name):
    with tempfile.TemporaryDirectory(prefix="kani-release-readback-") as temporary:
        root = Path(temporary)
        command(["tar", "--zstd", "-xf", str(archive), "-C", str(root)])
        extracted = root / release_name
        if not extracted.is_dir():
            raise ValueError("archive is missing its release root")
        for path in extracted.rglob("*"):
            if path.is_symlink():
                raise ValueError(
                    f"archive contains a symlink: {path.relative_to(extracted)}"
                )
        parse_member_manifest(extracted)


def make_notes(
    release_name, commit, tree, archive_hash, inventory_sha256, summary, members
):
    timeouts = summary["verdicts"].get("TIMEOUT", 0)
    return f"""# Kani run {commit[:7]} — {release_name.rsplit("-", 3)[-3]}-{release_name.rsplit("-", 3)[-2]}-{release_name.rsplit("-", 3)[-1]}

- Source commit: `{commit}`
- Source tree: `{tree}`
- Inventory: {summary["unique_targets"]} unique targets (`{inventory_sha256}`)
- Accepted verdicts: {summary["unique_targets"]} PASS, 0 FAIL
- Retained executions: {summary["records"]} ({timeouts} superseded TIMEOUT)
- Archive SHA-256: `{archive_hash}`
- Manifested payload members: {members}

The archive contains the exact proof package and raw run directory. `MANIFEST.json` records the source binding and claim boundary; `SHA256SUMS` covers every payload member.

This release certifies only the named source commit. It does not transfer its proof verdict to later source trees and is not a cryptographic execution attestation.
"""


def github_json(path, *, check=True):
    result = command(["gh", "api", path], capture=True, check=check)
    if not check and result.returncode != 0:
        return result
    try:
        return json.loads(result.stdout, object_pairs_hook=unique_object)
    except (json.JSONDecodeError, ValueError) as error:
        raise ValueError(f"cannot parse GitHub response for {path}: {error}") from error


def require_github_absent(path, label):
    result = github_json(path, check=False)
    if not isinstance(result, subprocess.CompletedProcess):
        raise ValueError(f"{label} already exists")
    if "HTTP 404" not in result.stderr:
        raise ValueError(f"cannot prove {label} is absent: {result.stderr.strip()}")


def remote_tag_commit(repo, release_name):
    reference = github_json(f"repos/{repo}/git/ref/tags/{release_name}")
    target = reference.get("object", {})
    for _ in range(8):
        if target.get("type") == "commit":
            return target.get("sha")
        if target.get("type") != "tag" or not re.fullmatch(
            r"[0-9a-f]{40}", target.get("sha", "")
        ):
            break
        tag = github_json(f"repos/{repo}/git/tags/{target['sha']}")
        target = tag.get("object", {})
    raise ValueError("release tag does not resolve to a commit")


def release_metadata(repo, release_name):
    view = command(
        [
            "gh",
            "release",
            "view",
            release_name,
            "--repo",
            repo,
            "--json",
            "tagName,targetCommitish,url,isDraft,isPrerelease,assets",
        ],
        capture=True,
    )
    return json.loads(view.stdout, object_pairs_hook=unique_object)


def verify_release_metadata(
    metadata, release_name, archive, checksum, archive_hash, draft
):
    assets = {asset["name"]: asset for asset in metadata.get("assets", [])}
    uploaded = assets.get(archive.name)
    uploaded_checksum = assets.get(checksum.name)
    if (
        metadata.get("tagName") != release_name
        or metadata.get("isDraft") is not draft
        or metadata.get("isPrerelease") is not False
        or uploaded is None
        or uploaded_checksum is None
        or uploaded.get("state") != "uploaded"
        or uploaded_checksum.get("state") != "uploaded"
        or uploaded.get("digest") != f"sha256:{archive_hash}"
        or uploaded_checksum.get("digest") != f"sha256:{hash_file(checksum)}"
    ):
        raise ValueError("release metadata does not match the local artifact")


def create_remote_tag(repo, release_name, commit):
    command(
        [
            "gh",
            "api",
            f"repos/{repo}/git/refs",
            "--method",
            "POST",
            "-f",
            f"ref=refs/tags/{release_name}",
            "-f",
            f"sha={commit}",
        ]
    )
    if remote_tag_commit(repo, release_name) != commit:
        raise ValueError("created release tag does not resolve to the requested commit")


def publish_release(repo, release_name, commit, archive, checksum, notes, archive_hash):
    require_github_absent(
        f"repos/{repo}/releases/tags/{release_name}", "GitHub release"
    )
    require_github_absent(f"repos/{repo}/git/ref/tags/{release_name}", "GitHub tag")
    create_remote_tag(repo, release_name, commit)
    command(
        [
            "gh",
            "release",
            "create",
            release_name,
            str(archive),
            str(checksum),
            "--repo",
            repo,
            "--verify-tag",
            "--title",
            f"Kani run {commit[:7]} — {release_name[-10:]}",
            "--notes-file",
            str(notes),
            "--draft",
        ]
    )
    try:
        metadata = release_metadata(repo, release_name)
        verify_release_metadata(
            metadata, release_name, archive, checksum, archive_hash, True
        )
        with tempfile.TemporaryDirectory(prefix="kani-release-download-") as temporary:
            temporary = Path(temporary)
            command(
                [
                    "gh",
                    "release",
                    "download",
                    release_name,
                    "--repo",
                    repo,
                    "--pattern",
                    f"{release_name}.tar.zst*",
                    "--dir",
                    str(temporary),
                ]
            )
            downloaded = temporary / archive.name
            downloaded_checksum = temporary / checksum.name
            if hash_file(downloaded_checksum) != hash_file(checksum):
                raise ValueError("downloaded checksum asset digest mismatch")
            if hash_file(downloaded) != archive_hash:
                raise ValueError("downloaded release archive digest mismatch")
            verify_archive(downloaded, release_name)
        if remote_tag_commit(repo, release_name) != commit:
            raise ValueError("draft release tag no longer resolves to requested commit")
    except ValueError as error:
        raise ValueError(
            f"draft release {release_name} retained after verification failure: {error}"
        ) from error

    command(["gh", "release", "edit", release_name, "--repo", repo, "--draft=false"])
    try:
        metadata = release_metadata(repo, release_name)
        verify_release_metadata(
            metadata, release_name, archive, checksum, archive_hash, False
        )
        if remote_tag_commit(repo, release_name) != commit:
            raise ValueError(
                "published release tag does not resolve to the requested commit"
            )
    except ValueError as error:
        command(
            ["gh", "release", "edit", release_name, "--repo", repo, "--draft"],
            check=False,
        )
        raise ValueError(
            f"release {release_name} returned to draft after final verification failure: {error}"
        ) from error
    return metadata["url"]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--package-dir", required=True, type=Path)
    parser.add_argument("--run-dir", required=True, type=Path)
    parser.add_argument("--repo", required=True, help="GitHub owner/repository")
    parser.add_argument("--commit", default="HEAD")
    parser.add_argument("--output-dir", type=Path, default=Path("target/kani-releases"))
    parser.add_argument(
        "--dry-run",
        action="store_true",
        help="Build and verify locally without publishing",
    )
    args = parser.parse_args()

    try:
        repo_root = Path.cwd().resolve()
        if not (repo_root / ".git").exists():
            raise ValueError("run from the repository root")
        if not re.fullmatch(r"[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+", args.repo):
            raise ValueError("--repo must be owner/repository")
        package_input = require_directory(args.package_dir, "package directory")
        run_input = require_directory(args.run_dir, "run directory")
        if (
            package_input == run_input
            or package_input.is_relative_to(run_input)
            or run_input.is_relative_to(package_input)
        ):
            raise ValueError("package and run directories must not overlap")
        output_requested = args.output_dir.expanduser()
        if output_requested.is_symlink():
            raise ValueError(
                f"output directory must not be a symlink: {output_requested}"
            )
        output_candidate = output_requested.resolve(strict=False)
        if any(
            output_candidate == path or output_candidate.is_relative_to(path)
            for path in (package_input, run_input)
        ):
            raise ValueError(
                "output directory must not be inside package or run directory"
            )

        commit = resolve_commit(repo_root, args.commit)
        tree = command(
            ["git", "rev-parse", f"{commit}^{{tree}}"], cwd=repo_root, capture=True
        ).stdout.strip()
        commit_sources = source_digests(repo_root, commit)

        with tempfile.TemporaryDirectory(prefix="kani-release-stage-") as temporary:
            stage_parent = Path(temporary)
            release_root = stage_parent / "payload"
            package_dir = snapshot_directory(
                package_input, release_root / "proof-packaging", "package directory"
            )
            run_dir = snapshot_directory(
                run_input, release_root / "integrated-run", "run directory"
            )
            missing = sorted(
                name
                for name in REQUIRED_PACKAGE_FILES
                if not (package_dir / name).is_file()
            )
            if missing:
                raise ValueError(f"package directory is missing: {', '.join(missing)}")

            inventory_path = package_dir / "inventory.json"
            inventory_sha256 = hash_file(inventory_path)
            _, inventory = parse_inventory(inventory_path)
            summary = validate_results(run_dir, inventory, commit_sources)
            validate_campaign_receipt(package_dir, inventory_sha256, summary)
            run_date = dt.datetime.fromtimestamp(
                summary["started_at"], tz=dt.timezone.utc
            ).date()
            release_name = f"kani-run-{commit[:7]}-{run_date.isoformat()}"
            release_root.rename(stage_parent / release_name)
            release_root = stage_parent / release_name
            package_dir = release_root / "proof-packaging"
            run_dir = release_root / "integrated-run"

            manifest = {
                "schema_version": 1,
                "release": release_name,
                "run_date": run_date.isoformat(),
                "source": {
                    "commit": commit,
                    "tree": tree,
                    "binding": {
                        "algorithm": "SHA-256 over every tracked Rust source and Cargo/rust-toolchain manifest",
                        "files": len(commit_sources),
                        "retained_execution_records": summary["records"],
                        "all_before_after_unchanged": True,
                        "exact_commit_match": True,
                    },
                },
                "inventory": {
                    "path": "proof-packaging/inventory.json",
                    "sha256": inventory_sha256,
                    "unique_targets": summary["unique_targets"],
                    "retained_run_digests": summary["retained_inventory_digests"],
                },
                "execution": {
                    "accepted_targets": summary["unique_targets"],
                    "accepted_passes": summary["unique_targets"],
                    "accepted_failures": 0,
                    "retained_records": summary["records"],
                    "retained_verdicts": summary["verdicts"],
                    "result_files": summary["result_files"],
                },
                "package": {
                    "runner_sha256": hash_file(package_dir / "kani.py"),
                    "obligations_sha256": hash_file(
                        package_dir / "original-obligations.json"
                    ),
                    "campaign_receipt_sha256": hash_file(
                        package_dir / "integrated-execution-receipt.json"
                    ),
                    "member_manifest": "SHA256SUMS",
                },
                "authority": {
                    "publisher": "trusted local operator authenticated by gh",
                    "cryptographic_execution_attestation": False,
                },
                "claim_boundary": f"This release certifies only source commit {commit}.",
            }
            (release_root / "MANIFEST.json").write_text(
                json.dumps(manifest, indent=2) + "\n"
            )
            members = write_member_manifest(release_root)
            private_archive = stage_parent / f"{release_name}.tar.zst"
            private_checksum = stage_parent / f"{release_name}.tar.zst.sha256"
            private_notes = stage_parent / f"{release_name}.md"
            build_archive(stage_parent, release_name, run_date, private_archive)
            archive_hash = hash_file(private_archive)
            private_checksum.write_text(f"{archive_hash}  {private_archive.name}\n")
            private_notes.write_text(
                make_notes(
                    release_name,
                    commit,
                    tree,
                    archive_hash,
                    inventory_sha256,
                    summary,
                    members,
                )
            )
            verify_archive(private_archive, release_name)

            output_dir = prepare_output_directory(output_candidate)
            archive = output_dir / private_archive.name
            checksum = output_dir / private_checksum.name
            notes = output_dir / private_notes.name
            installed = []
            try:
                for source, destination in (
                    (private_archive, archive),
                    (private_checksum, checksum),
                    (private_notes, notes),
                ):
                    install_exclusive(source, destination)
                    installed.append(destination)
            except BaseException:
                for destination in installed:
                    destination.unlink(missing_ok=True)
                raise

            if args.dry_run:
                print(f"DRY RUN {archive} sha256={archive_hash}")
            else:
                url = publish_release(
                    args.repo,
                    release_name,
                    commit,
                    private_archive,
                    private_checksum,
                    private_notes,
                    archive_hash,
                )
                print(url)
        return 0
    except (OSError, ValueError, json.JSONDecodeError) as error:
        parser.error(str(error))


if __name__ == "__main__":
    raise SystemExit(main())
