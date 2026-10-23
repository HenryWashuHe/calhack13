#!/usr/bin/env node
// Offline setup for frozen, configured snapshot coverage. No provider calls,
// public source edits, expected answers in the scenario, or access grants.
import { existsSync, lstatSync, mkdirSync, mkdtempSync, readFileSync, realpathSync, renameSync, rmSync, writeFileSync } from "node:fs";
import { dirname, isAbsolute, join, relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { parseArgs } from "node:util";
import { build, canonicalJson, isCalendarDate, jsonSha256, NAMESPACE_RE, sha256 } from "./lib.mjs";

const MAX_JSON = 2 * 1024 * 1024;
const MAX_HTML = 128 * 1024;
const SUITE_ID = /^[a-z0-9][a-z0-9.-]{2,63}$/;
const SAFE_ID = /^[A-Za-z0-9_.:-]+$/;
const bytes = (value) => Buffer.byteLength(JSON.stringify(value));
const same = (a, b) => canonicalJson(a) === canonicalJson(b);
function requireThat(condition, message) { if (!condition) throw new Error(message); }
function present(path) {
  try { lstatSync(path); return true; } catch (error) { if (error.code === "ENOENT") return false; throw error; }
}
function object(value, allowed, required, label) {
  requireThat(value && typeof value === "object" && !Array.isArray(value), `${label} must be an object`);
  requireThat(Object.keys(value).every((k) => allowed.includes(k)) && required.every((k) => Object.hasOwn(value, k)), `${label} has missing or unknown fields`);
}
function boundedText(value, limit) {
  return typeof value === "string" && value.length > 0 && Buffer.byteLength(value) <= limit && value.trim() === value && !/[\p{Cc}\p{Cs}]/u.test(value);
}
function readBounded(path, limit, label) {
  const stat = lstatSync(path);
  requireThat(stat.isFile() && stat.size <= limit, `${label} must be a bounded regular file`);
  const data = readFileSync(path);
  requireThat(data.length <= limit, `${label} exceeds its byte limit`);
  // Reject invalid UTF-8 instead of silently substituting replacement bytes.
  return new TextDecoder("utf-8", { fatal: true }).decode(data);
}
function readJson(path, label) {
  const text = readBounded(path, MAX_JSON, label);
  let value;
  try { value = JSON.parse(text); } catch { throw new Error(`${label} is not valid JSON`); }
  // Native parsing checks syntax; this bounded token walk also rejects duplicate
  // keys (including escaped aliases), as the server's strict decoder does.
  const tokens = text.match(/"(?:[^"\\]|\\.)*"|[{}\[\],:]|[^\s{}\[\],:]+/gs) ?? [];
  let at = 0;
  function visit(depth) {
    requireThat(depth <= 64, `${label} is nested too deeply`);
    const token = tokens[at++];
    if (token === "{") {
      const keys = new Set();
      while (tokens[at] !== "}") {
        const key = JSON.parse(tokens[at++]);
        requireThat(!keys.has(key), `${label} contains duplicate fields`);
        keys.add(key);
        at++; // colon, already checked by JSON.parse
        visit(depth + 1);
        if (tokens[at] !== ",") break;
        at++;
      }
      at++;
    } else if (token === "[") {
      while (tokens[at] !== "]") {
        visit(depth + 1);
        if (tokens[at] !== ",") break;
        at++;
      }
      at++;
    }
  }
  visit(0);
  requireThat(at === tokens.length, `${label} has invalid structure`);
  return value;
}
function timestamp(value) {
  if (typeof value !== "string" || value.length > 64) return NaN;
  const m = /^(\d{4}-\d{2}-\d{2})T([01]\d|2[0-3]):([0-5]\d):([0-5]\d)(?:\.(\d{1,9}))?(Z|[+-](?:[01]\d|2[0-3]):[0-5]\d)$/.exec(value);
  if (!m || !isCalendarDate(m[1])) return NaN;
  // Portal windows have whole seconds. Refuse sub-millisecond precision that
  // JavaScript would round away when checking Rust's exact time predicates.
  if (m[5] && /[1-9]/.test(m[5].slice(3))) return NaN;
  return Date.parse(value);
}
function validatePolicy(policy) {
  const keys = ["selected_venue_id", "attendees", "budget", "window", "step_free_required"];
  object(policy, keys, keys, "policy");
  object(policy.budget, ["minor_units", "currency"], ["minor_units", "currency"], "policy budget");
  object(policy.window, ["start", "end"], ["start", "end"], "policy window");
  requireThat(boundedText(policy.selected_venue_id, 256), "policy selected venue is invalid");
  requireThat(Number.isSafeInteger(policy.attendees) && policy.attendees >= 1 && policy.attendees <= 1_000_000, "policy attendees is invalid");
  requireThat(Number.isSafeInteger(policy.budget.minor_units) && policy.budget.minor_units >= 0 && policy.budget.minor_units <= 1_000_000_000_000 && /^[A-Z]{3}$/.test(policy.budget.currency), "policy budget is invalid");
  requireThat(Number.isFinite(timestamp(policy.window.start)) && timestamp(policy.window.end) > timestamp(policy.window.start), "policy window must be an increasing RFC3339 interval");
  requireThat(typeof policy.step_free_required === "boolean", "policy step-free requirement must be boolean");
}
function identity(value, label) {
  const keys = ["capability_id", "capability_version", "actor_name", "source_version"];
  object(value, keys, keys, label);
  requireThat(boundedText(value.capability_id, 256) && boundedText(value.capability_version, 256), `${label} capability identity is invalid`);
  requireThat(typeof value.actor_name === "string" && value.actor_name.length <= 64 && /^[a-z0-9](?:[a-z0-9-]*[a-z0-9])?$/.test(value.actor_name), `${label} Actor name is invalid`);
  requireThat(typeof value.source_version === "string" && /^(?:0|[1-9]\d?)\.(?:0|[1-9]\d?)$/.test(value.source_version), `${label} source version must be major.minor (0..99)`);
}
function validateTemplate(template) {
  object(template, ["run_id", "progression", "suite_a_id", "suite_b_id", "candidate", "repair", "allowed_source_urls", "examples", "invocation_input", "allow_development_roles", "provider_access_acknowledged", "venue_monitoring"], ["run_id", "candidate"], "scenario template");
  requireThat(typeof template.run_id === "string" && template.run_id.length <= 96 && SAFE_ID.test(template.run_id), "scenario run_id is invalid");
  requireThat(template.progression === undefined || ["manual", "automatic"].includes(template.progression), "scenario progression is invalid");
  for (const field of ["allow_development_roles", "provider_access_acknowledged"]) requireThat(template[field] === undefined || typeof template[field] === "boolean", `scenario ${field} must be boolean`);
  requireThat(template.venue_monitoring == null, "template already contains monitoring; supply an unconfigured template and explicit policy");
  identity(template.candidate, "candidate");
  if (template.repair != null) {
    identity(template.repair, "repair");
    requireThat(template.repair.capability_id === template.candidate.capability_id && template.repair.capability_version !== template.candidate.capability_version && template.repair.actor_name !== template.candidate.actor_name, "repair must use a distinct version and Actor for the same capability");
  }
}
function loadVariant(portal, namespace) {
  requireThat(typeof namespace === "string" && NAMESPACE_RE.test(namespace), "invalid portal namespace");
  const manifest = readJson(join(portal, "trusted", "manifests", `${namespace}.json`), "portal manifest");
  requireThat(manifest.namespace === namespace && SUITE_ID.test(manifest.suite_id), "manifest identity mismatch");
  const regenerated = build(manifest.spec, manifest.base_url, namespace);
  requireThat(same(manifest, regenerated.manifest), "manifest differs from generator output");
  const suitePath = join(portal, "trusted", "suites", `${manifest.suite_id}.json`);
  requireThat(same(readJson(suitePath, "portal suite"), regenerated.suite), "suite differs from generator output");
  requireThat(same(readJson(join(portal, "fragments", `${namespace}.json`), "portal fragment"), regenerated.fragment), "fragment differs from generator output");
  for (const page of regenerated.pages) {
    const html = readBounded(join(portal, "public", page.path), MAX_HTML, "public page");
    requireThat(html === page.html && sha256(Buffer.from(html, "utf8")) === page.sha256, "public page differs from its frozen snapshot");
  }
  return { ...regenerated, suitePath };
}
function selectedCase(variant, venueId, label) {
  const cases = variant.suite.cases.filter((c) => c.expected.status === "supported" && c.expected.venues.some((v) => v.venue_id === venueId));
  requireThat(cases.length === 1, `${label} must have exactly one supported case for the selected venue`);
  return cases[0];
}
function checks(venue, policy) {
  return {
    capacity: venue.capacity >= policy.attendees,
    price: venue.price.currency === policy.budget.currency && venue.price.minor_units <= policy.budget.minor_units,
    window: timestamp(venue.window.start) <= timestamp(policy.window.start) && timestamp(venue.window.end) >= timestamp(policy.window.end),
    availability: venue.availability === "available",
    step_free: !policy.step_free_required || venue.step_free,
  };
}
function validateTransition(baseline, update, policy) {
  const before = baseline.expected.venues.find((v) => v.venue_id === policy.selected_venue_id);
  const after = update.expected.venues.find((v) => v.venue_id === policy.selected_venue_id);
  requireThat(Object.values(checks(before, policy)).every(Boolean), "baseline selected venue is not feasible under the explicit policy");
  requireThat(same(checks(after, policy), { capacity: false, price: true, window: true, availability: true, step_free: true }), "updated venue must fail capacity only");
  requireThat(baseline.source.url !== update.source.url && baseline.source.sha256 !== update.source.sha256, "baseline and update must have distinct frozen URLs and bytes");
  const facts = (c, substituteCapacity) => c.expected.venues.map(({ evidence: _evidence, ...venue }) => ({ ...venue, capacity: substituteCapacity && venue.venue_id === policy.selected_venue_id ? before.capacity : venue.capacity }));
  requireThat(same(facts(baseline, false), facts(update, true)), "update must preserve every venue fact except the selected capacity");
  const { source_url: _beforeUrl, ...beforeInput } = baseline.input;
  const { source_url: _afterUrl, ...afterInput } = update.input;
  requireThat(same(beforeInput, afterInput), "baseline and update must describe the same event input");
  requireThat(baseline.input.attendees === policy.attendees && timestamp(baseline.input.requested_window.start) === timestamp(policy.window.start) && timestamp(baseline.input.requested_window.end) === timestamp(policy.window.end), "policy must match the selected invocation's attendees and event window");
}
function within(path, parent) {
  const rel = relative(parent, path);
  return rel === "" || (!rel.startsWith(`..${process.platform === "win32" ? "\\" : "/"}`) && rel !== ".." && !isAbsolute(rel));
}
function physicalDestination(path) {
  let ancestor = path;
  while (!existsSync(ancestor)) ancestor = dirname(ancestor);
  return resolve(realpathSync(ancestor), relative(ancestor, path));
}

