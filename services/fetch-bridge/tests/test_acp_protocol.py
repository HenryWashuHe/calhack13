"""ACP schema/handler and explicit public-startup boundary; no public network."""

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

from uagents import Model
from uagents_core.contrib.protocols.chat import (
    ChatAcknowledgement, ChatMessage, EndSessionContent, StartSessionContent, TextContent,
)

from genesis_fetch_bridge.acp import main, make_protocol
from genesis_fetch_bridge.wire import Invalid


class ProtocolTests(unittest.IsolatedAsyncioTestCase):
    def setUp(self):
        self.gateway = MagicMock()
        self.gateway.respond = AsyncMock(return_value="backend progress")
        self.protocol = make_protocol(self.gateway)
        self.handler = self.protocol.signed_message_handlers[Model.build_schema_digest(ChatMessage)]
        self.ctx = MagicMock()
        self.ctx.send = AsyncMock()

    def message(self, content):
        return ChatMessage(timestamp=datetime.now(UTC), msg_id=uuid4(), content=content)

    async def test_ack_precedes_action_and_response_ends_session(self):
        order = []
        async def send(_sender, message):
            order.append(type(message).__name__)
        async def respond(text):
            order.append(text)
            return "backend progress"
        self.ctx.send.side_effect = send
        self.gateway.respond.side_effect = respond
        msg = self.message([StartSessionContent(type="start-session"), TextContent(type="text", text="recover demo")])
        await self.handler(self.ctx, "authenticated-sender", msg)
        self.assertEqual(order, ["ChatAcknowledgement", "recover demo", "ChatMessage"])
        self.assertEqual(self.ctx.send.call_args_list[0].args[1].acknowledged_msg_id, msg.msg_id)
        reply = self.ctx.send.call_args_list[1].args[1]
        self.assertEqual(reply.content[0].text, "backend progress")
        self.assertIsInstance(reply.content[-1], EndSessionContent)
        self.assertTrue(self.protocol.verify())
        self.assertFalse(self.protocol.unsigned_message_handlers)

    async def test_multiple_text_parts_cannot_form_a_command(self):
        msg = self.message([TextContent(type="text", text="recover "), TextContent(type="text", text="demo")])
        await self.handler(self.ctx, "sender", msg)
        self.gateway.respond.assert_not_awaited()
        self.assertEqual(self.ctx.send.await_count, 2)

    async def test_end_session_alone_never_cancels_work(self):
        await self.handler(self.ctx, "sender", self.message([EndSessionContent(type="end-session")]))
        self.gateway.respond.assert_not_awaited()

    async def test_unsupported_content_never_calls_backend(self):
        # Unknown content is normally rejected by model parsing. Use an SDK
        # resource item here to exercise a valid non-text ACP payload.
        from uagents_core.contrib.protocols.chat import ResourceContent
        item = ResourceContent(type="resource", resource_id=uuid4(), resource={"uri": "https://example.com", "metadata": {}})
        msg = self.message([TextContent(type="text", text="recover demo"), item])
        await self.handler(self.ctx, "sender", msg)
        self.gateway.respond.assert_not_awaited()

    async def test_sensitive_failure_is_never_echoed(self):
        self.gateway.respond.side_effect = Invalid("secret operator token")
        await self.handler(self.ctx, "sender", self.message([TextContent(type="text", text="recover demo")]))
        self.assertNotIn("secret operator token", self.ctx.send.call_args_list[-1].args[1].content[0].text)

    async def test_acknowledgement_has_no_domain_effect(self):
        ack_handler = self.protocol.signed_message_handlers[Model.build_schema_digest(ChatAcknowledgement)]
        await ack_handler(self.ctx, "sender", ChatAcknowledgement(timestamp=datetime.now(UTC), acknowledged_msg_id=uuid4()))
        self.gateway.respond.assert_not_awaited()
        self.ctx.send.assert_not_awaited()


class CliTests(unittest.TestCase):
    def test_identity_does_not_construct_agent_or_open_database(self):
        with patch.dict(os.environ, {"GENESIS_ACP_SEED": "a" * 32}, clear=True), patch("sys.argv", ["acp", "identity"]), patch("uagents.Agent") as agent, patch("genesis_fetch_bridge.acp.RecoveryGateway") as gateway, contextlib.redirect_stdout(io.StringIO()) as output:
            main()
        self.assertTrue(output.getvalue().strip().startswith("agent1"))
        agent.assert_not_called()
        gateway.assert_not_called()

    def test_public_runner_uses_standard_protocol_and_loopback_inspector(self):
        import uagents.asgi
        from uagents.registration import AlmanacApiRegistrationPolicy
        with tempfile.TemporaryDirectory() as root:
            cases = Path(root) / "cases.json"
            readme = Path(root) / "agent.md"
            cases.write_text(json.dumps({"public_demo": True, "cases": {"demo": {"client_request_id": "demo-recovery", "reopened_assessment_id": "assessment-id", "candidate_job_ids": ["job-one"], "deadline": "2026-10-24T00:00:00Z"}}}))
            readme.write_text("GENESIS public demo; no live proof claimed.")
            env = {"GENESIS_ACP_SEED": "a" * 32, "GENESIS_ACP_BACKEND_URL": "http://127.0.0.1:4100/api/v1", "GENESIS_ACP_OPERATOR_TOKEN": "private-operator-token"}
            argv = ["acp", "serve-public", "--cases", str(cases), "--readme", str(readme), "--db", str(Path(root) / "gateway.sqlite")]
            with patch.dict(os.environ, env, clear=True), patch("sys.argv", argv), patch("uagents.Agent") as factory, patch.object(uagents.asgi, "HOST", "0.0.0.0"):
                main()
                self.assertEqual(uagents.asgi.HOST, "127.0.0.1")
                kwargs = factory.call_args.kwargs
                self.assertTrue(kwargs["mailbox"])
                self.assertTrue(kwargs["enable_agent_inspector"])
                self.assertFalse(kwargs["report_events"])
                self.assertFalse(kwargs["store_message_history"])
                self.assertTrue(kwargs["handle_messages_concurrently"])
                self.assertIsInstance(kwargs["registration_policy"], AlmanacApiRegistrationPolicy)
                self.assertEqual(kwargs["port"], 8103)
                protocol = factory.return_value.include.call_args.args[0]
                self.assertEqual(protocol.name, "AgentChatProtocol")
                self.assertEqual(protocol.version, "0.3.0")
                self.assertTrue(factory.return_value.include.call_args.kwargs["publish_manifest"])
                factory.return_value.run.assert_called_once()

    def test_invalid_configuration_never_constructs_public_agent(self):
        with patch.dict(os.environ, {"GENESIS_ACP_SEED": "too-short"}, clear=True), patch("sys.argv", ["acp", "serve-public", "--cases", "absent", "--readme", "absent", "--db", "absent"]), patch("uagents.Agent") as agent, contextlib.redirect_stderr(io.StringIO()):
            with self.assertRaises(SystemExit) as caught:
                main()
        self.assertEqual(caught.exception.code, 2)
        agent.assert_not_called()


if __name__ == "__main__":
    unittest.main()
