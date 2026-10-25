"""pc1_report.py computes the frozen process-comparison/1 endpoints from
synthetic retained records, written before any real data exists.

Run with: python3 -m unittest discover -s experiments/invention-lab/scoring/tools
Nothing executes a method or scorer. Records are inert and synthetic: the
spec comes from pc1_specs.fresh_spec over placeholder packages, and the typed
receipts are #116's retained ones rebound to each synthetic plan by the
test-only `pc1_fixture` example. The real `lab-score eligibility` reads them.
"""
import copy
import hashlib
import importlib.util
import json
import os
import shutil
from pathlib import Path
import subprocess
import tempfile
import unittest

TOOLS = Path(__file__).resolve().parent
SCORING = TOOLS.parent
ROOT = SCORING.parents[2]
TEMPLATE = ROOT / ("docs/invention-lab/evidence/live-method-20261024-b-revision-receiver/"
                   "vs-random-search")


def module(name, file):
    spec = importlib.util.spec_from_file_location(name, TOOLS / file)
    m = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(m)
    return m


pc1 = module("pc1_report_under_test", "pc1_report.py")
specs = module("pc1_specs_for_report", "pc1_specs.py")


def build():
    """lab-score and the fixture example, via cargo; reused when already set."""
    if os.environ.get("LAB_SCORE") and os.environ.get("PC1_FIXTURE"):
        return
    done = subprocess.run(
        ["cargo", "build", "--quiet", "--message-format=json", "--manifest-path",
         str(SCORING / "Cargo.toml"), "--bin", "lab-score", "--example", "pc1_fixture"],
        capture_output=True, text=True, check=True)
    for line in done.stdout.splitlines():
        msg = json.loads(line)
        if msg.get("reason") == "compiler-artifact" and msg.get("executable"):
            name = msg["target"]["name"]
            if name == "lab-score":
                os.environ["LAB_SCORE"] = msg["executable"]
            elif name == "pc1_fixture":
                os.environ["PC1_FIXTURE"] = msg["executable"]


build()
TEMPLATE_PLAN = json.loads((TEMPLATE / "plan.json").read_text())
ORG = "org_example_b"


def sha(text):
    return hashlib.sha256(text.encode()).hexdigest()


def packages(tmp, missing):
    """Placeholder package files: only their bytes' digests enter the spec."""
    def pkg(name):
        path = os.path.join(tmp, name + ".json")
        with open(path, "w") as f:
            json.dump({"package": {"id": name, "version": "0.1.0"}}, f)
        return path
    selections = {}
    for fam in pc1.FAMILIES:
        selections[fam] = {}
        for arm in ("W", "F"):
            if (fam, arm) in missing:
                selections[fam][arm] = None
            else:
                path = pkg(f"{arm.lower()}-{fam}")
                selections[fam][arm] = {"package": path,
                                        "sha256": hashlib.sha256(Path(path).read_bytes()).hexdigest()}
    return selections, pkg("random-search"), pkg("parent")


def criterion_bytes(plan):
    ids = [i["id"] for i in plan["development_instances"]]
    return json.dumps({"instances": ids, "min_margin": 1, "rule": "success-count-margin"},
                      sort_keys=True, separators=(",", ":")).encode()


def make_plan(spec, tid, fam, cand, ctrl, mutate=None):
    p = copy.deepcopy(TEMPLATE_PLAN)
    pid = f"receive-{tid}"
    p["id"] = pid
    first = TEMPLATE_PLAN["development_instances"][0]
    p["development_instances"] = [
        {**first, "id": f"{fam}/1/{1000 + i}", "sha256": sha(f"{fam}/1/{1000 + i}")}
        for i in range(12)]
    for k in ("public_task", "generator", "scoring_rule"):
        p["context"][k] = {**p["context"][k], "id": fam}
    p["context"]["instance_set"] = {**p["context"]["instance_set"], "sha256": sha(fam)}
    p["development_grant"] = {"model_calls": 0, "input_tokens": 0, "output_tokens": 0,
                              "wall_time_ms": spec["wall_time_ms"],
                              "simulation_steps": spec["simulation_steps"]}
    # The protocol's frozen scoring grant and the executor's margin-one
    # criterion over this panel (written beside the plan by `trial`).
    p["scoring_grant"] = {"model_calls": 0, "input_tokens": 0, "output_tokens": 0,
                          "wall_time_ms": 600000,
                          "simulation_steps": 12 * (1200 if fam == "projectile-target" else 2400)}
    p["criterion"] = {**p["criterion"],
                      "sha256": hashlib.sha256(criterion_bytes(p)).hexdigest()}
    for s, name, pkg in zip(p["slots"], ("candidate", "control"), (cand, ctrl)):
        s["method"] = {"domain": "genesis", "schema": "development-method-source/1",
                       "id": pkg, "version": "0.1.0", "sha256": spec["packages"][pkg]["sha256"]}
        s.update(org_id=ORG, run_id=f"run-{pid}-{name}", attempt_id=f"att_{pid}_{name}",
                 scoring_request_id=f"score-{pid}-{name}")
    if mutate:
        mutate(p)
    return p


