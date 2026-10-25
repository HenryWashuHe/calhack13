// Eric's Claude: turn files the operator chose into LabRecords
// (docs/invention-lab/ui.md#preview). Each file is recognized by shape or
// listed with the reason it was not used; nothing is guessed or fetched.

import { canonicalSha256, sha256Hex } from "./digest";
import type { LabRecords } from "./lineage";
import {
  decodeAdoptionOutcome,
  decodeCheckpointOutcome,
  decodeEvaluationReceipt,
  decodeExecutionReceipt,
  decodeGenerationStatus,
  decodeMetricsArtifact,
  decodePackageFile,
  decodeRevisionObservation,
  decodeStaticInspection,
  decodeTrialPlan,
  decodeVerdict,
  INSPECTION_SCHEMA,
  isMetricsShape,
  PACKAGE_DOMAIN,
  REVISION_SCHEMA,
  TRIAL_PLAN_FIELDS,
  type CheckpointOutcome,
  type TrialPlan,
  type Verdict,
} from "./records";

export interface Note {
  file: string;
  note: string;
}
export interface Intake {
  records: LabRecords;
  used: Note[];
  skipped: Note[];
}

const has = (o: Record<string, unknown>, ...keys: string[]) => keys.every((k) => k in o);

/** `bytes` are the exact file bytes when available (a package's identity is
 * their hash); otherwise the text is UTF-8 encoded. */
