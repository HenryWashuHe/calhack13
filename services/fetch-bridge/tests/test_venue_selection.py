"""Real deterministic selection; signed loopback transport is not cloud evidence."""

from __future__ import annotations

import copy
import hashlib
import json
import time
import unittest
from pathlib import Path

import test_bridge as transport
from genesis_fetch_bridge.specialist import Specialist
from genesis_fetch_bridge.store import Conflict
from genesis_fetch_bridge.venue_selection import select_venue
from genesis_fetch_bridge.wire import Invalid, canonical


def digest(value):
    return hashlib.sha256(canonical(value).encode()).hexdigest()


def bind(context):
    context["policy_sha256"] = digest(context["policy"])
    return {"schema_version": 1, "kind": "venue_replacement_v1", "context": context,
            "context_sha256": digest(context)}


def candidate(name="replacement", price=70_000, source="source-a"):
    return {"source_job_id": source, "invocation_proof_ref": "invocation:" + "a" * 64,
            "venue": {"venue_id": name, "capacity": 200,
                      "price": {"minor_units": price, "currency": "USD"},
                      "window": {"start": "2026-11-01T18:00:00Z", "end": "2026-11-01T22:00:00Z"},
                      "availability": "available", "step_free": True,
                      "evidence": {"source_url": "https://venues.example/frozen/a", "snapshot_sha256": "b" * 64,
                                   "locator": "#" + name}}}


def task_input(candidates=None):
    return bind({"recovery_id": "recovery-a", "policy": {
        "selected_venue_id": "failed", "attendees": 121,
        "budget": {"minor_units": 80_000, "currency": "USD"},
        "window": {"start": "2026-11-01T19:00:00Z", "end": "2026-11-01T21:00:00Z"},
        "step_free_required": True,
    }, "candidates": candidates if candidates is not None else [candidate()]})


