"""Public demo actions over frozen recovery intents; never a general operator proxy."""

from __future__ import annotations

import asyncio
import ipaddress
import json
import os
import re
import sqlite3
from datetime import datetime
from pathlib import Path
from urllib.parse import urlsplit

import aiohttp

from .wire import Invalid, canonical, decode, exact, ident

CASE_NAME = re.compile(r"[a-z][a-z0-9-]{0,47}\Z")
RECOVERY_ID = re.compile(r"recovery_[0-9a-f]{8}-(?:[0-9a-f]{4}-){3}[0-9a-f]{12}\Z")
PHASES = {"recruiting", "accepting", "selecting", "assessing", "completed", "retired"}


def config_from_bytes(raw: bytes) -> dict:
    config = exact(decode(raw, limit=32768), {"public_demo", "cases"})
    if config["public_demo"] is not True:
        raise Invalid("public demo authorization is required")
    cases = config["cases"]
    if not isinstance(cases, dict) or not 1 <= len(cases) <= 16:
        raise Invalid("configure 1..16 public demo cases")
    keys = set()
    for name, request in cases.items():
        if not CASE_NAME.fullmatch(name):
            raise Invalid("invalid case name")
        exact(request, {"client_request_id", "reopened_assessment_id", "candidate_job_ids", "deadline"})
        key = ident(request["client_request_id"])
        if key in keys:
            raise Invalid("case request keys must be distinct")
        keys.add(key)
        ident(request["reopened_assessment_id"])
        jobs = request["candidate_job_ids"]
        if not isinstance(jobs, list) or not 1 <= len(jobs) <= 8:
            raise Invalid("configure 1..8 candidate jobs")
        for job in jobs:
            ident(job)
        if len(set(jobs)) != len(jobs):
            raise Invalid("candidate jobs must be distinct")
        deadline = request["deadline"]
        if not isinstance(deadline, str) or not re.fullmatch(
            r"\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(?:\.\d{1,6})?(?:Z|[+-]\d{2}:\d{2})", deadline
        ):
            raise Invalid("deadline must be a fixed RFC3339 instant")
        try:
            datetime.fromisoformat(deadline.replace("Z", "+00:00"))
        except ValueError as exc:
            raise Invalid("invalid deadline") from exc
    return config


