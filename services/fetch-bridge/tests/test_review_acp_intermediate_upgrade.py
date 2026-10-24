"""Independent review of PR3 da5f2ce: upgrade the intermediate sender-index schema.

3ad7685 added reports_sender_message to existing journals without removing the
old inline UNIQUE(message_id). Those journals have both guards. This is a valid
previously produced state, not a malformed/hostile journal. No network calls.
"""

import sqlite3
import tempfile
import unittest
from pathlib import Path

from genesis_fetch_bridge.acp_mission import MissionGateway


class IntermediateJournalReview(unittest.TestCase):
    def exercise_upgrade(self, *, inline_unique, sender_index):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "journal.sqlite"
            with sqlite3.connect(path) as db:
                db.execute(f"""CREATE TABLE reports(
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    observation_id TEXT,
                    after_seq INTEGER NOT NULL,
                    created_at TEXT NOT NULL,
                    sender TEXT NOT NULL DEFAULT '',
                    message_id TEXT {'UNIQUE' if inline_unique else ''},
                    summary TEXT NOT NULL DEFAULT '',
                    visible_seq INTEGER NOT NULL DEFAULT 0
                )""")
                if sender_index:
                    db.execute("CREATE UNIQUE INDEX reports_sender_message ON reports(sender, message_id)")
                else:
                    db.execute("CREATE UNIQUE INDEX reports_message ON reports(message_id)")
                db.executemany(
                    "INSERT INTO reports VALUES(?,?,?,?,?,?,?,?)",
                    [(3, "obs_done", 5, "2026-10-24T00:00:00+00:00", "alice", "m1", "done", 9),
                     (7, None, 6, "2026-10-24T00:00:00+00:00", "alice", "m2", "pending", 6)],
                )
                db.execute("UPDATE sqlite_sequence SET seq=12 WHERE name='reports'")

            def open_gateway():
                return MissionGateway(
                    config={"public_demo": True, "evidence_ref": "fixture://review/input",
                            "source": "review", "max_reports_per_hour": 60, "follow_seconds": 0},
                    url="http://127.0.0.1:7101/api/v1", token="review-agent-token",
                    identity="agent1review", db=str(path),
                )

            gateway = open_gateway()
            try:
                self.assertEqual(
                    [tuple(r) for r in gateway.db.execute(
                        "SELECT id, observation_id, after_seq, sender, message_id, summary, visible_seq "
                        "FROM reports ORDER BY id")],
                    [(3, "obs_done", 5, "alice", "m1", "done", 9),
                     (7, None, 6, "alice", "m2", "pending", 6)],
                )
                self.assertEqual(gateway.db.execute(
                    "SELECT seq FROM sqlite_sequence WHERE name='reports'").fetchone()[0], 12)
                gateway.db.execute(
                    "INSERT INTO reports(after_seq, created_at, sender, message_id) VALUES(0, 'now', 'bob', 'm1')")
                self.assertGreater(gateway.db.execute(
                    "SELECT id FROM reports WHERE sender='bob'").fetchone()[0], 12)
                with self.assertRaises(sqlite3.IntegrityError):
                    gateway.db.execute(
                        "INSERT INTO reports(after_seq, created_at, sender, message_id) VALUES(0, 'now', 'bob', 'm1')")
            finally:
                gateway.close()
            reopened = open_gateway()
            try:
                self.assertEqual(reopened.db.execute("SELECT COUNT(*) FROM reports").fetchone()[0], 3)
            finally:
                reopened.close()

    def test_plain_legacy_schema_control(self):
        self.exercise_upgrade(inline_unique=True, sender_index=False)

    def test_current_sender_schema_control(self):
        self.exercise_upgrade(inline_unique=False, sender_index=True)

    def test_intermediate_inline_plus_sender_index_schema_upgrades(self):
        self.exercise_upgrade(inline_unique=True, sender_index=True)


if __name__ == "__main__":
    unittest.main()
