"""Controlled request-backlog regression; no namespace or memory-metering claim.

Usage: python3 tests/review_request_backlog.py /absolute/path/to/lab-exec
The fixture writes at most 32 MiB, never reads replies, then writes a marker.
With bounded request/reply queues, pipe backpressure must prevent that marker.
Mode none is intentional; the outer supervisor cleans up the process group.
"""
import json
import os
from pathlib import Path
import signal
import subprocess
import sys
import tempfile


def main(binary):
    binary = str(Path(binary).resolve())
    with tempfile.TemporaryDirectory(prefix="genesis-request-backlog-") as temp:
        root = Path(temp)
        source = root / "source"
        source.mkdir()
        marker = root / "all-requests-drained"
        (source / "main.py").write_text(
            "import json, sys, time\n"
            "from pathlib import Path\n"
            "line = json.dumps({'op':'instances','padding':'x'*65500})+'\\n'\n"
            "for _ in range(512):\n"
            "    sys.stdout.write(line); sys.stdout.flush()\n"
            f"Path({str(marker)!r}).write_text('all 512 requests left the child')\n"
            "time.sleep(30)\n"
        )

        def cli(*args):
            result = subprocess.run(
                [binary, *map(str, args)], capture_output=True, text=True,
                timeout=10, check=True,
            )
            return json.loads(result.stdout)

        package = root / "method.json"
        method = cli("package", source, "review-request-backlog", "1", "main.py", package)
        plan_dir = root / "plan"
        cli("example-plan", "none", 1, 10000, 1500, plan_dir)
        plan_path = plan_dir / "plan.json"
        plan = json.loads(plan_path.read_text())
        plan["slots"][0]["method"] = method
        plan_path.write_text(json.dumps(plan))
        state = root / "state"
        proc = subprocess.Popen(
            [binary, "run", str(plan_path), "candidate", str(package), str(state), "none"],
            stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True,
            start_new_session=True,
        )
        try:
            stdout, stderr = proc.communicate(timeout=4)
        finally:
            try:
                os.killpg(proc.pid, signal.SIGKILL)
            except ProcessLookupError:
                pass
            proc.communicate(timeout=3)
        assert proc.returncode == 0, stderr
        assert json.loads(stdout)["outcome"] == "interrupted"
        cli("check", plan_path, "candidate", state)
        assert not marker.exists(), (
            "reader drained all 512 padded requests (about 32 MiB) while replies "
            "were blocked; the parent request backlog has no bounded backpressure"
        )
        print("PASS: bounded request backlog applies pipe backpressure")


if __name__ == "__main__":
    main(sys.argv[1])