class SelectorTests(unittest.TestCase):
    def test_shared_rust_python_frozen_vector(self):
        path = Path(__file__).resolve().parents[3] / "contracts/fixtures/fetch_venue_replacement_v1.json"
        vector = json.loads(path.read_text())
        self.assertEqual(select_venue(vector["input"]), vector["output"])

    def test_evaluates_each_constraint_and_excludes_failed_venue_before_price_ranking(self):
        cases = [candidate("failed", 0), candidate("z-choice", 50_000), candidate("a-choice", 50_000)]
        for name, field, value in [
            ("small", "capacity", 120), ("closed", "availability", "unavailable"),
            ("stairs", "step_free", False),
            ("expensive", "price", {"minor_units": 80_001, "currency": "USD"}),
            ("foreign", "price", {"minor_units": 1, "currency": "CAD"}),
            ("short", "window", {"start": "2026-11-01T20:00:00Z", "end": "2026-11-01T22:00:00Z"}),
        ]:
            item = candidate(name, 1)
            item["venue"][field] = value
            cases.append(item)
        output = select_venue(task_input(cases))
        self.assertEqual(output["selected"], {"source_job_id": "source-a", "venue_id": "a-choice"})
        self.assertEqual([row["venue_id"] for row in output["evaluations"]], [c["venue"]["venue_id"] for c in cases])
        self.assertTrue(all(output["evaluations"][0]["checks"].values()))
        self.assertFalse(output["evaluations"][0]["eligible"])
        for row, predicate in zip(output["evaluations"][3:], ["capacity", "availability", "step_free", "price", "price", "window"]):
            self.assertFalse(row["checks"][predicate])
            self.assertFalse(row["eligible"])
        for row, item in zip(output["evaluations"], cases):
            self.assertEqual(row["evidence"], item["venue"]["evidence"])

    def test_no_match_requires_complete_evaluations_and_zero_budget_is_valid(self):
        request = task_input([candidate("failed", 0), candidate("too-small", 0)])
        request["context"]["candidates"][1]["venue"]["capacity"] = 1
        output = select_venue(bind(request["context"]))
        self.assertIsNone(output["selected"])
        self.assertEqual(len(output["evaluations"]), 2)
        request = task_input([candidate("free", 0)])
        request["context"]["policy"]["budget"]["minor_units"] = 0
        self.assertEqual(select_venue(bind(request["context"]))["selected"]["venue_id"], "free")

    def test_offsets_and_nanosecond_boundaries_compare_instants_not_strings_or_microseconds(self):
        request = task_input()
        request["context"]["policy"]["window"] = {
            "start": "2026-11-01T19:00:00.000000002Z", "end": "2026-11-01T20:00:00.000000002Z"}
        window = request["context"]["candidates"][0]["venue"]["window"]
        window.update(start="2026-11-01t11:00:00.000000002-08:00", end="2026-11-01T12:00:00.000000002-08:00")
        self.assertTrue(select_venue(bind(request["context"]))["evaluations"][0]["checks"]["window"])
        window["start"] = "2026-11-01T19:00:00.000000003Z"
        self.assertFalse(select_venue(bind(request["context"]))["evaluations"][0]["checks"]["window"])
        window["start"] = "2026-11-01T19:00:00.000000001Z"
        window["end"] = "2026-11-01T20:00:00.000000001Z"
        self.assertFalse(select_venue(bind(request["context"]))["evaluations"][0]["checks"]["window"])

    def test_malformed_context_is_refused_even_with_recomputed_hashes(self):
        mutations = [
            lambda c: c.update(oracle={}), lambda c: c.update(recovery_id="bad/id"),
            lambda c: c["policy"].update(attendees=True), lambda c: c["policy"].update(attendees=0),
            lambda c: c["policy"].update(step_free_required=1),
            lambda c: c["policy"]["budget"].update(minor_units=1.5),
            lambda c: c["policy"]["window"].update(start="2026-11-01T19:00:60Z"),
            lambda c: c["policy"]["window"].update(start="2026-02-30T19:00:00Z"),
            lambda c: c["policy"]["window"].update(start="2026-11-01T19:00:00+24:00"),
            lambda c: c["candidates"][0]["venue"].update(capacity=2**32),
            lambda c: c["candidates"][0]["venue"].update(step_free=1),
            lambda c: c["candidates"][0]["venue"]["price"].update(minor_units=2**64),
            lambda c: c["candidates"][0]["venue"]["evidence"].update(snapshot_sha256="B" * 64),
            lambda c: c["candidates"][0]["venue"]["evidence"].update(source_url="https://user:pass@venues.example/a"),
            lambda c: c["candidates"][0]["venue"]["evidence"].update(source_url="https://venues.example/a#"),
            lambda c: c["candidates"][0].update(invocation_proof_ref="forged"),
            lambda c: c.update(candidates=[]),
            lambda c: c["candidates"].append(copy.deepcopy(c["candidates"][0])),
            lambda c: c.update(candidates=[candidate(str(n)) for n in range(17)]),
            lambda c: c.update(candidates=[candidate(str(n), source=f"source-{n}") for n in range(9)]),
        ]
        for index, mutate in enumerate(mutations):
            with self.subTest(index=index):
                context = task_input()["context"]
                mutate(context)
                with self.assertRaises(Invalid):
                    select_venue(bind(context))

    def test_hashes_and_repeated_source_binding_are_not_advisory(self):
        request = task_input()
        request["context_sha256"] = "0" * 64
        with self.assertRaises(Invalid):
            select_venue(request)
        request = task_input()
        request["context"]["policy_sha256"] = "0" * 64
        request["context_sha256"] = digest(request["context"])
        with self.assertRaises(Invalid):
            select_venue(request)
        for mutate in [
            lambda c: c.update(invocation_proof_ref="invocation:" + "c" * 64),
            lambda c: c["venue"]["evidence"].update(snapshot_sha256="c" * 64),
            lambda c: c["venue"]["evidence"].update(source_url="https://venues.example/other"),
        ]:
            context = task_input([candidate("a"), candidate("b")])["context"]
            mutate(context["candidates"][1])
            with self.assertRaises(Invalid):
                select_venue(bind(context))


class SelectionPersistenceTests(transport.StoreCase):
    def test_mode_is_persistent_and_legacy_contracts_remain_advisory(self):
        Specialist(self.store, "venue_selection")
        with self.assertRaises(Conflict):
            Specialist(self.store)
        self.store.db.execute("DELETE FROM metadata WHERE key='specialist_mode'")
        advisory = Specialist(self.store)
        value = transport.message()
        self.store.receive(transport.IDENTITIES[1].address, value, int(time.time()) + 60)
        advisory.step()
        self.store.db.execute("DELETE FROM metadata WHERE key='specialist_mode'")
        with self.assertRaises(Conflict):
            Specialist(self.store, "venue_selection")

    def test_selector_does_not_offer_capabilities_it_cannot_perform(self):
        specialist = Specialist(self.store, "venue_selection")
        self.store.receive(transport.IDENTITIES[1].address, transport.message(), int(time.time()) + 60)
        self.assertTrue(specialist.step())
        self.assertEqual(self.store.db.execute("SELECT COUNT(*) FROM specialist_contracts").fetchone()[0], 0)
        self.assertEqual(self.store.db.execute("SELECT COUNT(*) FROM outbox").fetchone()[0], 0)


