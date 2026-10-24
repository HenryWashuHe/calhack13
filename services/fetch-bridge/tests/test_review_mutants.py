"""Review tests that pin refusal guards which the existing suite left unexercised.

Each test targets a guard that a mutation pass (forcing the guard false or
shifting its comparison boundary) showed could change without failing any test.
They are test-only: production code is unchanged.
"""

from __future__ import annotations

import gzip
import os
import stat
import tempfile
import time
import unittest
from datetime import UTC, datetime, timedelta
from pathlib import Path
from unittest import mock
from uuid import uuid4

import test_bridge as transport
from aiohttp import web
from bech32 import bech32_encode
from genesis_fetch_bridge.bridge import Bridge, peer_url
from genesis_fetch_bridge.specialist import Specialist
from genesis_fetch_bridge.store import Store
from genesis_fetch_bridge.venue_selection import select_venue
from genesis_fetch_bridge.wire import (
    PROTOCOL_DIGEST,
    SCHEMA_DIGEST,
    GenesisMessage,
    Invalid,
    address,
    canonical,
    decode,
    timestamp,
    validate,
)
from test_venue_selection import bind, candidate, task_input
from uagents_core.envelope import Envelope

IDENTITIES = transport.IDENTITIES
message = transport.message
stamp = transport.stamp


def whole_second(delta):
    """A message expiry with no fractional part, comparable to an integer envelope expiry."""
    instant = int(time.time()) + delta
    return instant, datetime.fromtimestamp(instant, UTC).isoformat().replace("+00:00", "Z")


def task_message(job="job", deadline=None, expires_at=None):
    value = transport.WireTests().task()
    value["body"]["task"].update(job_id=job, attempt_id=job + "-attempt", idempotency_key=job + "-key")
    if deadline is not None:
        value["body"]["task"]["deadline"] = deadline
    if expires_at is not None:
        value["expires_at"] = expires_at
    return value


