#!/usr/bin/env python3
"""Writes the mixed-terrain/1 confirmation freeze manifest (proposal #195).

    freeze.py <lab-eval-mixed> > ../confirmation/freeze.json

It pins the whole comparison, not just the generator: the evaluator identity
and build, the generator source, each control's source bytes, the fixed
designs, every harness script, the scoring rule, the panel-to-seed assignment
and the grants. Read-only: it runs only `lab-eval-mixed identity` and hashes
files. Anything not listed here is not part of the confirmation.
"""
import hashlib
import json
import os
import subprocess
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import confirm  # noqa: E402
import run  # noqa: E402

ROOT = run.ROOT


def sha(path):
    with open(path, "rb") as f:
        return hashlib.sha256(f.read()).hexdigest()


def rel(path):
    return os.path.relpath(os.path.realpath(path), ROOT)


def main(argv):
    lab_eval = argv[1]
    ident = json.loads(subprocess.run([lab_eval, "identity"], capture_output=True, text=True,
                                      check=True).stdout)
    crate = os.path.dirname(HERE)
    harness = [os.path.join(run.ANALYSIS, "calibrate.py"), os.path.join(run.ANALYSIS, "headroom.py"),
               os.path.join(HERE, "run.py"), os.path.join(HERE, "confirm.py"),
               os.path.join(HERE, "freeze.py")]
    out = {
        "schema": "mixed-terrain-freeze/1",
        "family": {"name": run.FAMILY, "version": ident["family_version"],
                   "generator": {rel(os.path.join(crate, "src", "family.rs")):
                                 sha(os.path.join(crate, "src", "family.rs"))}},
        "evaluator": {"id": ident["evaluator"], "sha256": ident["evaluator_sha256"],
                      "v1_sha256": ident["v1_evaluator_sha256"],
                      "files": {rel(os.path.join(crate, f)): sha(os.path.join(crate, f))
                                for f in ident["files"]},
                      "binary": {"path": "lab-eval-mixed (release build of the files above)",
                                 "sha256": sha(lab_eval)}},
        "controls": {name: {rel(p): sha(p)} for name, p in run.METHODS.items()},
        "fixed_designs": {name: {rel(p): sha(p)} for name, p in run.FIXED.items()},
        "harness": {rel(p): sha(p) for p in harness},
        "interpreter_for_controls": sys.version.split()[0],
        "scoring": {"rule": "lab-eval-mixed accept success count per panel of 6",
                    "margin": 1, "measured_outcomes": sorted(run.calibrate.MEASURED)},
        "panels": [[confirm.FIRST + 6 * p, confirm.FIRST + 6 * p + 5]
                   for p in range(confirm.PANELS)],
        "grant": {"simulation_steps": confirm.GRANT, "episode_steps": 2400,
                  "wall": "60 s per slot in v1 terms; the offline emulation enforces "
                          f"{run.calibrate.EVALUATOR_TIMEOUT_S} s per evaluator call and "
                          f"{run.calibrate.PANEL_DEADLINE_S} s per panel instead"},
        "reading": {"discriminating": f"portfolio_search beats random_search by margin one on "
                                      f">= {confirm.WINS_NEEDED} of {confirm.PANELS} panels and "
                                      f"loses on <= {confirm.LOSSES_ALLOWED}, all panels available",
                    "incomplete": "any panel unavailable for either arm; published, not rerun",
                    "otherwise": "not discriminating"},
    }
    json.dump(out, sys.stdout, indent=1, sort_keys=True)
    sys.stdout.write("\n")


if __name__ == "__main__":
    main(sys.argv)
