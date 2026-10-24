// Node VM probes for judge-triggered disruptions (replay only) and presenter mode. No network.
// Run: node --test apps/stage/presenter.test.mjs
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
      appendChild(c) { this.children.push(c); }, replaceChildren(...c) { this.children = c; }, closest() { return null; } };
    n.classList = { add(c) { n.classes.add(c); }, remove(c) { n.classes.delete(c); }, toggle(c, on) { if (on === undefined) on = !n.classes.has(c); if (on) n.classes.add(c); else n.classes.delete(c); return on; }, contains(c) { return n.classes.has(c); } };
    return n;
  };
  const get = (id) => { if (!nodes.has(id)) nodes.set(id, el()); return nodes.get(id); };
  const body = el();
  let fullscreenRequests = 0;
  vm.runInNewContext(source, {
    document: {
      getElementById: get, createElement: el, querySelectorAll: () => get("dots").children, addEventListener(k, f) { docHandlers[k] = f; },
      body, documentElement: { requestFullscreen() { fullscreenRequests += 1; return Promise.reject(new Error("denied by the viewer")); } },
      exitFullscreen() { return Promise.resolve(); }, fullscreenElement: null,
    },
    fetch: fetchImpl, setTimeout: (f) => { f(); }, setInterval: () => 1, clearInterval() {},
    Promise, Date, Math, Number, String, Object, Array, Set, Map, Error, encodeURIComponent, console,
  });
  for (const id of ["urlA", "urlB"]) get(id).value = "http://local-" + id;
  return { get, body, key: (k) => docHandlers.keydown({ key: k, target: null, preventDefault() {} }), fullscreen: () => fullscreenRequests,
    submit: () => get("liveform").handlers.submit({ preventDefault() {} }) };
}
const settle = async () => { for (let k = 0; k < 6; k++) await new Promise((r) => setImmediate(r)); };
const ok = (body) => ({ ok: true, status: 200, json: async () => body });
const state = (org_id) => ({ org_id, name: org_id, revision: 1, roles: {}, missions: {}, responsibilities: {}, capability_gaps: {}, installations: {} });
const eventText = (h) => h.get("events").children.map((n) => n.innerHTML).join("\n");
const chatText = (h) => h.get("chat").children.map((n) => n.textContent).join("\n");

test("disruptions are hidden and inert before a source is selected", () => {
  const h = harness(async () => { throw new Error("no network"); });
  assert.equal(h.get("disrupt").hidden, true);
  h.get("d-cater").handlers.click();
  assert.equal(h.get("events").children.length, 0);
  assert.equal(h.get("beat").textContent, "Organizations that build, verify, share and improve capabilities.");
});

test("a judge-triggered disruption in replay is appended, labeled as a transcript, persists across steps and resets with the replay", () => {
  const h = harness(async () => { throw new Error("no network"); });
  h.get("playreplay").handlers.click();
  assert.equal(h.get("disrupt").hidden, false);
  const before = h.get("events").children.length;
  h.get("d-cater").handlers.click();
  const after = h.get("events").children.length;
  assert.ok(after > before + 1, "the disruption adds the observation and the planner's decision");
  assert.match(eventText(h), /catering vendor cancelled/);
  assert.match(eventText(h), /anthropic\/claude-sonnet-5-5/);
  assert.match(chatText(h), /report the catering vendor cancelled/);
  assert.match(h.get("mode").textContent, /^Historical fixture replay/);
  assert.match(h.get("sub").textContent, /transcript/i);
  assert.doesNotMatch(h.get("sub").textContent, /\blive now\b/i);
  assert.match(h.get("auditline").textContent, /not audited/);
  assert.equal(h.get("d-cater").disabled, true, "each disruption is thrown once per replay");
  const owns = h.get("treeA").children.map((n) => n.innerHTML).join("\n");
  assert.match(owns, /replacement catering/i, "the existing coordinator role gains the responsibility; no new role is drawn");
  h.get("next").handlers.click();
  assert.match(eventText(h), /catering vendor cancelled/, "the disruption stays on the record when the storyboard advances");
  h.get("playreplay").handlers.click();
  assert.doesNotMatch(eventText(h), /catering vendor cancelled/);
  assert.equal(h.get("d-cater").disabled, false);
});

test("number keys throw disruptions only while replaying", () => {
  const h = harness(async () => { throw new Error("no network"); });
  h.key("2");
  assert.equal(h.get("events").children.length, 0);
  h.get("playreplay").handlers.click();
  h.key("2");
  assert.match(eventText(h), /no buses/i);
  assert.match(eventText(h), /no change/);
});

test("live mode hides the disruptions and renders a planner decision with its model", async () => {
  const decision = { seq: 1, revision: 1, timestamp: "t1", type: "planner_decision", payload: { verdict: "no_change", model: "anthropic/x", summary: "S", observation_id: "obs_1" } };
  const h = harness(async (url) => ok(url.includes("/events/") ? (url.includes("local-urlA") ? [decision] : []) : state(url.includes("local-urlA") ? "org_a" : "org_b")));
  h.get("playreplay").handlers.click();
  h.submit(); await settle();
  assert.equal(h.get("mode").textContent, "Live kernel");
  assert.equal(h.get("disrupt").hidden, true);
  h.get("d-cater").handlers.click();
  assert.doesNotMatch(eventText(h), /catering vendor cancelled/);
  assert.match(eventText(h), /Planner decision · no change/);
  assert.match(eventText(h), /anthropic\/x/);
});

test("presenter mode is a body class with a scene caption; Escape leaves it; a refused fullscreen is tolerated", async () => {
  const h = harness(async () => { throw new Error("no network"); });
  h.get("playreplay").handlers.click();
  h.get("present").handlers.click();
  await settle();
  assert.equal(h.body.classes.has("present"), true);
  assert.equal(h.fullscreen(), 1);
  assert.match(h.get("caption").textContent, /Scene 1 of 8/);
  assert.match(h.get("caption").textContent, /fixture replay/i);
  h.get("next").handlers.click();
  assert.match(h.get("caption").textContent, /Scene 2 of 8/);
  h.key("Escape");
  assert.equal(h.body.classes.has("present"), false);
  h.key("p");
  assert.equal(h.body.classes.has("present"), true);
  h.key("p");
  assert.equal(h.body.classes.has("present"), false);
});
