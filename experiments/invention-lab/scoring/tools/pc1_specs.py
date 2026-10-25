"""Receive specs for `process-comparison/1`, built from the frozen protocol
(docs/invention-lab/protocols/process-comparison-1.md) so no stage spec is
written by hand.

    pc1_specs.py dev   --arm W --round 1 --candidate PKG --control RANDOM --org ORG --out DIR
    pc1_specs.py fresh --selections SEL.json --random RANDOM --parent PARENT --org ORG --out DIR

`dev` writes one candidate's development stage: one trial per family, in
protocol family order, on seeds 0..5, against seeded random search.
`fresh` writes the fresh-panel stage on seeds 1000..1011: for each family, in
order, (1) W-selected vs F-selected, (2) W vs random search, (3) F vs random
search, (4) the unchanged parent vs random search. SEL.json maps each family
to {"W": entry, "F": entry}. An entry is null (`no_candidate`: every plan
needing that side is listed in `not-run.json` instead of being run) or
{"package": path, "sha256": the selected package's expected SHA-256}; the bytes
at the path must match it, so swapping a file after selection is refused. Plan
(4) never depends on a selection. Optional --*-sha256 flags pin the other
packages the same way.

This is preparation only. SEL.json is not the frozen selection record: the
selection itself (and the trusted random/parent identities) come from the
upstream roster/selection interface, whose exact references these pins should
copy. The output directory must not exist; spec.json and not-run.json are
written together into a staging directory and published by one rename, so a
spec is never left beside a stale or missing not-run declaration. This writes
specs; it runs nothing. The spec's SHA-256 still has to be posted on issue #1
before `receive.py` uses it.
"""
import argparse
import hashlib
import json
import os
import shutil
import sys
import tempfile

FAMILIES = ("ravine-crossing", "rough-transport", "return-mix", "projectile-target")
SIMULATION_STEPS = 72000
WALL_TIME_MS = 60000
DEV_INSTANCES = 6
FRESH_INSTANCES = 12
FRESH_FIRST_SEED = 1000
ARMS = ("W", "F")
ROUNDS = (1, 2, 3)


def die(msg):
    sys.exit(f"pc1_specs: {msg}")


def package(out, path, expected=None):
    with open(path, "rb") as f:
        data = f.read()
    digest = hashlib.sha256(data).hexdigest()
    if expected is not None and digest != expected:
        die(f"{path} is not the pinned package {expected}")
    return {"path": os.path.relpath(os.path.abspath(path), os.path.abspath(out)),
            "sha256": digest}


def selected(entry, where):
    """A selection entry: None, or exactly {package, sha256}."""
    if entry is None:
        return None
    if (not isinstance(entry, dict) or set(entry) != {"package", "sha256"}
            or not isinstance(entry["package"], str)
            or not isinstance(entry["sha256"], str) or len(entry["sha256"]) != 64
            or any(c not in "0123456789abcdef" for c in entry["sha256"])):
        die(f"{where}: a selection is null or exactly {{package, sha256}} with a hex digest")
    return entry


def spec(org, instances, packages, trials):
    return {"schema": "lab-receive-spec/1", "org_id": org, "sandbox": "namespaces",
            "instances": instances, "simulation_steps": SIMULATION_STEPS,
            "wall_time_ms": WALL_TIME_MS, "packages": packages, "trials": trials}


def dev_spec(out, arm, rnd, candidate, control, org, pins=None):
    pins = pins or {}
    if arm not in ARMS or rnd not in ROUNDS:
        die("arm must be W or F and round 1, 2 or 3")
    packages = {"candidate": package(out, candidate, pins.get("candidate")),
                "random-search": package(out, control, pins.get("control"))}
    trials = [{"id": f"dev-{arm.lower()}{rnd}-{fam}", "family": fam,
               "candidate": "candidate", "control": "random-search"} for fam in FAMILIES]
    return spec(org, DEV_INSTANCES, packages, trials), []


