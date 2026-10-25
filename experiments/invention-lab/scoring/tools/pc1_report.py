#!/usr/bin/env python3
"""Endpoints of `process-comparison/1` from the retained fresh-panel records,
exactly as the frozen protocol defines them
(docs/invention-lab/protocols/process-comparison-1.md, § Endpoints).

    pc1_report.py --spec-sha256 HEX [--not-run FILE] [--lab-score PATH] <fresh-out-dir>

<fresh-out-dir> is `receive.py`'s output for the fresh stage. It retains the
spec bytes it ran (`spec.json`); --spec-sha256 is the hash posted on issue #1
before the run, and the retained spec must match it. --not-run is the list
`pc1_specs.py fresh` wrote beside that spec. --lab-score (or $LAB_SCORE) is
the `lab-score` binary. Prints one JSON report and runs no method or scorer.

This file only aggregates. Per slot, eligibility is the shared Rust
assessment (`lab-score eligibility`, i.e. genesis_core::assess_process_slot),
which binds each execution and scoring receipt to the expected plan and reads
typed usage; nothing here re-derives it from loose JSON. What this file adds is
binding every retained plan to the pinned spec before anything is counted:
the exact 16-plan roster less declared not-run plans, plan ids, the 12-instance
fresh panel from seed 1000, the spec's package digests as slot methods, the
receiving organization and run identities, the spec's development grant, and
one shared evaluator/criterion/environment/scoring policy across all plans.
A record that does not bind makes the reading `unbound`, never a number.
"""
import argparse
import copy
import hashlib
import json
import os
import subprocess
import sys
import tempfile

FAMILIES = ("ravine-crossing", "rough-transport", "return-mix", "projectile-target")
PLANS = ("primary", "w-random", "f-random", "parent-random")
FRESH_INSTANCES, FIRST_SEED = 12, 1000
# process-comparison/1's frozen resource policy. Scoring steps are 12 episodes
# of the family's maximum length (evaluator `Family::max_steps`).
DEVELOPMENT_STEPS, DEVELOPMENT_WALL_MS, SCORING_WALL_MS = 72000, 60000, 600000
EPISODE_STEPS = {"projectile-target": 1200}
DEFAULT_EPISODE_STEPS = 2400
# Family-specific context; everything else in a plan must agree across all plans.
FAMILY_CONTEXT = ("public_task", "generator", "scoring_rule", "instance_set")


def load(path):
    try:
        with open(path, "rb") as f:
            return json.loads(f.read())
    except (OSError, ValueError):
        return None


def sides(family, name):
    return {"primary": (f"w-{family}", f"f-{family}"),
            "w-random": (f"w-{family}", "random-search"),
            "f-random": (f"f-{family}", "random-search"),
            "parent-random": ("parent", "random-search")}[name]


def lab_score_path(explicit=None):
    path = explicit or os.environ.get("LAB_SCORE")
    if not path:
        raise SystemExit("pc1_report: pass --lab-score or set LAB_SCORE")
    return path


def assess(trial_dir, plan, slot, lab_score):
    """The shared assessment for one slot against `plan`, or the refusal."""
    with tempfile.TemporaryDirectory(prefix="pc1-plan-") as tmp:
        expected = os.path.join(tmp, "plan.json")
        with open(expected, "w") as f:
            json.dump(plan, f)
        done = subprocess.run(
            [lab_score, "eligibility", expected, slot, os.path.join(trial_dir, slot),
             os.path.join(trial_dir, slot + "-score")],
            capture_output=True, text=True, timeout=120)
    if done.returncode != 0:
        return None, (done.stderr.strip() or f"exit {done.returncode}")[:500]
    try:
        return json.loads(done.stdout), None
    except ValueError:
        return None, "assessment output is not JSON"


def slot_record(trial_dir, plan, slot, lab_score=None):
    """One slot's eligibility from the shared assessment, with what was observed."""
    instances = len(plan.get("development_instances") or []) if isinstance(plan, dict) else 0
    if not os.path.exists(os.path.join(trial_dir, slot, "receipt.json")):
        return {"eligible": False, "reasons": ["no execution receipt"], "outcome": None,
                "observed_successes": None, "of": instances, "assessment": None}
    result, refusal = assess(trial_dir, plan, slot, lab_score_path(lab_score))
    if result is None:
        return {"eligible": False, "reasons": ["assessment refused: " + refusal],
                "outcome": None, "observed_successes": None, "of": instances,
                "assessment": None}
    reasons = list(result["reasons"])
    if not os.path.exists(os.path.join(trial_dir, slot + "-score", "evaluation.json")):
        reasons.insert(0, "no scoring receipt")
    return {"eligible": result["eligible"] is True and not reasons, "reasons": reasons,
            "outcome": result["execution"], "observed_successes": result["successes"],
            "of": instances, "assessment": result}


