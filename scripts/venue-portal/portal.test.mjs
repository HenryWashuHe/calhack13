// node --test scripts/venue-portal/
import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { existsSync, mkdtempSync, readdirSync, readFileSync, writeFileSync } from "node:fs";
import { createServer } from "node:http";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";
import { build, jsonSha256, presetSpec, sha256 } from "./lib.mjs";
import { writeVariant } from "./generate.mjs";
import { labHandler, publicHandler } from "./serve.mjs";
import { combine } from "./combine-fragments.mjs";
import { isCalendarDate } from "./lib.mjs";
import { referenceExtract } from "./reference-extract.mjs";

const HERE = dirname(fileURLToPath(import.meta.url));
const REPO = resolve(HERE, "../..");
const PRESETS = ["baseline", "capacity-cut", "layout-drift"];
const BASE = "https://portal.test.example";

function walk(dir) {
  return readdirSync(dir, { withFileTypes: true }).flatMap((e) => (e.isDirectory() ? walk(join(dir, e.name)) : [join(dir, e.name)]));
}

test("a seed fully determines page bytes, digests and suite", () => {
  for (const p of PRESETS) {
    const a = build(presetSpec(p, "s1"), BASE);
    const b = build(presetSpec(p, "s1"), BASE);
    assert.deepEqual(a.suite, b.suite, p);
    assert.deepEqual(a.pages.map((x) => x.html), b.pages.map((x) => x.html), p);
    const c = build(presetSpec(p, "s2"), BASE);
    assert.notDeepEqual(a.pages.map((x) => x.sha256), c.pages.map((x) => x.sha256), `${p}: seeds must differ`);
  }
});

test("digests cover exact page bytes and do not depend on the public base URL", () => {
  const a = build(presetSpec("baseline", "x"), BASE);
  const b = build(presetSpec("baseline", "x"), "http://127.0.0.1:7200/");
  for (const [i, page] of a.pages.entries()) {
    assert.equal(page.sha256, sha256(Buffer.from(page.html, "utf8")));
    assert.equal(page.sha256, b.pages[i].sha256, "re-pointing the base URL must not change snapshots");
    assert.equal(a.suite.cases[i].source.sha256, page.sha256);
    assert.equal(b.suite.cases[i].source.url, `http://127.0.0.1:7200/${a.suite.suite_id}/venues/${page.slug}.html`);
  }
});

test("every preset satisfies the verifier's coverage rule", () => {
  for (const p of PRESETS) {
    const { suite } = build(presetSpec(p, "cov"), BASE);
    const supported = suite.cases.filter((c) => c.expected.status === "supported");
    assert.ok(new Set(supported.map((c) => c.format_id)).size >= 2, p);
    assert.ok(new Set(supported.map((c) => c.source.sha256)).size >= 2, p);
    assert.ok(suite.cases.some((c) => c.expected.status === "unsupported"), p);
    assert.equal(new Set(suite.cases.map((c) => jsonSha256(c.input))).size, suite.cases.length, `${p}: inputs unique`);
  }
});

test("an independent reader of each page agrees with the oracle", () => {
  for (const p of PRESETS) {
    for (const seed of ["a", "b", "c", "d", "e"]) {
      const { pages, suite } = build(presetSpec(p, seed), BASE);
      for (const [i, page] of pages.entries()) {
        const got = referenceExtract(page.html, page.url, page.sha256);
        assert.deepEqual(got, suite.cases[i].expected, `${p}/${seed}/${page.slug}`);
      }
    }
  }
});

test("capacity-cut changes only the hall that fit, and it no longer fits", () => {
  const before = build(presetSpec("baseline", "k"), BASE).suite.cases[0].expected.venues;
  const after = build(presetSpec("capacity-cut", "k"), BASE).suite.cases[0].expected.venues;
  const attendees = presetSpec("baseline", "k").attendees;
  assert.ok(before[0].capacity >= attendees);
  assert.ok(after[0].capacity < attendees);
  assert.deepEqual(after.slice(1).map((v) => v.capacity), before.slice(1).map((v) => v.capacity));
});

