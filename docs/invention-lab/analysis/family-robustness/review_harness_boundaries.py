"""Independent #207 boundaries: inert records, fake clock and mocked processes only.

No evaluator, control, generated source, provider, namespace or calibration runs.
Run: python3 -m unittest review_harness_boundaries
"""
import contextlib
import io
import json
import tempfile
import unittest
from unittest import mock

import calibrate
import headroom
import panel_cap

FAMILY = "ravine-crossing"
DESIGN = {"payload_on": "deck"}
EXPERIMENT = {"op": "experiment", "instance": FAMILY + "/1/0", "design": DESIGN}
SUBMIT = {"op": "submit", "design": DESIGN}


def acceptance(outcome="scored"):
    # lab-eval accept computes succeeded from available metrics. An unknown
    # receipt has no metrics and is therefore absent from that aggregate count.
    return {"succeeded": 0, "of": 6, "receipts": [
        {"seed": s, "outcome": outcome,
         "simulation_steps": None if outcome == "unknown" else 100,
         "metrics": None if outcome == "unknown" else {"success": False}}
        for s in range(6)]}


class Boundaries(unittest.TestCase):
    def panel(self, requests=None, accepted=None, accept_elapsed=0):
        requests = [SUBMIT] if requests is None else requests
        accepted = acceptance() if accepted is None else accepted
        clock = [0.0]
        wire = "".join((r if isinstance(r, str) else json.dumps(r)) + "\n" for r in requests)
        proc = mock.Mock(stdin=io.StringIO(), stdout=io.StringIO(wire))
        self.addCleanup(proc.stdout.close)

        def evaluator(_executable, op, *_args):
            if op == "instance":
                return {"rules": {}}
            if op == "experiment":
                return {"receipt": {"simulation_steps": 100, "outcome": "scored", "metrics": {"success": False}}}
            self.assertEqual(op, "accept")
            clock[0] += accept_elapsed
            return accepted

        def reader(*, target, args, daemon):
            self.assertTrue(daemon)
            return mock.Mock(start=lambda: target(*args))

        with tempfile.TemporaryDirectory() as tmp, \
             mock.patch.object(calibrate, "lab", side_effect=evaluator), \
             mock.patch.object(calibrate.subprocess, "Popen", return_value=proc), \
             mock.patch.object(calibrate.threading, "Thread", side_effect=reader), \
             mock.patch.object(calibrate.time, "monotonic", side_effect=lambda: clock[0]), \
             mock.patch.object(calibrate, "PANEL_DEADLINE_S", 10):
            try:
                return calibrate.panel_run("not-executed", "not-executed.py", FAMILY,
                                           list(range(6)), 72000, tmp)
            finally:
                proc.wait.assert_called_once_with(timeout=5)
                self.assertTrue(proc.stdin.closed)

    def test_known_false_acceptance_is_measured_zero_with_known_cost(self):
        row = self.panel([EXPERIMENT, SUBMIT])
        self.assertEqual((row["successes"], row["experiments"], row["steps_used"]), (0, 1, 100))
        self.assertNotIn("unavailable", row)

    def test_unknown_acceptance_is_not_a_measured_zero(self):
        row = self.panel(accepted=acceptance("unknown"))
        self.assertIsNone(row["successes"])
        self.assertIn("unavailable", row)

    def test_malformed_acceptance_shape_is_retained_as_unavailable(self):
        row = self.panel(accepted=[])
        self.assertIsNone(row["successes"])
        self.assertIn("unavailable", row)

    def test_invalid_counts_cannot_be_reported_as_panel_measurements(self):
        for count in (True, -1, 7):
            with self.subTest(count=count):
                scored = acceptance()
                scored["succeeded"] = count
                row = self.panel(accepted=scored)
                self.assertIsNone(row["successes"])

    def test_prior_measured_cost_survives_a_later_malformed_request(self):
        row = self.panel([EXPERIMENT, "not json"])
        self.assertIsNone(row["successes"])
        self.assertEqual(row["experiments"], 1)
        self.assertEqual(row["steps_used"], 100)

    def test_final_acceptance_cannot_turn_an_expired_panel_into_success(self):
        row = self.panel(accept_elapsed=11)
        self.assertIsNone(row["successes"])
        self.assertIn("deadline", row["unavailable"])

    def test_fixed_headroom_keeps_unknown_acceptance_unavailable(self):
        output = io.StringIO()
        with mock.patch.object(headroom, "FAMILIES", (FAMILY,)), \
             mock.patch.object(headroom, "PANELS", 1), \
             mock.patch.object(headroom, "METHODS", {}), \
             mock.patch.object(headroom, "FIXED", {"fixture": "not-read.json"}), \
             mock.patch.object(calibrate, "lab", return_value=acceptance("unknown")), \
             contextlib.redirect_stdout(output):
            headroom.main(["headroom", "not-executed"])
        result = json.loads(output.getvalue())["families"][FAMILY]["fixture"]
        self.assertEqual(result["per_panel"], [None])
        self.assertIsNone(result["total"])

    def test_histogram_does_not_count_an_unknown_receipt_as_zero(self):
        def evaluator(_executable, op, *_args):
            return {"rules": {}, "target_x": 1} if op == "instance" else acceptance("unknown")
        with mock.patch.object(panel_cap, "lab", side_effect=evaluator), \
             mock.patch.object(panel_cap.mixed_search, "launcher", return_value=DESIGN), \
             contextlib.redirect_stdout(io.StringIO()), self.assertRaises(SystemExit):
            panel_cap.main(["panel_cap", "not-executed", "1", "1", "0"])


if __name__ == "__main__":
    unittest.main()
