// Independent PR198 guards: inert DOM/VM and injected fetch only.
// No real POST, provider, browser or generated-source execution.
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
const state = (org_id) => ({ org_id, name: org_id, revision: 1, roles: {}, missions: org_id === "org_a" ? { mis_1: { mission_id: "mis_1", goal: "Develop a reusable method" } } : {}, responsibilities: {}, capability_gaps: {}, installations: {} });
const text = (n) => [n.textContent, ...n.children.map(text)].join(" ");

test("a recorded observation alone never claims the planner is actively reading it", async () => {
  const h = harness(async (url, opts = {}) => {
    if (opts.method === "POST") return ok({ observation_id: "obs_pending", revision: 2 }, 201);
    return ok(url.includes("/events/") ? [] : state(url.includes("local-urlA") ? "org_a" : "org_b"));
  });
  h.submit(); await settle();
  await h.judge("the development requirement changed"); await settle();
  assert.equal(h.get("decision").hidden, false);
  assert.match(text(h.get("decision")), /obs_pending/);
  assert.doesNotMatch(text(h.get("decision")), /planner is reading|reading the evidence|planner is thinking/i,
    "the receipt records the observation but contains no planner activity signal");
});

test("an unrelated organization decision cannot replace the pending report's state", async () => {
  let unrelated = false;
  const answer = { seq: 2, revision: 2, timestamp: "2026-10-24T19:00:09Z", type: "planner_decision",
    payload: { observation_id: "obs_other", verdict: "change_proposed", model: "fixture-model", summary: "Foreign answer" } };
  const h = harness(async (url, opts = {}) => {
    if (opts.method === "POST") return ok({ observation_id: "obs_pending", revision: 2 }, 201);
    if (url.includes("/events/")) return ok(unrelated && url.includes("local-urlB") ? [answer] : []);
    return ok(state(url.includes("local-urlA") ? "org_a" : "org_b"));
  });
  h.submit(); await settle();
  await h.judge("the development requirement changed"); await settle();
  unrelated = true; h.tick(); await settle();
  assert.match(h.get("judgestatus").textContent, /waiting for the planner/);
  assert.match(text(h.get("decision")), /obs_pending/,
    "an unanswered report keeps its pending card until its own answer arrives");
  assert.doesNotMatch(text(h.get("decision")), /Foreign answer/);
});

test("opening Explain must not apply overlay-only layout or pointer rules to the body", () => {
  const h = harness(async () => { throw new Error("no network"); });
  h.key("?");
  assert.equal(h.get("explain").hidden, false);
  const css = html.slice(html.indexOf("<style>") + 7, html.indexOf("</style>"))
    .replace(/\/\*[\s\S]*?\*\//g, "");
  const matching = [...css.matchAll(/([^{}]+)\{([^{}]*)\}/g)]
    .filter((m) => m[1].split(",").some((s) => {
      const selector = s.trim();
      return selector.startsWith(".") && /^\.[a-z-]+$/.test(selector) && h.body.classes.has(selector.slice(1));
    }))
    .map((m) => m[2]).join(" ");
  assert.doesNotMatch(matching, /position:\s*fixed|pointer-events:\s*none/,
    "the body's annotation class must not match the fixed overlay CSS rule");
});
