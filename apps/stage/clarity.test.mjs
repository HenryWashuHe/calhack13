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
const state = (org_id) => ({ org_id, name: org_id, revision: 1, roles: {}, missions: org_id === "org_a" ? { mis_1: { mission_id: "mis_1", goal: "Run the event" } } : {}, responsibilities: {}, capability_gaps: {}, installations: {} });
const text = (n) => [n.textContent, ...n.children.map(text)].join(" ");
const REGIONS = ["orgA", "channel", "orgB", "trackEvents", "trackChat", "auditbar"];

test("every panel carries a plain caption that says what it is", () => {
  for (const id of ["cap-orgA", "cap-channel", "cap-orgB", "cap-events", "cap-chat", "cap-audit"]) {
    const m = html.match(new RegExp(`id="${id}"[^>]*>([^<]{20,})<`));
    assert.ok(m, `${id} has a caption`);
  }
  assert.match(html, /id="cap-chat"[^>]*>[^<]*cannot commit/i, "the conversation caption states the token boundary");
  assert.match(html, /id="cap-orgB"[^>]*>[^<]*inherit/i);
});

test("each replay scene lights the panels doing the work and dims the rest; the thesis dims nothing; live mode dims nothing", async () => {
  const h = harness(async (url) => ok(url.includes("/events/") ? [] : state(url.includes("local-urlA") ? "org_a" : "org_b")));
  h.get("playreplay").handlers.click();
  assert.ok(REGIONS.every((r) => !h.get(r).classes.has("dim")), "the thesis shows the whole screen");
  h.get("next").handlers.click(); // Evidence: the conversation and the record
  assert.ok(h.get("trackChat").classes.has("focus"));
  assert.ok(h.get("trackEvents").classes.has("focus"));
  assert.ok(h.get("orgB").classes.has("dim"));
  h.get("dots").children[6].handlers.click(); // Inheritance: the channel and Org B
  assert.ok(h.get("channel").classes.has("focus"));
  assert.ok(h.get("orgB").classes.has("focus"));
  assert.ok(!h.get("orgB").classes.has("dim"));
  h.submit(); await settle();
  assert.equal(h.get("mode").textContent, "Live kernel");
  assert.ok(REGIONS.every((r) => !h.get(r).classes.has("dim") && !h.get(r).classes.has("focus")), "live mode never hides the kernel's own state");
  h.get("backreplay").handlers.click();
  assert.ok(h.get("channel").classes.has("focus"), "returning to the replay restores the scene focus");
});

test("after a live report the decision card shows elapsed time since receipt until the answer lands", async () => {
  let answered = false;
  const decision = { seq: 2, revision: 2, timestamp: "2026-10-24T19:00:09.400Z", type: "planner_decision", payload: { observation_id: "obs_live1", verdict: "change_proposed", model: "fixture-model", summary: "A recorded answer" } };
  const h = harness(async (url, opts = {}) => {
    if (opts.method === "POST") return ok({ observation_id: "obs_live1", revision: 2 }, 201);
    if (url.includes("/events/")) return ok(url.includes("local-urlA") && answered ? [decision] : []);
    return ok(state(url.includes("local-urlA") ? "org_a" : "org_b"));
  });
  h.submit(); await settle();
  await h.judge("the caterer cancelled"); await settle();
  assert.equal(h.get("decision").hidden, false);
  assert.match(text(h.get("decision")), /waiting for answer/i);
  assert.ok(h.get("decision").classes.has("thinking"));
  h.tick(); // one second of waiting
  assert.match(text(h.get("decision")), /\d+ s since receipt/);
  answered = true; h.tick(); await settle(); // the poll brings the answer
  assert.doesNotMatch(text(h.get("decision")), /waiting for answer/i);
  assert.match(text(h.get("decision")), /change proposed/);
  assert.ok(!h.get("decision").classes.has("thinking"));
});

