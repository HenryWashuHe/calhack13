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


def generation_plan():
    # MethodSynthesisPlan/PublicDocument/TextModelSettings wire shape.
    reference = {"domain": "fixture", "id": "fixture", "schema": "fixture/1",
                 "version": "1", "sha256": hashlib.sha256(b"fixture").hexdigest()}
    document = {"reference": reference, "text": "fixture"}
    return {"operation_id": "generate", "org_id": "org", "target_id": "method",
            "target_version": "1", "runtime": reference, "protocol": document,
            "entrypoint": "method.py", "dependencies": [], "public_task": document,
            "public_inputs": [], "instruction": "Never executed synthetic generation request",
            "settings": {"model": "fixture", "max_tokens": 10,
                         "timeout_ms": 1000, "sampling": "provider_default"}}


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

    def test_slot_path_traversal_is_refused_before_commands(self):
        self.fixture()
        path = self.root / "trial/plan.json"
        plan = json.loads(path.read_text())
        sid = "x/../../escaped"
        plan["slots"][0]["slot_id"] = sid
        write(path, plan)
        write(self.root / "trial" / sid / "receipt.json", self.receipt)
        write(self.root / "trial" / (sid + "-score") / "evaluation.json", self.score)
        self.assertEqual(self.run_audit(), 1)
        self.assertEqual(self.calls, [])
        self.assertFalse((self.work.parent / "escaped/receipt.json").exists())

    def test_retained_evaluation_without_receipt_is_not_silently_omitted(self):
        self.fixture()
        path = self.root / "trial/plan.json"
        plan = json.loads(path.read_text())
        plan["slots"].append({"slot_id": "orphan", "method": plan["slots"][0]["method"]})
        write(path, plan)
        write(self.root / "trial/orphan-score/evaluation.json", self.score)
        self.assertEqual(self.run_audit(), 1)
        rows = json.loads((self.work / "audit.json").read_text())
        self.assertTrue(any(r.get("slot") == "orphan" and r.get("exec") != "same" for r in rows))

    def test_duplicate_slot_ids_are_refused_before_commands(self):
        self.fixture()
        path = self.root / "trial/plan.json"
        plan = json.loads(path.read_text())
        plan["slots"].append(plan["slots"][0].copy())
        write(path, plan)
        self.assertEqual(self.run_audit(), 1)
        self.assertEqual(self.calls, [])

    def test_invocation_evaluation_without_receipt_is_not_silently_omitted(self):
        self.fixture()
        write(self.root / "invocation/plan.json", json.loads((self.root / "trial/plan.json").read_text()))
        write(self.root / "invocation/score/evaluation.json", self.score)
        self.assertEqual(self.run_audit(), 1)
        rows = json.loads((self.work / "audit.json").read_text())
        self.assertTrue(any(r.get("trial") == "invocation" and r.get("exec") != "same" for r in rows))

    def test_slot_suffixes_cannot_alias_another_slots_scratch_output(self):
        self.fixture()
        path = self.root / "trial/plan.json"
        plan = json.loads(path.read_text())
        plan["slots"].append({"slot_id": "candidate-score", "method": plan["slots"][0]["method"]})
        write(path, plan)
        write(self.root / "trial/candidate-score/receipt.json", self.receipt)
        write(self.root / "trial/candidate-score-score/evaluation.json", self.score)
        self.assertEqual(self.run_audit(), 0)
        destinations = [Path(args[5]).resolve() for args in self.calls]
        self.assertEqual(len(set(destinations)), 4)
        self.assertTrue(all(p.is_relative_to(self.work.resolve()) for p in destinations))

    def test_generation_request_is_reported_as_excluded_beside_valid_trial(self):
        self.fixture()
        write(self.root / "generation/plan.json", generation_plan())
        self.assertEqual(self.run_audit(), 0)
        rows = json.loads((self.work / "audit.json").read_text())
        self.assertTrue(any(r.get("excluded") == "method_synthesis_request" for r in rows))
        self.assertEqual(len(self.calls), 2)

    def test_generation_request_alone_is_not_an_audited_trial(self):
        write(self.root / "generation/plan.json", generation_plan())
        self.assertEqual(self.run_audit(), 1)
        rows = json.loads((self.work / "audit.json").read_text())
        self.assertEqual(rows[0].get("excluded"), "method_synthesis_request")
        self.assertEqual(self.calls, [])

    def test_slots_key_has_trial_validation_precedence(self):
        self.fixture()
        request = generation_plan(); request["slots"] = []
        write(self.root / "other/plan.json", request)
        self.assertEqual(self.run_audit(), 1)
        rows = json.loads((self.work / "audit.json").read_text())
        self.assertFalse(any(r.get("excluded") for r in rows))

    def test_partial_generation_shape_is_still_a_malformed_plan(self):
        self.fixture()
        request = generation_plan(); request["protocol"] = {"text": "fixture"}
        write(self.root / "other/plan.json", request)
        self.assertEqual(self.run_audit(), 1)
        rows = json.loads((self.work / "audit.json").read_text())
        self.assertFalse(any(r.get("excluded") for r in rows))


if __name__ == "__main__":
    unittest.main()