def slot_config(successes, outcome=None, usage=None, scoring_kind="scored",
                scoring_usage=None, tamper=False):
    config = {"successes": successes, "scoring": scoring_kind}
    if outcome:
        config["outcome"] = outcome
    if usage:
        config["unknown_development"] = usage
    if scoring_usage:
        config["scoring_usage"] = scoring_usage
    return config, tamper


def trial(out, spec, tid, fam, cand, ctrl, cs, fs, overrides=None):
    overrides = overrides or {}
    d = os.path.join(out, tid)
    os.makedirs(d)
    plan = make_plan(spec, tid, fam, cand, ctrl, overrides.get("plan"))
    with open(os.path.join(d, "plan.json"), "w") as f:
        json.dump(plan, f)
    Path(d, "criterion.json").write_bytes(criterion_bytes(plan))
    with open(os.path.join(d, "verdict.json"), "w") as f:
        json.dump({"verdict": "synthetic"}, f)
    slots = {name: slot_config(n, **overrides.get(name, {}))
             for name, n in (("candidate", cs), ("control", fs))}
    config = os.path.join(d, "fixture.json")
    with open(config, "w") as f:
        json.dump({"template": str(TEMPLATE), "out": d,
                   "slots": {k: v[0] for k, v in slots.items()}}, f)
    subprocess.run([os.environ["PC1_FIXTURE"], config], check=True)
    os.remove(config)
    for name, (_, tamper) in slots.items():
        if tamper:
            ev = json.loads(Path(d, name + "-score", "evaluation.json").read_text())
            art = Path(d, name + "-score", "artifacts", ev["outcome"]["metrics"]["sha256"])
            art.write_bytes(art.read_bytes() + b" ")


def stage(out, w=(6, 6, 6, 6), f=(5, 5, 5, 5), ref=(4, 4, 4, 4), rep=(11, 11, 11, 11),
          skip=(), overrides=None, missing=()):
    """One fresh stage; returns (not_run, posted spec SHA-256)."""
    overrides = overrides or {}
    with tempfile.TemporaryDirectory() as tmp:
        selections, random, parent = packages(tmp, missing)
        spec, not_run = specs.fresh_spec(out, selections, random, parent, ORG)
        # receive.py retains a snapshot of every package under packages/.
        os.makedirs(os.path.join(out, "packages"))
        for name, pin in spec["packages"].items():
            shutil.copyfile(os.path.join(out, pin["path"]),
                            os.path.join(out, "packages", name + ".package.json"))
    data = json.dumps(spec, indent=2, sort_keys=True).encode()
    Path(out, "spec.json").write_bytes(data)
    listed = {t["id"] for t in spec["trials"]}
    for i, fam in enumerate(pc1.FAMILIES):
        for name, cs, fs in (("primary", w[i], f[i]), ("w-random", w[i], rep[i]),
                             ("f-random", f[i], rep[i]), ("parent-random", 3, ref[i])):
            tid = f"fresh-{fam}-{name}"
            if tid in listed and (fam, name) not in skip:
                cand, ctrl = pc1.sides(fam, name)
                trial(out, spec, tid, fam, cand, ctrl, cs, fs, overrides.get((fam, name)))
    return not_run, hashlib.sha256(data).hexdigest()


