#!/usr/bin/env python3

import datetime as dt
import importlib.util
import hashlib
import json
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest import mock


SCRIPT = Path(__file__).with_name("kani_release.py")
SPEC = importlib.util.spec_from_file_location("kani_release", SCRIPT)
KANI_RELEASE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(KANI_RELEASE)


def run(argv, cwd, *, check=True):
    return subprocess.run(
        argv,
        cwd=cwd,
        check=check,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    )


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


class KaniReleaseTest(unittest.TestCase):
    def test_dry_run_binds_results_to_commit(self):
        with tempfile.TemporaryDirectory(prefix="kani-release-test-") as temporary:
            root = Path(temporary)
            repo = root / "repo"
            package = root / "package"
            results = root / "results" / "shard-0"
            output = root / "output"
            (repo / "src").mkdir(parents=True)
            package.mkdir()
            results.mkdir(parents=True)
            (repo / "Cargo.toml").write_text(
                '[package]\nname = "proof-fixture"\nversion = "0.0.0"\n'
            )
            (repo / "src/lib.rs").write_text("pub fn value() -> u8 { 7 }\n")
            run(["git", "init", "-q"], repo)
            run(["git", "config", "user.name", "Kani Release Test"], repo)
            run(
                ["git", "config", "user.email", "kani-release-test@example.invalid"],
                repo,
            )
            run(["git", "add", "."], repo)
            run(
                ["git", "-c", "commit.gpgsign=false", "commit", "-q", "-m", "fixture"],
                repo,
            )
            commit = run(["git", "rev-parse", "HEAD"], repo).stdout.strip()

            harness = "pool::verification::prove_fixture"
            inventory = [
                {
                    "id": harness,
                    "package": "pool",
                    "harness": harness,
                    "expected_covers": 1,
                    "tests": True,
                }
            ]
            inventory_path = package / "inventory.json"
            inventory_path.write_text(json.dumps(inventory, indent=2) + "\n")
            (package / "kani.py").write_text("# fixture runner\n")
            (package / "original-obligations.json").write_text("{}\n")
            (package / "integrated-execution-receipt.json").write_text(
                json.dumps(
                    {
                        "status": "COMPLETE",
                        "inventory_sha256": digest(inventory_path),
                        "targets": 1,
                        "passed": 1,
                        "failed": 0,
                    },
                    indent=2,
                )
                + "\n"
            )
            sources = {
                "Cargo.toml": digest(repo / "Cargo.toml"),
                "src/lib.rs": digest(repo / "src/lib.rs"),
            }
            started_at = dt.datetime(2026, 9, 15, tzinfo=dt.timezone.utc).timestamp()
            log_text = (
                "VERIFICATION:- SUCCESSFUL\n"
                "1 of 1 cover properties satisfied\n"
                "Complete - 1 successfully verified harnesses, 0 failures, 1 total.\n"
            )
            log_path = results / "0001.log"
            log_path.write_text(log_text)
            record = {
                "harness": harness,
                "package": "pool",
                "expected_covers": 1,
                "tests": True,
                "log": log_path.name,
                "log_sha256": digest(log_path),
                "verdict": "PASS",
                "covers_passed": True,
                "cover_summaries": [[1, 1]],
                "terminal_summaries": [["1", "0", "1"]],
                "verification_summaries": ["SUCCESSFUL"],
                "exit": 0,
                "signal": None,
                "execution_error": None,
                "source_unchanged": True,
                "source_sha256_before": sources,
                "source_sha256_after": sources,
                "inventory_sha256": digest(inventory_path),
                "started_at": started_at,
            }
            result_path = results / "results.json"

            def publish(destination, *, check=True):
                result_path.write_text(json.dumps([record], indent=2) + "\n")
                return run(
                    [
                        "python3",
                        str(SCRIPT),
                        "--package-dir",
                        str(package),
                        "--run-dir",
                        str(results.parent),
                        "--repo",
                        "example/repository",
                        "--commit",
                        commit,
                        "--output-dir",
                        str(destination),
                        "--dry-run",
                    ],
                    repo,
                    check=check,
                )

            completed = publish(output)
            release = f"kani-run-{commit[:7]}-2026-09-15"
            self.assertIn("DRY RUN", completed.stdout)
            self.assertTrue((output / f"{release}.tar.zst").is_file())
            self.assertTrue((output / f"{release}.tar.zst.sha256").is_file())

            record["source_sha256_before"] = {**sources, "src/lib.rs": "0" * 64}
            record["source_sha256_after"] = record["source_sha256_before"]
            rejected = publish(root / "source-rejected", check=False)
            self.assertNotEqual(0, rejected.returncode)
            self.assertIn("source digests do not match", rejected.stderr)
            self.assertFalse((root / "source-rejected").exists())

            record["source_sha256_before"] = sources
            record["source_sha256_after"] = sources
            record.pop("tests")
            rejected = publish(root / "tests-rejected", check=False)
            self.assertNotEqual(0, rejected.returncode)
            self.assertIn("test mode differs", rejected.stderr)
            self.assertFalse((root / "tests-rejected").exists())

            record["tests"] = True
            log_path.write_text(log_text + "\n")
            rejected = publish(root / "log-rejected", check=False)
            self.assertNotEqual(0, rejected.returncode)
            self.assertIn("retained log digest mismatch", rejected.stderr)
            self.assertFalse((root / "log-rejected").exists())

            failed_log = (
                "VERIFICATION:- FAILED\n"
                "1 of 1 cover properties satisfied\n"
                "Complete - 0 successfully verified harnesses, 1 failures, 1 total.\n"
            )
            log_path.write_text(failed_log)
            record["log_sha256"] = digest(log_path)
            rejected = publish(root / "verdict-rejected", check=False)
            self.assertNotEqual(0, rejected.returncode)
            self.assertIn("result fields do not match retained log", rejected.stderr)
            self.assertFalse((root / "verdict-rejected").exists())

    def test_publish_checks_exact_tag_before_publication(self):
        release_name = "kani-run-1234567-2026-09-15"
        commit = "1" * 40
        archive_hash = "a" * 64
        archive = Path("/tmp") / f"{release_name}.tar.zst"
        checksum = Path("/tmp") / f"{archive.name}.sha256"
        notes = Path("/tmp/RELEASE_NOTES.md")
        events = []

        def fake_command(argv, **_kwargs):
            events.append(("command", tuple(argv)))
            return subprocess.CompletedProcess(argv, 0, "", "")

        def fake_tag(*_args):
            events.append(("tag", commit))
            return commit

        with (
            mock.patch.object(KANI_RELEASE, "command", side_effect=fake_command),
            mock.patch.object(KANI_RELEASE, "require_github_absent"),
            mock.patch.object(
                KANI_RELEASE,
                "release_metadata",
                return_value={"url": "https://example.invalid/release"},
            ),
            mock.patch.object(KANI_RELEASE, "verify_release_metadata"),
            mock.patch.object(KANI_RELEASE, "remote_tag_commit", side_effect=fake_tag),
            mock.patch.object(KANI_RELEASE, "verify_archive"),
            mock.patch.object(
                KANI_RELEASE,
                "hash_file",
                side_effect=lambda path: (
                    archive_hash if Path(path).name.endswith(".tar.zst") else "checksum"
                ),
            ),
        ):
            url = KANI_RELEASE.publish_release(
                "Templar-Protocol/blend-contracts-v2",
                release_name,
                commit,
                archive,
                checksum,
                notes,
                archive_hash,
            )

        commands = [event[1] for event in events if event[0] == "command"]
        create = next(
            args for args in commands if args[:3] == ("gh", "release", "create")
        )
        tag_create = next(
            args
            for args in commands
            if args[:2] == ("gh", "api") and args[2].endswith("/git/refs")
        )
        publish = next(
            index
            for index, event in enumerate(events)
            if event[0] == "command" and "--draft=false" in event[1]
        )
        tag_checks = [index for index, event in enumerate(events) if event[0] == "tag"]
        self.assertEqual("https://example.invalid/release", url)
        self.assertIn("--verify-tag", create)
        self.assertNotIn("--target", create)
        self.assertIn(f"ref=refs/tags/{release_name}", tag_create)
        self.assertIn(f"sha={commit}", tag_create)
        self.assertEqual(3, len(tag_checks))
        self.assertLess(tag_checks[1], publish)
        self.assertLess(publish, tag_checks[2])

    def test_publish_retains_draft_when_tag_moves(self):
        release_name = "kani-run-1234567-2026-09-15"
        commit = "1" * 40
        archive_hash = "a" * 64
        archive = Path("/tmp") / f"{release_name}.tar.zst"
        checksum = Path("/tmp") / f"{archive.name}.sha256"
        events = []
        tags = iter([commit, "2" * 40])

        def fake_command(argv, **_kwargs):
            events.append(tuple(argv))
            return subprocess.CompletedProcess(argv, 0, "", "")

        with (
            mock.patch.object(KANI_RELEASE, "command", side_effect=fake_command),
            mock.patch.object(KANI_RELEASE, "require_github_absent"),
            mock.patch.object(
                KANI_RELEASE,
                "release_metadata",
                return_value={"url": "https://example.invalid/release"},
            ),
            mock.patch.object(KANI_RELEASE, "verify_release_metadata"),
            mock.patch.object(
                KANI_RELEASE, "remote_tag_commit", side_effect=lambda *_: next(tags)
            ),
            mock.patch.object(KANI_RELEASE, "verify_archive"),
            mock.patch.object(
                KANI_RELEASE,
                "hash_file",
                side_effect=lambda path: (
                    archive_hash if Path(path).name.endswith(".tar.zst") else "checksum"
                ),
            ),
        ):
            with self.assertRaisesRegex(
                ValueError, "retained after verification failure"
            ):
                KANI_RELEASE.publish_release(
                    "Templar-Protocol/blend-contracts-v2",
                    release_name,
                    commit,
                    archive,
                    checksum,
                    Path("/tmp/RELEASE_NOTES.md"),
                    archive_hash,
                )

        self.assertTrue(any(args[:3] == ("gh", "release", "create") for args in events))
        self.assertFalse(any("--draft=false" in args for args in events))


if __name__ == "__main__":
    unittest.main()
