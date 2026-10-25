"""Independent record-binding guards; trusted local scheduler only, no model code."""
import copy
import json
import tempfile
import unittest
from pathlib import Path

from benchmark import canonical_bytes, digest
from run_local_pilot import run_pilot
from verify_run import verify


class VerifierBindingReview(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.storage = tempfile.TemporaryDirectory()
        root = Path(cls.storage.name) / "control"
        run_pilot(root, 82010, 1)
        cls.files = {p.name: p.read_bytes() for p in root.iterdir()}

    @classmethod
    def tearDownClass(cls):
        cls.storage.cleanup()

    def fixture(self, root):
        root.mkdir()
        for name, contents in self.files.items():
            (root / name).write_bytes(contents)

    def reseal(self, root, manifest=None, records=None):
        manifest = manifest or json.loads(self.files["manifest.json"])
        summary = json.loads(self.files["summary.json"])
        summary["manifest_sha256"] = digest(manifest)
        (root / "manifest.json").write_bytes(canonical_bytes(manifest) + b"\n")
        (root / "summary.json").write_bytes(canonical_bytes(summary) + b"\n")
        if records is None:
            records = [json.loads(line)["record"]
                       for line in self.files["events.jsonl"].splitlines()]
        records = copy.deepcopy(records)
        previous = "0" * 64
        lines = []
        for index, record in enumerate(records):
            if record["event"] == "run_started":
                record["manifest_sha256"] = digest(manifest)
            if record["event"] == "run_completed":
                record["summary_sha256"] = digest(summary)
            payload = {"sequence": index, "previous_sha256": previous, "record": record}
            previous = digest(payload)
            lines.append(canonical_bytes({**payload, "sha256": previous}))
        (root / "events.jsonl").write_bytes(b"\n".join(lines) + b"\n")

    def test_unchanged_pilot_is_verified(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory) / "pilot"
            self.fixture(root)
            self.assertEqual(verify(root)["evaluation_count"], 260)

    def test_manifest_seed_must_generate_retained_instances(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory) / "pilot"
            self.fixture(root)
            manifest = json.loads(self.files["manifest.json"])
            manifest["seed"] += 1
            self.reseal(root, manifest=manifest)
            with self.assertRaises(ValueError):
                verify(root)

    def test_required_source_hashes_cannot_be_omitted(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory) / "pilot"
            self.fixture(root)
            manifest = json.loads(self.files["manifest.json"])
            manifest["source_sha256"] = {}
            self.reseal(root, manifest=manifest)
            with self.assertRaises(ValueError):
                verify(root)

    def test_failed_journal_cannot_also_verify_as_complete(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory) / "pilot"
            self.fixture(root)
            records = [json.loads(line)["record"]
                       for line in self.files["events.jsonl"].splitlines()]
            records.insert(-1, {"event": "run_failed", "timestamp_utc": "fixture",
                                "error_type": "RuntimeError", "error_message": "fixture"})
            self.reseal(root, records=records)
            with self.assertRaises(ValueError):
                verify(root)

    def test_calibration_must_follow_frozen_selection(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory) / "pilot"
            self.fixture(root)
            records = [json.loads(line)["record"]
                       for line in self.files["events.jsonl"].splitlines()]
            selection = next(record for record in records
                             if record["event"] == "candidate_selected")
            records.remove(selection)
            records.insert(-1, selection)
            self.reseal(root, records=records)
            with self.assertRaises(ValueError):
                verify(root)


if __name__ == "__main__":
    unittest.main()