def spec_problems(spec, not_run):
    """The retained spec against the protocol's fresh roster and the not-run list."""
    if not isinstance(spec, dict) or spec.get("schema") != "lab-receive-spec/1":
        return ["no retained lab-receive-spec/1 spec.json"], {}
    problems = []
    if spec.get("instances") != FRESH_INSTANCES:
        problems.append(f"spec instances must be {FRESH_INSTANCES}")
    if (spec.get("simulation_steps"), spec.get("wall_time_ms")) != (
            DEVELOPMENT_STEPS, DEVELOPMENT_WALL_MS):
        problems.append("spec development grant is not the protocol's 72,000 steps / 60,000 ms")
    packages = spec.get("packages") if isinstance(spec.get("packages"), dict) else {}
    allowed = {"random-search", "parent"} | {f"{a}-{f}" for a in "wf" for f in FAMILIES}
    if set(packages) - allowed or not {"random-search", "parent"} <= set(packages):
        problems.append("spec packages are not the fresh-stage names")
    trials = {t.get("id"): t for t in spec.get("trials") or [] if isinstance(t, dict)}
    expected = {}
    expected_not_run = []
    for fam in FAMILIES:
        for name in PLANS:
            tid = f"fresh-{fam}-{name}"
            cand, ctrl = sides(fam, name)
            missing = [s for s in (cand, ctrl) if s not in packages]
            if missing:
                if tid in trials:
                    problems.append(f"{tid} is in the spec without package {missing}")
                elif not_run.get(tid) != "no_candidate: " + ", ".join(missing):
                    problems.append(f"{tid} lacks {missing} but is not declared not run")
                expected_not_run.append(tid)
                continue
            if tid in not_run:
                problems.append(f"{tid} is declared not run but the spec has its packages")
            t = trials.get(tid)
            if t != {"id": tid, "family": fam, "candidate": cand, "control": ctrl,
                     "first_seed": FIRST_SEED}:
                problems.append(f"{tid} is not the protocol's spec entry")
            expected[tid] = (fam, cand, ctrl)
    extra = set(trials) - set(expected)
    if extra or len(trials) != len(spec.get("trials") or []):
        problems.append(f"spec has unexpected or repeated trials: {sorted(extra)}")
    order = [t.get("id") for t in spec.get("trials") or [] if isinstance(t, dict)]
    if order != list(expected):
        problems.append("spec trials are not in the protocol's family and plan order")
    if list(not_run) != expected_not_run:
        problems.append("not-run list is not the exact ordered complement of the spec")
    return problems, expected


def package_ref(out, spec, name):
    """The full method reference receive.py derives from the retained package
    snapshot, if those bytes are the spec's pin; otherwise None."""
    try:
        with open(os.path.join(out, "packages", f"{name}.package.json"), "rb") as f:
            data = f.read()
        pkg = json.loads(data)["package"]
    except (OSError, ValueError, KeyError, TypeError):
        return None
    digest = hashlib.sha256(data).hexdigest()
    if digest != spec["packages"][name]["sha256"]:
        return None
    return {"domain": "genesis", "schema": "development-method-source/1",
            "id": pkg.get("id"), "version": pkg.get("version"), "sha256": digest}


def criterion_problems(trial_dir, plan, ids):
    """The retained criterion.json must be the executor's criterion for this panel."""
    try:
        with open(os.path.join(trial_dir, "criterion.json"), "rb") as f:
            data = f.read()
    except OSError:
        return ["criterion.json is not retained"]
    expected = json.dumps({"instances": ids, "min_margin": 1, "rule": "success-count-margin"},
                          sort_keys=True, separators=(",", ":")).encode()
    criterion = plan.get("criterion") or {}
    if data != expected or criterion.get("sha256") != hashlib.sha256(data).hexdigest():
        return ["criterion is not the success-count margin over this panel"]
    return []


