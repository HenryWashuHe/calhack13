// Eric's Claude: arrange decoded records into attempts, exact method nodes and
// typed edges (docs/invention-lab/ui.md). Pure and read-only. Inconsistent
// records are reported as problems, never silently reconciled.

import { tierOf, type Tier } from "./claims";
import {
  isSupportedMetricsRef,
  METRICS_IDENTITY,
  refKey,
  sameContext,
  samePurpose,
  type AdoptionOutcome,
  type CheckpointOutcome,
  type EvaluationReceipt,
  type ExactRef,
  type ExecutionReceipt,
  type GenerationStatus,
  type MetricsArtifact,
  type PackageFile,
  type RevisionObservation,
  type StaticInspection,
  type TrialPlan,
  type Verdict,
} from "./records";

export interface AdoptionInput {
  /** From the receiver's own `receiver.json`. */
  receiver_org: string;
  /** From the `lab-adopt precheck` output's `offer`. */
  offer: ExactRef;
  outcome: AdoptionOutcome;
}

export interface LabRecords {
  generations: { status: GenerationStatus; checkpoint?: CheckpointOutcome }[];
  revisions?: RevisionObservation[];
  adoptions?: AdoptionInput[];
  executions?: ExecutionReceipt[];
  evaluations?: EvaluationReceipt[];
  /** Exported package files; identity computed from their exact bytes. */
  packages?: PackageFile[];
  inspections?: StaticInspection[];
  trials?: TrialInput[];
  /** Scoring metrics artifacts, matched to scored evaluations by exact hash. */
  metrics?: MetricsArtifact[];
}

export interface TrialInput {
  plan: TrialPlan;
  /** A standalone verdict names no plan; the host pairs it (e.g. same folder). */
  verdict: Verdict | null;
}

export interface Trial extends TrialInput {
  /** Runs whose purpose names this exact plan. */
  executions: Execution[];
}

export interface Execution {
  receipt: ExecutionReceipt;
  /** Node key of the method the executor reports it ran. */
  method: string;
  /** Evaluations whose request names this run's receipt hash (`execution_sha256`),
   * with an output among this run's outputs and the same purpose. */
  evaluations: EvaluationReceipt[];
  /** The run's success count, resolved once from its evaluations (see `scoreOf`). */
  score: Score;
}

/** What the supplied records establish about one run's score. Only `counted`
 * carries a number; every other kind says why there is none. */
export type Score =
  | { kind: "counted"; successes: number; of: number; metrics: string }
  /** No evaluation of this run was supplied. */
  | { kind: "unscored" }
  /** Evaluations were supplied, but none scored the output (unknown, failed, invalid). */
  | { kind: "not_scored"; outcomes: string[] }
  /** A scored evaluation names a metrics artifact that was not supplied. */
  | { kind: "metrics_missing" }
  /** Every scored evaluation was refused; the reasons are also in `problems`. */
  | { kind: "rejected"; reasons: string[] }
  /** Scored evaluations of the same run disagree; none is chosen. */
  | { kind: "conflict"; counts: string[] };

export interface Attempt {
  status: GenerationStatus;
  /** Enum outcome from the matching checkpoint, if one was supplied and matches. */
  outcome: { kind: string; reason: string | null } | null;
  tier: Tier;
  /** Measured output equals the declared cap. Not proof of truncation. */
  reachedOutputCap: boolean;
}

export interface MethodNode {
  key: string;
  ref: ExactRef;
  /** A supplied package file whose computed hash is exactly this ref. */
  package?: PackageFile;
  inspections: StaticInspection[];
}

export type Edge =
  | { kind: "generated"; org_id: string; operation_id: string; to: string }
  | {
      kind: "revised_from";
      org_id: string;
      operation_id: string;
      /** The observation's exact generation request, when recorded. */
      request_sha256: string | null;
      from: string;
      to: string;
      source_changed: boolean;
    }
  | {
      kind: "adoption";
      org_id: string;
      to: string;
      outcome: AdoptionOutcome;
      /** `record`: the installation names org and method itself. `host`: the
       * outcome alone does not, so the pairing was supplied alongside it. */
      pairing: "record" | "host";
    };

