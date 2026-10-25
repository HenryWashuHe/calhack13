#!/usr/bin/env python3
"""Best single launcher per 6-instance projectile-target panel.

    panel_cap.py <lab-eval> <rng-seed> <designs> <first-seed>...

For each panel (6 consecutive public seeds from each first-seed), scores
<designs> random launchers from mixed_search.launcher on all six instances
with `lab-eval accept` and reports the histogram of success counts. A trial
scores one submitted design on every panel instance, so the best sampled
count describes what this finite random sample reached; it is not a bound on
what a method that submits a launcher of this shape can reach. Evaluator
processes that fail are omitted from the histogram, so a histogram summing to
<designs> shows no failures occurred. Only the evaluator runs. Prints JSON.
"""
import json, os, random, subprocess, sys, tempfile
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import mixed_search


def main(argv):
    lab_eval, rnd, n = argv[1], random.Random(int(argv[2])), int(argv[3])
    rows = []
    with tempfile.TemporaryDirectory() as tmp:
        path = os.path.join(tmp, "d.json")
        for first in map(int, argv[4:]):
            seeds = [str(s) for s in range(first, first + 6)]
            worlds = [json.loads(subprocess.run([lab_eval, "instance", "projectile-target", s],
                                                capture_output=True, text=True).stdout) for s in seeds]
            hist = {}
            for _ in range(n):
                with open(path, "w") as f:
                    json.dump(mixed_search.launcher(rnd, worlds[0]["rules"]), f)
                done = subprocess.run([lab_eval, "accept", path, "projectile-target", *seeds],
                                      capture_output=True, text=True)
                if done.returncode == 0:
                    k = json.loads(done.stdout)["succeeded"]
                    hist[k] = hist.get(k, 0) + 1
            rows.append({"seeds": [first, first + 5],
                         "targets": [round(w["target_x"], 2) for w in worlds],
                         "best": max(hist) if hist else None,
                         "histogram": {str(k): v for k, v in sorted(hist.items())}})
    json.dump({"designs_per_panel": n, "panels": rows}, sys.stdout, indent=1)
    sys.stdout.write("\n")


if __name__ == "__main__":
    main(sys.argv)
