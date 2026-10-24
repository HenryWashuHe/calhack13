"""ASI:One primary workflow: report a disruption to a GENESIS organization and
watch it change shape, entirely inside an Agent Chat Protocol conversation.

Importing this module never creates an Agent or registers on a public service.
The chat adapter holds only the organization's restricted *agent* token: it can
record evidence and read state/events. It cannot commit changes, provision
roles, publish capabilities or call generic kernel APIs. The planner proposes,
the kernel decides; chat only reports and observes.
"""

from __future__ import annotations

import argparse
import asyncio
import ipaddress
import os
import re
import sqlite3
import sys
from collections.abc import AsyncIterator
from datetime import UTC, datetime, timedelta
from pathlib import Path
from urllib.parse import urlsplit
from uuid import uuid4

import aiohttp

from .wire import Invalid, canonical, decode, exact

MAX_TEXT = 512
MAX_REPLY = 3500
MAX_EVENTS_PER_READ = 200
STATUS_PAGES_PER_CALL = 5
EVENT_LOG_CAP = 512 * 1024
STATE_CAP = 2 * 1024 * 1024
HTTP_TIMEOUT = 10
POLL_SECONDS = 2.0
SETTLE_SECONDS = 3.0
REPORT_VERBS = ("report", "disruption", "evidence", "observe")
STATUS_WORDS = {"status", "progress", "update", "updates", "what happened", "what happened?", "watch"}
ORG_WORDS = {"org", "organization", "organisation", "who", "team", "roles", "show org", "show organization"}
HELP_WORDS = {"help", "hi", "hello", "hey", "start", "?", "commands"}
_CONTROL = re.compile(r"[\x00-\x1f\x7f-\x9f]")


def config_from_bytes(raw: bytes) -> dict:
    config = exact(decode(raw, limit=32768), {"public_demo", "evidence_ref", "source", "max_reports_per_hour", "follow_seconds"})
    if config["public_demo"] is not True:
        raise Invalid("public demo authorization is required")
    ref = config["evidence_ref"]
    parsed = urlsplit(ref) if isinstance(ref, str) else None
    if parsed is None or parsed.scheme not in {"https", "fixture"} or not parsed.netloc or len(ref) > 512 or _CONTROL.search(ref):
        raise Invalid("evidence_ref must be a fixed https or fixture URL")
    source = config["source"]
    if not isinstance(source, str) or not re.fullmatch(r"[a-z][a-z0-9._-]{0,47}", source):
        raise Invalid("source must be a short lowercase label")
    limit = config["max_reports_per_hour"]
    if type(limit) is not int or not 1 <= limit <= 60:
        raise Invalid("max_reports_per_hour must be 1..60")
    follow = config["follow_seconds"]
    if type(follow) is not int or not 0 <= follow <= 120:
        raise Invalid("follow_seconds must be 0..120")
    return config


def backend_url(value: str) -> str:
    try:
        parsed = urlsplit(value)
        valid = (
            parsed.scheme == "http"
            and parsed.hostname is not None
            and ipaddress.ip_address(parsed.hostname).is_loopback
            and parsed.port is not None
            and parsed.username is None
            and parsed.password is None
            and not parsed.query
            and not parsed.fragment
            and parsed.path == "/api/v1"
        )
    except ValueError as exc:
        raise Invalid("backend must be a literal loopback API URL") from exc
    if not valid:
        raise Invalid("backend must be a literal loopback API URL")
    return value


def clean(value: object, limit: int = 240) -> str:
    """Organization-authored text shown as data: control characters removed, length capped."""
    if not isinstance(value, str):
        return "?"
    text = _CONTROL.sub(" ", value).strip()
    return text if len(text) <= limit else text[: limit - 1] + "…"


