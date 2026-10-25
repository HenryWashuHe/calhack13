#!/usr/bin/env python3
"""Endpoints of `process-comparison/1` from the retained fresh-panel records,
exactly as the frozen protocol defines them
(docs/invention-lab/protocols/process-comparison-1.md, § Endpoints).

    pc1_report.py <fresh-out-dir> [<not-run.json>]

<fresh-out-dir> is `receive.py`'s output for the fresh stage (trials named
`fresh-<family>-<plan>`, plan one of primary, w-random, f-random,
parent-random). <not-run.json> is the list `pc1_specs.py fresh` wrote for
plans that were never run. Prints one JSON report. Standard library only;
reads retained files and runs nothing.

Eligibility is the protocol's one rule, applied here to the retained files
independently of any other implementation so the two can be compared:
execution `output_generated` with every usage field measured, a `scored`
scoring receipt whose measured usage is within the plan's scoring grant, and
a success count known from the metrics artifact (digest checked, one boolean
row per scoring instance). Anything else is ineligible and its observed
result is still reported. Nothing here reads a verdict as improvement.
"""
import hashlib
import json
import os
import sys

FAMILIES = ("ravine-crossing", "rough-transport", "return-mix", "projectile-target")
PLANS = ("primary", "w-random", "f-random", "parent-random")
USAGE = ("model_calls", "input_tokens", "output_tokens", "wall_time_ms", "simulation_steps")


def load(path):
    try:
        with open(path, "rb") as f:
            return json.loads(f.read())
    except (OSError, ValueError):
        return None


def slot_record(trial_dir, plan, slot):
    """The protocol's eligibility for one slot, with what was observed."""
    reasons = []
    receipt = load(os.path.join(trial_dir, slot, "receipt.json"))
    outcome = receipt.get("outcome") if isinstance(receipt, dict) else None
    if receipt is None:
        reasons.append("no execution receipt")
    elif outcome != "output_generated":
        reasons.append(f"execution {outcome}")
    if isinstance(receipt, dict):
        usage = receipt.get("usage") or {}
        unknown = [k for k in USAGE if (usage.get(k) or {}).get("kind") != "measured"]
        if unknown:
            reasons.append("execution usage not measured: " + ", ".join(unknown))
    score_dir = os.path.join(trial_dir, slot + "-score")
    evaluation = load(os.path.join(score_dir, "evaluation.json"))
    successes, of = None, None
    if evaluation is None:
        reasons.append("no scoring receipt")
    else:
        result = evaluation.get("outcome") or {}
        if result.get("kind") != "scored":
            reasons.append(f"scoring {result.get('kind')}")
        grant = plan.get("scoring_grant") or {}
        used = evaluation.get("usage") or {}
        for k in USAGE:
            u = used.get(k) or {}
            if u.get("kind") != "measured":
                reasons.append(f"scoring usage {k} not measured")
            elif not isinstance(grant.get(k), int) or u.get("value", 0) > grant[k]:
                reasons.append(f"scoring {k} over its grant")
        metrics_ref = result.get("metrics") or {}
        digest = metrics_ref.get("sha256")
        path = os.path.join(score_dir, "artifacts", str(digest))
        try:
            with open(path, "rb") as f:
                data = f.read()
        except OSError:
            data = None
        if result.get("kind") == "scored":
            expected = [i.get("id") for i in plan.get("development_instances") or []]
            if data is None or hashlib.sha256(data).hexdigest() != digest:
                reasons.append("metrics artifact missing or not its digest")
            else:
                rows = (load(path) or {}).get("instances")
                ok = (isinstance(rows, list) and len(rows) == len(expected)
                      and all(isinstance(r, dict) and r.get("instance") == e
                              and isinstance(r.get("success"), bool)
                              for r, e in zip(rows, expected)))
                if ok:
                    successes, of = sum(r["success"] for r in rows), len(rows)
                else:
                    reasons.append("success count unknown from the metrics")
    return {"eligible": not reasons, "reasons": reasons, "outcome": outcome,
            "observed_successes": successes, "of": of}


def trial_record(out, family, name, not_run):
    tid = f"fresh-{family}-{name}"
    if tid in not_run:
        return {"trial": tid, "status": "not_run", "reason": not_run[tid]}
    d = os.path.join(out, tid)
    plan = load(os.path.join(d, "plan.json"))
    if not isinstance(plan, dict):
        return {"trial": tid, "status": "missing"}
    return {"trial": tid, "status": "run",
            "verdict": load(os.path.join(d, "verdict.json")),
            "candidate": slot_record(d, plan, "candidate"),
            "control": slot_record(d, plan, "control")}


def contribution(slot):
    """An ineligible slot contributes 0 and blocks the positive reading."""
    return slot["observed_successes"] if slot["eligible"] else 0


def report(out, not_run_list=()):
    not_run = {r["trial"]: r.get("reason") for r in not_run_list}
    families = {f: {p: trial_record(out, f, p, not_run) for p in PLANS} for f in FAMILIES}
    primaries = [families[f]["primary"] for f in FAMILIES]
    complete = all(t["status"] == "run" for t in primaries)
    ran = [t for t in primaries if t["status"] == "run"]
    w_sum = sum(contribution(t["candidate"]) for t in ran)
    f_sum = sum(contribution(t["control"]) for t in ran)
    primary_eligible = complete and all(t["candidate"]["eligible"] and t["control"]["eligible"]
                                        for t in primaries)
    refs = [families[f]["parent-random"] for f in FAMILIES]
    ref_ok = all(t["status"] == "run" and t["control"]["eligible"] for t in refs)
    ref_sum = sum(t["control"]["observed_successes"] for t in refs) if ref_ok else None
    conditions = {
        "1_primary_complete": complete,
        "2_every_primary_slot_eligible": primary_eligible,
        "3_all_reference_slots_eligible": ref_ok,
        "4_w_exceeds_f_by_2": complete and w_sum - f_sum >= 2,
        "5_w_at_least_reference": ref_ok and w_sum >= ref_sum,
    }
    if all(conditions.values()):
        reading = "positive_exploratory"
    elif not (complete and primary_eligible and ref_ok):
        reading = "incomplete"
    else:
        reading = "not_positive"
    return {
        "schema": "pc1-fresh-report/1",
        "protocol": "process-comparison/1",
        "primary": {"complete": complete, "w_sum": w_sum, "f_sum": f_sum,
                    "difference": w_sum - f_sum if complete else None,
                    "of": 12 * len(ran)},
        "reference": {"all_eligible": ref_ok, "sum": ref_sum},
        "positive_reading": conditions,
        "reading": reading,
        "note": ("Null, negative and incomplete are reported as such; 'not_positive' leaves "
                 "that label to the reader with the numbers above. No significance test."),
        "families": families,
    }


def main(argv):
    if len(argv) not in (2, 3):
        sys.exit("usage: pc1_report.py <fresh-out-dir> [<not-run.json>]")
    not_run = load(argv[2]) if len(argv) == 3 else []
    if not isinstance(not_run, list):
        sys.exit("pc1_report: not-run file must be a JSON list")
    json.dump(report(argv[1], not_run), sys.stdout, indent=2, sort_keys=True)
    sys.stdout.write("\n")


if __name__ == "__main__":
    main(sys.argv)
