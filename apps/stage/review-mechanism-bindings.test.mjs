// Independent review guards for #199. All input is inert kernel-event JSON; fetch is mocked.
// No browser, network, provider or runtime execution. Author tests remain unchanged.
// Run: node --test apps/stage/review-mechanism-bindings.test.mjs
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

test("review control: a recorded A decision follows its own change and B cannot alter the mechanism", async () => {
  const h = live([[proposal(1, "chg_a"), status(2, "chg_a", "PROPOSED", "VALIDATED"),
    status(3, "chg_a", "VALIDATED", "PROVISIONING"), decision(4, "change_proposed", "chg_a")], []],
    [[], [proposal(1, "chg_a", ["AttachCapability"]), status(2, "chg_a", "READY", "COMMITTED"), decision(3, "no_change")]]);
  h.submit(); await settle();
  assert.match(chosen(h), /Create a new role/);
  assert.ok(stageState(h).includes("PROVISIONING:current"));
  await h.next();
  assert.match(chosen(h), /Create a new role/);
  assert.ok(stageState(h).includes("PROVISIONING:current"));
});

for (const verdict of ["escalated", "rejected"]) {
  test(`review: ${verdict} without a change ID is not a successful no-change choice`, async () => {
    // Both are ordinary PlannerVerdict variants; agent.rs records them with change_id=None.
    const h = live([[decision(1, verdict)]]);
    h.submit(); await settle();
    assert.doesNotMatch(chosen(h), /No change/, `the ${verdict} record provides no no_change decision`);
  });
}

test("review: provisioning failure retains FAILED rather than inventing a kernel refusal", async () => {
  const h = live([[proposal(1, "chg_a"), status(2, "chg_a", "PROPOSED", "VALIDATED"),
    status(3, "chg_a", "VALIDATED", "PROVISIONING"), decision(4, "change_proposed", "chg_a")],
    [status(5, "chg_a", "PROVISIONING", "FAILED", ["provider_unavailable"])]]);
  h.submit(); await settle(); await h.next();
  assert.ok(!stages(h).some((s) => text(s).trim() === "REJECTED" && s.classes.has("rejected")),
    "ChangeStatus::Failed is distinct from ChangeStatus::Rejected");
  assert.match(stageState(h).join(" ") + h.get("pipelinewhy").textContent, /failed/i);
});

test("review: an unrelated A change cannot become the displayed decision's successful pipeline", async () => {
  const h = live([[proposal(1, "chg_selected"), status(2, "chg_selected", "PROPOSED", "VALIDATED"),
    status(3, "chg_selected", "VALIDATED", "PROVISIONING"), decision(4, "change_proposed", "chg_selected")],
    [proposal(5, "chg_operator", ["AttachCapability"]), status(6, "chg_operator", "PROPOSED", "VALIDATED"),
      status(7, "chg_operator", "VALIDATED", "PROVISIONING"), status(8, "chg_operator", "PROVISIONING", "READY"),
      status(9, "chg_operator", "READY", "COMMITTED")]]);
  h.submit(); await settle(); await h.next();
  const misleadingPair = /Create a new role/.test(chosen(h)) && stageState(h).includes("COMMITTED:current");
  const explicitSeparateIdentity = /chg_selected/.test(h.get("ladderwhy").textContent)
    && /chg_operator/.test(h.get("pipelinewhy").textContent);
  assert.ok(!misleadingPair || explicitSeparateIdentity,
    "the selected role creation is still PROVISIONING; only the unrelated attachment committed");
});

test("review: pipeline cells do not inherit the organization panel's three-column layout class", () => {
  const h = harness(async () => { throw new Error("no network"); });
  h.get("playreplay").handlers.click();
  const sectionClass = html.match(/<section class="([^"]+)" aria-label="Two organizations"/)[1];
  assert.ok(stages(h).every((s) => !s.classes.has(sectionClass)),
    `pipeline cells also match .${sectionClass}'s minmax(220px, 300px) middle column`);
});
