#!/usr/bin/env python3
"""Offline calibration of the hand-written controls: how often does each one's
submitted design succeed on its own development panel?

    calibrate.py <lab-eval> <method.py> <family> <first-seed> <panels> [grant]

Runs <method.py> (a hand-written control only, never generated source) against
an emulation of the executor's line protocol: `instances`, `experiment`
(answered by `lab-eval experiment`, with a full episode reserved per call) and
`submit`. Each panel is 6 consecutive public seeds starting at first-seed,
first-seed+6, and so on. The submitted design is then scored on its own panel
with `lab-eval accept`, as a development trial scores it. This is not the
executor, records nothing and is not trial evidence. Prints JSON.
"""
import json
import os
import subprocess
import sys
import tempfile

STEPS = {"projectile-target": 1200}
DEFAULT_STEPS = 2400


def lab(lab_eval, *args):
    done = subprocess.run([lab_eval, *args], capture_output=True, text=True)
    return json.loads(done.stdout) if done.returncode == 0 else None


def panel_run(lab_eval, method, family, seeds, grant, tmp):
    episode = STEPS.get(family, DEFAULT_STEPS)
    instances = [{"id": f"{family}/1/{s}", "max_steps": episode,
                  "world": lab(lab_eval, "instance", family, str(s))} for s in seeds]
    remaining = grant
    design_path = os.path.join(tmp, "design.json")
    proc = subprocess.Popen([sys.executable, "-I", method], stdin=subprocess.PIPE,
                            stdout=subprocess.PIPE, text=True)
    submitted, experiments = None, 0
    for line in proc.stdout:
        req = json.loads(line)
        if req["op"] == "instances":
            reply = {"ok": True, "instances": instances, "remaining_steps": remaining}
        elif req["op"] == "experiment":
            inst = next((i for i in instances if i["id"] == req["instance"]), None)
            if inst is None or remaining < episode:
                reply = {"ok": False, "refused": "no reservation"}
            else:
                with open(design_path, "w") as f:
                    json.dump(req["design"], f)
                receipt = (lab(lab_eval, "experiment", design_path, family,
                               inst["id"].rsplit("/", 1)[1], str(episode)) or {}).get("receipt", {})
                steps = receipt.get("simulation_steps") or 0
                remaining -= steps
                experiments += 1
                reply = {"ok": True, "outcome": receipt.get("outcome"),
                         "metrics": receipt.get("metrics"), "reason": receipt.get("reason"),
                         "steps": steps, "remaining_steps": remaining}
        elif req["op"] == "submit":
            submitted = req["design"]
            reply = {"ok": True}
        else:
            reply = {"ok": False, "refused": "unknown op"}
        proc.stdin.write(json.dumps(reply) + "\n")
        proc.stdin.flush()
        if submitted is not None:
            break
    proc.stdin.close()
    proc.wait(timeout=60)
    row = {"seeds": [seeds[0], seeds[-1]], "experiments": experiments,
           "steps_used": grant - remaining, "successes": None}
    if submitted is not None:
        with open(design_path, "w") as f:
            json.dump(submitted, f)
        scored = lab(lab_eval, "accept", design_path, family, *map(str, seeds))
        row["successes"] = scored["succeeded"] if scored else None
        row["kind"] = "launcher" if submitted.get("payload_on") == "arm" else "cart"
    return row


def main(argv):
    lab_eval, method, family = argv[1], argv[2], argv[3]
    first, panels = int(argv[4]), int(argv[5])
    grant = int(argv[6]) if len(argv) > 6 else 72000
    rows = []
    with tempfile.TemporaryDirectory() as tmp:
        for p in range(panels):
            seeds = list(range(first + 6 * p, first + 6 * p + 6))
            rows.append(panel_run(lab_eval, method, family, seeds, grant, tmp))
    total = sum(r["successes"] or 0 for r in rows)
    json.dump({"method": os.path.basename(method), "family": family, "grant": grant,
               "panels": rows, "successes": total, "of": 6 * panels}, sys.stdout, indent=1)
    sys.stdout.write("\n")


if __name__ == "__main__":
    main(sys.argv)
