// Eric's Claude: strict decoders for the retained invention-lab records this
// view reads (docs/invention-lab/ui.md). Shapes mirror the Rust serializers;
// enum-valued fields stay strings here and are labeled in claims.ts, so an
// unexpected value is shown as unrecognized rather than guessed. Decoders never
// read generated text, prompts or request bodies.

import { canonicalSha256, sha256Hex } from "./digest";

export class DecodeError extends Error {}

type Obj = Record<string, unknown>;

function obj(v: unknown, at: string): Obj {
  if (typeof v !== "object" || v === null || Array.isArray(v)) throw new DecodeError(`${at}: expected object`);
  return v as Obj;
}
function str(o: Obj, k: string, at: string): string {
  const v = o[k];
  if (typeof v !== "string") throw new DecodeError(`${at}.${k}: expected string`);
  return v;
}
function optStr(o: Obj, k: string, at: string): string | null {
  return o[k] === null || o[k] === undefined ? null : str(o, k, at);
}
function nat(o: Obj, k: string, at: string): number {
  const v = o[k];
  if (typeof v !== "number" || !Number.isSafeInteger(v) || v < 0) throw new DecodeError(`${at}.${k}: expected count`);
  return v;
}

/** `ExactContentRef`: identity is all five fields, not id/version alone. */
export interface ExactRef {
  domain: string;
  schema: string;
  id: string;
  version: string;
  sha256: string;
}
export function decodeRef(v: unknown, at = "ref"): ExactRef {
  const o = obj(v, at);
  return {
    domain: str(o, "domain", at),
    schema: str(o, "schema", at),
    id: str(o, "id", at),
    version: str(o, "version", at),
    sha256: str(o, "sha256", at),
  };
}
export function refKey(r: ExactRef): string {
  return [r.domain, r.schema, r.id, r.version, r.sha256].join("\u0000");
}

export const USAGE_DIMENSIONS = ["model_calls", "input_tokens", "output_tokens", "wall_time_ms", "simulation_steps"] as const;
export type UsageDimension = (typeof USAGE_DIMENSIONS)[number];

/** `ResourceUsage`, plus `not_recorded` for a dimension the record omits. */
export type Usage =
  | { kind: "measured"; value: number }
  | { kind: "unknown"; reason: string }
  | { kind: "not_recorded" }
  | { kind: "unrecognized"; raw: string };

function decodeUsage(v: unknown, at: string): Usage {
  const o = obj(v, at);
  const kind = str(o, "kind", at);
  if (kind === "measured") return { kind, value: nat(o, "value", at) };
  if (kind === "unknown") return { kind, reason: str(o, "reason", at) };
  return { kind: "unrecognized", raw: kind };
}
export function decodeUsageRecord(v: unknown, at = "usage"): Record<UsageDimension, Usage> {
  const o = obj(v, at);
  const out = {} as Record<UsageDimension, Usage>;
  for (const d of USAGE_DIMENSIONS) out[d] = d in o ? decodeUsage(o[d], `${at}.${d}`) : { kind: "not_recorded" };
  return out;
}

export interface GenerationSettings {
  model: string;
  max_tokens: number;
  timeout_ms: number;
  sampling: string;
}

