// Node VM probes for the live judge input: one observation posted to Org A with the agent token,
// only in live mode, rate limited, never from the replay. No network. Run: node --test apps/stage/judge.test.mjs
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

function liveFetch(posts) {
  return async (url, opts = {}) => {
    if (opts.method === "POST") { posts.push({ url, opts }); return ok({ observation_id: "obs_live1", revision: 2 }, 201); }
    if (url.includes("/events/")) return ok([]);
    return ok(state(url.includes("local-urlA") ? "org_a" : "org_b"));
  };
}

test("the judge input is hidden and inert until the kernel is live", async () => {
  const posts = [];
  const h = harness(liveFetch(posts));
  assert.equal(h.get("judgeform").hidden, true);
  await h.judge("the caterer cancelled"); await settle();
  assert.equal(posts.length, 0, "nothing is posted from idle");
  h.get("playreplay").handlers.click();
  assert.equal(h.get("judgeform").hidden, true, "the replay never gets a live input");
  await h.judge("the caterer cancelled"); await settle();
  assert.equal(posts.length, 0, "nothing is posted from the replay");
});

test("in live mode one observation is posted to Org A's mission with the agent token and shown in the chat", async () => {
  const posts = [];
  const h = harness(liveFetch(posts));
  h.submit(); await settle();
  assert.equal(h.get("mode").textContent, "Live kernel");
  assert.equal(h.get("judgeform").hidden, false);
  await h.judge("  the caterer cancelled for Nov 14  "); await settle();
  assert.equal(posts.length, 1);
  assert.equal(posts[0].url, "http://local-urlA/missions/mis_1/observations");
  assert.equal(posts[0].opts.headers.Authorization, "Bearer org_a-agent");
  const body = JSON.parse(posts[0].opts.body);
  assert.equal(body.summary, "the caterer cancelled for Nov 14");
  assert.equal(body.source, "stage-judge");
  assert.match(body.evidence_ref, /^stage:\/\/judge\//);
  assert.match(chatText(h), /report the caterer cancelled for Nov 14/);
  assert.match(chatText(h), /obs_live1/);
  assert.match(h.get("judgestatus").textContent, /obs_live1/);
  assert.equal(h.get("judgetext").value, "", "the field clears after a successful post");
});

test("the planner's answer to the judge's own evidence is acknowledged in the status line", async () => {
  const posts = [];
  let answered = false;
  const h = harness(async (url, opts = {}) => {
    if (opts.method === "POST") { posts.push(url); return ok({ observation_id: "obs_live1" }, 201); }
    if (url.includes("/events/")) return ok(url.includes("local-urlA") && answered ? [
      { seq: 1, revision: 2, timestamp: "2026-10-24T19:00:00.000Z", type: "observation_recorded", payload: { observation_id: "obs_live1", source: "stage-judge", summary: "the caterer cancelled" } },
      { seq: 2, revision: 2, timestamp: "2026-10-24T19:00:09.400Z", type: "planner_decision", payload: { observation_id: "obs_live1", verdict: "change_proposed", model: "anthropic/x", summary: "S" } },
    ] : []);
    return ok(state(url.includes("local-urlA") ? "org_a" : "org_b"));
  });
  h.submit(); await settle();
  await h.judge("the caterer cancelled"); await settle();
  assert.equal(posts.length, 1);
  assert.match(h.get("judgestatus").textContent, /waiting for the planner/);
  answered = true; h.poll(); await settle(); // the next poll brings the observation and the decision
  assert.match(h.get("judgestatus").textContent, /answered: change proposed/);
  assert.match(h.get("decision").children.map((n) => n.textContent).join(" "), /9\.4 s after the evidence/);
  h.get("backreplay").handlers.click(); h.submit(); await settle(); h.poll(); await settle();
  assert.doesNotMatch(h.get("judgestatus").textContent, /answered/, "a new connection does not inherit the old judge observation");
});

test("empty, oversized and rapid repeats never reach the kernel", async () => {
  const posts = [];
  const h = harness(liveFetch(posts));
  h.submit(); await settle();
  await h.judge("   "); await settle();
  assert.equal(posts.length, 0);
  assert.match(h.get("judgestatus").textContent, /type/i);
  await h.judge("x".repeat(500)); await settle();
  assert.equal(posts.length, 1);
  assert.equal(JSON.parse(posts[0].opts.body).summary.length, 200, "summary is cut to 200 characters");
  await h.judge("another one right away"); await settle();
  assert.equal(posts.length, 1, "a second post within the cooldown is refused");
  assert.match(h.get("judgestatus").textContent, /wait/i);
});

test("a kernel refusal is shown, not hidden, and nothing is added to the chat", async () => {
  const h = harness(async (url, opts = {}) => {
    if (opts.method === "POST") return ok({ error: "forbidden", message: "agent token cannot do that" }, 403);
    if (url.includes("/events/")) return ok([]);
    return ok(state(url.includes("local-urlA") ? "org_a" : "org_b"));
  });
  h.submit(); await settle();
  await h.judge("the caterer cancelled"); await settle();
  assert.match(h.get("judgestatus").textContent, /403/);
  assert.doesNotMatch(chatText(h), /report the caterer/);
});

test("returning to the replay hides the input and a late POST response changes nothing there", async () => {
  let release;
  const h = harness(async (url, opts = {}) => {
    if (opts.method === "POST") return new Promise((r) => { release = () => r(ok({ observation_id: "obs_late" }, 201)); });
    if (url.includes("/events/")) return ok([]);
    return ok(state(url.includes("local-urlA") ? "org_a" : "org_b"));
  });
  h.submit(); await settle();
  const pending = h.judge("the caterer cancelled");
  h.get("backreplay").handlers.click();
  assert.equal(h.get("judgeform").hidden, true);
  release(); await pending; await settle();
  assert.doesNotMatch(chatText(h), /obs_late/);
  assert.equal(h.get("mode").textContent, "Historical fixture replay");
});
