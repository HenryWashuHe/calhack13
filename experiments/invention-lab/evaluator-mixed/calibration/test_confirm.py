"""The predeclared confirmation reading, without running anything.

    python3 -I -m unittest discover -s experiments/invention-lab/evaluator-mixed/calibration
"""
import os
import sys
import unittest

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import confirm  # noqa: E402


class Reading(unittest.TestCase):
    def test_discriminating_needs_seven_wins_and_at_most_two_losses(self):
        a = [3] * 7 + [1] * 2 + [2] * 3
        b = [2] * 7 + [2] * 2 + [2] * 3
        self.assertEqual(confirm.reading(a, b)["reading"], "discriminating")
        self.assertEqual(confirm.reading(a[:6] + [2] + a[7:], b)["reading"], "not discriminating")
        three_losses = [3] * 7 + [1] * 3 + [2] * 2
        self.assertEqual(confirm.reading(three_losses, b)["reading"], "not discriminating")

    def test_a_tie_is_neither_a_win_nor_a_loss(self):
        r = confirm.reading([2] * 12, [2] * 12)
        self.assertEqual((r["wins"], r["losses"], r["reading"]), (0, 0, "not discriminating"))

    def test_any_unavailable_panel_makes_it_incomplete(self):
        a = [6] * 12
        b = [0] * 11 + [None]
        r = confirm.reading(a, b)
        self.assertEqual(r, {"reading": "incomplete", "unavailable_panels": [11]})
        self.assertEqual(confirm.reading([None] + a[1:], [0] * 12)["unavailable_panels"], [0])

    def test_a_different_evaluator_is_refused(self):
        class Lab:
            @staticmethod
            def lab(*_):
                return {"evaluator_sha256": "other"}
        saved = confirm.calibrate
        confirm.calibrate = Lab
        try:
            with self.assertRaises(SystemExit):
                confirm.main(["confirm.py", "lab-eval-mixed", "frozen"])
        finally:
            confirm.calibrate = saved


if __name__ == "__main__":
    unittest.main()