/** Output of `genesis-forge method-generate-status`. */
export interface GenerationStatus {
  operation_id: string;
  org_id: string;
  phase: string;
  settings: GenerationSettings;
  plan_sha256: string;
  request_sha256: string | null;
  wire_sha256: string | null;
  transport: string | null;
  actual_model: string | null;
  usage: Record<UsageDimension, Usage> | null;
  package: ExactRef | null;
  authority: string;
  /** Optional enum-only protocol rejection diagnostic (#90); absent in older records. */
  diagnostic: { rejection: string; stop_reason: string | null } | null;
  /** Top-level fields this decoder does not know, listed rather than dropped. */
  unrecognized_fields: string[];
}
const STATUS_FIELDS = new Set([
  "operation_id", "org_id", "phase", "settings", "plan_sha256", "request_sha256", "wire_sha256",
  "transport", "actual_model", "usage", "package", "authority", "diagnostic",
]);
export function decodeGenerationStatus(v: unknown): GenerationStatus {
  const at = "status";
  const o = obj(v, at);
  const s = obj(o.settings, `${at}.settings`);
  let diagnostic: GenerationStatus["diagnostic"] = null;
  if (o.diagnostic != null) {
    const d = obj(o.diagnostic, `${at}.diagnostic`);
    diagnostic = { rejection: str(d, "rejection", `${at}.diagnostic`), stop_reason: optStr(d, "stop_reason", `${at}.diagnostic`) };
  }
  return {
    diagnostic,
    unrecognized_fields: Object.keys(o).filter((k) => !STATUS_FIELDS.has(k)).sort(),
    operation_id: str(o, "operation_id", at),
    org_id: str(o, "org_id", at),
    phase: str(o, "phase", at),
    settings: {
      model: str(s, "model", `${at}.settings`),
      max_tokens: nat(s, "max_tokens", `${at}.settings`),
      timeout_ms: nat(s, "timeout_ms", `${at}.settings`),
      sampling: str(s, "sampling", `${at}.settings`),
    },
    plan_sha256: str(o, "plan_sha256", at),
    request_sha256: optStr(o, "request_sha256", at),
    wire_sha256: optStr(o, "wire_sha256", at),
    transport: optStr(o, "transport", at),
    actual_model: optStr(o, "actual_model", at),
    usage: o.usage == null ? null : decodeUsageRecord(o.usage, `${at}.usage`),
    package: o.package == null ? null : decodeRef(o.package, `${at}.package`),
    authority: str(o, "authority", at),
  };
}

/** Only the enum outcome of a generation checkpoint; returned text is skipped. */
export interface CheckpointOutcome {
  org_id: string;
  operation_id: string;
  phase: string;
  request_sha256: string;
  outcome: { kind: string; reason: string | null } | null;
}
export function decodeCheckpointOutcome(v: unknown): CheckpointOutcome {
  const at = "checkpoint";
  const o = obj(v, at);
  const plan = obj(o.plan, `${at}.plan`);
  let outcome: CheckpointOutcome["outcome"] = null;
  if (o.completion != null) {
    const c = obj(obj(o.completion, `${at}.completion`).outcome, `${at}.completion.outcome`);
    const kind = str(c, "kind", `${at}.completion.outcome`);
    // `returned` carries model text; never copy it.
    outcome = { kind, reason: kind === "returned" ? null : optStr(c, "reason", `${at}.completion.outcome`) };
  }
  return {
    org_id: str(plan, "org_id", `${at}.plan`),
    operation_id: str(plan, "operation_id", `${at}.plan`),
    phase: str(o, "phase", at),
    request_sha256: str(o, "request_sha256", at),
    outcome,
  };
}

/** `genesis-method-revision-observation/1` from `method-revise-observe`. */
export interface RevisionObservation {
  parent: ExactRef;
  child: ExactRef;
  org_id: string;
  operation_id: string;
  transport: string;
  source_changed: boolean;
}
export const REVISION_SCHEMA = "genesis-method-revision-observation/1";
export function decodeRevisionObservation(v: unknown): RevisionObservation {
  const at = "revision";
  const o = obj(v, at);
  if (o.schema !== REVISION_SCHEMA) throw new DecodeError(`${at}.schema: expected ${REVISION_SCHEMA}`);
  if (typeof o.source_changed !== "boolean") throw new DecodeError(`${at}.source_changed: expected boolean`);
  return {
    parent: decodeRef(o.parent, `${at}.parent`),
    child: decodeRef(o.child, `${at}.child`),
    org_id: str(o, "org_id", at),
    operation_id: str(o, "operation_id", at),
    transport: str(o, "transport", at),
    source_changed: o.source_changed,
  };
}

