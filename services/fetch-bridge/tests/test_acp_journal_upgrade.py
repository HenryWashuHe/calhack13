"""Henry's Claude: upgrading a journal from the message-only UNIQUE schema keeps every
row, the id sequence and pending intents, and reopening is a no-op. No cloud calls."""

import sqlite3
import tempfile
import unittest
from datetime import UTC, datetime
from pathlib import Path
from unittest.mock import AsyncMock

from genesis_fetch_bridge.acp_mission import MissionGateway

OLD_SCHEMA = """
    CREATE TABLE gateway_binding(id INTEGER PRIMARY KEY CHECK(id=1), value TEXT NOT NULL);
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
"""


class JournalUpgrade(unittest.IsolatedAsyncioTestCase):
    async def asyncSetUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.path = str(Path(self.temp.name) / "gateway.sqlite")
        now = datetime.now(UTC).isoformat()
        with sqlite3.connect(self.path) as db:
            db.executescript(OLD_SCHEMA)
            db.executemany(
                "INSERT INTO reports VALUES(?,?,?,?,?,?,?,?)",
                [
                    (3, "obs_done", 5, now, "agent1alice", "m-1", "done", 9),
                    # A pending intent: POSTed or about to be, outcome not yet recorded.
                    (7, None, 6, now, "agent1alice", "m-2", "pending", 6),
                ],
            )
            # The sequence had gone past the surviving rows (a later row was removed).
            db.execute("UPDATE sqlite_sequence SET seq=12 WHERE name='reports'")
        self.gateways = []

    async def asyncTearDown(self):
        for g in self.gateways:
            g.close()
        self.temp.cleanup()

    def open(self):
        g = MissionGateway(
            config={"public_demo": True, "evidence_ref": "fixture://upgrade/input",
                    "source": "upgrade", "max_reports_per_hour": 60, "follow_seconds": 0},
            url="http://127.0.0.1:7101/api/v1", token="upgrade-agent-token",
            identity="agent1upgrade", db=self.path,
        )
        g._state = AsyncMock(return_value={"missions": {
            "mis_up": {"mission_id": "mis_up", "status": "ACTIVE", "goal": "Evaluate a design"}}})
        g._latest_seq = AsyncMock(return_value=7)
        g._http = AsyncMock(return_value=(201, {"observation_id": "obs_new"}))
        self.gateways.append(g)
        return g

    def rows(self, g):
        return [tuple(r) for r in g.db.execute(
            "SELECT id, observation_id, after_seq, sender, message_id, summary, visible_seq "
            "FROM reports ORDER BY id")]

    async def test_upgrade_keeps_rows_sequence_and_pending_intents(self):
        g = self.open()
        self.assertEqual(self.rows(g), [
            (3, "obs_done", 5, "agent1alice", "m-1", "done", 9),
            (7, None, 6, "agent1alice", "m-2", "pending", 6),
        ])
        seq = g.db.execute("SELECT seq FROM sqlite_sequence WHERE name='reports'").fetchone()[0]
        self.assertEqual(seq, 12, "the id sequence must not move backwards")
        uniques = [i for i in g.db.execute("PRAGMA index_list(reports)").fetchall() if i["unique"]]
        self.assertEqual([i["name"] for i in uniques], ["reports_sender_message"])
        # A new report gets an id past the old sequence, never a reused one.
        replies = [r async for r in g.respond("report the load doubled", sender="agent1bob", message_id="m-1")]
        self.assertIn("Recorded as evidence", "\n".join(replies))
        new_id = g.db.execute("SELECT id FROM reports WHERE sender='agent1bob'").fetchone()[0]
        self.assertGreater(new_id, 12)

    async def test_reopening_an_upgraded_journal_changes_nothing(self):
        first = self.open()
        before = self.rows(first)
        first.close()
        self.gateways.remove(first)
        second = self.open()
        self.assertEqual(self.rows(second), before)
        # The same sender's redelivery is still one effect.
        replies = [r async for r in second.respond("report again", sender="agent1alice", message_id="m-1")]
        second._http.assert_not_awaited()
        self.assertTrue(replies)


if __name__ == "__main__":
    unittest.main()
