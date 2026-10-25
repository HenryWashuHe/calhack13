// Independent PR125 probes. Mocked HTTP values only; no provider or kernel calls.
import test from "node:test";
import assert from "node:assert/strict";
import fs from "node:fs";
import vm from "node:vm";

const html = fs.readFileSync(new URL("./index.html", import.meta.url), "utf8");
const source = html.slice(html.indexOf("<script>") + 8, html.lastIndexOf("</script>"));
function harness(pages) {
  const nodes = new Map(), intervals = new Map(); let timer = 0, round = 0;
  const el = () => {
    const n = { textContent: "", innerHTML: "", value: "", children: [], handlers: {}, hidden: false,
      disabled: false, classes: new Set(), style: { setProperty() {} },
      setAttribute(k, v) { this[k] = v; }, addEventListener(k, f) { this.handlers[k] = f; },
      appendChild(c) { this.children.push(c); }, replaceChildren(...c) { this.children = c; } };
    Object.defineProperty(n, "className", { get() { return [...n.classes].join(" "); }, set(v) { n.classes = new Set(v.split(/\s+/)); } });
    n.classList = { add(c) { n.classes.add(c); }, remove(c) { n.classes.delete(c); },
      toggle(c, on) { if (on) n.classes.add(c); else n.classes.delete(c); } };
    return n;
  };
  const get = (id) => { if (!nodes.has(id)) nodes.set(id, el()); return nodes.get(id); };
  vm.runInNewContext(source, {
    document: { getElementById: get, createElement: el, querySelectorAll: () => get("dots").children,
      addEventListener() {}, body: el(), documentElement: {} },
    fetch: async (url) => {
      const org = url.includes("local-a") ? "A" : "B";
      const body = url.includes("/events/") ? (pages[round]?.[org] || []) : {
        org_id: `org_${org}`, revision: round + 1, roles: {}, missions: {},
        responsibilities: {}, capability_gaps: {}, installations: {},
      };
      return { ok: true, status: 200, json: async () => body };
    },
    setTimeout: (f) => f(), setInterval: (f) => { intervals.set(++timer, f); return timer; },
    clearInterval: (id) => intervals.delete(id), console,
  });
  get("urlA").value = "http://local-a/api/v1";
  get("urlB").value = "http://local-b/api/v1";
  return { get, start() { get("liveform").handlers.submit({ preventDefault() {} }); },
    poll() { round += 1; for (const f of intervals.values()) f(); } };
}
const settle = async () => { for (let k = 0; k < 6; k++) await new Promise((r) => setImmediate(r)); };
const text = (n) => [n.textContent, ...n.children.map(text)].join(" ");
const at = (second) => `2026-10-24T15:31:${String(second).padStart(2, "0")}.000Z`;
const observation = (id, second) => ({ seq: 1, revision: 1, timestamp: at(second),
  type: "observation_recorded", payload: { observation_id: id, source: "fixture", summary: id } });
const decision = (id, second) => ({ seq: 2, revision: 1, timestamp: at(second),
  type: "planner_decision", payload: { observation_id: id, verdict: "no_change", model: "fixture-model", summary: id } });

test("decision timing uses its own organization's observation when IDs coincide", async () => {
  const h = harness([{ A: [observation("obs_shared", 0)], B: [observation("obs_shared", 3)] },
    { A: [decision("obs_shared", 7)] }]);
  h.start(); await settle(); h.poll(); await settle();
  const card = text(h.get("decision"));
  assert.match(card, /live kernel org_A/);
  assert.match(card, /decided 7\.0 s after the evidence/);
});

test("a foreign organization's observation cannot invent a missing local interval", async () => {
  const h = harness([{ B: [observation("obs_shared", 0)] }, { A: [decision("obs_shared", 7)] }]);
  h.start(); await settle(); h.poll(); await settle();
  const card = text(h.get("decision"));
  assert.match(card, /live kernel org_A/);
  assert.doesNotMatch(card, /after the evidence/);
});

test("one organization's own evidence continues to supply the supported interval", async () => {
  const h = harness([{ A: [observation("obs_A", 0), decision("obs_A", 7)], B: [observation("obs_B", 3)] }]);
  h.start(); await settle();
  assert.match(text(h.get("decision")), /decided 7\.0 s after the evidence/);
});

test("absent, malformed and reversed observation timestamps omit timing but retain the verdict", async () => {
  for (const timestamp of [undefined, "not-a-timestamp", at(8)]) {
    const obs = observation("obs_A", 0); obs.timestamp = timestamp;
    const h = harness([{ A: [obs, decision("obs_A", 7)] }]);
    h.start(); await settle();
    assert.equal(h.get("mode").textContent, "Live kernel");
    assert.match(text(h.get("decision")), /no change/);
    assert.doesNotMatch(text(h.get("decision")), /after the evidence/);
  }
});

test("decision and disruption hidden attributes retain explicit CSS precedence", () => {
  for (const cls of ["decision", "disrupt"]) {
    const rule = html.match(new RegExp(`\\.${cls}\\[hidden\\]\\s*\\{([^}]*)\\}`));
    assert.ok(rule);
    assert.match(rule[1], /display:\s*none/);
  }
});