/** Scoring `Verdict` (tag `verdict`). */
export type Verdict =
  | { verdict: "accepted"; candidate: number; control: number; of: number }
  | { verdict: string; reason: string | null };

export function decodeVerdict(v: unknown, at = "verdict"): Verdict {
  const o = obj(v, at);
  const verdict = str(o, "verdict", at);
  if (verdict === "accepted")
    return { verdict, candidate: nat(o, "candidate", at), control: nat(o, "control", at), of: nat(o, "of", at) };
  return { verdict, reason: optStr(o, "reason", at) };
}

/** Adoption `Outcome` (tag `outcome`) from `lab-adopt conclude[-selected]`. */
export interface AdoptionOutcome {
  outcome: string;
  reason: string | null;
  /** Present only for `adopted`: the installation's own org and method. */
  installed: { org_id: string; method: ExactRef; verdict: Verdict } | null;
}
export function decodeAdoptionOutcome(v: unknown): AdoptionOutcome {
  const at = "adoption";
  const o = obj(v, at);
  const outcome = str(o, "outcome", at);
  if (outcome === "adopted") {
    const i = obj(o.installation, `${at}.installation`);
    return {
      outcome,
      reason: null,
      installed: {
        org_id: str(i, "org_id", `${at}.installation`),
        method: decodeRef(i.method, `${at}.installation.method`),
        verdict: decodeVerdict(i.verdict, `${at}.installation.verdict`),
      },
    };
  }
  return { outcome, reason: optStr(o, "reason", at), installed: null };
}

/** `ExecutionPurpose` (tag `kind`); an unexpected kind is kept verbatim. */
export type Purpose =
  | { kind: "ordinary" }
  | { kind: "acceptance"; plan: ExactRef; role: string }
  | { kind: "comparison"; plan: ExactRef; trial_id: string; arm_id: string }
  | { kind: "unrecognized"; raw: string };

function decodePurpose(v: unknown, at: string): Purpose {
  const o = obj(v, at);
  const kind = str(o, "kind", at);
  if (kind === "ordinary") return { kind };
  if (kind === "acceptance") return { kind, plan: decodeRef(o.plan, `${at}.plan`), role: str(o, "role", at) };
  if (kind === "comparison")
    return { kind, plan: decodeRef(o.plan, `${at}.plan`), trial_id: str(o, "trial_id", at), arm_id: str(o, "arm_id", at) };
  return { kind: "unrecognized", raw: kind };
}
export function samePurpose(a: Purpose, b: Purpose): boolean {
  if (a.kind !== b.kind) return false;
  if (a.kind === "acceptance" && b.kind === "acceptance") return a.role === b.role && refKey(a.plan) === refKey(b.plan);
  if (a.kind === "comparison" && b.kind === "comparison")
    return a.trial_id === b.trial_id && a.arm_id === b.arm_id && refKey(a.plan) === refKey(b.plan);
  return a.kind === "ordinary";
}

function refs(v: unknown, at: string): ExactRef[] {
  if (!Array.isArray(v)) throw new DecodeError(`${at}: expected array`);
  return v.map((r, i) => decodeRef(r, `${at}[${i}]`));
}

