"""Consumer-facing regressions for Kani acceptance and private provisioning."""

import hashlib
import io
import json
from pathlib import Path
import stat
import subprocess
import sys
import tempfile
import unittest
from unittest import mock
import zipfile
sys.path.insert(0, str(Path(__file__).resolve().parent))


import check_kani_ci as checker
import provision_verus as provisioner

ROOT = Path(__file__).resolve().parent.parent
POSITIVE = "pool::actions::verification::prove_supply_cap_decides_on_post_state"
ZERO = "pool::reserve::verification::prove_b_token_forward_floor"


def log(harness=POSITIVE, summary=" ** 2 of 2 cover properties satisfied\n"):
    return (f"Checking harness {harness}...\n" + summary
            + "VERIFICATION:- SUCCESSFUL\n"
            + "Complete - 1 successfully verified harnesses, 0 failures, 1 total.\n")


class KaniAcceptanceTest(unittest.TestCase):
    def invoke(self, text, *, harness=POSITIVE, exit_code=0):
        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary) / "proof.log"
            path.write_text(text)
            return subprocess.run([
                sys.executable, str(ROOT / "scripts/check_kani_ci.py"),
                "--selection", str(ROOT / "verification/kani-ci-harnesses.txt"),
                "--harness", harness, "--log", str(path), "--exit-code", str(exit_code),
            ], capture_output=True, text=True, timeout=15)

    def test_complete_pass_and_zero_cover_pass(self):
        for harness, summary in ((POSITIVE, " ** 2 of 2 cover properties satisfied\n"),
                                 (ZERO, ""), (ZERO, " ** 0 of 0 cover properties satisfied\n")):
            with self.subTest(harness=harness, summary=summary):
                result = self.invoke(log(harness, summary), harness=harness)
                self.assertEqual(result.returncode, 0, result.stderr)

    def test_cover_contract_rejects_missing_wrong_and_multiple_summaries(self):
        for summary in ("", " ** 1 of 2 cover properties satisfied\n",
                        " ** 1 of 1 cover properties satisfied\n",
                        " ** 3 of 3 cover properties satisfied\n",
                        " ** two of two cover properties satisfied\n",
                        " ** 2 of 2 cover properties satisfied\n" * 2):
            with self.subTest(summary=summary):
                result = self.invoke(log(summary=summary))
                self.assertNotEqual(result.returncode, 0)
                self.assertIn("expected exactly 2", result.stderr)

    def test_zero_covers_rejects_nonzero_or_multiple_even_when_satisfied(self):
        for summary in (" ** 1 of 1 cover properties satisfied\n",
                        " ** 0 of 0 cover properties satisfied\n" * 2):
            result = self.invoke(log(ZERO, summary), harness=ZERO)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn("expected exactly 0", result.stderr)

    def test_verdict_integrity_and_real_process_status(self):
        valid = log()
        variants = [
            valid.replace("VERIFICATION:- SUCCESSFUL\n", ""),
            valid.replace("VERIFICATION:- SUCCESSFUL", "VERIFICATION:- SUCCESSFUL garbage"),
            valid.replace("VERIFICATION:- SUCCESSFUL", "VERIFICATION:- FAILED"),
            valid + "VERIFICATION:- SUCCESSFUL\n",
            valid + "VERIFICATION\n",
            valid.replace("Complete - 1 successfully", "Complete - 2 successfully"),
            valid + "Complete - 1 successfully verified harnesses, 0 failures, 1 total.\n",
            valid.replace("Complete - 1 successfully verified harnesses, 0 failures, 1 total.\n", ""),
            valid.replace(POSITIVE, ZERO),
        ]
        for text in variants:
            with self.subTest(text=text):
                self.assertNotEqual(self.invoke(text).returncode, 0)
        for code in (1, 124, 137, -9):
            result = self.invoke(valid, exit_code=code)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn(f"process exit={code}", result.stderr)

    def test_inventory_must_cover_exact_selector_union(self):
        entries = json.loads(checker.INVENTORY.read_text())
        for changed in (entries[:-1], entries + [{
                "id": "pool::unexpected", "harness": "pool::unexpected",
                "package": "pool", "expected_covers": 0}]):
            with tempfile.TemporaryDirectory() as temporary:
                path = Path(temporary) / "inventory.json"
                path.write_text(json.dumps(changed))
                with self.assertRaisesRegex(ValueError, "selector union"):
                    checker.selected_entry(checker.SELECTIONS[0], POSITIVE, path)
        with self.assertRaisesRegex(ValueError, "not in the requested selection"):
            checker.selected_entry(checker.SELECTIONS[1], POSITIVE)