export function intake(files: { name: string; text: string; bytes?: Uint8Array }[]): Intake {
  const records: LabRecords = {
    generations: [],
    revisions: [],
    adoptions: [],
    executions: [],
    evaluations: [],
    packages: [],
    inspections: [],
    trials: [],
    metrics: [],
  };
  const used: Note[] = [];
  const skipped: Note[] = [];
  const checkpoints: { file: string; c: CheckpointOutcome }[] = [];
  const plans: { file: string; plan: TrialPlan }[] = [];
  const verdicts: { file: string; verdict: Verdict }[] = [];
  // Identical records (e.g. a status saved as both step-result.json and
  // final-status.json) are one record: counted once, the copy listed. A
  // package's identity is its raw bytes, so only byte-identical copies merge;
  // a standalone verdict names no trial, so copies merge only within a folder.
  const firstFile = new Map<string, string>();
  const folder = (f: string) => (f.includes("/") ? f.slice(0, f.lastIndexOf("/")) : "");

  for (const { name, text, bytes } of files) {
    let v: unknown;
    try {
      v = JSON.parse(text);
    } catch {
      skipped.push({ file: name, note: "not JSON" });
      continue;
    }
    if (typeof v !== "object" || v === null || Array.isArray(v)) {
      skipped.push({ file: name, note: "not a JSON object" });
      continue;
    }
    const o = v as Record<string, unknown>;
    const isVerdict = typeof o.verdict === "string" && !("outcome" in o);
    const identity =
      o.domain === PACKAGE_DOMAIN || isMetricsShape(o)
        ? `bytes ${sha256Hex(bytes ?? new TextEncoder().encode(text))}`
        : isVerdict
          ? `verdict ${folder(name)} ${canonicalSha256(o)}`
          : canonicalSha256(o);
    const first = firstFile.get(identity);
    if (first !== undefined) {
      skipped.push({ file: name, note: `identical to ${first}; counted once` });
      continue;
    }
    firstFile.set(identity, name);
    try {
      if (o.domain === PACKAGE_DOMAIN) {
        records.packages!.push(decodePackageFile(bytes ?? new TextEncoder().encode(text)));
        used.push({ file: name, note: "method package (identity hashed from the file bytes here)" });
      } else if (isMetricsShape(o)) {
        records.metrics!.push(decodeMetricsArtifact(bytes ?? new TextEncoder().encode(text)));
        used.push({ file: name, note: "scoring metrics (identity hashed from the file bytes here)" });
      } else if (o.schema === INSPECTION_SCHEMA) {
        records.inspections!.push(decodeStaticInspection(o));
        used.push({ file: name, note: "static source inspection" });
      } else if (has(o, "authority", "phase", "plan_sha256", "operation_id")) {
        records.generations.push({ status: decodeGenerationStatus(o) });
        used.push({ file: name, note: "generation status" });
      } else if (has(o, "plan", "request", "request_sha256", "output_contract", "phase")) {
        checkpoints.push({ file: name, c: decodeCheckpointOutcome(o) });
      } else if (o.schema === REVISION_SCHEMA) {
        records.revisions!.push(decodeRevisionObservation(o));
        used.push({ file: name, note: "revision observation" });
      } else if (has(o, "binding", "actual_method", "outputs", "transcript")) {
        records.executions!.push(decodeExecutionReceipt(o));
        used.push({ file: name, note: "execution receipt" });
      } else if (has(o, "request", "outcome", "trace") && typeof o.request === "object" && o.request && "execution_sha256" in o.request) {
        records.evaluations!.push(decodeEvaluationReceipt(o));
        used.push({ file: name, note: "evaluation receipt" });
      } else if (has(o, ...TRIAL_PLAN_FIELDS)) {
        plans.push({ file: name, plan: decodeTrialPlan(o) });
        used.push({ file: name, note: "trial plan (reference computed here)" });
      } else if (isVerdict) {
        verdicts.push({ file: name, verdict: decodeVerdict(o) });
      } else if (typeof o.outcome === "string") {
        const outcome = decodeAdoptionOutcome(o);
        if (outcome.installed) {
          const { org_id, method } = outcome.installed;
          records.adoptions!.push({ receiver_org: org_id, offer: method, outcome });
          used.push({ file: name, note: "adoption outcome (installation names its own org and method)" });
        } else {
          skipped.push({ file: name, note: `adoption outcome "${outcome.outcome}" does not name its receiver or method; needs host pairing` });
        }
      } else {
        skipped.push({ file: name, note: "not a record type this view reads" });
      }
    } catch (e) {
      skipped.push({ file: name, note: `malformed: ${(e as Error).message}` });
    }
  }

  // A status pairs with the checkpoint of the same organization, operation and
  // exact request, in the same phase. Operation names can repeat across
  // organizations; an operation's earlier (e.g. Prepared) checkpoint is listed, not used.
  for (const { file, c } of checkpoints) {
    const g = records.generations.find(
      (x) => x.status.org_id === c.org_id && x.status.operation_id === c.operation_id && x.status.request_sha256 === c.request_sha256,
    );
    if (g && !g.checkpoint && g.status.phase === c.phase) {
      g.checkpoint = c;
      used.push({ file, note: "generation checkpoint (outcome enum only)" });
    } else {
      skipped.push({
        file,
        note: g
          ? `generation checkpoint in phase ${c.phase}; the status for ${c.operation_id} of ${c.org_id} is ${g.status.phase}`
          : `generation checkpoint without a matching status for ${c.operation_id} of ${c.org_id}`,
      });
    }
  }
  // A standalone verdict names no plan. Pair it with the single trial plan in the
  // same folder; otherwise list it unused rather than guess.
  const paired = new Set<string>();
  for (const { file, verdict } of verdicts) {
    const here = plans.filter((p) => folder(p.file) === folder(file));
    if (here.length === 1) {
      records.trials!.push({ plan: here[0]!.plan, verdict });
      paired.add(here[0]!.file);
      used.push({ file, note: `trial verdict (paired by folder with ${here[0]!.file}; the verdict names no plan)` });
    } else {
      skipped.push({ file, note: `trial verdict with ${here.length} trial plans in its folder; names no plan, so not paired` });
    }
  }
  for (const p of plans) if (!paired.has(p.file)) records.trials!.push({ plan: p.plan, verdict: null });
  return { records, used, skipped };
}

/** Organizations named anywhere in the records, for the operator to choose A and B. */
export function orgsIn(i: Intake): string[] {
  const r = i.records;
  const orgs = [
    ...r.generations.map((g) => g.status.org_id),
    ...(r.revisions ?? []).map((x) => x.org_id),
    ...(r.adoptions ?? []).map((x) => x.receiver_org),
    ...(r.executions ?? []).map((x) => x.org_id),
  ];
  return [...new Set(orgs)].sort();
}