class SpecialistRefusalTests(transport.StoreCase):
    def process(self, value, sender=IDENTITIES[1].address, clock=None):
        self.store.receive(sender, value, int(time.time()) + 60)
        if clock is None:
            self.assertTrue(Specialist(self.store).step())
        else:
            with mock.patch("genesis_fetch_bridge.specialist.now", side_effect=clock):
                self.assertTrue(Specialist(self.store).step())

    def recruit(self, lease=None):
        value = message()
        if lease is not None:
            value["body"]["lease_expires_at"] = lease
        self.process(value)
        return self.store.db.execute("SELECT * FROM specialist_contracts").fetchone()

    def accept_message(self, contract, expires_at=None):
        value = message(
            "accept_offer",
            body={"offer_message_id": contract["offer_message_id"], "offer_id": contract["offer_id"]},
        )
        if expires_at is not None:
            value["expires_at"] = expires_at
        return value

    def state(self):
        return self.store.db.execute("SELECT state FROM specialist_contracts").fetchone()[0]

    def kinds(self):
        return [decode(row[0])["body"]["kind"] for row in self.store.db.execute("SELECT payload FROM outbox ORDER BY rowid")]

    def count(self, table):
        return self.store.db.execute(f"SELECT COUNT(*) FROM {table}").fetchone()[0]

    def test_unknown_mode_is_refused_before_any_mode_is_persisted(self):
        # specialist.py:16
        for mode in ["oracle", "", "ADVISORY"]:
            with self.subTest(mode=mode), self.assertRaises(Invalid):
                Specialist(self.store, mode)
        self.assertIsNone(self.store.db.execute("SELECT value FROM metadata WHERE key='specialist_mode'").fetchone())

    def test_request_expiring_exactly_now_is_receipted_but_never_processed(self):
        # specialist.py:79 -- an instant equal to expiry is already expired (wire.py:209).
        value = message()
        expiry = timestamp(value["expires_at"])
        self.process(value, clock=lambda: expiry)
        self.assertEqual(self.count("specialist_contracts"), 0)
        self.assertEqual(self.count("outbox"), 0)
        cursor = self.store.db.execute("SELECT value FROM metadata WHERE key='specialist_cursor'").fetchone()
        self.assertEqual(cursor[0], "1")

    def test_acceptance_cannot_outlive_the_lease_but_may_end_exactly_at_it(self):
        # specialist.py:147
        lease = stamp(180)
        contract = self.recruit(lease)
        self.process(self.accept_message(contract, stamp(240)))
        self.assertEqual(self.state(), "offered")
        self.assertEqual(self.kinds(), ["offer"])
        self.process(self.accept_message(contract, lease))
        self.assertEqual(self.state(), "active")
        self.assertEqual(self.kinds(), ["offer", "contract_accepted"])

    def test_task_request_outliving_the_lease_is_refused(self):
        # specialist.py:147 for work: the task itself is otherwise valid and fresh.
        contract = self.recruit(stamp(180))
        self.process(self.accept_message(contract))
        self.assertEqual(self.state(), "active")
        self.process(task_message(expires_at=stamp(240)))
        self.assertEqual(self.count("specialist_tasks"), 0)
        self.assertNotIn("task_result", self.kinds())

    def test_lease_reaching_its_expiry_during_processing_refuses_acceptance(self):
        # specialist.py:148 -- the clock reads before expiry at intake and exactly the
        # lease instant at the lease check, so only the lease guard can refuse.
        lease = stamp(180)
        contract = self.recruit(lease)
        readings = iter([timestamp(stamp(0))])
        self.process(self.accept_message(contract, lease), clock=lambda: next(readings, timestamp(lease)))
        self.assertEqual(self.state(), "offered")
        self.assertEqual(self.kinds(), ["offer"])

    def test_acceptance_must_name_the_exact_offer(self):
        # specialist.py:167 (multi-line guard the line-based mutator did not reach)
        contract = self.recruit()
        for field, value in [("offer_id", "offer-other"), ("offer_message_id", str(uuid4()))]:
            with self.subTest(field=field):
                wrong = self.accept_message(contract)
                wrong["body"][field] = value
                self.process(wrong)
                self.assertEqual(self.state(), "offered")
                self.assertEqual(self.kinds(), ["offer"])
        self.process(self.accept_message(contract))
        self.assertEqual(self.state(), "active")

    def test_released_contract_is_not_released_or_acknowledged_twice(self):
        # specialist.py:152
        self.process(self.accept_message(self.recruit()))
        self.process(message("release_contract", body={"reason": "retire"}))
        self.assertEqual(self.state(), "released")
        self.process(message("release_contract", body={"reason": "retire again"}))
        self.assertEqual(self.kinds().count("contract_released"), 1)

    def test_task_whose_deadline_passed_before_processing_is_not_executed(self):
        # specialist.py:189 -- the enclosing message and lease remain live, only the deadline lapsed.
        self.process(self.accept_message(self.recruit()))
        for job, offset in [("at-deadline", 0), ("after-deadline", 1)]:
            with self.subTest(job=job):
                value = task_message(job, deadline=stamp(60), expires_at=stamp(120))
                passed = timestamp(value["body"]["task"]["deadline"]) + timedelta(seconds=offset)
                self.process(value, clock=lambda: passed)
                self.assertEqual(self.count("specialist_tasks"), 0)
                self.assertFalse({"task_accepted", "task_result"} & set(self.kinds()))
        # Control: a live deadline is executed by the same contract.
        self.process(task_message("live"))
        self.assertEqual(self.count("specialist_tasks"), 1)
        self.assertIn("task_result", self.kinds())


def signed(value, expires, sender=IDENTITIES[0], target=IDENTITIES[1].address):
    env = Envelope(
        version=1,
        sender=sender.address,
        target=target,
        session=uuid4(),
        schema_digest=SCHEMA_DIGEST,
        protocol_digest=PROTOCOL_DIGEST,
        expires=expires,
    )
    env.encode_payload(GenesisMessage(wire_json=canonical(value)).model_dump_json())
    env.sign(sender)
    return env.model_dump(mode="json")


