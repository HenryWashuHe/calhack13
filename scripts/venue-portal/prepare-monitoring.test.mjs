// node --test scripts/venue-portal/prepare-monitoring.test.mjs
// Optional real binary checks (no builds, native providers or tunnels):
// GENESIS_VERIFY_BIN=/path/genesis-verify GENESIS_SCENARIO_BIN=/path/genesis node --test scripts/venue-portal/prepare-monitoring.test.mjs
import assert from "node:assert/strict";
import { spawn, spawnSync } from "node:child_process";
import { existsSync, mkdirSync, mkdtempSync, readFileSync, readdirSync, renameSync, rmSync, symlinkSync, writeFileSync } from "node:fs";
import { createServer } from "node:http";
import { tmpdir } from "node:os";
import { dirname, join, relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { test } from "node:test";
import { jsonSha256, presetSpec, sha256, withHoldoutExamples } from "./lib.mjs";
import { writeVariant } from "./generate.mjs";
import { publicHandler } from "./serve.mjs";
import { referenceExtract } from "./reference-extract.mjs";
import { prepareMonitoring } from "./prepare-monitoring.mjs";

const HERE = dirname(fileURLToPath(import.meta.url));
const REPO = resolve(HERE, "../..");
const SCRIPT = join(HERE, "prepare-monitoring.mjs");
const BASE = "https://portal.test.example";
const read = (path) => JSON.parse(readFileSync(path, "utf8"));
const write = (path, value) => writeFileSync(path, `${JSON.stringify(value, null, 2)}\n`);
const walk = (dir) => readdirSync(dir, { withFileTypes: true }).flatMap((e) => e.isDirectory() ? walk(join(dir, e.name)) : [join(dir, e.name)]);
const snapshot = (dir) => Object.fromEntries(walk(dir).map((f) => [relative(dir, f), sha256(readFileSync(f))]));
function fixture(t, { holdout = true, base = BASE } = {}) {
  const dir = mkdtempSync(join(tmpdir(), "genesis-monitor-setup-"));
  t.after(() => rmSync(dir, { recursive: true, force: true }));
  const portal = join(dir, "portal");
  const aSpec = presetSpec("baseline", "independent-a");
  const bSpec = presetSpec("baseline", "same-event-b");
  Object.assign(bSpec.pages[0].venues[0], { capacity: 180, price_minor: 75000, currency: "USD", start: "18:00", end: "22:00", availability: "available", step_free: true });
  bSpec.attendees = 150;
  const updateSpec = structuredClone(bSpec);
  updateSpec.suite_id = "portal-update-b-v1";
  updateSpec.pages[0].venues[0].capacity = 100;
  const av = writeVariant({ spec: holdout ? withHoldoutExamples(aSpec, "independent-examples-a") : aSpec, baseUrl: base, out: portal, namespace: "org-a" });
  const bv = writeVariant({ spec: bSpec, baseUrl: base, out: portal, namespace: "org-b" });
  const uv = writeVariant({ spec: updateSpec, baseUrl: base, out: portal, namespace: "org-b-update" });
  const policy = {
    selected_venue_id: bSpec.pages[0].venues[0].venue_id,
    attendees: 150, budget: { minor_units: 80000, currency: "USD" },
    window: bv.suite.cases[0].input.requested_window, step_free_required: true,
  };
  // A minimal operator template is supported; fragments' explanatory `note`
  // and oracle data are never spread into the strict server ScenarioSpec.
  const template = {
    run_id: "monitor-setup-test", progression: "automatic",
    candidate: { capability_id: "venue-reader", capability_version: "1", actor_name: "venue-monitor-one", source_version: "0.1" },
    repair: { capability_id: "venue-reader", capability_version: "2", actor_name: "venue-monitor-two", source_version: "0.1" },
    allow_development_roles: false,
  };
  const policyPath = join(dir, "policy.json"), templatePath = join(dir, "template.json");
  write(policyPath, policy); write(templatePath, template);
  const options = { portal, a: "org-a", b: "org-b", update: "org-b-update", policy: policyPath, suiteId: "monitoring-b-frozen-v1", template: templatePath, out: join(dir, "setup") };
  return { dir, av, bv, uv, bSpec, updateSpec, policy, template, options,
    replaceB: () => writeVariant({ spec: bSpec, baseUrl: base, out: portal, namespace: "org-b", replace: true }),
    replaceUpdate: () => writeVariant({ spec: updateSpec, baseUrl: base, out: portal, namespace: "org-b-update", replace: true }),
    prepare: () => prepareMonitoring(options) };
}
function cliArgs(options) {
  return [SCRIPT, ...Object.entries(options).flatMap(([key, value]) => [`--${key === "suiteId" ? "suite-id" : key}`, value])];
}

// Independent extraction from bytes served by the actual public handler, not a
// copied oracle output, supplies every positive synthetic verifier receipt.
async function servedRuns(t, fixture, suite) {
  const server = createServer(publicHandler(join(fixture.options.portal, "public")));
  await new Promise((ok) => server.listen(0, "127.0.0.1", ok));
  t.after(() => { server.closeAllConnections(); server.close(); });
  const origin = `http://127.0.0.1:${server.address().port}`;
  const outputs = [];
  for (const c of suite.cases) {
    const response = await fetch(`${origin}${new URL(c.source.url).pathname}`);
    assert.equal(response.status, 200);
    const raw = Buffer.from(await response.arrayBuffer());
    assert.equal(sha256(raw), c.source.sha256);
    const output = referenceExtract(raw.toString("utf8"), c.source.url, sha256(raw));
    assert.deepEqual(output, c.expected);
    outputs.push(output);
  }
  for (const privatePath of ["/trusted/suite-b.json", "/setup.json", "/scenario.json"]) assert.equal((await fetch(origin + privatePath)).status, 404);
  return outputs;
}

test("actual generator creates an immutable four-case B setup and public-only scenario", async (t) => {
  const f = fixture(t);
  const before = snapshot(f.options.portal);
  const paths = f.prepare();
  const scenario = read(paths.scenarioPath), suite = read(paths.suitePath), setup = read(paths.setupPath);
  assert.deepEqual(snapshot(f.options.portal), before, "all public pages, original suites, manifests and fragments remain unchanged");
  assert.deepEqual(read(f.bv.suitePath).cases, f.bv.suite.cases);
  assert.equal(suite.cases.length, 4);
  assert.deepEqual(suite.cases.slice(0, 3), f.bv.suite.cases);
  assert.equal(suite.cases[3].case_id, "monitoring-update-campus-rooms");
  assert.deepEqual(scenario.examples, f.av.fragment.examples);
  assert.deepEqual(scenario.allowed_source_urls, [...f.av.fragment.allowed_source_urls, ...f.bv.fragment.allowed_source_urls, f.uv.suite.cases[0].source.url]);
  assert.deepEqual(scenario.invocation_input, f.bv.suite.cases[0].input);
  assert.deepEqual(scenario.venue_monitoring, { policy: f.policy, updated_invocation_input: f.uv.suite.cases[0].input });
  assert.equal(scenario.provider_access_acknowledged, false, "missing operator acknowledgment is never granted");
  assert.equal(scenario.allow_development_roles, false);
  assert.deepEqual(scenario.candidate, f.template.candidate);
  assert.deepEqual(scenario.repair, f.template.repair);
  assert.deepEqual(Object.keys(scenario).sort(), ["run_id", "progression", "suite_a_id", "suite_b_id", "candidate", "repair", "allowed_source_urls", "examples", "invocation_input", "allow_development_roles", "provider_access_acknowledged", "venue_monitoring"].sort());
  const encoded = JSON.stringify(scenario);
  for (const key of ['"expected"', '"snapshot_sha256"', '"format_id"', '"scope"', '"note"']) assert.ok(!encoded.includes(key), key);
  assert.equal(setup.suite_b.sha256, jsonSha256(suite));
  assert.equal(setup.scenario.sha256, jsonSha256(scenario));
  assert.equal(setup.policy_sha256, jsonSha256(f.policy));
  assert.match(setup.coverage, /frozen configured snapshot coverage/);
  assert.match(setup.generation_examples, /separate/);
  assert.equal(new Set(suite.cases.map((c) => jsonSha256(c.input))).size, 4);
  const outputs = await servedRuns(t, f, suite);
  assert.equal(outputs[0].venues[0].capacity, 180);
  assert.equal(outputs[3].venues[0].capacity, 100);
});

test("suite-page examples remain honestly labeled, and explicit template flags are preserved", (t) => {
  const f = fixture(t, { holdout: false });
  f.template.provider_access_acknowledged = true;
  f.template.allow_development_roles = true;
  f.template.progression = "manual";
  write(f.options.template, f.template);
  const paths = f.prepare(), scenario = read(paths.scenarioPath);
  assert.match(read(paths.setupPath).generation_examples, /reproducibility, not held-out/);
  assert.equal(scenario.provider_access_acknowledged, true);
  assert.equal(scenario.allow_development_roles, true);
  assert.equal(scenario.progression, "manual");
});

test("the selected case need not be the fragment's first case", (t) => {
  const f = fixture(t);
  // Move the feasible baseline page later; preserve the event and its facts.
  f.bSpec.pages.reverse(); f.updateSpec.pages.reverse();
  f.replaceB(); f.replaceUpdate();
  const paths = f.prepare();
  assert.equal(read(paths.scenarioPath).invocation_input.source_url, f.bv.suite.cases[0].source.url);
  assert.equal(read(paths.suitePath).cases[3].case_id, "monitoring-update-campus-rooms");
});

test("modified public bytes, suites, manifests and fragments fail without writes", (t) => {
  for (const kind of ["html", "suite", "manifest", "fragment"]) {
    const f = fixture(t);
    if (kind === "html") writeFileSync(join(f.options.portal, "public", f.uv.pages[0].path), `${f.uv.pages[0].html}<!-- changed -->`);
    if (kind === "suite") { const v = read(f.uv.suitePath); v.cases[0].expected.venues[0].capacity = 99; write(f.uv.suitePath, v); }
    if (kind === "manifest") { const path = join(f.options.portal, "trusted/manifests/org-b-update.json"), v = read(path); v.pages[0].sha256 = "a".repeat(64); write(path, v); }
    if (kind === "fragment") { const v = read(f.av.fragmentPath); v.examples[0].html = "oracle substitute"; write(f.av.fragmentPath, v); }
    const before = snapshot(f.options.portal);
    assert.throws(f.prepare, /differs/, kind);
    assert.equal(existsSync(f.options.out), false);
    assert.deepEqual(snapshot(f.options.portal), before);
  }
});

test("a feasible baseline is required for every policy predicate", (t) => {
  const mutations = [
    (f) => { f.policy.attendees = 200; },
    (f) => { f.policy.budget.minor_units = 74999; },
    (f) => { f.policy.budget.currency = "CAD"; },
    (f) => { f.policy.window.end = "2026-11-14T23:00:00-08:00"; },
    (f) => { f.bSpec.pages[0].venues[0].availability = "unavailable"; f.replaceB(); },
    (f) => { f.bSpec.pages[0].venues[0].step_free = false; f.replaceB(); },
  ];
  for (const mutate of mutations) {
    const f = fixture(t); mutate(f); write(f.options.policy, f.policy);
    assert.throws(f.prepare, /baseline selected venue is not feasible/);
    assert.equal(existsSync(f.options.out), false);
  }
});

test("the second snapshot must be exactly a same-event capacity failure", (t) => {
  const mutations = [
    [(f) => { f.updateSpec.pages[0].venues[0].capacity = 160; }, /fail capacity only/],
    [(f) => { f.updateSpec.pages[0].venues[0].price_minor += 1; }, /every venue fact/],
    [(f) => { f.updateSpec.pages[0].venues[1].capacity += 1; }, /every venue fact/],
    [(f) => { f.updateSpec.pages[0].venues[0].step_free = false; }, /fail capacity only/],
    [(f) => { f.updateSpec.pages[0].venues[0].venue_id = "foreign-venue"; }, /exactly one supported/],
    [(f) => { f.updateSpec.attendees = 151; }, /same event input/],
    [(f) => { f.updateSpec.date = "2026-11-15"; }, /fail capacity only/],
    [(f) => { f.updateSpec.pages[0].missing = "price"; f.updateSpec.pages[0].venues = [f.updateSpec.pages[0].venues[0]]; }, /two distinct supported layouts|exactly one supported/],
  ];
  for (const [mutate, expected] of mutations) {
    const f = fixture(t); mutate(f);
    assert.throws(() => { f.replaceUpdate(); f.prepare(); }, expected);
    assert.equal(existsSync(f.options.out), false);
  }
});

test("the appended case cannot reuse an existing B case identity", (t) => {
  const f = fixture(t);
  f.bSpec.pages[1].slug = "monitoring-update-campus-rooms";
  f.replaceB();
  assert.throws(f.prepare, /case identities and inputs must be unique/);
  assert.equal(existsSync(f.options.out), false);
});

test("a changed policy cannot silently change the invocation's event", (t) => {
  const f = fixture(t);
  f.policy.attendees = 140; write(f.options.policy, f.policy);
  assert.throws(f.prepare, /policy must match/);
});

test("configuration rejects unknown and duplicate fields, unsafe numbers and malformed windows", (t) => {
  const variants = [
    (f) => write(f.options.policy, { ...f.policy, oracle: {} }),
    (f) => write(f.options.policy, { ...f.policy, attendees: 1.5 }),
    (f) => write(f.options.policy, { ...f.policy, budget: { minor_units: 1e15, currency: "USD" } }),
    (f) => write(f.options.policy, { ...f.policy, window: { start: "2026-02-31T18:00:00Z", end: "2026-03-04T22:00:00Z" } }),
    (f) => write(f.options.policy, { ...f.policy, window: { ...f.policy.window, end: f.policy.window.start } }),
    (f) => write(f.options.policy, { ...f.policy, window: { ...f.policy.window, start: "2026-11-14T18:00:00.000000001-08:00" } }),
    (f) => write(f.options.template, { ...f.template, note: "not ScenarioSpec" }),
    (f) => write(f.options.template, { ...f.template, provider_access_acknowledged: "true" }),
    (f) => write(f.options.template, { ...f.template, candidate: { ...f.template.candidate, source_version: "01.1" } }),
    (f) => write(f.options.template, { ...f.template, repair: f.template.candidate }),
    (f) => write(f.options.template, { ...f.template, venue_monitoring: { policy: f.policy } }),
    (f) => writeFileSync(f.options.policy, JSON.stringify(f.policy).replace('"attendees":150', '"attendees":150,"attend\\u0065es":151')),
  ];
  for (const mutate of variants) {
    const f = fixture(t); mutate(f); assert.throws(f.prepare); assert.equal(existsSync(f.options.out), false);
  }
});

test("sources and identities obey the executable pipeline's stricter limits", (t) => {
  const local = fixture(t, { base: "http://127.0.0.1:7200" });
  assert.throws(local.prepare, /canonical HTTPS/);
  const long = fixture(t, { base: `${BASE}/${"a".repeat(1000)}` });
  assert.throws(long.prepare, /canonical HTTPS/);
  const f = fixture(t);
  assert.throws(() => prepareMonitoring({ ...f.options, a: "../org-a" }), /namespace/);
  assert.throws(() => prepareMonitoring({ ...f.options, update: "org-b" }), /distinct namespaces/);
  assert.throws(() => prepareMonitoring({ ...f.options, suiteId: f.bv.suite.suite_id }), /distinct identity/);
  assert.throws(() => prepareMonitoring({ ...f.options, suiteId: undefined }), /new suite ID/);
  assert.throws(() => prepareMonitoring({ ...f.options, suiteId: "../invalid" }), /new suite ID/);
});

test("generator-coercible identities cannot escape into strict Rust JSON", (t) => {
  for (const suiteId of [123, ["numeric-like-suite"]]) {
    const f = fixture(t);
    const spec = presetSpec("baseline", "coercible-suite");
    spec.suite_id = suiteId;
    writeVariant({ spec, baseUrl: BASE, out: f.options.portal, namespace: "org-a", replace: true });
    assert.throws(f.prepare, /manifest identity mismatch/);
    assert.equal(existsSync(f.options.out), false);
  }
  const badCase = fixture(t);
  badCase.bSpec.pages[0].slug = 123;
  badCase.replaceB();
  assert.throws(badCase.prepare, /identities must be strings/);
  const badVenue = fixture(t);
  badVenue.bSpec.pages[0].venues[1].venue_id = ["coercible-venue"];
  badVenue.replaceB();
  assert.throws(badVenue.prepare, /identities must be strings/);
});

test("fresh output and physical separation prevent overwrite or public oracle publication", (t) => {
  const f = fixture(t), before = snapshot(f.options.portal);
  f.prepare();
  const setupBefore = snapshot(f.options.out);
  assert.throws(f.prepare, /already exists/);
  assert.deepEqual(snapshot(f.options.out), setupBefore);
  assert.throws(() => prepareMonitoring({ ...f.options, out: join(f.options.portal, "public", "leak") }), /outside the portal/);
  const alias = join(f.dir, "public-alias");
  symlinkSync(join(f.options.portal, "public"), alias, "dir");
  assert.throws(() => prepareMonitoring({ ...f.options, out: join(alias, "leak") }), /outside the portal/);
  const dangling = join(f.dir, "dangling-output");
  symlinkSync(join(f.dir, "missing-target"), dangling, "dir");
  assert.throws(() => prepareMonitoring({ ...f.options, out: dangling }), /already exists/);
  assert.deepEqual(snapshot(f.options.portal), before);
  assert.deepEqual(readdirSync(f.dir).filter((name) => name.includes(".prepare-monitoring")), []);
});

test("public symlink aliases cannot publish a supposedly external trusted setup", async (t) => {
  for (const kind of ["public root", "public child"]) {
    const f = fixture(t);
    const publicRoot = join(f.options.portal, "public");
    const served = join(f.dir, "externally-served");
    if (kind === "public root") {
      renameSync(publicRoot, served);
      symlinkSync(served, publicRoot, "dir");
    } else {
      mkdirSync(served);
      symlinkSync(served, join(publicRoot, "export"), "dir");
    }
    const out = join(served, "setup");
    assert.throws(() => prepareMonitoring({ ...f.options, out }), /no symlinks/, kind);
    assert.equal(existsSync(out), false);
    const server = createServer(publicHandler(publicRoot));
    await new Promise((ok) => server.listen(0, "127.0.0.1", ok));
    try {
      const origin = `http://127.0.0.1:${server.address().port}`;
      assert.equal((await fetch(`${origin}/${f.av.pages[0].path}`)).status, 200, "the public tree actually remains served");
      const prefix = kind === "public child" ? "/export" : "";
      assert.equal((await fetch(`${origin}${prefix}/setup/trusted/suite-b.json`)).status, 404, "no oracle was emitted through a served alias");
    } finally { server.closeAllConnections(); await new Promise((ok) => server.close(ok)); }
  }
});

test("the strict CLI rejects duplicates and concurrent writers leave one complete output", async (t) => {
  const f = fixture(t);
  for (const extra of [["--out", "elsewhere"], ["--unknown", "bad"], ["unexpected"]]) {
    const result = spawnSync(process.execPath, [...cliArgs(f.options), ...extra], { encoding: "utf8" });
    assert.equal(result.status, 2);
    assert.equal(existsSync(f.options.out), false);
  }
  const run = () => new Promise((ok, reject) => {
    const child = spawn(process.execPath, cliArgs(f.options), { stdio: "pipe" });
    let stderr = ""; child.stderr.on("data", (chunk) => { stderr += chunk; });
    child.on("error", reject); child.on("close", (code) => ok({ code, stderr }));
  });
  const results = await Promise.all([run(), run()]);
  assert.deepEqual(results.map((r) => r.code).sort(), [0, 2]);
  assert.equal(read(join(f.options.out, "trusted/suite-b.json")).cases.length, 4);
  assert.equal(read(join(f.options.out, "scenario.json")).venue_monitoring.policy.attendees, 150);
  assert.deepEqual(readdirSync(f.dir).filter((name) => name.includes(".prepare-monitoring")), []);
});

const VERIFY = process.env.GENESIS_VERIFY_BIN ?? join(REPO, "target/debug/genesis-verify");
test("the real Rust verifier enforces all four cases and rejects wrong/stale capacity evidence", { skip: !existsSync(VERIFY) && "set GENESIS_VERIFY_BIN to an already-built genesis-verify" }, async (t) => {
  const f = fixture(t), paths = f.prepare(), suite = read(paths.suitePath);
  const outputs = await servedRuns(t, f, suite);
  const artifact = { capability_id: "setup-offline", capability_version: "1", source_sha256: sha256("independent fixture extractor"), actor_id: "synthetic-actor", build_id: "synthetic-build", build_number: "0.1.1" };
  const runs = suite.cases.map((c, i) => ({ case_id: c.case_id, artifact, input_sha256: jsonSha256(c.input), source: c.source, run_id: `run-${i}`, dataset_id: `dataset-${i}`, status: "succeeded", output: outputs[i] }));
  const artifactPath = join(f.dir, "artifact.json"), runsPath = join(f.dir, "runs.json");
  write(artifactPath, artifact);
  const check = (value) => { write(runsPath, value); return spawnSync(VERIFY, ["--suite", paths.suitePath, "--artifact", artifactPath, "--runs", runsPath], { encoding: "utf8", timeout: 10000 }); };
  const good = check(runs); assert.equal(good.status, 0, good.stdout + good.stderr);
  const wrong = structuredClone(runs); wrong[3].output.venues[0].capacity = 180;
  assert.equal(check(wrong).status, 1, "baseline capacity cannot stand in for update evidence");
  const stale = structuredClone(runs); stale[3].source.sha256 = runs[0].source.sha256;
  assert.equal(check(stale).status, 1, "a prior snapshot cannot stand in for the update");
  assert.equal(check(runs.slice(0, 3)).status, 1, "three baseline cases cannot claim the extended suite");
});

const SCENARIO = process.env.GENESIS_SCENARIO_BIN;
test("the complete scenario passes the real strict Rust decoder without granting Actor access", { skip: !SCENARIO && "set GENESIS_SCENARIO_BIN to the monitoring-enabled genesis binary" }, (t) => {
  const f = fixture(t), paths = f.prepare();
  const run = (path) => spawnSync(SCENARIO, ["scenario", "--spec", path, "--org-a", "http://127.0.0.1:1", "--org-b", "http://127.0.0.1:2"], {
    encoding: "utf8", timeout: 10000,
    env: { ...process.env, GENESIS_ORG_A_OPERATOR_TOKEN: "offline-test-a", GENESIS_ORG_B_OPERATOR_TOKEN: "offline-test-b", HTTP_PROXY: "", HTTPS_PROXY: "", ALL_PROXY: "", NO_PROXY: "*" },
  });
  const unacknowledged = run(paths.scenarioPath);
  assert.notEqual(unacknowledged.status, 0);
  assert.match(unacknowledged.stderr, /permitted shared Actor access must be acknowledged/);
  const acknowledged = read(paths.scenarioPath); acknowledged.provider_access_acknowledged = true;
  const acknowledgedPath = join(f.dir, "explicit-test-ack.json"); write(acknowledgedPath, acknowledged);
  const offline = run(acknowledgedPath);
  assert.notEqual(offline.status, 0);
  assert.match(offline.stderr, /transport|request failed/, offline.stderr);
  assert.doesNotMatch(offline.stderr, /unknown field|invalid scenario|invalid JSON/);
  assert.equal(read(paths.scenarioPath).provider_access_acknowledged, false, "smoke check cannot change the generated setup");
});
