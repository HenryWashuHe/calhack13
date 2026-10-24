"""ASI:One mission gateway: real loopback backend fixture, no public network or live providers."""

from __future__ import annotations

import asyncio
import contextlib
import io
import json
import os
import tempfile
import unittest
from datetime import UTC, datetime
from pathlib import Path
from unittest.mock import AsyncMock, MagicMock, patch
from uuid import uuid4

from aiohttp import web

from genesis_fetch_bridge import acp_mission
from genesis_fetch_bridge.acp_mission import MissionGateway, config_from_bytes, make_protocol
from genesis_fetch_bridge.wire import Invalid

TOKEN = "agent-secret-DO-NOT-SHOW-IN-CHAT"
MISSION = "mis_11111111111111111111111111111111"
COORD = "role_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
LEAD = "role_bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"


def config(**extra):
    value = {"public_demo": True, "evidence_ref": "https://portal.example/venues", "source": "asi-one", "max_reports_per_hour": 3, "follow_seconds": 1}
    value.update(extra)
    return value


def state(revision=1, with_lead=False):
    roles = {COORD: {"role_id": COORD, "name": "Mission coordinator", "charter": "Owns the plan", "state": "ACTIVE", "binding": {"provider": "internal", "provider_agent_id": "kernel:coordinator"}}}
    responsibilities = {"rsp_1": {"responsibility_id": "rsp_1", "purpose": "Coordinate the event plan", "owner": {"kind": "role", "id": COORD}, "status": "OPEN"}}
    if with_lead:
        roles[LEAD] = {"role_id": LEAD, "name": "Relocation lead", "charter": "Find a venue", "state": "ACTIVE", "binding": {"provider": "dev_stub", "provider_agent_id": "devstub-elastic-1"}}
        responsibilities["rsp_2"] = {"responsibility_id": "rsp_2", "purpose": "Relocate the event", "owner": {"kind": "role", "id": LEAD}, "status": "OPEN"}
    return {
        "org_id": "org_a", "name": "Org A · Campus Events", "revision": revision,
        "missions": {MISSION: {"mission_id": MISSION, "goal": "Run the campus hack night for 120 attendees", "status": "ACTIVE"}},
        "roles": roles, "responsibilities": responsibilities,
        "capability_gaps": {"gap_1": {"capability_id": "room-matrix-reader", "status": "REQUESTED"}} if with_lead else {},
    }


def event(seq, kind, **payload):
    return {"seq": seq, "type": kind, "revision": 1, "payload": payload}


EVOLUTION = [
    event(3, "observation_recorded", observation_id="obs_1", source="asi-one", summary="main hall closed"),
    event(4, "requirement_recorded", requirement_id="req_1", summary="Relocate the event"),
    event(5, "planner_decision", planner="genesis-planner", model="anthropic/claude-x", observation_id="obs_1", verdict="change_proposed", summary="The closure needs a relocation owner", kernel_errors=[]),
    event(6, "change_proposed", change_id="chg_1", operations=["CreateRole", "AssignResponsibility"], rationale="No existing role owns relocation"),
    event(7, "role_created", role_id=LEAD, name="Relocation lead", provider="elastic"),
    event(8, "role_provisioned", change_id="chg_1", role_key="lead", role_id=LEAD, provider="elastic", provider_agent_id="devstub-elastic-1", dev_stub=True),
    event(9, "change_status_changed", change_id="chg_1", **{"from": "READY", "to": "COMMITTED"}),
    event(10, "role_activated", role_id=LEAD),
    event(11, "responsibility_assigned", responsibility_id="rsp_2", owner=LEAD, epoch=1),
]


