"""Deterministic replacement selection over supplied facts, never an oracle.

The organization must authenticate every candidate's invocation and independently
verify this output. This module performs no network calls or kernel mutations.
"""

from __future__ import annotations

import hashlib
import re
import unicodedata
from datetime import datetime
from urllib.parse import urlsplit

from .wire import MAX_VALUE, Invalid, canonical, decode, exact

_HASH = re.compile(r"^[0-9a-f]{64}$")
_CURRENCY = re.compile(r"^[A-Z]{3}$")
_RECOVERY_ID = re.compile(r"^[A-Za-z0-9_.:-]{1,128}$")
_TIME = re.compile(
    r"^([0-9]{4})-([0-9]{2})-([0-9]{2})[Tt]([0-9]{2}):([0-9]{2}):([0-9]{2})"
    r"(?:\.([0-9]{1,9}))?([Zz]|[+-][0-9]{2}:[0-9]{2})$"
)


def _text(value, limit):
    if (
        not isinstance(value, str)
        or not value
        or value.strip() != value
        or len(value.encode("utf-8")) > limit
        or any(unicodedata.category(c) == "Cc" for c in value)
    ):
        raise Invalid("invalid selection text")
    return value


def _integer(value, minimum, maximum):
    if type(value) is not int or not minimum <= value <= maximum:
        raise Invalid("invalid selection integer")
    return value


def _boolean(value):
    if type(value) is not bool:
        raise Invalid("invalid selection boolean")
    return value


def _hash(value):
    if not isinstance(value, str) or not _HASH.fullmatch(value):
        raise Invalid("invalid selection hash")
    return value


def _digest(value):
    return hashlib.sha256(canonical(value).encode("utf-8")).hexdigest()


def _money(value, maximum):
    exact(value, {"minor_units", "currency"})
    _integer(value["minor_units"], 0, maximum)
    if not isinstance(value["currency"], str) or not _CURRENCY.fullmatch(value["currency"]):
        raise Invalid("invalid selection currency")


def _instant(value):
    """Compare RFC3339 instants exactly, including nine-digit fractions."""
    _text(value, 64)
    match = _TIME.fullmatch(value)
    if match is None:
        raise Invalid("invalid selection timestamp")
    year, month, day, hour, minute, second = map(int, match.groups()[:6])
    try:
        moment = datetime(year, month, day, hour, minute, second)
    except ValueError as exc:
        raise Invalid("invalid selection timestamp") from exc
    fraction, offset = match.groups()[6:]
    offset_seconds = 0
    if offset.lower() != "z":
        offset_hour, offset_minute = map(int, offset[1:].split(":"))
        if offset_hour > 23 or offset_minute > 59:
            raise Invalid("invalid selection offset")
        offset_seconds = (offset_hour * 60 + offset_minute) * 60
        if offset[0] == "-":
            offset_seconds = -offset_seconds
    seconds = (
        moment.toordinal() * 86400 + hour * 3600 + minute * 60 + second - offset_seconds
    )
    return seconds * 1_000_000_000 + int((fraction or "").ljust(9, "0"))


def _window(value):
    exact(value, {"start", "end"})
    start, end = _instant(value["start"]), _instant(value["end"])
    if end <= start:
        raise Invalid("invalid selection window")
    return start, end


def _policy(value):
    exact(value, {"selected_venue_id", "attendees", "budget", "window", "step_free_required"})
    _text(value["selected_venue_id"], 256)
    _integer(value["attendees"], 1, 1_000_000)
    _money(value["budget"], 1_000_000_000_000)
    _boolean(value["step_free_required"])
    return _window(value["window"])


def _venue(value):
    exact(
        value,
        {"venue_id", "capacity", "price", "window", "availability", "step_free", "evidence"},
    )
    _text(value["venue_id"], 256)
    _integer(value["capacity"], 0, 2**32 - 1)
    _money(value["price"], 2**64 - 1)
    _boolean(value["step_free"])
    if value["availability"] not in ("available", "unavailable"):
        raise Invalid("invalid selection availability")
    evidence = exact(value["evidence"], {"source_url", "snapshot_sha256", "locator"})
    _hash(evidence["snapshot_sha256"])
    _text(evidence["locator"], 1024)
    _text(evidence["source_url"], 1024)
    try:
        url = urlsplit(evidence["source_url"])
        if (
            url.scheme not in ("http", "https")
            or not url.hostname
            or url.username
            or url.password is not None
            or "#" in evidence["source_url"]
        ):
            raise Invalid("invalid selection source")
        _ = url.port
    except ValueError as exc:
        raise Invalid("invalid selection source") from exc
    return _window(value["window"])