/** `ExecutionReceipt` written by the lab executor (genesis-core development_method). */
export interface ExecutionReceipt {
  org_id: string;
  run_id: string;
  attempt_id: string;
  /** The method the frozen binding named. */
  bound_method: ExactRef;
  /** The method the executor reports it actually ran. */
  actual_method: ExactRef;
  actual_environment: ExactRef;
  /** The task family the binding names (e.g. `ravine-crossing`). */
  public_task: ExactRef;
  purpose: Purpose;
  outcome: string;
  outputs: ExactRef[];
  usage: Record<UsageDimension, Usage>;
  /** genesis-core `execution_receipt_identity_hash`: canonical SHA-256 of the
   * whole retained receipt under its domain. Evaluations name this value. */
  receipt_sha256: string;
}
export const EXECUTION_RECEIPT_DOMAIN = "genesis.development_execution_receipt";
export function decodeExecutionReceipt(v: unknown): ExecutionReceipt {
  const at = "execution";
  const o = obj(v, at);
  const b = obj(o.binding, `${at}.binding`);
  return {
    receipt_sha256: canonicalSha256({ domain: EXECUTION_RECEIPT_DOMAIN, version: 1, record: o }),
    org_id: str(b, "org_id", `${at}.binding`),
    run_id: str(b, "run_id", `${at}.binding`),
    attempt_id: str(b, "attempt_id", `${at}.binding`),
    bound_method: decodeRef(b.method, `${at}.binding.method`),
    actual_method: decodeRef(o.actual_method, `${at}.actual_method`),
    actual_environment: decodeRef(o.actual_environment, `${at}.actual_environment`),
    public_task: decodeRef(b.public_task, `${at}.binding.public_task`),
    purpose: decodePurpose(b.purpose, `${at}.binding.purpose`),
    outcome: str(o, "outcome", at),
    outputs: refs(o.outputs, `${at}.outputs`),
    usage: decodeUsageRecord(o.usage, `${at}.usage`),
  };
}

/** `EvaluationReceipt` written by the trusted scorer (genesis-core development_evaluation). */
export interface EvaluationReceipt {
  request_id: string;
  execution_sha256: string;
  output: ExactRef;
  purpose: Purpose;
  outcome: string;
  metrics: ExactRef | null;
  reason: string | null;
  usage: Record<UsageDimension, Usage>;
  /** The scoring context the request named (task, rule, instance set, ...). */
  context: ScoringContext;
  /** The context the scorer reports it used; absent unless it scored or judged
   * the output invalid. */
  observed_context: ScoringContext | null;
}
/** A scoring context: named exact references (`public_task`, `scoring_rule`, ...). */
export type ScoringContext = Record<string, ExactRef>;
function decodeContext(v: unknown, at: string): ScoringContext {
  const o = obj(v, at);
  return Object.fromEntries(Object.keys(o).map((k) => [k, decodeRef(o[k], `${at}.${k}`)]));
}
export function sameContext(a: ScoringContext, b: ScoringContext): boolean {
  const ka = Object.keys(a).sort();
  const kb = Object.keys(b).sort();
  return ka.length === kb.length && ka.every((k, i) => k === kb[i] && refKey(a[k]!) === refKey(b[k]!));
}
export function decodeEvaluationReceipt(v: unknown): EvaluationReceipt {
  const at = "evaluation";
  const o = obj(v, at);
  const r = obj(o.request, `${at}.request`);
  const out = obj(o.outcome, `${at}.outcome`);
  const kind = str(out, "kind", `${at}.outcome`);
  return {
    request_id: str(r, "request_id", `${at}.request`),
    execution_sha256: str(r, "execution_sha256", `${at}.request`),
    output: decodeRef(r.output, `${at}.request.output`),
    purpose: decodePurpose(r.purpose, `${at}.request.purpose`),
    context: decodeContext(r.context, `${at}.request.context`),
    observed_context:
      o.observed_context === null || o.observed_context === undefined
        ? null
        : decodeContext(o.observed_context, `${at}.observed_context`),
    outcome: kind,
    metrics: kind === "scored" ? decodeRef(out.metrics, `${at}.outcome.metrics`) : null,
    reason: kind === "scored" ? null : optStr(out, "reason", `${at}.outcome`),
    usage: decodeUsageRecord(o.usage, `${at}.usage`),
  };
}

/** An exported canonical method package file (`genesis.development_method_source`).
 * Its identity is the SHA-256 of the exact file bytes, computed here; source text
 * is measured, never retained in the view. */
