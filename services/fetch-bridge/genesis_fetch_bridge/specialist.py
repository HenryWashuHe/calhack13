"""Independent contractor modes; only the kernel can verify domain success."""

from __future__ import annotations

from uuid import NAMESPACE_URL, uuid5

from .store import Conflict, Store
from .venue_selection import select_venue
from .wire import Invalid, canonical, decode, now, timestamp

MODES = ("advisory", "venue_selection")


class Specialist:
    def __init__(self, store: Store, mode: str = "advisory"):
        if mode not in MODES:
            raise Invalid("invalid specialist mode")
        self.store = store
        self.mode = mode
        # Mode is part of this specialist's persistent identity. In particular,
        # legacy advisory contracts must never become selection contracts merely
        # because a restarted process received a different command-line flag.
        with store.transaction():
            saved = store.db.execute(
                "SELECT value FROM metadata WHERE key='specialist_mode'"
            ).fetchone()
            legacy = store.db.execute(
                "SELECT EXISTS(SELECT 1 FROM specialist_contracts)"
            ).fetchone()[0]
            previous = saved[0] if saved else ("advisory" if legacy else mode)
            if previous != mode:
                raise Conflict("specialist mode does not match database")
            store.db.execute(
                "INSERT OR IGNORE INTO metadata VALUES('specialist_mode',?)", (mode,)
            )

    def _reply(self, message: dict, kind: str, body: dict, suffix: str = "") -> dict:
        return {
            **{
                key: message[key]
                for key in (
                    "schema_version",
                    "org_id",
                    "mission_id",
                    "contract_id",
                    "correlation_id",
                    "expires_at",
                )
            },
            "message_id": str(
                uuid5(
                    NAMESPACE_URL,
                    self.store.identity
                    + ":"
                    + message["message_id"]
                    + ":"
                    + kind
                    + suffix,
                )
            ),
            "body": {"kind": kind, **body},
        }

    def step(self) -> bool:
        db = self.store.db
        with self.store.transaction():
            cursor = db.execute(
                "SELECT value FROM metadata WHERE key='specialist_cursor'"
            ).fetchone()
            after = int(cursor[0]) if cursor else 0
            row = db.execute(
                "SELECT * FROM inbox WHERE sequence>? ORDER BY sequence LIMIT 1",
                (after,),
            ).fetchone()
            if row is None:
                return False
            message = decode(row["payload"])
            # An inbox receipt survives expiry, but late requests never trigger new effects.
            if timestamp(message["expires_at"]) > now():
                self._process(row["sender"], message)
            db.execute(
                "INSERT INTO metadata(key,value) VALUES('specialist_cursor',?) ON CONFLICT(key) DO UPDATE SET value=excluded.value",
                (str(row["sequence"]),),
            )
        return True

    def _process(self, sender: str, message: dict):
        db = self.store.db
        body = message["body"]
        kind = body["kind"]
        key = (message["org_id"], message["contract_id"])
        contract = db.execute(
            "SELECT * FROM specialist_contracts WHERE org_id=? AND contract_id=?", key
        ).fetchone()
        if kind == "recruit_request":
            if contract is not None:
                return  # A contract ID is never repurposed, including after retirement.
            if self.mode == "venue_selection" and set(body["capabilities"]) != {
                "venue.select"
            }:
                return  # This specialist cannot promise arbitrary capabilities.
            terms = (
                {
                    "scope": "venue_selection",
                    "kind": "venue_replacement_v1",
                    "domain_verified": False,
                }
                if self.mode == "venue_selection"
                else {"synthetic": True, "scope": "advisory_only", "domain_verified": False}
            )
            reply = self._reply(
                message,
                "offer",
                {
                    "recruit_message_id": message["message_id"],
                    "offer_id": "offer-" + message["message_id"],
                    "capabilities": body["capabilities"],
                    "terms": terms,
                    "lease_expires_at": body["lease_expires_at"],
                },
            )
            self.store._capacity()
            db.execute(
                "INSERT INTO specialist_contracts VALUES(?,?,?,?,?,?,?,?,?,?)",
                (
                    *key,
                    sender,
                    message["mission_id"],
                    message["correlation_id"],
                    "offered",
                    body["lease_expires_at"],
                    reply["message_id"],
                    reply["body"]["offer_id"],
                    canonical(body["capabilities"]),
                ),
            )
            self.store._queue(sender, reply)
            return
        if (
            contract is None
            or contract["sender"] != sender
            or contract["mission_id"] != message["mission_id"]
            or contract["correlation_id"] != message["correlation_id"]
        ):
            return
        if kind != "release_contract" and (
            timestamp(message["expires_at"]) > timestamp(contract["lease"])
            or timestamp(contract["lease"]) <= now()
        ):
            return
        if kind == "release_contract":
            if contract["state"] == "released":
                return
            db.execute(
                "UPDATE specialist_contracts SET state='released' WHERE org_id=? AND contract_id=?",
                key,
            )
            self.store._queue(
                sender,
                self._reply(
                    message,
                    "contract_released",
                    {"release_message_id": message["message_id"]},
                ),
            )
        elif kind == "accept_offer" and contract["state"] == "offered":
            if (
                body["offer_id"] != contract["offer_id"]
                or body["offer_message_id"] != contract["offer_message_id"]
            ):
                return
            db.execute(
                "UPDATE specialist_contracts SET state='active' WHERE org_id=? AND contract_id=?",
                key,
            )
            self.store._queue(
                sender,
                self._reply(
                    message,
                    "contract_accepted",
                    {
                        "accept_message_id": message["message_id"],
                        "offer_id": body["offer_id"],
                    },
                ),
            )
        elif kind == "task_request" and contract["state"] == "active":
            task = body["task"]
            if timestamp(task["deadline"]) <= now():
                return
            previous = db.execute(
                "SELECT task FROM specialist_tasks WHERE org_id=? AND contract_id=? AND (idempotency_key=? OR (job_id=? AND attempt_id=?))",
                (*key, task["idempotency_key"], task["job_id"], task["attempt_id"]),
            ).fetchone()
            if previous:
                # Existing task receipt owns this idempotency key. Changed requests cannot execute.
                return
            self.store._capacity()
            db.execute(
                "INSERT INTO specialist_tasks VALUES(?,?,?,?,?,?)",
                (
                    *key,
                    task["idempotency_key"],
                    task["job_id"],
                    task["attempt_id"],
                    canonical(task),
                ),
            )
            common = {
                "task_request_id": message["message_id"],
                "job_id": task["job_id"],
                "attempt_id": task["attempt_id"],
            }
            if self.mode == "venue_selection":
                try:
                    output = select_venue(task["input"])
                except Invalid:
                    # Null is invalid domain evidence, not a fabricated
                    # no-match decision. Persist the refusal with this exact
                    # task attempt so restart or replay cannot reinterpret it.
                    output = None
            else:
                output = {
                    "synthetic": True,
                    "domain_verified": False,
                    "advisory": "Independent specialist received this task. Verify all mission outcomes in the organization kernel.",
                    "specialist_address": self.store.identity,
                    "capabilities": decode(contract["capabilities"]),
                }
            self.store._queue(sender, self._reply(message, "task_accepted", common))
            self.store._queue(
                sender,
                self._reply(
                    message,
                    "task_result",
                    {
                        **common,
                        "epoch": task["epoch"],
                        "output": output,
                    },
                ),
            )
