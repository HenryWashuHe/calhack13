// Independent PR126 receipt/race probes. Mocked fetch only; no live POSTs or provider calls.
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

test("a planner answer observed before the POST acknowledgment remains acknowledged", async () => {
  let release, answered = false;
  const h = harness((url, opts = {}) => {
    if (opts.method === "POST") return new Promise((r) => { release = () => r(ok({ observation_id: "obs_fast", revision: 2 }, 201)); });
    return Promise.resolve(read(url, answered ? [answer("obs_fast")] : []));
  });
  h.submit(); await settle();
  const pending = h.judge("the site changed");
  answered = true; h.poll(); await settle();
  assert.equal(h.get("decision").hidden, false, "the decision was already consumed by the event cursor");
  release(); await pending; h.poll(); await settle();
  assert.match(h.get("judgestatus").textContent, /answered: change proposed/);
});

test("a transient read failure does not discard a confirmed POST receipt from the same connection", async () => {
  let release, failReads = false;
  const h = harness((url, opts = {}) => {
    if (opts.method === "POST") return new Promise((r) => { release = () => r(ok({ observation_id: "obs_confirmed", revision: 2 }, 201)); });
    return failReads ? Promise.reject(new Error("temporary GET failure")) : Promise.resolve(read(url));
  });
  h.submit(); await settle();
  const pending = h.judge("the site changed");
  failReads = true; h.poll(); await settle();
  assert.match(h.get("mode").textContent, /unreachable/);
  release(); await pending;
  failReads = false; h.poll(); await settle();
  assert.equal(h.get("mode").textContent, "Live kernel");
  assert.match(chatText(h), /Recorded as evidence obs_confirmed/);
  assert.notEqual(h.get("judgestatus").textContent, "Recording…");
});

test("Org B's decision cannot answer a report posted to Org A even if observation IDs coincide", async () => {
  let answered = false;
  const h = harness(async (url, opts = {}) => opts.method === "POST"
    ? ok({ observation_id: "obs_shared", revision: 2 }, 201)
    : read(url, [], answered ? [answer("obs_shared")] : []));
  h.submit(); await settle(); await h.judge("the site changed");
  answered = true; h.poll(); await settle();
  assert.doesNotMatch(h.get("judgestatus").textContent, /planner answered/);
});

test("a successful HTTP response without an observation identity is not presented as recorded evidence", async () => {
  const h = harness(async (url, opts = {}) => opts.method === "POST" ? ok({}, 201) : read(url));
  h.submit(); await settle(); await h.judge("the site changed");
  assert.doesNotMatch(chatText(h), /Recorded as evidence/);
  assert.doesNotMatch(h.get("judgestatus").textContent, /waiting for the planner/);
  assert.equal(h.get("judgetext").value, "the site changed", "unconfirmed input is not silently discarded");
});

test("the accepted report uses the frozen connection token and mission rather than edited form values", async () => {
  const posts = [];
  const h = harness(async (url, opts = {}) => {
    if (opts.method === "POST") { posts.push({ url, opts }); return ok({ observation_id: "obs_frozen", revision: 2 }, 201); }
    return read(url);
  });
  h.submit(); await settle();
  h.get("urlA").value = "http://foreign/api/v1"; h.get("tokA").value = "foreign-token";
  await h.judge("the site changed");
  assert.equal(posts.length, 1);
  assert.equal(posts[0].url, "http://local-urlA/missions/mis_1/observations");
  assert.equal(posts[0].opts.headers.Authorization, "Bearer org_a-agent");
  assert.match(chatText(h), /obs_frozen/);
});

test("a transport error is never automatically retried by event polling", async () => {
  let posts = 0;
  const h = harness(async (url, opts = {}) => {
    if (opts.method === "POST") { posts += 1; throw new Error("response lost after dispatch"); }
    return read(url);
  });
  h.submit(); await settle(); await h.judge("the site changed");
  h.poll(); await settle(); h.poll(); await settle();
  assert.equal(posts, 1);
  assert.doesNotMatch(chatText(h), /Recorded as evidence/);
});
