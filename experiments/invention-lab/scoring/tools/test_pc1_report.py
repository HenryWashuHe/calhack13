"""pc1_report.py computes the frozen process-comparison/1 endpoints from
synthetic retained records, written before any real data exists.

Run with: python3 -m unittest discover -s experiments/invention-lab/scoring/tools
Nothing is executed; every record here is synthetic.
"""
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import tempfile
import unittest

MODULE = importlib.util.spec_from_file_location(
    "pc1_report_under_test", Path(__file__).with_name("pc1_report.py"))
pc1 = importlib.util.module_from_spec(MODULE)
MODULE.loader.exec_module(pc1)

N = 12
GRANT = {"model_calls": 0, "input_tokens": 0, "output_tokens": 0,
         "wall_time_ms": 600000, "simulation_steps": 28800}


def measured(**over):
    usage = {k: {"kind": "measured", "value": 0} for k in pc1.USAGE}
    for k, v in over.items():
        usage[k] = v
    return usage


def write(path, value, raw=None):
    os.makedirs(os.path.dirname(path), exist_ok=True)
    with open(path, "wb") as f:
        f.write(raw if raw is not None else json.dumps(value).encode())


def slot(d, family, name, successes, outcome="output_generated", usage=None,
         scoring_kind="scored", scoring_usage=None, tamper=False):
    write(os.path.join(d, name, "receipt.json"),
          {"outcome": outcome, "usage": usage or measured()})
    rows = [{"instance": f"{family}/1/{1000 + i}", "success": i < successes, "progress": 1.0}
            for i in range(N)]
    data = json.dumps({"instances": rows}).encode()
    digest = hashlib.sha256(data).hexdigest()
    write(os.path.join(d, name + "-score", "artifacts", digest), None,
          raw=data + (b" " if tamper else b""))
    write(os.path.join(d, name + "-score", "evaluation.json"),
          {"outcome": {"kind": scoring_kind, "metrics": {"sha256": digest}},
           "usage": scoring_usage or measured(simulation_steps={"kind": "measured", "value": 100})})


def trial(out, family, plan, cand, ctrl, **slot_overrides):
    d = os.path.join(out, f"fresh-{family}-{plan}")
    write(os.path.join(d, "plan.json"), {
        "scoring_grant": GRANT,
        "development_instances": [{"id": f"{family}/1/{1000 + i}"} for i in range(N)]})
    write(os.path.join(d, "verdict.json"), {"verdict": "synthetic"})
    slot(d, family, "candidate", cand, **slot_overrides.get("candidate", {}))
    slot(d, family, "control", ctrl, **slot_overrides.get("control", {}))


def stage(out, w=(6, 6, 6, 6), f=(5, 5, 5, 5), ref=(4, 4, 4, 4), rep=(11, 11, 11, 11),
          skip=(), overrides=None):
    overrides = overrides or {}
    for i, fam in enumerate(pc1.FAMILIES):
        for plan, cand, ctrl in (("primary", w[i], f[i]), ("w-random", w[i], rep[i]),
                                 ("f-random", f[i], rep[i]), ("parent-random", 3, ref[i])):
            if (fam, plan) not in skip:
                trial(out, fam, plan, cand, ctrl, **overrides.get((fam, plan), {}))


