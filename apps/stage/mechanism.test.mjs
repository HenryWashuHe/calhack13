// Node VM probes for the "How it decided" strip: the planner's decision ladder and the kernel's change
// pipeline, drawn from recorded verdicts, operations and status events in replay and live. No network.
// Run: node --test apps/stage/mechanism.test.mjs
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

test("the ladder has the planner's five preferences in order; the thesis shows no decision and no pipeline progress", () => {
  const h = harness(async () => { throw new Error("no network"); });
  h.get("playreplay").handlers.click();
  assert.deepEqual(rungs(h).map((r) => text(r).trim()), ["No change", "Reuse an existing role", "Attach a capability", "Recruit a specialist", "Create a new role"]);
  assert.equal(chosen(h), "", "the thesis scene carries no decision");
  assert.ok(stages(h).length >= 5);
  assert.ok(stages(h).every((s) => !s.classes.has("done") && !s.classes.has("current") && !s.classes.has("rejected")));
  assert.match(h.get("ladderwhy").textContent, /no decision in this scene/i);
});

test("the Decision scene marks the fixture's proposal honestly: create a role, written by the scenario script, validated and provisioning", () => {
  const h = harness(async () => { throw new Error("no network"); });
  h.get("playreplay").handlers.click();
  h.get("dots").children[2].handlers.click();
  assert.match(chosen(h), /Create a new role/);
  assert.match(h.get("ladderwhy").textContent, /scenario script/i, "the fixture's proposal was not a model's");
  const st = stageState(h);
  assert.ok(st.some((s) => /PROPOSED:done/.test(s)) && st.some((s) => /VALIDATED:done/.test(s)) && st.some((s) => /PROVISIONING:current/.test(s)), st.join(", "));
  h.get("next").handlers.click();
  assert.ok(stageState(h).some((s) => /COMMITTED:(current|done)/.test(s)), "the New role scene reaches COMMITTED");
});

test("thrown disruptions drive the ladder and pipeline: no change, reuse a role, and a kernel rejection", () => {
  const h = harness(async () => { throw new Error("no network"); });
  h.get("playreplay").handlers.click();
  h.key("2");
  assert.match(chosen(h), /No change/);
  assert.match(h.get("ladderwhy").textContent, /transport/i);
  assert.match(h.get("ladderwhy").textContent, /claude-sonnet-5-5/);
  assert.ok(stages(h).every((s) => !s.classes.has("done") && !s.classes.has("current")), "no change means no change pipeline");
  h.key("1");
  assert.match(chosen(h), /Reuse an existing role/);
  assert.ok(stageState(h).some((s) => /COMMITTED:(current|done)/.test(s)));
  h.key("3");
  assert.match(chosen(h), /Create a new role/);
  assert.ok(stages(h).some((s) => s.classes.has("rejected")), "the hostile proposal stops at the kernel");
  assert.doesNotMatch(stageState(h).join(" "), /COMMITTED:(current|done)/);
});

test("live mode draws the ladder from the recorded operations and the pipeline from status events, in arrival order", async () => {
  let round = 0;
  const batches = [
    [ev(1, "change_proposed", { change_id: "chg_1", operations: ["AssignResponsibility"], rationale: "the coordinator already owns the plan" }), ev(2, "change_status_changed", { change_id: "chg_1", from: "PROPOSED", to: "VALIDATED", reasons: [] })],
    [ev(3, "change_status_changed", { change_id: "chg_1", from: "VALIDATED", to: "PROVISIONING", reasons: [] }), ev(4, "change_status_changed", { change_id: "chg_1", from: "PROVISIONING", to: "READY", reasons: [] }), ev(5, "change_status_changed", { change_id: "chg_1", from: "READY", to: "COMMITTED", reasons: [] }), ev(6, "planner_decision", { observation_id: "obs_1", verdict: "change_proposed", change_id: "chg_1", model: "anthropic/x", summary: "food for 120" })],
    [ev(7, "planner_decision", { observation_id: "obs_2", verdict: "no_change", change_id: null, model: "anthropic/x", summary: "already covered" })],
  ];
  const h = harness(async (url) => ok(url.includes("/events/") ? (url.includes("local-urlA") ? (batches[round] || []) : []) : state(url.includes("local-urlA") ? "org_a" : "org_b")));
  h.submit(); await settle();
  assert.equal(h.get("mode").textContent, "Live kernel");
  assert.ok(stageState(h).some((s) => /VALIDATED:current/.test(s)), stageState(h).join(", "));
  round = 1; h.poll(); await settle();
  assert.match(chosen(h), /Reuse an existing role/, "AssignResponsibility alone is reuse of an existing role");
  assert.match(h.get("ladderwhy").textContent, /anthropic\/x/);
  assert.ok(stageState(h).some((s) => /COMMITTED:(current|done)/.test(s)));
  round = 2; h.poll(); await settle();
  assert.match(chosen(h), /No change/);
  assert.ok(stages(h).every((s) => !s.classes.has("current")), "a no-change verdict leaves no change in flight");
  h.get("backreplay").handlers.click();
  assert.equal(chosen(h), "", "live state never leaks into the replay");
});

test("live operations map to the right rung: CreateRole wins over the lower rungs; a capability op is attach; a contract is recruit", async () => {
  const cases = [[["CreateRole", "AssignResponsibility", "RecruitSpecialist"], /Create a new role/], [["AttachCapability"], /Attach a capability/], [["RecruitSpecialist", "AssignResponsibility"], /Recruit a specialist/]];
  for (const [ops, want] of cases) {
    const batch = [ev(1, "change_proposed", { change_id: "chg_x", operations: ops, rationale: "r" }), ev(2, "planner_decision", { observation_id: "obs_x", verdict: "change_proposed", change_id: "chg_x", model: "m", summary: "s" })];
    const h = harness(async (url) => ok(url.includes("/events/") ? (url.includes("local-urlA") ? batch : []) : state(url.includes("local-urlA") ? "org_a" : "org_b")));
    h.submit(); await settle();
    assert.match(chosen(h), want, ops.join(","));
  }
});
