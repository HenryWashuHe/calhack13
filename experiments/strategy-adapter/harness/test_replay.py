"""Lifecycle tests for replay.run_method, with inert hand-written fixture methods.

Run from the repository root:  python3 -m unittest discover -s experiments/strategy-adapter/harness
Every process started here is a few lines of fixture code written below, run
with isolation `host`; no retained or generated method source is executed.
The namespaces case runs only where the host can create namespaces (root).
"""
import os
import shutil
import sys
import tempfile
import textwrap
import time
import unittest
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import replay  # noqa: E402


class Echo:
    """Answers every request with ok; records what it saw."""

    def __init__(self):
        self.seen = []

    def reply(self, req):
        self.seen.append(req)
        return {'ok': True, 'n': len(self.seen)}


class Boom:
    def reply(self, req):
        raise ValueError('oracle failure')


def alive(pid):
    try:
        os.kill(pid, 0)
    except ProcessLookupError:
        return False
    # A reaped-or-zombie child of someone else still answers kill(0); check its state.
    try:
        return Path('/proc/%d/stat' % pid).read_text().split(')')[-1].split()[0] != 'Z'
    except FileNotFoundError:
        return False


class Controller(unittest.TestCase):
    def setUp(self):
        self.tmp = Path(tempfile.mkdtemp())

    def tearDown(self):
        shutil.rmtree(self.tmp)

    def method(self, source):
        d = Path(tempfile.mkdtemp(dir=self.tmp))
        (d / 'method.py').write_text(textwrap.dedent(source))
        return d

    def test_a_well_behaved_method_runs_to_its_exit_code(self):
        m = self.method("""
            import sys
            for op in ('instances', 'submit'):
                sys.stdout.write('{"op": "%s"}\\n' % op); sys.stdout.flush()
                assert sys.stdin.readline().startswith('{"ok": true')
            sys.exit(3)
        """)
        oracle = Echo()
        lines, code = replay.run_method(m, oracle, 'host', timeout=20)
        self.assertEqual((lines, code), (['{"op": "instances"}\n', '{"op": "submit"}\n'], 3))
        self.assertEqual(oracle.seen, [{'op': 'instances'}, {'op': 'submit'}])

    def test_the_deadline_covers_a_method_that_never_writes_a_newline(self):
        m = self.method("""
            import sys, time
            sys.stdout.write('{"op": "instances"'); sys.stdout.flush()
            time.sleep(60)
        """)
        t = time.monotonic()
        with self.assertRaisesRegex(replay.Stopped, 'within 1 s'):
            replay.run_method(m, Echo(), 'host', timeout=1)
        self.assertLess(time.monotonic() - t, 15)

    def test_the_deadline_covers_a_method_that_never_reads_its_replies(self):
        # Replies fill the pipe; a blocking write would hang past any wait() timeout.
        m = self.method("""
            import sys, time
            for _ in range(2000):
                sys.stdout.write('{"op": "instances"}\\n')
            sys.stdout.flush()
            time.sleep(60)
        """)

        class Big:
            def reply(self, req):
                return {'ok': True, 'pad': 'x' * 4096}

        with self.assertRaises(replay.Stopped):
            replay.run_method(m, Big(), 'host', timeout=2)

    def test_line_request_and_output_bounds(self):
        long_line = self.method("""
            import sys
            sys.stdout.write('x' * 5000); sys.stdout.flush()
        """)
        with self.assertRaisesRegex(replay.Stopped, 'request line over 1024'):
            replay.run_method(long_line, Echo(), 'host', timeout=20, max_line=1024)
        many = self.method("""
            import sys
            while True:
                sys.stdout.write('{"op": "instances"}\\n'); sys.stdout.flush(); sys.stdin.readline()
        """)
        with self.assertRaisesRegex(replay.Stopped, 'more than 50 requests'):
            replay.run_method(many, Echo(), 'host', timeout=20, max_requests=50)
        flood = self.method("""
            import sys
            while True:
                sys.stdout.write('y\\n')
        """)
        with self.assertRaisesRegex(replay.Stopped, 'more than 100000 bytes'):
            replay.run_method(flood, Echo(), 'host', timeout=20, max_output=100_000, max_requests=10 ** 9)

    def test_a_failure_kills_and_reaps_the_whole_process_group(self):
        pidfile = self.tmp / 'grandchild.pid'
        m = self.method("""
            import subprocess, sys
            child = subprocess.Popen(['sleep', '60'])
            open(%r, 'w').write(str(child.pid))
            sys.stdout.write('{"op": "instances"}\\n'); sys.stdout.flush()
            sys.stdin.readline()
        """ % str(pidfile))
        with self.assertRaisesRegex(ValueError, 'oracle failure'):
            replay.run_method(m, Boom(), 'host', timeout=20)
        pid = int(pidfile.read_text())
        for _ in range(100):
            if not alive(pid):
                break
            time.sleep(0.05)
        self.assertFalse(alive(pid), 'grandchild survived the run')

    def test_a_normal_exit_also_ends_leftover_descendants(self):
        pidfile = self.tmp / 'left.pid'
        m = self.method("""
            import subprocess
            child = subprocess.Popen(['sleep', '60'], stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL)
            open(%r, 'w').write(str(child.pid))
        """ % str(pidfile))
        replay.run_method(m, Echo(), 'host', timeout=20)
        pid = int(pidfile.read_text())
        for _ in range(100):
            if not alive(pid):
                break
            time.sleep(0.05)
        self.assertFalse(alive(pid), 'descendant survived a normal exit')

    def test_isolation_is_required_and_named(self):
        m = self.method('pass\n')
        with self.assertRaisesRegex(ValueError, 'isolation must be one of'):
            replay.run_method(m, Echo(), 'none')
        self.assertEqual(replay.main(['replay.py', 'diff', str(m), str(m), '1']), 2)

    @unittest.skipUnless(os.geteuid() == 0 and shutil.which('unshare'), 'namespaces need root and unshare')
    def test_namespaces_hide_the_host_and_drop_privileges(self):
        secret = Path('/root') / ('replay-test-%d' % os.getpid())
        secret.write_text('host file')
        try:
            m = self.method("""
                import json, os, sys
                facts = {'uid': os.getuid(), 'root': os.listdir('/root') if os.access('/root', os.R_OK) else None,
                         'cwd': os.getcwd()}
                sys.stdout.write(json.dumps({'op': 'facts', 'facts': facts}) + '\\n'); sys.stdout.flush()
            """)
            oracle = Echo()
            _, code = replay.run_method(m, oracle, 'namespaces', timeout=30)
        finally:
            secret.unlink()
        self.assertEqual(code, 0)
        facts = oracle.seen[0]['facts']
        self.assertEqual(facts['uid'], 65534)
        self.assertNotIn(secret.name, facts['root'] or [])
        self.assertEqual(facts['cwd'], '/tmp/work')
        self.assertEqual(list(replay.STAGE_ROOT.iterdir()), [])


if __name__ == '__main__':
    unittest.main()
