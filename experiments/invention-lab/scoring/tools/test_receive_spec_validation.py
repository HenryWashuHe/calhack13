"""receive.py refuses a malformed or substituted spec before anything runs, and
records (never retries) lab failures inside a trial.

A mutation pass found every spec guard in `load_spec`, the retained-package
check and the trial failure paths untested: disabling any of them left the
suite green. The package hash check is what makes a published spec SHA-256
meaningful, so it needs a test.

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
import sys
import tempfile
import unittest
from unittest.mock import patch

MODULE = importlib.util.spec_from_file_location(
    "receive_spec_validation", Path(__file__).with_name("receive.py"))
receive = importlib.util.module_from_spec(MODULE)
MODULE.loader.exec_module(receive)


def valid(tmp):
    pkg = os.path.join(tmp, "a.package.json")
    with open(pkg, "wb") as f:
        f.write(b'{"package":{"id":"a","version":"1"}}')
    with open(pkg, "rb") as f:
        sha = hashlib.sha256(f.read()).hexdigest()
    return {"schema": "lab-receive-spec/1", "org_id": "org_b", "sandbox": "namespaces",
            "instances": 6, "simulation_steps": 100, "wall_time_ms": 100,
            "packages": {"cand": {"path": "a.package.json", "sha256": sha},
                         "ctrl": {"path": "a.package.json", "sha256": sha}},
            "trials": [{"id": "t-one", "family": "rough-transport",
                        "candidate": "cand", "control": "ctrl"}]}


def load(tmp, spec):
    path = os.path.join(tmp, "spec.json")
    with open(path, "w") as f:
        json.dump(spec, f)
    return receive.load_spec(path, tmp)


class SpecValidation(unittest.TestCase):
    def test_the_valid_spec_and_its_exact_boundaries_load(self):
        with tempfile.TemporaryDirectory() as tmp:
            spec = valid(tmp)
            load(tmp, spec)
            for k in ("instances", "simulation_steps", "wall_time_ms"):
                load(tmp, {**spec, k: 1})
            spec["trials"][0]["first_seed"] = 0
            load(tmp, spec)
            spec["sandbox"] = "none"
            load(tmp, spec)

    def test_each_malformed_or_substituted_spec_is_refused_before_any_run(self):
        cases = {
            "extra key": lambda s: s.update(extra=1),
            "schema": lambda s: s.update(schema="lab-receive-spec/2"),
            "sandbox": lambda s: s.update(sandbox="docker"),
            "zero instances": lambda s: s.update(instances=0),
            "bool steps": lambda s: s.update(simulation_steps=True),
            "string wall": lambda s: s.update(wall_time_ms="100"),
            "package keys": lambda s: s["packages"]["cand"].update(size=1),
            "package name": lambda s: s["packages"].update({"a/b": s["packages"]["cand"]}),
            "package bytes": lambda s: s["packages"]["cand"].update(sha256="0" * 64),
            "trial keys": lambda s: s["trials"][0].update(note="x"),
            "trial missing key": lambda s: s["trials"][0].pop("family"),
            "negative seed": lambda s: s["trials"][0].update(first_seed=-1),
            "bool seed": lambda s: s["trials"][0].update(first_seed=False),
            "seed range": lambda s: s["trials"][0].update(first_seed=(1 << 63) - 5),
            "repeated id": lambda s: s["trials"].append(dict(s["trials"][0])),
            "id name": lambda s: s["trials"][0].update(id="../t"),
            "unknown package": lambda s: s["trials"][0].update(control="other"),
            "no trials": lambda s: s.update(trials=[]),
        }
        for name, change in cases.items():
            with self.subTest(name), tempfile.TemporaryDirectory() as tmp:
                spec = valid(tmp)
                change(spec)
                with patch.object(receive, "run") as run, \
                        self.assertRaises(SystemExit):
                    load(tmp, spec)
                run.assert_not_called()

    def test_the_last_public_seed_range_is_allowed(self):
        with tempfile.TemporaryDirectory() as tmp:
            spec = valid(tmp)
            spec["trials"][0]["first_seed"] = (1 << 63) - 6
            spec["instances"] = 6
            load(tmp, spec)


class MainRefusals(unittest.TestCase):
    def call(self, argv, env):
        with patch.object(sys, "argv", argv), patch.dict(os.environ, env, clear=True), \
                patch.object(receive, "run") as run, \
                contextlib.redirect_stdout(io.StringIO()), self.assertRaises(SystemExit):
            receive.main()
        run.assert_not_called()

    def test_wrong_arguments_missing_binaries_and_existing_output_stop_first(self):
        with tempfile.TemporaryDirectory() as tmp:
            env = {"LAB_EXEC": "x", "LAB_SCORE": "y"}
            self.call(["receive.py", "spec.json"], env)
            self.call(["receive.py", "spec.json", os.path.join(tmp, "new")], {"LAB_EXEC": "x"})
            self.call(["receive.py", "spec.json", tmp], env)
            self.assertEqual(os.listdir(tmp), [])

    def test_a_trial_that_raises_stops_the_run_and_names_it_in_the_summary(self):
        with tempfile.TemporaryDirectory() as tmp:
            spec = valid(tmp)
            spec["trials"].append({**spec["trials"][0], "id": "t-two"})
            path = os.path.join(tmp, "spec.json")
            with open(path, "w") as f:
                json.dump(spec, f)
            out = os.path.join(tmp, "out")
            calls = []

            def boom(spec, packages, t, d, lab_exec, lab_score):
                calls.append(t["id"])
                raise RuntimeError("disk")
            with patch.object(sys, "argv", ["receive.py", path, out]), \
                    patch.dict(os.environ, {"LAB_EXEC": "x", "LAB_SCORE": "y"}), \
                    patch.object(receive, "run", return_value=(0, {"id": "python3"})), \
                    patch.object(receive, "trial", boom), \
                    contextlib.redirect_stdout(io.StringIO()), self.assertRaises(SystemExit):
                receive.main()
            self.assertEqual(calls, ["t-one"])
            with open(os.path.join(out, "summary.json")) as f:
                summary = json.load(f)
            self.assertEqual(summary["stopped"], "t-one")
            self.assertEqual(summary["trials"], [{"trial": "t-one", "error": "RuntimeError: disk"}])


class TrialFailures(unittest.TestCase):
    """`trial` with a scripted `run`: each failure is recorded once."""

    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        tmp = self.tmp.name
        self.spec = valid(tmp)
        self.pkg = os.path.join(tmp, "a.package.json")
        self.d = os.path.join(tmp, "t-one")
        os.makedirs(self.d)
        self.calls = []

    def scripted(self, plan_ok=True, receipt=True, verdict_code=0, feedback_code=0):
        d = self.d

        def run(args, out=None):
            self.calls.append(args[1])
            if out:
                open(out, "w").close()
                open(out + ".err", "w").close()
            if args[1] == "example-plan":
                if not plan_ok:
                    return 1, None
                plan = {"id": "example", "slots": [
                    {"slot_id": s, "org_id": "x", "run_id": "r", "attempt_id": "a",
                     "scoring_request_id": "s", "method": {}} for s in ("candidate", "control")]}
                with open(os.path.join(d, "plan.json"), "w") as f:
                    json.dump(plan, f)
                return 0, {}
            if args[1] == "run":
                if receipt:
                    os.makedirs(args[5], exist_ok=True)
                    open(os.path.join(args[5], "receipt.json"), "w").close()
                return 0, {"outcome": "output_generated", "usage": {}}
            if args[1] == "score":
                return 0, {"result": {"success_count": 1, "of": 6}}
            if args[1] == "verdict":
                return verdict_code, {"verdict": "accepted"}
            if args[1] == "feedback":
                return feedback_code, {"feedback": "x"}
            raise AssertionError(args)
        return run

    def trial(self, **kw):
        packages = {"cand": self.pkg, "ctrl": self.pkg}
        with patch.object(receive, "run", self.scripted(**kw)):
            return receive.trial(self.spec, packages, self.spec["trials"][0], self.d, "lab-exec", "lab-score")

    def test_a_changed_retained_package_runs_nothing(self):
        with open(self.pkg, "ab") as f:
            f.write(b" ")
        result = self.trial()
        self.assertIn("changed", result["error"])
        self.assertEqual(self.calls, [])

    def test_a_plan_that_cannot_be_built_runs_no_slot(self):
        result = self.trial(plan_ok=False)
        self.assertEqual(result, {"trial": "t-one", "error": "plan could not be built"})
        self.assertEqual(self.calls, ["example-plan"])

    def test_a_slot_without_a_receipt_is_not_scored(self):
        result = self.trial(receipt=False)
        self.assertNotIn("score", self.calls)
        for slot in ("candidate", "control"):
            self.assertNotIn("score_exit", result["slots"][slot])
            self.assertEqual(result["slots"][slot]["outcome"], "output_generated")

    def test_a_scored_slot_records_its_result(self):
        result = self.trial()
        self.assertEqual(self.calls.count("score"), 2)
        self.assertEqual(result["slots"]["candidate"]["result"], {"success_count": 1, "of": 6})
        self.assertEqual(result["verdict"], {"verdict": "accepted"})
        self.assertEqual(result["candidate_feedback"], {"feedback": "x"})

    def test_failed_verdict_and_feedback_are_recorded_not_trusted(self):
        result = self.trial(verdict_code=1, feedback_code=1)
        self.assertEqual(result["verdict"], {"error": "no verdict"})
        self.assertIsNone(result["candidate_feedback"])


if __name__ == "__main__":
    unittest.main()
