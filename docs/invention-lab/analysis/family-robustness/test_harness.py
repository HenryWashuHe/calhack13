"""Failure accounting in the analysis harness, with a fake evaluator and
hand-written fake controls. No real evaluator, method or generated source runs.

    python3 -m unittest test_harness
"""
import json
import os
import stat
import sys
import tempfile
import time
import unittest

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import calibrate  # noqa: E402

FAKE_EVAL = r'''#!/usr/bin/env python3
import json, os, sys
mode = os.environ["FAKE_MODE"]
op = sys.argv[1]
if op == "instance":
    print(json.dumps({"rules": {"zone": [-6, 0, 0, 3]}}))
elif op == "experiment":
    if mode == "experiment_fails":
        sys.exit(2)
    steps = {"bad_steps": -5, "too_many_steps": 999999}.get(mode, 100)
    print(json.dumps({"receipt": {"outcome": "scored", "simulation_steps": steps,
                                  "metrics": {"success": False}}}))
elif op == "accept":
    if mode == "accept_fails":
        sys.exit(3)
    seeds = [int(x) for x in sys.argv[4:]]
    print(json.dumps({"succeeded": 4, "of": len(seeds), "receipts": [
        {"seed": x, "outcome": "scored", "metrics": {"success": i < 4}}
        for i, x in enumerate(seeds)]}))
'''

CONTROL = r'''import json, sys, time
def call(r):
    sys.stdout.write(json.dumps(r) + "\n"); sys.stdout.flush()
    return json.loads(sys.stdin.readline())
info = call({"op": "instances"})
MODE
call({"op": "experiment", "instance": info["instances"][0]["id"], "design": {"payload_on": "deck"}})
call({"op": "submit", "design": {"payload_on": "deck"}})
'''


class Harness(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.eval = os.path.join(self.tmp.name, "fake-eval")
        with open(self.eval, "w") as f:
            f.write(FAKE_EVAL)
        os.chmod(self.eval, os.stat(self.eval).st_mode | stat.S_IEXEC)

    def control(self, before=""):
        path = os.path.join(self.tmp.name, "control.py")
        with open(path, "w") as f:
            f.write(CONTROL.replace("MODE", before))
        return path

    def run_panel(self, mode, before=""):
        os.environ["FAKE_MODE"] = mode
        return calibrate.panel_run(self.eval, self.control(before), "ravine-crossing",
                                   list(range(6)), 72000, self.tmp.name)

    def test_a_clean_panel_is_counted(self):
        row = self.run_panel("ok")
        self.assertEqual((row["successes"], row["experiments"], row["steps_used"]), (4, 1, 100))
        self.assertNotIn("unavailable", row)

    def test_failed_or_untrustworthy_experiments_make_the_panel_unavailable(self):
        for mode in ("experiment_fails", "bad_steps", "too_many_steps"):
            with self.subTest(mode=mode):
                row = self.run_panel(mode)
                self.assertIsNone(row["successes"])
                self.assertIn("unavailable", row)

    def test_failed_acceptance_is_unavailable_not_zero(self):
        row = self.run_panel("accept_fails")
        self.assertIsNone(row["successes"])
        self.assertIsNone(calibrate.total([row, {"successes": 6}]))
        self.assertEqual(calibrate.total([{"successes": 2}, {"successes": 6}]), 8)

    def test_a_stalled_control_hits_the_deadline_and_is_reaped(self):
        old = calibrate.PANEL_DEADLINE_S
        calibrate.PANEL_DEADLINE_S = 2
        self.addCleanup(setattr, calibrate, "PANEL_DEADLINE_S", old)
        start = time.monotonic()
        row = self.run_panel("ok", before="time.sleep(600)")
        self.assertLess(time.monotonic() - start, 30)
        self.assertEqual(row["unavailable"], "panel deadline passed")
        self.assertIsNone(row["successes"])

    def test_a_malformed_control_line_is_unavailable(self):
        row = self.run_panel("ok", before='sys.stdout.write("not json\\n"); sys.stdout.flush(); time.sleep(600)')
        self.assertEqual(row["unavailable"], "control wrote a malformed line")


if __name__ == "__main__":
    unittest.main()