def plan_problems(out, plan, spec, tid, fam, cand, ctrl):
    """The retained plan against what the pinned spec and receive.py determine."""
    if not isinstance(plan, dict):
        return ["plan.json missing or not JSON"]
    problems = []
    pid = f"receive-{tid}"
    if plan.get("id") != pid:
        problems.append("plan id is not receive-<trial>")
    ids = [i.get("id") for i in plan.get("development_instances") or [] if isinstance(i, dict)]
    if ids != [f"{fam}/1/{FIRST_SEED + i}" for i in range(FRESH_INSTANCES)]:
        problems.append(f"instances are not the frozen fresh panel {fam}/1/1000..1011")
    if plan.get("scoring_instances") != {"kind": "development"}:
        problems.append("scoring instances are not the public panel")
    context = plan.get("context") or {}
    if any((context.get(k) or {}).get("id") != fam for k in ("public_task", "generator",
                                                              "scoring_rule")):
        problems.append("task context is not this family")
    grant = plan.get("development_grant") or {}
    if (grant.get("simulation_steps"), grant.get("wall_time_ms")) != (
            spec.get("simulation_steps"), spec.get("wall_time_ms")) or any(
            grant.get(k) != 0 for k in ("model_calls", "input_tokens", "output_tokens")):
        problems.append("development grant differs from the spec")
    expected_scoring = {"model_calls": 0, "input_tokens": 0, "output_tokens": 0,
                        "wall_time_ms": SCORING_WALL_MS,
                        "simulation_steps": FRESH_INSTANCES * EPISODE_STEPS.get(
                            fam, DEFAULT_EPISODE_STEPS)}
    if plan.get("scoring_grant") != expected_scoring:
        problems.append("scoring grant is not the protocol's for this family")
    slots = plan.get("slots") or []
    if [s.get("slot_id") for s in slots] != ["candidate", "control"]:
        return problems + ["slots are not candidate then control"]
    for s, name, pkg in ((slots[0], "candidate", cand), (slots[1], "control", ctrl)):
        m = s.get("method") or {}
        if m != package_ref(out, spec, pkg):
            problems.append(f"{name} method is not spec package {pkg}")
        if (s.get("org_id"), s.get("run_id"), s.get("attempt_id"),
                s.get("scoring_request_id")) != (
                spec.get("org_id"), f"run-{pid}-{name}", f"att_{pid}_{name}",
                f"score-{pid}-{name}"):
            problems.append(f"{name} organization or run identities differ")
        if s.get("purpose") != {"kind": "acceptance", "role": name} or s.get("dependencies"):
            problems.append(f"{name} purpose or dependencies differ")
    return problems


def policy(plan):
    """What every fresh plan must share: all but identities, panel, task and
    the family-specific parts (criterion over the panel, scoring steps)."""
    p = copy.deepcopy(plan)
    for k in ("id", "development_instances", "slots", "criterion"):
        p.pop(k, None)
    for k in FAMILY_CONTEXT:
        (p.get("context") or {}).pop(k, None)
    if isinstance(p.get("scoring_grant"), dict):
        p["scoring_grant"].pop("simulation_steps", None)
    return p


def family_policy(plan):
    """What every plan of one family must share besides the common policy."""
    return {"criterion": plan.get("criterion"),
            "scoring_steps": (plan.get("scoring_grant") or {}).get("simulation_steps")}


def trial_record(out, tid, expected, spec, lab_score):
    fam, cand, ctrl = expected
    d = os.path.join(out, tid)
    if not os.path.isdir(d):
        return {"trial": tid, "status": "missing"}
    plan = load(os.path.join(d, "plan.json"))
    problems = plan_problems(out, plan, spec, tid, fam, cand, ctrl)
    if isinstance(plan, dict):
        ids = [i.get("id") for i in plan.get("development_instances") or []
               if isinstance(i, dict)]
        problems += criterion_problems(d, plan, ids)
    record = {"trial": tid, "status": "unbound" if problems else "run", "problems": problems,
              "verdict": load(os.path.join(d, "verdict.json"))}
    if isinstance(plan, dict):
        record["candidate"] = slot_record(d, plan, "candidate", lab_score)
        record["control"] = slot_record(d, plan, "control", lab_score)
        record["policy"] = hashlib.sha256(
            json.dumps(policy(plan), sort_keys=True).encode()).hexdigest()
        record["family_policy"] = hashlib.sha256(
            json.dumps(family_policy(plan), sort_keys=True).encode()).hexdigest()
    return record


def contribution(trial, side):
    """An ineligible slot contributes 0 and blocks the positive reading."""
    s = trial[side]
    return s["observed_successes"] if s["eligible"] else 0


