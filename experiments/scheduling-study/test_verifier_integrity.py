"""Pin an externally published head without changing retained pilot evidence."""

import tempfile
import unittest
from pathlib import Path

from run_local_pilot import run_pilot
from verify_run import verify


class PublishedHeadTest(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.storage = tempfile.TemporaryDirectory()
        cls.pilot_root = Path(cls.storage.name) / "pilot"
        run_pilot(cls.pilot_root, 82020, 0)

    @classmethod
    def tearDownClass(cls):
        cls.storage.cleanup()

    def test_exact_published_head(self):
        head = verify(self.pilot_root)["last_event_sha256"]
        self.assertEqual(verify(self.pilot_root, head)["last_event_sha256"], head)
        with self.assertRaisesRegex(ValueError, "published event head mismatch"):
            verify(self.pilot_root, "0" * 64)

    def test_malformed_published_head(self):
        for head in ("0" * 63, "G" * 64, "A" * 64):
            with self.subTest(head=head), self.assertRaisesRegex(ValueError, "expected head"):
                verify(self.pilot_root, head)


if __name__ == "__main__":
    unittest.main()