/** Inputs are trusted local configuration, not candidate-provided oracle data.
 * Validates everything before writing; the only writes are a new setup directory
 * and a temporary sibling/lock. Concurrent helper runs cannot replace output.
 */
export function prepareMonitoring({ portal, a, b, update, policy: policyPath, suiteId, template: templatePath, out }) {
  requireThat([portal, policyPath, templatePath, out].every((p) => typeof p === "string" && p.length > 0), "portal, policy, template and out paths are required");
  requireThat(typeof suiteId === "string" && SUITE_ID.test(suiteId), "new suite ID must be 3..64 lowercase letters, digits, dots or hyphens");
  requireThat(new Set([a, b, update]).size === 3, "A, B and update must use distinct namespaces");
  const root = realpathSync(resolve(portal));
  const destination = resolve(out);
  requireThat(!within(physicalDestination(destination), root), "output must be outside the portal directory (never publicly served)");
  requireThat(!present(destination), "output directory already exists; choose a fresh path");
  const policy = readJson(policyPath, "policy");
  const template = readJson(templatePath, "scenario template");
  validatePolicy(policy);
  validateTemplate(template);
  const av = loadVariant(root, a), bv = loadVariant(root, b), uv = loadVariant(root, update);
  requireThat(new Set([av.suite.suite_id, bv.suite.suite_id, uv.suite.suite_id, suiteId]).size === 4 && !present(join(root, "trusted", "suites", `${suiteId}.json`)), "combined suite needs a new, distinct identity");
  const baseline = selectedCase(bv, policy.selected_venue_id, "B baseline");
  const cut = selectedCase(uv, policy.selected_venue_id, "update");
  validateTransition(baseline, cut, policy);
  requireThat(bv.suite.cases.length < 128, "combined suite exceeds 128 cases");
  const appended = { ...cut, case_id: `monitoring-update-${cut.case_id}` };
  const suite = { ...bv.suite, suite_id: suiteId, scope: "Synthetic venue extraction with frozen configured snapshots: baseline plus one same-event capacity cut. This does not verify arbitrary later web changes or HTML freshness.", cases: [...bv.suite.cases, appended] };
  requireThat(new Set(suite.cases.map((c) => c.case_id)).size === suite.cases.length && new Set(suite.cases.map((c) => jsonSha256(c.input))).size === suite.cases.length, "combined suite case identities and inputs must be unique");
  const urls = [...av.fragment.allowed_source_urls, ...bv.fragment.allowed_source_urls, cut.source.url];
  requireThat(urls.length <= 100 && new Set(urls).size === urls.length, "source URLs must be distinct and within the 100-source bound");
  for (const source of urls) {
    const url = new URL(source);
    requireThat(Buffer.byteLength(source) <= 1024 && url.protocol === "https:" && url.hostname && !url.username && !url.password && !url.hash && url.href === source, "Actor sources require canonical HTTPS URLs without credentials or fragments");
  }
  requireThat(av.fragment.examples.length >= 1 && av.fragment.examples.length <= 8 && av.fragment.examples.every((e) => e.html.trim() && !e.html.includes("\0") && Buffer.byteLength(e.html) <= MAX_HTML && urls.includes(e.url)), "A examples exceed synthesis bounds");
  const scenario = {
    run_id: template.run_id, progression: template.progression ?? "manual",
    suite_a_id: av.suite.suite_id, suite_b_id: suiteId,
    candidate: template.candidate, repair: template.repair ?? null,
    allowed_source_urls: urls, examples: av.fragment.examples,
    invocation_input: baseline.input,
    allow_development_roles: template.allow_development_roles ?? false,
    provider_access_acknowledged: template.provider_access_acknowledged ?? false,
    venue_monitoring: { policy, updated_invocation_input: cut.input },
  };
  // Leave room for the server's fixed acquisition requirement and operation ID.
  // This intentionally refuses near-limit templates rather than emitting a
  // scenario whose generated public SynthesisRequest could exceed 512 KiB.
  requireThat(bytes({ candidate: scenario.candidate, allowed_source_urls: urls, examples: scenario.examples }) + 32 * 1024 <= 512 * 1024, "public synthesis request exceeds its byte budget");
  requireThat(bytes(scenario) <= MAX_JSON && bytes(suite) <= MAX_JSON, "setup exceeds its JSON byte budget");
  const paths = { scenarioPath: join(destination, "scenario.json"), suitePath: join(destination, "trusted", "suite-b.json"), setupPath: join(destination, "setup.json") };
  const setup = {
    schema_version: 1,
    coverage: "frozen configured snapshot coverage; no native execution or arbitrary later-update proof",
    suite_a: { id: av.suite.suite_id, path: av.suitePath, sha256: jsonSha256(av.suite) },
    suite_b: { id: suiteId, path: paths.suitePath, sha256: jsonSha256(suite) },
    scenario: { path: paths.scenarioPath, sha256: jsonSha256(scenario) },
    policy_sha256: jsonSha256(policy),
    public_root: join(root, "public"),
    namespaces: { a, b, update },
    baseline: { case_id: baseline.case_id, source: baseline.source },
    updated: { case_id: appended.case_id, source: cut.source },
    generation_examples: av.manifest.held_out_examples ? "A examples are separate from A verification pages" : "A suite pages are examples: reproducibility, not held-out A verification",
    provider_access_acknowledged: scenario.provider_access_acknowledged,
  };
  mkdirSync(dirname(destination), { recursive: true });
  const lock = `${destination}.prepare-monitoring.lock`;
  mkdirSync(lock); // Exclusive across concurrent invocations of this helper.
  let staging;
  try {
    requireThat(!present(destination), "output directory already exists; choose a fresh path");
    staging = mkdtempSync(join(dirname(destination), ".prepare-monitoring-"));
    mkdirSync(join(staging, "trusted"));
    for (const [path, value] of [["scenario.json", scenario], ["trusted/suite-b.json", suite], ["setup.json", setup]]) writeFileSync(join(staging, path), `${JSON.stringify(value, null, 2)}\n`, { flag: "wx" });
    requireThat(!present(destination), "output directory already exists; choose a fresh path");
    renameSync(staging, destination);
    staging = undefined;
  } finally {
    if (staging) rmSync(staging, { recursive: true, force: true });
    rmSync(lock, { recursive: true, force: true });
  }
  return paths;
}

function main() {
  const { values, tokens } = parseArgs({ strict: true, allowPositionals: false, tokens: true, options: Object.fromEntries(["portal", "a", "b", "update", "policy", "suite-id", "template", "out"].map((key) => [key, { type: "string" }]).concat([["help", { type: "boolean" }]])) });
  const names = tokens.filter((t) => t.kind === "option").map((t) => t.name);
  requireThat(names.length === new Set(names).size, "duplicate CLI options are not accepted");
  if (values.help) {
    console.log("usage: prepare-monitoring.mjs --portal DIR --a NS --b NS --update NS --policy policy.json --suite-id NEW-ID --template mission.json --out FRESH-DIR\nGenerate all three immutable namespaces first. Output must be outside the portal. The template supplies identities and permissions; this command never grants provider access.");
    return;
  }
  const paths = prepareMonitoring({ ...values, suiteId: values["suite-id"] });
  console.log(JSON.stringify(paths, null, 2));
}
if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try { main(); } catch (error) { console.error(`error: ${error.message}`); process.exitCode = 2; }
}