export interface PackageFile {
  ref: ExactRef;
  entrypoint: string;
  runtime: ExactRef;
  protocol: ExactRef;
  dependencies: ExactRef[];
  files: { path: string; bytes: number; lines: number; sha256: string }[];
}
export const PACKAGE_DOMAIN = "genesis.development_method_source";
export function decodePackageFile(bytes: Uint8Array): PackageFile {
  const at = "package";
  const o = obj(JSON.parse(new TextDecoder("utf-8", { fatal: true }).decode(bytes)), at);
  if (o.domain !== PACKAGE_DOMAIN) throw new DecodeError(`${at}.domain: expected ${PACKAGE_DOMAIN}`);
  // Only the envelope version this view understands; anything else is not
  // labelled as a checked package.
  if (o.version !== 1) throw new DecodeError(`${at}.version: expected 1`);
  const p = obj(o.package, `${at}.package`);
  if (!Array.isArray(p.files)) throw new DecodeError(`${at}.package.files: expected array`);
  const enc = new TextEncoder();
  const files = p.files.map((f, i) => {
    const fo = obj(f, `${at}.package.files[${i}]`);
    const text = str(fo, "content", `${at}.package.files[${i}]`);
    const content = enc.encode(text);
    return {
      path: str(fo, "path", `${at}.package.files[${i}]`),
      bytes: content.length,
      lines: text === "" ? 0 : text.split("\n").length - (text.endsWith("\n") ? 1 : 0),
      sha256: sha256Hex(content),
    };
  });
  return {
    ref: {
      domain: "genesis",
      schema: "development-method-source/1",
      id: str(p, "id", `${at}.package`),
      version: str(p, "version", `${at}.package`),
      sha256: sha256Hex(bytes),
    },
    entrypoint: str(p, "entrypoint", `${at}.package`),
    runtime: decodeRef(p.runtime, `${at}.package.runtime`),
    protocol: decodeRef(p.protocol, `${at}.package.protocol`),
    dependencies: refs(p.dependencies, `${at}.package.dependencies`),
    files,
  };
}

/** `genesis-static-source-inspection/1`: syntax and observed imports only.
 * Two retained shapes use this schema name: one names `method_package_sha256`,
 * `entrypoint`, `python_ast_parse`, `execution` and `claim`; the other names a
 * `package` ref, `ast_parse`, `source_executed` and `authority`. Both are kept
 * verbatim as "key value" phrases rather than translated into one vocabulary. */
export interface StaticInspection {
  method_package_sha256: string;
  /** The full package reference when the record names one; it must match exactly. */
  package_ref: ExactRef | null;
  /** Null when the record does not name its entrypoint. */
  entrypoint: string | null;
  source_sha256: string;
  parse: string;
  imports: string[];
  execution: string;
  claim: string;
}
export const INSPECTION_SCHEMA = "genesis-static-source-inspection/1";
function flag(o: Obj, k: string, at: string): string {
  if (typeof o[k] !== "boolean") throw new DecodeError(`${at}.${k}: expected boolean`);
  return `${k} ${String(o[k])}`;
}
export function decodeStaticInspection(v: unknown): StaticInspection {
  const at = "inspection";
  const o = obj(v, at);
  if (o.schema !== INSPECTION_SCHEMA) throw new DecodeError(`${at}.schema: expected ${INSPECTION_SCHEMA}`);
  if (!Array.isArray(o.imports) || !o.imports.every((x) => typeof x === "string"))
    throw new DecodeError(`${at}.imports: expected strings`);
  const imports = o.imports as string[];
  if ("method_package_sha256" in o)
    return {
      method_package_sha256: str(o, "method_package_sha256", at),
      package_ref: null,
      entrypoint: str(o, "entrypoint", at),
      source_sha256: str(o, "source_sha256", at),
      parse: `python_ast_parse ${str(o, "python_ast_parse", at)}`,
      imports,
      execution: `execution ${str(o, "execution", at)}`,
      claim: str(o, "claim", at),
    };
  const package_ref = decodeRef(o.package, `${at}.package`);
  return {
    method_package_sha256: package_ref.sha256,
    package_ref,
    entrypoint: null,
    source_sha256: str(o, "source_sha256", at),
    parse: flag(o, "ast_parse", at),
    imports,
    execution: flag(o, "source_executed", at),
    claim: str(o, "authority", at),
  };
}

