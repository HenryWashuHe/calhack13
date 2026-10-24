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
to {"W": package path or null, "F": package path or null}; null means
`no_candidate`, and every plan needing that side is listed in
`not-run.json` instead of being run. Plan (4) never depends on a selection.

Outputs are create-only. This writes specs; it runs nothing. The spec's
SHA-256 still has to be posted on issue #1 before `receive.py` uses it.
"""
import argparse
import hashlib
import json
import os
import sys

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


def package(out, path):
    with open(path, "rb") as f:
        data = f.read()
    return {"path": os.path.relpath(os.path.abspath(path), os.path.abspath(out)),
            "sha256": hashlib.sha256(data).hexdigest()}


def spec(org, instances, packages, trials):
    return {"schema": "lab-receive-spec/1", "org_id": org, "sandbox": "namespaces",
            "instances": instances, "simulation_steps": SIMULATION_STEPS,
            "wall_time_ms": WALL_TIME_MS, "packages": packages, "trials": trials}


def dev_spec(out, arm, rnd, candidate, control, org):
    if arm not in ARMS or rnd not in ROUNDS:
        die("arm must be W or F and round 1, 2 or 3")
    packages = {"candidate": package(out, candidate), "random-search": package(out, control)}
    trials = [{"id": f"dev-{arm.lower()}{rnd}-{fam}", "family": fam,
               "candidate": "candidate", "control": "random-search"} for fam in FAMILIES]
    return spec(org, DEV_INSTANCES, packages, trials), []


def fresh_spec(out, selections, random, parent, org):
    if set(selections) != set(FAMILIES) or any(
            set(sel) != set(ARMS) for sel in selections.values()):
        die(f"selections need exactly {list(FAMILIES)}, each with exactly W and F")
    packages = {"random-search": package(out, random), "parent": package(out, parent)}
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
            path = selections[fam][arm]
            if path is not None:
                packages[f"{arm.lower()}-{fam}"] = package(out, path)
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
    f = sub.add_parser("fresh")
    f.add_argument("--selections", required=True)
    f.add_argument("--random", required=True)
    f.add_argument("--parent", required=True)
    for s in (d, f):
        s.add_argument("--org", required=True)
        s.add_argument("--out", required=True)
    a = p.parse_args(argv)
    os.makedirs(a.out, exist_ok=True)
    if a.stage == "dev":
        result, not_run = dev_spec(a.out, a.arm, a.round, a.candidate, a.control, a.org)
    else:
        with open(a.selections) as fh:
            selections = json.load(fh)
        result, not_run = fresh_spec(a.out, selections, a.random, a.parent, a.org)
    target = os.path.join(a.out, "spec.json")
    write_new(target, result)
    if not_run:
        write_new(os.path.join(a.out, "not-run.json"), not_run)
    with open(target, "rb") as fh:
        print(hashlib.sha256(fh.read()).hexdigest())


if __name__ == "__main__":
    main()