class BridgeRefusalTests(unittest.IsolatedAsyncioTestCase):
    asyncSetUp = transport.HttpTests.asyncSetUp
    asyncTearDown = transport.HttpTests.asyncTearDown
    post = transport.HttpTests.post

    async def raw(self, path, data, headers, base=None):
        async with self.client.post((base or self.base) + path, data=data, headers=headers) as response:
            return response.status

    async def get(self, query):
        async with self.client.get(
            self.base + "/v1/inbox" + query, headers={"Authorization": "Bearer " + transport.TOKEN}
        ) as response:
            return response.status

    def count(self, table):
        return self.bridge.store.db.execute(f"SELECT COUNT(*) FROM {table}").fetchone()[0]

    def idle_bridge(self, peers):
        """A bridge whose app and background worker are not running, so only the test sends."""
        bridge = Bridge(
            seed=transport.SEEDS[1],
            token=transport.TOKEN,
            db=str(Path(self.tmp.name) / f"idle-{uuid4()}.db"),
            peers=peers,
            http_timeout=0.5,
        )
        self.addCleanup(bridge.store.close)
        return bridge

    def queued(self, value=None):
        return {"schema_version": 1, "recipient": IDENTITIES[0].address, "message": value or message()}

    async def test_queue_refuses_valid_but_unconfigured_recipient(self):
        # bridge.py:186
        request = self.queued()
        request["recipient"] = IDENTITIES[2].address
        self.assertEqual(address(request["recipient"]), request["recipient"])
        self.assertEqual((await self.post("/v1/messages", request, True))[0], 400)
        self.assertEqual(self.count("outbox"), 0)

    async def test_queue_refuses_other_request_schema_versions(self):
        # bridge.py:183
        for version in [2, 0, True, "1", 1.0]:
            request = self.queued()
            request["schema_version"] = version
            with self.subTest(version=version):
                self.assertEqual((await self.post("/v1/messages", request, True))[0], 400)
        self.assertEqual(self.count("outbox"), 0)

    async def test_non_json_content_type_is_refused(self):
        # bridge.py:155
        auth = {"Authorization": "Bearer " + transport.TOKEN}
        for content_type in ["text/plain", "application/x-www-form-urlencoded"]:
            with self.subTest(content_type=content_type):
                headers = {"Content-Type": content_type}
                self.assertEqual(
                    await self.raw("/v1/messages", canonical(self.queued()), {**headers, **auth}), 400
                )
                self.assertEqual(await self.raw("/submit", canonical(transport.envelope(message())), headers), 400)
        self.assertEqual(self.count("outbox") + self.count("inbox"), 0)

    async def test_compressed_bodies_are_refused_even_when_the_server_decompresses(self):
        # bridge.py:150 -- independent of the runner's auto_decompress setting.
        headers = {"Content-Type": "application/json", "Content-Encoding": "gzip"}
        plain = canonical(transport.envelope(message())).encode()
        self.assertEqual(await self.raw("/submit", plain, headers), 400)
        # A separate bridge served with aiohttp's default auto_decompress=True.
        other = self.idle_bridge({IDENTITIES[0].address: "http://127.0.0.1:1/submit"})
        runner = web.AppRunner(other.app)
        await runner.setup()
        site = web.TCPSite(runner, "127.0.0.1", 0)
        await site.start()
        try:
            base = f"http://127.0.0.1:{site._server.sockets[0].getsockname()[1]}"
            json_headers = {"Content-Type": "application/json"}
            # Control: the same runner accepts the uncompressed envelope.
            self.assertEqual(await self.raw("/submit", canonical(transport.envelope(message())), json_headers, base), 200)
            compressed = gzip.compress(canonical(transport.envelope(message())).encode())
            self.assertEqual(await self.raw("/submit", compressed, headers, base), 400)
            auth = {"Authorization": "Bearer " + transport.TOKEN}
            self.assertEqual(
                await self.raw("/v1/messages", gzip.compress(canonical(self.queued()).encode()), {**headers, **auth}, base),
                400,
            )
            self.assertEqual(other.store.db.execute("SELECT COUNT(*) FROM inbox").fetchone()[0], 1)
            self.assertEqual(other.store.db.execute("SELECT COUNT(*) FROM outbox").fetchone()[0], 0)
        finally:
            await runner.cleanup()  # Its lifecycle also closes the store; closing twice is harmless.
        self.assertEqual(self.count("inbox"), 0)

    async def test_inbox_parameters_are_single_and_bounded(self):
        # bridge.py:202 and bridge.py:217
        for query, status in [
            ("?after=0&limit=50", 200),
            ("?after=9223372036854775807&limit=1", 200),
            ("?after=0&after=1", 400),
            ("?limit=1&limit=2", 400),
            ("?limit=0", 400),
            ("?limit=51", 400),
            ("?after=9223372036854775808", 400),
            ("?cursor=1", 400),  # bridge.py:201
            ("?after=%D9%A1", 400),  # bridge.py:209: int() would accept an Arabic-Indic digit
            ("?limit=%2B5", 400),
            ("?limit=%205", 400),
        ]:
            with self.subTest(query=query):
                self.assertEqual(await self.get(query), status)

    async def test_transport_expiry_cannot_outlive_the_signed_message(self):
        # bridge.py:270 -- including the exact boundary, which is allowed.
        instant, expires_at = whole_second(60)
        later = message()
        later["expires_at"] = expires_at
        self.assertEqual((await self.post("/submit", signed(later, instant + 1)))[0], 400)
        self.assertEqual(self.count("inbox"), 0)
        equal = message()
        equal["expires_at"] = expires_at
        self.assertEqual((await self.post("/submit", signed(equal, instant)))[0], 200)
        self.assertEqual(self.count("inbox"), 1)

    async def test_validly_signed_envelope_with_foreign_schema_digest_is_refused(self):
        # bridge.py:245 -- the existing test edits the digest after signing, so the signature
        # check alone refused it. Here the foreign digest is signed.
        env = Envelope.model_validate(transport.envelope(message()))
        env.schema_digest = "model:" + "0" * 64
        env.sign(IDENTITIES[0])
        self.assertTrue(env.verify())
        self.assertEqual((await self.post("/submit", env.model_dump(mode="json")))[0], 400)
        self.assertEqual(self.count("inbox"), 0)

    async def test_nonce_must_be_a_json_integer_even_when_the_signature_verifies(self):
        # bridge.py:253 -- the SDK coerces true/1.0 to nonce 1 and the signature over 1 verifies.
        env = Envelope.model_validate(transport.envelope(message()))
        env.nonce = 1
        env.sign(IDENTITIES[0])
        signed_env = env.model_dump(mode="json")
        for nonce in [True, 1.0]:
            changed = {**signed_env, "nonce": nonce}
            self.assertTrue(Envelope.model_validate(changed).verify())
            with self.subTest(nonce=nonce):
                self.assertEqual((await self.post("/submit", changed))[0], 400)
        self.assertEqual(self.count("inbox"), 0)
        self.assertEqual((await self.post("/submit", signed_env))[0], 200)

    async def send_with_status(self, status):
        hits = []

        async def submit(request):
            hits.append(await request.read())
            return web.json_response({}, status=status)

        app = web.Application()
        app.router.add_post("/submit", submit)
        runner = web.AppRunner(app)
        await runner.setup()
        site = web.TCPSite(runner, "127.0.0.1", 0)
        await site.start()
        try:
            port = site._server.sockets[0].getsockname()[1]
            bridge = self.idle_bridge({IDENTITIES[0].address: f"http://127.0.0.1:{port}/submit"})
            value = message()
            bridge.store.queue(IDENTITIES[0].address, value)
            await bridge.send_one(self.client, bridge.store.next_outgoing())
        finally:
            await runner.cleanup()
        self.assertEqual(len(hits), 1)
        return bridge.store.db.execute(
            "SELECT state,attempts FROM outbox WHERE message_id=?", (value["message_id"],)
        ).fetchone()

    async def test_only_http_200_marks_delivery(self):
        # bridge.py:344
        self.assertEqual(tuple(await self.send_with_status(200)), ("delivered", 0))
        for status in [202, 500]:
            with self.subTest(status=status):
                self.assertEqual(tuple(await self.send_with_status(status)), ("queued", 1))

    async def test_queued_message_for_a_removed_peer_is_retried_not_sent_or_crashed(self):
        # bridge.py:316 -- e.g. restart with a peer file that no longer lists the recipient.
        bridge = self.idle_bridge({IDENTITIES[0].address: "http://127.0.0.1:1/submit"})
        bridge.store.queue(IDENTITIES[0].address, message())
        del bridge.peers[IDENTITIES[0].address]
        await bridge.send_one(self.client, bridge.store.next_outgoing())
        row = bridge.store.db.execute("SELECT state,attempts,retry_at FROM outbox").fetchone()
        self.assertEqual((row[0], row[1]), ("queued", 1))
        self.assertGreater(row[2], time.time())


