// Static probes for the silent view (apps/stage/network.html): three acts, one honesty chip per act, the
// recorded numbers identical to the stage page's embedded extract, no running text, reduced motion
// honoured, keyboard control present. The page draws with SVG DOM APIs the VM harness lacks, so these
// probes read the source. Run: node --test apps/stage/network.test.mjs
import test from "node:test";
import assert from "node:assert/strict";
import fs from "node:fs";
import vm from "node:vm";

const html = fs.readFileSync(new URL("./network.html", import.meta.url), "utf8");
const stage = fs.readFileSync(new URL("./index.html", import.meta.url), "utf8");
const script = html.slice(html.indexOf("<script>") + "<script>".length, html.lastIndexOf("</script>"));
const literal = (source, name) => { const m = source.match(new RegExp(`const ${name} = (\\{[\\s\\S]*?\\});\\n`)); assert.ok(m, `${name} literal present`); return vm.runInNewContext("(" + m[1] + ")", {}); };

test("three acts, each with its own honesty chip wording and an accessible label", () => {
  for (const id of ["act-kernel", "act-improve", "act-network"]) assert.match(html, new RegExp(`<section class="act" id="${id}"[^>]*aria-label="`));
  assert.match(script, /chip: \["fixture", "scripted · fixture scenario"\]/);
  assert.match(script, /chip: \["recorded", "recorded · 2026-10-25"\]/);
  assert.match(script, /chip: \["concept", "concept"\]/);
  const markup = html.slice(html.indexOf("<body"), html.indexOf("<script>"));
  assert.doesNotMatch(markup, /\blive\b/i, "no static markup calls anything live");
  for (const chip of ["scripted · fixture scenario", "recorded · 2026-10-25", "concept"]) assert.doesNotMatch(chip, /live/);
});

test("live mode exists only behind a connection: key L, ?live=1, the chip says live kernel only after a successful poll, and the input posts with the agent token", () => {
  assert.match(script, /e\.key === "l" \|\| e\.key === "L"/);
  assert.match(script, /params\.get\("live"\) === "1"/);
  const pollBody = script.slice(script.indexOf("async function pollLive"), script.indexOf("async function startLive"));
  assert.match(pollBody, /"live kernel · "/, "the live chip is set inside the poll");
  assert.match(pollBody, /unreachable/, "an unreachable kernel is said so, never shown as live");
  assert.doesNotMatch(script.replace(pollBody, ""), /"live kernel · "/, "nothing else claims a live kernel");
  assert.match(script, /\/observations`, \{ method: "POST"/);
  assert.match(script, /source: "silent-stage"/);
  assert.match(script, /const rungFor = /);
  assert.match(script, /history is not replayed as motion/);
});

test("the recorded numbers are the stage page's own extract, not retyped", () => {
  const net = literal(script, "LAB");
  const main = literal(stage.slice(stage.indexOf("<script>")), "LAB");
  assert.equal(net.of, main.of);
  for (const seed of net.seeds) {
    const ref = main.seeds.find((s) => s.id === seed.id);
    assert.ok(ref, `${seed.id} exists on the stage page`);
    assert.equal(JSON.stringify(seed.gens), JSON.stringify(ref.gens), `${seed.id} generations match`); // the two literals come from different VM realms, so compare by value
    assert.equal(seed.confirm, ref.confirm, `${seed.id} confirmation matches`);
    for (const k of ["random-search", "MWKR", "SPT"]) assert.equal(seed.panel[k], ref.panel[k][0], `${seed.id} ${k} matches`);
    assert.equal(seed.head, ref.ledger.head.slice(0, 10), `${seed.id} ledger head prefix matches`);
  }
});

test("no running text: the markup carries no paragraphs and the visible words are labels only", () => {
  const body = html.slice(html.indexOf("<body"), html.indexOf("<script>"));
  assert.doesNotMatch(body, /<p[\s>]/);
  const words = body.replace(/<[^>]+>/g, " ").split(/\s+/).filter(Boolean);
  assert.ok(words.length < 40, `visible words outside SVG: ${words.length}`);
});

test("keyboard control, pausable clock and reduced motion are present", () => {
  assert.match(script, /e\.key === " "/);
  assert.match(script, /ArrowRight/);
  assert.match(script, /\/\^\[1-3\]\$\//);
  assert.match(script, /prefers-reduced-motion: reduce/);
  assert.match(html, /@media \(prefers-reduced-motion: reduce\) \{[^}]*animation: none/);
  assert.match(script, /clock\.paused/);
});

test("the kernel act names the planner's five rungs and the kernel's five states plus the refusal", () => {
  assert.match(script, /const RUNGS = \["no change", "reuse a role", "attach a capability", "recruit a specialist", "create a role"\]/);
  assert.match(script, /const STAGES = \["PROPOSED", "VALIDATED", "PROVISIONING", "READY", "COMMITTED"\]/);
  assert.match(script, /"REJECTED"/);
});
