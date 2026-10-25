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
import queue
import subprocess
import sys
import tempfile
import threading
import time

STEPS = {"projectile-target": 1200}
DEFAULT_STEPS = 2400
# Bounds: one evaluator call, and one whole panel (control plus evaluator).
EVALUATOR_TIMEOUT_S = 120
PANEL_DEADLINE_S = 900


class Unavailable(Exception):
    """The panel's measurement is unavailable: never counted as a failure."""


# The running panel's deadline (time.monotonic), if any: every evaluator call
# made for that panel ends by it.
_panel_deadline = None


def lab(lab_eval, *args):
    """One evaluator call's JSON; Unavailable on a failed, slow or malformed call."""
    timeout = EVALUATOR_TIMEOUT_S
    if _panel_deadline is not None:
        timeout = min(timeout, _panel_deadline - time.monotonic())
        if timeout <= 0:
            raise Unavailable("panel deadline passed")
    try:
        done = subprocess.run([lab_eval, *args], capture_output=True, text=True,
                              timeout=timeout)
    except subprocess.TimeoutExpired:
        raise Unavailable(f"evaluator timed out: {args[0]}")
    if done.returncode != 0:
        raise Unavailable(f"evaluator exited {done.returncode}: {args[0]}")
    try:
        return json.loads(done.stdout)
    except ValueError:
        raise Unavailable(f"evaluator output is not JSON: {args[0]}")


MEASURED = {"scored", "invalid"}


def acceptance_count(scored, seeds):
    """Successes from a `lab-eval accept` result, checked against its receipts.

    Every declared seed needs exactly one receipt with a measured outcome:
    `scored` (with a boolean success metric) or `invalid` (a design the rules
    refuse, which is a measured failure). A failed or unknown receipt, a
    missing or extra seed, or a count that doesn't match the receipts makes
    the measurement Unavailable rather than a zero.
    """
    if not isinstance(scored, dict) or not isinstance(scored.get("receipts"), list):
        raise Unavailable("acceptance output has no receipts")
    receipts = scored["receipts"]
    if [r.get("seed") if isinstance(r, dict) else None for r in receipts] != list(seeds):
        raise Unavailable("acceptance receipts are not the declared seeds")
    successes = 0
    for r in receipts:
        outcome = r.get("outcome")
        if not isinstance(outcome, str) or outcome not in MEASURED:
            raise Unavailable(f"acceptance receipt for seed {r.get('seed')} is {outcome!r}")
        if outcome == "scored":
            metrics = r.get("metrics")
            if not isinstance(metrics, dict) or not isinstance(metrics.get("success"), bool):
                raise Unavailable(f"scored receipt for seed {r.get('seed')} has no success metric")
            successes += metrics["success"]
    count, of = scored.get("succeeded"), scored.get("of")
    if isinstance(count, bool) or count != successes or of != len(seeds) or isinstance(of, bool):
        raise Unavailable("acceptance count does not match its receipts")
    return successes


def charged_steps(receipt, episode):
    """The experiment's measured steps, or Unavailable if they can't be trusted."""
    steps = receipt.get("simulation_steps") if isinstance(receipt, dict) else None
    if isinstance(steps, bool) or not isinstance(steps, int) or not 0 <= steps <= episode:
        raise Unavailable(f"experiment receipt has no valid step count: {steps!r}")
    return steps


def lines(stream, out):
    for line in stream:
        out.put(line)
    out.put(None)


