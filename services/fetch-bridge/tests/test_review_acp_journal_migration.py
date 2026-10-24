"""Independent report-identity migration and effect-status checks; no cloud calls."""

import sqlite3
import tempfile
import unittest
from datetime import UTC, datetime
from pathlib import Path
from unittest.mock import AsyncMock, patch
from uuid import uuid4

from genesis_fetch_bridge.acp_mission import MissionGateway


class JournalIdentityReview(unittest.IsolatedAsyncioTestCase):
    async def asyncSetUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.path = str(Path(self.temp.name) / "gateway.sqlite")
        self.gateway = None

    async def asyncTearDown(self):
        if self.gateway is not None:
            self.gateway.close()
        self.temp.cleanup()

    def open_gateway(self):
        self.gateway = MissionGateway(
            config={"public_demo": True, "evidence_ref": "fixture://review/input",
                    "source": "review", "max_reports_per_hour": 60, "follow_seconds": 0},
            url="http://127.0.0.1:7101/api/v1", token="review-agent-token",
            identity="agent1review", db=self.path,
        )
        self.gateway._state = AsyncMock(return_value={"missions": {
            "mis_review": {"mission_id": "mis_review", "status": "ACTIVE", "goal": "Evaluate a design"}
        }})
        self.gateway._latest_seq = AsyncMock(return_value=7)
        self.gateway._http = AsyncMock(return_value=(201, {"observation_id": "obs_review"}))
        return self.gateway

    async def report(self, sender, message_id):
        return [reply async for reply in self.gateway.respond(
            "report load exceeds the current design envelope", sender=sender, message_id=message_id,
        )]

    async def test_fresh_journal_deduplicates_only_the_same_sender_and_message(self):
        gateway = self.open_gateway()
        message_id = str(uuid4())
        await self.report("agent1alice", message_id)
        await self.report("agent1bob", message_id)
        await self.report("agent1alice", message_id)
        self.assertEqual(gateway._http.await_count, 2)

    async def test_existing_inline_unique_journal_accepts_another_sender_with_same_uuid(self):
        message_id = str(uuid4())
        with sqlite3.connect(self.path) as db:
            # Exact reports schema before 3ad7685: UNIQUE is a table constraint,
            # in addition to the separately named reports_message index.
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
            db.execute(
                "INSERT INTO reports VALUES(1, 'obs_original', 5, ?, 'agent1alice', ?, 'original report', 7)",
                (datetime.now(UTC).isoformat(), message_id),
            )
        gateway = self.open_gateway()
        await self.report("agent1alice", message_id)
        gateway._http.assert_not_awaited()
        replies = await self.report("agent1bob", message_id)
        self.assertEqual(gateway._http.await_count, 1,
                         "upgraded journals must remove the old message-only UNIQUE constraint")
        self.assertIn("Recorded as evidence", "\n".join(replies))
        original = gateway.db.execute("SELECT * FROM reports WHERE id=1").fetchone()
        self.assertEqual((original["observation_id"], original["visible_seq"]), ("obs_original", 7))

    async def test_confirmed_post_with_journal_error_never_claims_no_effect(self):
        gateway = self.open_gateway()
        with patch.object(gateway, "_set_observation", side_effect=sqlite3.OperationalError("review fault")):
            replies = await self.report("agent1alice", str(uuid4()))
        self.assertEqual(gateway._http.await_count, 1)
        self.assertIn("Recorded as evidence obs_review", "\n".join(replies))
        self.assertNotIn("Nothing was changed", "\n".join(replies))


if __name__ == "__main__":
    unittest.main()
