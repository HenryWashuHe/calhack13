// Node VM probes for the story pass: cold open, chapter rail, proof chips, decision card, change
// spotlight, autoplay progress. No network. Run: node --test apps/stage/story.test.mjs
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
    Promise, Date, Math, Number, String, Object, Array, Set, Map, Error, encodeURIComponent, console,
  });
  for (const k of ["urlA", "urlB"]) get(k).value = "http://local-" + k;
  return { get, body, intervals, key: (k) => docHandlers.keydown({ key: k, target: null, preventDefault() {} }),
    submit: () => get("liveform").handlers.submit({ preventDefault() {} }), poll: () => { for (const f of intervals.values()) f(); } };
}
const settle = async () => { for (let k = 0; k < 6; k++) await new Promise((r) => setImmediate(r)); };
const ok = (body) => ({ ok: true, status: 200, json: async () => body });
const state = (org_id) => ({ org_id, name: org_id, revision: 1, roles: {}, missions: {}, responsibilities: {}, capability_gaps: {}, installations: {} });
const classes = (n) => [...n.classes].join(" ");
const text = (n) => [n.textContent, ...n.children.map(text)].join(" "); // the card is built from elements, as the DOM would read it
const card = (h) => text(h.get("decision"));

test("the replay opens on the thesis and a named chapter rail replaces the dots", () => {
  const h = harness(async () => { throw new Error("no network"); });
  assert.equal(h.get("dots").children.length, 8);
  assert.equal(h.get("dots").children[0].textContent, "Thesis");
  assert.equal(h.get("dots").children[7].textContent, "Audit");
  assert.ok(h.get("dots").children.every((b) => b.classes.has("dot")), "chapter buttons keep the dot contract");
  h.get("playreplay").handlers.click();
  assert.equal(h.get("beat").textContent, "An AI organization that changes shape and keeps what it learns.");
  assert.equal(h.get("dots").children[0]["@aria-current"], "step");
});

test("every scene says which path is the real kernel and which is a fixture stand-in", () => {
  const h = harness(async () => { throw new Error("no network"); });
  assert.equal(h.get("proof").children.length, 0, "nothing claimed before a source is selected");
  h.get("playreplay").handlers.click();
  h.get("dots").children[2].handlers.click();
  const chips = h.get("proof").children.map(classes);
  assert.ok(chips.some((c) => c.includes("real")), "the kernel's validation and commit are real");
  assert.ok(chips.some((c) => c.includes("fixture")), "the proposal in the fixture came from the scenario script");
  for (let i = 0; i < 8; i++) { h.get("dots").children[i].handlers.click(); assert.ok(h.get("proof").children.length >= 1, `scene ${i} carries proof chips`); }
});

test("a thrown disruption shows a decision card that clears with the replay", () => {
  const h = harness(async () => { throw new Error("no network"); });
  h.get("playreplay").handlers.click();
  assert.equal(h.get("decision").hidden, true);
  h.get("d-shuttle").handlers.click();
  assert.equal(h.get("decision").hidden, false);
  assert.match(card(h), /no change/i);
  assert.match(card(h), /anthropic\/claude-sonnet-5-5/);
  assert.match(card(h), /transcript/i);
  h.get("playreplay").handlers.click();
  assert.equal(h.get("decision").hidden, true);
});

test("a live planner decision becomes a decision card with the model and the time since its observation", async () => {
  const obs = { seq: 1, revision: 1, timestamp: "2026-10-24T15:31:44.000Z", type: "observation_recorded", payload: { observation_id: "obs_1", source: "asi-one", summary: "no buses" } };
  const dec = { seq: 2, revision: 1, timestamp: "2026-10-24T15:31:51.000Z", type: "planner_decision", payload: { observation_id: "obs_1", verdict: "no_change", model: "anthropic/x", summary: "transport is already covered" } };
  const h = harness(async (url) => ok(url.includes("/events/") ? (url.includes("local-urlA") ? [obs, dec] : []) : state(url.includes("local-urlA") ? "org_a" : "org_b")));
  h.submit(); await settle();
  assert.equal(h.get("mode").textContent, "Live kernel");
  assert.equal(h.get("decision").hidden, false);
  assert.match(card(h), /no change/);
  assert.match(card(h), /anthropic\/x/);
  assert.match(card(h), /7\.0 s/);
  assert.match(card(h), /live/i);
  h.get("backreplay").handlers.click();
  assert.equal(h.get("decision").hidden, true, "a live card never survives into the replay");
});

test("stepping spotlights the roles that changed and only those", () => {
  const h = harness(async () => { throw new Error("no network"); });
  h.get("playreplay").handlers.click();
  h.get("dots").children[4].handlers.click();
  h.get("next").handlers.click(); // scene 5: relocation lead retires, venue monitor arrives
  const cards = h.get("treeA").children.map((n) => ({ html: n.innerHTML, cls: classes(n) }));
  const reloc = cards.find((c) => c.html.includes("Relocation lead")), coord = cards.find((c) => c.html.includes("Mission coordinator"));
  assert.ok(reloc.cls.includes("changed"), "the retired role is spotlighted");
  assert.ok(!coord.cls.includes("changed"), "an untouched role is not");
  h.get("dots").children[5].handlers.click();
  assert.ok(!h.get("treeA").children.some((n) => classes(n).includes("changed")), "jumping to the same scene again spotlights nothing");
});

test("autoplay shows a progress bar while running and stops it on pause", () => {
  const h = harness(async () => { throw new Error("no network"); });
  h.get("playreplay").handlers.click();
  assert.ok(!h.get("progress").classes.has("running"));
  h.get("play").handlers.click();
  assert.ok(h.get("progress").classes.has("running"));
  assert.equal(h.intervals.size, 1);
  h.get("play").handlers.click();
  assert.ok(!h.get("progress").classes.has("running"));
  assert.equal(h.intervals.size, 0);
});
