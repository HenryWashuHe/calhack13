"""Independent retained-file checks only; no lab subprocess or generated source."""
import copy
import json
from pathlib import Path
import tempfile
import unittest
import test_pc1_report as fixture

pc1 = fixture.pc1


class ReportBindings(unittest.TestCase):
    def test_normal_author_fixture_is_positive_control(self):
        with tempfile.TemporaryDirectory() as out:
            fixture.stage(out)
            self.assertEqual(pc1.report(out)["reading"], "positive_exploratory")

    def test_actual_receipt_cannot_be_assessed_against_a_different_expected_method(self):
        root = Path(__file__).resolve().parents[4]
        trial = root / "docs/invention-lab/evidence/live-method-20261024-b-revision-receiver/vs-random-search"
        plan = json.loads((trial / "plan.json").read_text())
        self.assertTrue(pc1.slot_record(str(trial), plan, "candidate")["eligible"])
        other = copy.deepcopy(plan)
        other["slots"][0]["method"]["sha256"] = "f" * 64
        result = pc1.slot_record(str(trial), other, "candidate")
        self.assertFalse(result["eligible"], "receipt belongs to another expected method/plan")

    def test_measured_tag_without_numeric_usage_cannot_support_positive_reading(self):
        for part in ("receipt", "evaluation"):
            with self.subTest(part=part), tempfile.TemporaryDirectory() as out:
                fixture.stage(out)
                d = Path(out) / "fresh-rough-transport-primary"
                path = d / ("candidate/receipt.json" if part == "receipt"
                            else "candidate-score/evaluation.json")
                record = json.loads(path.read_text())
                record["usage"]["model_calls"] = {"kind": "measured"}
                path.write_text(json.dumps(record))
                self.assertNotEqual(pc1.report(out)["reading"], "positive_exploratory",
                                    "missing measurement is not a measured zero")

    def test_development_seed_panels_are_not_the_frozen_fresh_panel(self):
        with tempfile.TemporaryDirectory() as out:
            fixture.stage(out)
            for plan_path in Path(out).glob("*/plan.json"):
                plan = json.loads(plan_path.read_text())
                for instance in plan["development_instances"]:
                    parts = instance["id"].split("/")
                    parts[-1] = str(int(parts[-1]) - 1000)
                    instance["id"] = "/".join(parts)
                plan_path.write_text(json.dumps(plan))
                for slot in ("candidate", "control"):
                    path = plan_path.parent / f"{slot}-score/evaluation.json"
                    evaluation = json.loads(path.read_text())
                    artifact = path.parent / "artifacts" / evaluation["outcome"]["metrics"]["sha256"]
                    metrics = json.loads(artifact.read_text())
                    for row, instance in zip(metrics["instances"], plan["development_instances"]):
                        row["instance"] = instance["id"]
                    data = json.dumps(metrics).encode()
                    digest = fixture.hashlib.sha256(data).hexdigest()
                    (artifact.parent / digest).write_bytes(data)
                    evaluation["outcome"]["metrics"]["sha256"] = digest
                    path.write_text(json.dumps(evaluation))
            self.assertNotEqual(pc1.report(out)["reading"], "positive_exploratory",
                                "a fresh filename does not bind seeds1000..1011")


if __name__ == "__main__":
    unittest.main()
