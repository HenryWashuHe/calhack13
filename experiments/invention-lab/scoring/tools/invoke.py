#!/usr/bin/env python3
"""Run an adopted method once under its invocation grant.

    invoke.py prepare --org URL --key KEY --request REQ.json --plan PLAN.json --out DIR
    invoke.py run DIR --package PKG.json --lab-exec PATH --sandbox namespaces|none
    invoke.py score DIR --lab-score PATH [--send-output]

`prepare` asks the receiving organization to freeze the proposed one-slot
invocation plan (POST /method-runs) and keeps its record in DIR, which must not
exist. `run` starts that record once (POST /method-runs/{id}/start); only if
the server agrees the grant is still current does it run `lab-exec run` on the
plan the server froze, then posts the execution receipt. A refused start runs
nothing. A run that was already started is never started again: a lost runner
is recovered with `lab-exec recover` and its receipt posted by hand.

`score` runs `lab-score score` once on a completed run's frozen plan and
receipt, then posts the evaluation (POST /method-runs/{id}/result). The server
decides whether the scored output is attached to the responsibility; the
score never stands in for an acceptance or a comparison. With --send-output it
also sends the scored output's bytes, which an organization that rescores
results requires: it reruns its own pinned scorer and refuses an evaluation
that doesn't reproduce.

The operator token is read from $GENESIS_OPERATOR_TOKEN, never from argv.
Standard library only.
"""
import argparse
import hashlib
import json
import os
import subprocess
import sys
import urllib.error
import urllib.request

TOKEN = "GENESIS_OPERATOR_TOKEN"


def die(msg):
    sys.exit(f"invoke: {msg}")


def call(url, body):
    token = os.environ.get(TOKEN)
    if not token:
        die(f"set {TOKEN}")
    data = json.dumps(body).encode()
    req = urllib.request.Request(url, data=data, method="POST", headers={
        "authorization": f"Bearer {token}", "content-type": "application/json"})
    try:
        with urllib.request.urlopen(req, timeout=60) as r:
            return r.status, json.loads(r.read(1 << 22) or b"null")
    except urllib.error.HTTPError as e:
        try:
            return e.code, json.loads(e.read(1 << 20) or b"null")
        except ValueError:
            return e.code, None


def write_new(path, value):
    with open(path, "x") as f:
        json.dump(value, f, indent=2, sort_keys=True)


def load(path):
    with open(path) as f:
        return json.load(f)


def prepare(a):
    os.mkdir(a.out)
    body = {"client_request_id": a.key, "request": load(a.request), "plan": load(a.plan)}
    status, record = call(f"{a.org.rstrip('/')}/method-runs", body)
    if status not in (200, 201) or not isinstance(record, dict):
        write_new(os.path.join(a.out, "prepare-refused.json"), {"status": status, "body": record})
        die(f"prepare refused ({status})")
    write_new(os.path.join(a.out, "org.json"), {"url": a.org.rstrip("/")})
    write_new(os.path.join(a.out, "run.json"), record)
    print(json.dumps({"id": record["id"], "status": record["status"]}))