test("the Explain overlay toggles with ? and closes with Escape, in replay and in idle", () => {
  const h = harness(async () => { throw new Error("no network"); });
  assert.equal(h.get("explain").hidden, true);
  h.key("?");
  assert.equal(h.get("explain").hidden, false);
  assert.ok(h.body.classes.has("explain"));
  h.key("Escape");
  assert.equal(h.get("explain").hidden, true);
  assert.ok(!h.body.classes.has("explain"));
  h.get("playreplay").handlers.click();
  h.key("?"); assert.equal(h.get("explain").hidden, false);
  h.key("?"); assert.equal(h.get("explain").hidden, true);
  assert.ok(html.match(/id="explain"[\s\S]*?<\/div>/)[0].split("<li").length - 1 >= 6, "the overlay names at least six regions");
});


test("pending reports ignore unrelated Org A answers and foreign answers with the same observation id", async () => {
  let phase = 0;
  const decision = (seq, observation_id, summary) => ({ seq, revision: 2, timestamp: "2026-10-24T19:00:09.400Z", type: "planner_decision",
    payload: { observation_id, verdict: "change_proposed", model: "fixture-model", summary } });
  const h = harness(async (url, opts = {}) => {
    if (opts.method === "POST") return ok({ observation_id: "obs_pending", revision: 2 }, 201);
    if (url.includes("/events/")) return ok(url.includes("local-urlA")
      ? phase === 0 ? [] : phase === 1 ? [decision(2, "obs_other", "Unrelated Org A answer")]
        : [decision(2, "obs_other", "Unrelated Org A answer"), decision(3, "obs_pending", "Matching Org A answer")]
      : phase > 0 ? [decision(2, "obs_pending", "Foreign matching-id answer")] : []);
    return ok(state(url.includes("local-urlA") ? "org_a" : "org_b"));
  });
  h.submit(); await settle();
  await h.judge("a development requirement changed"); await settle();
  assert.match(text(h.get("chat")), /Waiting for a matching planner decision/);
  assert.doesNotMatch(text(h.get("chat")), /planner is reading|planner is thinking/i);
  phase = 1; h.tick(); await settle();
  assert.match(text(h.get("decision")), /obs_pending.*since receipt/);
  assert.ok(h.get("decision").classes.has("thinking"), "unrelated answers do not end the pending state");
  assert.doesNotMatch(text(h.get("decision")), /Unrelated Org A answer|Foreign matching-id answer/);
  assert.ok(h.get("events").children.some((n) => n.innerHTML.includes("Unrelated Org A answer")), "unrelated decisions remain in the event log");
  assert.ok(h.get("events").children.some((n) => n.innerHTML.includes("Foreign matching-id answer")));
  phase = 2; h.tick(); await settle();
  assert.match(text(h.get("decision")), /Matching Org A answer/);
  assert.ok(!h.get("decision").classes.has("thinking"));
  assert.match(h.get("judgestatus").textContent, /The planner answered: change proposed/);
});

test("a new connection clears the pending report and can show unrelated decisions normally", async () => {
  let decisions = [];
  const h = harness(async (url, opts = {}) => {
    if (opts.method === "POST") return ok({ observation_id: "obs_pending", revision: 2 }, 201);
    if (url.includes("/events/")) return ok(url.includes("local-urlA") ? decisions : []);
    return ok(state(url.includes("local-urlA") ? "org_a" : "org_b"));
  });
  h.submit(); await settle();
  await h.judge("a development requirement changed"); await settle();
  assert.match(text(h.get("decision")), /obs_pending/);
  decisions = [{ seq: 2, revision: 2, timestamp: "2026-10-24T19:00:09.400Z", type: "planner_decision",
    payload: { observation_id: "obs_other", verdict: "no_change", model: "fixture-model", summary: "New connection answer" } }];
  h.submit(); await settle();
  assert.match(text(h.get("decision")), /New connection answer/);
  assert.doesNotMatch(text(h.get("decision")), /obs_pending/);
  assert.ok(!h.get("decision").classes.has("thinking"));
});
