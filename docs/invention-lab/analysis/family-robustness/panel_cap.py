#!/usr/bin/env python3
"""Best single launcher per 6-instance projectile-target panel.

    panel_cap.py <lab-eval> <rng-seed> <designs> <first-seed>...

For each panel (6 consecutive public seeds from each first-seed), scores
<designs> random launchers from mixed_search.launcher on all six instances
with `lab-eval accept` and reports the histogram of success counts. A trial
scores one submitted design on every panel instance, so the best sampled
count describes what this finite random sample reached; it is not a bound on
what a method that submits a launcher of this shape can reach. Evaluator
calls that fail, time out or print no count stop the run with an error: a
histogram is never printed with a failure left out. Only the evaluator runs.
Prints JSON.
"""
import json, os, random, subprocess, sys, tempfile
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import mixed_search


def lab(lab_eval, *args):
    """One evaluator call's JSON; exits on a failed, slow or malformed call."""
    try:
        done = subprocess.run([lab_eval, *args], capture_output=True, text=True, timeout=120)
        if done.returncode == 0:
            return json.loads(done.stdout)
    except (subprocess.TimeoutExpired, ValueError):
        pass
    sys.exit(f"panel_cap: evaluator call failed ({args[0]}); refusing a partial histogram")


def main(argv):
    lab_eval, rnd, n = argv[1], random.Random(int(argv[2])), int(argv[3])
    rows = []
    with tempfile.TemporaryDirectory() as tmp:
        path = os.path.join(tmp, "d.json")
        for first in map(int, argv[4:]):
            seeds = [str(s) for s in range(first, first + 6)]
            worlds = [lab(lab_eval, "instance", "projectile-target", s) for s in seeds]
            hist = {}
            for _ in range(n):
                with open(path, "w") as f:
                    json.dump(mixed_search.launcher(rnd, worlds[0]["rules"]), f)
                k = lab(lab_eval, "accept", path, "projectile-target", *seeds).get("succeeded")
                if isinstance(k, bool) or not isinstance(k, int):
                    sys.exit("panel_cap: evaluator printed no success count; refusing a partial histogram")
                hist[k] = hist.get(k, 0) + 1
            rows.append({"seeds": [first, first + 5],
                         "targets": [round(w["target_x"], 2) for w in worlds],
                         "best": max(hist) if hist else None,
                         "histogram": {str(k): v for k, v in sorted(hist.items())}})
    json.dump({"designs_per_panel": n, "panels": rows}, sys.stdout, indent=1)
    sys.stdout.write("\n")


if __name__ == "__main__":
    main(sys.argv)
