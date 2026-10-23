"""Regression checks for proof selection and checked-dependency admission."""

import json
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
    def rejected(self, entries):
        with tempfile.TemporaryDirectory(prefix="verify-local-inventory-") as temporary:
            path = Path(temporary) / "proofs.json"
            path.write_text(json.dumps(entries))
            with mock.patch.object(verify_local, "INVENTORY", path):
                with self.assertRaises(ValueError):
                    verify_local.load_inventory()

    def test_invalid_selection_cannot_enter_proof_execution(self):
        entry = {
            "target": "pool",
            "harness": "pool::reserve::verification::example",
            "expected_covers": 1,
            "seconds": 1,
            "depends_on": [],
        }
        self.rejected([])
        for row in (None, 7, list(entry)):
            with self.subTest(row=row):
                self.rejected([row])
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
                    self.rejected([{**entry, field: value}])
        self.rejected([entry, entry])

    def test_a_contract_caller_cannot_precede_its_dependency(self):
        callee = {
            "target": "contracts",
            "harness": "pool::actions::verification::prove_supply_chain_contract",
            "expected_covers": 3,
            "seconds": 300,
            "depends_on": [],
        }
        caller = {
            **callee,
            "harness": "pool::actions::verification::prove_supply_cap_decides_on_post_state",
            "expected_covers": 2,
            "depends_on": [callee["harness"]],
        }
        self.rejected([caller])
        self.rejected([caller, callee])


if __name__ == "__main__":
    unittest.main()
