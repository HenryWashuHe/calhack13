"""receive.py namespaces run, attempt and scoring IDs by the predeclared trial:
distinct trials of one organization never share them, and preparing the same
trial again is stable. Requested in issue #1 (comment 6389372825).

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
    "receive_trial_ids", Path(__file__).with_name("receive.py"))
receive = importlib.util.module_from_spec(MODULE)
MODULE.loader.exec_module(receive)

# The shape `lab-exec example-plan` writes: the same slot IDs for every trial.
EXAMPLE_SLOTS = [
    {"slot_id": s, "scoring_request_id": f"score-{s}", "org_id": "org_example_b",
     "run_id": f"run-{s}", "attempt_id": f"att_{s}", "method": {}, "dependencies": []}
    for s in ("candidate", "control")
]
ID_FIELDS = ("run_id", "attempt_id", "scoring_request_id")


class TrialIdTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="receive-ids-")
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        data = json.dumps({"package": {"id": "m", "version": "0.1.0"}}).encode()
        (self.root / "m.json").write_bytes(data)
        self.spec = {
            "schema": "lab-receive-spec/1", "org_id": "org_b", "sandbox": "none",
            "instances": 6, "simulation_steps": 4800, "wall_time_ms": 1000,
            "packages": {"m": {"path": "m.json", "sha256": hashlib.sha256(data).hexdigest()}},
            "trials": [
                {"id": "w1-rough", "family": "rough-transport", "candidate": "m", "control": "m"},
                {"id": "f1-rough", "family": "rough-transport", "candidate": "m", "control": "m"},
            ],
        }
        self.run_plans = []

    def fake(self, args, **kwargs):
        verb = args[1]
        if verb == "example-plan":
            d = Path(args[6])
            (d / "plan.json").write_text(json.dumps({"id": "p", "slots": EXAMPLE_SLOTS}))
            (d / "criterion.json").write_text("{}")
        if verb == "run":
            self.run_plans.append(json.loads(Path(args[2]).read_text()))
        return subprocess.CompletedProcess(args, 1 if verb == "run" else 0, "{}", "")

    def invoke(self, out):
        spec = self.root / "spec.json"
        spec.write_text(json.dumps(self.spec))
        with patch.dict(os.environ, {"LAB_EXEC": "x", "LAB_SCORE": "y"}), \
             patch.object(sys, "argv", ["receive.py", str(spec), str(self.root / out)]), \
             patch.object(receive.subprocess, "run", side_effect=self.fake), \
             contextlib.redirect_stdout(io.StringIO()):
            receive.main()
        return {t["id"]: json.loads((self.root / out / t["id"] / "plan.json").read_text())
                for t in self.spec["trials"]}

    def ids(self, plan):
        return [(s["org_id"], s[f]) for s in plan["slots"] for f in ID_FIELDS]

    def test_distinct_trials_never_share_execution_or_scoring_ids(self):
        plans = self.invoke("out")
        first, second = self.ids(plans["w1-rough"]), self.ids(plans["f1-rough"])
        self.assertEqual(len(set(first + second)), len(first) + len(second))
        slot = plans["w1-rough"]["slots"][0]
        self.assertEqual(slot["run_id"], "run-receive-w1-rough-candidate")
        self.assertEqual(slot["attempt_id"], "att_receive-w1-rough_candidate")
        self.assertEqual(slot["scoring_request_id"], "score-receive-w1-rough-candidate")
        self.assertEqual(plans["w1-rough"]["slots"][1]["slot_id"], "control")

    def test_ids_are_bound_before_any_run(self):
        plans = self.invoke("out")
        # Every `lab-exec run` already reads the namespaced plan.
        self.assertTrue(self.run_plans)
        for plan in self.run_plans:
            self.assertIn(plan, plans.values())

    def test_preparing_the_same_trial_again_is_stable(self):
        self.assertEqual(self.invoke("out-1"), self.invoke("out-2"))


if __name__ == "__main__":
    unittest.main()
