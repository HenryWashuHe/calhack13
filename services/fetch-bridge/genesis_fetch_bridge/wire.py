"""Strict GENESIS wire v1 validation, independent of provider metadata."""

from __future__ import annotations

import json
import re
from datetime import UTC, datetime
from typing import Any
from uuid import UUID

from bech32 import bech32_decode, convertbits
from ecdsa import SECP256k1, VerifyingKey
from pydantic.v1 import StrictStr
from uagents import Model, Protocol

MAX_MESSAGE = 64 * 1024
MAX_VALUE = 32 * 1024
_ID = re.compile(r"^[A-Za-z0-9_.:-]{1,128}$")
_TIME = re.compile(r"^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(?:\.\d{1,9})?(?:Z|\+00:00)$")


class Invalid(ValueError):
    """Invalid untrusted input; errors intentionally do not echo its contents."""


class GenesisMessage(Model):
    wire_json: StrictStr

    class Config:
        extra = "forbid"


SCHEMA_DIGEST = Model.build_schema_digest(GenesisMessage)
_PROTOCOL = Protocol(name="genesis-specialist", version="1.0.0")


@_PROTOCOL.on_message(model=GenesisMessage)
async def _schema_handler(_ctx, _sender, _message):
    """Schema registration only; no Agent or automatic network tasks are created."""


PROTOCOL_DIGEST = _PROTOCOL.digest


def canonical(value: Any) -> str:
    try:
        return json.dumps(
            value,
            ensure_ascii=False,
            sort_keys=True,
            separators=(",", ":"),
            allow_nan=False,
        )
    except (ValueError, TypeError, RecursionError, OverflowError) as exc:
        raise Invalid("invalid JSON value") from exc


def decode(raw: bytes | str, limit: int = MAX_MESSAGE) -> Any:
    if len(raw.encode() if isinstance(raw, str) else raw) > limit:
        raise Invalid("JSON exceeds limit")

    def pairs(items):
        result = {}
        for key, value in items:
            if key in result:
                raise Invalid("duplicate JSON field")
            result[key] = value
        return result

    def constant(_):
        raise Invalid("non-finite JSON value")

    def compatible(value, depth=0):
        # Python otherwise accepts integers and nesting the Rust inbox cannot retain.
        if depth > 64:
            raise Invalid("JSON nesting exceeds limit")
        if type(value) is int and not -(2**63) <= value <= 2**64 - 1:
            raise Invalid("JSON integer exceeds interoperable range")
        if isinstance(value, dict):
            for item in value.values():
                compatible(item, depth + 1)
        elif isinstance(value, list):
            for item in value:
                compatible(item, depth + 1)

    try:
        value = json.loads(raw, object_pairs_hook=pairs, parse_constant=constant)
        compatible(value)
        canonical(
            value
        )  # Also rejects exponent overflow to infinity and surrogate encoding below.
        canonical(value).encode("utf-8")
        return value
    except (ValueError, UnicodeError, TypeError, RecursionError, OverflowError) as exc:
        raise Invalid("invalid JSON") from exc


def exact(value: Any, keys: set[str]) -> dict:
    if not isinstance(value, dict) or set(value) != keys:
        raise Invalid("unexpected object fields")
    return value


def ident(value: Any) -> str:
    if not isinstance(value, str) or not _ID.fullmatch(value):
        raise Invalid("invalid domain identifier")
    return value


def uuid(value: Any) -> str:
    try:
        if not isinstance(value, str) or str(UUID(value)) != value:
            raise Invalid("invalid message identifier")
        return value
    except (ValueError, AttributeError) as exc:
        raise Invalid("invalid message identifier") from exc


def timestamp(value: Any) -> datetime:
    try:
        if not isinstance(value, str) or not _TIME.fullmatch(value):
            raise Invalid("expected UTC timestamp")
        return datetime.fromisoformat(value)
    except (ValueError, OverflowError) as exc:
        raise Invalid("invalid UTC timestamp") from exc


def now() -> datetime:
    return datetime.now(UTC)


def text(value: Any, limit: int = 4096) -> str:
    if (
        not isinstance(value, str)
        or not value.strip()
        or len(value.encode("utf-8")) > limit
        or any(
            (ord(char) < 32 or 127 <= ord(char) <= 159) and char not in "\n\t"
            for char in value
        )
    ):
        raise Invalid("invalid text")
    return value


