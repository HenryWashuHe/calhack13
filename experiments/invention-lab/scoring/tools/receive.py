#!/usr/bin/env python3
"""Run a predeclared set of receiving-side trials from one spec file.

    receive.py <spec.json> <out-dir>

Environment: LAB_EXEC and LAB_SCORE point at built `lab-exec` and `lab-score`
binaries. Standard library only.

The spec (schema `lab-receive-spec/1`) names the receiving organization, the
sandbox, equal per-arm grants, the method packages by path *and* expected
SHA-256, and the trials (family, candidate, control). Its own SHA-256 is printed
before anything runs and recorded in the summary, so the declaration can be
posted first and checked afterwards. Every trial is a fresh two-slot plan from
`lab-exec example-plan`, with the control slot's method, the slots'
organization and the plan id rewritten; every slot runs once, is scored by
`lab-score score`, decided by `lab-score verdict`, and the candidate gets
`lab-score feedback`. Failures are recorded, never retried. The output
directory must not exist. Nothing here interprets a verdict as improvement.
"""
import hashlib
import json
import os
import shutil
import subprocess
import sys

SCHEMA = "lab-receive-spec/1"
SPEC_KEYS = {"schema", "org_id", "sandbox", "instances", "simulation_steps",
             "wall_time_ms", "packages", "trials"}
TRIAL_KEYS = {"id", "family", "candidate", "control"}


def die(msg):
    sys.exit(f"receive: {msg}")


def sha256(path):
    with open(path, "rb") as f:
        return hashlib.sha256(f.read()).hexdigest()


def run(args, out=None):
    """Runs a lab binary; returns (exit code, parsed JSON or None)."""
    p = subprocess.run(args, capture_output=True, text=True)
    if out:
        with open(out, "w") as f:
            f.write(p.stdout)
        with open(out + ".err", "w") as f:
            f.write(p.stderr)
    try:
        return p.returncode, json.loads(p.stdout)
    except ValueError:
        return p.returncode, None


def load_spec(path, base):
    with open(path) as f:
        spec = json.load(f)
    if set(spec) != SPEC_KEYS or spec["schema"] != SCHEMA:
        die(f"spec must have exactly {sorted(SPEC_KEYS)} and schema {SCHEMA}")
    if spec["sandbox"] not in ("namespaces", "none"):
        die("sandbox must be namespaces or none")
    for k in ("instances", "simulation_steps", "wall_time_ms"):
        if not isinstance(spec[k], int) or isinstance(spec[k], bool) or spec[k] < 1:
            die(f"{k} must be a positive integer")
    packages = {}
    for name, p in spec["packages"].items():
        if set(p) != {"path", "sha256"}:
            die(f"package {name} needs exactly path and sha256")
        full = os.path.join(base, p["path"])
        if sha256(full) != p["sha256"]:
            die(f"package {name} bytes differ from the declared sha256")
        packages[name] = full
    ids = set()
    for t in spec["trials"]:
        if set(t) != TRIAL_KEYS:
            die(f"trial needs exactly {sorted(TRIAL_KEYS)}")
        if t["id"] in ids or not t["id"].replace("-", "").isalnum():
            die(f"trial id {t['id']!r} is repeated or not a plain name")
        ids.add(t["id"])
        for role in ("candidate", "control"):
            if t[role] not in packages:
                die(f"trial {t['id']} names unknown package {t[role]!r}")
    if not spec["trials"]:
        die("no trials")
    return spec, packages


def method_ref(path):
    with open(path) as f:
        pkg = json.load(f)["package"]
    return {"domain": "genesis", "schema": "development-method-source/1",
            "id": pkg["id"], "version": pkg["version"], "sha256": sha256(path)}