def panel_run(lab_eval, method, family, seeds, grant, tmp):
    episode = STEPS.get(family, DEFAULT_STEPS)
    global _panel_deadline
    row = {"seeds": [seeds[0], seeds[-1]], "experiments": 0, "steps_used": 0, "successes": None}
    deadline = time.monotonic() + PANEL_DEADLINE_S
    _panel_deadline = deadline
    proc = None
    try:
        instances = [{"id": f"{family}/1/{s}", "max_steps": episode,
                      "world": lab(lab_eval, "instance", family, str(s))} for s in seeds]
        remaining = grant
        design_path = os.path.join(tmp, "design.json")
        proc = subprocess.Popen([sys.executable, "-I", method], stdin=subprocess.PIPE,
                                stdout=subprocess.PIPE, text=True)
        replies = queue.Queue()
        threading.Thread(target=lines, args=(proc.stdout, replies), daemon=True).start()
        submitted = None
        while submitted is None:
            left = deadline - time.monotonic()
            if left <= 0:
                raise Unavailable("panel deadline passed")
            try:
                line = replies.get(timeout=left)
            except queue.Empty:
                raise Unavailable("panel deadline passed")
            if line is None:
                break
            try:
                req = json.loads(line)
            except ValueError:
                raise Unavailable("control wrote a malformed line")
            if not isinstance(req, dict):
                raise Unavailable("control request is not a JSON object")
            if req.get("op") == "instances":
                reply = {"ok": True, "instances": instances, "remaining_steps": remaining}
            elif req.get("op") == "experiment":
                inst = next((i for i in instances if i["id"] == req.get("instance")), None)
                if inst is None or remaining < episode:
                    reply = {"ok": False, "refused": "no reservation"}
                else:
                    with open(design_path, "w") as f:
                        json.dump(req.get("design"), f)
                    result = lab(lab_eval, "experiment", design_path, family,
                                 inst["id"].rsplit("/", 1)[1], str(episode))
                    receipt = result.get("receipt") if isinstance(result, dict) else None
                    steps = charged_steps(receipt, episode)
                    remaining -= steps
                    row["experiments"] += 1
                    row["steps_used"] = grant - remaining
                    reply = {"ok": True, "outcome": receipt.get("outcome"),
                             "metrics": receipt.get("metrics"), "reason": receipt.get("reason"),
                             "steps": steps, "remaining_steps": remaining}
            elif req.get("op") == "submit":
                submitted = req.get("design")
                reply = {"ok": True}
            else:
                reply = {"ok": False, "refused": "unknown op"}
            proc.stdin.write(json.dumps(reply) + "\n")
            proc.stdin.flush()
        if submitted is None:
            raise Unavailable("control ended without submitting")
        with open(design_path, "w") as f:
            json.dump(submitted, f)
        scored = lab(lab_eval, "accept", design_path, family, *map(str, seeds))
        if time.monotonic() > deadline:
            raise Unavailable("panel deadline passed")
        row["successes"] = acceptance_count(scored, seeds)
        row["kind"] = "launcher" if submitted.get("payload_on") == "arm" else "cart"
    except Unavailable as e:
        row["successes"] = None
        row["unavailable"] = str(e)
    finally:
        _panel_deadline = None
        if proc is not None:
            try:
                proc.stdin.close()
            except OSError:
                pass
            try:
                proc.wait(timeout=5)
            except subprocess.TimeoutExpired:
                proc.kill()
                proc.wait()
    return row


def total(rows):
    """The sum of successes, or None if any panel's measurement is unavailable."""
    counts = [r["successes"] for r in rows]
    return None if any(c is None for c in counts) else sum(counts)


def main(argv):
    lab_eval, method, family = argv[1], argv[2], argv[3]
    first, panels = int(argv[4]), int(argv[5])
    grant = int(argv[6]) if len(argv) > 6 else 72000
    rows = []
    with tempfile.TemporaryDirectory() as tmp:
        for p in range(panels):
            seeds = list(range(first + 6 * p, first + 6 * p + 6))
            rows.append(panel_run(lab_eval, method, family, seeds, grant, tmp))
    total_successes = total(rows)
    json.dump({"method": os.path.basename(method), "family": family, "grant": grant,
               "panels": rows, "successes": total_successes, "of": 6 * panels}, sys.stdout, indent=1)
    sys.stdout.write("\n")


if __name__ == "__main__":
    main(sys.argv)
