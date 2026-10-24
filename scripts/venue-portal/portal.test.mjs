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
import { publicHandler } from "./serve.mjs";
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
    assert.equal(b.suite.cases[i].source.url, `http://127.0.0.1:7200/venues/${page.slug}.html`);
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
  assert.equal(files.length, r.pages.length + 1);
  for (const f of files) {
    const text = readFileSync(f, "utf8");
    for (const leak of ["minor_units", "snapshot_sha256", "\"expected\"", r.suite.suite_id]) {
      assert.ok(!text.includes(leak), `${f} leaks ${leak}`);
    }
  }
  const fragment = readFileSync(join(out, "mission-fragment.json"), "utf8");
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