class GatewayTests(unittest.IsolatedAsyncioTestCase):
    async def asyncSetUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.calls = []
        self.events = []
        self.state = state()
        self.observation_status = 201
        self.hang_post = False
        app = web.Application()
        app.router.add_route("*", "/{tail:.*}", self.backend)
        self.runner = web.AppRunner(app, access_log=None)
        await self.runner.setup()
        site = web.TCPSite(self.runner, "127.0.0.1", 0)
        await site.start()
        self.url = f"http://127.0.0.1:{self.runner.addresses[0][1]}/api/v1"
        self.gateway = MissionGateway(config=config(), url=self.url, token=TOKEN, identity="agent1test", db=str(Path(self.temp.name) / "g.sqlite"))
        self.speed = patch.multiple(acp_mission, POLL_SECONDS=0.05, SETTLE_SECONDS=0.05)
        self.speed.start()

    async def asyncTearDown(self):
        self.speed.stop()
        self.gateway.close()
        await self.runner.cleanup()
        self.temp.cleanup()

    async def backend(self, request):
        raw = await request.read()
        self.calls.append({"method": request.method, "path": request.path_qs, "auth": request.headers.get("Authorization"), "body": json.loads(raw) if raw else None})
        if request.path == "/api/v1/organization":
            return web.json_response(self.state)
        if request.path == "/api/v1/events/log":
            after = int(request.query.get("after", "0"))
            return web.json_response([e for e in self.events if e["seq"] > after][:200])
        if request.path == f"/api/v1/missions/{MISSION}/observations" and request.method == "POST":
            if self.hang_post:
                await asyncio.sleep(30)
            if self.observation_status != 201:
                return web.json_response({"error": "forbidden", "message": "private-detail"}, status=self.observation_status)
            self.events.append(event(len(self.events) + 1, "observation_recorded", observation_id="obs_1", source=request.json and (await request.json())["source"], summary=(await request.json())["summary"]))
            return web.json_response({"observation_id": "obs_1", "revision": 2}, status=201)
        return web.json_response({"error": "not_found"}, status=404)

    async def collect(self, text):
        return [chunk async for chunk in self.gateway.respond(text)]

    def test_config_validation(self):
        config_from_bytes(json.dumps(config()).encode())
        for bad in (
            config(public_demo=False), config(evidence_ref="http://portal.example/x"), config(evidence_ref="javascript:alert(1)"),
            config(source="ASI One"), config(max_reports_per_hour=0), config(follow_seconds=121), {**config(), "extra": 1},
        ):
            with self.assertRaises(Invalid):
                config_from_bytes(json.dumps(bad).encode())
        with self.assertRaises(Invalid):
            MissionGateway(config=config(), url="http://example.com/api/v1", token=TOKEN, identity="a", db=str(Path(self.temp.name) / "x.sqlite"))

    async def test_help_and_unknown_text_never_call_backend(self):
        for text in ("help", "hello", "", "book me a venue now", "x" * 600):
            chunks = await self.collect(text)
            self.assertEqual(len(chunks), 1)
        self.assertEqual(self.calls, [])
        self.assertIn("report <what happened>", (await self.collect("help"))[0])

    async def test_org_summary_uses_agent_token_and_labels_stubs(self):
        self.state = state(with_lead=True)
        chunks = await self.collect("org")
        self.assertEqual(len(chunks), 1)
        text = chunks[0]
        self.assertIn("Run the campus hack night", text)
        self.assertIn("Mission coordinator (internal", text)
        self.assertIn("Relocation lead (dev_stub [dev stub]", text)
        self.assertIn("room-matrix-reader requested", text)
        self.assertEqual(self.calls[0]["auth"], "Bearer " + TOKEN)
        self.assertEqual([c["method"] for c in self.calls], ["GET"])

    async def test_report_records_evidence_and_relays_the_evolution(self):
        self.gateway.close()
        self.gateway = MissionGateway(config=config(follow_seconds=4), url=self.url, token=TOKEN, identity="agent1test", db=str(Path(self.temp.name) / "follow.sqlite"))
        async def planner():
            await asyncio.sleep(0.1)
            self.events.extend(EVOLUTION[1:])
            self.state = state(revision=2, with_lead=True)
        task = asyncio.create_task(planner())
        chunks = await self.collect("report: the main hall is closed for maintenance on Nov 14")
        await task
        post = next(c for c in self.calls if c["method"] == "POST")
        self.assertEqual(post["auth"], "Bearer " + TOKEN)
        self.assertEqual(post["body"], {"source": "asi-one", "summary": "the main hall is closed for maintenance on Nov 14", "evidence_ref": "https://portal.example/venues"})
        joined = "\n".join(chunks)
        self.assertIn("Recorded as evidence obs_1", chunks[0])
        self.assertIn("Planner (anthropic/claude-x) verdict change_proposed", joined)
        self.assertIn("Change proposed [CreateRole, AssignResponsibility]", joined)
        self.assertIn("[DEV STUB, not a live provider]", joined)
        self.assertIn("Change committed", joined)
        self.assertIn("Role activated: Relocation lead", joined)
        self.assertIn("Relocation lead is now accountable", joined)
        self.assertIn("Ask status later", chunks[-1])
        self.assertTrue(len(chunks) >= 3)

    async def test_status_reads_only_events_after_the_report(self):
        self.events = [event(1, "role_created", role_id=COORD, name="Mission coordinator", provider="internal")]
        self.assertIn("No report from this agent yet", (await self.collect("status"))[0])
        chunks = await self.collect("report main hall closed on Nov 14")
        self.assertIn("Recorded as evidence", chunks[0])
        self.events.extend([event(3, "planner_decision", planner="p", model="replay", observation_id="obs_1", verdict="no_change", summary="Not relevant to the mission", kernel_errors=[])])
        text = (await self.collect("status"))[0]
        self.assertIn("verdict no_change", text)
        self.assertNotIn("Mission coordinator", text)

    async def test_same_message_id_from_different_senders_are_distinct_reports(self):
        from uagents import Model
        from uagents_core.contrib.protocols.chat import ChatMessage, TextContent
        self.gateway.close()
        self.gateway = MissionGateway(config=config(follow_seconds=0), url=self.url, token=TOKEN, identity="agent1test", db=str(Path(self.temp.name) / "senders.sqlite"))
        shared_id = uuid4()
        handler = make_protocol(self.gateway).signed_message_handlers[Model.build_schema_digest(ChatMessage)]
        ctx = MagicMock(); ctx.send = AsyncMock()
        for sender in ("agent1alice", "agent1bob"):
            message = ChatMessage(timestamp=datetime.now(UTC), msg_id=shared_id, content=[TextContent(type="text", text="report the main hall is closed on Nov 14")])
            await handler(ctx, sender, message)
        self.assertEqual(sum(c["method"] == "POST" for c in self.calls), 2, "identity is sender plus message id")
        await handler(ctx, "agent1alice", ChatMessage(timestamp=datetime.now(UTC), msg_id=shared_id, content=[TextContent(type="text", text="report the main hall is closed on Nov 14")]))
        self.assertEqual(sum(c["method"] == "POST" for c in self.calls), 2, "the same sender redelivering is still suppressed")

    async def test_journal_failure_after_confirmed_post_keeps_effect_visible(self):
        import sqlite3
        with patch.object(self.gateway, "_set_observation", side_effect=sqlite3.OperationalError("disk I/O error")):
            chunks = await self.collect("report the main hall is closed on Nov 14")
        self.assertEqual(sum(c["method"] == "POST" for c in self.calls), 1)
        joined = "\n".join(chunks)
        self.assertIn("Recorded as evidence obs_1", joined)
        self.assertIn("could not journal", joined)
        self.assertNotIn("Nothing was changed", joined)

    async def test_rate_limit_and_refusals_never_echo_private_detail(self):
        self.observation_status = 403
        text = (await self.collect("report main hall closed on Nov 14"))[0]
        self.assertIn("refused the report (HTTP 403)", text)
        self.assertNotIn("private-detail", text)
        self.assertNotIn(TOKEN, text)
        self.observation_status = 201
        # The refused attempt counts: intent is journaled before every submission.
        for _ in range(2):
            await self.collect("report main hall closed on Nov 14 again")
        text = (await self.collect("report yet another closure report"))[0]
        self.assertIn("limited number of reports per hour", text)
        self.assertEqual(sum(1 for c in self.calls if c["method"] == "POST"), 3)

    async def test_unconfirmed_post_is_reported_not_repeated(self):
        self.hang_post = True
        with patch.object(acp_mission, "HTTP_TIMEOUT", 0.2):
            chunks = await self.collect("report main hall closed on Nov 14")
        self.assertIn("did not confirm the report", chunks[0])
        self.assertEqual(sum(1 for c in self.calls if c["method"] == "POST"), 1)

    async def test_busy_gateway_records_nothing(self):
        self.gateway.lock = asyncio.Lock()
        await self.gateway.lock.acquire()
        try:
            text = (await self.collect("report main hall closed on Nov 14"))[0]
        finally:
            self.gateway.lock.release()
        self.assertIn("nothing was recorded", text)
        self.assertEqual(self.calls, [])

    def test_journal_binds_configuration(self):
        db = str(Path(self.temp.name) / "bound.sqlite")
        MissionGateway(config=config(), url=self.url, token=TOKEN, identity="agent1test", db=db).close()
        with self.assertRaises(Invalid):
            MissionGateway(config=config(source="other"), url=self.url, token=TOKEN, identity="agent1test", db=db)


    async def test_confirmed_report_follow_failure_keeps_effect_visible(self):
        original_http = self.gateway._http
        async def failed_follow(method, path, **kwargs):
            if method == "GET" and path.startswith("/events/log") and any(c["method"] == "POST" for c in self.calls):
                raise Invalid("fixture follow read failed")
            return await original_http(method, path, **kwargs)
        with patch.object(self.gateway, "_http", side_effect=failed_follow):
            chunks = await self.collect("report the main hall is closed on Nov 14")
        self.assertEqual(sum(c["method"] == "POST" for c in self.calls), 1)
        self.assertIn("Recorded as evidence", chunks[0])
        self.assertNotIn("Nothing was changed by this message", "\n".join(chunks))

    async def test_same_acp_message_after_restart_records_one_observation(self):
        from uagents import Model
        from uagents_core.contrib.protocols.chat import ChatMessage, TextContent
        self.gateway.close()
        args = dict(config=config(follow_seconds=0), url=self.url, token=TOKEN,
                    identity="agent1test", db=str(Path(self.temp.name) / "replay.sqlite"))
        self.gateway = MissionGateway(**args)
        message = ChatMessage(timestamp=datetime.now(UTC), msg_id=uuid4(),
                              content=[TextContent(type="text", text="report the main hall is closed on Nov 14")])
        ctx = MagicMock()
        ctx.send = AsyncMock()
        digest = Model.build_schema_digest(ChatMessage)
        await make_protocol(self.gateway).signed_message_handlers[digest](ctx, "agent1sender", message)
        self.gateway.close()
        self.gateway = MissionGateway(**args)
        await make_protocol(self.gateway).signed_message_handlers[digest](ctx, "agent1sender", message)
        self.assertEqual(sum(c["method"] == "POST" for c in self.calls), 1,
                         "re-delivery of the same signed ACP message must not submit another observation")


    async def test_org_renders_native_tagged_capability_gap_status(self):
        # genesis-core::CapabilityGapStatus uses serde(tag = "state"), not a
        # string status. This is the public /organization wire representation.
        self.state = state(with_lead=True)
        self.state["capability_gaps"]["gap_1"]["status"] = {
            "state": "installed", "acquisition_id": "acq_1",
            "capability": {"capability_id": "room-matrix-reader", "version": "1.0.0"},
            "installation_id": "ins_1",
        }
        text = "\n".join(await self.collect("org"))
        self.assertIn("room-matrix-reader installed", text)

    def test_event_relay_renders_native_tagged_capability_gap_status(self):
        # EventKind::CapabilityGapStatusChanged embeds the same tagged enum.
        changed = event(7, "capability_gap_status_changed", gap_id="gap_1", **{
            "from": {"state": "open"},
            "to": {"state": "acquiring", "acquisition_id": "acq_1"},
        })
        text = acp_mission.describe(changed, acp_mission.RoleNames(state()))
        self.assertIn("acquiring", text)

    async def test_status_event_pagination_preserves_later_outcome(self):
        self.gateway.close()
        self.gateway = MissionGateway(
            config=config(follow_seconds=0), url=self.url, token=TOKEN,
            identity="agent1test", db=str(Path(self.temp.name) / "pages.sqlite"),
        )
        await self.collect("report the main hall is closed on Nov 14")
        # These are legitimate, unrendered intermediate job events, so the
        # first page stays below MAX_REPLY: this isolates the page boundary.
        self.events.extend(event(i, "job_status_changed", job_id=f"job_{i}", **{
            "from": "SUBMITTING", "to": "RUNNING",
        }) for i in range(2, 202))
        self.events.append(event(202, "installation_accepted", installation_id="ins_1", report_ref="proof:accepted"))
        replies = []
        for _ in range(5):
            replies.extend(await self.collect("status"))
        self.assertIn("Local acceptance checks passed", "\n".join(replies),
                      "bounded status reads must eventually expose outcomes beyond the first event page")

    async def test_status_reply_bound_does_not_permanently_discard_outcome(self):
        self.gateway.close()
        self.gateway = MissionGateway(
            config=config(follow_seconds=0), url=self.url, token=TOKEN,
            identity="agent1test", db=str(Path(self.temp.name) / "chunks.sqlite"),
        )
        await self.collect("report the main hall is closed on Nov 14")
        # Fewer than 200 events, but more than one chat reply of readable text.
        self.events.extend(event(i, "job_dispatched", job_id=f"job_{i}", role_id=COORD,
                                 responsibility_id="rsp_1", epoch=1,
                                 task=f"Review candidate {i}: " + "venue facts " * 12)
                           for i in range(2, 32))
        self.events.append(event(32, "installation_accepted", installation_id="ins_1", report_ref="proof:accepted"))
        replies = []
        for _ in range(5):
            replies.extend(await self.collect("status"))
        self.assertTrue(all(len(reply) <= acp_mission.MAX_REPLY for reply in replies))
        self.assertIn("Local acceptance checks passed", "\n".join(replies),
                      "reply truncation must leave later outcomes reachable in this or a subsequent status response")

    async def test_unrelated_planner_decision_does_not_end_current_report_follow(self):
        # The runner may still be processing an older observation when this
        # report arrives. Native decisions identify the observation explicitly.
        self.events = [event(1, "observation_recorded", observation_id="obs_old",
                             source="monitor", summary="An earlier venue update")]
        original_http = self.gateway._http
        follow_reads = 0

        async def interleaved_planner(method, path, **kwargs):
            nonlocal follow_reads
            response = await original_http(method, path, **kwargs)
            if method == "POST":
                older = event(3, "planner_decision", planner="p", model="fixture",
                              observation_id="obs_old", verdict="no_change",
                              summary="Earlier observation needs no change", kernel_errors=[])
                older.update(mission_id=MISSION, causation_id="obs_old")
                self.events.append(older)
            elif path.startswith("/events/log") and any(c["method"] == "POST" for c in self.calls):
                follow_reads += 1
                if follow_reads == 2:
                    # The current decision arrives just after an empty read,
                    # within follow_seconds. No wall-clock scheduling race.
                    current = event(4, "planner_decision", planner="p", model="fixture",
                                    observation_id="obs_1", verdict="no_change",
                                    summary="Current report independently evaluated", kernel_errors=[])
                    current.update(mission_id=MISSION, causation_id="obs_1")
                    self.events.append(current)
            return response

        with patch.object(self.gateway, "_http", side_effect=interleaved_planner):
            replies = await self.collect("report the main hall is closed on Nov 14")
        self.assertIn("Current report independently evaluated", "\n".join(replies),
                      "an older observation's decision must not terminate the current report's follow window")


