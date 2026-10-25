// Eric's Claude: arrange decoded records into attempts, exact method nodes and
// typed edges (docs/invention-lab/ui.md). Pure and read-only. Inconsistent
// records are reported as problems, never silently reconciled.

import { tierOf, type Tier } from "./claims";
import {
  refKey,
  samePurpose,
  type AdoptionOutcome,
  type CheckpointOutcome,
  type EvaluationReceipt,
  type ExactRef,
  type ExecutionReceipt,
  type GenerationStatus,
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
}

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
  | { kind: "revised_from"; org_id: string; operation_id: string; from: string; to: string; source_changed: boolean }
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
    edges.push({
      kind: "revised_from",
      org_id: r.org_id,
      operation_id: r.operation_id,
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
    executions.push({ receipt, method: node(receipt.actual_method), evaluations: [] });
  }
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
    executions: executions.filter(
      (e) =>
        (e.receipt.purpose.kind === "acceptance" || e.receipt.purpose.kind === "comparison") &&
        refKey(e.receipt.purpose.plan) === refKey(t.plan.ref),
    ),
  }));

  return { attempts, nodes: [...nodes.values()], edges, executions, trials, problems };
}
