from __future__ import annotations

import base64
import copy
import json
import os
import socket
import subprocess
import sys
import tempfile
import time
import unittest
import urllib.error
import urllib.request
from datetime import timedelta
from pathlib import Path
from uuid import uuid4

from aiohttp import ClientSession, web
from genesis_fetch_bridge.bridge import Bridge, peer_url
from genesis_fetch_bridge.specialist import Specialist
from genesis_fetch_bridge.store import Capacity, Conflict, Store
from genesis_fetch_bridge.wire import (
    PROTOCOL_DIGEST,
    SCHEMA_DIGEST,
    GenesisMessage,
    Invalid,
    address,
    canonical,
    decode,
    now,
    validate,
)
from uagents_core.envelope import Envelope
from uagents_core.identity import Identity

SEEDS = [
    "GENESIS local test identity " + str(i) + " never use in production"
    for i in range(4)
]
IDENTITIES = [Identity.from_seed(seed, 0) for seed in SEEDS]
TOKEN = "genesis-local-test-token-never-use-in-production"


def stamp(delta=120):
    return (now() + timedelta(seconds=delta)).isoformat().replace("+00:00", "Z")


def message(kind="recruit_request", org="org-a", contract="contract-a", body=None):
    return {
        "schema_version": 1,
        "message_id": str(uuid4()),
        "org_id": org,
        "mission_id": "mission-1",
        "contract_id": contract,
        "correlation_id": "correlation-1",
        "expires_at": stamp(),
        "body": {
            "kind": kind,
            **(
                body
                if body is not None
                else {
                    "requirement": "Assess a synthetic missing capability",
                    "capabilities": ["synthetic.advice"],
                    "lease_expires_at": stamp(180),
                }
            ),
        },
    }


def envelope(value, sender=IDENTITIES[0], target=IDENTITIES[1].address):
    env = Envelope(
        version=1,
        sender=sender.address,
        target=target,
        session=uuid4(),
        schema_digest=SCHEMA_DIGEST,
        protocol_digest=PROTOCOL_DIGEST,
        expires=int(now().timestamp()) + 60,
    )
    env.encode_payload(GenesisMessage(wire_json=canonical(value)).model_dump_json())
    env.sign(sender)
    return env.model_dump(mode="json")


class WireTests(unittest.TestCase):
    def test_schema_digest_is_stable_and_native(self):
        self.assertEqual(
            SCHEMA_DIGEST,
            "model:3d35410a0bac7663333dda8af6d33f8e7c4d2677da8dd74804a2559574d46e2b",
        )
        self.assertEqual(
            PROTOCOL_DIGEST,
            "proto:5ff9983d2f971db76bc44c6bb98c9de76f825023ebfff1366d0352a37f2b0756",
        )

    def test_reject_values_that_cannot_enter_rust_inbox(self):
        for change in ["uuid", "blank", "control", "unicode_control", "capability"]:
            value = message()
            if change == "uuid":
                value["message_id"] = "ABCDEFAB-1234-4123-8123-123456789012"
            elif change == "blank":
                value["body"]["requirement"] = "  \n\t"
            elif change == "control":
                value["body"]["requirement"] = "hello\x00"
            elif change == "unicode_control":
                value["body"]["requirement"] = "hello\x85"
            else:
                value["body"]["capabilities"] = ["contains spaces"]
            with self.subTest(change=change), self.assertRaises(Invalid):
                validate(value)

    def test_strict_json(self):
        for raw in [
            '{"a":1,"a":2}',
            '{"x":NaN}',
            '{"x":1e999}',
            '{"x":"\\ud800"}',
            '{"x":' + "9" * 400 + "}",
            "[" * 70 + "0" + "]" * 70,
        ]:
            with self.subTest(raw=raw), self.assertRaises(Invalid):
                decode(raw)

    def test_reject_unknown_context_and_bool_epoch(self):
        value = message()
        value["authority"] = "kernel"
        with self.assertRaises(Invalid):
            validate(value)
        task = self.task()
        task["body"]["task"]["epoch"] = True
        with self.assertRaises(Invalid):
            validate(task)

    def task(self):
        return message(
            "task_request",
            body={
                "task": {
                    "job_id": "job",
                    "attempt_id": "attempt",
                    "role_id": "role",
                    "assignment_id": "assignment",
                    "idempotency_key": "key",
                    "epoch": 0,
                    "deadline": stamp(60),
                    "input": {"x": "y"},
                }
            },
        )

    def test_deadline_and_leases(self):
        value = self.task()
        value["body"]["task"]["deadline"] = stamp(200)
        with self.assertRaises(Invalid):
            validate(value)
        value = message()
        value["body"]["lease_expires_at"] = stamp(-1)
        with self.assertRaises(Invalid):
            validate(value)
        value = message()
        value["expires_at"] = stamp(-1)
        with self.assertRaises(Invalid):
            validate(value)

    def test_large_values_and_capabilities(self):
        value = self.task()
        value["body"]["task"]["input"] = "a" * 32768
        with self.assertRaises(Invalid):
            validate(value)
        value = message()
        value["body"]["capabilities"] *= 2
        with self.assertRaises(Invalid):
            validate(value)

    def test_address_checks_actual_checksum_and_curve_key(self):
        self.assertEqual(address(IDENTITIES[0].address), IDENTITIES[0].address)
        for value in [
            "agent1" + "q" * 59,
            IDENTITIES[0].address[:-1] + "x",
            "user1whatever",
        ]:
            with self.assertRaises(Invalid):
                address(value)

    def test_peer_urls_never_accept_arbitrary_plaintext_or_credentials(self):
        self.assertEqual(
            peer_url("http://127.0.0.1:9999/submit"), "http://127.0.0.1:9999/submit"
        )
        self.assertEqual(
            peer_url("https://example.org/submit"), "https://example.org/submit"
        )
        for value in [
            "http://localhost:9999/submit",
            "https://token@example.org/submit",
            "https://example.org/submit?q=x",
            "file:///submit",
        ]:
            with self.assertRaises(Invalid):
                peer_url(value)


