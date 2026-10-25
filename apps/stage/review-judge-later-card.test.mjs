// Independent PR126 acknowledgment/card identity probes. Mocked fetch only; no live calls.
import test from "node:test";
import assert from "node:assert/strict";
import fs from "node:fs";
import vm from "node:vm";

const html = fs.readFileSync(new URL("./index.html", import.meta.url), "utf8");
const source = html.slice(html.indexOf("<script>") + "<script>".length, html.lastIndexOf("</script>"));

function harness(fetchImpl) {
  const nodes = new Map();
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
    document: { getElementById: get, createElement: el, querySelectorAll: () => get("dots").children, addEventListener() {}, body: el(), documentElement: {}, fullscreenElement: null },
    fetch: fetchImpl, setTimeout: (f) => { f(); }, setInterval: (f) => { intervals.set(++id, f); return id; }, clearInterval: (k) => intervals.delete(k),
    Promise, Date, Math, Number, String, Object, Array, Set, Map, Error, JSON, encodeURIComponent, console,
  });
  for (const k of ["urlA", "urlB"]) get(k).value = "http://local-" + k;
  get("tokA").value = "org_a-agent";
  return { get, submit: () => get("liveform").handlers.submit({ preventDefault() {} }), poll: () => { for (const f of intervals.values()) f(); },
    judge: (text) => { get("judgetext").value = text; return get("judgeform").handlers.submit({ preventDefault() {} }); } };
}
const settle = async () => { for (let k = 0; k < 8; k++) await new Promise((r) => setImmediate(r)); };
const ok = (body, status = 200) => ({ ok: status < 300, status, json: async () => body });
const state = (org_id) => ({ org_id, name: org_id, revision: 1, roles: {}, missions: org_id === "org_a" ? { mis_1: { mission_id: "mis_1", goal: "Run the event" } } : {}, responsibilities: {}, capability_gaps: {}, installations: {} });
const chatText = (h) => h.get("chat").children.map((n) => n.textContent).join("\n");

const answer = (id) => ({ seq: 2, revision: 2, timestamp: "2026-10-24T19:00:09.400Z", type: "planner_decision",
  payload: { observation_id: id, verdict: "change_proposed", model: "fixture-model", summary: "A recorded answer" } });
const read = (url, eventsA = [], eventsB = []) => ok(url.includes("/events/")
  ? (url.includes("local-urlA") ? eventsA : eventsB)
  : state(url.includes("local-urlA") ? "org_a" : "org_b"));

const text = (node) => [node.textContent, ...node.children.map(text)].join(" ");

for (const receiptFirst of [true, false]) {
  test(`a later unrelated poll cannot invalidate the acknowledged answer link (receipt first: ${receiptFirst})`, async () => {
    let release, answered = false;
    const ours = answer("obs_judge");
    const unrelated = { ...answer("obs_other"), payload: { observation_id: "obs_other", verdict: "no_change", model: "fixture-model", summary: "Unrelated Org B answer" } };
    const h = harness((url, opts = {}) => {
      if (opts.method === "POST") return new Promise((r) => { release = () => r(ok({ observation_id: "obs_judge", revision: 2 }, 201)); });
      return Promise.resolve(read(url, answered ? [ours] : [], answered ? [unrelated] : []));
    });
    h.submit(); await settle();
    const pending = h.judge("the site changed");
    if (receiptFirst) { release(); await pending; }
    answered = true; h.poll(); await settle();
    if (!receiptFirst) { release(); await pending; }
    // A subsequent poll contains only a new unrelated B answer. The status
    // still links to the acknowledged A card unless explicitly withdrawn.
    unrelated.seq = 3;
    h.poll(); await settle();
    const status = h.get("judgestatus").textContent;
    assert.match(status, /answered: change proposed/, "the confirmed answer remains acknowledged");
    if (status.includes("See the decision card")) {
      assert.match(text(h.get("decision")), /live kernel org_a/, "a status linking the answer must not point at another organization's card");
      assert.match(text(h.get("decision")), /A recorded answer/);
    }
  });
}