class Endpoints(unittest.TestCase):
    def run_report(self, **kw):
        with tempfile.TemporaryDirectory() as out:
            not_run, digest = stage(out, **kw)
            return pc1.report(out, not_run, digest)

    def test_all_five_conditions_give_the_positive_exploratory_reading(self):
        r = self.run_report()
        self.assertEqual((r["primary"]["w_sum"], r["primary"]["f_sum"]), (24, 20))
        self.assertEqual(r["reference"]["sum"], 16)
        self.assertEqual((r["reading"], r["spec_pinned"], r["unbound"]),
                         ("positive_exploratory", True, []))
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
        # F selected nothing for return-mix: its primary and f-random are not run.
        r = self.run_report(missing={("return-mix", "F")})
        self.assertEqual(r["unbound"], [])
        self.assertFalse(r["primary"]["complete"])
        self.assertIsNone(r["primary"]["difference"])
        self.assertEqual(r["families"]["return-mix"]["primary"],
                         {"trial": "fresh-return-mix-primary", "status": "not_run",
                          "reason": "no_candidate: f-return-mix"})
        self.assertEqual(r["reading"], "incomplete")

    def test_a_scored_failed_execution_counts_zero_and_blocks_the_reading(self):
        bad = {("rough-transport", "primary"): {"candidate": {"outcome": "failed"}}}
        r = self.run_report(w=(12, 12, 12, 12), overrides=bad)
        cand = r["families"]["rough-transport"]["primary"]["candidate"]
        self.assertFalse(cand["eligible"])
        self.assertEqual(cand["observed_successes"], 12, "still reported as observed")
        self.assertIn("execution outcome Failed", cand["reasons"])
        self.assertEqual(r["primary"]["w_sum"], 36)
        self.assertFalse(r["positive_reading"]["2_every_primary_slot_eligible"])
        self.assertEqual(r["reading"], "incomplete")

    def test_unknown_usage_in_a_reference_slot_leaves_no_reference(self):
        bad = {("projectile-target", "parent-random"): {"control": {"usage": ["model_calls"]}}}
        r = self.run_report(overrides=bad)
        self.assertIsNone(r["reference"]["sum"], "never read as a zero baseline")
        self.assertEqual(r["reading"], "incomplete")

    def test_tampered_metrics_and_over_grant_scoring_are_ineligible(self):
        bad = {("ravine-crossing", "primary"): {
            "candidate": {"tamper": True},
            "control": {"scoring_usage": {"simulation_steps": 28801}}}}
        r = self.run_report(overrides=bad)
        t = r["families"]["ravine-crossing"]["primary"]
        self.assertIn("metrics bytes differ", t["candidate"]["reasons"][0])
        self.assertIn("scoring usage is missing, unknown or over grant", t["control"]["reasons"])
        self.assertEqual(r["reading"], "incomplete")

    def test_a_valid_zero_is_eligible(self):
        r = self.run_report(f=(0, 0, 0, 0), ref=(0, 0, 0, 0))
        self.assertTrue(r["families"]["ravine-crossing"]["primary"]["control"]["eligible"])
        self.assertEqual(r["primary"]["f_sum"], 0)
        self.assertEqual(r["reading"], "positive_exploratory")

    def test_missing_records_and_unknown_outcomes_are_named_not_guessed(self):
        bad = {("return-mix", "primary"): {
            "candidate": {"scoring_kind": "unknown"},
            "control": {"scoring_usage": {"wall_time_ms": None}}}}
        with tempfile.TemporaryDirectory() as out:
            not_run, digest = stage(out, overrides=bad, skip={("projectile-target", "w-random")})
            t = os.path.join(out, "fresh-ravine-crossing-primary")
            os.remove(os.path.join(t, "candidate", "receipt.json"))
            os.remove(os.path.join(t, "control-score", "evaluation.json"))
            r = pc1.report(out, not_run, digest)
        rc = r["families"]["ravine-crossing"]["primary"]
        self.assertIn("no execution receipt", rc["candidate"]["reasons"])
        self.assertIn("no scoring receipt", rc["control"]["reasons"])
        rm = r["families"]["return-mix"]["primary"]
        self.assertEqual(rm["candidate"]["assessment"]["scoring"]["kind"], "unknown")
        self.assertIn("no validated Scored success count", rm["candidate"]["reasons"])
        self.assertIn("scoring usage is missing, unknown or over grant", rm["control"]["reasons"])
        self.assertEqual(r["families"]["projectile-target"]["w-random"],
                         {"trial": "fresh-projectile-target-w-random", "status": "missing"})
        self.assertEqual(r["reading"], "incomplete")


