// What needs an operator, and what the kernel refused or failed, derived only
// from persisted records and events. Nothing here infers success.

import type { DomainEvent, Job, OrgChangeRecord, OrgState } from "./contracts";
import { roleName } from "./graph";

/**
 * - `action`: blocked until an operator decides (lost provider outcome).
 * - `unknown`: a job whose outcome is unknown; reconcile before retrying.
 * - `waiting`: progressing but not proven (readiness, domain verification).
 * - `refused`: the kernel rejected an unsafe or invalid request.
 * - `failed`: work that ended FAILED; provider IDs are kept for cleanup.
 */
export type AttentionKind = "action" | "unknown" | "waiting" | "refused" | "failed";

export interface ReconcileTarget {
  change_id: string;
  role_key: string;
  provider: string;
  operation_id: string;
  run_id?: string | null;
}

export interface AttentionItem {
  key: string;
  kind: AttentionKind;
  title: string;
  detail: string;
  refs: string[];
  /** Event sequence for history items. */
  seq?: number;
  reconcile?: ReconcileTarget;
}

const OPEN: AttentionKind[] = ["action", "unknown", "waiting"];

export function isOpen(item: AttentionItem): boolean {
  return OPEN.includes(item.kind);
}

/** Open items first (actions, unknown outcomes, waiting), then history newest first. */
export function attention(
  changes: OrgChangeRecord[],
  jobs: Job[],
  events: DomainEvent[],
  state: OrgState | null,
): AttentionItem[] {
  const open: AttentionItem[] = [];
  for (const c of changes) {
    if (c.status !== "PROVISIONING") continue;
    for (const [roleKey, op] of Object.entries(c.operations ?? {})) {
      const where = `${roleKey} on ${op.provider} · operation ${op.operation_id}${op.run_id ? ` · run ${op.run_id}` : ""}`;
      if (op.state === "needs_reconciliation") {
        open.push({
          key: `reconcile:${c.change_id}:${roleKey}`,
          kind: "action",
          title: "Provider outcome unknown: attach the agent you found, or abandon",
          detail: where + (op.attention ? ` — ${op.attention}` : ""),
          refs: [c.change_id],
          reconcile: {
            change_id: c.change_id,
            role_key: roleKey,
            provider: op.provider,
            operation_id: op.operation_id,
            run_id: op.run_id,
          },
        });
      } else if (op.state === "awaiting_readiness") {
        open.push({
          key: `readiness:${c.change_id}:${roleKey}`,
          kind: "waiting",
          title: "Agent created; role inactive until readiness is proven",
          detail: where + (op.attention ? ` — ${op.attention}` : ""),
          refs: [c.change_id],
        });
      } else if (op.state === "pending" && op.attention) {
        open.push({
          key: `pending:${c.change_id}:${roleKey}`,
          kind: "waiting",
          title: "Provider operation retrying",
          detail: `${where} — ${op.attention}`,
          refs: [c.change_id],
        });
      }
    }
  }
  for (const j of jobs) {
    const who = roleName(state, j.role_id);
    if (j.status === "UNKNOWN") {
      open.push({
        key: `job-unknown:${j.job_id}`,
        kind: "unknown",
        title: "Job outcome unknown: reconcile with the provider before retrying",
        detail: `${j.task} · ${who}${j.raw_status ? ` · provider status ${j.raw_status}` : ""}`,
        refs: [j.job_id],
      });
    } else if (j.status === "VERIFYING") {
      open.push({
        key: `job-verifying:${j.job_id}`,
        kind: "waiting",
        title: "Provider reported success; not counted until the result is verified",
        detail: `${j.task} · ${who}${j.raw_status ? ` · provider status ${j.raw_status}` : ""}`,
        refs: [j.job_id],
      });
    }
  }

  // Provider resources kept by failed provisioning, by change.
  const retained = new Map<string, string[]>();
  for (const e of events) {
    if (e.type === "provisioning_failed" && e.payload.provider_agent_id) {
      const list = retained.get(e.payload.change_id) ?? [];
      list.push(e.payload.provider_agent_id);
      retained.set(e.payload.change_id, list);
    }
  }

  const history: AttentionItem[] = [];
  for (const e of events) {
    switch (e.type) {
      case "change_status_changed": {
        if (e.payload.to !== "REJECTED" && e.payload.to !== "FAILED") break;
        const rejected = e.payload.to === "REJECTED";
        const kept = retained.get(e.payload.change_id) ?? [];
        history.push({
          key: `change:${e.seq}`,
          kind: rejected ? "refused" : "failed",
          title: rejected ? "Change rejected" : "Change failed; organization unchanged",
          detail:
            (e.payload.reasons?.length ? e.payload.reasons.join("; ") : `${e.payload.from} → ${e.payload.to}`) +
            (kept.length ? ` (provider agent ${kept.join(", ")} kept for cleanup)` : ""),
          refs: [e.payload.change_id, ...kept],
          seq: e.seq,
        });
        break;
      }
      case "job_result_rejected":
        history.push({
          key: `result:${e.seq}`,
          kind: "refused",
          title: "Result rejected",
          detail: e.payload.reason,
          refs: [e.payload.job_id],
          seq: e.seq,
        });
        break;
      case "installation_rejected":
        history.push({
          key: `install:${e.seq}`,
          kind: "refused",
          title: "Local acceptance failed; capability not usable",
          detail: e.payload.reasons.join("; ") || "acceptance failed",
          refs: [e.payload.installation_id],
          seq: e.seq,
        });
        break;
      case "job_status_changed":
        if (e.payload.to === "FAILED") {
          history.push({
            key: `job-failed:${e.seq}`,
            kind: "failed",
            title: "Job failed",
            detail: `${e.payload.from} → FAILED`,
            refs: [e.payload.job_id],
            seq: e.seq,
          });
        }
        break;
      default:
        break;
    }
  }
  history.sort((a, b) => (b.seq ?? 0) - (a.seq ?? 0));
  return [...open, ...history];
}

export function counts(items: AttentionItem[]): Record<AttentionKind, number> {
  const c: Record<AttentionKind, number> = { action: 0, unknown: 0, waiting: 0, refused: 0, failed: 0 };
  for (const i of items) c[i.kind] += 1;
  return c;
}

/** Client-side check mirroring the kernel's rule for attach requests. */
export function validAgentId(id: string): boolean {
  return /^[A-Za-z0-9_.:-]{1,128}$/.test(id);
}