def capabilities(value: Any) -> list[str]:
    if not isinstance(value, list) or not 1 <= len(value) <= 16:
        raise Invalid("invalid capabilities")
    for item in value:
        ident(item)
    if len(set(value)) != len(value):
        raise Invalid("duplicate capability")
    return value


def address(value: Any) -> str:
    try:
        if (
            not isinstance(value, str)
            or len(value) != 65
            or not value.startswith("agent1")
            or value != value.lower()
        ):
            raise Invalid("invalid agent address")
        prefix, words = bech32_decode(value)
        if prefix != "agent" or words is None:
            raise Invalid("invalid agent address")
        decoded = convertbits(words, 5, 8, False)
        if decoded is None or len(decoded) != 33:
            raise Invalid("invalid agent address")
        VerifyingKey.from_string(bytes(decoded), curve=SECP256k1)
        return value
    except Exception as exc:
        raise Invalid("invalid agent address") from exc


def json_value(value: Any) -> None:
    if len(canonical(value).encode()) > MAX_VALUE:
        raise Invalid("nested JSON exceeds limit")


def uint(value: Any) -> None:
    if type(value) is not int or not 0 <= value <= 2**64 - 1:
        raise Invalid("invalid epoch")


def validate(message: Any, *, fresh: bool = True) -> dict:
    exact(
        message,
        {
            "schema_version",
            "message_id",
            "org_id",
            "mission_id",
            "contract_id",
            "correlation_id",
            "expires_at",
            "body",
        },
    )
    if type(message["schema_version"]) is not int or message["schema_version"] != 1:
        raise Invalid("unsupported schema version")
    uuid(message["message_id"])
    for key in ("org_id", "mission_id", "contract_id", "correlation_id"):
        ident(message[key])
    expiry = timestamp(message["expires_at"])
    if fresh and expiry <= now():
        raise Invalid("expired message")
    body = message["body"]
    if not isinstance(body, dict):
        raise Invalid("invalid body")
    kind = body.get("kind")
    fields = {
        "recruit_request": {"requirement", "capabilities", "lease_expires_at"},
        "offer": {
            "recruit_message_id",
            "offer_id",
            "capabilities",
            "terms",
            "lease_expires_at",
        },
        "accept_offer": {"offer_message_id", "offer_id"},
        "contract_accepted": {"accept_message_id", "offer_id"},
        "task_request": {"task"},
        "task_accepted": {"task_request_id", "job_id", "attempt_id"},
        "task_progress": {"task_request_id", "job_id", "attempt_id", "detail"},
        "task_result": {"task_request_id", "job_id", "attempt_id", "epoch", "output"},
        "release_contract": {"reason"},
        "contract_released": {"release_message_id"},
    }
    if not isinstance(kind, str) or kind not in fields:
        raise Invalid("unsupported message kind")
    exact(body, {"kind"} | fields[kind])
    for key in (
        "recruit_message_id",
        "offer_message_id",
        "accept_message_id",
        "task_request_id",
        "release_message_id",
    ):
        if key in body:
            uuid(body[key])
    for key in ("offer_id", "job_id", "attempt_id"):
        if key in body:
            ident(body[key])
    for key in ("requirement", "detail", "reason"):
        if key in body:
            text(body[key])
    if "capabilities" in body:
        capabilities(body["capabilities"])
    if "lease_expires_at" in body and expiry > timestamp(body["lease_expires_at"]):
        raise Invalid("message exceeds requested lease")
    for key in ("terms", "output"):
        if key in body:
            json_value(body[key])
    if "epoch" in body:
        uint(body["epoch"])
    if kind == "task_request":
        task = exact(
            body["task"],
            {
                "job_id",
                "attempt_id",
                "role_id",
                "assignment_id",
                "idempotency_key",
                "epoch",
                "deadline",
                "input",
            },
        )
        for key in (
            "job_id",
            "attempt_id",
            "role_id",
            "assignment_id",
            "idempotency_key",
        ):
            ident(task[key])
        uint(task["epoch"])
        deadline = timestamp(task["deadline"])
        if deadline > expiry or (fresh and deadline <= now()):
            raise Invalid("invalid task deadline")
        json_value(task["input"])
    if len(canonical(message).encode()) > MAX_MESSAGE:
        raise Invalid("message exceeds limit")
    return message