class WireRefusalTests(unittest.TestCase):
    def test_peer_url_requires_a_host_and_nonzero_port(self):
        # bridge.py:54
        for value in ["https:///submit", "https://example.org:0/submit", "http://127.0.0.1:0/submit"]:
            with self.subTest(value=value), self.assertRaises(Invalid):
                peer_url(value)

    def test_uppercase_address_alias_is_refused(self):
        # wire.py:159 -- bech32 decodes an all-uppercase string to the same key.
        with self.assertRaises(Invalid):
            address(IDENTITIES[0].address.upper())

    def test_bridge_refuses_short_seeds_and_malformed_tokens(self):
        # bridge.py:102 and bridge.py:108
        with tempfile.TemporaryDirectory() as tmp:
            db = str(Path(tmp) / "bridge.db")
            for seed, token in [
                ("too short", transport.TOKEN),
                ("x" * 4097, transport.TOKEN),
                (transport.SEEDS[0], "short"),
                (transport.SEEDS[0], transport.TOKEN + " with space"),
                (transport.SEEDS[0], "t" * 513),
                (transport.SEEDS[0], "\u00e9" * 40),
            ]:
                with self.subTest(seed=len(seed), token=token[:12]), self.assertRaises(Invalid):
                    Bridge(seed=seed, token=token, db=db, peers={})
            self.assertFalse(Path(db).exists())

    def test_only_message_schema_version_1_is_supported(self):
        # wire.py:203
        for version in [2, 0, True, "1", 1.0]:
            value = message()
            value["schema_version"] = version
            with self.subTest(version=version), self.assertRaises(Invalid):
                validate(value)

    def test_message_expiring_exactly_now_is_expired(self):
        # wire.py:209
        value = message()
        with mock.patch("genesis_fetch_bridge.wire.now", return_value=timestamp(value["expires_at"])):
            with self.assertRaises(Invalid):
                validate(value)
            self.assertEqual(validate(value, fresh=False), value)

    def test_task_deadline_may_equal_expiry_but_not_now(self):
        # wire.py:284 (both comparisons)
        value = task_message(deadline=stamp(60), expires_at=stamp(60))
        value["body"]["task"]["deadline"] = value["expires_at"]
        self.assertEqual(validate(value), value)
        value = task_message(deadline=stamp(60), expires_at=stamp(120))
        with mock.patch("genesis_fetch_bridge.wire.now", return_value=timestamp(value["body"]["task"]["deadline"])):
            with self.assertRaises(Invalid):
                validate(value)
            self.assertEqual(validate(value, fresh=False), value)

    def test_body_must_be_an_object_with_a_known_kind(self):
        # wire.py:212 and wire.py:233 -- refusals must be Invalid, not a crash.
        for body in [[], "recruit_request", None, 1]:
            value = message()
            value["body"] = body
            with self.subTest(body=body), self.assertRaises(Invalid):
                validate(value)
        for kind in ["grant_authority", None, ["offer"], 1]:
            value = message()
            value["body"]["kind"] = kind
            with self.subTest(kind=kind), self.assertRaises(Invalid):
                validate(value)
        value = message()
        del value["body"]["kind"]
        with self.assertRaises(Invalid):
            validate(value)

    def test_message_references_must_be_canonical_uuids(self):
        # wire.py:243
        bodies = {
            "accept_offer": {"offer_message_id": "offer-1", "offer_id": "offer-1"},
            "contract_accepted": {"accept_message_id": "not-a-uuid", "offer_id": "offer-1"},
            "task_accepted": {
                "task_request_id": str(uuid4()).upper(),
                "job_id": "job",
                "attempt_id": "attempt",
            },
            "contract_released": {"release_message_id": 7},
            "offer": {
                "recruit_message_id": "",
                "offer_id": "offer-1",
                "capabilities": ["synthetic.advice"],
                "terms": {},
                "lease_expires_at": stamp(180),
            },
        }
        for kind, body in bodies.items():
            with self.subTest(kind=kind), self.assertRaises(Invalid):
                validate(message(kind, body=body))

    def test_offer_job_and_attempt_identifiers_are_checked(self):
        # wire.py:246
        good = {"task_request_id": str(uuid4()), "job_id": "job", "attempt_id": "attempt"}
        validate(message("task_accepted", body=good))
        for key, bad in [("job_id", "job/1"), ("attempt_id", ""), ("job_id", 7)]:
            with self.subTest(key=key, bad=bad), self.assertRaises(Invalid):
                validate(message("task_accepted", body={**good, key: bad}))
        with self.assertRaises(Invalid):
            validate(message("accept_offer", body={"offer_message_id": str(uuid4()), "offer_id": "has space"}))

    def test_nested_terms_and_output_are_bounded_below_the_message_limit(self):
        # wire.py:256
        large = "a" * (33 * 1024)
        result = {
            "task_request_id": str(uuid4()),
            "job_id": "job",
            "attempt_id": "attempt",
            "epoch": 1,
            "output": large,
        }
        offer = {
            "recruit_message_id": str(uuid4()),
            "offer_id": "offer-1",
            "capabilities": ["synthetic.advice"],
            "terms": {"note": large},
            "lease_expires_at": stamp(180),
        }
        for kind, body in [("task_result", result), ("offer", offer)]:
            self.assertLess(len(canonical(message(kind, body=body))), 64 * 1024)
            with self.subTest(kind=kind), self.assertRaises(Invalid):
                validate(message(kind, body=body))

    def test_result_epoch_must_be_an_unsigned_integer(self):
        # wire.py:258
        body = {"task_request_id": str(uuid4()), "job_id": "job", "attempt_id": "attempt", "epoch": 1, "output": None}
        validate(message("task_result", body=body))
        for epoch in [True, -1, 2**64, 1.0, "1"]:
            with self.subTest(epoch=epoch), self.assertRaises(Invalid):
                validate(message("task_result", body={**body, "epoch": epoch}))

    def test_message_may_expire_exactly_at_its_requested_lease(self):
        # wire.py:253
        value = message()
        value["body"]["lease_expires_at"] = value["expires_at"]
        self.assertEqual(validate(value), value)

    def test_timestamps_must_be_utc_rfc3339(self):
        # wire.py:122 -- fromisoformat alone would accept offsets, naive times and other forms.
        for bad in ["2030-01-01T00:00:00+01:00", "2030-01-01T00:00:00", "2030-01-01", "20300101T000000Z", 1893456000]:
            with self.subTest(bad=bad):
                with self.assertRaises(Invalid):
                    timestamp(bad)
                value = message()
                value["expires_at"] = bad
                with self.assertRaises(Invalid):
                    validate(value)

    def test_alternate_bech32_prefixes_of_agent_length_are_refused(self):
        # Pins the property behind wire.py:167/170 (see review notes: those guards are
        # redundant with the length and curve-key checks for every 65-character input).
        for hrp in ["agent1", "agent1q", "agent1qq"]:
            value = bech32_encode(hrp, [2] * (58 - len(hrp)))
            self.assertEqual(len(value), 65)
            with self.subTest(hrp=hrp), self.assertRaises(Invalid):
                address(value)


