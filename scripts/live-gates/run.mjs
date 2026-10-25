#!/usr/bin/env node
// Run the four live sponsor gates (docs/TEAM_WORKSTREAMS.md, "Hours 0–4") with
// the repository's own durable CLIs, and write sanitized evidence.
//
//   node scripts/live-gates/run.mjs [--gates elastic,fetch,apify,creao]
//        [--out data/live-gates] [--bin-dir target/debug]
//        [--portal data/portal --portal-suite <suite.json>]   # reachable pages for Apify
//        [--allow-billable]                                    # Apify build/run, CREAO app run
//
// Each gate reports passed | partial | failed | needs_reconciliation |
// not_tested, with provider IDs and measured durations. A gate without its
// credentials or binary is "not_tested", never a pass. The kit only drives
// the CLIs one step at a time: it never edits checkpoints, never replays a
// POST, and stops at any unknown outcome. Output keeps only parsed fields;
// configured credentials are scrubbed from public evidence reports. Private
// resumable checkpoints remain owned by their CLIs and must not be shared.
import { spawnSync } from "node:child_process";
import { closeSync, cpSync, existsSync, fsyncSync, mkdirSync, openSync, readFileSync, realpathSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { parseArgs } from "node:util";

import { runTrigger, triggerRunsUrl } from "./creao-trigger.mjs";

export const GATES = ["elastic", "fetch", "apify", "creao"];
export const SECRET_ENV = [
  "ELASTIC_DATA_API_KEY",
  "ELASTIC_MANAGEMENT_API_KEY",
  "AGENTVERSE_API_KEY",
  "GENESIS_FETCH_BRIDGE_TOKEN",
  "APIFY_TOKEN",
  "CREAO_TRIGGER_API_KEY",
  "CREAO_TRIGGER_API_KEY_PREVIOUS",
  "CREAO_ACCOUNT_API_KEY",
  "CREAO_V2_API_KEY",
  "ASI_ONE_API_KEY",
  "GENESIS_LLM_API_KEY",
  "GENESIS_AGENT_TOKEN",
  "ANTHROPIC_API_KEY",
];
const REQUIRED = {
  elastic: ["ELASTICSEARCH_URL", "ELASTIC_DATA_API_KEY", "ELASTIC_KIBANA_URL", "ELASTIC_MANAGEMENT_API_KEY"],
  fetch: ["AGENTVERSE_API_KEY"],
  apify: ["APIFY_TOKEN"],
  creao: ["CREAO_TRIGGER_API_KEY", "CREAO_TRIGGER_APP_ID"],
};
const BILLABLE = new Set(["apify", "creao"]);

/** Scrub configured credentials from strings, preserving JSON scalar types. */
export function scrub(value, env = process.env) {
  const names = new Set([
    ...SECRET_ENV,
    ...Object.keys(env).filter((name) => /(?:^|_)(?:API_KEY|TOKEN|SECRET|PASSWORD|SEED)(?:_(?:PREVIOUS|OLD|BACKUP|RETIRED))?$/i.test(name)),
  ]);
  const replacements = new Map();
  for (const name of names) {
    const secret = env[name];
    if (typeof secret !== "string" || !secret.length) continue;
    // Diagnostics can contain raw, JSON-escaped or URL-encoded credentials.
    const variants = [secret, JSON.stringify(secret).slice(1, -1)];
    try { variants.push(encodeURIComponent(secret)); } catch { /* unpaired surrogate */ }
    for (const variant of variants) {
      if (!replacements.has(variant)) replacements.set(variant, `<${name}>`);
    }
  }
  const alternatives = [...replacements.keys()].sort((a, b) => b.length - a.length);
  const pattern = alternatives.length
    ? new RegExp(alternatives.map((secret) => secret.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")).join("|"), "g")
    : null;
  // One replacement pass keeps prefix-overlapping secrets and marker text safe.
  const redact = (text) => pattern ? text.replace(pattern, (secret) => replacements.get(secret)) : text;
  const visit = (item) => {
    if (typeof item === "string") return redact(item);
    if (Array.isArray(item)) return item.map(visit);
    if (item && typeof item === "object") {
      return Object.fromEntries(Object.entries(item).map(([key, child]) => [redact(key), visit(child)]));
    }
    return item;
  };
  if (typeof value === "string") return redact(value);
  const serialized = JSON.stringify(value);
  return serialized === undefined ? undefined : visit(JSON.parse(serialized));
}

export function missingEnv(gate, env = process.env) {
  return REQUIRED[gate].filter((n) => !env[n]);
}

const now = () => new Date().toISOString();
const sleep = (ms) => new Promise((ok) => setTimeout(ok, ms));

/** Run a CLI once; return exit code, parsed JSON stdout (if any) and timing. */
export function runCli(bin, args, { env = process.env, cwd } = {}) {
  const started = Date.now();
  const r = spawnSync(bin, args, { env, cwd, encoding: "utf8", maxBuffer: 16 * 1024 * 1024 });
  let json = null;
  try {
    json = JSON.parse(r.stdout);
  } catch {
    /* not JSON */
  }
  return {
    code: r.status ?? (r.error ? 127 : 1),
    json,
    stdout: r.stdout ?? "",
    // Keep only a short, scrubbed error tail for diagnosis.
    stderr: scrub(r.stderr ?? "", env).slice(-600),
    ms: Date.now() - started,
  };
}

// ---------------- gates ----------------

export async function elasticGate(ctx) {
  const bin = join(ctx.binDir, "genesis-elastic-smoke");
  if (!existsSync(bin)) return { status: "not_tested", reason: `no ${bin} (built from the Elastic branch, PR #11)` };
  const report = join(ctx.out, "elastic-smoke.json");
  // Agent Builder rejects the ids derived from a long organization name (the stamped name gave a 69-character
  // tool id and HTTP 400 at "create specialist"); the time part of the stamp keeps runs distinct and the ids short.
  const org = `genesis-smoke-${String(ctx.stamp).slice(-6)}`;
  const r = runCli(bin, ["--org", org, "--skip-capabilities", "--converse", "--cleanup", "--report", report], ctx);
  if (!existsSync(report)) return { status: "failed", reason: "smoke wrote no report", exit: r.code, error: r.stderr, duration_ms: r.ms };
  const rawReport = readFileSync(report, "utf8");
  let rep;
  try {
    rep = JSON.parse(rawReport);
  } catch {
    writeFileSync(report, scrub(rawReport, ctx.env));
    throw new Error("Elastic smoke report was not valid JSON");
  }
  // The smoke CLI's detailed report is public evidence too, including details
  // that are not selected for the summary. Do not rewrite private checkpoints.
  writeFileSync(report, `${JSON.stringify(scrub(rep, ctx.env), null, 2)}\n`);
  const step = (name) => rep.steps?.find((s) => s.step === name);
  const okDetail = (name) => step(name)?.detail?.Ok ?? null;
  return {
    status: rep.ok && r.code === 0 ? "passed" : "failed",
    duration_ms: r.ms,
    evidence: {
      org,
      agent_id: okDetail("create specialist"),
      reconcile_agent_id: okDetail("reconcile (no duplicate)"),
      evidence_doc: step("evidence search")?.detail?.doc_id ?? null,
      conversation_id: step("converse uses evidence tool")?.detail?.conversation_id ?? null,
      converse_status: step("converse uses evidence tool")?.detail?.status ?? null,
      steps: (rep.steps ?? []).map((s) => ({ step: s.step, ok: s.ok })),
    },
    note: "--skip-capabilities: the shared capability index write was not exercised. --cleanup deletes the agent only.",
  };
}

export async function fetchGate(ctx) {
  const bin = join(ctx.binDir, "genesis-fetch");
  if (!existsSync(bin)) return { status: "not_tested", reason: `no ${bin}` };
  const r = runCli(bin, ["discover", "--query", "logistics specialist", "--limit", "10"], ctx);
  if (r.code !== 0 || !r.json) return { status: "failed", exit: r.code, error: r.stderr, duration_ms: r.ms };
  return {
    status: "partial",
    duration_ms: r.ms,
    evidence: {
      search_id: r.json.search_id ?? null,
      num_hits: r.json.num_hits ?? null,
      total: r.json.total ?? null,
      compatible_candidates: (r.json.candidates ?? []).map((c) => c.address),
      rejected_candidates: r.json.rejected_candidates ?? null,
    },
    note: "Authenticated discovery only. The typed offer/accept/task/result round trip through the bridge is a separate step (scripts/demo-fetch-recruitment.py).",
  };
}

/** Repeat a one-action `step` until a terminal phase, an error or a limit. */
async function stepLoop({ ctx, bin, stepArgs, statusOf, terminal, pollMs, maxSteps, retryRead }) {
  const log = [];
  let last = null;
  for (let i = 0; i < maxSteps; i++) {
    const args = [...stepArgs];
    if (retryRead && last && retryRead(last)) args.push("--retry-read");
    const r = runCli(bin, args, ctx);
    const s = r.json ? statusOf(r.json) : null;
    log.push({ at: now(), exit: r.code, phase: s?.phase ?? null, ms: r.ms });
    if (!r.json) return { log, last, exit: r.code, error: r.stderr };
    last = r.json;
    if (terminal(s)) return { log, last, exit: r.code };
    // A nonzero exit stops the gate unless the CLI's own record explicitly
    // allows a read retry (Forge reports a retryable GET failure with exit 1).
    if (r.code !== 0 && !(retryRead && retryRead(r.json))) return { log, last, exit: r.code };
    await sleep((typeof pollMs === "function" ? pollMs(r.json) : pollMs) * (ctx.pollScale ?? 1));
  }
  return { log, last, exit: 0, limit: true };
}

/** POSIX single-quote an argument unless it is plainly safe. */
export function shellQuote(arg) {
  const s = String(arg);
  return /^[A-Za-z0-9_@%+=:,./-]+$/.test(s) ? s : `'${s.replace(/'/g, `'\\''`)}'`;
}

/**
 * The exact next step for a stopped gate. `--retry-read` is included only
 * when the CLI's own last record explicitly allows a read retry (the same
 * predicate the step loop uses), so the command can advance the saved
 * operation. `argv` is the structured form; `command` is shell-safe.
 */
export function resumeOf(bin, stepArgs, last, retryRead) {
  const argv = [bin, ...stepArgs];
  if (last && retryRead && retryRead(last)) argv.push("--retry-read");
  return { argv, command: argv.map(shellQuote).join(" ") };
}

export async function apifyGate(ctx) {
  const bin = join(ctx.binDir, "genesis-forge");
  if (!existsSync(bin)) return { status: "not_tested", reason: `no ${bin}` };
  // One directory per run: Forge never overwrites a checkpoint, and earlier
  // runs' evidence must stay intact and resumable.
  const work = join(ctx.out, "apify", ctx.stamp);
  mkdirSync(work, { recursive: true });
  // Hand-authored baseline parser (not AI-generated), adjusted to the scaffold layout.
  const parser = join(work, "parser.ts");
  const src = readFileSync(join(ctx.repo, "actors/venue-extractor-template/tests/baseline-extractor.ts"), "utf8");
  writeFileSync(parser, src.replace("'../src/contract.js'", "'./contract.js'"));
  const candidate = JSON.parse(readFileSync(join(ctx.repo, "crates/genesis-forge/examples/synthesis/candidate.json"), "utf8"));
  candidate.operation_id = `live-gate-${ctx.stamp}`;
  candidate.actor_name = `genesis-live-gate-${ctx.stamp}`;
  writeFileSync(join(work, "candidate.json"), JSON.stringify(candidate, null, 2));
  const suite = ctx.portalSuite ?? join(ctx.repo, "crates/genesis-verification/examples/fixtures/trusted/suite.json");
  const reachable = Boolean(ctx.portalSuite);
  const checkpoint = join(work, "forge.json");
  const prep = runCli(bin, ["prepare", "--candidate", join(work, "candidate.json"), "--extractor", parser, "--suite", suite, "--checkpoint", checkpoint], ctx);
  if (prep.code !== 0) return { status: "failed", stage: "prepare", exit: prep.code, error: prep.stderr };
  const started = Date.now();
  const stepArgs = ["step", "--suite", suite, "--checkpoint", checkpoint];
  const retryRead = (j) => Boolean(j.failure?.retryable && !j.failure?.ambiguous);
  const res = await stepLoop({
    ctx,
    bin,
    stepArgs,
    statusOf: (j) => j,
    terminal: (s) => ["verified", "rejected", "failed", "needs_reconciliation"].includes(s?.phase),
    pollMs: (j) => (j.pending_action === "poll_build" || j.pending_action === "collect_case" ? 5000 : 500),
    maxSteps: ctx.maxSteps,
    retryRead,
  });
  const resume = resumeOf(bin, stepArgs, res.last, retryRead);
  const last = res.last ?? {};
  const phase = last.phase ?? null;
  // An explicit terminal outcome takes precedence over partial evidence: an
  // oracle rejection after a good build is a failed verification gate.
  const status =
    phase === "needs_reconciliation"
      ? "needs_reconciliation"
      : phase === "verified"
        ? "passed"
        : phase === "rejected" || phase === "failed"
          ? "failed"
          : last.artifact?.build_id || last.failure?.build_id
            ? "partial"
            : "failed";
  return {
    status,
    duration_ms: Date.now() - started,
    evidence: {
      phase,
      actor_id: last.artifact?.actor_id ?? last.failure?.actor_id ?? null,
      build_id: last.artifact?.build_id ?? last.failure?.build_id ?? null,
      build_number: last.artifact?.build_number ?? null,
      failed_run_id: last.failure?.run_id ?? null,
      completed_cases: last.completed_cases ?? null,
      total_cases: last.total_cases ?? null,
      checkpoint: `apify/${ctx.stamp}/forge.json (private: contains source and outputs; do not share)`,
      resume: resume.command,
      resume_argv: resume.argv,
    },
    steps: res.log,
    note: reachable
      ? "Suite pages served from the portal; a pass is source creation, a pinned build and oracle-checked runs."
      : "Checked-in suite points at fixtures.genesis.example (unreachable): build evidence only. Pass --portal-suite for case runs.",
    ...(res.limit ? { stopped: `step limit ${ctx.maxSteps} reached; resume with genesis-forge step` } : {}),
    ...(res.error ? { error: res.error } : {}),
  };
}

// Benign, synthetic organization-local decision. A completed run alone does not
// validate this output and never establishes readiness or a created agent.
export const CREAO_WORKFLOW_CHECK = "genesis.role-workflow-check/1";
export function creaoWorkflowInputs() {
  return {
    task: "Assess selective adoption for GENESIS's receiving organization",
    instructions: "Use only the synthetic facts below. Do not browse, call tools, send messages, create agents, change resources, or execute code. The existing design-review role owns this responsibility. A candidate is eligible only when both local runs completed and it beats the control by at least one success. Return only a JSON object with schema, responsibility_id, owner_role_id, decision, candidate_successes and control_successes. The decision must reflect this local comparison, not the publisher's claim.",
    schema: CREAO_WORKFLOW_CHECK,
    responsibility: { id: "smoke-method-review", owner_role_id: "smoke-design-review" },
    offered_method: { id: "smoke-candidate", publisher_claim: "12 of 12 on a different task" },
    local_trial: { instances: 6, candidate: { completed: true, successes: 2 }, control: { completed: true, successes: 4 }, required_margin: 1 },
  };
}
export function validCreaoWorkflowResult(result) {
  let answer = result;
  if (result && typeof result === "object" && typeof result.text === "string") answer = JSON.parse(result.text);
  else if (typeof result === "string") answer = JSON.parse(result);
  return answer && typeof answer === "object" && !Array.isArray(answer)
    && answer.schema === CREAO_WORKFLOW_CHECK && answer.responsibility_id === "smoke-method-review"
    && answer.owner_role_id === "smoke-design-review" && answer.decision === "decline"
    && answer.candidate_successes === 2 && answer.control_successes === 4;
}
function durableDirectory(path) {
  mkdirSync(path, { recursive: true, mode: 0o700 });
  // Persist newly created directory entries before runTrigger's durable pre-POST record.
  for (let current = resolve(path); ; current = dirname(current)) {
    const fd = openSync(current, "r");
    try { fsyncSync(fd); } finally { closeSync(fd); }
    if (current === dirname(current)) break;
  }
}
export async function creaoGate(ctx) {
  if (!ctx.allowBillable) return { status: "not_tested", reason: "billable; rerun with --allow-billable" };
  const appId = ctx.env.CREAO_TRIGGER_APP_ID, key = ctx.env.CREAO_TRIGGER_API_KEY;
  let runsUrl;
  try {
    const version = ctx.env.CREAO_API_VERSION;
    if (version !== undefined && !(typeof version === "string" && (version.trim() === "" || version === "trigger"))) {
      throw new Error("CREAO_API_VERSION must be trigger; Developer API execution is disabled");
    }
    runsUrl = triggerRunsUrl(appId, ctx.env.CREAO_TRIGGER_RUNS_URL);
    if (typeof key !== "string" || !key.startsWith("capi_")) throw new Error("CREAO_TRIGGER_API_KEY must be a capi_ key");
  } catch (error) { return { status: "failed", stage: "configuration", reason: error.message }; }
  const work = join(ctx.out, "creao", ctx.stamp);
  durableDirectory(work);
  const journal = join(work, "trigger.json");
  const started = Date.now();
  const record = await runTrigger({ appId, key, runsUrl, inputs: creaoWorkflowInputs(), out: journal,
    allowCredits: true, fetchImpl: ctx.triggerFetchImpl ?? fetch, maxPolls: ctx.maxSteps,
    pollMs: 2000 * (ctx.pollScale ?? 1), checkId: CREAO_WORKFLOW_CHECK, checkResult: validCreaoWorkflowResult });
  const status = record.phase === "completed" ? (record.result_check?.passed === true ? "passed" : "failed")
    : record.phase === "needs_reconciliation" ? "needs_reconciliation"
    : ["failed", "cancelled"].includes(record.phase) ? "failed" : "partial";
  const resumeArgv = [process.execPath, join(ctx.repo, "scripts/live-gates/run.mjs"), "--gates", "creao", "--out", ctx.out,
    "--run-id", ctx.stamp, "--max-steps", String(ctx.maxSteps), "--allow-billable"];
  return {
    status, duration_ms: Date.now() - started,
    evidence: {
      api_version: "trigger", app_id: record.app_id, run_id: record.run_id, phase: record.phase,
      native_status: record.status, input_sha256: record.input_sha256, result_sha256: record.result_sha256,
      credits_used: record.credits_used, usage_reason: record.credits_used === null ? "not_reported_by_provider" : "reported_by_provider",
      workflow_check: record.result_check?.passed === true, readiness_verified: false,
      reason: record.reason ?? (record.phase === "completed" && !record.result_check?.passed ? "synthetic_workflow_check_failed" : null),
      checkpoint: journal, resume: resumeArgv.map(shellQuote).join(" "), resume_argv: resumeArgv,
    },
    note: "One credit-consuming run of an existing app-scoped CREAO agent. Passed means the synthetic local-decline workflow check matched; it does not establish a created agent, readiness, sponsor qualification or actual organizational adoption.",
    ...(record.reason ? { stopped: record.reason } : {}),
  };
}

const RUN = { elastic: elasticGate, fetch: fetchGate, apify: apifyGate, creao: creaoGate };

export async function runGates(opts) {
  const ctx = {
    repo: opts.repo,
    binDir: resolve(opts.binDir),
    out: resolve(opts.out),
    stamp: opts.stamp ?? new Date().toISOString().replace(/[-:.TZ]/g, "").slice(0, 14),
    env: opts.env ?? process.env,
    portalSuite: opts.portalSuite ? resolve(opts.portalSuite) : null,
    maxSteps: opts.maxSteps ?? 400,
    pollScale: opts.pollScale ?? 1,
    allowBillable: opts.allowBillable === true,
    triggerFetchImpl: opts.triggerFetchImpl,
  };
  if (!/^[A-Za-z0-9][A-Za-z0-9._-]{0,127}$/.test(ctx.stamp)) throw new Error("run ID must be a plain name of at most 128 characters");
  mkdirSync(ctx.out, { recursive: true });
  const results = {};
  for (const gate of opts.gates) {
    const missing = missingEnv(gate, ctx.env);
    const started = now();
    let result;
    if (missing.length) result = { status: "not_tested", reason: `missing ${missing.join(", ")}` };
    else if (BILLABLE.has(gate) && !opts.allowBillable) result = { status: "not_tested", reason: "billable; rerun with --allow-billable" };
    else {
      try {
        result = await RUN[gate](ctx);
      } catch (e) {
        result = { status: "failed", error: String(e?.message ?? e) };
      }
    }
    results[gate] = scrub({ gate, started_at: started, finished_at: now(), ...result }, ctx.env);
  }
  const report = { schema: "genesis-live-gates/1", run: ctx.stamp, results };
  writeFileSync(join(ctx.out, `gates-${ctx.stamp}.json`), `${JSON.stringify(report, null, 2)}\n`);
  writeFileSync(join(ctx.out, `gates-${ctx.stamp}.md`), summary(report));
  return report;
}

export function summary(report) {
  const rows = Object.values(report.results).map((r) => {
    const ids = Object.entries(r.evidence ?? {})
      .filter(([k, v]) => v !== null && v !== undefined && !["steps", "checkpoint", "readiness_verified", "resume"].includes(k) && !Array.isArray(v))
      .map(([k, v]) => `${k}=\`${v}\``)
      .join(", ");
    const secs = r.duration_ms !== undefined ? `${(r.duration_ms / 1000).toFixed(1)} s` : "—";
    return `| ${r.gate} | **${r.status}** | ${secs} | ${ids || r.reason || r.error || "—"} |`;
  });
  return `# Live gate run ${report.run}\n\n| Gate | Status | Duration | Evidence / reason |\n| --- | --- | --- | --- |\n${rows.join("\n")}\n\n${Object.values(report.results)
    .filter((r) => r.note)
    .map((r) => `- **${r.gate}:** ${r.note}`)
    .join("\n")}\n`;
}