test("layout-drift uses layouts absent from the baseline", () => {
  const base = new Set(build(presetSpec("baseline", "d"), BASE).pages.map((p) => p.layout));
  const drift = build(presetSpec("layout-drift", "d"), BASE).pages.map((p) => p.layout);
  assert.ok(drift.every((l) => !base.has(l)), drift.join(","));
});

test("invalid specs are refused with a path", () => {
  const spec = presetSpec("baseline", "v");
  spec.pages[1].layout = "table";
  assert.throws(() => build(spec, BASE), /two distinct supported layouts/);
  const dup = presetSpec("baseline", "v");
  dup.pages[1].venues[0].venue_id = dup.pages[0].venues[0].venue_id;
  assert.throws(() => build(dup, BASE), /venue_id/);
  assert.throws(() => build(presetSpec("baseline", "v"), "ftp://x"), /base-url/);
  assert.throws(() => build(presetSpec("baseline", "v"), "https://u:p@x"), /base-url/);
});

test("written output keeps truth out of public/", () => {
  const out = mkdtempSync(join(tmpdir(), "portal-"));
  const r = writeVariant({ spec: presetSpec("layout-drift", "w"), baseUrl: BASE, out });
  const files = walk(join(out, "public"));
  assert.equal(files.length, r.pages.length + 2, "pages + namespace index + root index");
  for (const f of files) {
    const text = readFileSync(f, "utf8");
    for (const leak of ["minor_units", "snapshot_sha256", "\"expected\"", "\"venues\":"]) {
      assert.ok(!text.includes(leak), `${f} leaks ${leak}`);
    }
  }
  const fragment = readFileSync(r.fragmentPath, "utf8");
  assert.deepEqual(JSON.parse(fragment).invocation_input, r.suite.cases[0].input);
  assert.ok(!fragment.includes("minor_units") && !fragment.includes("\"expected\""));
  assert.ok(existsSync(r.suitePath));
});

test("the public server serves exact bytes and nothing outside public/", async () => {
  const out = mkdtempSync(join(tmpdir(), "portal-"));
  const r = writeVariant({ spec: presetSpec("baseline", "srv"), baseUrl: BASE, out });
  const server = createServer(publicHandler(join(out, "public")));
  await new Promise((ok) => server.listen(0, "127.0.0.1", ok));
  const url = `http://127.0.0.1:${server.address().port}`;
  try {
    for (const p of r.pages) {
      const res = await fetch(`${url}/${p.path}`);
      assert.equal(res.status, 200);
      const body = Buffer.from(await res.arrayBuffer());
      assert.equal(sha256(body), p.sha256);
      assert.equal(res.headers.get("x-content-sha256"), p.sha256);
      assert.equal(res.headers.get("cache-control"), "no-store");
    }
    for (const bad of ["/../trusted/manifest.json", "/%2e%2e/trusted/manifest.json", "/..%2ftrusted%2fmanifest.json", `/../trusted/${r.suite.suite_id}.json`, "/venues/"]) {
      const res = await fetch(`${url}${bad}`);
      assert.equal(res.status, 404, bad);
    }
    assert.equal((await fetch(`${url}/index.html`, { method: "POST" })).status, 405);
  } finally {
    server.close();
  }
});

// Round trip through the real Rust verifier: a correct candidate is accepted,
// a one-field error and a changed snapshot are rejected.
const VERIFY = process.env.GENESIS_VERIFY_BIN ?? join(REPO, "target/debug/genesis-verify");

