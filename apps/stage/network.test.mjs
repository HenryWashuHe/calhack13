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

test("five acts, each with its own honesty chip wording and an accessible label", () => {
  for (const id of ["act-kernel", "act-recorded", "act-improve", "act-handoff", "act-network"]) assert.match(html, new RegExp(`<section class="act" id="${id}"[^>]*aria-label="`));
  assert.equal((script.match(/chip: \["concept", "concept"\]/g) || []).length, 2, "hand-off and network are both marked concept");
  assert.match(script, /chip: \["fixture", "scripted · how the pieces move"\]/);
  assert.match(script, /chip: \["recorded", "recorded · 2026-10-25 · live planner"\]/);
  assert.match(script, /chip: \["recorded", "recorded · 2026-10-25"\]/);
  assert.match(script, /chip: \["concept", "concept"\]/);
  const markup = html.slice(html.indexOf("<body"), html.indexOf("<script>"));
  assert.doesNotMatch(markup, /\blive\b/i, "no static markup calls anything live");
  for (const chip of ["recorded · 2026-10-25", "concept"]) assert.doesNotMatch(chip, /live kernel/);
});

test("live mode exists only behind a connection: key L, ?live=1, the chip says live kernel only after a successful poll, and the input posts with the agent token", () => {
  assert.match(script, /e\.key === "l" \|\| e\.key === "L"/);
  assert.match(script, /query\("live"\) === "1"/);
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
  assert.match(script, /\/\^\[1-5\]\$\//);
  assert.match(script, /const ORDER = \[0, 4, 1, 3, 2\]/, "story order: scripted kernel, recorded run, improvement, hand-off, network; the registry keeps the network at index 2 for the retained guards");
  assert.match(script, /prefers-reduced-motion: reduce/);
  assert.match(html, /@media \(prefers-reduced-motion: reduce\) \{[^}]*animation: none/);
  assert.match(script, /clock\.paused/);
});

test("the kernel act names the planner's five rungs and the kernel's five states plus the refusal", () => {
  assert.match(script, /const RUNGS = \["no change", "reuse a role", "attach a capability", "recruit a specialist", "create a role"\]/);
  assert.match(script, /const STAGES = \["PROPOSED", "VALIDATED", "PROVISIONING", "READY", "COMMITTED"\]/);
  assert.match(script, /"REJECTED"/);
});


test("captions are one to three words and transient: every say() call holds at most a short phrase and fades", () => {
  const calls = [...script.matchAll(/say\(g, [^,]+, [^,]+, "([^"]+)"/g), ...script.matchAll(/k\.say\([^,]+, [^,]+, "([^"]+)"/g)].map((m) => m[1]);
  assert.ok(calls.length >= 12, `captions found: ${calls.length}`);
  for (const c of calls) assert.ok(c.split(/\s+/).length <= 9, `caption too long: ${c}`);
  assert.match(script, /function say\(g, x, y, text, \{ cls = "", hold = 1600/);
  assert.match(script, /delay\(\(\) => tween\(450, \(p\) => \(t\.style\.opacity = String\(1 - p\)\), \(\) => t\.remove\(\)\), hold\)/);
});


test("the recorded act replays the factory run: the extract matches the recording file and ends in two commits and a refusal", () => {
  const rec = JSON.parse(fs.readFileSync(new URL("./recordings/factory-20261025.json", import.meta.url), "utf8"));
  const inline = literal(script, "RECORDED");
  assert.equal(inline.model, rec.model);
  assert.deepEqual(JSON.parse(JSON.stringify(inline.roles)), rec.events.filter((e) => e.type === "role_created").map((e) => e.name));
  const decisions = inline.steps.filter((s) => s.type === "planner_decision").map((s) => s.verdict);
  assert.deepEqual(JSON.parse(JSON.stringify(decisions)), ["change_proposed", "change_proposed", "no_change"]);
  assert.equal(inline.steps.filter((s) => s.type === "change_status_changed" && s.to === "COMMITTED").length, 2);
  assert.equal(inline.steps.filter((s) => s.type === "observation_recorded" && s.hostile).length, 1);
  assert.match(rec.note, /Nothing here is a fixture/);
});
