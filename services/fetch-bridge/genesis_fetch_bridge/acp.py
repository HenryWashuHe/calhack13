"""Opt-in Agent Chat Protocol facade for preauthorized public recovery demos.

Importing this module never creates an Agent or registers on a public service.
The private signed GenesisMessage transport remains a separate process/identity.
"""

from __future__ import annotations

import argparse
import os
import sqlite3
import sys
from datetime import UTC, datetime
from pathlib import Path
from uuid import uuid4

from uagents import Protocol
from uagents_core.contrib.protocols.chat import (
    ChatAcknowledgement,
    ChatMessage,
    EndSessionContent,
    StartSessionContent,
    TextContent,
    chat_protocol_spec,
)
from uagents_core.identity import Identity

from .acp_recovery import RecoveryGateway, config_from_bytes
from .wire import Invalid


def make_protocol(gateway: RecoveryGateway) -> Protocol:
    protocol = Protocol(spec=chat_protocol_spec)

    @protocol.on_message(ChatMessage)
    async def handle_message(ctx, sender: str, msg: ChatMessage):
        # SDK authenticated message dispatch is the transport boundary. Public
        # sender identity does not confer an operator grant; only frozen cases do.
        await ctx.send(sender, ChatAcknowledgement(
            timestamp=datetime.now(UTC), acknowledged_msg_id=msg.msg_id,
        ))
        texts = [item.text for item in msg.content if isinstance(item, TextContent)]
        supported = all(isinstance(item, (TextContent, StartSessionContent, EndSessionContent)) for item in msg.content)
        # Do not concatenate chunks: that could turn malformed/partial messages
        # or content beside an attachment into an executable command.
        if not supported or len(texts) != 1 or len(msg.content) > 3:
            response = "Send one text command: help, recover <case>, or status <case>. No attachments are processed."
        else:
            try:
                response = await gateway.respond(texts[0])
            except (Invalid, sqlite3.Error, UnicodeError):
                response = "Gateway unavailable. Existing requests retain their identities; operator reconciliation is required."
        await ctx.send(sender, ChatMessage(
            timestamp=datetime.now(UTC), msg_id=uuid4(),
            content=[TextContent(type="text", text=response), EndSessionContent(type="end-session")],
        ))

    @protocol.on_message(ChatAcknowledgement)
    async def handle_ack(_ctx, _sender: str, _msg: ChatAcknowledgement):
        # ACP receipt is not domain completion, cancellation, or an action.
        pass

    return protocol


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    commands.add_parser("identity", help="Print only the public ACP identity, with no network")
    serve = commands.add_parser("serve-public", help="Explicitly register and serve an ACP public demo agent")
    serve.add_argument("--cases", required=True, help="Private JSON file declaring public_demo:true and frozen cases")
    serve.add_argument("--db", required=True, help="Separate persistent ACP configuration/recovery journal")
    serve.add_argument("--port", type=int, default=8103, help="Loopback-only SDK Inspector/bootstrap port")
    serve.add_argument("--name", default="GENESIS Venue Recovery")
    serve.add_argument("--readme", required=True, help="Agent README published to Agentverse; inspect it before use")
    args = parser.parse_args()
    gateway = None
    try:
        seed = os.environ.get("GENESIS_ACP_SEED", "")
        if not 32 <= len(seed.encode()) <= 4096:
            raise Invalid("GENESIS_ACP_SEED must contain 32..4096 bytes")
        identity = Identity.from_seed(seed, 0).address
        if args.command == "identity":
            print(identity)
            return
        if not 1 <= args.port <= 65535:
            raise Invalid("port must be in 1..65535")
        with Path(args.cases).open("rb") as stream:
            config = config_from_bytes(stream.read(32769))
        readme = Path(args.readme).read_text(encoding="utf-8")
        if not readme.strip() or len(readme.encode()) > 32768:
            raise Invalid("invalid agent README")
        gateway = RecoveryGateway(
            config=config,
            url=os.environ.get("GENESIS_ACP_BACKEND_URL", ""),
            token=os.environ.get("GENESIS_ACP_OPERATOR_TOKEN", ""),
            identity=identity, db=args.db,
        )
        # Import and construct only on the explicitly public execution path.
        from uagents import Agent
        import uagents.asgi
        from uagents.registration import AlmanacApiRegistrationPolicy

        # uAgents 0.26.1 has no host constructor argument. Its ASGI server reads
        # this module constant at startup. Keep Inspector for mailbox bootstrap
        # and a long-lived server task, but expose it only on loopback. This is
        # deliberately version-specific; recheck when upgrading the pinned SDK.
        uagents.asgi.HOST = "127.0.0.1"
        agent = Agent(
            name=args.name, seed=seed, mailbox=True, port=args.port,
            registration_policy=AlmanacApiRegistrationPolicy(),
            enable_agent_inspector=True, publish_agent_details=True,
            store_message_history=False, report_events=False,
            readme_path=str(Path(args.readme).resolve()),
        )
        agent.include(make_protocol(gateway), publish_manifest=True)
        agent.run()
    except (Invalid, OSError, UnicodeError, sqlite3.Error):
        print("ACP configuration or journal invalid. Check its seed, backend token, fixed cases, README and database binding.", file=sys.stderr)
        raise SystemExit(2) from None
    finally:
        if gateway is not None:
            gateway.close()


if __name__ == "__main__":
    main()
