"""Independent wall-limit regression; only runs the controlled fixture below.

Usage: python3 tests/review_backpressure.py /absolute/path/to/lab-exec
Uses mode=none for portability, NOT evidence of namespace enforcement. The
outer supervisor kills the entire fixture process group on failure.
"""
import json
import os
from pathlib import Path
import signal
import subprocess
import sys
import tempfile
import time


def main(binary):
    binary = str(Path(binary).resolve())
    with tempfile.TemporaryDirectory(prefix="genesis-backpressure-") as temp:
        root = Path(temp)
        source = root / "source"
        source.mkdir()
        (source / "main.py").write_text(
            "import sys, time\n"
            "# Bounded requests, no reads: fill the parent's reply pipe.\n"
            "for _ in range(4096):\n"
            "    print('{\"op\":\"instances\"}', flush=True)\n"
            "time.sleep(30)\n"
        )

        def cli(*args):
            result = subprocess.run(
                [binary, *map(str, args)], capture_output=True, text=True,
                timeout=10, check=True,
            )
            return json.loads(result.stdout)

        package = root / "method.json"
        method = cli("package", source, "review-backpressure", "1", "main.py", package)
        plan_dir = root / "plan"
        cli("example-plan", "none", 1, 10000, 500, plan_dir)
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
        timed_out = False
        started = time.monotonic()
        try:
            stdout, stderr = proc.communicate(timeout=3)
        except subprocess.TimeoutExpired:
            timed_out = True
            stdout, stderr = "", ""
        finally:
            # Also clean up descendants if the executor returned prematurely.
            try:
                os.killpg(proc.pid, signal.SIGKILL)
            except ProcessLookupError:
                pass
            proc.communicate(timeout=3)
        elapsed = time.monotonic() - started
        assert not timed_out, (
            f"executor blocked for {elapsed:.2f}s with a 500ms wall grant; "
            f"receipt published: {(state / 'receipt.json').exists()}"
        )
        assert proc.returncode == 0, stderr
        receipt = json.loads(stdout)
        assert receipt["outcome"] == "interrupted", receipt
        assert (state / "receipt.json").is_file()
        cli("check", plan_path, "candidate", state)
        print("PASS: response backpressure respects the wall grant and retains a receipt")


if __name__ == "__main__":
    main(sys.argv[1])
