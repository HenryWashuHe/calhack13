// Independent #199 unpinned cross-organization display guard. Inert JSON; fetch is mocked.
// No browser, network, provider or runtime execution. Author tests remain unchanged.
// Run: node --test apps/stage/review-unpinned-card.test.mjs
import test from "node:test";
import assert from "node:assert/strict";
import fs from "node:fs";
import vm from "node:vm";

const html = fs.readFileSync(new URL("./index.html", import.meta.url), "utf8");
const source = html.slice(html.indexOf("<script>") + "<script>".length, html.lastIndexOf("</script>"));

function harness(fetchImpl) {
  const nodes = new Map();
  const docHandlers = {};
  const el = () => {
    const n = { textContent: "", innerHTML: "", value: "", children: [], handlers: {}, hidden: false, disabled: false, classes: new Set(),
      style: { setProperty(k, v) { this[k] = v; } }, setAttribute(k, v) { this["@" + k] = v; }, addEventListener(k, f) { this.handlers[k] = f; },
      appendChild(c) { this.children.push(c); }, replaceChildren(...c) { this.children = c; }, closest() { return null; }, focus() {} };
    Object.defineProperty(n, "className", { get() { return [...n.classes].join(" "); }, set(v) { n.classes = new Set(String(v).split(/\s+/).filter(Boolean)); } });
    n.classList = { add(c) { n.classes.add(c); }, remove(c) { n.classes.delete(c); }, toggle(c, on) { if (on === undefined) on = !n.classes.has(c); if (on) n.classes.add(c); else n.classes.delete(c); return on; }, contains(c) { return n.classes.has(c); } };
    return n;
  };
  const get = (id) => { if (!nodes.has(id)) nodes.set(id, el()); return nodes.get(id); };
  const intervals = new Map(); let id = 0;
  vm.runInNewContext(source, {
    document: { getElementById: get, createElement: el, querySelectorAll: () => get("dots").children, addEventListener(k, f) { docHandlers[k] = f; }, body: el(), documentElement: {}, fullscreenElement: null },
    fetch: fetchImpl, setTimeout: (f) => { f(); }, setInterval: (f) => { intervals.set(++id, f); return id; }, clearInterval: (k) => intervals.delete(k),
    Promise, Date, Math, Number, String, Object, Array, Set, Map, Error, JSON, encodeURIComponent, console,
  });
  for (const k of ["urlA", "urlB"]) get(k).value = "http://local-" + k;
  return { get, key: (k) => docHandlers.keydown({ key: k, target: null, preventDefault() {} }), submit: () => get("liveform").handlers.submit({ preventDefault() {} }), poll: () => { for (const f of intervals.values()) f(); } };
}
const settle = async () => { for (let k = 0; k < 8; k++) await new Promise((r) => setImmediate(r)); };
const ok = (body) => ({ ok: true, status: 200, json: async () => body });
const state = (org_id) => ({ org_id, name: org_id, revision: 1, roles: {}, missions: {}, responsibilities: {}, capability_gaps: {}, installations: {} });
const text = (n) => [n.textContent, ...n.children.map(text)].join(" ");
const rungs = (h) => h.get("ladder").children;
const chosen = (h) => rungs(h).filter((r) => r.classes.has("chosen")).map((r) => text(r)).join(" | ");
const stages = (h) => h.get("pipeline").children;
const stageState = (h) => stages(h).map((s) => `${text(s).trim()}:${[...s.classes].filter((c) => c !== "stage").sort().join("+") || "idle"}`);
const ev = (seq, type, payload) => ({ seq, revision: 1, timestamp: `2026-10-24T19:00:${String(seq).padStart(2, "0")}.000Z`, type, payload });


function live(batchesA, batchesB = []) {
  let round = 0;
  const h = harness(async (url) => ok(url.includes("/events/")
    ? (url.includes("local-urlA") ? batchesA[round] || [] : batchesB[round] || [])
    : state(url.includes("local-urlA") ? "org_a" : "org_b")));
  return { ...h, next: async () => { round++; h.poll(); await settle(); } };
}
const proposal = (seq, id, ops = ["CreateRole"]) => ev(seq, "change_proposed", { change_id: id, operations: ops, rationale: "persistent ownership" });
const status = (seq, id, from, to, reasons = []) => ev(seq, "change_status_changed", { change_id: id, from, to, reasons });
const decision = (seq, verdict, id = null) => ev(seq, "planner_decision", {
  observation_id: "obs_" + seq, verdict, change_id: id, model: "inert-model",
  summary: verdict === "escalated" ? "Model unavailable" : "recorded assessment",
  rationale: "recorded rationale", kernel_errors: verdict === "rejected" ? ["scope_not_in_constitution"] : [],
});

test("unpinned control: the A card and mechanism describe the same A decision", async () => {
  const h = live([[proposal(1, "chg_a"), status(2, "chg_a", "PROPOSED", "PROVISIONING"), decision(3, "change_proposed", "chg_a")]]);
  h.submit(); await settle();
  assert.match(text(h.get("decision")), /live kernel org_a/);
  assert.match(chosen(h), /Create a new role/);
  assert.match(h.get("pipelinewhy").textContent, /chg_a/);
});

test("an unpinned B card cannot silently borrow A's selected mechanism", async () => {
  const h = live([[proposal(1, "chg_a"), status(2, "chg_a", "PROPOSED", "PROVISIONING"), decision(3, "change_proposed", "chg_a")], []],
    [[], [decision(1, "no_change")]]);
  h.submit(); await settle(); await h.next();
  const card = text(h.get("decision"));
  // A one-selection policy may keep the A card, or select B and clear/change
  // the mechanism. It must not silently present A's change beside B's card.
  if (/live kernel org_b/.test(card)) {
    assert.doesNotMatch(chosen(h) + " " + h.get("pipelinewhy").textContent, /Create a new role|chg_a/,
      "B's no-change card is displayed beside A's role-creation/provisioning mechanism");
  } else {
    assert.match(card, /live kernel org_a/);
    assert.match(chosen(h), /Create a new role/);
    assert.match(h.get("pipelinewhy").textContent, /chg_a/);
  }
});
