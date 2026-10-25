"""Independent frozen-policy regressions; all eligibility results are mocks.

No scorer, fixture binary, evaluator, method, provider or subprocess executes.
The original binding guards remain unchanged; this fixture additionally writes
actual per-family criteria and the protocol's exact scoring ceilings.
"""
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

TOOLS = Path(__file__).resolve().parent
module_spec = importlib.util.spec_from_file_location(
    "pc1_binding_fixture", TOOLS / "test_pc1_report_binding_review.py")
fixture = importlib.util.module_from_spec(module_spec)
module_spec.loader.exec_module(fixture)
pc1 = fixture.pc1


def frozen_policy(out):
    not_run, digest = fixture.retained(out)
    for path in out.glob("fresh-*/plan.json"):
        plan = json.loads(path.read_bytes())
        criterion = {"rule": "success-count-margin", "min_margin": 1,
                     "instances": [r["id"] for r in plan["development_instances"]]}
        plan["criterion"]["sha256"] = fixture.write(path.parent / "criterion.json", criterion)
        plan["scoring_grant"] = {"model_calls": 0, "input_tokens": 0, "output_tokens": 0,
                                 "wall_time_ms": 600000,
                                 "simulation_steps": 14400 if "projectile-target" in path.parent.name else 28800}
        fixture.write(path, plan)
    return not_run, digest


class FrozenPolicyGuards(unittest.TestCase):
    def setUp(self):
        assessment = patch.object(pc1, "slot_record", side_effect=fixture.assess_mock)
        no_process = patch.object(pc1.subprocess, "run", side_effect=AssertionError("no processes"))
        assessment.start()
        no_process.start()
        self.addCleanup(assessment.stop)
        self.addCleanup(no_process.stop)

    def test_actual_family_policy_remains_a_positive_binding_control(self):
        with tempfile.TemporaryDirectory() as tmp:
            out = Path(tmp)
            not_run, digest = frozen_policy(out)
            result = pc1.report(str(out), not_run, digest)
            self.assertEqual(result["reading"], "positive_exploratory", result["unbound"])

    def test_coherently_changed_ceilings_are_not_the_frozen_protocol(self):
        for change in ("projectile_steps", "ravine_steps", "scoring_wall", "scoring_models",
                       "development_steps", "development_wall"):
            with self.subTest(change=change), tempfile.TemporaryDirectory() as tmp:
                out = Path(tmp)
                not_run, digest = frozen_policy(out)
                if change.startswith("development_"):
                    spec = json.loads((out / "spec.json").read_bytes())
                    field = "simulation_steps" if change == "development_steps" else "wall_time_ms"
                    spec[field] += 1
                    digest = fixture.write(out / "spec.json", spec)
                for path in out.glob("fresh-*/plan.json"):
                    plan = json.loads(path.read_bytes())
                    if change == "projectile_steps" and "projectile-target" in path.parent.name:
                        plan["scoring_grant"]["simulation_steps"] = 99999
                    elif change == "ravine_steps" and "ravine-crossing" in path.parent.name:
                        plan["scoring_grant"]["simulation_steps"] = 99999
                    elif change == "scoring_wall":
                        plan["scoring_grant"]["wall_time_ms"] += 1
                    elif change == "scoring_models":
                        plan["scoring_grant"]["model_calls"] = 1
                    elif change.startswith("development_"):
                        plan["development_grant"][field] = spec[field]
                    fixture.write(path, plan)
                result = pc1.report(str(out), not_run, digest)
                self.assertEqual(result["reading"], "unbound", result["unbound"])

    def test_missing_criterion_does_not_disable_criterion_binding(self):
        for change_digest in (False, True):
            with self.subTest(change_digest=change_digest), tempfile.TemporaryDirectory() as tmp:
                out = Path(tmp)
                not_run, digest = frozen_policy(out)
                for path in out.glob("fresh-projectile-target-*/plan.json"):
                    (path.parent / "criterion.json").unlink()
                    if change_digest:
                        plan = json.loads(path.read_bytes())
                        plan["criterion"]["sha256"] = "f" * 64
                        fixture.write(path, plan)
                result = pc1.report(str(out), not_run, digest)
                self.assertEqual(result["reading"], "unbound", result["unbound"])


if __name__ == "__main__":
    unittest.main()
