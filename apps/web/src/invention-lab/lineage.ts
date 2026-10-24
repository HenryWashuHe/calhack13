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
}

export interface Execution {
  receipt: ExecutionReceipt;
  /** Node key of the method the executor reports it ran. */
  method: string;
  /** Evaluations whose scored output is one of this run's outputs, with the same purpose.
   * Matched by exact output ref; `execution_sha256` is not recomputed here. */
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
  | { kind: "generated"; operation_id: string; to: string }
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

  const seen = new Set<string>();
  const attempts = records.generations.map(({ status, checkpoint }): Attempt => {
    const op = status.operation_id;
    if (seen.has(op)) problems.push(`operation ${op} appears more than once`);
    seen.add(op);
    let outcome: Attempt["outcome"] = null;
    if (checkpoint) {
      if (checkpoint.operation_id !== op || checkpoint.request_sha256 !== status.request_sha256) {
        problems.push(`checkpoint does not match status for operation ${op}; its outcome is not shown`);
      } else if (checkpoint.phase !== status.phase) {
        problems.push(`checkpoint phase ${checkpoint.phase} differs from status phase ${status.phase} for ${op}`);
      } else {
        outcome = checkpoint.outcome;
      }
    }
    if (status.package) {
      if (status.phase === "generated") edges.push({ kind: "generated", operation_id: op, to: node(status.package) });
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
    const gen = attempts.find((a) => a.status.operation_id === r.operation_id);
    if (gen?.status.package && refKey(gen.status.package) !== refKey(r.child))
      problems.push(`revision child differs from the package of operation ${r.operation_id}`);
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

  const executions: Execution[] = [];
  const runs = new Set<string>();
  for (const receipt of records.executions ?? []) {
    const run = `${receipt.org_id}/${receipt.run_id}/${receipt.attempt_id}`;
    if (runs.has(run)) problems.push(`execution ${run} appears more than once`);
    runs.add(run);
    if (refKey(receipt.bound_method) !== refKey(receipt.actual_method))
      problems.push(`execution ${run} ran a different method than its binding named; shown under the method it ran`);
    executions.push({ receipt, method: node(receipt.actual_method), evaluations: [] });
  }
  for (const ev of records.evaluations ?? []) {
    const out = refKey(ev.output);
    const matches = executions.filter(
      (e) => e.receipt.outputs.some((o) => refKey(o) === out) && samePurpose(e.receipt.purpose, ev.purpose),
    );
    if (matches.length === 1) matches[0]!.evaluations.push(ev);
    else
      problems.push(
        `evaluation ${ev.request_id} matches ${matches.length} executions by output and purpose; not attached`,
      );
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
    nodes.get(node(p.ref))!.package = p;
  }
  for (const i of records.inspections ?? []) {
    const n = [...nodes.values()].find((x) => x.ref.sha256 === i.method_package_sha256);
    if (!n) {
      problems.push(`static inspection names package ${i.method_package_sha256.slice(0, 12)}, which no supplied record contains`);
      continue;
    }
    n.inspections.push(i);
    const entry = n.package?.files.find((f) => f.path === i.entrypoint);
    if (n.package && (!entry || entry.sha256 !== i.source_sha256))
      problems.push(`static inspection of ${i.entrypoint} names different source bytes than the supplied package`);
  }

  return { attempts, nodes: [...nodes.values()], edges, executions, problems };
}
