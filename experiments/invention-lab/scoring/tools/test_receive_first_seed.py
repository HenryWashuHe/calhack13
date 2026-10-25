"""receive.py's optional first_seed: passed through only when declared, and
refused when the range would reach the reserved hidden-seed domain.

Run with: python3 -m unittest discover -s experiments/invention-lab/scoring/tools
Every lab subprocess is mocked; nothing is executed.
"""
import contextlib
import hashlib
import importlib.util
import io
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

MODULE = importlib.util.spec_from_file_location(
    "receive_first_seed", Path(__file__).with_name("receive.py"))
receive = importlib.util.module_from_spec(MODULE)
MODULE.loader.exec_module(receive)


class FirstSeedTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="receive-seed-")
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        pkg = {"package": {"id": "m", "version": "0.1.0"}}
        data = json.dumps(pkg).encode()
        (self.root / "m.json").write_bytes(data)
        self.spec = {
            "schema": "lab-receive-spec/1", "org_id": "org_a", "sandbox": "none",
            "instances": 12, "simulation_steps": 4800, "wall_time_ms": 1000,
            "packages": {"m": {"path": "m.json", "sha256": hashlib.sha256(data).hexdigest()}},
            "trials": [
                {"id": "dev", "family": "rough-transport", "candidate": "m", "control": "m"},
                {"id": "fresh", "family": "rough-transport", "candidate": "m", "control": "m",
                 "first_seed": 1000},
            ],
        }
        self.plans = []

    def fake(self, args, **kwargs):
        verb = args[1]
        if verb == "example-plan":
            self.plans.append(list(args))
            d = Path(args[6])
            (d / "plan.json").write_text(json.dumps({"id": "p", "slots": [{}, {}]}))
            (d / "criterion.json").write_text("{}")
        return subprocess.CompletedProcess(args, 1 if verb == "run" else 0, "{}", "")

    def invoke(self):
        spec = self.root / "spec.json"
        spec.write_text(json.dumps(self.spec))
        with patch.dict(os.environ, {"LAB_EXEC": "x", "LAB_SCORE": "y"}), \
             patch.object(sys, "argv", ["receive.py", str(spec), str(self.root / "out")]), \
             patch.object(receive.subprocess, "run", side_effect=self.fake), \
             contextlib.redirect_stdout(io.StringIO()):
            receive.main()

    def test_seed_is_passed_only_when_declared(self):
        self.invoke()
        self.assertEqual(len(self.plans), 2)
        self.assertEqual(len(self.plans[0]), 9)
        self.assertEqual(self.plans[1][9:], ["1000"])
        summary = json.loads((self.root / "out" / "summary.json").read_text())
        self.assertEqual([t["first_seed"] for t in summary["trials"]], [0, 1000])

    def test_a_range_reaching_hidden_seeds_is_refused_before_any_effect(self):
        self.spec["trials"][1]["first_seed"] = (1 << 63) - 11
        with self.assertRaises(SystemExit):
            self.invoke()
        self.assertEqual(self.plans, [])
        self.assertFalse((self.root / "out").exists())
        self.spec["trials"][1]["first_seed"] = (1 << 63) - 12
        self.invoke()
        self.assertEqual(len(self.plans), 2)

    def test_non_integer_seed_is_refused(self):
        for bad in ("1000", True, -1, 1.5):
            self.spec["trials"][1]["first_seed"] = bad
            with self.assertRaises(SystemExit):
                self.invoke()
        self.assertEqual(self.plans, [])


if __name__ == "__main__":
    unittest.main()
