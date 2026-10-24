"""Independent PR #71 review at c02c251f5a8b1a31eb4623bfdbe9584cf53b52aa.

SQLite-only upgrade fault injection: no backend or agent calls. The connection
opened before migration represents a writer still using the journal during an
upgrade. A failed upgrade must not leave its deduplication constraint absent.
"""

import sqlite3
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

from genesis_fetch_bridge.acp_mission import MissionGateway


class FailReplacementIndex(sqlite3.Connection):
    def execute(self, sql, *args):
        normalized = " ".join(sql.split()).upper()
        if normalized.startswith("CREATE UNIQUE INDEX") and "REPORTS_SENDER_MESSAGE" in normalized:
            raise sqlite3.OperationalError("review: replacement index creation failed")
        return super().execute(sql, *args)


class MigrationAtomicityReview(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.path = str(Path(self.temp.name) / "journal.sqlite")
        # Legacy inline uniqueness, plus the old separately named index.
        with sqlite3.connect(self.path) as db:
            db.executescript("""
                CREATE TABLE reports(
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    observation_id TEXT,
                    after_seq INTEGER NOT NULL,
                    created_at TEXT NOT NULL,
                    sender TEXT NOT NULL DEFAULT '',
                    message_id TEXT UNIQUE,
                    summary TEXT NOT NULL DEFAULT '',
                    visible_seq INTEGER NOT NULL DEFAULT 0
                );
                CREATE UNIQUE INDEX reports_message ON reports(message_id);
            """)
        self.existing = sqlite3.connect(self.path, isolation_level=None)

    def tearDown(self):
        self.existing.close()
        self.temp.cleanup()

    def open_gateway(self):
        return MissionGateway(
            config={"public_demo": True, "evidence_ref": "fixture://review/input",
                    "source": "review", "max_reports_per_hour": 60, "follow_seconds": 0},
            url="http://127.0.0.1:7101/api/v1", token="review-agent-token",
            identity="agent1review", db=self.path,
        )

    def insert(self, sender):
        self.existing.execute(
            "INSERT INTO reports(after_seq, created_at, sender, message_id) VALUES(?,?,?,?)",
            (0, "2026-10-24T00:00:00+00:00", sender, "same-message"),
        )

    def test_successful_upgrade_preserves_sender_message_deduplication(self):
        gateway = self.open_gateway()
        try:
            self.insert("agent1alice")
            self.insert("agent1bob")
            with self.assertRaises(sqlite3.IntegrityError):
                self.insert("agent1alice")
        finally:
            gateway.close()

    def test_failed_replacement_index_cannot_commit_unprotected_reports(self):
        real_connect = sqlite3.connect
        with patch(
            "genesis_fetch_bridge.acp_mission.sqlite3.connect",
            side_effect=lambda *args, **kwargs: real_connect(
                *args, **kwargs, factory=FailReplacementIndex),
        ):
            with self.assertRaisesRegex(sqlite3.OperationalError, "replacement index"):
                self.open_gateway()
        # Either the old or the new uniqueness guard must survive a failure.
        # The old open connection must not be able to insert a duplicate intent.
        self.insert("agent1alice")
        with self.assertRaises(sqlite3.IntegrityError):
            self.insert("agent1alice")


if __name__ == "__main__":
    unittest.main()