async function main() {
  const { values } = parseArgs({
    options: {
      gates: { type: "string", default: GATES.join(",") },
      out: { type: "string", default: "data/live-gates" },
      "bin-dir": { type: "string", default: "target/debug" },
      "portal-suite": { type: "string" },
      "allow-billable": { type: "boolean" },
      "max-steps": { type: "string" },
      "run-id": { type: "string" },
    },
  });
  const gates = values.gates.split(",").map((g) => g.trim()).filter(Boolean);
  const unknown = gates.filter((g) => !GATES.includes(g));
  if (unknown.length) {
    console.error(`unknown gate(s): ${unknown.join(", ")}; use ${GATES.join(", ")}`);
    process.exit(2);
  }
  const report = await runGates({
    repo: fileURLToPath(new URL("../..", import.meta.url)),
    binDir: values["bin-dir"],
    out: values.out,
    gates,
    portalSuite: values["portal-suite"],
    allowBillable: values["allow-billable"],
    stamp: values["run-id"],
    maxSteps: values["max-steps"] ? Number(values["max-steps"]) : undefined,
  });
  process.stdout.write(summary(report));
  const bad = Object.values(report.results).some((r) => ["failed", "needs_reconciliation"].includes(r.status));
  process.exit(bad ? 1 : 0);
}

if (process.argv[1] && existsSync(process.argv[1])
  && fileURLToPath(import.meta.url) === realpathSync(process.argv[1])) main();
