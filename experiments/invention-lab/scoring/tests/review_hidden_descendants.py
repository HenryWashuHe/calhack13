"""Bounded local Mode::None closure regression; no Linux namespace claim.

Usage: python3 review_hidden_descendants.py /path/lab-exec /path/lab-score
The submitted method exits after starting a child that closes inherited pipes.
All processes belong to supervised fixture groups and are killed in finally.
"""
import json
import os
from pathlib import Path
import signal
import subprocess
import sys
import tempfile
import time


def main(executor, scorer):
    executor, scorer = str(Path(executor).resolve()), str(Path(scorer).resolve())
    groups = []
    with tempfile.TemporaryDirectory(prefix="genesis-hidden-descendants-") as temp:
        root = Path(temp)
        marker = root / "heartbeat.json"
        design = Path(__file__).resolve().parents[2] / "evaluator/designs/reference-five-wheel.json"
        source = (
            "import json, os, sys, time\nfrom pathlib import Path\n"
            f"design = json.loads({design.read_text()!r})\n"
            "print(json.dumps({'op':'submit','design':design}), flush=True)\n"
            "assert json.loads(sys.stdin.readline())['ok']\n"
            "if os.fork() == 0:\n"
            "    for fd in (0,1,2):\n"
            "        os.close(fd)\n"
            f"    marker = Path({str(marker)!r})\n"
            "    for i in range(3000):\n"
            "        marker.write_text(json.dumps({'pid':os.getpid(),'counter':i}))\n"
            "        time.sleep(0.01)\n"
            "    os._exit(0)\n"
        )

        def cli(binary, *args):
            p = subprocess.run([binary, *map(str, args)], capture_output=True, text=True,
                               timeout=10, check=True)
            return json.loads(p.stdout)

        def read_marker():
            end = time.monotonic() + 2
            while time.monotonic() < end:
                try:
                    return json.loads(marker.read_text())
                except (OSError, ValueError):
                    time.sleep(0.01)
            raise AssertionError("heartbeat was not readable")

        try:
            packages = {}
            methods = {}
            for slot, text in [("candidate", source), ("control", "pass\n")]:
                src = root / (slot + "-source")
                src.mkdir()
                (src / "method.py").write_text(text)
                packages[slot] = root / (slot + "-package.json")
                methods[slot] = cli(executor, "package", src, "review-" + slot, "1",
                                    "method.py", packages[slot])
            base_dir = root / "base"
            cli(executor, "example-plan", "none", 1, 2400, 5000, base_dir)
            base_path = base_dir / "plan.json"
            plan = json.loads(base_path.read_text())
            for slot in plan["slots"]:
                slot["method"] = methods[slot["slot_id"]]
            base_path.write_text(json.dumps(plan))
            salt = root / "salt.txt"
            salt.write_text("independent-descendant-review-salt")
            comparison = root / "comparison"
            cli(scorer, "comparison-plan", base_path, "descendant-review", 1, salt, comparison)
            plan_path = comparison / "plan.json"
            for slot in ("candidate", "control"):
                p = subprocess.Popen([executor, "run", str(plan_path), slot,
                                      str(packages[slot]), str(root / slot), "none"],
                                     stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True,
                                     start_new_session=True)
                groups.append(p)
                stdout, stderr = p.communicate(timeout=6)
                assert p.returncode == 0, stderr
                if slot == "candidate":
                    assert json.loads(stdout)["outcome"] == "output_generated"
            state = json.loads((root / "candidate/run.json").read_text())
            assert state["finished"] and state["terminated"], state
            p = subprocess.run([scorer, "score-hidden", str(plan_path), "candidate",
                                str(comparison / "descriptor.json"), str(salt), str(root / "score"),
                                "candidate=" + str(root / "candidate"),
                                "control=" + str(root / "control")],
                               capture_output=True, text=True, timeout=6)
            before = read_marker()
            time.sleep(0.05)
            after = read_marker()
            assert after["counter"] > before["counter"], "descendant must still be running after scoring"
            assert p.returncode != 0, (
                "hidden scoring accepted parent-only termination while its descendant still ran: "
                + p.stdout.strip()
            )
            print("PASS: hidden scoring refuses an incompletely terminated process tree")
        finally:
            for p in groups:
                try:
                    os.killpg(p.pid, signal.SIGKILL)
                except ProcessLookupError:
                    pass
                p.communicate(timeout=3)


if __name__ == "__main__":
    main(*sys.argv[1:])