test("generated suites are accepted and enforced by genesis-verify", { skip: !existsSync(VERIFY) && `build it first: cargo build -p genesis-verification (looked for ${VERIFY})` }, () => {
  const artifact = {
    capability_id: "portal-check",
    capability_version: "0.0.1",
    source_sha256: sha256(Buffer.from("portal-check")),
    actor_id: "synthetic-actor",
    build_id: "synthetic-build",
    build_number: "0.0.1",
  };
  const toUtc = (s) => new Date(s).toISOString().replace(".000Z", "Z");
  for (const p of PRESETS) {
    const dir = mkdtempSync(join(tmpdir(), "verify-"));
    const { suite } = build(presetSpec(p, "verify"), BASE);
    const runs = suite.cases.map((c) => {
      const output = structuredClone(c.expected);
      if (output.status === "supported") {
        output.venues.reverse(); // order is not meaning
        for (const v of output.venues) v.window = { start: toUtc(v.window.start), end: toUtc(v.window.end) };
      }
      return {
        case_id: c.case_id,
        artifact,
        input_sha256: jsonSha256(c.input),
        source: c.source,
        run_id: `run-${c.case_id}`,
        dataset_id: `ds-${c.case_id}`,
        status: "succeeded",
        output,
      };
    });
    const run = (name, value) => {
      writeFileSync(join(dir, "suite.json"), JSON.stringify(suite));
      writeFileSync(join(dir, "artifact.json"), JSON.stringify(artifact));
      writeFileSync(join(dir, `${name}.json`), JSON.stringify(value));
      return spawnSync(VERIFY, ["--suite", join(dir, "suite.json"), "--artifact", join(dir, "artifact.json"), "--runs", join(dir, `${name}.json`)], { encoding: "utf8" });
    };
    const ok = run("runs", runs);
    assert.equal(ok.status, 0, `${p}: correct candidate rejected\n${ok.stdout}${ok.stderr}`);

    const wrong = structuredClone(runs);
    wrong.find((r) => r.output.status === "supported").output.venues[0].capacity += 10;
    assert.equal(run("wrong", wrong).status, 1, `${p}: wrong capacity accepted`);

    const drifted = structuredClone(runs);
    drifted[0].source = { ...drifted[0].source, sha256: sha256(Buffer.from("edited page")) };
    assert.equal(run("drifted", drifted).status, 1, `${p}: changed snapshot accepted`);
  }
});

// ---- PR #35 review regressions ----

test("A and B are served at the same time at distinct URLs with their own bytes", async () => {
  const out = mkdtempSync(join(tmpdir(), "portal-ab-"));
  const a = writeVariant({ spec: presetSpec("baseline", "org-a"), baseUrl: "http://placeholder", out, namespace: "org-a" });
  const bSpec = presetSpec("baseline", "org-b");
  bSpec.attendees = 150;
  const b = writeVariant({ spec: bSpec, baseUrl: "http://placeholder", out, namespace: "org-b" });
  const server = createServer(publicHandler(join(out, "public")));
  await new Promise((ok) => server.listen(0, "127.0.0.1", ok));
  const base = `http://127.0.0.1:${server.address().port}`;
  try {
    for (const org of [a, b]) {
      for (const [i, p] of org.pages.entries()) {
        const res = await fetch(`${base}/${p.path}`);
        assert.equal(res.status, 200, p.path);
        const html = Buffer.from(await res.arrayBuffer()).toString("utf8");
        const c = org.suite.cases[i];
        assert.equal(sha256(Buffer.from(html, "utf8")), c.source.sha256, `${p.path} served the other org's bytes`);
        assert.deepEqual(referenceExtract(html, c.source.url, c.source.sha256), c.expected, p.path);
      }
    }
    const aUrls = new Set(a.suite.cases.map((c) => c.source.url));
    assert.ok(b.suite.cases.every((c) => !aUrls.has(c.source.url)));
  } finally {
    server.close();
  }
  const merged = combine(JSON.parse(readFileSync(a.fragmentPath, "utf8")), JSON.parse(readFileSync(b.fragmentPath, "utf8")));
  assert.deepEqual(merged.allowed_source_urls, [...a.fragment.allowed_source_urls, ...b.fragment.allowed_source_urls]);
  assert.deepEqual(merged.examples, a.fragment.examples);
  assert.deepEqual(merged.invocation_input, b.fragment.invocation_input);
  assert.equal(merged.invocation_input.attendees, 150);
  assert.ok(b.suite.cases.some((c) => JSON.stringify(c.input) === JSON.stringify(merged.invocation_input)), "B's suite holds B's invocation input");
  assert.throws(() => combine(a.fragment, a.fragment), /share page URLs/);
});