def report(out, not_run_list=(), spec_sha256=None, lab_score=None):
    not_run = {}
    unbound = []
    for r in not_run_list:
        tid = r.get("trial") if isinstance(r, dict) else None
        if tid in not_run or not isinstance(tid, str):
            unbound.append(f"not-run entry {tid!r} is repeated or malformed")
            continue
        not_run[tid] = r.get("reason")
    try:
        with open(os.path.join(out, "spec.json"), "rb") as f:
            spec_bytes = f.read()
    except OSError:
        spec_bytes = b""
    retained_sha = hashlib.sha256(spec_bytes).hexdigest()
    if spec_sha256 is not None and spec_sha256 != retained_sha:
        unbound.append("retained spec.json is not the posted spec")
    try:
        spec = json.loads(spec_bytes)
    except ValueError:
        spec = None
    problems, expected = spec_problems(spec, not_run)
    unbound += problems
    families = {f: {} for f in FAMILIES}
    for fam in FAMILIES:
        for name in PLANS:
            tid = f"fresh-{fam}-{name}"
            if tid in expected:
                families[fam][name] = trial_record(out, tid, expected[tid], spec, lab_score)
            else:
                families[fam][name] = {"trial": tid, "status": "not_run",
                                       "reason": not_run.get(tid)}
                if os.path.exists(os.path.join(out, tid)):
                    unbound.append(f"{tid} is declared not run but has records")
    records = [t for fam in families.values() for t in fam.values()]
    unbound += [f"{t['trial']}: {p}" for t in records for p in t.get("problems", [])]
    if len({t["policy"] for t in records if "policy" in t}) > 1:
        unbound.append("plans differ in evaluator, environment or grants")
    for fam, plans in families.items():
        if len({t["family_policy"] for t in plans.values() if "family_policy" in t}) > 1:
            unbound.append(f"{fam} plans differ in criterion or scoring steps")
    primaries = [families[f]["primary"] for f in FAMILIES]
    complete = not unbound and all(t["status"] == "run" for t in primaries)
    ran = [t for t in primaries if t["status"] == "run"]
    w_sum = sum(contribution(t, "candidate") for t in ran)
    f_sum = sum(contribution(t, "control") for t in ran)
    primary_eligible = complete and all(t["candidate"]["eligible"] and t["control"]["eligible"]
                                        for t in primaries)
    refs = [families[f]["parent-random"] for f in FAMILIES]
    ref_ok = not unbound and all(t["status"] == "run" and t["control"]["eligible"]
                                 for t in refs)
    ref_sum = sum(t["control"]["observed_successes"] for t in refs) if ref_ok else None
    conditions = {
        "1_primary_complete": complete,
        "2_every_primary_slot_eligible": primary_eligible,
        "3_all_reference_slots_eligible": ref_ok,
        "4_w_exceeds_f_by_2": complete and w_sum - f_sum >= 2,
        "5_w_at_least_reference": ref_ok and w_sum >= ref_sum,
    }
    if unbound:
        reading = "unbound"
    elif all(conditions.values()):
        reading = "positive_exploratory"
    elif not (complete and primary_eligible and ref_ok):
        reading = "incomplete"
    else:
        reading = "not_positive"
    return {
        "schema": "pc1-fresh-report/1",
        "protocol": "process-comparison/1",
        "spec_sha256": retained_sha,
        "spec_pinned": spec_sha256 is not None and not unbound,
        "unbound": unbound,
        "primary": {"complete": complete, "w_sum": w_sum, "f_sum": f_sum,
                    "difference": w_sum - f_sum if complete else None,
                    "of": FRESH_INSTANCES * len(ran)},
        "reference": {"all_eligible": ref_ok, "sum": ref_sum},
        "positive_reading": conditions,
        "reading": reading,
        "note": ("Null, negative, incomplete and unbound are reported as such; 'not_positive' "
                 "leaves that label to the reader with the numbers above. No significance "
                 "test. Plan evaluator/environment identities are checked for agreement "
                 "across plans, not against an external pin."),
        "families": families,
    }


def main(argv):
    p = argparse.ArgumentParser(prog="pc1_report.py")
    p.add_argument("--spec-sha256", required=True)
    p.add_argument("--not-run")
    p.add_argument("--lab-score")
    p.add_argument("out")
    a = p.parse_args(argv[1:])
    if len(a.spec_sha256) != 64 or set(a.spec_sha256) - set("0123456789abcdef"):
        sys.exit("pc1_report: --spec-sha256 must be a lowercase hex SHA-256")
    not_run = load(a.not_run) if a.not_run else []
    if not isinstance(not_run, list):
        sys.exit("pc1_report: not-run file must be a JSON list")
    result = report(a.out, not_run, a.spec_sha256, lab_score_path(a.lab_score))
    json.dump(result, sys.stdout, indent=2, sort_keys=True)
    sys.stdout.write("\n")


if __name__ == "__main__":
    main(sys.argv)
