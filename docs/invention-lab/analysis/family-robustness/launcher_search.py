#!/usr/bin/env python3
"""Is projectile-target solvable, and does one launcher fit many instances?

    launcher_search.py <lab-eval> <rng-seed> <designs-per-instance> <instance-seed>...

For each instance seed: random single-arm launchers (steel base, motorized arm,
payload on the arm), each scored once with `lab-eval experiment`. Then the
first hit for each instance is scored on public seeds 20000..20039 with
`lab-eval accept`. Only the trusted evaluator runs. Prints JSON.
"""
import json, os, random, subprocess, sys, tempfile


def launcher(rnd):
    length, height = rnd.uniform(0.8, 4.0), rnd.uniform(0.4, 2.0)
    x, angle = rnd.uniform(-5.5, -0.4), rnd.uniform(-1.2, 1.2)
    speed = rnd.choice([-1, 1]) * rnd.uniform(2, 10)
    return {"language": 1, "payload_on": "arm", "bodies": [
        {"shape": "rect", "id": "base", "x": x, "y": height / 2, "w": 0.6, "h": height,
         "material": "steel"},
        {"shape": "rect", "id": "arm", "x": x, "y": height * 0.9, "w": length, "h": 0.12,
         "angle": angle, "material": rnd.choice(["wood", "steel"])}],
        "joints": [{"a": "base", "b": "arm", "kind": "hinge", "x": x, "y": height * 0.9,
                    "motor_speed": speed}]}


def run(lab_eval, *args):
    return json.loads(subprocess.run([lab_eval, *args], capture_output=True, text=True,
                                     check=True).stdout)


def main(argv):
    lab_eval, rnd, per = argv[1], random.Random(int(argv[2])), int(argv[3])
    panel = [str(s) for s in range(20000, 20040)]
    out = []
    with tempfile.TemporaryDirectory() as tmp:
        path = os.path.join(tmp, "design.json")
        for seed in argv[4:]:
            valid, hits = 0, []
            for _ in range(per):
                design = launcher(rnd)
                with open(path, "w") as f:
                    json.dump(design, f)
                receipt = run(lab_eval, "experiment", path, "projectile-target", seed, "1200")["receipt"]
                if receipt.get("outcome") != "scored":
                    continue
                valid += 1
                if receipt["metrics"]["success"]:
                    hits.append(design)
            row = {"instance": int(seed), "tried": per, "valid": valid, "hits": len(hits)}
            if hits:
                with open(path, "w") as f:
                    json.dump(hits[0], f)
                row["first_hit_on_panel"] = run(lab_eval, "accept", path, "projectile-target",
                                                *panel)["succeeded"]
            out.append(row)
    json.dump({"panel": [20000, 20039], "rows": out}, sys.stdout, indent=1)
    sys.stdout.write("\n")


if __name__ == "__main__":
    main(sys.argv)
