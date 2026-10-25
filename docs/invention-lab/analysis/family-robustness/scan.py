#!/usr/bin/env python3
"""Score every design retained under docs/invention-lab/evidence, plus the
evaluator's reference designs, on every family over a fixed public seed range.

    scan.py <lab-eval> <first-seed> <count> > results.json

Only the trusted evaluator runs (`lab-eval accept`); no method or generated
source executes, and nothing is written outside stdout.
"""
import hashlib, json, os, subprocess, sys

ROOT = os.path.dirname(os.path.abspath(__file__))
EVIDENCE = os.path.join(ROOT, "..", "..", "evidence")
REFERENCE = os.path.join(ROOT, "..", "..", "..", "..", "experiments", "invention-lab",
                         "evaluator", "designs")
FAMILIES = ("ravine-crossing", "rough-transport", "return-mix", "projectile-target")


def designs():
    """Each submitted design once, with every method and family it came from."""
    found = {}
    for top, _, names in os.walk(EVIDENCE):
        if "receipt.json" not in names:
            continue
        try:
            receipt = json.load(open(os.path.join(top, "receipt.json")))
            binding = receipt["binding"]
        except (ValueError, KeyError):
            continue
        for output in receipt.get("outputs", []):
            path = os.path.join(top, "artifacts", output["sha256"])
            if os.path.exists(path):
                entry = found.setdefault(output["sha256"], {"path": path, "submitted_by": set()})
                entry["submitted_by"].add((binding["method"]["id"], binding["method"]["sha256"][:8],
                                           binding["public_task"]["id"]))
    for name in sorted(os.listdir(REFERENCE)):
        path = os.path.join(REFERENCE, name)
        sha = hashlib.sha256(open(path, "rb").read()).hexdigest()
        found.setdefault(sha, {"path": path, "submitted_by": set()})["submitted_by"].add(
            ("evaluator-reference", name, "-"))
    return found


def main(argv):
    lab_eval, first, count = argv[1], int(argv[2]), int(argv[3])
    seeds = [str(s) for s in range(first, first + count)]
    rows = []
    for sha, entry in sorted(designs().items()):
        scores = {}
        for family in FAMILIES:
            done = subprocess.run([lab_eval, "accept", entry["path"], family, *seeds],
                                  capture_output=True, text=True, check=True)
            scores[family] = json.loads(done.stdout)["succeeded"]
        rows.append({"design": sha, "submitted_by": sorted(map(list, entry["submitted_by"])),
                     "successes": scores})
    json.dump({"seeds": [first, first + count - 1], "of": count, "families": FAMILIES,
               "designs": rows}, sys.stdout, indent=1)
    sys.stdout.write("\n")


if __name__ == "__main__":
    main(sys.argv)
