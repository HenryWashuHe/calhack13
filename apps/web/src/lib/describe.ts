// Human-readable descriptions of domain events for the evidence log.

import type { DomainEvent, OrgState } from "./contracts";
import { roleName, short } from "./graph";
import { gapLabel } from "./gaps";

export type Category = "organization" | "evidence" | "jobs" | "capabilities" | "changes";

export interface Described {
  title: string;
  detail: string;
  category: Category;
  /** Identifiers worth showing verbatim (provider IDs, digests). */
  refs: string[];
  devStub?: boolean;
  tone?: "ok" | "warn" | "bad" | "neutral";
}

export function describe(ev: DomainEvent, state: OrgState | null): Described {
  const r = (id: string) => roleName(state, id);
  switch (ev.type) {
    case "organization_initialized":
      return { title: "Organization initialized", detail: ev.payload.name, category: "organization", refs: [] };
    case "mission_created":
      return { title: "Mission created", detail: ev.payload.goal, category: "evidence", refs: [ev.payload.mission_id] };
    case "observation_recorded":
      return {
        // This source identifies a gap update, not proof the planner consumed it.
        title: ev.payload.source === "kernel.capability_gap" ? "Capability gap update recorded" : "Observation",
        detail: `${ev.payload.source}: ${ev.payload.summary}`,
        category: "evidence",
        refs: [ev.payload.observation_id],
      };
    case "requirement_recorded":
      return {
        title: "Requirement derived",
        detail: ev.payload.summary,
        category: "evidence",
        refs: [ev.payload.requirement_id, ...ev.payload.evidence],
      };
    case "change_proposed":
      return {
        title: "Change proposed",
        detail: `${ev.payload.operations.join(", ")} · ${ev.payload.rationale}`,
        category: "changes",
        refs: [ev.payload.change_id, `expects r${ev.payload.expected_revision}`],
      };
    case "change_status_changed": {
      const to = ev.payload.to;
      const tone = to === "COMMITTED" ? "ok" : to === "FAILED" || to === "REJECTED" ? "bad" : "neutral";
      return {
        title: `Change ${to.toLowerCase()}`,
        detail: ev.payload.reasons?.length ? ev.payload.reasons.join("; ") : `${ev.payload.from} → ${to}`,
        category: "changes",
        refs: [ev.payload.change_id],
        tone,
      };
    }
    case "role_provisioned":
      return {
        title: ev.payload.dev_stub ? "Role provisioned (dev stub)" : "Role provisioned",
        detail: `${ev.payload.role_key} on ${ev.payload.provider}`,
        category: "organization",
        refs: [ev.payload.provider_agent_id, ev.payload.role_id],
        devStub: ev.payload.dev_stub,
        tone: ev.payload.dev_stub ? "warn" : "ok",
      };
    case "provisioning_progress": {
      const labels: Record<string, string> = {
        pending: "Provider creating agent",
        awaiting_readiness: "Agent created, awaiting readiness",
        ready: "Agent readiness proven",
        needs_reconciliation: "Provider outcome unknown",
        failed: "Provider operation failed",
      };
      const p = ev.payload;
      // The kernel's attach note: the agent ID came from an operator, not the provider.
      const attached = p.state === "awaiting_readiness" && (p.attention ?? "").includes("attached by operator");
      return {
        title: attached ? "Agent ID attached by operator, awaiting readiness" : labels[p.state] ?? `Provider operation ${p.state}`,
        detail:
          `${p.role_key} on ${p.provider} · ${p.operation_id}` +
          (p.run_id ? ` · run ${p.run_id}` : "") +
          (p.attention ? ` — ${p.attention}` : ""),
        category: "organization",
        refs: [p.change_id],
        tone:
          p.state === "ready" ? "ok" : p.state === "failed" || p.state === "needs_reconciliation" ? "bad" : attached || p.attention ? "warn" : undefined,
      };
    }
    case "provisioning_failed":
      return {
        title: "Provisioning failed",
        detail:
          `${ev.payload.role_key}: ${ev.payload.error}` +
          (ev.payload.provider_agent_id ? ` (agent ${ev.payload.provider_agent_id} retained for cleanup)` : ""),
        category: "organization",
        refs: [ev.payload.change_id],
        tone: "bad",
      };
    case "role_created":
      return {
        title: "Role created",
        detail: `${ev.payload.name} · ${ev.payload.provider}`,
        category: "organization",
        refs: [ev.payload.role_id],
      };
    case "role_activated":
      return { title: "Role activated", detail: r(ev.payload.role_id), category: "organization", refs: [ev.payload.role_id], tone: "ok" };
    case "responsibility_created":
      return { title: "Responsibility created", detail: ev.payload.purpose, category: "organization", refs: [ev.payload.responsibility_id] };
    case "responsibility_assigned":
      return {
        title: "Ownership assigned",
        detail: `${r(ev.payload.owner)} now accountable (epoch ${ev.payload.epoch})`,
        category: "organization",
        refs: [ev.payload.responsibility_id],
      };
    case "responsibility_transferred":
      return {
        title: "Ownership transferred",
        detail: `${r(ev.payload.from)} → ${r(ev.payload.to)} (epoch ${ev.payload.epoch})`,
        category: "organization",
        refs: [ev.payload.responsibility_id],
      };
    case "responsibility_completed":
      return { title: "Responsibility completed", detail: short(ev.payload.responsibility_id), category: "organization", refs: [], tone: "ok" };
    case "task_delegated":
      return {
        title: "Task delegated",
        detail: `${ev.payload.task} → ${ev.payload.executor.kind} ${short(ev.payload.executor.id)}`,
        category: "organization",
        refs: [ev.payload.responsibility_id],
      };
    case "contract_requested":
      return {
        title: "Specialist recruitment requested",
        detail: ev.payload.scope,
        category: "organization",
        refs: [ev.payload.specialist_address, ev.payload.contract_id],
      };
    case "contract_state_changed":
      return {
        title: `Contract ${ev.payload.to.toLowerCase()}`,
        detail: `${ev.payload.from} → ${ev.payload.to}`,
        category: "organization",
        refs: [ev.payload.contract_id],
      };
    case "capability_attached":
      return {
        title: "Capability attached",
        detail: `${ev.payload.capability.capability_id}@${ev.payload.capability.version} → ${r(ev.payload.role_id)}`,
        category: "capabilities",
        refs: [],
      };
    case "capability_gap_opened":
      return {
        title: "Capability requested",
        detail: `${ev.payload.capability_id} for ${r(ev.payload.role_id)}`,
        category: "capabilities",
        refs: [ev.payload.gap_id],
      };
    case "capability_gap_status_changed": {
      const from = gapLabel(ev.payload.from);
      const to = gapLabel(ev.payload.to);
      return {
        title: `Requested capability ${to.label.toLowerCase()}`,
        detail: [`${from.label} → ${to.label}`, to.version, to.detail].filter(Boolean).join(" · "),
        category: "capabilities",
        refs: [ev.payload.gap_id],
        tone: to.tone,
      };
    }
    case "reporting_edge_changed":
      return {
        title: "Reporting line changed",
        detail: `${r(ev.payload.role_id)} → ${ev.payload.reports_to ? r(ev.payload.reports_to) : "top level"}`,
        category: "organization",
        refs: [],
      };
    case "role_draining":
      return { title: "Role draining", detail: r(ev.payload.role_id), category: "organization", refs: [], tone: "warn" };
    case "role_retired":
      return { title: "Role retired", detail: r(ev.payload.role_id), category: "organization", refs: [ev.payload.role_id] };
    case "assignment_fenced":
      return {
        title: "Assignment fenced",
        detail: `epoch ${ev.payload.epoch} can no longer change commitments`,
        category: "organization",
        refs: [ev.payload.assignment_id],
      };
    case "job_dispatched":
      return {
        title: "Job dispatched",
        detail: `${ev.payload.task} · ${r(ev.payload.role_id)} · epoch ${ev.payload.epoch}`,
        category: "jobs",
        refs: [ev.payload.job_id],
      };
    case "job_status_changed": {
      const to = ev.payload.to;
      const tone = to === "SUCCEEDED" ? "ok" : to === "FAILED" || to === "RESULT_REJECTED" ? "bad" : to === "UNKNOWN" ? "warn" : "neutral";
      return { title: `Job ${to.toLowerCase().replace("_", " ")}`, detail: `${ev.payload.from} → ${to}`, category: "jobs", refs: [ev.payload.job_id], tone };
    }
    case "job_result_rejected":
      return { title: "Result rejected", detail: ev.payload.reason, category: "jobs", refs: [ev.payload.job_id], tone: "bad" };
    case "commitment_recorded":
      return { title: "Ledger commitment", detail: ev.payload.operation_id, category: "jobs", refs: [ev.payload.job_id], tone: "ok" };
    case "capability_installed":
      return {
        title: "Capability installed",
        detail: `${ev.payload.capability.capability_id}@${ev.payload.capability.version} · ${ev.payload.synthesis_calls} synthesis calls`,
        category: "capabilities",
        refs: [ev.payload.source_digest.slice(0, 16), ev.payload.installation_id],
      };
    case "installation_accepted":
      return { title: "Local acceptance passed", detail: ev.payload.report_ref, category: "capabilities", refs: [ev.payload.installation_id], tone: "ok" };
    case "installation_rejected":
      return { title: "Local acceptance failed", detail: ev.payload.reasons.join("; "), category: "capabilities", refs: [ev.payload.installation_id], tone: "bad" };
    case "capability_run_requested":
      return {
        title: "Capability run",
        detail: `${ev.payload.capability.capability_id}@${ev.payload.capability.version} by ${r(ev.payload.role_id)}`,
        category: "capabilities",
        refs: [ev.payload.job_id],
      };
    case "planner_decision": {
      const v = ev.payload.verdict;
      const label: Record<typeof v, string> = {
        change_proposed: "Planner proposed a change",
        no_change: "Planner: no change needed",
        rejected: "Planner proposal rejected",
        escalated: "Planner escalated to a human",
      };
      const tone = v === "change_proposed" ? "ok" : v === "no_change" ? "neutral" : v === "rejected" ? "warn" : "bad";
      return {
        title: label[v],
        detail: `${ev.payload.summary} — ${ev.payload.rationale}`,
        category: "changes",
        refs: [
          `${ev.payload.planner} · ${ev.payload.model}`,
          `attempt ${ev.payload.attempt} · ${ev.payload.latency_ms} ms`,
          ...(ev.payload.change_id ? [ev.payload.change_id] : []),
          ...(ev.payload.kernel_errors ?? []),
        ],
        tone,
      };
    }
    default: {
      const unknown = ev as { type: string };
      return { title: unknown.type, detail: "", category: "organization", refs: [] };
    }
  }
}
