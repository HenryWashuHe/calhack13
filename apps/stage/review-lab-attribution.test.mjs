// Independent #226 attribution guards. Inert page VM and static JSON only.
// No network, provider, evaluator or generated-method execution.
import test from "node:test";
import assert from "node:assert/strict";
import fs from "node:fs";
import vm from "node:vm";

const html = fs.readFileSync(new URL("./index.html", import.meta.url), "utf8");
const source = html.slice(html.indexOf("<script>") + "<script>".length, html.lastIndexOf("</script>"));
const css = html.slice(html.indexOf("<style>") + 7, html.indexOf("</style>"));

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
  vm.runInNewContext(source, {
    document: { getElementById: get, createElement: el, querySelectorAll: () => get("dots").children, addEventListener(k, f) { docHandlers[k] = f; }, body, documentElement: {}, fullscreenElement: null },
    fetch: fetchImpl, setTimeout: (f) => { f(); }, setInterval: () => 1, clearInterval() {},
    Promise, Date, Math, Number, String, Object, Array, Set, Map, Error, JSON, encodeURIComponent, console,
  });
  for (const k of ["urlA", "urlB"]) get(k).value = "http://local-" + k;
  return { get, body, key: (k) => docHandlers.keydown({ key: k, target: null, preventDefault() {} }), submit: () => get("liveform").handlers.submit({ preventDefault() {} }) };
}
const settle = async () => { for (let k = 0; k < 8; k++) await new Promise((r) => setImmediate(r)); };
const ok = (body) => ({ ok: true, status: 200, json: async () => body });
const state = (org_id) => ({ org_id, name: org_id, revision: 1, roles: {}, missions: {}, responsibilities: {}, capability_gaps: {}, installations: {} });
const text = (n) => [n.textContent, ...n.children.map(text)].join(" ");
const has = (n, c) => n.classes.has(c);
const LAB = JSON.parse(source.match(/const LAB = (\{.*\});/)[1]); // the embedded extract, parsed independently of the page's own code
const open = () => { const h = harness(async () => { throw new Error("no network"); }); h.get("playreplay").handlers.click(); h.get("dots").children[7].handlers.click(); return h; };
const rows = (h) => h.get("labgens").children.filter((n) => has(n, "lab-row")); // the grid also holds its header cells
const section = html.match(/<section class="lab"[\s\S]*?<\/section>/)[0]; // the static copy, which the harness DOM does not hold
const rendered = (h) => ["labhead", "labsummary", "labgens", "labconfirm", "labledger"].map((id) => text(h.get(id))).join(" ") + " " + section.replace(/<[^>]+>/g, " ");
const cells = (row) => row.children.filter((c) => has(c, "lab-cell"));
const pick = (h, k) => h.get("labseeds").children[k].handlers.click();


test("Lab report flags never render as a real verified proof or a page-performed ledger check", () => {
  const h = open();
  assert.ok(h.get("proof").children.every((c) => !has(c, "real")), "embedded author reports are not real-kernel proof chips");
  assert.match(text(h.get("proof")), /author.reported/i);
  assert.match(text(h.get("labledger")), /author.reported.*verified/i);
  assert.match(text(h.get("labledger")), /not checked by this page/i);
  assert.ok(h.get("labledger").children.every((c) => !has(c, "ok")), "the report boolean must not produce a green verified badge");
});

test("promotion indices report selection only, not causal improvement or a cross-organization result", () => {
  const h = open();
  LAB.seeds.forEach((_, k) => {
    pick(h, k);
    assert.doesNotMatch(text(h.get("labsummary")), /later generations (?:did not improve|improved) it/);
    assert.match(text(h.get("labsummary")), /selection panels/i);
  });
  assert.match(section, /one lab/i);
  assert.match(section, /not.*cross.organizational improvement/i);
});

// Digests independently read from the six retained JSON files. These bind links to
// immutable artifacts; they do not authenticate a provider or verify a ledger.
const evidence = {
  "sourceCommit": "5fcb126ff913433a1d7d615fdfd61e0450e53ec2",
  "seeds": [
    {
      "id": "smoke-2",
      "result_sha256": "8afbf2fb49419136920dbefa5d32f2b91c2f19a441f988a2381b09d491660b4f",
      "ledger_sha256": "156928e39736b236c575988b63530201cecef20d9679fa3b05804ae68343851b"
    },
    {
      "id": "smoke-3",
      "result_sha256": "4ed191732600fb1783b5ce96963ddf5edf553704e3d0d60be26e9251182ae5c7",
      "ledger_sha256": "bc627c838f1e9fd16a510e3736c01934aec9bd0b502ea78fe804c2dffc18ee1b"
    },
    {
      "id": "smoke-4",
      "result_sha256": "51cf66fb7a363bba5a13cb3aacec7b0a6b163ed844b313aae4a6695859ae4544",
      "ledger_sha256": "0f3c16494b54a35840925085cbf3e635f2a5dc5f86aa0b0d2cad7a6517ef670e"
    }
  ]
};
test("each displayed seed links its own exact recorded artifacts without fetching them", () => {
  const h = open();
  evidence.seeds.forEach((e, k) => {
    pick(h, k);
    for (const name of ["result", "ledger"]) {
      const url = `https://github.com/HenryWashuHe/genesis-history-experiment/blob/${evidence.sourceCommit}/scripts/iteration/results/jobshop-${e.id}/${name}.json`;
      const link = h.get("labledger").children.find((c) => c.href === url);
      assert.ok(link, `${e.id}: immutable ${name} link`);
      assert.equal(link.title, "SHA-256 " + e[name + "_sha256"]);
    }
  });
});

test("recorded data preserves the reviewed generation and confirmation counts", () => {
  assert.deepEqual(LAB.seeds.map(s => [s.id, s.confirm, s.frontierGen, s.ledger.records]), [
    ["smoke-2", 19, 1, 60], ["smoke-3", 17, 5, 64], ["smoke-4", 18, 5, 61],
  ]);
  assert.deepEqual(LAB.seeds.map(s => s.gens.map(g => g[2])), [[0,null,null,null,null], [3,0,0,1,1], [0,null,null,null,1]]);
});