class Endpoints(unittest.TestCase):
    def run_report(self, not_run=(), **kw):
        with tempfile.TemporaryDirectory() as out:
            stage(out, **kw)
            return pc1.report(out, list(not_run))

    def test_all_five_conditions_give_the_positive_exploratory_reading(self):
        r = self.run_report()
        self.assertEqual((r["primary"]["w_sum"], r["primary"]["f_sum"]), (24, 20))
        self.assertEqual(r["reference"]["sum"], 16)
        self.assertEqual(r["reading"], "positive_exploratory")
        self.assertTrue(all(r["positive_reading"].values()))

    def test_a_margin_of_one_is_not_positive(self):
        r = self.run_report(w=(6, 6, 6, 6), f=(6, 6, 6, 5))
        self.assertEqual(r["primary"]["difference"], 1)
        self.assertFalse(r["positive_reading"]["4_w_exceeds_f_by_2"])
        self.assertEqual(r["reading"], "not_positive")

    def test_w_below_the_reference_is_not_positive(self):
        r = self.run_report(ref=(7, 7, 7, 7))
        self.assertTrue(r["positive_reading"]["4_w_exceeds_f_by_2"])
        self.assertFalse(r["positive_reading"]["5_w_at_least_reference"])
        self.assertEqual(r["reading"], "not_positive")

    def test_the_reference_is_only_the_parent_plans_control_slots(self):
        # Random repetitions in plans (2)/(3) score 11 and must never replace it.
        r = self.run_report(ref=(1, 1, 1, 1), rep=(11, 11, 11, 11))
        self.assertEqual(r["reference"]["sum"], 4)

    def test_a_primary_not_run_makes_the_comparison_incomplete(self):
        nr = [{"trial": "fresh-return-mix-primary", "status": "not_run",
               "reason": "no_candidate: f-return-mix"}]
        r = self.run_report(not_run=nr, skip={("return-mix", "primary")})
        self.assertFalse(r["primary"]["complete"])
        self.assertIsNone(r["primary"]["difference"])
        self.assertEqual(r["families"]["return-mix"]["primary"]["status"], "not_run")
        self.assertEqual(r["reading"], "incomplete")

    def test_a_scored_failed_execution_counts_zero_and_blocks_the_reading(self):
        bad = {("rough-transport", "primary"): {"candidate": {"outcome": "failed"}}}
        r = self.run_report(w=(12, 12, 12, 12), overrides=bad)
        cand = r["families"]["rough-transport"]["primary"]["candidate"]
        self.assertFalse(cand["eligible"])
        self.assertEqual(cand["observed_successes"], 12, "still reported as observed")
        self.assertEqual(r["primary"]["w_sum"], 36)
        self.assertFalse(r["positive_reading"]["2_every_primary_slot_eligible"])
        self.assertEqual(r["reading"], "incomplete")

    def test_unknown_usage_in_a_reference_slot_leaves_no_reference(self):
        unknown = measured(model_calls={"kind": "unknown", "reason": "x"})
        bad = {("projectile-target", "parent-random"): {"control": {"usage": unknown}}}
        r = self.run_report(overrides=bad)
        self.assertIsNone(r["reference"]["sum"], "never read as a zero baseline")
        self.assertEqual(r["reading"], "incomplete")

    def test_tampered_metrics_and_over_grant_scoring_are_ineligible(self):
        over = measured(simulation_steps={"kind": "measured", "value": 28801})
        bad = {("ravine-crossing", "primary"): {"candidate": {"tamper": True},
                                                "control": {"scoring_usage": over}}}
        r = self.run_report(overrides=bad)
        t = r["families"]["ravine-crossing"]["primary"]
        self.assertIn("metrics artifact missing or not its digest", t["candidate"]["reasons"])
        self.assertIn("scoring simulation_steps over its grant", t["control"]["reasons"])
        self.assertEqual(r["reading"], "incomplete")

    def test_a_valid_zero_is_eligible(self):
        r = self.run_report(f=(0, 0, 0, 0), ref=(0, 0, 0, 0))
        self.assertTrue(r["families"]["ravine-crossing"]["primary"]["control"]["eligible"])
        self.assertEqual(r["primary"]["f_sum"], 0)
        self.assertEqual(r["reading"], "positive_exploratory")

    def test_missing_records_and_unknown_outcomes_are_named_not_guessed(self):
        unknown = measured(wall_time_ms={"kind": "unknown", "reason": "x"})
        bad = {("return-mix", "primary"): {"candidate": {"scoring_kind": "unknown"},
                                           "control": {"scoring_usage": unknown}}}
        with tempfile.TemporaryDirectory() as out:
            stage(out, overrides=bad, skip={("projectile-target", "w-random")})
            t = os.path.join(out, "fresh-ravine-crossing-primary")
            os.remove(os.path.join(t, "candidate", "receipt.json"))
            os.remove(os.path.join(t, "control-score", "evaluation.json"))
            r = pc1.report(out)
        rc = r["families"]["ravine-crossing"]["primary"]
        self.assertIn("no execution receipt", rc["candidate"]["reasons"])
        self.assertIn("no scoring receipt", rc["control"]["reasons"])
        rm = r["families"]["return-mix"]["primary"]
        self.assertIn("scoring unknown", rm["candidate"]["reasons"])
        self.assertIn("scoring usage wall_time_ms not measured", rm["control"]["reasons"])
        self.assertEqual(r["families"]["projectile-target"]["w-random"], {"trial": "fresh-projectile-target-w-random", "status": "missing"})
        self.assertEqual(r["reading"], "incomplete")


class RealRecords(unittest.TestCase):
    def test_a_retained_trial_reads_as_its_recorded_verdict(self):
        # #116's primary trial: child 3/6 vs seeded random search 4/6, all usage known.
        root = Path(__file__).resolve().parents[4]
        d = root / "docs/invention-lab/evidence/live-method-20261024-b-revision-receiver/vs-random-search"
        plan = json.loads((d / "plan.json").read_text())
        cand, ctrl = (pc1.slot_record(str(d), plan, s) for s in ("candidate", "control"))
        self.assertEqual((cand["eligible"], cand["observed_successes"], cand["of"]), (True, 3, 6))
        self.assertEqual((ctrl["eligible"], ctrl["observed_successes"]), (True, 4))


class Cli(unittest.TestCase):
    def test_the_command_reads_the_not_run_list_and_refuses_bad_input(self):
        import contextlib, io
        with tempfile.TemporaryDirectory() as out:
            stage(out, skip={("return-mix", "primary")})
            nr = os.path.join(out, "not-run.json")
            write(nr, [{"trial": "fresh-return-mix-primary", "reason": "no_candidate: w-return-mix"}])
            buf = io.StringIO()
            with contextlib.redirect_stdout(buf):
                pc1.main(["pc1_report.py", out, nr])
            self.assertEqual(json.loads(buf.getvalue())["families"]["return-mix"]["primary"]["status"], "not_run")
            write(nr, {"not": "a list"})
            for argv in (["pc1_report.py"], ["pc1_report.py", out, nr], ["pc1_report.py", out, nr, "x"]):
                with self.assertRaises(SystemExit):
                    pc1.main(argv)


if __name__ == "__main__":
    unittest.main()
