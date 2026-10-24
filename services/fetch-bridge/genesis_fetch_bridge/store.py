"""Local durable transport evidence. A receipt is never a kernel command."""

from __future__ import annotations

import os
import sqlite3
from collections.abc import Iterator
from contextlib import contextmanager
from pathlib import Path

from .wire import Invalid, canonical, decode, now, validate


class Conflict(Invalid):
    pass


class Capacity(Invalid):
    pass


class Store:
    def __init__(
        self,
        path: str,
        identity: str,
        max_records: int = 10000,
        max_bytes: int = 64 * 1024 * 1024,
    ):
        if not 1 <= max_records <= 100000 or not 1024 * 1024 <= max_bytes <= 1024**3:
            raise Invalid("invalid storage limits")
        target = Path(path)
        target.parent.mkdir(parents=True, exist_ok=True)
        if target.is_symlink():
            raise Invalid("database must not be a symlink")
        fd = os.open(target, os.O_CREAT | os.O_RDWR, 0o600)
        os.close(fd)
        os.chmod(target, 0o600)
        self.db = sqlite3.connect(path, isolation_level=None, timeout=5)
        self.db.row_factory = sqlite3.Row
        self.db.execute("PRAGMA synchronous=FULL")
        self.db.execute("PRAGMA journal_mode=DELETE")
        self.db.execute("PRAGMA foreign_keys=ON")
        page_size = self.db.execute("PRAGMA page_size").fetchone()[0]
        self.db.execute(f"PRAGMA max_page_count={max_bytes // page_size}")
        self.db.executescript("""
        CREATE TABLE IF NOT EXISTS metadata(key TEXT PRIMARY KEY, value TEXT NOT NULL);
        CREATE TABLE IF NOT EXISTS outbox(
          message_id TEXT PRIMARY KEY, recipient TEXT NOT NULL, payload TEXT NOT NULL,
          state TEXT NOT NULL DEFAULT 'queued', attempts INTEGER NOT NULL DEFAULT 0,
          retry_at REAL NOT NULL DEFAULT 0);
        CREATE TABLE IF NOT EXISTS inbox(
          sequence INTEGER PRIMARY KEY AUTOINCREMENT, message_id TEXT NOT NULL UNIQUE,
          sender TEXT NOT NULL, payload TEXT NOT NULL);
        CREATE TABLE IF NOT EXISTS specialist_contracts(
          org_id TEXT NOT NULL, contract_id TEXT NOT NULL, sender TEXT NOT NULL,
          mission_id TEXT NOT NULL, correlation_id TEXT NOT NULL,
          state TEXT NOT NULL, lease TEXT NOT NULL,
          offer_message_id TEXT NOT NULL, offer_id TEXT NOT NULL,
          capabilities TEXT NOT NULL, PRIMARY KEY(org_id,contract_id));
        CREATE TABLE IF NOT EXISTS specialist_tasks(
          org_id TEXT NOT NULL, contract_id TEXT NOT NULL, idempotency_key TEXT NOT NULL,
          job_id TEXT NOT NULL, attempt_id TEXT NOT NULL, task TEXT NOT NULL,
          PRIMARY KEY(org_id,contract_id,idempotency_key),
          UNIQUE(org_id,contract_id,job_id,attempt_id));
        """)
        self.max_records = max_records
        with self.transaction():
            existing = self.db.execute(
                "SELECT value FROM metadata WHERE key='identity'"
            ).fetchone()
            if existing and existing[0] != identity:
                raise Conflict("seed does not match database identity")
            self.db.execute(
                "INSERT OR IGNORE INTO metadata VALUES('identity',?)", (identity,)
            )
        self.identity = identity

    @contextmanager
    def transaction(self) -> Iterator[None]:
        self.db.execute("BEGIN IMMEDIATE")
        try:
            yield
            self.db.execute("COMMIT")
        except BaseException:
            if self.db.in_transaction:
                self.db.execute("ROLLBACK")
            raise

    def close(self):
        self.db.close()

    def _capacity(self):
        count = sum(
            self.db.execute(f"SELECT COUNT(*) FROM {table}").fetchone()[0]
            for table in ("inbox", "outbox", "specialist_contracts", "specialist_tasks")
        )
        if count >= self.max_records:
            raise Capacity("transport storage capacity reached")

    def _queue(self, recipient: str, message: dict):
        validate(message, fresh=False)
        payload = canonical(message)
        existing = self.db.execute(
            "SELECT recipient,payload FROM outbox WHERE message_id=?",
            (message["message_id"],),
        ).fetchone()
        if existing:
            if (existing[0], existing[1]) != (recipient, payload):
                raise Conflict("message identifier already has different content")
            return
        validate(message)
        self._capacity()
        self.db.execute(
            "INSERT INTO outbox(message_id,recipient,payload) VALUES(?,?,?)",
            (message["message_id"], recipient, payload),
        )

    def queue(self, recipient: str, message: dict):
        with self.transaction():
            self._queue(recipient, message)

    def receive(self, sender: str, message: dict, envelope_expiry: int) -> bool:
        validate(message, fresh=False)
        payload = canonical(message)
        with self.transaction():
            existing = self.db.execute(
                "SELECT sender,payload FROM inbox WHERE message_id=?",
                (message["message_id"],),
            ).fetchone()
            if existing:
                if (existing[0], existing[1]) != (sender, payload):
                    raise Conflict(
                        "received identifier has different sender or content"
                    )
                return False
            validate(message)
            if envelope_expiry <= now().timestamp():
                raise Invalid("expired transport envelope")
            self._capacity()
            self.db.execute(
                "INSERT INTO inbox(message_id,sender,payload) VALUES(?,?,?)",
                (message["message_id"], sender, payload),
            )
            return True

    def inbox(self, after: int, limit: int) -> dict:
        items = []
        # Leave headroom for the outer response envelope; never omit an item by advancing cursor.
        size = 512
        for row in self.db.execute(
            "SELECT sequence,sender,payload FROM inbox WHERE sequence>? ORDER BY sequence LIMIT ?",
            (after, limit),
        ):
            item = {"sequence": row[0], "sender": row[1], "message": decode(row[2])}
            item_size = len(canonical(item).encode())
            if size + item_size > 1024 * 1024:
                break
            items.append(item)
            size += item_size + 1
        return {
            "schema_version": 1,
            "address": self.identity,
            "items": items,
            "next_cursor": items[-1]["sequence"] if items else after,
        }

    def next_outgoing(self):
        return self.db.execute(
            "SELECT * FROM outbox WHERE state='queued' AND retry_at<=? ORDER BY rowid LIMIT 1",
            (now().timestamp(),),
        ).fetchone()

    def delivered(self, message_id: str):
        self.db.execute(
            "UPDATE outbox SET state='delivered' WHERE message_id=?", (message_id,)
        )

    def failed_attempt(self, message_id: str):
        row = self.db.execute(
            "SELECT attempts FROM outbox WHERE message_id=?", (message_id,)
        ).fetchone()
        if row:
            self.db.execute(
                "UPDATE outbox SET attempts=attempts+1,retry_at=? WHERE message_id=?",
                (now().timestamp() + min(60, 2 ** min(row[0], 5)), message_id),
            )

    def expire(self, message_id: str):
        self.db.execute(
            "UPDATE outbox SET state='expired' WHERE message_id=?", (message_id,)
        )
