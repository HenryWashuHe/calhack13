// Independent #199 current-selection guards. Inert JSON and mocked fetch only.
// Node VM probes for the clarity pass: panel captions, per-scene focus, the live pending receipt state on the
// decision card, and the Explain overlay. No network. Run: node --test apps/stage/clarity.test.mjs
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
  const body = el();
  const intervals = new Map(); let id = 0;
  vm.runInNewContext(source, {
    document: { getElementById: get, createElement: el, querySelectorAll: () => get("dots").children, addEventListener(k, f) { docHandlers[k] = f; }, body, documentElement: {}, fullscreenElement: null },
    fetch: fetchImpl, setTimeout: (f) => { f(); }, setInterval: (f) => { intervals.set(++id, f); return id; }, clearInterval: (k) => intervals.delete(k),
    Promise, Date, Math, Number, String, Object, Array, Set, Map, Error, JSON, encodeURIComponent, console,
  });
  for (const k of ["urlA", "urlB"]) get(k).value = "http://local-" + k;
  get("tokA").value = "org_a-agent";
  return { get, body, key: (k) => docHandlers.keydown({ key: k, target: null, preventDefault() {} }),
    submit: () => get("liveform").handlers.submit({ preventDefault() {} }), tick: () => { for (const f of intervals.values()) f(); },
    judge: (text) => { get("judgetext").value = text; return get("judgeform").handlers.submit({ preventDefault() {} }); } };
}
const settle = async () => { for (let k = 0; k < 8; k++) await new Promise((r) => setImmediate(r)); };
const ok = (body, status = 200) => ({ ok: status < 300, status, json: async () => body });
const state = (org_id) => ({ org_id, name: org_id, revision: 1, roles: {}, missions: org_id === "org_a" ? { mis_1: { mission_id: "mis_1", goal: "Develop a method" } } : {}, responsibilities: {}, capability_gaps: {}, installations: {} });
const text = (n) => [n.textContent, ...n.children.map(text)].join(" ");
const REGIONS = ["orgA", "channel", "orgB", "trackEvents", "trackChat", "auditbar"];

const ev = (seq, type, payload) => ({ seq, revision: 1, timestamp: `2026-10-24T19:00:${String(seq).padStart(2, "0")}.000Z`, type, payload });
const proposal = (seq, id, ops = ["CreateRole"]) => ev(seq, "change_proposed", { change_id: id, operations: ops, rationale: "persistent ownership" });
const status = (seq, id, to) => ev(seq, "change_status_changed", { change_id: id, from: "PROPOSED", to, reasons: [] });
const decision = (seq, obs, verdict, id = null) => ev(seq, "planner_decision", { observation_id: obs, verdict, change_id: id, model: "inert-model", summary: obs, rationale: "recorded rationale" });
const chosen = (h) => h.get("ladder").children.filter((r) => r.classes.has("chosen")).map(text).join(" ");
const current = (h) => h.get("pipeline").children.filter((r) => r.classes.has("current")).map(text).join(" ");
function live(batches) {
  let round = 0;
  const h = harness(async (url, opts = {}) => {
    if (opts.method === "POST") return ok({ observation_id: "obs_owned", revision: 2 }, 201);
    return ok(url.includes("/events/") ? (url.includes("local-urlA") ? batches[round] || [] : []) : state(url.includes("local-urlA") ? "org_a" : "org_b"));
  });
  return { ...h, next: async () => { round++; h.tick(); await settle(); } };
}

test("selection control: matching report retains its own provisioning pipeline", async () => {
  const h = live([[], [proposal(1, "chg_owned"), status(2, "chg_owned", "PROVISIONING"), decision(3, "obs_owned", "change_proposed", "chg_owned")]]);
  h.submit(); await settle(); await h.judge("Improve the development method"); await settle(); await h.next();
  assert.match(text(h.get("decision")), /obs_owned/);
  assert.match(chosen(h), /Create a new role/);
  assert.match(current(h), /PROVISIONING/);
});

test("a different observation's decision cannot replace the mechanism beside the pinned report answer", async () => {
  const h = live([[], [proposal(1, "chg_owned"), status(2, "chg_owned", "PROVISIONING"), decision(3, "obs_owned", "change_proposed", "chg_owned")],
    [proposal(4, "chg_other", ["AttachCapability"]), status(5, "chg_other", "COMMITTED"), decision(6, "obs_other", "change_proposed", "chg_other")]]);
  h.submit(); await settle(); await h.judge("Improve the development method"); await settle(); await h.next(); await h.next();
  assert.match(text(h.get("decision")), /obs_owned/, "the card is still correctly pinned to our report");
  assert.match(chosen(h), /Create a new role/, "the ladder must describe that same report's decision");
  assert.match(current(h), /PROVISIONING/, "only the unrelated change committed");
  assert.doesNotMatch(h.get("pipelinewhy").textContent, /chg_other/);
});

for (const verdict of ["escalated", "rejected"]) {
  test(`a new ${verdict} decision without a change cannot inherit an older committed pipeline`, async () => {
    const h = live([[proposal(1, "chg_old"), status(2, "chg_old", "COMMITTED"), decision(3, "obs_old", "change_proposed", "chg_old")],
      [decision(4, "obs_new", verdict)]]);
    h.submit(); await settle(); await h.next();
    assert.match(text(h.get("decision")), /obs_new/);
    assert.equal(chosen(h), "");
    assert.doesNotMatch(current(h), /COMMITTED/, "no change is attached to the current decision");
    assert.doesNotMatch(h.get("pipelinewhy").textContent, /chg_old/);
  });
}