class SpecBinding(unittest.TestCase):
    """Valid, eligible records that answer a different question are unbound."""

    def reading(self, edit=None, pin=None, not_run_edit=None, **kw):
        with tempfile.TemporaryDirectory() as out:
            not_run, digest = stage(out, **kw)
            if edit:
                edit(out)
            if not_run_edit:
                not_run = not_run_edit(not_run)
            return pc1.report(out, not_run, pin or digest)

    def test_a_spec_other_than_the_posted_one_is_unbound(self):
        r = self.reading(pin="0" * 64)
        self.assertEqual(r["reading"], "unbound")
        self.assertIn("retained spec.json is not the posted spec", r["unbound"])

    def test_records_bound_to_a_different_method_than_the_spec_names(self):
        # The primary's candidate slot ran F's package: the receipts bind to their
        # own plan and are eligible, but that plan is not the spec's trial.
        def swap(p):
            p["slots"][0]["method"]["sha256"] = p["slots"][1]["method"]["sha256"]
        r = self.reading(overrides={("rough-transport", "primary"): {"plan": swap}})
        t = r["families"]["rough-transport"]["primary"]
        self.assertTrue(t["candidate"]["eligible"])
        self.assertIn("candidate method is not spec package w-rough-transport", t["problems"])
        self.assertEqual(r["reading"], "unbound")

    def test_a_plan_with_its_own_evaluator_or_grant_is_unbound(self):
        def other(p):
            p["scoring_grant"]["simulation_steps"] = 99999
        r = self.reading(overrides={("return-mix", "f-random"): {"plan": other}})
        # Scoring steps may differ between families, never within one.
        self.assertIn("return-mix plans differ in criterion or scoring steps", r["unbound"])
        self.assertEqual(r["reading"], "unbound")

    def test_a_plan_with_its_own_evaluator_is_unbound(self):
        def other(p):
            p["context"]["evaluator"]["sha256"] = "e" * 64
        r = self.reading(overrides={("return-mix", "f-random"): {"plan": other}})
        self.assertIn("plans differ in evaluator, environment or grants", r["unbound"])
        self.assertEqual(r["reading"], "unbound")

    def test_not_run_declarations_must_match_the_spec(self):
        r = self.reading(missing={("return-mix", "F")}, not_run_edit=lambda nr: nr[:1])
        self.assertTrue(any("not declared not run" in p for p in r["unbound"]))
        stray = self.reading(missing={("return-mix", "F")},
                             edit=lambda out: os.makedirs(os.path.join(
                                 out, "fresh-return-mix-primary")))
        self.assertIn("fresh-return-mix-primary is declared not run but has records",
                      stray["unbound"])

    def test_a_spec_trial_off_the_fresh_panel_is_unbound(self):
        def edit(out):
            path = Path(out, "spec.json")
            spec = json.loads(path.read_text())
            spec["trials"][0]["first_seed"] = 0
            path.write_text(json.dumps(spec))
        with tempfile.TemporaryDirectory() as out:
            not_run, _ = stage(out)
            edit(out)
            digest = hashlib.sha256(Path(out, "spec.json").read_bytes()).hexdigest()
            r = pc1.report(out, not_run, digest)
        self.assertIn("fresh-ravine-crossing-primary is not the protocol's spec entry",
                      r["unbound"])


    def test_records_rebound_to_another_seed_panel_are_unbound(self):
        # Receipts bind to their own (0..11) plans and assess as eligible; only
        # the frozen-panel check can tell this is not the fresh panel.
        def shift(p):
            for i, instance in enumerate(p["development_instances"]):
                instance["id"] = instance["id"].rsplit("/", 1)[0] + f"/{i}"
        r = self.reading(overrides={("return-mix", "parent-random"): {"plan": shift}})
        t = r["families"]["return-mix"]["parent-random"]
        self.assertTrue(t["control"]["eligible"])
        self.assertIn("instances are not the frozen fresh panel return-mix/1/1000..1011",
                      t["problems"])
        self.assertEqual(r["reading"], "unbound")

    def test_plan_identity_and_slot_shape_are_checked_beyond_the_assessment(self):
        # Each plan is rebound consistently, so its receipts still assess as
        # eligible; only the spec binding can refuse it.
        def other_id(p):
            p["id"] = "receive-fresh-rough-transport-w-random"
        def dependency(p):
            p["slots"][1]["dependencies"] = [dict(p["slots"][1]["method"], id="dep")]
        cases = {
            ("rough-transport", "primary", other_id): "plan id is not receive-<trial>",
            ("ravine-crossing", "f-random", dependency): "control purpose or dependencies differ",
        }
        for (fam, name, change), problem in cases.items():
            with self.subTest(problem):
                r = self.reading(overrides={(fam, name): {"plan": change}})
                t = r["families"][fam][name]
                self.assertIn(problem, t["problems"])
                self.assertEqual(r["reading"], "unbound")

    def test_unreadable_plans_and_off_roster_trials_are_unbound(self):
        def garbage(out):
            Path(out, "fresh-return-mix-w-random", "plan.json").write_text("not json")
        r = self.reading(edit=garbage)
        self.assertIn("fresh-return-mix-w-random: plan.json missing or not JSON", r["unbound"])

        def extra_trial(out):
            path = Path(out, "spec.json")
            spec = json.loads(path.read_text())
            spec["trials"].append(dict(spec["trials"][0]))
            path.write_text(json.dumps(spec))
        with tempfile.TemporaryDirectory() as out:
            not_run, _ = stage(out)
            extra_trial(out)
            digest = hashlib.sha256(Path(out, "spec.json").read_bytes()).hexdigest()
            r = pc1.report(out, not_run, digest)
        self.assertTrue(any(p.startswith("spec has unexpected or repeated trials")
                            for p in r["unbound"]))
        listed = self.reading(not_run_edit=lambda nr: nr + [
            {"trial": "fresh-rough-transport-primary", "reason": "no_candidate: w-rough-transport"}])
        self.assertIn("fresh-rough-transport-primary is declared not run but the spec has "
                      "its packages", listed["unbound"])

    def test_a_missing_or_malformed_spec_is_unbound(self):
        def respec(change):
            def edit(out):
                path = Path(out, "spec.json")
                spec = json.loads(path.read_text())
                change(spec)
                path.write_text(json.dumps(spec))
            return edit
        cases = {
            "no retained lab-receive-spec/1 spec.json":
                lambda out: os.remove(os.path.join(out, "spec.json")),
            "spec instances must be 12": respec(lambda s: s.update(instances=6)),
            "spec packages are not the fresh-stage names":
                respec(lambda s: s["packages"].update(extra=s["packages"]["parent"])),
        }
        for problem, edit in cases.items():
            with self.subTest(problem), tempfile.TemporaryDirectory() as out:
                not_run, _ = stage(out)
                edit(out)
                spec = Path(out, "spec.json")
                digest = hashlib.sha256(spec.read_bytes() if spec.exists() else b"").hexdigest()
                r = pc1.report(out, not_run, digest)
                self.assertIn(problem, r["unbound"])
                self.assertEqual(r["reading"], "unbound")


