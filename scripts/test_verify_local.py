"""Regression checks for proof selection and checked-dependency admission."""

import json
from itertools import combinations
from pathlib import Path
import sys
import tempfile
import unittest
from unittest import mock

sys.path.insert(0, str(Path(__file__).resolve().parent))
try:
    import verify_local
finally:
    sys.path.pop(0)


class InventoryTest(unittest.TestCase):
    def setUp(self):
        self.entries = json.loads(verify_local.INVENTORY.read_text())

    def rejected(self, entries):
        with tempfile.TemporaryDirectory(prefix="verify-local-inventory-") as temporary:
            path = Path(temporary) / "proofs.json"
            path.write_text(json.dumps(entries))
            with mock.patch.object(verify_local, "INVENTORY", path):
                with self.assertRaises(ValueError):
                    verify_local.load_inventory()

    def test_invalid_selection_cannot_enter_proof_execution(self):
        entry = self.entries[0]
        self.rejected([])
        for row in (None, 7, list(entry)):
            with self.subTest(row=row):
                self.rejected([row, *self.entries[1:]])
        invalid_fields = {
            "target": [[], "unknown"],
            "harness": ["pool::", "pool::bad/name", "pool::a::::b", "pool::a b"],
            "expected_covers": [0, True],
            "seconds": [0, 3601, True],
            "depends_on": [None, [entry["harness"]]],
        }
        for field, values in invalid_fields.items():
            for value in values:
                with self.subTest(field=field, value=value):
                    self.rejected([{**entry, field: value}, *self.entries[1:]])
        self.rejected([*self.entries, entry])

    def test_a_contract_caller_cannot_precede_its_dependency(self):
        entries = self.entries.copy()
        entries[1], entries[2] = entries[2], entries[1]
        self.rejected(entries)
        entries = self.entries.copy()
        entries[2] = {**entries[2], "depends_on": ["missing::callee"]}
        self.rejected(entries)

    def test_no_nonempty_proper_subset_can_pass_acceptance(self):
        for size in range(1, len(self.entries)):
            for entries in combinations(self.entries, size):
                with self.subTest(harnesses=[entry["harness"] for entry in entries]):
                    self.rejected(list(entries))

    def test_unexpected_proofs_cannot_replace_the_acceptance_contract(self):
        extra = {**self.entries[0], "harness": "unexpected::proof"}
        self.rejected([*self.entries, extra])


if __name__ == "__main__":
    unittest.main()