def gap_state(value: object) -> str:
    """`CapabilityGapStatus` is serde-tagged (`{"state": "installed", ...}`); older fixtures used a string."""
    if isinstance(value, dict):
        state = clean(value.get("state"), 40).lower()
        cap = value.get("capability")
        if isinstance(cap, dict) and isinstance(cap.get("version"), str):
            return f"{state} ({clean(cap.get('capability_id'), 60)}@{clean(cap.get('version'), 20)})"
        return state or "?"
    if isinstance(value, str):
        return clean(value, 40).lower()
    return "?"


class RoleNames:
    def __init__(self, state: dict | None):
        roles = state.get("roles", {}) if isinstance(state, dict) else {}
        self.names = {rid: clean(r.get("name"), 60) for rid, r in roles.items() if isinstance(r, dict)}

    def __call__(self, role_id: object) -> str:
        if isinstance(role_id, str):
            return self.names.get(role_id, role_id[:13])
        return "?"


def describe(event: dict, names: RoleNames) -> str | None:
    """One line per event the operator cares about; None for noise. Mirrors the console's descriptions."""
    kind = event.get("type")
    p = event.get("payload")
    if not isinstance(kind, str) or not isinstance(p, dict):
        return None
    match kind:
        case "observation_recorded":
            return f"Evidence recorded from {clean(p.get('source'), 40)}: {clean(p.get('summary'))}"
        case "requirement_recorded":
            return f"New requirement: {clean(p.get('summary'))}"
        case "planner_decision":
            verdict = clean(p.get("verdict"), 20)
            line = f"Planner ({clean(p.get('model'), 60)}) verdict {verdict}: {clean(p.get('summary'))}"
            errors = p.get("kernel_errors")
            if isinstance(errors, list) and errors:
                line += " Kernel pushed back: " + "; ".join(clean(e, 80) for e in errors[:3])
            return line
        case "change_proposed":
            ops = p.get("operations")
            ops_text = ", ".join(clean(o, 40) for o in ops[:8]) if isinstance(ops, list) else "?"
            return f"Change proposed [{ops_text}]: {clean(p.get('rationale'))}"
        case "change_status_changed":
            to = clean(p.get("to"), 20)
            if to == "COMMITTED":
                return "Change committed. The organization now has a new shape."
            if to in {"REJECTED", "FAILED"}:
                reasons = p.get("reasons")
                why = "; ".join(clean(r, 80) for r in reasons[:3]) if isinstance(reasons, list) and reasons else "no reason recorded"
                return f"Change {to.lower()} by the kernel: {why}"
            return None
        case "role_created":
            return f"Role created: {clean(p.get('name'), 60)} (provider {clean(p.get('provider'), 20)})"
        case "role_provisioned":
            stub = " [DEV STUB, not a live provider]" if p.get("dev_stub") is True else ""
            return f"Role {clean(p.get('role_key'), 40)} provisioned on {clean(p.get('provider'), 20)} as {clean(p.get('provider_agent_id'), 80)}{stub}"
        case "provisioning_failed":
            return f"Provisioning failed for {clean(p.get('role_key'), 40)}: {clean(p.get('error'), 120)}"
        case "role_activated":
            return f"Role activated: {names(p.get('role_id'))}"
        case "responsibility_assigned":
            return f"{names(p.get('owner'))} is now accountable (epoch {p.get('epoch')})"
        case "responsibility_transferred":
            return f"Ownership transferred: {names(p.get('from'))} → {names(p.get('to'))} (epoch {p.get('epoch')})"
        case "contract_requested":
            return f"Recruiting independent specialist {clean(p.get('specialist_address'), 70)} for: {clean(p.get('scope'), 120)}"
        case "contract_state_changed":
            return f"Specialist contract {clean(p.get('to'), 20).lower()}"
        case "capability_gap_opened":
            return f"Capability requested: {clean(p.get('capability_id'), 60)} for {names(p.get('role_id'))}"
        case "capability_gap_status_changed":
            return f"Requested capability now {gap_state(p.get('to'))}"
        case "capability_attached":
            cap = p.get("capability") if isinstance(p.get("capability"), dict) else {}
            return f"Capability {clean(cap.get('capability_id'), 60)}@{clean(cap.get('version'), 20)} attached to {names(p.get('role_id'))}"
        case "capability_installed":
            cap = p.get("capability") if isinstance(p.get("capability"), dict) else {}
            return f"Capability {clean(cap.get('capability_id'), 60)}@{clean(cap.get('version'), 20)} installed with {p.get('synthesis_calls')} synthesis calls"
        case "installation_accepted":
            return "Local acceptance checks passed"
        case "installation_rejected":
            return "Local acceptance checks failed"
        case "job_dispatched":
            return f"Work dispatched to {names(p.get('role_id'))}: {clean(p.get('task'), 120)}"
        case "job_status_changed":
            to = clean(p.get("to"), 24)
            return f"Work {to.lower().replace('_', ' ')}" if to in {"SUCCEEDED", "FAILED", "RESULT_REJECTED"} else None
        case "role_draining":
            return f"Role draining: {names(p.get('role_id'))}"
        case "role_retired":
            return f"Role retired: {names(p.get('role_id'))}. Its responsibilities were handed off first."
        case "assignment_fenced":
            return f"Stale assignment fenced (epoch {p.get('epoch')} can no longer act)"
        case _:
            return None


