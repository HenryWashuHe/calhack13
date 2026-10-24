"""Henry's Claude: reproduction for #132. A journal row written before #132 has visible_seq 0 after migration,
so a direct caller's status replays events from before that report. Loopback fixture only."""

from __future__ import annotations

import sqlite3
import unittest
from pathlib import Path

from genesis_fetch_bridge.acp_mission import MissionGateway

from test_acp_mission import TOKEN, config
from test_acp_mission_review132 import Review132


class LegacyCursor(Review132):
    async def test_a_legacy_report_row_does_not_replay_history_from_before_it(self):
        # A pre-change row has visible_seq 0 by migration default, while new rows start at after_seq.
        # A direct caller (no sender) asking status then reads events from before its report.
        self.gateway.close()
        path = Path(self.temp.name) / "legacy2.sqlite"
        db = sqlite3.connect(path)
        db.executescript("""
            CREATE TABLE reports(id INTEGER PRIMARY KEY AUTOINCREMENT, observation_id TEXT, after_seq INTEGER NOT NULL, created_at TEXT NOT NULL);
            INSERT INTO reports(observation_id, after_seq, created_at) VALUES('obs_legacy', 5, '2026-10-20T00:00:00+00:00');
        """)
        db.commit()
        db.close()
        self.gateway = MissionGateway(config=config(follow_seconds=0), url=self.url, token=TOKEN,
                                      identity="agent1test", db=str(path))
        self.events = [self.decision(3, "obs_before", "no_change", "BEFORE THE LEGACY REPORT"),
                       self.decision(6, "obs_legacy", "no_change", "after the legacy report")]
        text = "\n".join(await self.say("status"))
        self.assertIn("after the legacy report", text)
        self.assertNotIn("BEFORE THE LEGACY REPORT", text)


def load_tests(loader, tests, pattern):
    return unittest.TestSuite([LegacyCursor("test_a_legacy_report_row_does_not_replay_history_from_before_it")])