class VenueSeparateProcessTests(transport.SeparateProcessTests):
    specialist_mode = "venue_selection"

    def recruit(self, index, org, contract):
        value = transport.message(org=org, contract=contract)
        value["body"]["capabilities"] = ["venue.select"]
        self.send(index, value)
        offer = self.wait_kind(index, "offer", contract)
        self.assertEqual(offer["body"]["terms"]["scope"], "venue_selection")
        self.assertFalse(offer["body"]["terms"]["domain_verified"])
        self.assertNotIn("synthetic", offer["body"]["terms"])
        self.send(index, transport.message("accept_offer", org, contract, {
            "offer_message_id": offer["message_id"], "offer_id": offer["body"]["offer_id"]}))
        return self.wait_kind(index, "contract_accepted", contract)

    def selection_task(self, org, contract, n=1, value=None):
        task = self.task(org, contract)
        task["body"]["task"].update(job_id=f"job-{n}", attempt_id=f"attempt-{n}", idempotency_key=f"task-{n}",
                                      input=task_input() if value is None else value)
        return task

    def result(self, index, task):
        deadline = time.monotonic() + 8
        while time.monotonic() < deadline:
            for item in transport.http(self.bases[index], "/v1/inbox")["items"]:
                body = item["message"]["body"]
                if body["kind"] == "task_result" and body["task_request_id"] == task["message_id"]:
                    self.assertEqual(item["sender"], transport.IDENTITIES[2].address)
                    return item["message"]
            time.sleep(0.05)
        self.fail("no native selection result")

    def test_real_processes_contracts_restart_duplicate_delivery_and_release_isolation(self):
        self.recruit(0, "org-a", "contract-a")
        self.recruit(1, "org-b", "contract-b")
        a = self.selection_task("org-a", "contract-a")
        self.send(0, a)
        output = self.result(0, a)["body"]["output"]
        self.assertEqual(output["selected"], {"source_job_id": "source-a", "venue_id": "replacement"})
        self.assertTrue(output["evaluations"][0]["eligible"])
        self.assertTrue(all(output["evaluations"][0]["checks"].values()))
        malformed = self.selection_task("org-a", "contract-a", 2, {"unsupported": True})
        self.send(0, malformed)
        refused = self.result(0, malformed)
        self.assertIsNone(refused["body"]["output"])
        self.stop(2)
        b = self.selection_task("org-b", "contract-b")
        self.send(1, b)
        self.stop(1)
        self.start(1)
        self.start(2)
        self.assertEqual(self.result(1, b)["body"]["output"], output)
        self.send(0, malformed)
        replay = transport.envelope(b, sender=transport.IDENTITIES[1], target=transport.IDENTITIES[2].address)
        self.assertTrue(transport.http(self.bases[2], "/submit", replay)["received"])
        self.send(0, transport.message("release_contract", "org-a", "contract-a", {"reason": "handoff"}))
        self.wait_kind(0, "contract_released", "contract-a")
        after_release = self.selection_task("org-a", "contract-a", 3)
        self.send(0, after_release)
        b2 = self.selection_task("org-b", "contract-b", 2)
        self.send(1, b2)
        self.assertEqual(self.result(1, b2)["body"]["output"], output)
        time.sleep(0.3)
        for index, task in [(0, malformed), (1, b)]:
            results = [i["message"] for i in transport.http(self.bases[index], "/v1/inbox")["items"]
                       if i["message"]["body"].get("kind") == "task_result"
                       and i["message"]["body"].get("task_request_id") == task["message_id"]]
            self.assertEqual(len(results), 1)
        self.assertEqual(self.result(0, malformed), refused)
        self.assertFalse(any(i["message"]["body"].get("task_request_id") == after_release["message_id"]
                             for i in transport.http(self.bases[0], "/v1/inbox")["items"]))


if __name__ == "__main__":
    unittest.main()