class ProtocolTests(unittest.IsolatedAsyncioTestCase):
    def setUp(self):
        from uagents import Model
        from uagents_core.contrib.protocols.chat import ChatMessage
        self.gateway = MagicMock()
        async def respond(text, **_identity):
            yield "first"
            yield "second"
            yield "last"
        self.gateway.respond = respond
        self.protocol = make_protocol(self.gateway)
        self.handler = self.protocol.signed_message_handlers[Model.build_schema_digest(ChatMessage)]
        self.ctx = MagicMock()
        self.ctx.send = AsyncMock()

    def message(self, content):
        from uagents_core.contrib.protocols.chat import ChatMessage
        return ChatMessage(timestamp=datetime.now(UTC), msg_id=uuid4(), content=content)

    async def test_streams_progress_and_ends_session_once(self):
        from uagents_core.contrib.protocols.chat import ChatAcknowledgement, ChatMessage, EndSessionContent, StartSessionContent, TextContent
        msg = self.message([StartSessionContent(type="start-session"), TextContent(type="text", text="report hall closed")])
        await self.handler(self.ctx, "sender", msg)
        sent = [call.args[1] for call in self.ctx.send.call_args_list]
        self.assertIsInstance(sent[0], ChatAcknowledgement)
        self.assertEqual(sent[0].acknowledged_msg_id, msg.msg_id)
        replies = [m for m in sent[1:] if isinstance(m, ChatMessage)]
        self.assertEqual([m.content[0].text for m in replies], ["first", "second", "last"])
        self.assertFalse(any(isinstance(c, EndSessionContent) for m in replies[:-1] for c in m.content))
        self.assertIsInstance(replies[-1].content[-1], EndSessionContent)
        self.assertTrue(self.protocol.verify())
        self.assertEqual(self.protocol.name, "AgentChatProtocol")

    async def test_split_text_and_attachments_never_reach_gateway(self):
        from uagents_core.contrib.protocols.chat import ResourceContent, TextContent
        self.gateway.respond = MagicMock(side_effect=AssertionError("must not be called"))
        await self.handler(self.ctx, "sender", self.message([TextContent(type="text", text="report "), TextContent(type="text", text="x")]))
        item = ResourceContent(type="resource", resource_id=uuid4(), resource={"uri": "https://example.com", "metadata": {}})
        await self.handler(self.ctx, "sender", self.message([TextContent(type="text", text="report hall closed"), item]))

    async def test_gateway_failure_is_generic(self):
        async def failing(_text, **_identity):
            raise Invalid("secret agent token value")
            yield  # pragma: no cover
        self.gateway.respond = failing
        from uagents_core.contrib.protocols.chat import TextContent
        await self.handler(self.ctx, "sender", self.message([TextContent(type="text", text="org")]))
        last = self.ctx.send.call_args_list[-1].args[1]
        self.assertNotIn("secret", last.content[0].text)


