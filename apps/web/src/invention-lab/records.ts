// Eric's Claude: strict decoders for the retained invention-lab records this
// view reads (docs/invention-lab/ui.md). Shapes mirror the Rust serializers;
// enum-valued fields stay strings here and are labeled in claims.ts, so an
// unexpected value is shown as unrecognized rather than guessed. Decoders never
// read generated text, prompts or request bodies.

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

function decodeVerdict(v: unknown, at: string): Verdict {
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