class ProvisioningBoundaryTest(unittest.TestCase):
    def test_private_home_creation_and_unsafe_custody(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            home = provisioner.private_home(root / "parent" / "verus")
            self.assertEqual(home.stat().st_mode & 0o777, 0o700)
            self.assertEqual(home.parent.stat().st_mode & 0o777, 0o700)
            home.chmod(0o777)
            with self.assertRaisesRegex(ValueError, "exclusively writable"):
                provisioner.private_home(home)
            home.chmod(0o700)
            alias = root / "alias"
            alias.symlink_to(home.parent, target_is_directory=True)
            with self.assertRaisesRegex(ValueError, "unsafe tool ancestor"):
                provisioner.private_home(alias / "verus")
            home.parent.chmod(0o777)
            with self.assertRaisesRegex(ValueError, "writable tool ancestor"):
                provisioner.private_home(home)

    def test_archive_rejects_escape_symlink_and_duplicate_members(self):
        cases = [("../escaped", stat.S_IFREG | 0o644),
                 ("verus-x86-linux/../../escaped", stat.S_IFREG | 0o644),
                 ("verus-x86-linux/verus", stat.S_IFLNK | 0o777),
                 ("/verus-x86-linux/verus", stat.S_IFREG | 0o644)]
        for name, mode in cases:
            with self.subTest(name=name), tempfile.TemporaryDirectory() as temporary:
                root = Path(temporary)
                archive = root / "bundle.zip"
                with zipfile.ZipFile(archive, "w") as output:
                    member = zipfile.ZipInfo(name)
                    member.external_attr = mode << 16
                    output.writestr(member, b"untrusted")
                with self.assertRaisesRegex(ValueError, "unsafe Verus archive member"):
                    provisioner.extract_bundle(archive, root)
                self.assertFalse((root.parent / "escaped").exists())

    def test_archive_rejects_duplicate_normalized_names(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            archive = root / "bundle.zip"
            with zipfile.ZipFile(archive, "w") as output:
                output.writestr("verus-x86-linux/tool", b"first")
                output.writestr("verus-x86-linux/./tool", b"second")
            with self.assertRaisesRegex(ValueError, "unsafe Verus archive member"):
                provisioner.extract_bundle(archive, root)

    def test_archive_keeps_executability_without_shared_write_access(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            archive = root / "bundle.zip"
            with zipfile.ZipFile(archive, "w") as output:
                member = zipfile.ZipInfo("verus-x86-linux/verus")
                member.external_attr = (stat.S_IFREG | 0o777) << 16
                output.writestr(member, b"binary fixture")
            provisioner.extract_bundle(archive, root)
            installed = root / "verus-x86-linux/verus"
            self.assertEqual(installed.read_bytes(), b"binary fixture")
            self.assertEqual(installed.stat().st_mode & 0o777, 0o700)

    def test_download_checks_actual_stream_hash_and_https(self):
        class Response(io.BytesIO):
            url = "https://example.invalid/pinned.zip"

        data = b"actual downloaded archive bytes" * 100
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            for expected, succeeds in ((hashlib.sha256(data).hexdigest(), True), ("0" * 64, False)):
                destination = root / ("good" if succeeds else "bad")
                with mock.patch.object(provisioner.urllib.request, "urlopen", return_value=Response(data)):
                    if succeeds:
                        provisioner.download(Response.url, destination, expected)
                        self.assertEqual(destination.read_bytes(), data)
                    else:
                        with self.assertRaisesRegex(ValueError, "download hash mismatch"):
                            provisioner.download(Response.url, destination, expected)
            with self.assertRaisesRegex(ValueError, "require HTTPS"):
                provisioner.download("http://example.invalid/archive", root / "http", "0" * 64)


if __name__ == "__main__":
    unittest.main()