class StoreCase(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.path = str(Path(self.tmp.name) / "bridge.db")
        self.store = Store(self.path, IDENTITIES[0].address)

    def tearDown(self):
        self.store.close()
        self.tmp.cleanup()


class StoreTests(StoreCase):
    def test_restart_identity_and_queue(self):
        value = message()
        self.store.queue(IDENTITIES[1].address, value)
        self.store.close()
        self.store = Store(self.path, IDENTITIES[0].address)
        self.assertEqual(decode(self.store.next_outgoing()["payload"]), value)
        self.assertEqual(os.stat(self.path).st_mode & 0o777, 0o600)
        with self.assertRaises(Conflict):
            Store(self.path, IDENTITIES[1].address)

    def test_changed_retry_conflicts_and_dedup_is_persistent(self):
        value = message()
        self.store.queue(IDENTITIES[1].address, value)
        self.store.queue(IDENTITIES[1].address, value)
        changed = copy.deepcopy(value)
        changed["body"]["requirement"] = "changed"
        with self.assertRaises(Conflict):
            self.store.queue(IDENTITIES[1].address, changed)
        self.assertTrue(
            self.store.receive(IDENTITIES[1].address, value, int(time.time()) + 60)
        )
        self.assertFalse(self.store.receive(IDENTITIES[1].address, value, 1))
        with self.assertRaises(Conflict):
            self.store.receive(IDENTITIES[2].address, value, int(time.time()) + 60)
        self.assertEqual(len(self.store.inbox(0, 50)["items"]), 1)

    def test_expired_new_receipt_rejected(self):
        with self.assertRaises(Invalid):
            self.store.receive(IDENTITIES[1].address, message(), 1)
        self.assertEqual(self.store.inbox(0, 50)["next_cursor"], 0)

    def test_capacity_stops_accepting_without_eviction(self):
        self.store.max_records = 1
        self.store.queue(IDENTITIES[1].address, message())
        with self.assertRaises(Capacity):
            self.store.queue(IDENTITIES[1].address, message())
        self.assertIsNotNone(self.store.next_outgoing())

    def test_pagination_does_not_advance_past_omitted_items(self):
        for _ in range(3):
            self.store.receive(IDENTITIES[1].address, message(), int(time.time()) + 60)
        first = self.store.inbox(0, 2)
        second = self.store.inbox(first["next_cursor"], 2)
        self.assertEqual(
            [x["sequence"] for x in first["items"] + second["items"]], [1, 2, 3]
        )


class SpecialistTests(StoreCase):
    def process(self, value, sender=IDENTITIES[1].address):
        self.store.receive(sender, value, int(time.time()) + 60)
        Specialist(self.store).step()

    def contract(self):
        self.process(message())
        return self.store.db.execute("SELECT * FROM specialist_contracts").fetchone()

    def accept(self, contract, correlation="correlation-1"):
        value = message(
            "accept_offer",
            body={
                "offer_message_id": contract["offer_message_id"],
                "offer_id": contract["offer_id"],
            },
        )
        value["correlation_id"] = correlation
        self.process(value)

    def test_wrong_correlation_cannot_accept_or_release_contract(self):
        contract = self.contract()
        self.accept(contract, "wrong-correlation")
        self.assertEqual(
            self.store.db.execute("SELECT state FROM specialist_contracts").fetchone()[
                0
            ],
            "offered",
        )
        self.accept(contract)
        release = message("release_contract", body={"reason": "retire"})
        release["correlation_id"] = "wrong-correlation"
        self.process(release)
        self.assertEqual(
            self.store.db.execute("SELECT state FROM specialist_contracts").fetchone()[
                0
            ],
            "active",
        )

    def test_wrong_sender_cannot_accept_contract(self):
        contract = self.contract()
        self.process(
            message(
                "accept_offer",
                body={
                    "offer_message_id": contract["offer_message_id"],
                    "offer_id": contract["offer_id"],
                },
            ),
            IDENTITIES[2].address,
        )
        self.assertEqual(
            self.store.db.execute("SELECT state FROM specialist_contracts").fetchone()[
                0
            ],
            "offered",
        )

    def test_expired_lease_can_be_released_but_not_reactivated(self):
        contract = self.contract()
        self.accept(contract)
        self.store.db.execute("UPDATE specialist_contracts SET lease=?", (stamp(-1),))
        self.process(
            message("release_contract", body={"reason": "cleanup after lease expiry"})
        )
        self.assertEqual(
            self.store.db.execute("SELECT state FROM specialist_contracts").fetchone()[
                0
            ],
            "released",
        )
        self.process(message())
        self.assertEqual(
            self.store.db.execute("SELECT state FROM specialist_contracts").fetchone()[
                0
            ],
            "released",
        )

    def test_task_attempt_cannot_be_replaced_by_another_idempotency_key(self):
        self.accept(self.contract())
        task = WireTests().task()
        self.process(task)
        changed = copy.deepcopy(task)
        changed["message_id"] = str(uuid4())
        changed["body"]["task"]["idempotency_key"] = "new-key"
        changed["body"]["task"]["input"] = {"changed": True}
        self.process(changed)
        self.assertEqual(
            self.store.db.execute("SELECT COUNT(*) FROM specialist_tasks").fetchone()[
                0
            ],
            1,
        )
        result_rows = [
            decode(row[0])
            for row in self.store.db.execute("SELECT payload FROM outbox")
        ]
        self.assertEqual(
            len([row for row in result_rows if row["body"]["kind"] == "task_result"]), 1
        )

    def test_capacity_rolls_back_state_reply_and_cursor_together(self):
        self.store.max_records = 2
        self.store.receive(IDENTITIES[1].address, message(), int(time.time()) + 60)
        with self.assertRaises(Capacity):
            Specialist(self.store).step()
        self.assertEqual(
            self.store.db.execute(
                "SELECT COUNT(*) FROM specialist_contracts"
            ).fetchone()[0],
            0,
        )
        self.assertEqual(
            self.store.db.execute("SELECT COUNT(*) FROM outbox").fetchone()[0], 0
        )
        self.assertIsNone(
            self.store.db.execute(
                "SELECT value FROM metadata WHERE key='specialist_cursor'"
            ).fetchone()
        )


class HttpTests(unittest.IsolatedAsyncioTestCase):
    async def asyncSetUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.bridge = Bridge(
            seed=SEEDS[1],
            token=TOKEN,
            db=str(Path(self.tmp.name) / "bridge.db"),
            peers={IDENTITIES[0].address: "http://127.0.0.1:1/submit"},
            http_timeout=0.2,
        )
        self.runner = web.AppRunner(self.bridge.app, auto_decompress=False)
        await self.runner.setup()
        self.site = web.TCPSite(self.runner, "127.0.0.1", 0)
        await self.site.start()
        port = self.site._server.sockets[0].getsockname()[1]
        self.base = f"http://127.0.0.1:{port}"
        self.client = ClientSession()

    async def asyncTearDown(self):
        await self.client.close()
        await self.runner.cleanup()
        self.tmp.cleanup()

    async def post(self, path, data, auth=False):
        async with self.client.post(
            self.base + path,
            json=data,
            headers={"Authorization": "Bearer " + TOKEN} if auth else {},
        ) as response:
            return response.status, await response.json()

    async def test_private_auth_queue_semantics_and_conflict(self):
        value = {
            "schema_version": 1,
            "recipient": IDENTITIES[0].address,
            "message": message(),
        }
        self.assertEqual((await self.post("/v1/messages", value))[0], 401)
        status, receipt = await self.post("/v1/messages", value, True)
        self.assertEqual(status, 202)
        self.assertEqual(receipt["state"], "queued")
        self.assertEqual((await self.post("/v1/messages", value, True))[0], 202)
        value["message"]["body"]["requirement"] = "changed"
        self.assertEqual((await self.post("/v1/messages", value, True))[0], 409)

    async def test_signed_receipt_tamper_and_sender_target_checks(self):
        value = message()
        signed = envelope(value)
        self.assertEqual((await self.post("/submit", signed))[0], 200)
        self.assertEqual((await self.post("/submit", signed))[0], 200)
        changed = copy.deepcopy(signed)
        changed["payload"] = base64.b64encode(b"{}").decode()
        self.assertEqual((await self.post("/submit", changed))[0], 400)
        self.assertEqual(
            (
                await self.post(
                    "/submit", envelope(message(), target=IDENTITIES[2].address)
                )
            )[0],
            400,
        )
        self.assertEqual(
            (await self.post("/submit", envelope(message(), sender=IDENTITIES[3])))[0],
            400,
        )
        self.assertEqual(len(self.bridge.store.inbox(0, 50)["items"]), 1)

    async def test_expired_and_unsigned_or_wrong_schema_rejected(self):
        for change in [
            {"signature": None},
            {"schema_digest": "model:unknown"},
            {"expires": 1},
            {"version": True},
        ]:
            signed = envelope(message())
            signed.update(change)
            self.assertEqual((await self.post("/submit", signed))[0], 400)
        self.assertEqual(len(self.bridge.store.inbox(0, 50)["items"]), 0)

    async def test_unknown_fields_duplicate_keys_and_oversize_rejected(self):
        signed = envelope(message())
        signed["pretend_authority"] = "kernel"
        self.assertEqual((await self.post("/submit", signed))[0], 400)
        for raw in ['{"version":1,"version":1}', '"' + "x" * 270000 + '"']:
            async with self.client.post(
                self.base + "/submit",
                data=raw,
                headers={"Content-Type": "application/json"},
            ) as response:
                self.assertEqual(response.status, 400)

    async def test_redirect_does_not_forward_a_signed_message(self):
        hits = []
        app = web.Application()

        async def redirect(request):
            raise web.HTTPTemporaryRedirect("/destination")

        async def destination(request):
            hits.append(True)
            return web.Response()

        app.router.add_post("/submit", redirect)
        app.router.add_post("/destination", destination)
        runner = web.AppRunner(app)
        await runner.setup()
        site = web.TCPSite(runner, "127.0.0.1", 0)
        await site.start()
        port = site._server.sockets[0].getsockname()[1]
        self.bridge.peers[IDENTITIES[0].address] = f"http://127.0.0.1:{port}/submit"
        self.bridge.store.queue(IDENTITIES[0].address, message())
        await self.bridge.send_one(self.client, self.bridge.store.next_outgoing())
        self.assertEqual(hits, [])
        await runner.cleanup()


def http(base, path, value=None):
    request = urllib.request.Request(
        base + path,
        data=json.dumps(value).encode() if value is not None else None,
        headers={
            "Authorization": "Bearer " + TOKEN,
            "Content-Type": "application/json",
        },
    )
    with urllib.request.urlopen(request, timeout=2) as response:
        return json.load(response)


def free_port():
    with socket.socket() as sock:
        sock.bind(("127.0.0.1", 0))
        return sock.getsockname()[1]


class SeparateProcessTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.processes = {}
        self.ports = [free_port() for _ in range(3)]
        self.bases = [f"http://127.0.0.1:{port}" for port in self.ports]
        self.peers = Path(self.tmp.name) / "peers.json"
        self.peers.write_text(
            json.dumps(
                {
                    identity.address: base + "/submit"
                    for identity, base in zip(IDENTITIES, self.bases)
                }
            )
        )
        for index in range(3):
            self.start(index)

    def start(self, index):
        root = str(Path(__file__).resolve().parents[1])
        env = {
            "PATH": os.environ.get("PATH", ""),
            "PYTHONPATH": root,
            "GENESIS_FETCH_SEED": SEEDS[index],
            "GENESIS_FETCH_BRIDGE_TOKEN": TOKEN,
        }
        command = [
            sys.executable,
            "-m",
            "genesis_fetch_bridge",
            "serve",
            "--db",
            str(Path(self.tmp.name) / f"{index}.db"),
            "--peers",
            str(self.peers),
            "--port",
            str(self.ports[index]),
        ]
        if index == 2:
            command.append("--specialist")
        process = subprocess.Popen(
            command,
            env=env,
            cwd=self.tmp.name,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.PIPE,
        )
        self.processes[index] = process
        deadline = time.monotonic() + 10
        while time.monotonic() < deadline:
            try:
                self.assertEqual(
                    http(self.bases[index], "/v1/identity")["address"],
                    IDENTITIES[index].address,
                )
                return
            except (OSError, urllib.error.URLError):
                if process.poll() is not None:
                    self.fail("child failed: " + process.stderr.read().decode())
                time.sleep(0.05)
        self.fail("child startup timed out")

    def stop(self, index):
        process = self.processes.pop(index)
        process.terminate()
        try:
            process.wait(timeout=5)
        except subprocess.TimeoutExpired:
            process.kill()
            process.wait(timeout=5)
        process.stderr.close()

    def tearDown(self):
        for index in list(self.processes):
            self.stop(index)
        self.tmp.cleanup()

    def send(self, index, value):
        return http(
            self.bases[index],
            "/v1/messages",
            {"schema_version": 1, "recipient": IDENTITIES[2].address, "message": value},
        )

    def wait_kind(self, index, kind, contract):
        deadline = time.monotonic() + 8
        while time.monotonic() < deadline:
            items = http(self.bases[index], "/v1/inbox")["items"]
            for item in items:
                if (
                    item["message"]["body"]["kind"] == kind
                    and item["message"]["contract_id"] == contract
                ):
                    self.assertEqual(item["sender"], IDENTITIES[2].address)
                    return item["message"]
            time.sleep(0.05)
        self.fail(f"did not receive {kind}")

    def recruit(self, index, org, contract):
        self.send(index, message(org=org, contract=contract))
        offer = self.wait_kind(index, "offer", contract)
        self.send(
            index,
            message(
                "accept_offer",
                org,
                contract,
                {
                    "offer_message_id": offer["message_id"],
                    "offer_id": offer["body"]["offer_id"],
                },
            ),
        )
        return self.wait_kind(index, "contract_accepted", contract)

    def task(self, org, contract):
        return message(
            "task_request",
            org,
            contract,
            {
                "task": {
                    "job_id": "job-1",
                    "attempt_id": "attempt-1",
                    "role_id": "role-1",
                    "assignment_id": "assignment-1",
                    "idempotency_key": "task-1",
                    "epoch": 7,
                    "deadline": stamp(60),
                    "input": {"fixture": "synthetic"},
                }
            },
        )

    def test_real_processes_contracts_restart_duplicate_delivery_and_release_isolation(
        self,
    ):
        self.recruit(0, "org-a", "contract-a")
        self.recruit(1, "org-b", "contract-b")
        self.send(
            0,
            message(
                "release_contract",
                "org-a",
                "contract-a",
                {"reason": "responsibility transferred"},
            ),
        )
        self.wait_kind(0, "contract_released", "contract-a")
        self.stop(2)
        task = self.task("org-b", "contract-b")
        self.send(1, task)  # Durable offline queue before peer restart.
        self.stop(1)
        self.start(1)
        self.start(2)
        result = self.wait_kind(1, "task_result", "contract-b")
        self.assertEqual(result["body"]["epoch"], 7)
        self.assertFalse(result["body"]["output"]["domain_verified"])
        self.assertTrue(result["body"]["output"]["synthetic"])
        self.send(1, task)
        # Direct native HTTP replay forces receiver dedup, not just sender outbox dedup.
        native = envelope(task, sender=IDENTITIES[1], target=IDENTITIES[2].address)
        self.assertTrue(http(self.bases[2], "/submit", native)["received"])
        self.send(0, self.task("org-a", "contract-a"))
        time.sleep(0.4)
        self.assertEqual(
            len(
                [
                    x
                    for x in http(self.bases[1], "/v1/inbox")["items"]
                    if x["message"]["body"]["kind"] == "task_result"
                ]
            ),
            1,
        )
        self.assertEqual(
            len(
                [
                    x
                    for x in http(self.bases[0], "/v1/inbox")["items"]
                    if x["message"]["body"]["kind"] == "task_result"
                ]
            ),
            0,
        )
        inbound = http(self.bases[2], "/v1/inbox")["items"]
        self.assertEqual(
            len(
                [x for x in inbound if x["message"]["message_id"] == task["message_id"]]
            ),
            1,
        )


if __name__ == "__main__":
    unittest.main()