def trial(spec, packages, t, d, lab_exec, lab_score):
    cand, ctrl = packages[t["candidate"]], packages[t["control"]]
    code, _ = run([lab_exec, "example-plan", spec["sandbox"], str(spec["instances"]),
                   str(spec["simulation_steps"]), str(spec["wall_time_ms"]), d,
                   t["family"], cand], os.path.join(d, "example-plan.json"))
    if code != 0:
        return {"trial": t["id"], "error": "plan could not be built"}
    with open(os.path.join(d, "plan.json")) as f:
        plan = json.load(f)
    plan["id"] = f"receive-{t['id']}"
    plan["slots"][1]["method"] = method_ref(ctrl)
    for slot in plan["slots"]:
        slot["org_id"] = spec["org_id"]
    with open(os.path.join(d, "plan.json"), "w") as f:
        json.dump(plan, f, separators=(",", ":"))
    shutil.copyfile(cand, os.path.join(d, "candidate.package.json"))
    shutil.copyfile(ctrl, os.path.join(d, "control.package.json"))
    os.remove(os.path.join(d, "example-plan.json"))
    os.remove(os.path.join(d, "example-plan.json.err"))
    result = {"trial": t["id"], "family": t["family"],
              "candidate": t["candidate"], "control": t["control"], "slots": {}}
    p = os.path.join(d, "plan.json")
    for slot in ("candidate", "control"):
        state, score = os.path.join(d, slot), os.path.join(d, slot + "-score")
        code, ran = run([lab_exec, "run", p, slot, os.path.join(d, slot + ".package.json"),
                         state, spec["sandbox"]], os.path.join(d, slot + ".run.json"))
        row = {"run_exit": code}
        if ran:
            row["outcome"] = ran.get("outcome")
            row["usage"] = ran.get("usage")
        if os.path.exists(os.path.join(state, "receipt.json")):
            code, scored = run([lab_score, "score", p, slot, state, score],
                               os.path.join(d, slot + ".score.json"))
            row["score_exit"] = code
            if scored:
                row["result"] = scored.get("result")
        result["slots"][slot] = row
    code, verdict = run([lab_score, "verdict", p, os.path.join(d, "criterion.json"),
                         os.path.join(d, "candidate"), os.path.join(d, "candidate-score"),
                         os.path.join(d, "control"), os.path.join(d, "control-score")],
                        os.path.join(d, "verdict.json"))
    result["verdict"] = verdict if code == 0 else {"error": "no verdict"}
    code, fb = run([lab_score, "feedback", p, "candidate", os.path.join(d, "candidate"),
                    os.path.join(d, "candidate-score"),
                    os.path.join(d, "candidate-feedback.json")])
    result["candidate_feedback"] = fb if code == 0 else None
    return result


def main():
    if len(sys.argv) != 3:
        die(__doc__.strip().splitlines()[2].strip())
    spec_path, out = sys.argv[1], sys.argv[2]
    lab_exec, lab_score = os.environ.get("LAB_EXEC"), os.environ.get("LAB_SCORE")
    if not lab_exec or not lab_score:
        die("set LAB_EXEC and LAB_SCORE to the built binaries")
    spec, packages = load_spec(spec_path, os.path.dirname(os.path.abspath(spec_path)))
    spec_sha = sha256(spec_path)
    print(f"spec sha256 {spec_sha}", flush=True)
    if os.path.lexists(out):
        die(f"{out} already exists; results are never overwritten")
    os.makedirs(out)
    shutil.copyfile(spec_path, os.path.join(out, "spec.json"))
    code, runtime = run([lab_exec, "ref", "runtime"])
    results = []
    for t in spec["trials"]:
        d = os.path.join(out, t["id"])
        os.makedirs(d)
        results.append(trial(spec, packages, t, d, lab_exec, lab_score))
        v = results[-1].get("verdict") or {}
        print(f"{t['id']}: {v.get('verdict', v)}", flush=True)
    summary = {"schema": "lab-receive-summary/1", "spec_sha256": spec_sha,
               "host_runtime": runtime, "trials": results,
               "authority": "receiving-side observations; not adoption or improvement"}
    with open(os.path.join(out, "summary.json"), "w") as f:
        json.dump(summary, f, indent=2, sort_keys=True)
        f.write("\n")


if __name__ == "__main__":
    main()