class SelectionRefusalTests(unittest.TestCase):
    def refused(self, mutate):
        context = task_input()["context"]
        mutate(context)
        with self.assertRaises(Invalid):
            select_venue(bind(context))

    def test_windows_must_have_positive_length(self):
        # venue_selection.py:96
        for target in ["policy", "candidate"]:
            for end in ["2026-11-01T19:00:00Z", "2026-11-01T18:59:59.999999999Z"]:
                def mutate(c, target=target, end=end):
                    window = c["policy"]["window"] if target == "policy" else c["candidates"][0]["venue"]["window"]
                    window.update(start="2026-11-01T19:00:00Z", end=end)
                with self.subTest(target=target, end=end):
                    self.refused(mutate)

    def test_selection_text_fields_are_trimmed_bounded_and_control_free(self):
        # venue_selection.py:27
        for value in ["", " padded", "padded ", "a\x00b", "x" * 257, 7]:
            with self.subTest(value=value):
                self.refused(lambda c, v=value: c["candidates"][0]["venue"].update(venue_id=v))
                self.refused(lambda c, v=value: c["candidates"][0].update(source_job_id=v))

    def test_selection_schema_and_recovery_identifier_are_checked(self):
        # venue_selection.py:147 and venue_selection.py:154
        for change in [{"schema_version": 2}, {"schema_version": True}, {"kind": "venue_replacement_v2"}]:
            value = {**task_input(), **change}
            with self.subTest(change=change), self.assertRaises(Invalid):
                select_venue(value)
        for recovery_id in [123, None, ["recovery-a"]]:
            with self.subTest(recovery_id=recovery_id):
                self.refused(lambda c, v=recovery_id: c.update(recovery_id=v))

    def test_availability_is_a_closed_enum(self):
        # venue_selection.py:119
        for value in ["maybe", "Available", True, None]:
            with self.subTest(value=value):
                self.refused(lambda c, value=value: c["candidates"][0]["venue"].update(availability=value))

    def test_invocation_reference_requires_its_prefix(self):
        # venue_selection.py:173 -- a bare hash would otherwise pass _hash after removeprefix.
        for proof in ["a" * 64, "Invocation:" + "a" * 64, "proof:" + "a" * 64, 123, None]:
            with self.subTest(proof=proof):
                self.refused(lambda c, proof=proof: c["candidates"][0].update(invocation_proof_ref=proof))

    def test_currency_must_be_three_uppercase_letters(self):
        # venue_selection.py:63
        for currency in ["usd", "US", "USDX", 840, None]:
            with self.subTest(currency=currency):
                self.refused(lambda c, v=currency: c["candidates"][0]["venue"]["price"].update(currency=v))
                self.refused(lambda c, v=currency: c["policy"]["budget"].update(currency=v))

    def test_non_rfc3339_time_text_is_refused_as_invalid(self):
        # venue_selection.py:71 -- must be a sanitized Invalid, not an AttributeError.
        for value in ["tomorrow evening", "2026-11-01 19:00:00Z", "2026-11-01T19:00Z"]:
            with self.subTest(value=value):
                self.refused(lambda c, v=value: c["policy"]["window"].update(start=v))

    def test_maximum_valid_offsets_are_accepted_and_compared_as_instants(self):
        # venue_selection.py:82 -- +23:59 is valid; only 24 hours or 60 minutes are refused.
        request = task_input()
        request["context"]["candidates"][0]["venue"]["window"] = {
            "start": "2026-11-02T17:59:00+23:59",  # 2026-11-01T18:00:00Z
            "end": "2026-11-02T21:00:00+23:00",  # 2026-11-01T22:00:00Z
        }
        output = select_venue(bind(request["context"]))
        self.assertTrue(output["evaluations"][0]["checks"]["window"])
        self.assertEqual(output["selected"]["venue_id"], "replacement")


class StoreRefusalTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.root = Path(self.tmp.name)

    def tearDown(self):
        self.tmp.cleanup()

    def test_database_path_that_is_a_symlink_is_refused_without_touching_its_target(self):
        # store.py:34
        target = self.root / "elsewhere"
        target.write_bytes(b"not a database")
        os.chmod(target, 0o644)
        link = self.root / "bridge.db"
        os.symlink(target, link)
        with self.assertRaises(Invalid):
            Store(str(link), IDENTITIES[0].address)
        self.assertEqual(target.read_bytes(), b"not a database")
        self.assertEqual(stat.S_IMODE(os.stat(target).st_mode), 0o644)

    def test_envelope_expiring_exactly_now_is_not_receipted(self):
        # store.py:138
        store = Store(str(self.root / "bridge.db"), IDENTITIES[0].address)
        try:
            expiry = int(time.time()) + 30
            with mock.patch("genesis_fetch_bridge.store.now", return_value=datetime.fromtimestamp(expiry, UTC)):
                with self.assertRaises(Invalid):
                    store.receive(IDENTITIES[1].address, message(), expiry)
                self.assertTrue(store.receive(IDENTITIES[1].address, message(), expiry + 1))
            self.assertEqual(len(store.inbox(0, 50)["items"]), 1)
        finally:
            store.close()

    def test_failed_attempt_for_unknown_message_is_a_no_op(self):
        # store.py:183 -- defensive; the bridge only passes IDs it just read from the outbox.
        store = Store(str(self.root / "bridge.db"), IDENTITIES[0].address)
        try:
            store.failed_attempt(str(uuid4()))
            self.assertEqual(store.db.execute("SELECT COUNT(*) FROM outbox").fetchone()[0], 0)
        finally:
            store.close()


if __name__ == "__main__":
    unittest.main()