def select_venue(value):
    """Return a complete selection or raise sanitized Invalid; never improvise."""
    # Apply the same byte/depth/numeric boundary as the native task value before
    # computing hashes. Canonicalization does not confer input authenticity.
    value = decode(canonical(value), MAX_VALUE)
    exact(value, {"schema_version", "kind", "context_sha256", "context"})
    if (
        type(value["schema_version"]) is not int
        or value["schema_version"] != 1
        or value["kind"] != "venue_replacement_v1"
    ):
        raise Invalid("invalid selection schema")
    context = exact(value["context"], {"recovery_id", "policy_sha256", "policy", "candidates"})
    if not isinstance(context["recovery_id"], str) or not _RECOVERY_ID.fullmatch(
        context["recovery_id"]
    ):
        raise Invalid("invalid recovery identifier")
    policy = context["policy"]
    wanted_start, wanted_end = _policy(policy)
    if (
        _hash(context["policy_sha256"]) != _digest(policy)
        or _hash(value["context_sha256"]) != _digest(context)
    ):
        raise Invalid("selection binding mismatch")
    candidates = context["candidates"]
    if not isinstance(candidates, list) or not 1 <= len(candidates) <= 16:
        raise Invalid("invalid selection candidates")
    evaluations, eligible, venues, sources = [], [], set(), {}
    for candidate in candidates:
        exact(candidate, {"source_job_id", "invocation_proof_ref", "venue"})
        source = _text(candidate["source_job_id"], 256)
        proof = candidate["invocation_proof_ref"]
        if not isinstance(proof, str) or not proof.startswith("invocation:"):
            raise Invalid("invalid invocation reference")
        _hash(proof.removeprefix("invocation:"))
        venue = candidate["venue"]
        offered_start, offered_end = _venue(venue)
        venue_id = venue["venue_id"]
        if venue_id in venues:
            raise Invalid("duplicate selection venue")
        venues.add(venue_id)
        source_binding = (
            proof, venue["evidence"]["source_url"], venue["evidence"]["snapshot_sha256"]
        )
        if source in sources and sources[source] != source_binding:
            raise Invalid("inconsistent selection source")
        sources[source] = source_binding
        if len(sources) > 8:
            raise Invalid("too many selection sources")
        checks = {
            "capacity": venue["capacity"] >= policy["attendees"],
            "price": venue["price"]["currency"] == policy["budget"]["currency"]
            and venue["price"]["minor_units"] <= policy["budget"]["minor_units"],
            "window": offered_start <= wanted_start and offered_end >= wanted_end,
            "availability": venue["availability"] == "available",
            "step_free": not policy["step_free_required"] or venue["step_free"],
        }
        fits = all(checks.values()) and venue_id != policy["selected_venue_id"]
        evaluations.append(
            {
                "source_job_id": source,
                "venue_id": venue_id,
                "invocation_proof_ref": proof,
                "evidence": dict(venue["evidence"]),
                "checks": checks,
                "eligible": fits,
            }
        )
        if fits:
            eligible.append(
                (
                    venue["price"]["minor_units"],
                    venue_id.encode("utf-8"),
                    source.encode("utf-8"),
                    source,
                    venue_id,
                )
            )
    chosen = min(eligible) if eligible else None
    output = {
        "schema_version": 1,
        "recovery_id": context["recovery_id"],
        "policy_sha256": context["policy_sha256"],
        "context_sha256": value["context_sha256"],
        "evaluations": evaluations,
        "selected": {"source_job_id": chosen[3], "venue_id": chosen[4]} if chosen else None,
    }
    if len(canonical(output).encode("utf-8")) > MAX_VALUE:
        raise Invalid("selection output exceeds limit")
    return output
