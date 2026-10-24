// Dependency-free Node VM checks of apps/stage/index.html's inline script, in the shape of the
// review probes on PR #20 (no browser, no provider calls). Run: node --test apps/stage/stage.test.mjs
import test from "node:test";
import assert from "node:assert/strict";
import fs from "node:fs";
import vm from "node:vm";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";

const html = fs.readFileSync(join(dirname(fileURLToPath(import.meta.url)), "index.html"), "utf8");
const source = html.slice(html.indexOf("<script>") + "<script>".length, html.lastIndexOf("</script>"));

function harness(fetchImpl) {
  const nodes = new Map();
  const el = () => ({
    textContent: "", innerHTML: "", value: "", children: [], handlers: {}, hidden: false, disabled: false, classes: new Set(),
    classList: { add(c) { this.classes?.add(c); }, remove() {}, toggle(c, on) { if (on) this.classes?.add(c); else this.classes?.delete(c); } },
    style: { setProperty(k, v) { this[k] = v; } }, setAttribute(k, v) { this["@" + k] = v; }, addEventListener(k, f) { this.handlers[k] = f; },
    appendChild(c) { this.children.push(c); }, replaceChildren(...c) { this.children = c; }, closest() { return null; },
  });
  const get = (id) => { if (!nodes.has(id)) nodes.set(id, el()); return nodes.get(id); };
  const pending = new Map(); let i = 0;
  vm.runInNewContext(source, {
    document: { getElementById: get, createElement: el, querySelectorAll: () => [], addEventListener() {} },
    fetch: fetchImpl, setTimeout: (f) => { f(); }, setInterval: (f) => { pending.set(++i, f); return i; }, clearInterval: (k) => pending.delete(k),
    Promise, Date, Math, Number, String, Object, Array, Set, Map, Error, encodeURIComponent, console,
  });
  for (const id of ["urlA", "urlB"]) get(id).value = "http://local-" + id;
  return { get, pending, submit: () => get("liveform").handlers.submit({ preventDefault() {} }) };
}
const settle = async () => { for (let k = 0; k < 6; k++) await new Promise((r) => setImmediate(r)); };
const ok = (body) => ({ ok: true, status: 200, json: async () => body });
const stateWith = (installations, org_id = "org_a") => ({ org_id, name: org_id, revision: 1, roles: {}, missions: {}, responsibilities: {}, capability_gaps: {}, installations });

test("the page starts neutral; the fixture replay is opt-in and labeled", () => {
  const h = harness(async () => { throw new Error("no network in replay"); });
  assert.equal(h.get("mode").textContent, "No source selected");
  assert.equal(h.get("headA").textContent, "Organization A");
  assert.equal(h.get("events").children.length, 0);
  h.get("playreplay").handlers.click();
  assert.equal(h.get("mode").textContent, "Historical fixture replay");
  assert.equal(h.get("headA").textContent, "Org A · Campus Events (fixture)");
  assert.ok(h.get("auditline").textContent.startsWith("fixture run:"));
  assert.equal(h.pending.size, 0, "no polling before going live");
});

test("an unreachable kernel never shows replay data under a live badge, and replay controls are inert", async () => {
  const h = harness(async () => { throw new Error("offline"); });
  h.submit(); await settle();
  assert.equal(h.get("mode").textContent, "Live kernel: unreachable");
  assert.equal(h.get("beat").textContent, "Not connected.");
  assert.ok(!h.get("auditline").textContent.includes("0 violations"));
  assert.equal(h.get("chat").children.length, 1, "only the live note, no scripted conversation");
  assert.equal(h.get("events").children.length, 0);
  h.get("next").handlers.click();
  assert.equal(h.get("beat").textContent, "Not connected.", "Next does nothing while live");
  assert.equal(h.get("next").disabled, true);
  h.get("backreplay").handlers.click();
  assert.equal(h.get("mode").textContent, "Historical fixture replay");
  assert.equal(h.pending.size, 0, "polling stops when returning to replay");
});

test("a rejected or unrelated B installation is never drawn as inheritance", async () => {
  const a = stateWith({ x: { status: "ACCEPTED", capability: { capability_id: "cap_a", version: "1.0" } } });
  const b = stateWith({ y: { status: "REJECTED", capability: { capability_id: "cap_a", version: "1.0" } }, z: { status: "ACCEPTED", capability: { capability_id: "unrelated", version: "1.0" } } }, "org_b");
  const h = harness(async (url) => ok(url.includes("/events/") ? [] : url.includes("local-urlA") ? a : b));
  h.submit(); await settle();
  assert.equal(h.get("mode").textContent, "Live kernel");
  assert.equal(h.get("headA").textContent, "org_a", "live headings use the kernel's own name");
  const rows = h.get("lineage").children.map((n) => n.innerHTML);
  assert.ok(rows.some((r) => r.includes("Org B · installation rejected")));
  assert.ok(rows.some((r) => r.includes("Org A has no accepted installation of this exact version")));
  assert.ok(!rows.some((r) => r.includes("Org B · adopted")));
  assert.equal(h.get("token").hidden, false);
  assert.equal(h.get("token").style["--pos"], "6%", "token stays on A's side");
});

test("inheritance is drawn only for an exact accepted version and digest A published", async () => {
  const cap = { capability_id: "cap_a", version: "1.0" };
  const digest = "d".repeat(64);
  const a = stateWith({ x: { status: "ACCEPTED", capability: cap, source_digest: digest } });
  const b = stateWith({ y: { status: "ACCEPTED", capability: cap, source_digest: digest } }, "org_b");
  const h = harness(async (url) => ok(url.includes("/capabilities/") ? { publisher: "org_a", capability_id: "cap_a", version: "1.0", source_digest: digest } : url.includes("/events/") ? [] : url.includes("local-urlA") ? a : b));
  h.submit(); await settle();
  assert.ok(h.get("lineage").children.some((n) => n.innerHTML.includes("Org B · adopted")));
  assert.equal(h.get("token").style["--pos"], "94%");
});

test("event polling advances its cursor instead of rereading the first page", async () => {
  const urls = [];
  let batch = [{ seq: 1, type: "role_created", revision: 1, timestamp: "t1", payload: { name: "R", provider: "internal" } }, { seq: 2, type: "role_activated", revision: 1, timestamp: "t2", payload: {} }];
  const h = harness(async (url) => { urls.push(url); if (url.includes("/events/")) { const out = batch; batch = []; return ok(out); } return ok(stateWith({})); });
  h.submit(); await settle();
  for (const f of h.pending.values()) f();
  await settle();
  const eventUrls = urls.filter((u) => u.includes("/events/") && u.includes("local-urlA"));
  assert.ok(eventUrls[0].includes("after=0"));
  assert.ok(eventUrls[1].includes("after=2"), `second poll must continue after seq 2, got ${eventUrls[1]}`);
  assert.equal(h.get("events").children.length, 2);
});

test("a stale poll response is ignored after returning to replay", async () => {
  let release;
  const h = harness(() => new Promise((r) => { release = () => r(ok(stateWith({}))); }));
  h.submit(); await settle();
  h.get("backreplay").handlers.click();
  release(); await settle();
  assert.equal(h.get("mode").textContent, "Historical fixture replay");
  assert.ok(h.get("auditline").textContent.startsWith("fixture run:"));
});