class RealRecords(unittest.TestCase):
    def test_a_retained_trial_reads_as_its_recorded_verdict(self):
        # #116's primary trial: child 3/6 vs seeded random search 4/6, all usage known.
        plan = json.loads((TEMPLATE / "plan.json").read_text())
        cand, ctrl = (pc1.slot_record(str(TEMPLATE), plan, s) for s in ("candidate", "control"))
        self.assertEqual((cand["eligible"], cand["observed_successes"], cand["of"]), (True, 3, 6))
        self.assertEqual((ctrl["eligible"], ctrl["observed_successes"]), (True, 4))


class Cli(unittest.TestCase):
    def test_the_command_reads_the_not_run_list_and_refuses_bad_input(self):
        import contextlib
        import io
        with tempfile.TemporaryDirectory() as out:
            not_run, digest = stage(out, missing={("return-mix", "W")})
            nr = os.path.join(out, "..", os.path.basename(out) + "-not-run.json")
            Path(nr).write_text(json.dumps(not_run))
            buf = io.StringIO()
            with contextlib.redirect_stdout(buf):
                pc1.main(["pc1_report.py", "--spec-sha256", digest, "--not-run", nr, out])
            result = json.loads(buf.getvalue())
            self.assertEqual(result["families"]["return-mix"]["primary"]["status"], "not_run")
            self.assertEqual(result["unbound"], [])
            Path(nr).write_text(json.dumps({"not": "a list"}))
            for argv in (["pc1_report.py", out], ["pc1_report.py", "--spec-sha256", "x", out],
                         ["pc1_report.py", "--spec-sha256", digest, "--not-run", nr, out]):
                with self.assertRaises(SystemExit):
                    pc1.main(argv)
            os.remove(nr)
            # Without a lab-score binary nothing is assessed or reported.
            saved = os.environ.pop("LAB_SCORE")
            try:
                with self.assertRaises(SystemExit):
                    pc1.main(["pc1_report.py", "--spec-sha256", digest, out])
            finally:
                os.environ["LAB_SCORE"] = saved


if __name__ == "__main__":
    unittest.main()