def fresh_spec(out, selections, random, parent, org, pins=None):
    pins = pins or {}
    if (not isinstance(selections, dict) or set(selections) != set(FAMILIES) or any(
            not isinstance(sel, dict) or set(sel) != set(ARMS) for sel in selections.values())):
        die(f"selections need exactly {list(FAMILIES)}, each with exactly W and F")
    packages = {"random-search": package(out, random, pins.get("random")),
                "parent": package(out, parent, pins.get("parent"))}
    trials, not_run = [], []

    def plan(fam, name, candidate, control):
        missing = [side for side, pkg in ((candidate, packages.get(candidate)),
                                          (control, packages.get(control))) if pkg is None]
        tid = f"fresh-{fam}-{name}"
        if missing:
            not_run.append({"trial": tid, "family": fam, "status": "not_run",
                            "reason": "no_candidate: " + ", ".join(missing)})
        else:
            trials.append({"id": tid, "family": fam, "candidate": candidate,
                           "control": control, "first_seed": FRESH_FIRST_SEED})

    for fam in FAMILIES:
        for arm in ARMS:
            entry = selected(selections[fam][arm], f"{fam}/{arm}")
            if entry is not None:
                packages[f"{arm.lower()}-{fam}"] = package(out, entry["package"], entry["sha256"])
        w, f = f"w-{fam}", f"f-{fam}"
        plan(fam, "primary", w, f)
        plan(fam, "w-random", w, "random-search")
        plan(fam, "f-random", f, "random-search")
        plan(fam, "parent-random", "parent", "random-search")
    return spec(org, FRESH_INSTANCES, packages, trials), not_run


def write_new(path, value):
    with open(path, "x") as f:
        json.dump(value, f, indent=2, sort_keys=True)
        f.write("\n")


def main(argv=None):
    p = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    sub = p.add_subparsers(dest="stage", required=True)
    d = sub.add_parser("dev")
    d.add_argument("--arm", required=True)
    d.add_argument("--round", type=int, required=True)
    d.add_argument("--candidate", required=True)
    d.add_argument("--control", required=True)
    d.add_argument("--candidate-sha256")
    d.add_argument("--control-sha256")
    f = sub.add_parser("fresh")
    f.add_argument("--selections", required=True)
    f.add_argument("--random", required=True)
    f.add_argument("--parent", required=True)
    f.add_argument("--random-sha256")
    f.add_argument("--parent-sha256")
    for s in (d, f):
        s.add_argument("--org", required=True)
        s.add_argument("--out", required=True)
    a = p.parse_args(argv)
    out = os.path.abspath(a.out)
    if os.path.lexists(out):
        die(f"{a.out} already exists; specs are published once into a new directory")
    if a.stage == "dev":
        pins = {"candidate": a.candidate_sha256, "control": a.control_sha256}
        result, not_run = dev_spec(out, a.arm, a.round, a.candidate, a.control, a.org, pins)
    else:
        with open(a.selections) as fh:
            selections = json.load(fh)
        pins = {"random": a.random_sha256, "parent": a.parent_sha256}
        result, not_run = fresh_spec(out, selections, a.random, a.parent, a.org, pins)
    # Stage the complete pair beside the destination, then publish it whole.
    parent = os.path.dirname(out)
    os.makedirs(parent, exist_ok=True)
    staging = tempfile.mkdtemp(prefix=".pc1-specs-", dir=parent)
    try:
        write_new(os.path.join(staging, "spec.json"), result)
        if not_run:
            write_new(os.path.join(staging, "not-run.json"), not_run)
        with open(os.path.join(staging, "spec.json"), "rb") as fh:
            digest = hashlib.sha256(fh.read()).hexdigest()
        os.rename(staging, out)
    except BaseException:
        shutil.rmtree(staging, ignore_errors=True)
        raise
    print(digest)


if __name__ == "__main__":
    main()
