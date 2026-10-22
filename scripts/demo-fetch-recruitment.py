#!/usr/bin/env python3
"""Exercise actual Rust journals and three separate signed uAgent transport processes.

Synthetic advisory tasks only. No Agentverse registration, API keys or public HTTP.
Run with the Python environment installed from services/fetch-bridge, after
cargo build -p genesis-fetch. This is local protocol evidence, not cloud proof.
"""
from __future__ import annotations
import argparse
import datetime as dt
import json
import os
from pathlib import Path
import socket
import subprocess
import sys
import tempfile
import time
import urllib.request
import uuid

ROOT = Path(__file__).resolve().parents[1]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--rust-bin", type=Path, default=ROOT / "target/debug/genesis-fetch")
    parser.add_argument("--output", type=Path)
    args = parser.parse_args()
    from uagents_core.identity import Identity
    token = "genesis-local-demo-token-not-a-production-credential"
    seeds = {name: f"genesis-public-local-demo-seed-{name}-000000" for name in ("a", "b", "specialist")}
    addresses = {name: Identity.from_seed(seed, 0).address for name, seed in seeds.items()}
    ports = {}
    for name in seeds:
        with socket.socket() as sock:
            sock.bind(("127.0.0.1", 0))
            ports[name] = sock.getsockname()[1]
    if len(set(ports.values())) != 3:
        raise RuntimeError("ephemeral port collision; rerun demo")
    processes = {}
    logs = []
    with tempfile.TemporaryDirectory(prefix="genesis-fetch-demo-") as temporary:
        directory = Path(temporary)
        base_env = dict(os.environ, GENESIS_FETCH_BRIDGE_TOKEN=token, PYTHONPATH=str(ROOT / "services/fetch-bridge"))
        for name in seeds:
            peer_names = ("a", "b") if name == "specialist" else ("specialist",)
            (directory / f"{name}-peers.json").write_text(json.dumps({addresses[p]: f"http://127.0.0.1:{ports[p]}/submit" for p in peer_names}))

        def start(name):
            log = (directory / f"{name}.log").open("ab")
            logs.append(log)
            command = [sys.executable, "-m", "genesis_fetch_bridge", "serve", "--db", str(directory / f"{name}-bridge.sqlite"), "--peers", str(directory / f"{name}-peers.json"), "--port", str(ports[name])]
            if name == "specialist":
                command.append("--specialist")
            processes[name] = subprocess.Popen(command, env=dict(base_env, GENESIS_FETCH_SEED=seeds[name]), stdout=log, stderr=log)
            until = time.monotonic() + 15
            while time.monotonic() < until:
                if processes[name].poll() is not None:
                    raise RuntimeError(f"{name} bridge exited before readiness; inspect isolated local logs")
                try:
                    req = urllib.request.Request(f"http://127.0.0.1:{ports[name]}/v1/identity", headers={"Authorization": f"Bearer {token}"})
                    with urllib.request.urlopen(req, timeout=0.5) as response:
                        identity = json.load(response)
                    assert identity["address"] == addresses[name]
                    return
                except (OSError, TimeoutError):
                    time.sleep(0.05)
            raise RuntimeError(f"{name} bridge did not become ready")

        def stop(name):
            process = processes.pop(name, None)
            if process:
                process.terminate()
                try:
                    process.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    process.kill()
                    process.wait(timeout=5)

        def rust(org, command, *extra):
            result = subprocess.run([str(args.rust_bin), command, "--db", str(directory / f"{org}-runtime.sqlite"), "--org", f"org_{org}", "--bridge-address", addresses[org], *map(str, extra)], env=base_env, capture_output=True, text=True, timeout=20)
            if result.returncode:
                raise RuntimeError(f"Rust {command} failed: {result.stderr[:2000]}")
            return json.loads(result.stdout)

        def status(org):
            return rust(org, "status", "--contract", f"contract_{org}")

        def exchange(org):
            return rust(org, "exchange", "--bridge-url", f"http://127.0.0.1:{ports[org]}/v1")

        def wait(org, predicate):
            until = time.monotonic() + 20
            while time.monotonic() < until:
                exchange(org)
                current = status(org)
                if predicate(current):
                    return current
                time.sleep(0.05)
            raise RuntimeError(f"{org} timed out waiting for native contract evidence")

        expires = (dt.datetime.now(dt.timezone.utc) + dt.timedelta(minutes=10)).isoformat().replace("+00:00", "Z")

        def prepare(org, body):
            message = {"schema_version": 1, "message_id": str(uuid.uuid4()), "org_id": f"org_{org}", "mission_id": f"mission_{org}", "contract_id": f"contract_{org}", "correlation_id": f"correlation_{org}", "expires_at": expires, "body": body}
            path = directory / f"{message['message_id']}.json"
            path.write_text(json.dumps(message))
            rust(org, "prepare", "--peer", addresses["specialist"], "--message", path)
            return message

        def task(org, number):
            return prepare(org, {"kind": "task_request", "task": {"job_id": f"job_{org}_{number}", "attempt_id": f"attempt_{org}_{number}", "role_id": f"logistics_{org}", "assignment_id": f"assignment_{org}", "idempotency_key": f"event_{org}_{number}", "epoch": 1, "deadline": expires, "input": {"kind": "synthetic_transport_advice", "attendees": 120, "event": number}}})

        try:
            for name in seeds:
                start(name)
            for org in ("a", "b"):
                prepare(org, {"kind": "recruit_request", "requirement": "Suggest transport options after a simulated venue closure", "capabilities": ["logistics.advice"], "lease_expires_at": expires})
                offered = wait(org, lambda c: c["phase"] == "offered")
                prepare(org, {"kind": "accept_offer", "offer_message_id": offered["offer_message_id"], "offer_id": offered["offer_id"]})
                wait(org, lambda c: c["phase"] == "active")
                task(org, 1)
                wait(org, lambda c: len(c["tasks"]) == 1 and c["tasks"][0]["phase"] == "collected")
            # Fresh processes retain the exact SDK identity and SQLite evidence.
            stop("specialist")
            stop("a")
            start("specialist")
            start("a")
            prepare("a", {"kind": "release_contract", "reason": "Relocation responsibility handed back"})
            released = wait("a", lambda c: c["native_release_acknowledged"])
            task("b", 2)
            active = wait("b", lambda c: len(c["tasks"]) == 2 and all(t["phase"] == "collected" for t in c["tasks"]))
            assert released["phase"] == "released" and active["phase"] == "active"
            assert not released["domain_verified"] and not active["domain_verified"]
            report = {"mode": "local_signed_sdk_transport", "separate_python_processes": 3, "specialist_address": addresses["specialist"], "org_a": released, "org_b": active, "specialist_and_org_a_restarted": True, "public_registration": False, "agentverse_discovery_smoke": False, "kernel_role_activation": False, "semantic_verification": False}
            if args.output:
                args.output.write_text(json.dumps(report, indent=2) + "\n")
            print(json.dumps(report, indent=2))
        finally:
            for name in list(processes):
                stop(name)
            for log in logs:
                log.close()


if __name__ == "__main__":
    main()