export interface Lineage {
  attempts: Attempt[];
  nodes: MethodNode[];
  edges: Edge[];
  executions: Execution[];
  trials: Trial[];
  /** Metrics artifacts by their byte hash. */
  metrics: Map<string, MetricsArtifact>;
  problems: string[];
}

export function buildLineage(records: LabRecords): Lineage {
  const nodes = new Map<string, MethodNode>();
  const edges: Edge[] = [];
  const problems: string[] = [];
  const node = (ref: ExactRef): string => {
    const key = refKey(ref);
    if (!nodes.has(key)) nodes.set(key, { key, ref, inspections: [] });
    return key;
  };

  // An operation is identified by its organization and operation id together:
  // two organizations may reuse an operation name.
  const opKey = (org: string, op: string) => `${org}\u0000${op}`;
  const seen = new Set<string>();
  const attempts = records.generations.map(({ status, checkpoint }): Attempt => {
    const op = status.operation_id;
    const key = opKey(status.org_id, op);
    if (seen.has(key)) problems.push(`operation ${op} of ${status.org_id} appears more than once`);
    seen.add(key);
    let outcome: Attempt["outcome"] = null;
    if (checkpoint) {
      if (
        checkpoint.org_id !== status.org_id ||
        checkpoint.operation_id !== op ||
        checkpoint.request_sha256 !== status.request_sha256
      ) {
        problems.push(`checkpoint does not match status for operation ${op}; its outcome is not shown`);
      } else if (checkpoint.phase !== status.phase) {
        problems.push(`checkpoint phase ${checkpoint.phase} differs from status phase ${status.phase} for ${op}`);
      } else {
        outcome = checkpoint.outcome;
      }
    }
    if (status.package) {
      if (status.phase === "generated")
        edges.push({ kind: "generated", org_id: status.org_id, operation_id: op, to: node(status.package) });
      else problems.push(`operation ${op} names a package in phase ${status.phase}`);
    }
    const out = status.usage?.output_tokens;
    return {
      status,
      outcome,
      tier: tierOf(status.transport),
      reachedOutputCap: out?.kind === "measured" && out.value === status.settings.max_tokens,
    };
  });

  for (const r of records.revisions ?? []) {
    const gen = attempts.find((a) => a.status.org_id === r.org_id && a.status.operation_id === r.operation_id);
    if (gen?.status.package && refKey(gen.status.package) !== refKey(r.child))
      problems.push(`revision child differs from the package of operation ${r.operation_id} of ${r.org_id}`);
    if (gen && r.request_sha256 !== null && gen.status.request_sha256 !== r.request_sha256)
      problems.push(
        `revision observation names request ${r.request_sha256.slice(0, 12)}, but the status of operation ${r.operation_id} of ${r.org_id} records ${gen.status.request_sha256?.slice(0, 12) ?? "none"}; its cost is not attributed`,
      );
    edges.push({
      kind: "revised_from",
      org_id: r.org_id,
      operation_id: r.operation_id,
      request_sha256: r.request_sha256,
      from: node(r.parent),
      to: node(r.child),
      source_changed: r.source_changed,
    });
  }

  for (const a of records.adoptions ?? []) {
    const installed = a.outcome.installed;
    if (installed) {
      if (installed.org_id !== a.receiver_org || refKey(installed.method) !== refKey(a.offer))
        problems.push(`installation for ${installed.org_id} does not match the supplied receiver/offer; showing the installation`);
      edges.push({ kind: "adoption", org_id: installed.org_id, to: node(installed.method), outcome: a.outcome, pairing: "record" });
    } else {
      edges.push({ kind: "adoption", org_id: a.receiver_org, to: node(a.offer), outcome: a.outcome, pairing: "host" });
    }
  }

  // A run's identity is its whole receipt: trials may reuse run and attempt ids
  // under different plans, so only an identical receipt is a duplicate.
  const executions: Execution[] = [];
  const runs = new Set<string>();
  for (const receipt of records.executions ?? []) {
    const run = `${receipt.org_id}/${receipt.run_id}/${receipt.attempt_id}`;
    if (runs.has(receipt.receipt_sha256)) problems.push(`execution ${run} appears more than once`);
    runs.add(receipt.receipt_sha256);
    if (refKey(receipt.bound_method) !== refKey(receipt.actual_method))
      problems.push(`execution ${run} ran a different method than its binding named; shown under the method it ran`);
    executions.push({ receipt, method: node(receipt.actual_method), evaluations: [], score: { kind: "unscored" } });
  }
  // A task family is an exact reference; one id under two references is two
  // columns, never one.
  const taskRefs = new Map<string, Set<string>>();
  for (const e of executions) {
    const t = e.receipt.public_task;
    taskRefs.set(t.id, (taskRefs.get(t.id) ?? new Set()).add(refKey(t)));
  }
  for (const [id, keys] of taskRefs)
    if (keys.size > 1) problems.push(`task family ${id} appears under ${keys.size} different exact references; shown as separate columns`);
  // An evaluation attaches only to the execution its request names by receipt
  // hash; matching output and purpose alone could be another organization's run.
  for (const ev of records.evaluations ?? []) {
    const named = executions.filter((e) => e.receipt.receipt_sha256 === ev.execution_sha256);
    const run = named[0];
    if (!run) {
      problems.push(`evaluation ${ev.request_id} names execution ${ev.execution_sha256.slice(0, 12)}, which no supplied receipt has; not attached`);
    } else if (!run.receipt.outputs.some((o) => refKey(o) === refKey(ev.output))) {
      problems.push(`evaluation ${ev.request_id} scores an output its named execution did not produce; not attached`);
    } else if (!samePurpose(run.receipt.purpose, ev.purpose)) {
      problems.push(`evaluation ${ev.request_id} has a different purpose than its named execution; not attached`);
    } else {
      run.evaluations.push(ev);
    }
  }

  for (const p of records.packages ?? []) {
    const n = nodes.get(refKey(p.ref));
    if (n) {
      n.package = p;
      continue;
    }
    const same = [...nodes.values()].find((x) => x.ref.id === p.ref.id && x.ref.version === p.ref.version);
    if (same)
      problems.push(
        `package file for ${p.ref.id}@${p.ref.version} hashes to ${p.ref.sha256.slice(0, 12)}, not the recorded ${same.ref.sha256.slice(0, 12)}`,
      );
    // A record may copy a real hash under another identity; the bytes decide.
    for (const other of nodes.values())
      if (other.ref.sha256 === p.ref.sha256)
        problems.push(
          `a record names package ${p.ref.sha256.slice(0, 12)} as ${other.ref.id}@${other.ref.version}; its bytes identify ${p.ref.id}@${p.ref.version}`,
        );
    nodes.get(node(p.ref))!.package = p;
  }
  for (const i of records.inspections ?? []) {
    // Prefer the version whose supplied bytes have this hash; a hash alone may
    // be shared by a contradictory reference.
    const sharing = [...nodes.values()].filter((x) => x.ref.sha256 === i.method_package_sha256);
    if (i.package_ref) {
      // An explicit full reference must match exactly; a shared digest under
      // another identity is reported, never silently attributed.
      const exact = nodes.get(refKey(i.package_ref));
      if (!exact) {
        problems.push(
          sharing.length === 0
            ? `static inspection names package ${i.method_package_sha256.slice(0, 12)}, which no supplied record contains`
            : `static inspection names ${i.package_ref.id}@${i.package_ref.version} ${i.method_package_sha256.slice(0, 12)}, but the supplied records with that hash are ${sharing.map((x) => `${x.ref.id}@${x.ref.version}`).join(", ")}; not attached`,
        );
        continue;
      }
      sharing.splice(0, sharing.length, exact);
    }
    const byBytes = sharing.filter((x) => x.package?.ref.sha256 === i.method_package_sha256);
    const n = byBytes.length === 1 ? byBytes[0] : sharing.length === 1 ? sharing[0] : undefined;
    if (!n) {
      problems.push(
        sharing.length === 0
          ? `static inspection names package ${i.method_package_sha256.slice(0, 12)}, which no supplied record contains`
          : `static inspection names package ${i.method_package_sha256.slice(0, 12)}, which ${sharing.length} different references share; not attached`,
      );
      continue;
    }
    n.inspections.push(i);
    const path = i.entrypoint ?? n.package?.entrypoint;
    const entry = n.package?.files.find((f) => f.path === path);
    if (n.package && (!entry || entry.sha256 !== i.source_sha256))
      problems.push(`static inspection of ${path} names different source bytes than the supplied package`);
  }

  const trials: Trial[] = (records.trials ?? []).map((t) => ({
    ...t,
    executions: executions.filter((e) => {
      const p = e.receipt.purpose;
      if ((p.kind !== "acceptance" && p.kind !== "comparison") || refKey(p.plan) !== refKey(t.plan.ref)) return false;
      const why = trialBindingProblem(e, t.plan);
      if (why) problems.push(`execution ${e.receipt.org_id}/${e.receipt.run_id}/${e.receipt.attempt_id}: ${why}; not attached to trial`);
      return why === null;
    }),
  }));

  const metrics = new Map((records.metrics ?? []).map((m) => [m.sha256, m]));
  for (const e of executions) {
    const p = e.receipt.purpose;
    const plan =
      p.kind === "acceptance" || p.kind === "comparison"
        ? (records.trials ?? []).find((t) => refKey(t.plan.ref) === refKey(p.plan))?.plan ?? null
        : null;
    e.score = scoreOf(e, plan, metrics, problems);
  }
  return { attempts, nodes: [...nodes.values()], edges, executions, trials, metrics, problems };
}

