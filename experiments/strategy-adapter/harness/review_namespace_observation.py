"""Pure controller observations: no process, method or namespace is started."""
from contextlib import ExitStack
from pathlib import Path
import signal
import sys
import tempfile
import unittest
from unittest.mock import Mock, patch

sys.path.insert(0, str(Path(__file__).resolve().parent))
import replay


class NamespaceObservation(unittest.TestCase):
    def test_only_successful_natural_launcher_exit_confirms_closure(self):
        for status, expected in ((0, True), (-signal.SIGKILL, False), (1, False)):
            with self.subTest(status=status):
                process = Mock(pid=12345)
                process.poll.return_value = status
                with patch.object(replay, "_namespace_inits") as lookup, \
                        patch.object(replay.os, "kill") as kill:
                    self.assertEqual(replay._end_namespace(process), expected)
                lookup.assert_not_called()
                kill.assert_not_called()

    def test_observed_namespace_init_signal_then_launcher_exit_confirms(self):
        process = Mock(pid=12345)
        process.poll.side_effect = [None, -signal.SIGKILL]
        with patch.object(replay, "_namespace_inits", return_value=[12346]), \
                patch.object(replay, "_in_other_pid_namespace", return_value=True), \
                patch.object(replay.os, "kill") as kill, \
                patch.object(replay.time, "sleep"):
            self.assertTrue(replay._end_namespace(process))
        kill.assert_called_once_with(12346, signal.SIGKILL)

    def test_failed_signal_or_foreign_child_cannot_certify_nonzero_exit(self):
        for other, error in ((False, None), (True, PermissionError()),
                             (True, ProcessLookupError())):
            with self.subTest(other=other, error=error):
                process = Mock(pid=12345)
                process.poll.side_effect = [None, -signal.SIGKILL]
                with patch.object(replay, "_namespace_inits", return_value=[12346]), \
                        patch.object(replay, "_in_other_pid_namespace", return_value=other), \
                        patch.object(replay.os, "kill", side_effect=error) as kill, \
                        patch.object(replay.time, "sleep"):
                    self.assertFalse(replay._end_namespace(process))
                if not other:
                    kill.assert_not_called()

    def test_signal_without_launcher_exit_is_still_unconfirmed(self):
        process = Mock(pid=12345)
        process.poll.return_value = None
        with patch.object(replay, "_namespace_inits", return_value=[12346]), \
                patch.object(replay, "_in_other_pid_namespace", return_value=True), \
                patch.object(replay.os, "kill"), \
                patch.object(replay.time, "monotonic", side_effect=[0, 11]):
            self.assertFalse(replay._end_namespace(process, wait=10))

    def test_unconfirmed_exit_raises_after_group_cleanup_and_stage_removal(self):
        process = Mock(pid=12345, returncode=-signal.SIGKILL)
        process.poll.return_value = -signal.SIGKILL
        process.stdout.fileno.return_value = 10
        process.stdin.fileno.return_value = 11
        with tempfile.TemporaryDirectory() as root, ExitStack() as stack:
            stage = Path(root) / "stage"
            stage.mkdir()
            stack.enter_context(patch.object(replay, "_launch", return_value=(["inert"], "/", {}, stage)))
            stack.enter_context(patch.object(replay.subprocess, "Popen", return_value=process))
            stack.enter_context(patch.object(replay.os, "set_blocking"))
            stack.enter_context(patch.object(replay.select, "select", return_value=([10], [], [])))
            stack.enter_context(patch.object(replay.os, "read", return_value=b""))
            stop = stack.enter_context(patch.object(replay, "_stop"))
            with self.assertRaisesRegex(replay.Stopped, "not observed to end"):
                replay.run_method("unused", Mock(), "namespaces")
            stop.assert_called_once_with(process)
            self.assertFalse(stage.exists())


if __name__ == "__main__":
    unittest.main()
