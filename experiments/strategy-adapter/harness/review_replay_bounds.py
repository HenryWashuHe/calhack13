"""Independent #180 controller guards. No process or method is launched.

Run explicitly: python3 -m unittest discover -s
experiments/strategy-adapter/harness -p review_replay_bounds.py
"""
from contextlib import ExitStack
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import Mock, patch

sys.path.insert(0, str(Path(__file__).resolve().parent))
import replay


class ReviewController(unittest.TestCase):
    def test_staging_failure_removes_the_new_stage_before_any_process(self):
        with tempfile.TemporaryDirectory() as root:
            stage_root = Path(root) / "stages"
            method = Path(root) / "method"
            method.mkdir()
            with patch.object(replay, "STAGE_ROOT", stage_root), \
                    patch.object(replay.shutil, "copytree", side_effect=OSError("copy failed")), \
                    patch.object(replay.subprocess, "Popen") as launch:
                with self.assertRaisesRegex(OSError, "copy failed"):
                    replay.run_method(method, Mock(), "namespaces")
            launch.assert_not_called()
            self.assertEqual(list(stage_root.iterdir()), [], "failed preparation must not leave its stage")

    def test_deadline_is_rechecked_between_buffered_requests(self):
        clock = [0.0]
        seen = []

        class Oracle:
            def reply(self, request):
                seen.append(request)
                clock[0] = 2.0  # the first response consumes the remaining deadline
                return {"ok": True}

        process = Mock(pid=12345)
        process.stdout.fileno.return_value = 10
        process.stdin.fileno.return_value = 11
        with ExitStack() as stack:
            stack.enter_context(patch.object(replay, "_launch", return_value=(["inert"], "/", {}, None)))
            stack.enter_context(patch.object(replay.subprocess, "Popen", return_value=process))
            stack.enter_context(patch.object(replay.os, "set_blocking"))
            stack.enter_context(patch.object(replay.time, "monotonic", side_effect=lambda: clock[0]))
            stack.enter_context(patch.object(replay.select, "select", return_value=([10], [], [])))
            stack.enter_context(patch.object(replay.os, "read", return_value=b'{"op":"first"}\n{"op":"second"}\n'))
            stop = stack.enter_context(patch.object(replay, "_stop"))
            with self.assertRaises(replay.Stopped):
                replay.run_method("unused", Oracle(), "host", timeout=1)
            stop.assert_called_once_with(process)
        self.assertEqual(seen, [{"op": "first"}], "no next request should be dispatched after the deadline")

    def test_stage_cleanup_still_runs_when_process_stop_reports_failure(self):
        process = Mock(pid=12345)
        with tempfile.TemporaryDirectory() as root, ExitStack() as stack:
            stage = Path(root) / "stage"
            stage.mkdir()
            stack.enter_context(patch.object(replay, "_launch", return_value=(["inert"], "/", {}, stage)))
            stack.enter_context(patch.object(replay.subprocess, "Popen", return_value=process))
            stack.enter_context(patch.object(replay.os, "set_blocking", side_effect=OSError("pipe error")))
            stack.enter_context(patch.object(replay, "_stop", side_effect=replay.Stopped("stop failed")))
            with self.assertRaises(replay.Stopped):
                replay.run_method("unused", Mock(), "namespaces")
            self.assertFalse(stage.exists(), "cleanup failure must not skip removal of the stage")


if __name__ == "__main__":
    unittest.main()