/** Resolves one run's score from its attached evaluations. A scored evaluation
 * counts only when its metrics reference is the supported identity, its scoring
 * context names the run's own task and matches what the scorer observed, and
 * its rows are one per instance of the panel it claims. Disagreeing scored
 * evaluations of one run are a conflict: none is chosen, whatever their order. */
function scoreOf(e: Execution, plan: TrialPlan | null, metrics: Map<string, MetricsArtifact>, problems: string[]): Score {
  const run = `${e.receipt.org_id}/${e.receipt.run_id}/${e.receipt.attempt_id}`;
  const bindingProblem = plan ? trialBindingProblem(e, plan) : null;
  if (bindingProblem) return { kind: "rejected", reasons: [bindingProblem] };
  if (e.evaluations.length === 0) return { kind: "unscored" };
  const scored = e.evaluations.filter((v) => v.outcome === "scored");
  if (scored.length === 0) return { kind: "not_scored", outcomes: [...new Set(e.evaluations.map((v) => v.outcome))].sort() };
  const counted = new Map<string, { successes: number; of: number }>();
  const reasons: string[] = [];
  const refuse = (v: EvaluationReceipt, why: string) => {
    reasons.push(why);
    problems.push(`evaluation ${v.request_id} of execution ${run}: ${why}; not counted`);
  };
  const unchecked: string[] = [];
  for (const v of scored) {
    // Without the named bytes there is nothing to count, so nothing to refuse.
    const m = v.metrics ? metrics.get(v.metrics.sha256) : undefined;
    if (!m) {
      unchecked.push(v.request_id);
      continue;
    }
    const why = contextProblem(e, v, plan);
    if (why) {
      refuse(v, why);
      continue;
    }
    const rows = rowsProblem(m, e, v, plan);
    if (rows) {
      refuse(v, rows);
      continue;
    }
    counted.set(m.sha256, { successes: m.instances.filter((r) => r.success).length, of: m.instances.length });
  }
  if (counted.size > 1) {
    const counts = [...counted.values()].map((c) => `${c.successes}/${c.of}`).sort();
    problems.push(`execution ${run} has scored evaluations with different results (${counts.join(", ")}); none is counted`);
    return { kind: "conflict", counts };
  }
  const [only] = counted.entries();
  if (only && unchecked.length > 0)
    problems.push(
      `execution ${run} is counted from one scoring, but ${unchecked.join(", ")} names metrics that were not supplied and could not be compared with it`,
    );
  if (only) return { kind: "counted", successes: only[1].successes, of: only[1].of, metrics: only[0] };
  return reasons.length > 0 ? { kind: "rejected", reasons } : { kind: "metrics_missing" };
}

