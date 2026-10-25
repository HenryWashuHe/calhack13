"""Pure/mocked shape checks for #207; no subprocess or control is executed."""
import unittest
from types import SimpleNamespace
from unittest import mock

import calibrate
import review_harness_boundaries as prior


class Shapes(unittest.TestCase):
    def test_invalid_design_remains_a_measured_failure(self):
        scored = prior.acceptance("invalid")
        self.assertEqual(calibrate.acceptance_count(scored, list(range(6))), 0)

    def test_evaluator_timeout_is_capped_by_panel_time_left(self):
        with mock.patch.object(calibrate, "_panel_deadline", 105), \
             mock.patch.object(calibrate.time, "monotonic", return_value=100), \
             mock.patch.object(calibrate.subprocess, "run", return_value=SimpleNamespace(returncode=0, stdout="{}")) as run:
            self.assertEqual(calibrate.lab("not-executed", "accept"), {})
            self.assertEqual(run.call_args.kwargs["timeout"], 5)

    def test_malformed_outcome_is_unavailable_instead_of_aborting_panel(self):
        for outcome in ([], {}):
            with self.subTest(outcome=outcome):
                scored = prior.acceptance()
                scored["receipts"][0]["outcome"] = outcome
                case = prior.Boundaries()
                self.addCleanup(case.doCleanups)
                row = case.panel(accepted=scored)
                self.assertIsNone(row["successes"])
                self.assertIn("unavailable", row)

    def test_nonobject_request_is_unavailable_instead_of_aborting_panel(self):
        case = prior.Boundaries()
        self.addCleanup(case.doCleanups)
        row = case.panel(requests=[[]])
        self.assertIsNone(row["successes"])
        self.assertIn("unavailable", row)


if __name__ == "__main__":
    unittest.main()