class CliTests(unittest.TestCase):
    def test_identity_does_not_construct_agent_or_open_database(self):
        with patch.dict(os.environ, {"GENESIS_ACP_MISSION_SEED": "b" * 32}, clear=True), patch("sys.argv", ["acp", "identity"]), patch("uagents.Agent") as agent, patch("genesis_fetch_bridge.acp_mission.MissionGateway") as gateway, contextlib.redirect_stdout(io.StringIO()) as output:
            acp_mission.main()
        self.assertTrue(output.getvalue().strip().startswith("agent1"))
        agent.assert_not_called()
        gateway.assert_not_called()

    def test_public_runner_uses_standard_protocol_and_loopback_inspector(self):
        import uagents.asgi
        from uagents.registration import AlmanacApiRegistrationPolicy
        with tempfile.TemporaryDirectory() as root:
            cfg = Path(root) / "mission.json"
            readme = Path(root) / "agent.md"
            cfg.write_text(json.dumps(config()))
            readme.write_text("GENESIS organization agent; no live proof claimed.")
            env = {"GENESIS_ACP_MISSION_SEED": "b" * 32, "GENESIS_ACP_BACKEND_URL": "http://127.0.0.1:7101/api/v1", "GENESIS_ACP_AGENT_TOKEN": "org_a-agent-token"}
            argv = ["acp", "serve-public", "--config", str(cfg), "--readme", str(readme), "--db", str(Path(root) / "g.sqlite")]
            with patch.dict(os.environ, env, clear=True), patch("sys.argv", argv), patch("uagents.Agent") as factory, patch.object(uagents.asgi, "HOST", "0.0.0.0"):
                acp_mission.main()
                self.assertEqual(uagents.asgi.HOST, "127.0.0.1")
                kwargs = factory.call_args.kwargs
                self.assertTrue(kwargs["mailbox"])
                self.assertIsInstance(kwargs["registration_policy"], AlmanacApiRegistrationPolicy)
                self.assertEqual(kwargs["port"], 8104)
                protocol = factory.return_value.include.call_args.args[0]
                self.assertEqual(protocol.version, "0.3.0")
                factory.return_value.run.assert_called_once()

    def test_invalid_configuration_never_constructs_public_agent(self):
        with patch.dict(os.environ, {"GENESIS_ACP_MISSION_SEED": "short"}, clear=True), patch("sys.argv", ["acp", "serve-public", "--config", "absent", "--readme", "absent", "--db", "absent"]), patch("uagents.Agent") as agent, contextlib.redirect_stderr(io.StringIO()):
            with self.assertRaises(SystemExit) as caught:
                acp_mission.main()
        self.assertEqual(caught.exception.code, 2)
        agent.assert_not_called()


if __name__ == "__main__":
    unittest.main()
