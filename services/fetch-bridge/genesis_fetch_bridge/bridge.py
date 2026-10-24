"""A bounded aiohttp boundary around native Fetch signed envelopes."""

from __future__ import annotations

import asyncio
import base64
import hmac
import sqlite3
from urllib.parse import urlsplit
from uuid import uuid4

from aiohttp import ClientError, ClientSession, ClientTimeout, web
from uagents_core.envelope import Envelope
from uagents_core.identity import Identity

from .specialist import Specialist
from .store import Capacity, Conflict, Store
from .wire import (
    MAX_MESSAGE,
    PROTOCOL_DIGEST,
    SCHEMA_DIGEST,
    GenesisMessage,
    Invalid,
    address,
    canonical,
    decode,
    exact,
    timestamp,
    validate,
)

MAX_HTTP = 256 * 1024


def respond(value, *, status=200):
    return web.json_response(value, status=status, dumps=canonical)


def peer_url(value: str) -> str:
    try:
        parsed = urlsplit(value)
        if (
            parsed.username
            or parsed.password
            or parsed.query
            or parsed.fragment
            or parsed.path != "/submit"
        ):
            raise Invalid("peer URL must identify /submit without credentials")
        if parsed.scheme != "https" and not (
            parsed.scheme == "http" and parsed.hostname in ("127.0.0.1", "::1")
        ):
            raise Invalid("peer URL requires HTTPS or literal loopback")
        if not parsed.hostname or parsed.port == 0:
            raise Invalid("invalid peer URL")
        return value
    except (ValueError, AttributeError) as exc:
        raise Invalid("invalid peer URL") from exc


def validate_peers(peers) -> dict[str, str]:
    if not isinstance(peers, dict) or len(peers) > 128:
        raise Invalid("invalid peers")
    return {address(key): peer_url(value) for key, value in peers.items()}


@web.middleware
async def errors(request, handler):
    try:
        return await handler(request)
    except Conflict:
        return respond({"error": "conflicting receipt"}, status=409)
    except Capacity:
        return respond({"error": "storage capacity reached"}, status=503)
    except (
        Invalid,
        UnicodeError,
        ValueError,
        TypeError,
        OverflowError,
        RecursionError,
    ):
        return respond({"error": "invalid request"}, status=400)
    except sqlite3.Error:
        return respond({"error": "storage unavailable"}, status=503)


