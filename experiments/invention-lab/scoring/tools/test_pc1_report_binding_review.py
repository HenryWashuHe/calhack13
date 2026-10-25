"""Independent report-binding guards; slot assessments are mocked as eligible.

Only Python JSON preparation/aggregation runs. No subprocess, scorer, evaluator,
namespace, provider, fixture binary or generated method executes.
"""
import copy
import hashlib
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

TOOLS = Path(__file__).resolve().parent
ROOT = TOOLS.parents[3]
spec = importlib.util.spec_from_file_location("pc1_report_binding_review", TOOLS / "pc1_report.py")
pc1 = importlib.util.module_from_spec(spec)
spec.loader.exec_module(pc1)
TEMPLATE = json.loads((ROOT / "docs/invention-lab/evidence/"
                       "live-method-20261024-b-revision-receiver/vs-random-search/plan.json").read_bytes())


def write(path, value):
    data = json.dumps(value, sort_keys=True, separators=(",", ":")).encode()
    path.write_bytes(data)
    return hashlib.sha256(data).hexdigest()


def retained(out, missing=()):
    """The author fixture's shape, with retained inert package metadata too."""
    packages = {}
    (out / "packages").mkdir()
    names = ["parent", "random-search"] + [f"{a}-{f}" for f in pc1.FAMILIES for a in "wf"]
    for name in names:
        if name in missing:
            continue
        path = out / "packages" / f"{name}.package.json"
        digest = write(path, {"package": {"id": name, "version": "0.1.0"}})
        packages[name] = {"path": str(path), "sha256": digest}
    frozen = {"schema": "lab-receive-spec/1", "org_id": "org_example_b",
              "sandbox": "namespaces", "instances": 12, "simulation_steps": 72000,
              "wall_time_ms": 60000, "packages": packages, "trials": []}
    not_run = []
    for family in pc1.FAMILIES:
        for name in pc1.PLANS:
            tid = f"fresh-{family}-{name}"
            candidate, control = pc1.sides(family, name)
            absent = [key for key in (candidate, control) if key not in packages]
            if absent:
                not_run.append({"trial": tid, "family": family, "status": "not_run",
                                "reason": "no_candidate: " + ", ".join(absent)})
                continue
            frozen["trials"].append({"id": tid, "family": family, "candidate": candidate,
                                     "control": control, "first_seed": 1000})
            plan = copy.deepcopy(TEMPLATE)
            plan["id"] = f"receive-{tid}"
            first = plan["development_instances"][0]
            plan["development_instances"] = [dict(first, id=f"{family}/1/{1000+i}")
                                               for i in range(12)]
            for key in ("public_task", "generator", "scoring_rule"):
                plan["context"][key]["id"] = family
            plan["scoring_grant"]["simulation_steps"] = 28800
            for slot, role, package in zip(plan["slots"], ("candidate", "control"),
                                            (candidate, control)):
                slot.update(org_id=frozen["org_id"], run_id=f"run-{plan['id']}-{role}",
                            attempt_id=f"att_{plan['id']}_{role}",
                            scoring_request_id=f"score-{plan['id']}-{role}")
                slot["method"] = {"domain": "genesis", "schema": "development-method-source/1",
                                   "id": package, "version": "0.1.0",
                                   "sha256": packages[package]["sha256"]}
            directory = out / tid
            directory.mkdir()
            write(directory / "plan.json", plan)
    return not_run, write(out / "spec.json", frozen)


def assess_mock(_directory, plan, slot, _lab_score=None):
    # Isolate aggregation/binding: eligibility itself has its independent Rust
    # tests. No claim that this fixture was executed or scored.
    count = 6 if slot == "candidate" else 4
    return {"eligible": True, "reasons": [], "outcome": "output_generated",
            "observed_successes": count, "of": len(plan["development_instances"]),
            "assessment": {"fixture": "mocked eligibility"}}


class ReportBindingGuards(unittest.TestCase):
    def setUp(self):
        self.assessment = patch.object(pc1, "slot_record", side_effect=assess_mock)
        self.assessment.start()
        self.no_process = patch.object(pc1.subprocess, "run", side_effect=AssertionError("no processes"))
        self.no_process.start()
        self.addCleanup(self.assessment.stop)
        self.addCleanup(self.no_process.stop)

    def test_positive_binding_control(self):
        with tempfile.TemporaryDirectory() as tmp:
            out = Path(tmp)
            not_run, digest = retained(out)
            self.assertEqual(pc1.report(str(out), not_run, digest)["reading"], "positive_exploratory")

    def test_real_family_criterion_and_scoring_ceiling_do_not_make_policy_unbound(self):
        for change in ("criterion", "scoring_grant"):
            with self.subTest(change=change), tempfile.TemporaryDirectory() as tmp:
                out = Path(tmp)
                not_run, digest = retained(out)
                for path in out.glob("fresh-*/plan.json"):
                    plan = json.loads(path.read_bytes())
                    if change == "criterion":
                        # executor::example::criterion_bytes includes the panel IDs.
                        criterion = {"rule": "success-count-margin", "min_margin": 1,
                                     "instances": [row["id"] for row in plan["development_instances"]]}
                        plan["criterion"]["sha256"] = write(path.parent / "criterion.json", criterion)
                    elif "projectile-target" in path.parent.name:
                        # family::max_steps(): projectile 1200, other families 2400.
                        plan["scoring_grant"]["simulation_steps"] = 12 * 1200
                    write(path, plan)
                result = pc1.report(str(out), not_run, digest)
                self.assertEqual(result["reading"], "positive_exploratory", result["unbound"])

    def test_full_method_identity_must_match_pinned_package_metadata(self):
        for field in ("id", "version"):
            with self.subTest(field=field), tempfile.TemporaryDirectory() as tmp:
                out = Path(tmp)
                not_run, digest = retained(out)
                path = out / "fresh-rough-transport-primary/plan.json"
                plan = json.loads(path.read_bytes())
                plan["slots"][0]["method"][field] = "foreign-identity"
                write(path, plan)
                self.assertEqual(pc1.report(str(out), not_run, digest)["reading"], "unbound")

    def test_not_run_is_exact_without_extra_or_duplicate_entries(self):
        for mode in ("extra", "duplicate"):
            with self.subTest(mode=mode), tempfile.TemporaryDirectory() as tmp:
                out = Path(tmp)
                not_run, digest = retained(out, missing=("w-return-mix",))
                if mode == "extra":
                    not_run.append({"trial": "foreign-trial", "reason": "no_candidate: foreign"})
                else:
                    not_run.append(dict(not_run[0]))
                self.assertEqual(pc1.report(str(out), not_run, digest)["reading"], "unbound")

    def test_fresh_spec_must_keep_declared_family_and_plan_order(self):
        with tempfile.TemporaryDirectory() as tmp:
            out = Path(tmp)
            not_run, _ = retained(out)
            path = out / "spec.json"
            frozen = json.loads(path.read_bytes())
            frozen["trials"].reverse()
            digest = write(path, frozen)
            self.assertEqual(pc1.report(str(out), not_run, digest)["reading"], "unbound")


if __name__ == "__main__":
    unittest.main()