test("an existing namespace is never overwritten without replace", () => {
  const out = mkdtempSync(join(tmpdir(), "portal-ns-"));
  const first = writeVariant({ spec: presetSpec("baseline", "n"), baseUrl: BASE, out, namespace: "run-1" });
  const before = readFileSync(join(out, "public", first.pages[0].path));
  assert.throws(() => writeVariant({ spec: presetSpec("capacity-cut", "n"), baseUrl: BASE, out, namespace: "run-1" }), /already exists/);
  assert.deepEqual(readFileSync(join(out, "public", first.pages[0].path)), before, "bound page bytes unchanged");
  writeVariant({ spec: presetSpec("capacity-cut", "n"), baseUrl: BASE, out, namespace: "run-2" });
  assert.deepEqual(readFileSync(join(out, "public", first.pages[0].path)), before);
  assert.ok(existsSync(first.suitePath), "earlier suite file is kept");
  const replaced = writeVariant({ spec: presetSpec("capacity-cut", "n"), baseUrl: BASE, out, namespace: "run-1", replace: true });
  assert.equal(replaced.namespace, "run-1");
  assert.throws(() => build(presetSpec("baseline", "n"), BASE, "../x"), /namespace/);
});

test("the lab writes each variant to a new namespace", async () => {
  const out = mkdtempSync(join(tmpdir(), "portal-lab-"));
  const lab = createServer(labHandler({ out, baseUrl: BASE }));
  await new Promise((ok) => lab.listen(0, "127.0.0.1", ok));
  const url = `http://127.0.0.1:${lab.address().port}`;
  const post = (form) => fetch(`${url}/generate`, { method: "POST", body: new URLSearchParams(form), redirect: "manual" });
  try {
    assert.equal((await post({ preset: "baseline", seed: "s" })).status, 303);
    const page = join(out, "public", "portal-baseline-s-v1", "venues", "campus-rooms.html");
    const before = readFileSync(page);
    assert.equal((await post({ preset: "capacity-cut", seed: "s" })).status, 303);
    assert.deepEqual(readFileSync(page), before, "baseline pages unchanged after a capacity cut");
    assert.ok(existsSync(join(out, "public", "portal-capacity-cut-s-v1", "venues", "campus-rooms.html")));
    assert.equal((await post({ preset: "baseline", seed: "s" })).status, 400, "repeat refuses to overwrite");
  } finally {
    lab.close();
  }
});

test("impossible calendar dates are refused; leap days are not", () => {
  for (const bad of ["2026-02-31", "2026-02-29", "2026-13-01", "2026-00-10", "2026-04-31"]) {
    assert.ok(!isCalendarDate(bad), bad);
    const spec = presetSpec("baseline", "cal");
    spec.date = bad;
    assert.throws(() => build(spec, BASE), /calendar date/, bad);
    const pageSpec = presetSpec("baseline", "cal");
    pageSpec.pages[1].date = bad;
    assert.throws(() => build(pageSpec, BASE), /pages\[1\]\.date/, bad);
  }
  for (const good of ["2028-02-29", "2000-02-29", "2026-12-31"]) {
    assert.ok(isCalendarDate(good), good);
    const spec = presetSpec("baseline", "cal");
    spec.date = good;
    assert.ok(build(spec, BASE).suite.cases[0].input.requested_window.start.startsWith(good));
  }
  assert.ok(!isCalendarDate("1900-02-29"));
});
