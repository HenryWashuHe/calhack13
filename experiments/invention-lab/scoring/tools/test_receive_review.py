"""Independent receiver-tool regressions; every lab subprocess is a Python mock.

Run with: python3 -m unittest discover -s experiments/invention-lab/scoring/tools
No executor, scorer, sandbox, provider, or method source is executed here.
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
    "receiver_under_review", Path(__file__).with_name("receive.py")
)
receive = importlib.util.module_from_spec(MODULE)
MODULE.loader.exec_module(receive)


def digest(data):
    return hashlib.sha256(data).hexdigest()


def package_bytes(name):
    # Start from an existing exact-format package rather than a metadata-only
    # object that the real executor would reject. Its contents are never run.
    root = Path(__file__).resolve().parents[4]
    fixture = root / "docs/invention-lab/evidence/live-method-20261024-receiver/control.package.json"
    package = json.loads(fixture.read_text())
    package["package"]["id"] = name
    package["package"]["files"][0]["content"] = "# " + name + "\n" + package["package"]["files"][0]["content"]
    return json.dumps(package, sort_keys=True, separators=(",", ":")).encode()


class ReceiverBoundaryTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="receive-review-")
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.out = self.root / "results"
        self.spec_path = self.root / "spec.json"
        self.originals = {role: package_bytes(role) for role in ("candidate", "control")}
        for role, data in self.originals.items():
            (self.root / (role + ".json")).write_bytes(data)
        self.spec = {
            "schema": "lab-receive-spec/1", "org_id": "org_original",
            "sandbox": "none", "instances": 2,
            "simulation_steps": 4800, "wall_time_ms": 1000,
            "packages": {
                role: {"path": role + ".json", "sha256": digest(data)}
                for role, data in self.originals.items()
            },
            "trials": [{"id": "first", "family": "rough-transport",
                        "candidate": "candidate", "control": "control"}],
        }
        self.save_spec()
        self.calls = []
        self.executed = []
        self.on_runtime_ref = None
        self.fail_plan_id = None
        self.run_exit = 0

    def save_spec(self):
        self.spec_path.write_text(json.dumps(self.spec))

    def fake_subprocess(self, args, **kwargs):
        """A file-producing CLI stub, never a process launch."""
        self.assertEqual(kwargs, {"capture_output": True, "text": True})
        self.calls.append(list(args))
        verb = args[1]
        code, value = 0, {}
        if verb == "ref":
            if self.on_runtime_ref:
                self.on_runtime_ref()
            value = {"id": "mock-runtime", "sha256": "1" * 64}
        elif verb == "example-plan":
            directory = Path(args[6])
            if directory.name == self.fail_plan_id:
                raise OSError("mock binary unavailable on second trial")
            # Match example-plan's source read: the reference is derived from
            # the current bytes, not from the expected hash in the input spec.
            plan = {"id": "example", "slots": [
                {"slot_id": "candidate", "method": receive.method_ref(args[8])},
                {"slot_id": "control", "method": {"id": "default-control"}},
            ]}
            (directory / "plan.json").write_text(json.dumps(plan))
            (directory / "criterion.json").write_text("{}")
        elif verb == "run":
            plan = json.loads(Path(args[2]).read_text())
            data = Path(args[4]).read_bytes()
            self.executed.append({"trial": Path(args[2]).parent.name,
                                  "slot": args[3], "bytes": data, "plan": plan})
            code = self.run_exit
            if code == 0:
                state = Path(args[5])
                state.mkdir()
                (state / "receipt.json").write_text("{}")
                value = {"outcome": "output_generated", "usage": {}}
        elif verb == "score":
            Path(args[5]).mkdir()
            value = {"result": "scored"}
        elif verb == "verdict":
            value = {"verdict": "insufficient"}
        elif verb == "feedback":
            Path(args[6]).write_text("{}")
            value = {"sha256": "2" * 64}
        else:
            self.fail("Unexpected mocked command: " + repr(args))
        return subprocess.CompletedProcess(args, code, json.dumps(value), "")

    def invoke(self):
        with patch.dict(os.environ, {"LAB_EXEC": "mock-exec", "LAB_SCORE": "mock-score"}), \
             patch.object(sys, "argv", ["receive.py", str(self.spec_path), str(self.out)]), \
             patch.object(receive.subprocess, "run", side_effect=self.fake_subprocess), \
             contextlib.redirect_stdout(io.StringIO()):
            receive.main()

    def assert_original_bytes_only(self):
        for row in self.executed:
            self.assertEqual(
                digest(row["bytes"]), self.spec["packages"][row["slot"]]["sha256"],
                "method execution must use the package whose hash was predeclared",
            )

    def test_unchanged_packages_run_once_and_existing_output_refuses(self):
        self.invoke()
        self.assert_original_bytes_only()
        self.assertEqual([r["slot"] for r in self.executed], ["candidate", "control"])
        summary = json.loads((self.out / "summary.json").read_text())
        self.assertEqual(summary["spec_sha256"], digest((self.out / "spec.json").read_bytes()))
        calls = len(self.calls)
        with self.assertRaises(SystemExit):
            self.invoke()
        self.assertEqual(len(self.calls), calls)

    def test_nonzero_run_is_retained_without_retry(self):
        self.run_exit = 7
        self.invoke()
        summary = json.loads((self.out / "summary.json").read_text())
        self.assertEqual([r["slot"] for r in self.executed], ["candidate", "control"])
        self.assertEqual([row["run_exit"] for row in summary["trials"][0]["slots"].values()], [7, 7])

    def test_candidate_change_after_preflight_never_executes_undeclared_bytes(self):
        self.on_runtime_ref = lambda: (self.root / "candidate.json").write_bytes(package_bytes("substituted"))
        try:
            self.invoke()
        except SystemExit:
            pass  # Refusal is safe; running the original frozen copy is safe too.
        self.assert_original_bytes_only()

    def test_control_change_after_preflight_never_executes_undeclared_bytes(self):
        self.on_runtime_ref = lambda: (self.root / "control.json").write_bytes(package_bytes("substituted"))
        try:
            self.invoke()
        except SystemExit:
            pass
        self.assert_original_bytes_only()

    def test_retained_spec_hash_describes_the_spec_actually_run(self):
        load = receive.load_spec

        def change_after_parse(path, base):
            loaded = load(path, base)
            changed = json.loads(self.spec_path.read_text())
            changed["org_id"] = "org_changed_after_parse"
            self.spec_path.write_text(json.dumps(changed))
            return loaded

        with patch.object(receive, "load_spec", side_effect=change_after_parse):
            try:
                self.invoke()
            except SystemExit:
                pass
        retained = json.loads((self.out / "spec.json").read_text()) if self.out.exists() else None
        for row in self.executed:
            self.assertTrue(all(slot["org_id"] == retained["org_id"] for slot in row["plan"]["slots"]),
                            "published spec bytes must describe the organization actually run")

    def test_later_launch_exception_retains_completed_trial_and_failure_history(self):
        self.spec["trials"].append({**self.spec["trials"][0], "id": "second"})
        self.save_spec()
        self.fail_plan_id = "second"
        try:
            self.invoke()
        except (OSError, SystemExit):
            pass
        self.assertEqual([r["trial"] for r in self.executed], ["first", "first"])
        summary = self.out / "summary.json"
        self.assertTrue(summary.exists(), "a later launch failure must not lose the entire trial summary")
        trials = json.loads(summary.read_text())["trials"]
        self.assertEqual([t["trial"] for t in trials], ["first", "second"])
        self.assertIn("error", trials[1])


if __name__ == "__main__":
    unittest.main()
