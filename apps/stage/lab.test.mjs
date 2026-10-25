// Node VM probes for the Lab beat: the recorded iteration-engine run (three seeds, job-shop dispatching
// rules) drawn from the JSON embedded in the page. Nothing is fetched; the page never claims it is live.
// Run: node --test apps/stage/lab.test.mjs
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

test("the embedded extract carries the three recorded seeds with the write-up's headline numbers", () => {
  assert.equal(LAB.of, 20);
  assert.deepEqual(LAB.seeds.map((s) => s.id), ["smoke-2", "smoke-3", "smoke-4"]);
  assert.deepEqual(LAB.seeds.map((s) => s.confirm), [19, 17, 18]);
  assert.deepEqual(LAB.seeds.map((s) => s.panel["random-search"][0]), [14, 17, 12]);
  assert.deepEqual(LAB.seeds.map((s) => s.panel.MWKR[0]), [15, 15, 15]);
  assert.deepEqual(LAB.seeds.map((s) => s.panel.SPT[0]), [5, 12, 10]);
  assert.deepEqual(LAB.seeds.map((s) => s.gens.filter((g) => g[2] !== null).length), [1, 5, 2], "one, five and two promotions");
  assert.deepEqual(LAB.seeds.map((s) => s.ledger.records), [60, 64, 61]);
  for (const s of LAB.seeds) { assert.match(s.ledger.head, /^[0-9a-f]{64}$/); assert.equal(s.ledger.ok, true); assert.equal(s.gens.length, 5); }
});

test("the Lab chapter sits between Inheritance and Audit on a nine-chapter rail", () => {
  const h = harness(async () => { throw new Error("no network"); });
  assert.equal(h.get("dots").children.length, 9);
  assert.deepEqual(h.get("dots").children.slice(6).map((b) => b.textContent), ["Inheritance", "Lab", "Audit"]);
});

test("the beat renders every seed: five generations, the incumbent and four candidates out of 20, the promoted candidate marked, the frontier slot filled", () => {
  const h = open();
  LAB.seeds.forEach((seed, k) => {
    pick(h, k);
    assert.equal(h.get("labseeds").children[k]["@aria-pressed"], "true");
    assert.equal(rows(h).length, 5, seed.id);
    seed.gens.forEach(([inc, cands, promoted], g) => {
      const row = rows(h)[g], cs = cells(row);
      assert.equal(cs.length, 6, "incumbent, four candidates, frontier");
      assert.equal(text(cs[0]).trim(), String(inc));
      cands.forEach((n, j) => {
        const c = cs[1 + j];
        assert.equal(text(c).trim(), String(n));
        assert.equal(has(c, "promoted"), j === promoted, `${seed.id} g${g + 1} candidate ${j}`);
        assert.equal(c.style["--v"], String(n / LAB.of), "the bar height is the count over the panel size");
      });
      assert.equal(text(cs[5]).trim(), String(promoted === null ? inc : cands[promoted]), "the frontier slot holds the survivor on that panel");
      assert.equal(has(cs[5], "moved"), promoted !== null, "the frontier moves only when a candidate was promoted");
    });
    assert.equal(rows(h).filter((r) => cells(r).some((c) => has(c, "promoted"))).length, seed.gens.filter((g) => g[2] !== null).length);
  });
  assert.match(section, /id="labhead">[^<]*out of 20/, "the header says the panel size once");
  assert.match(section, /id="labhead">[^<]*sealed panel/i);
});

test("the seed switch tells the truth: later generations helped in two seeds and not in the third", () => {
  const h = open();
  pick(h, 0);
  assert.match(h.get("labsummary").textContent, /smoke-2/);
  assert.match(h.get("labsummary").textContent, /generation 1/);
  assert.match(h.get("labsummary").textContent, /did not improve/);
  pick(h, 1);
  assert.match(h.get("labsummary").textContent, /every generation/);
  assert.match(h.get("labsummary").textContent, /frontier came from generation 5/);
  pick(h, 2);
  assert.match(h.get("labsummary").textContent, /generations 1 and 5/);
  assert.match(h.get("labsummary").textContent, /frontier came from generation 5/);
});

test("the fresh-panel confirmation is one row with the four numbers and the words that make it honest", () => {
  const h = open();
  LAB.seeds.forEach((seed, k) => {
    pick(h, k);
    const row = text(h.get("labconfirm"));
    assert.match(row, /fresh panel, never used for selection/);
    for (const [name, [n]] of Object.entries(seed.panel)) assert.match(row, new RegExp(`${name}[^/]*\\b${n}/${LAB.of}\\b`), `${seed.id} ${name}`);
    assert.equal(h.get("labconfirm").children.filter((c) => has(c, "lab-cell")).length, 4, "frontier, random-search, MWKR, SPT");
    assert.ok(has(h.get("labconfirm").children.find((c) => has(c, "lab-cell")), "promoted"), "the frontier cell carries the same mark as the promoted candidate");
  });
});