/** A trial plan (genesis-core development_trial). Its reference is computed as
 * the canonical SHA-256 under `genesis.development_trial_plan`, the value that
 * execution receipts name in `purpose.plan`. */
export interface TrialPlan {
  ref: ExactRef;
  criterion: ExactRef;
  /** `scoring_instances.kind`: `development` (the public development panel) or
   * `hidden_commitment` (a hidden panel revealed only to the scorer). */
  scoring_kind: string;
  /** Ordered public development instance ids, as the plan lists them. */
  development_instances: string[];
}
export const TRIAL_PLAN_FIELDS = [
  "id", "version", "criterion", "development_instances", "scoring_instances",
  "context", "model_settings", "development_environment", "slots",
] as const;
export function decodeTrialPlan(v: unknown): TrialPlan {
  const at = "plan";
  const o = obj(v, at);
  return {
    ref: {
      domain: "genesis",
      schema: "development-trial-plan/1",
      id: str(o, "id", at),
      version: str(o, "version", at),
      sha256: canonicalSha256({ domain: "genesis.development_trial_plan", version: 1, plan: o }),
    },
    criterion: decodeRef(o.criterion, `${at}.criterion`),
    scoring_kind: str(obj(o.scoring_instances, `${at}.scoring_instances`), "kind", `${at}.scoring_instances`),
    development_instances: ((): string[] => {
      if (!Array.isArray(o.development_instances)) throw new DecodeError(`${at}.development_instances: expected array`);
      return o.development_instances.map((d, i) => str(obj(d, `${at}.development_instances[${i}]`), "id", `${at}.development_instances[${i}]`));
    })(),
  };
}

/** The only metrics identity this view counts: the scorer's `lab-scoring-metrics/1`
 * artifact. Any other domain, schema, id or version is unsupported, even when
 * its digest names bytes this view could read. */
export const METRICS_IDENTITY = {
  domain: "genesis-lab",
  schema: "lab-scoring-metrics/1",
  id: "lab-scoring-metrics/1",
  version: "1",
} as const;
export function isSupportedMetricsRef(r: ExactRef): boolean {
  return (
    r.domain === METRICS_IDENTITY.domain &&
    r.schema === METRICS_IDENTITY.schema &&
    r.id === METRICS_IDENTITY.id &&
    r.version === METRICS_IDENTITY.version
  );
}

/** A scoring metrics artifact (`lab-scoring-metrics/1`), identified by the
 * SHA-256 of its exact bytes, which a scored evaluation names. Each row is one
 * scored instance; only the recorded boolean `success` is counted. `of` and
 * `success_count` are the scorer's own totals, kept to check the rows. */
export interface MetricsArtifact {
  sha256: string;
  instances: { instance: string; success: boolean }[];
  of: number | null;
  success_count: number | null;
}
export function isMetricsShape(o: Obj): boolean {
  return (
    Array.isArray(o.instances) &&
    o.instances.length > 0 &&
    o.instances.every((r) => typeof r === "object" && r !== null && "instance" in r && "success" in r)
  );
}
export function decodeMetricsArtifact(bytes: Uint8Array): MetricsArtifact {
  const at = "metrics";
  const o = obj(JSON.parse(new TextDecoder("utf-8", { fatal: true }).decode(bytes)), at);
  if (!Array.isArray(o.instances) || o.instances.length === 0)
    throw new DecodeError(`${at}.instances: expected a non-empty array`);
  return {
    sha256: sha256Hex(bytes),
    instances: o.instances.map((r, i) => {
      const row = obj(r, `${at}.instances[${i}]`);
      if (typeof row.success !== "boolean") throw new DecodeError(`${at}.instances[${i}].success: expected boolean`);
      return { instance: str(row, "instance", `${at}.instances[${i}]`), success: row.success };
    }),
    of: o.of === undefined ? null : nat(o, "of", at),
    success_count: o.success_count === undefined ? null : nat(o, "success_count", at),
  };
}