def run(a):
    record = load(os.path.join(a.dir, "run.json"))
    org = load(os.path.join(a.dir, "org.json"))["url"]
    for name in ("started.json", "start-refused.json", "plan.json", "state"):
        if os.path.exists(os.path.join(a.dir, name)):
            die(f"{name} exists: this run was already started or refused; never start twice")
    plan = record["plan"]
    [slot] = plan["slots"]
    with open(a.package, "rb") as f:
        package = f.read()
    if hashlib.sha256(package).hexdigest() != slot["method"]["sha256"]:
        die("the package is not the method the frozen plan names")
    status, started = call(f"{org}/method-runs/{record['id']}/start", None)
    if status != 200 or not isinstance(started, dict) or started.get("status") != "started":
        write_new(os.path.join(a.dir, "start-refused.json"), {"status": status, "body": started})
        die(f"start refused ({status}); nothing ran")
    write_new(os.path.join(a.dir, "started.json"), started)
    # Run exactly the plan the server froze, not the operator's proposal.
    write_new(os.path.join(a.dir, "plan.json"), started["plan"])
    state = os.path.join(a.dir, "state")
    with open(os.path.join(a.dir, "run.out.json"), "x") as out, \
            open(os.path.join(a.dir, "run.err"), "x") as err:
        code = subprocess.run([a.lab_exec, "run", os.path.join(a.dir, "plan.json"),
                               slot["slot_id"], a.package, state, a.sandbox],
                              stdout=out, stderr=err).returncode
    receipt_path = os.path.join(state, "receipt.json")
    if not os.path.exists(receipt_path):
        die(f"lab-exec exited {code} without a receipt; recover with lab-exec recover")
    status, done = call(f"{org}/method-runs/{record['id']}/receipt", load(receipt_path))
    write_new(os.path.join(a.dir, "completed.json"), {"status": status, "body": done})
    if status != 200:
        die(f"receipt refused ({status})")
    print(json.dumps({"id": record["id"], "status": done["status"],
                      "outcome": done["receipt"]["outcome"]}))


def score(a):
    completed = os.path.join(a.dir, "completed.json")
    if not os.path.exists(completed):
        die("this run has no accepted receipt; nothing to score")
    if load(completed).get("status") != 200:
        die("this run's receipt was refused; nothing to score")
    for name in ("score", "result.json", "result-refused.json"):
        if os.path.exists(os.path.join(a.dir, name)):
            die(f"{name} exists: this run was already scored; never score twice")
    record = load(os.path.join(a.dir, "run.json"))
    org = load(os.path.join(a.dir, "org.json"))["url"]
    [slot] = record["plan"]["slots"]
    out = os.path.join(a.dir, "score")
    with open(os.path.join(a.dir, "score.out.json"), "x") as so, \
            open(os.path.join(a.dir, "score.err"), "x") as se:
        code = subprocess.run([a.lab_score, "score", os.path.join(a.dir, "plan.json"),
                               slot["slot_id"], os.path.join(a.dir, "state"), out],
                              stdout=so, stderr=se).returncode
    evaluation = os.path.join(out, "evaluation.json")
    if not os.path.exists(evaluation):
        die(f"lab-score exited {code} without an evaluation; nothing posted")
    body = load(evaluation)
    if a.send_output:
        state = os.path.join(a.dir, "state")
        outputs = load(os.path.join(state, "receipt.json")).get("outputs") or []
        if not outputs:
            die("the receipt names no output to send; nothing posted")
        with open(os.path.join(state, "artifacts", outputs[0]["sha256"]), "rb") as f:
            body = {"evaluation": body, "output_hex": f.read().hex()}
    status, done = call(f"{org}/method-runs/{record['id']}/result", body)
    if status != 200:
        write_new(os.path.join(a.dir, "result-refused.json"), {"status": status, "body": done})
        die(f"result refused ({status})")
    write_new(os.path.join(a.dir, "result.json"), done)
    print(json.dumps({"id": record["id"], "attachment": done["result"]["attachment"]}))


def main(argv=None):
    p = argparse.ArgumentParser(prog="invoke.py")
    sub = p.add_subparsers(dest="cmd", required=True)
    pr = sub.add_parser("prepare")
    for k in ("--org", "--key", "--request", "--plan", "--out"):
        pr.add_argument(k, required=True)
    r = sub.add_parser("run")
    r.add_argument("dir")
    r.add_argument("--package", required=True)
    r.add_argument("--lab-exec", required=True)
    r.add_argument("--sandbox", choices=("namespaces", "none"), required=True)
    sc = sub.add_parser("score")
    sc.add_argument("dir")
    sc.add_argument("--lab-score", required=True)
    sc.add_argument("--send-output", action="store_true",
                    help="also send the scored output, for an organization that rescores")
    a = p.parse_args(argv)
    {"prepare": prepare, "run": run, "score": score}[a.cmd](a)


if __name__ == "__main__":
    main()