def summarize_org(state: dict) -> str:
    missions = state.get("missions", {})
    roles = state.get("roles", {})
    responsibilities = state.get("responsibilities", {})
    gaps = state.get("capability_gaps", {})
    names = RoleNames(state)
    lines = [f"{clean(state.get('name'), 80)} at revision {state.get('revision')}."]
    active = [m for m in missions.values() if isinstance(m, dict) and m.get("status") == "ACTIVE"]
    if active:
        lines.append(f"Mission: {clean(active[0].get('goal'))}")
    else:
        lines.append("No active mission.")
    live = [r for r in roles.values() if isinstance(r, dict) and r.get("state") in {"ACTIVE", "PROVISIONING", "DRAINING"}]
    retired = sum(1 for r in roles.values() if isinstance(r, dict) and r.get("state") == "RETIRED")
    lines.append(f"Roles ({len(live)} active, {retired} retired):")
    for role in live[:12]:
        binding = role.get("binding") if isinstance(role.get("binding"), dict) else {}
        provider = clean(binding.get("provider"), 20)
        stub = " [dev stub]" if provider == "dev_stub" else ""
        owned = [clean(r.get("purpose"), 70) for r in responsibilities.values() if isinstance(r, dict) and isinstance(r.get("owner"), dict) and r["owner"].get("id") == role.get("role_id") and r.get("status") == "OPEN"]
        duties = "; ".join(owned[:3]) if owned else "no open responsibility"
        lines.append(f"- {clean(role.get('name'), 60)} ({provider}{stub}, {clean(role.get('state'), 14).lower()}): {duties}")
    open_gaps = [g for g in gaps.values() if isinstance(g, dict)]
    if open_gaps:
        lines.append("Requested capabilities: " + "; ".join(f"{clean(g.get('capability_id'), 50)} {gap_state(g.get('status'))}" for g in open_gaps[:6]))
    return "\n".join(lines)