def backend_url(value: str) -> str:
    try:
        parsed = urlsplit(value)
        valid = (
            parsed.scheme == "http"
            and parsed.hostname is not None
            and ipaddress.ip_address(parsed.hostname).is_loopback
            and parsed.port is not None
            and 1 <= parsed.port <= 65535
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


class RecoveryGateway:
    """One bounded journal per public agent, backend and immutable case set.

    Backend creation is idempotent under the frozen client_request_id, including
    after an uncertain POST or a crash before saving its response. Status only
    performs GET. Remote chat never renews deadlines or resets retry budgets.
    """

    def __init__(self, *, config: dict, url: str, token: str, identity: str, db: str):
        # Revalidate/copy even when constructed by another Python caller.
        self.config = config_from_bytes(canonical(config).encode())
        self.url = backend_url(url)
        if not isinstance(token, str) or not 8 <= len(token) <= 512 or not all(33 <= ord(c) <= 126 for c in token):
            raise Invalid("invalid backend operator token")
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
        try:
            self.db.execute("PRAGMA synchronous=FULL")
            self.db.execute("PRAGMA journal_mode=DELETE")
            page_size = self.db.execute("PRAGMA page_size").fetchone()[0]
            self.db.execute(f"PRAGMA max_page_count={1024 * 1024 // page_size}")
            self.db.executescript("""
                CREATE TABLE IF NOT EXISTS gateway_binding(id INTEGER PRIMARY KEY CHECK(id=1), value TEXT NOT NULL);
                CREATE TABLE IF NOT EXISTS recoveries(name TEXT PRIMARY KEY, recovery_id TEXT NOT NULL);
            """)
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

    def help(self) -> str:
        return (
            "GENESIS public demo: recover <case> starts one preapproved venue recovery; "
            "status <case> reads its progress. Cases: " + ", ".join(self.config["cases"]) + ". "
            "The organization recruits an independent Fetch specialist and checks its selection, "
            "then asks its CREAO monitor to assess the replacement. Candidate sources and the "
            "reopened assessment must already exist. These commands cannot book a venue or "
            "complete the whole mission."
        )

    async def respond(self, text: str) -> str:
        try:
            if not isinstance(text, str) or len(text.encode()) > 512:
                return "Use help, recover <case>, or status <case>."
        except UnicodeError:
            return "Use help, recover <case>, or status <case>."
        words = text.strip().split()
        if words in (["help"], ["cases"]):
            return self.help()
        if len(words) != 2 or words[0] not in {"recover", "status"} or words[1] not in self.config["cases"]:
            return self.help()
        command, name = words
        if self.lock.locked():
            return "The demo gateway is busy. Repeat the same command shortly; no new request was started."
        async with self.lock:
            row = self.db.execute("SELECT recovery_id FROM recoveries WHERE name=?", (name,)).fetchone()
            if command == "status" and row is None:
                return f"Case {name}: no confirmed recovery ID. Use recover {name} to start or reconcile its exact saved request."
            if row is not None and not RECOVERY_ID.fullmatch(row[0]):
                raise Invalid("invalid saved recovery identity")
            method = "POST" if command == "recover" and row is None else "GET"
            suffix = "/venue-recoveries" + ("" if method == "POST" else "/" + row[0])
            try:
                async with aiohttp.ClientSession(
                    timeout=aiohttp.ClientTimeout(total=10), trust_env=False,
                    auto_decompress=False, cookie_jar=aiohttp.DummyCookieJar(),
                ) as client:
                    async with client.request(
                        method, self.url + suffix,
                        headers={"Authorization": "Bearer " + self.token, "Accept": "application/json"},
                        json=self.config["cases"][name] if method == "POST" else None,
                        allow_redirects=False,
                    ) as response:
                        if response.status not in ({200, 201} if method == "POST" else {200}):
                            return f"Case {name}: backend did not confirm the request (HTTP {response.status}). Further calls retain the same frozen request key and deadline; chat cannot reset retry budgets."
                        if response.headers.get("Content-Encoding") or response.content_type != "application/json":
                            raise Invalid("unsupported response")
                        raw = await response.content.read(65537)
                        # StreamReader.read(n) may return early; continue to EOF within the cap.
                        while len(raw) <= 65536:
                            chunk = await response.content.read(65537 - len(raw))
                            if not chunk:
                                break
                            raw += chunk
                        record = decode(raw)
                        result = self._project(name, record)
                        received_id = record["recovery_id"]
                        if row is not None and received_id != row[0]:
                            raise Invalid("backend returned another recovery")
                        self.db.execute("INSERT OR IGNORE INTO recoveries VALUES(?,?)", (name, received_id))
                        saved = self.db.execute("SELECT recovery_id FROM recoveries WHERE name=?", (name,)).fetchone()[0]
                        if saved != received_id:
                            raise Invalid("conflicting recovery identity")
                        return result
            except (aiohttp.ClientError, TimeoutError, Invalid, sqlite3.Error):
                return f"Case {name}: response unconfirmed. Use recover {name} to reconcile the same frozen request; its sources, key and deadline stay unchanged."

    @staticmethod
    def _project(name: str, record: object) -> str:
        if not isinstance(record, dict) or not isinstance(record.get("recovery_id"), str) or not RECOVERY_ID.fullmatch(record["recovery_id"]):
            raise Invalid("invalid recovery response")
        phase = record.get("phase")
        if not isinstance(phase, str) or phase not in PHASES:
            raise Invalid("invalid recovery phase")
        attention = record.get("attention")
        if attention is not None and not isinstance(attention, str):
            raise Invalid("invalid recovery attention")
        # No raw backend errors, candidate text, prompts, or arbitrary URLs go to chat.
        text = f"Case {name}: {phase}. Recovery {record['recovery_id']}."
        if attention is not None:
            text += " Operator attention is required; chat cannot reset the hold."
        if phase == "completed":
            assessment = record.get("assessment_id")
            proof = record.get("assessment_proof_ref")
            venue = record.get("selected_venue_id")
            if not isinstance(assessment, str) or not re.fullmatch(r"assessment_[0-9a-f]{8}-(?:[0-9a-f]{4}-){3}[0-9a-f]{12}", assessment):
                raise Invalid("completed response lacks assessment identity")
            if not isinstance(proof, str) or not re.fullmatch(r"venue-assessment:[0-9a-f]{64}", proof):
                raise Invalid("completed response lacks assessment evidence reference")
            if not isinstance(venue, str) or not venue.strip() or len(venue.encode()) > 256 or any(ord(c) < 32 or 127 <= ord(c) <= 159 for c in venue):
                raise Invalid("completed response lacks selected venue")
            venue_label = json.dumps(venue, ensure_ascii=True)
            text += f" Selected venue (quoted data): {venue_label}. Assessment {assessment}. Evidence {proof}."
            text += " The backend recorded an accepted feasible replacement assessment. This is not a booking or mission completion."
        elif phase == "retired":
            text += " Local work authority has ended; remote cancellation is not implied."
        elif attention is None:
            text += f" The organization worker continues asynchronously. Use status {name} for progress."
        text += " Public registration and live provider execution need separate evidence."
        return text