test("the ledger shows the record count, the chain head's first 10 hex, a verified chip and the append-only line", () => {
  const h = open();
  LAB.seeds.forEach((seed, k) => {
    pick(h, k);
    const t = text(h.get("labledger"));
    assert.match(t, new RegExp(`\\b${seed.ledger.records} records\\b`));
    assert.ok(t.includes(seed.ledger.head.slice(0, 10)), "the head's first ten hex digits");
    assert.ok(!t.includes(seed.ledger.head.slice(0, 12)), "and no more than that");
    assert.match(t, /\bverified\b/);
    assert.match(t, /append-only/i);
    assert.match(t, /chain-hashed/i);
    assert.match(t, /changed or reordered record/i);
  });
  assert.match(section, /<p class="lab-thesis">The model proposes, the engine decides, the controls stay in the room\.<\/p>/);
});

test("the caption says recorded and never says live, and it is labelled with the fixture token, not the accent", () => {
  const cap = html.match(/id="cap-lab"[^>]*>([^<]+)</);
  assert.ok(cap, "the lab has a .cap caption");
  assert.equal(cap[1].trim(), "Recorded run, 2026-10-25, claude-sonnet-5-5 proposing rules behind the iteration engine. Not live.");
  const h = open();
  for (let k = 0; k < LAB.seeds.length; k++) { pick(h, k); assert.doesNotMatch(rendered(h).replace(/Not live\./g, ""), /\blive\b/i, "the only 'live' in the beat is the denial"); }
  assert.match(section, /id="labtag">recorded run</);
  const tagRule = css.match(/\.lab \.tag\s*\{([^}]*)\}/);
  assert.ok(tagRule, "the recorded tag has its own rule");
  assert.match(tagRule[1], /var\(--fixture\)/);
  assert.doesNotMatch(tagRule[1], /var\(--accent\)/);
  const chips = h.get("proof").children.map((c) => c.className + " " + c.textContent);
  assert.ok(chips.some((c) => /recorded/.test(c) && /not live/i.test(c)), "the proof chip for the beat says recorded, not live");
  assert.match(h.get("caption").textContent, /recorded lab run/i);
  assert.doesNotMatch(h.get("caption").textContent, /fixture replay/i, "the presenter caption does not call the recorded run a fixture");
});

test("keyboard navigation reaches the beat; the lab alone is lit there and folds everywhere else, in idle and in live mode", async () => {
  const h = harness(async (url) => ok(url.includes("/events/") ? [] : state(url.includes("local-urlA") ? "org_a" : "org_b")));
  assert.ok(has(h.get("lab"), "dim"), "folded before a source is selected");
  h.get("playreplay").handlers.click();
  assert.ok(has(h.get("lab"), "dim"), "folded on the thesis");
  for (let k = 0; k < 7; k++) h.key("ArrowRight");
  assert.match(h.get("caption").textContent, /Scene 8 of 9/);
  assert.ok(has(h.get("lab"), "focus") && !has(h.get("lab"), "dim"));
  for (const r of ["orgA", "channel", "orgB", "trackEvents", "trackChat", "auditbar", "mechanism"]) assert.ok(has(h.get(r), "dim"), `${r} steps back`);
  h.key("ArrowRight");
  assert.match(h.get("caption").textContent, /Scene 9 of 9/);
  assert.ok(has(h.get("auditbar"), "focus") && has(h.get("lab"), "dim"));
  h.key("ArrowLeft");
  assert.ok(has(h.get("lab"), "focus"));
  h.key("1");
  assert.ok(has(h.get("lab"), "dim"), "a thrown disruption folds the lab with the other panels outside the beat");
  h.submit(); await settle();
  assert.equal(h.get("mode").textContent, "Live kernel");
  assert.ok(has(h.get("lab"), "dim") && !has(h.get("lab"), "focus"), "a recorded run never sits open beside live kernels");
  assert.ok(["orgA", "channel", "orgB", "trackEvents", "trackChat", "auditbar"].every((r) => !has(h.get(r), "dim")), "live mode still hides none of the kernel's own state");
});

test("reduced motion is honoured in CSS: the frontier move and the row reveal are stilled; the folded lab names itself as recorded", () => {
  const reduced = css.match(/@media \(prefers-reduced-motion: reduce\) \{([\s\S]*?)\n\}/);
  assert.ok(reduced);
  assert.match(reduced[1], /\.lab-cell\.moved[^{]*\{[^}]*animation:\s*none/);
  assert.match(reduced[1], /\.lab-row[^{]*\{[^}]*(transition:\s*none|animation:\s*none)/);
  assert.match(css, /\.lab-cell\.moved\s*\{[^}]*animation:[^}]*promote/);
  assert.match(css, /\.lab\.dim[^{]*::after\s*\{[^}]*content:\s*"[^"]*recorded[^"]*"/i);
  assert.match(css, /body\.present \.lab/);
  assert.match(css, /\.lab\.dim [^{]*\{[^}]*display:\s*none/, "folded lab hides its body like the other panels");
});

test("the Explain overlay numbers the lab and the page keeps one numbered callout per region", () => {
  assert.match(html, /<section class="lab" id="lab" data-n="8"/);
  const li = html.match(/<li><b>8<\/b><span>([^<]+)<\/span><\/li>/);
  assert.ok(li);
  assert.match(li[1], /recorded/i);
  assert.match(li[1], /not live/i);
  const ns = [...html.matchAll(/data-n="(\d+)"/g)].map((m) => Number(m[1])).sort((a, b) => a - b);
  assert.deepEqual(ns, [1, 2, 3, 4, 5, 6, 7, 8]);
  assert.ok(/<li><b>9<\/b>/.test(html), "the headline keeps a number after the lab");
});