function trialBindingProblem(e: Execution, plan: TrialPlan): string | null {
  if (!plan.slots) return `trial plan ${plan.ref.id} has no supported declared slots`;
  const matches = plan.slots.filter((s) => s.binding_sha256 === e.receipt.binding_sha256);
  if (!e.receipt.binding_hash_matches || matches.length !== 1)
    return `its binding does not match exactly one declared slot of trial plan ${plan.ref.id}`;
  const slot = matches[0]!;
  if (refKey(e.receipt.actual_method) !== refKey(slot.method) || refKey(e.receipt.actual_environment) !== refKey(slot.environment))
    return `its actual method or environment differs from its declared trial slot`;
  return null;
}

function contextProblem(e: Execution, v: EvaluationReceipt, plan: TrialPlan | null): string | null {
  if (!v.metrics || !isSupportedMetricsRef(v.metrics))
    return `its metrics reference ${v.metrics ? `${v.metrics.schema} ${v.metrics.id}@${v.metrics.version}` : "is missing and"} is not the supported ${METRICS_IDENTITY.schema} identity`;
  const task = v.context.public_task;
  const own = e.receipt.public_task;
  if (!task || refKey(task) !== refKey(own))
    return `it was scored against task ${task ? `${task.id}@${task.version} ${task.sha256.slice(0, 12)}` : "(none named)"}, not the run's ${own.id}@${own.version} ${own.sha256.slice(0, 12)}`;
  if (!v.context.evaluator || refKey(v.context.evaluator) !== refKey(e.receipt.evaluator))
    return "its scoring evaluator differs from the evaluator frozen in the execution binding";
  if (!v.observed_context) return "the scorer's observed context is not recorded";
  if (!sameContext(v.context, v.observed_context)) return "the scorer's observed context differs from the requested one";
  // Agreeing with the scorer's own request is not enough: the trial's plan froze its context.
  if (plan && !sameContext(v.context, plan.context)) return `its scoring context differs from the one its trial plan ${plan.ref.id} froze`;
  return null;
}

function rowsProblem(m: MetricsArtifact, e: Execution, v: EvaluationReceipt, plan: TrialPlan | null): string | null {
  const ids = m.instances.map((r) => r.instance);
  if (new Set(ids).size !== ids.length) return "its metrics repeat an instance row";
  if (m.of !== null && m.of !== ids.length) return `its metrics have ${ids.length} rows but record of = ${m.of}`;
  const wins = m.instances.filter((r) => r.success).length;
  if (m.success_count !== null && m.success_count !== wins)
    return `its metrics count ${wins} successful rows but record success_count = ${m.success_count}`;
  if (plan && plan.scoring_kind === "development") {
    if (ids.length !== plan.development_instances.length || ids.some((id, i) => id !== plan.development_instances[i]))
      return "its metrics rows are not the plan's development instances, one each, in order";
  } else if (v.context.instance_set?.id === "development") {
    const prefix = `${e.receipt.public_task.id}/`;
    if (ids.some((id) => !id.startsWith(prefix))) return `its metrics rows name instances outside ${e.receipt.public_task.id}`;
  }
  return null;
}