class MissionGateway:
    """One bounded journal per public agent, backend and immutable configuration.

    `report` records one observation with a fixed source and evidence_ref; the
    organization's own planner and kernel decide what, if anything, changes.
    `status` reads events after the last report. Nothing here can commit.
    """

    def __init__(self, *, config: dict, url: str, token: str, identity: str, db: str):
        self.config = config_from_bytes(canonical(config).encode())
        self.url = backend_url(url)
        if not isinstance(token, str) or not 8 <= len(token) <= 512 or not all(33 <= ord(c) <= 126 for c in token):
            raise Invalid("invalid backend agent token")
        self.token = token
        self.lock = asyncio.Lock()
        target = Path(db)
        target.parent.mkdir(parents=True, exist_ok=True)
        if target.is_symlink():
            raise Invalid("journal must not be a symlink")
        fd = os.open(target, os.O_CREAT | os.O_RDWR, 0o600)
        os.close(fd)
        os.chmod(target, 0o600)
        self.db = sqlite3.connect(target, isolation_level=None, timeout=5)
        self.db.row_factory = sqlite3.Row
        try:
            self.db.execute("PRAGMA synchronous=FULL")
            self.db.execute("PRAGMA journal_mode=DELETE")
            self.db.executescript("""
                CREATE TABLE IF NOT EXISTS gateway_binding(id INTEGER PRIMARY KEY CHECK(id=1), value TEXT NOT NULL);
                CREATE TABLE IF NOT EXISTS reports(
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    observation_id TEXT,
                    after_seq INTEGER NOT NULL,
                    created_at TEXT NOT NULL,
                    sender TEXT NOT NULL DEFAULT '',
                    message_id TEXT UNIQUE,
                    summary TEXT NOT NULL DEFAULT '',
                    visible_seq INTEGER NOT NULL DEFAULT 0
                );
            """)
            columns = {row[1] for row in self.db.execute("PRAGMA table_info(reports)")}
            for name, decl in (("sender", "TEXT NOT NULL DEFAULT ''"), ("message_id", "TEXT"), ("summary", "TEXT NOT NULL DEFAULT ''"), ("visible_seq", "INTEGER NOT NULL DEFAULT 0")):
                if name not in columns:
                    self.db.execute(f"ALTER TABLE reports ADD COLUMN {name} {decl}")
                    if name == "visible_seq":
                        # A row written before the cursor existed starts where its report did, like a new row;
                        # otherwise status would replay events from before that report.
                        self.db.execute("UPDATE reports SET visible_seq = after_seq")
            self.db.execute("CREATE UNIQUE INDEX IF NOT EXISTS reports_message ON reports(message_id)")
            binding = canonical({"identity": identity, "url": self.url, "config": self.config})
            self.db.execute("BEGIN IMMEDIATE")
            self.db.execute("INSERT OR IGNORE INTO gateway_binding VALUES(1,?)", (binding,))
            if self.db.execute("SELECT value FROM gateway_binding WHERE id=1").fetchone()[0] != binding:
                raise Invalid("journal identity or configuration changed")
            self.db.execute("COMMIT")
        except BaseException:
            if self.db.in_transaction:
                self.db.execute("ROLLBACK")
            self.db.close()
            raise

    def close(self):
        self.db.close()

    @staticmethod
    def help() -> str:
        return (
            "I am the front door of a GENESIS organization: an AI organization that changes its own shape. "
            "Commands:\n"
            "- org: who is in the organization right now and what each role owns\n"
            "- report <what happened>: record evidence (for example: report the main hall is closed on Nov 14). "
            "The organization's planner decides whether roles, specialists or capabilities must change; I relay each step.\n"
            "- status: what has happened since your last report\n"
            "Chat cannot approve, commit, book or spend anything. Provider steps labeled DEV STUB are stand-ins, not live integrations."
        )

    async def _http(self, method: str, path: str, *, body: dict | None = None, cap: int) -> tuple[int, object | None]:
        async with aiohttp.ClientSession(
            timeout=aiohttp.ClientTimeout(total=HTTP_TIMEOUT), trust_env=False,
            auto_decompress=False, cookie_jar=aiohttp.DummyCookieJar(),
        ) as client:
            async with client.request(
                method, self.url + path,
                headers={"Authorization": "Bearer " + self.token, "Accept": "application/json"},
                json=body, allow_redirects=False,
            ) as response:
                if response.headers.get("Content-Encoding") or response.content_type != "application/json":
                    raise Invalid("unsupported response")
                raw = await response.content.read(cap + 1)
                while len(raw) <= cap:
                    chunk = await response.content.read(cap + 1 - len(raw))
                    if not chunk:
                        break
                    raw += chunk
                return response.status, decode(raw, limit=cap, max_depth=32)

    async def _state(self) -> dict:
        status, state = await self._http("GET", "/organization", cap=STATE_CAP)
        if status != 200 or not isinstance(state, dict):
            raise Invalid("organization unavailable")
        return state

    async def _events_after(self, seq: int) -> list[dict]:
        status, events = await self._http("GET", f"/events/log?after={int(seq)}&limit={MAX_EVENTS_PER_READ}", cap=EVENT_LOG_CAP)
        if status != 200 or not isinstance(events, list):
            raise Invalid("event log unavailable")
        return [e for e in events if isinstance(e, dict) and type(e.get("seq")) is int]

    async def _latest_seq(self) -> int:
        # The log only pages forward; find the tail so a report's cursor starts at "now".
        seq = int(self.db.execute("SELECT COALESCE(MAX(after_seq),0) FROM reports").fetchone()[0])
        while True:
            batch = await self._events_after(seq)
            if not batch:
                return seq
            seq = max(seq, max(e["seq"] for e in batch))

    def _reports_last_hour(self) -> int:
        since = (datetime.now(UTC) - timedelta(hours=1)).isoformat()
        return int(self.db.execute("SELECT COUNT(*) FROM reports WHERE created_at > ?", (since,)).fetchone()[0])

    def _find_report(self, message_id: str) -> sqlite3.Row | None:
        return self.db.execute("SELECT * FROM reports WHERE message_id=?", (message_id,)).fetchone()

    def _last_report(self, sender: str) -> sqlite3.Row | None:
        return self.db.execute("SELECT * FROM reports WHERE sender=? ORDER BY id DESC LIMIT 1", (sender,)).fetchone()

    def _insert_report(self, *, sender: str, message_id: str, after_seq: int, summary: str) -> int:
        # Intent is durable before the observation POST: a redelivered or
        # replayed message finds this row and never submits a second effect.
        cursor = self.db.execute(
            "INSERT INTO reports(observation_id, after_seq, created_at, sender, message_id, summary, visible_seq) VALUES(NULL,?,?,?,?,?,?)",
            (after_seq, datetime.now(UTC).isoformat(), sender, message_id, summary, after_seq),
        )
        return int(cursor.lastrowid)

    def _set_observation(self, row_id: int, observation_id: str) -> None:
        self.db.execute("UPDATE reports SET observation_id=? WHERE id=?", (observation_id, row_id))

    def _set_visible(self, row_id: int, seq: int) -> None:
        self.db.execute("UPDATE reports SET visible_seq=MAX(visible_seq, ?) WHERE id=?", (int(seq), row_id))

    @staticmethod
    def _lines(events: list[dict], names: RoleNames, observation_id: str | None = None) -> list[tuple[int, str]]:
        out = []
        for e in events:
            p = e.get("payload") if isinstance(e.get("payload"), dict) else {}
            if observation_id is not None and e.get("type") == "planner_decision" and p.get("observation_id") != observation_id:
                continue  # another observation's decision is not this report's progress
            line = describe(e, names)
            if line:
                out.append((e["seq"], line))
        return out

    def _render(self, events: list[dict], names: RoleNames) -> list[str]:
        return [line for _, line in self._lines(events, names)]

    async def _status_reply(self, row: sqlite3.Row) -> str:
        """Read forward from this report's durable visible cursor. Bounded in pages and in reply size; the
        cursor advances only past events that were shown (or that render nothing), so a later outcome is
        always reachable by asking again."""
        row_id, seq = int(row["id"]), int(row["visible_seq"])
        names: RoleNames | None = None
        chosen: list[str] = []
        size = 0
        pages = 0
        more = False
        while pages < STATUS_PAGES_PER_CALL:
            pages += 1
            batch = await self._events_after(seq)
            if not batch:
                break
            if names is None:
                names = RoleNames(await self._state())
            lines = self._lines(batch, names)
            if not lines:
                seq = max(e["seq"] for e in batch)
                self._set_visible(row_id, seq)
                continue
            for line_seq, line in lines:
                if chosen and size + len(line) + 1 > MAX_REPLY - 120:
                    more = True
                    break
                chosen.append(line[:MAX_REPLY - 120])
                size += len(chosen[-1]) + 1
                seq = line_seq
            if more:
                break
            seq = max(e["seq"] for e in batch)
            self._set_visible(row_id, seq)
        if not chosen:
            return "Nothing new since your report. The planner may still be reading the evidence; ask again shortly."
        self._set_visible(row_id, seq)
        text = "\n".join(chosen)
        if more:
            text += "\nThere is more. Ask status again to continue from here."
        return text

    async def _follow(self, after_seq: int, seconds: int, observation_id: str) -> AsyncIterator[str]:
        """Relay what the organization does after a report, in bounded time, as it happens. Ends early only on
        this report's own planner decision (and, when it proposed a change, that change's terminal status)."""
        deadline = asyncio.get_running_loop().time() + seconds
        seq = after_seq
        names: RoleNames | None = None
        own_change: str | None = None
        while asyncio.get_running_loop().time() < deadline:
            await asyncio.sleep(POLL_SECONDS)
            batch = await self._events_after(seq)
            if not batch:
                continue
            seq = max(e["seq"] for e in batch)
            if names is None or any(e.get("type") in {"role_created", "role_activated"} for e in batch):
                names = RoleNames(await self._state())
            lines = [line for _, line in self._lines(batch, names, observation_id)]
            if lines:
                yield "\n".join(lines)
            done = False
            for e in batch:
                p = e.get("payload") if isinstance(e.get("payload"), dict) else {}
                if e.get("type") == "planner_decision" and p.get("observation_id") == observation_id:
                    if p.get("verdict") in {"no_change", "escalated"}:
                        done = True
                    elif isinstance(p.get("change_id"), str):
                        own_change = p["change_id"]
                if e.get("type") == "change_status_changed" and own_change is not None and p.get("change_id") == own_change and p.get("to") in {"COMMITTED", "REJECTED", "FAILED"}:
                    done = True
            if done:
                await asyncio.sleep(SETTLE_SECONDS)
                tail = await self._events_after(seq)
                if tail:
                    seq = max(e["seq"] for e in tail)
                    lines = [line for _, line in self._lines(tail, names, observation_id)]
                    if lines:
                        yield "\n".join(lines)
                return

    async def respond(self, text: str, *, sender: str | None = None, message_id: str | None = None) -> AsyncIterator[str]:
        """`sender`/`message_id` are the authenticated ACP identities of the incoming message. A redelivered
        message (same id) never records a second observation; a missing id is a genuinely new report."""
        sender = sender if isinstance(sender, str) else ""
        message_id = message_id if isinstance(message_id, str) and message_id else str(uuid4())
        try:
            if not isinstance(text, str) or len(text.encode()) > MAX_TEXT:
                yield "Keep messages under 512 bytes. Use help, org, report <what happened>, or status."
                return
        except UnicodeError:
            yield "Use help, org, report <what happened>, or status."
            return
        words = text.strip().split()
        lowered = " ".join(words).lower()
        if not words or lowered in HELP_WORDS:
            yield self.help()
            return
        if self.lock.locked():
            yield "I am relaying another conversation's report right now. Repeat your message in a moment; nothing was recorded."
            return
        async with self.lock:
            try:
                if lowered in ORG_WORDS:
                    yield summarize_org(await self._state())
                    return
                if lowered in STATUS_WORDS:
                    last = self._last_report(sender)
                    if last is None:
                        yield "No report from this agent yet. Use report <what happened> first, or org to see the current organization."
                        return
                    yield await self._status_reply(last)
                    return
                if words[0].lower().rstrip(":") in REPORT_VERBS and len(words) >= 2:
                    summary = clean(" ".join(words[1:]), 400)
                    if len(summary) < 8:
                        yield "Describe what happened in a sentence, for example: report the main hall is closed on Nov 14."
                        return
                    known = self._find_report(message_id)
                    if known is not None:
                        # Redelivery of a message already acted on: report, never repeat the effect.
                        if known["observation_id"]:
                            yield (
                                f"This message was already recorded as evidence {known['observation_id'][:17]}. "
                                "Nothing new was submitted. Ask status for what the organization did with it."
                            )
                        else:
                            yield (
                                "This message was already submitted once and the organization never confirmed it. "
                                "It was not submitted again; ask status to check, or send a new report."
                            )
                        return
                    if self._reports_last_hour() >= self.config["max_reports_per_hour"]:
                        yield "This public demo accepts a limited number of reports per hour. Try again later; nothing was recorded."
                        return
                    state = await self._state()
                    missions = [m for m in state.get("missions", {}).values() if isinstance(m, dict) and m.get("status") == "ACTIVE"]
                    if not missions:
                        yield "The organization has no active mission, so there is nothing to report evidence against."
                        return
                    mission_id = missions[0].get("mission_id")
                    if not isinstance(mission_id, str) or not re.fullmatch(r"[A-Za-z0-9_.:-]{1,128}", mission_id):
                        raise Invalid("invalid mission identity")
                    cursor = await self._latest_seq()
                    row_id = self._insert_report(sender=sender, message_id=message_id, after_seq=cursor, summary=summary)
                    body = {"source": self.config["source"], "summary": summary, "evidence_ref": self.config["evidence_ref"]}
                    try:
                        status, record = await self._http("POST", f"/missions/{mission_id}/observations", body=body, cap=65536)
                    except (aiohttp.ClientError, TimeoutError, Invalid):
                        yield "The organization did not confirm the report. It may still have been recorded; use status to check before repeating it."
                        return
                    if status != 201 or not isinstance(record, dict) or not isinstance(record.get("observation_id"), str):
                        yield f"The organization refused the report (HTTP {status}). Nothing to relay."
                        return
                    observation_id = record["observation_id"]
                    self._set_observation(row_id, observation_id)
                    yield (
                        f"Recorded as evidence {observation_id[:17]} for the mission \"{clean(missions[0].get('goal'), 100)}\". "
                        "The organization's planner is reading it now; I will relay each step."
                    )
                    try:
                        async for chunk in self._follow(cursor, self.config["follow_seconds"], observation_id):
                            yield chunk[:MAX_REPLY]
                    except (aiohttp.ClientError, TimeoutError, Invalid, sqlite3.Error):
                        # The observation exists; only our view of what followed is unavailable.
                        yield (
                            f"Your report is recorded as evidence {observation_id[:17]}, but progress is unavailable right now. "
                            "Ask status later to see what the organization did with it."
                        )
                        return
                    yield "That is what the organization did so far. Ask status later for anything that follows, or org for the current shape."
                    return
                yield self.help()
            except (aiohttp.ClientError, TimeoutError, Invalid, sqlite3.Error):
                yield "The organization is not reachable right now. Nothing was changed by this message."