class Bridge:
    def __init__(
        self,
        *,
        seed: str,
        token: str,
        db: str,
        peers: dict[str, str],
        specialist: bool = False,
        specialist_mode: str | None = None,
        max_records: int = 10000,
        max_bytes: int = 64 * 1024 * 1024,
        http_timeout: float = 5,
    ):
        if (
            not isinstance(seed, str)
            or len(seed.encode()) < 32
            or len(seed.encode()) > 4096
        ):
            raise Invalid("seed must contain 32..4096 bytes")
        if (
            not isinstance(token, str)
            or not 32 <= len(token) <= 512
            or not token.isascii()
            or any(c.isspace() for c in token)
        ):
            raise Invalid(
                "bridge token must contain 32..512 non-whitespace ASCII characters"
            )
        if not 0.1 <= http_timeout <= 30:
            raise Invalid("invalid HTTP timeout")
        self.identity = Identity.from_seed(seed, 0)
        self.token = token
        self.peers = validate_peers(peers)
        self.store = Store(db, self.identity.address, max_records, max_bytes)
        self.specialist = (
            Specialist(self.store, "advisory" if specialist_mode is None else specialist_mode)
            if specialist or specialist_mode is not None
            else None
        )
        self.http_timeout = http_timeout
        self.app = web.Application(
            middlewares=[errors, self.authenticate], client_max_size=MAX_HTTP
        )
        self.app.add_routes(
            [
                web.get("/v1/identity", self.get_identity),
                web.post("/v1/messages", self.queue),
                web.get("/v1/inbox", self.inbox),
                web.post("/submit", self.submit),
            ]
        )
        self.app.cleanup_ctx.append(self.lifecycle)

    @web.middleware
    async def authenticate(self, request, handler):
        if request.path.startswith("/v1/"):
            supplied = request.headers.getall("Authorization", [])
            if len(supplied) != 1 or not hmac.compare_digest(
                supplied[0], "Bearer " + self.token
            ):
                return respond({"error": "unauthorized"}, status=401)
        if request.headers.get("Content-Encoding"):
            raise Invalid("compressed request bodies are not accepted")
        return await handler(request)

    async def body(self, request):
        if request.content_type != "application/json":
            raise Invalid("expected JSON content type")
        data = bytearray()
        try:
            async with asyncio.timeout(self.http_timeout):
                async for chunk in request.content.iter_chunked(16384):
                    data.extend(chunk)
                    if len(data) > MAX_HTTP:
                        raise Invalid("body exceeds limit")
        except TimeoutError as exc:
            raise Invalid("request read timeout") from exc
        # Private requests wrap Message by one level; Message validation owns its
        # separate root-relative depth64 limit after this bounded HTTP parse.
        return decode(bytes(data), MAX_HTTP, max_depth=128)

    async def get_identity(self, _request):
        return respond(
            {
                "schema_version": 1,
                "address": self.identity.address,
                "protocol_digest": PROTOCOL_DIGEST,
            }
        )

    async def queue(self, request):
        data = exact(
            await self.body(request), {"schema_version", "recipient", "message"}
        )
        if type(data["schema_version"]) is not int or data["schema_version"] != 1:
            raise Invalid("unsupported schema")
        recipient = address(data["recipient"])
        if recipient not in self.peers:
            raise Invalid("unconfigured recipient")
        self.store.queue(recipient, data["message"])
        return respond(
            {
                "schema_version": 1,
                "message_id": data["message"]["message_id"],
                "bridge_address": self.identity.address,
                "recipient": recipient,
                "state": "queued",
            },
            status=202,
        )

    async def inbox(self, request):
        if set(request.query) - {"after", "limit"} or any(
            len(request.query.getall(k)) != 1 for k in request.query
        ):
            raise Invalid("invalid inbox parameters")
        raw_after, raw_limit = (
            request.query.get("after", "0"),
            request.query.get("limit", "50"),
        )
        if (
            not raw_after.isascii()
            or not raw_after.isdigit()
            or not raw_limit.isascii()
            or not raw_limit.isdigit()
        ):
            raise Invalid("invalid inbox cursor")
        after, limit = int(raw_after), int(raw_limit)
        if not 0 <= after <= 2**63 - 1 or not 1 <= limit <= 50:
            raise Invalid("invalid inbox cursor")
        return respond(self.store.inbox(after, limit))

    async def submit(self, request):
        raw = await self.body(request)
        allowed = {
            "version",
            "sender",
            "target",
            "session",
            "schema_digest",
            "protocol_digest",
            "payload",
            "expires",
            "nonce",
            "signature",
        }
        required = allowed - {"protocol_digest", "nonce"}
        if not isinstance(raw, dict) or set(raw) - allowed or not required <= set(raw):
            raise Invalid("invalid envelope fields")
        if (
            type(raw["version"]) is not int
            or raw["version"] != 1
            or type(raw["expires"]) is not int
            or raw["expires"] < 0
        ):
            raise Invalid("invalid envelope version or expiry")
        if raw["schema_digest"] != SCHEMA_DIGEST or raw.get("protocol_digest") not in (
            None,
            PROTOCOL_DIGEST,
        ):
            raise Invalid("unsupported schema")
        sender = address(raw["sender"])
        if sender not in self.peers or raw["target"] != self.identity.address:
            raise Invalid("unexpected sender or target")
        if raw.get("nonce") is not None and (
            type(raw["nonce"]) is not int or not 0 <= raw["nonce"] <= 2**64 - 1
        ):
            raise Invalid("invalid nonce")
        try:
            envelope = Envelope.model_validate(raw)
            if not envelope.verify():
                raise Invalid("invalid signature")
            if not isinstance(envelope.payload, str):
                raise Invalid("missing payload")
            payload = base64.b64decode(envelope.payload, validate=True)
            typed = exact(decode(payload, 2 * MAX_MESSAGE + 512), {"wire_json"})
            if not isinstance(typed["wire_json"], str):
                raise Invalid("invalid wire payload")
            message = validate(decode(typed["wire_json"]), fresh=False)
        except Exception as exc:
            raise Invalid("invalid signed payload") from exc
        if envelope.expires > timestamp(message["expires_at"]).timestamp():
            raise Invalid("transport expiry exceeds message expiry")
        self.store.receive(sender, message, envelope.expires)
        # Persisted signed receipt only. Specialist/kernel work runs after this acknowledgement.
        return respond({"received": True})

    async def lifecycle(self, _app):
        session = ClientSession(
            timeout=ClientTimeout(total=self.http_timeout),
            trust_env=False,
            auto_decompress=False,
        )
        task = asyncio.create_task(self.run_worker(session))
        yield
        task.cancel()
        try:
            await task
        except asyncio.CancelledError:
            pass
        await session.close()
        self.store.close()

    async def run_worker(self, session):
        while True:
            try:
                if self.specialist:
                    for _ in range(10):
                        if not self.specialist.step():
                            break
                row = self.store.next_outgoing()
                if row:
                    await self.send_one(session, row)
            except (Invalid, sqlite3.Error):
                # Preserve evidence and retry after explicit capacity/configuration repair.
                pass
            await asyncio.sleep(0.05)

    async def send_one(self, session, row):
        message = decode(row["payload"])
        try:
            validate(message)
        except Invalid:
            # A queued task can reach its deadline before its enclosing message expires.
            self.store.expire(row["message_id"])
            return
        recipient = row["recipient"]
        if recipient not in self.peers:
            self.store.failed_attempt(row["message_id"])
            return
        env = Envelope(
            version=1,
            sender=self.identity.address,
            target=recipient,
            session=uuid4(),
            schema_digest=SCHEMA_DIGEST,
            protocol_digest=PROTOCOL_DIGEST,
            expires=int(timestamp(message["expires_at"]).timestamp()),
        )
        env.encode_payload(
            GenesisMessage(wire_json=canonical(message)).model_dump_json()
        )
        env.sign(self.identity)
        try:
            async with session.post(
                self.peers[recipient],
                data=env.model_dump_json(),
                headers={"Content-Type": "application/json"},
                allow_redirects=False,
            ) as response:
                response_bytes = 0
                async for chunk in response.content.iter_chunked(4096):
                    response_bytes += len(chunk)
                    if response_bytes > 4096:
                        raise Invalid("response exceeds limit")
                if response.status == 200:
                    self.store.delivered(row["message_id"])
                    return
        except (ClientError, TimeoutError, Invalid, OSError):
            pass
        # The request may have arrived. Retry only the immutable application ID/payload.
        self.store.failed_attempt(row["message_id"])
