"""Only synthetic JSON and mocked subprocesses; never launches any executable."""
import contextlib
import hashlib
import importlib.util
import io
import json
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location("audit_under_review", Path(__file__).with_name("reexec_audit.py"))
audit = importlib.util.module_from_spec(spec)
spec.loader.exec_module(audit)


def write(path, value):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(value))


class AuditGuards(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        base = Path(self.temp.name)
        self.root, self.work = base / "evidence", base / "work"
        self.root.mkdir()
        self.receipt = {"usage": {"wall_time_ms": {"measured": 1}}, "outcome": "fixture", "outputs": []}
        self.score = {"outcome": "fixture-score"}
        self.calls = []
        self.failure = None
        self.mock = patch.object(audit.subprocess, "run", self.command)
        self.mock.start()
        self.addCleanup(self.mock.stop)

    def fixture(self):
        package = b'{"source":"inert fixture, never executed"}'
        (self.root / "package.json").write_bytes(package)
        write(self.root / "trial/plan.json", {"development_environment": {"sha256": "a" * 64},
              "slots": [{"slot_id": "candidate", "method": {"sha256": hashlib.sha256(package).hexdigest()}}]})
        write(self.root / "trial/candidate/receipt.json", self.receipt)
        write(self.root / "trial/candidate-score/evaluation.json", self.score)

    def command(self, args, **_kwargs):
        self.calls.append(args)
        # Write plausible output even when the mocked process fails. Its status
        # must be checked before an existing/output file can establish success.
        if args[1] == "run":
            write(Path(args[5]) / "receipt.json", self.receipt)
        else:
            write(Path(args[5]) / "evaluation.json", self.score)
        return subprocess.CompletedProcess(args, 17 if args[1] == self.failure else 0,
                                           "", "synthetic command result")

    def run_audit(self):
        with contextlib.redirect_stdout(io.StringIO()), contextlib.redirect_stderr(io.StringIO()):
            return audit.main([str(self.root), str(self.work), "never-run-score", "aaaaaaaa=never-run-exec"])

    def test_matching_fresh_results_control(self):
        self.fixture()
        self.assertEqual(self.run_audit(), 0)
        self.assertEqual(len(self.calls), 2)

    def test_nonzero_executor_cannot_be_rescored(self):
        self.fixture(); self.failure = "run"
        self.assertEqual(self.run_audit(), 1)
        self.assertEqual(len(self.calls), 1)

    def test_nonzero_scorer_cannot_claim_reproduction(self):
        self.fixture(); self.failure = "score"
        self.assertEqual(self.run_audit(), 1)
        self.assertEqual(len(self.calls), 2)

    def test_existing_scratch_is_refused_without_commands(self):
        self.fixture()
        write(self.work / "000-candidate/receipt.json", self.receipt)
        write(self.work / "000-candidate-score/evaluation.json", self.score)
        self.assertEqual(self.run_audit(), 1)
        self.assertEqual(self.calls, [])

    def test_malformed_plan_cannot_disappear_beside_valid_trial(self):
        self.fixture()
        path = self.root / "other/plan.json"; path.parent.mkdir(); path.write_text("{invalid")
        self.assertEqual(self.run_audit(), 1)
        self.assertTrue(any(r.get("exec", "").startswith("invalid retained plan")
                            for r in json.loads((self.work / "audit.json").read_text())))

    def test_zero_audited_slots_is_not_success(self):
        self.assertEqual(self.run_audit(), 1)
        self.assertEqual(self.calls, [])

    def test_work_inside_evidence_is_refused(self):
        self.fixture(); self.work = self.root / "scratch"
        self.assertEqual(self.run_audit(), 1)
        self.assertFalse(self.work.exists())
        self.assertEqual(self.calls, [])


if __name__ == "__main__":
    unittest.main()
