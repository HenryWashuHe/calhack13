"""Henry's Claude: independent checks for #132's claims, found by mutation testing its changed lines.
Same loopback backend fixture as test_acp_mission; no public network or live providers."""

from __future__ import annotations

import asyncio
import sqlite3
import time
from pathlib import Path
from unittest.mock import patch

from genesis_fetch_bridge import acp_mission
from genesis_fetch_bridge.acp_mission import MissionGateway

from test_acp_mission import COORD, MISSION, TOKEN, GatewayTests, config, event, state


class Review132(GatewayTests):
    def fresh(self, name, **extra):
        self.gateway.close()
        self.gateway = MissionGateway(config=config(**extra), url=self.url, token=TOKEN,
                                      identity="agent1test", db=str(Path(self.temp.name) / name))

    async def say(self, text, **ids):
        return [c async for c in self.gateway.respond(text, **ids)]

    def decision(self, seq, obs, verdict, summary, **extra):
        e = event(seq, "planner_decision", planner="p", model="fixture", observation_id=obs,
                  verdict=verdict, summary=summary, kernel_errors=[], **extra)
        e.update(mission_id=MISSION, causation_id=obs)
        return e

    async def test_another_observations_decision_is_not_relayed_during_follow(self):
        # The PR says other observations' decisions "are not relayed". The existing test only checks
        # the current decision still arrives.
        self.fresh("other.sqlite", follow_seconds=2)
        self.events = [event(1, "observation_recorded", observation_id="obs_old", source="m", summary="older")]

        async def later():
            await asyncio.sleep(0.15)
            self.events.append(self.decision(3, "obs_old", "no_change", "OLDER DECISION TEXT"))
            await asyncio.sleep(0.15)
            self.events.append(self.decision(4, "obs_1", "no_change", "current decision text"))
        task = asyncio.create_task(later())
        joined = "\n".join(await self.say("report the main hall is closed on Nov 14"))
        await task
        self.assertIn("current decision text", joined)
        self.assertNotIn("OLDER DECISION TEXT", joined)

    async def test_follow_ends_early_on_this_reports_no_change_decision(self):
        self.fresh("early.sqlite", follow_seconds=20)

        async def later():
            await asyncio.sleep(0.15)
            self.events.append(self.decision(3, "obs_1", "no_change", "nothing to change"))
        task = asyncio.create_task(later())
        start = time.monotonic()
        joined = "\n".join(await self.say("report the main hall is closed on Nov 14"))
        await task
        self.assertIn("nothing to change", joined)
        self.assertLess(time.monotonic() - start, 5, "follow must end on its own decision, not at follow_seconds")

    async def test_follow_ends_on_its_own_change_terminal_status_not_another_change(self):
        self.fresh("change.sqlite", follow_seconds=3)

        async def later():
            await asyncio.sleep(0.15)
            self.events.append(self.decision(3, "obs_1", "change_proposed", "needs an owner", change_id="chg_own"))
            await asyncio.sleep(0.15)
            self.events.append(event(4, "change_status_changed", change_id="chg_other", **{"from": "READY", "to": "COMMITTED"}))
        task = asyncio.create_task(later())
        start = time.monotonic()
        await self.say("report the main hall is closed on Nov 14")
        await task
        self.assertGreater(time.monotonic() - start, 2.5, "another change committing must not end this report's follow")

        self.fresh("change2.sqlite", follow_seconds=20)
        self.events = []

        async def own():
            await asyncio.sleep(0.15)
            self.events.append(self.decision(3, "obs_1", "change_proposed", "needs an owner", change_id="chg_own"))
            await asyncio.sleep(0.15)
            self.events.append(event(4, "change_status_changed", change_id="chg_own", **{"from": "READY", "to": "COMMITTED"}))
        task = asyncio.create_task(own())
        start = time.monotonic()
        await self.say("report the main hall is closed on Nov 15")
        await task
        self.assertLess(time.monotonic() - start, 5, "its own change committing ends the follow")

    async def test_status_with_nothing_new_says_so(self):
        self.fresh("quiet.sqlite", follow_seconds=0)
        await self.say("report the main hall is closed on Nov 14")
        self.events.clear()
        text = (await self.say("status"))[0]
        self.assertIn("Nothing new since your report", text)

    async def test_a_bounded_status_reply_says_there_is_more(self):
        self.fresh("more.sqlite", follow_seconds=0)
        await self.say("report the main hall is closed on Nov 14")
        self.events.extend(event(i, "job_dispatched", job_id=f"job_{i}", role_id=COORD, responsibility_id="rsp_1",
                                 epoch=1, task=f"Review candidate {i}: " + "facts " * 30) for i in range(2, 40))
        first = (await self.say("status"))[0]
        self.assertTrue(first.endswith("There is more. Ask status again to continue from here."), first[-200:])

    async def test_one_status_call_reads_at_most_the_page_bound(self):
        self.fresh("pagebound.sqlite", follow_seconds=0)
        await self.say("report the main hall is closed on Nov 14")
        self.events.extend(event(i, "job_status_changed", job_id=f"j{i}", **{"from": "SUBMITTING", "to": "RUNNING"})
                           for i in range(2, 2 + 200 * (acp_mission.STATUS_PAGES_PER_CALL + 2)))
        before = sum(c["path"].startswith("/api/v1/events/log") for c in self.calls)
        await self.say("status")
        reads = sum(c["path"].startswith("/api/v1/events/log") for c in self.calls) - before
        self.assertEqual(reads, acp_mission.STATUS_PAGES_PER_CALL)

    async def test_org_shows_the_installed_version(self):
        self.state = state(with_lead=True)
        self.state["capability_gaps"]["gap_1"]["status"] = {
            "state": "installed", "acquisition_id": "acq_1",
            "capability": {"capability_id": "room-matrix-reader", "version": "1.0.0"}, "installation_id": "ins_1"}
        text = "\n".join(await self.say("org"))
        self.assertIn("room-matrix-reader@1.0.0", text)

    async def test_redelivery_reply_distinguishes_confirmed_from_unconfirmed(self):
        self.fresh("redeliver.sqlite", follow_seconds=0)
        ids = dict(sender="agent1sender", message_id="m-1")
        await self.say("report the main hall is closed on Nov 14", **ids)
        again = "\n".join(await self.say("report the main hall is closed on Nov 14", **ids))
        self.assertIn("already recorded as evidence obs_1", again)
        self.observation_status = 500
        await self.say("report the side hall is closed on Nov 14", sender="agent1sender", message_id="m-2")
        posts = sum(c["method"] == "POST" for c in self.calls)
        again = "\n".join(await self.say("report the side hall is closed on Nov 14", sender="agent1sender", message_id="m-2"))
        self.assertIn("never confirmed", again)
        self.assertEqual(sum(c["method"] == "POST" for c in self.calls), posts)

    async def test_a_journal_from_before_this_change_migrates_and_keeps_redelivery_safe(self):
        self.gateway.close()
        path = Path(self.temp.name) / "legacy.sqlite"
        db = sqlite3.connect(path)
        db.executescript("""
            CREATE TABLE reports(id INTEGER PRIMARY KEY AUTOINCREMENT, observation_id TEXT, after_seq INTEGER NOT NULL, created_at TEXT NOT NULL);
            INSERT INTO reports(observation_id, after_seq, created_at) VALUES('obs_legacy', 7, '2026-10-20T00:00:00+00:00');
        """)
        db.commit()
        db.close()
        self.gateway = MissionGateway(config=config(follow_seconds=0), url=self.url, token=TOKEN,
                                      identity="agent1test", db=str(path))
        ids = dict(sender="agent1sender", message_id="m-legacy")
        await self.say("report the main hall is closed on Nov 14", **ids)
        await self.say("report the main hall is closed on Nov 14", **ids)
        self.assertEqual(sum(c["method"] == "POST" for c in self.calls), 1)


def load_tests(loader, tests, pattern):
    # Only this module's own tests; the imported fixture class's tests run in test_acp_mission.
    import unittest
    return unittest.TestSuite(Review132(n) for n in sorted(vars(Review132)) if n.startswith("test_"))
