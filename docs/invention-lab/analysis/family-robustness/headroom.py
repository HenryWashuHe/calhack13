#!/usr/bin/env python3
"""The predeclared headroom comparison (commit 7ec9a24): portfolio_search.py
against random_search.py on fresh public panels, with two fixed designs scored
on the same panels for reference.

    headroom.py <lab-eval> > headroom.json

Panels: seeds 31000..31035, six panels of six, for ravine-crossing,
rough-transport and return-mix. Both methods are hand-written controls run
through calibrate.py's offline emulation of the executor protocol (not the
executor, not trial evidence). The fixed designs run only through
`lab-eval accept`. No generated source executes.
"""
import json
import os
import subprocess
import sys
import tempfile

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import calibrate  # noqa: E402

FAMILIES = ("ravine-crossing", "rough-transport", "return-mix")
FIRST, PANELS, GRANT = 31000, 6, 72000
METHODS = {
    "portfolio_search": os.path.join(HERE, "portfolio_search.py"),
    "random_search": os.path.join(HERE, "..", "..", "..", "..", "experiments", "invention-lab",
                                  "executor", "methods", "random-search", "random_search.py"),
}
PARENT = "2483f08f"


def parent_design():
    evidence = os.path.join(HERE, "..", "..", "evidence")
    for top, _, names in os.walk(evidence):
        for name in names:
            if name.startswith(PARENT) and os.path.basename(top) == "artifacts":
                return os.path.join(top, name)
    raise SystemExit("parent design not found")


FIXED = {
    "parent_2483f08f": parent_design(),
    "reference_five_wheel": os.path.join(HERE, "..", "..", "..", "..", "experiments",
                                         "invention-lab", "evaluator", "designs",
                                         "reference-five-wheel.json"),
}


def main(argv):
    lab_eval = argv[1]
    out = {"seeds": [FIRST, FIRST + 6 * PANELS - 1], "grant": GRANT, "families": {}}
    with tempfile.TemporaryDirectory() as tmp:
        for family in FAMILIES:
            row = {}
            panels = [list(range(FIRST + 6 * p, FIRST + 6 * p + 6)) for p in range(PANELS)]
            for name, path in METHODS.items():
                runs = [calibrate.panel_run(lab_eval, path, family, seeds, GRANT, tmp)
                        for seeds in panels]
                row[name] = {"per_panel": [r["successes"] for r in runs],
                             "total": sum(r["successes"] or 0 for r in runs)}
            for name, path in FIXED.items():
                per = []
                for seeds in panels:
                    done = subprocess.run([lab_eval, "accept", path, family, *map(str, seeds)],
                                          capture_output=True, text=True, check=True)
                    per.append(json.loads(done.stdout)["succeeded"])
                row[name] = {"per_panel": per, "total": sum(per)}
            out["families"][family] = row
    json.dump(out, sys.stdout, indent=1)
    sys.stdout.write("\n")


if __name__ == "__main__":
    main(sys.argv)
