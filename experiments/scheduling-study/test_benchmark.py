import copy
import hashlib
import tempfile
import unittest
from pathlib import Path

from benchmark import digest, generate_instance, make_policy, schedule
from independent_checker import check_schedule
from run_local_pilot import run_pilot
from verify_run import verify


class SchedulingStudyTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.instance = generate_instance(81000, "balanced")
        cls.policy = make_policy((1, 0, 0, 0, 0, 0))
        cls.entries = schedule(cls.instance, cls.policy)

    def test_generator_and_schedule_are_deterministic(self):
        self.assertEqual(digest(self.instance), digest(generate_instance(81000, "balanced")))
        self.assertEqual(digest(self.entries), digest(schedule(self.instance, self.policy)))
        self.assertGreater(check_schedule(self.instance, self.entries), 0)
        self.assertNotEqual(digest(self.instance), digest(generate_instance(81000, "bottleneck")))

    def test_rejects_missing_and_duplicate_operations(self):
        for bad in (self.entries[:-1], self.entries[:-1] + [self.entries[0]]):
            with self.subTest(length=len(bad)), self.assertRaises(ValueError):
                check_schedule(self.instance, bad)

    def test_rejects_wrong_machine_and_duration(self):
        for field, value in (("machine", 12), ("duration", 0), ("start", -1)):
            bad = copy.deepcopy(self.entries)
            bad[0][field] = value
            with self.subTest(field=field), self.assertRaises(ValueError):
                check_schedule(self.instance, bad)

    def test_rejects_job_precedence_and_machine_overlap(self):
        later = next(entry for entry in self.entries if entry["job"] == 0 and entry["operation"] == 1)
        bad = copy.deepcopy(self.entries)
        match = next(entry for entry in bad if entry["job"] == 0 and entry["operation"] == 1)
        match["start"] = 0
        with self.assertRaisesRegex(ValueError, "precedence"):
            check_schedule(self.instance, bad)
        first = self.entries[0]
        on_same_machine = next(entry for entry in self.entries[1:] if entry["machine"] == first["machine"])
        bad = copy.deepcopy(self.entries)
        match = next(entry for entry in bad if entry["job"] == on_same_machine["job"]
                     and entry["operation"] == on_same_machine["operation"])
        match["start"] = first["start"]
        with self.assertRaises(ValueError):
            check_schedule(self.instance, bad)

    def test_policy_rejects_unsupported_fields_and_boolean_weights(self):
        with self.assertRaises(ValueError):
            make_policy((True, 0, 0, 0, 0, 0))
        with self.assertRaises(ValueError):
            make_policy((0, 0, 0, 0, 0, 0))

    def test_pilot_retains_every_evaluation_in_english_hash_chain(self):
        with tempfile.TemporaryDirectory() as root:
            output = Path(root) / "pilot"
            summary = run_pilot(output, 82000, 1)
            self.assertFalse(summary["formal_test"])
            self.assertEqual(summary["model_requests"], 0)
            self.assertEqual(summary["development_evaluations"], 100)
            self.assertEqual(summary["calibration_evaluations"], 160)
            self.assertEqual(verify(output)["evaluation_count"], 260)
            lines = (output / "events.jsonl").read_text(encoding="ascii").splitlines()
            import json
            self.assertEqual(sum(json.loads(line)["record"]["event"] == "policy_evaluated"
                                 for line in lines), 260)
            previous = "0" * 64
            for sequence, line in enumerate(lines):
                entry = json.loads(line)
                self.assertEqual(entry["sequence"], sequence)
                self.assertEqual(entry["previous_sha256"], previous)
                payload = {key: value for key, value in entry.items() if key != "sha256"}
                expected = hashlib.sha256(json.dumps(payload, sort_keys=True,
                    separators=(",", ":"), ensure_ascii=True).encode("ascii")).hexdigest()
                self.assertEqual(entry["sha256"], expected)
                previous = expected
            with self.assertRaises(FileExistsError):
                run_pilot(output, 82000, 1)

            lines[3] = lines[3].replace('"sha256":"', '"sha256":"0')
            (output / "events.jsonl").write_text("\n".join(lines) + "\n", encoding="ascii")
            with self.assertRaises(ValueError):
                verify(output)


if __name__ == "__main__":
    unittest.main()
