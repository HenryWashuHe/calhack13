// Run: node --test scripts/iteration/sponsors/sponsors.test.mjs  (no network: fetch is injected)
import test from "node:test";
import assert from "node:assert/strict";
import { toBulk, post } from "./elastic.mjs";
import { toOffer, toChatText, fromChatText, toObservation, PROTOCOL } from "./fetch-offer.mjs";
import { createState, run, exportBundle } from "../engine.mjs";
import { syntheticFamily, syntheticControls, syntheticMutator } from "../synthetic-family.mjs";

const criterion = { rule: "success-count-margin", min_margin: 1 };
function ledgerAndBundle() {
  const family = syntheticFamily({ id: "t", seed: "s", target: 20 });
  const state = createState({ lab: "lab_a", seed: "seed" });
  run(state, { family, mutator: syntheticMutator(), controls: syntheticControls(family), criterion, k: 3, heldout: 4, generations: 3 });
  return { state, bundle: exportBundle(state, family.id) };
}

test("the Elastic bulk body carries one idempotent document per verified record and refuses a tampered ledger", async () => {
  const { state } = ledgerAndBundle();
  const bulk = toBulk(state.ledger, { exported_at: "2026-10-25T00:00:00Z" });
  const lines = bulk.body.trim().split("\n");
  assert.equal(lines.length, 2 * state.ledger.records.length);
  assert.equal(JSON.parse(lines[0]).index._id, state.ledger.records[0].sha256);
  assert.equal(JSON.parse(lines[1]).ledger_head, state.ledger.head);
  const tampered = { ...state.ledger, records: state.ledger.records.map((r, i) => (i === 1 ? { ...r, reason: "edited" } : r)) };
  assert.throws(() => toBulk(tampered), /unverified/);
  const sent = [];
  const res = await post(bulk, { url: "https://es.example", apiKey: "k", fetchImpl: async (url, opts) => { sent.push({ url, auth: opts.headers.authorization }); return { ok: true, status: 200, json: async () => ({ errors: false, items: lines.filter((_, i) => i % 2).map(() => ({ index: {} })) }) }; } });
  assert.equal(sent[0].url, "https://es.example/_bulk");
  assert.equal(res.indexed, state.ledger.records.length);
  const skipped = await post(bulk, { url: "", apiKey: "" });
  assert.equal(skipped.sent, false);
});

test("an exported bundle becomes a chat-carriable offer, round-trips, and becomes the bridge's observation body", () => {
  const { bundle } = ledgerAndBundle();
  const offer = toOffer(bundle, { from: "agent1q...a", to: "agent1q...b" });
  assert.equal(offer.protocol, PROTOCOL);
  assert.equal(offer.package.sha256, bundle.package.sha256);
  assert.equal(offer.ledger_sha256, bundle.ledger.head);
  assert.ok(!("source" in offer.package), "the offer names the package; bytes travel in the sealed bundle");
  const text = toChatText(offer);
  assert.match(text.split("\n")[0], /Verify before adopting/);
  assert.deepEqual(fromChatText(text), offer);
  assert.equal(fromChatText("hello"), null);
  const obs = toObservation(offer);
  assert.deepEqual(Object.keys(obs).sort(), ["evidence_ref", "source", "summary"]);
  assert.match(obs.evidence_ref, /^ledger:/);
});