def make_protocol(gateway: MissionGateway):
    from uagents import Protocol
    from uagents_core.contrib.protocols.chat import (
        ChatAcknowledgement, ChatMessage, EndSessionContent, StartSessionContent, TextContent, chat_protocol_spec,
    )

    protocol = Protocol(spec=chat_protocol_spec)

    async def send_text(ctx, sender: str, text: str, *, end: bool):
        content = [TextContent(type="text", text=text)]
        if end:
            content.append(EndSessionContent(type="end-session"))
        await ctx.send(sender, ChatMessage(timestamp=datetime.now(UTC), msg_id=uuid4(), content=content))

    @protocol.on_message(ChatMessage)
    async def handle_message(ctx, sender: str, msg: ChatMessage):
        await ctx.send(sender, ChatAcknowledgement(timestamp=datetime.now(UTC), acknowledged_msg_id=msg.msg_id))
        texts = [item.text for item in msg.content if isinstance(item, TextContent)]
        supported = all(isinstance(item, (TextContent, StartSessionContent, EndSessionContent)) for item in msg.content)
        if not supported or len(texts) != 1 or len(msg.content) > 3:
            if texts or not supported:
                await send_text(ctx, sender, "Send one text message: help, org, report <what happened>, or status. Attachments are not processed.", end=True)
            return
        pending: str | None = None
        try:
            async for chunk in gateway.respond(texts[0], sender=sender, message_id=str(msg.msg_id)):
                if pending is not None:
                    await send_text(ctx, sender, pending, end=False)
                pending = chunk
        except (Invalid, sqlite3.Error, UnicodeError, aiohttp.ClientError, TimeoutError):
            pending = "Gateway unavailable. Nothing was changed by this message."
        await send_text(ctx, sender, pending or "No response.", end=True)

    @protocol.on_message(ChatAcknowledgement)
    async def handle_ack(_ctx, _sender: str, _msg: ChatAcknowledgement):
        pass

    return protocol


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    commands.add_parser("identity", help="Print only the public ACP identity, with no network")
    serve = commands.add_parser("serve-public", help="Explicitly register and serve the public organization agent")
    serve.add_argument("--config", required=True, help="JSON file declaring public_demo:true, the fixed evidence_ref, source and limits")
    serve.add_argument("--db", required=True, help="Separate persistent journal for this agent")
    serve.add_argument("--port", type=int, default=8104, help="Loopback-only SDK Inspector/bootstrap port")
    serve.add_argument("--name", default="GENESIS Organization")
    serve.add_argument("--readme", required=True, help="Agent README published to Agentverse; inspect it before use")
    args = parser.parse_args()
    gateway = None
    try:
        from uagents_core.identity import Identity

        seed = os.environ.get("GENESIS_ACP_MISSION_SEED", "")
        if not 32 <= len(seed.encode()) <= 4096:
            raise Invalid("GENESIS_ACP_MISSION_SEED must contain 32..4096 bytes")
        identity = Identity.from_seed(seed, 0).address
        if args.command == "identity":
            print(identity)
            return
        if not 1 <= args.port <= 65535:
            raise Invalid("port must be in 1..65535")
        with Path(args.config).open("rb") as stream:
            config = config_from_bytes(stream.read(32769))
        readme = Path(args.readme).read_text(encoding="utf-8")
        if not readme.strip() or len(readme.encode()) > 32768:
            raise Invalid("invalid agent README")
        gateway = MissionGateway(
            config=config,
            url=os.environ.get("GENESIS_ACP_BACKEND_URL", ""),
            token=os.environ.get("GENESIS_ACP_AGENT_TOKEN", ""),
            identity=identity, db=args.db,
        )
        from uagents import Agent
        import uagents.asgi
        from uagents.registration import AlmanacApiRegistrationPolicy

        uagents.asgi.HOST = "127.0.0.1"
        agent = Agent(
            name=args.name, seed=seed, mailbox=True, port=args.port,
            registration_policy=AlmanacApiRegistrationPolicy(),
            enable_agent_inspector=True, publish_agent_details=True,
            store_message_history=False, report_events=False,
            handle_messages_concurrently=True,
            readme_path=str(Path(args.readme).resolve()),
        )
        agent.include(make_protocol(gateway), publish_manifest=True)
        agent.run()
    except (Invalid, OSError, UnicodeError, sqlite3.Error):
        print("ACP mission gateway configuration or journal invalid. Check its seed, agent token, config, README and database.", file=sys.stderr)
        raise SystemExit(2) from None
    finally:
        if gateway is not None:
            gateway.close()


if __name__ == "__main__":
    main()
