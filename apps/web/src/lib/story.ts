// A self-contained HTML record of how an organization evolved: each
// generation and refusal, the evidence chain behind every role that exists
// now, and the constitution audit of the current snapshot. Built only from
// the records the console already holds. DevStub bindings stay labeled.

import { type HistoryAudit, ruleName } from "./audit";
import type { DomainEvent, OrgChangeRecord, OrgState } from "./contracts";
import { TimelineEntry, timeline } from "./evolution";
import { roleProvenance } from "./provenance";

const esc = (s: unknown) =>
  String(s ?? "").replace(/[&<>"']/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[c]!);

function entryHtml(e: TimelineEntry): string {
  if (e.kind === "refusal") {
    return `<li class="refusal ${e.by}"><div class="rev">r${e.revision}<span class="tag bad">${esc(
      e.by === "kernel" ? "Refused" : e.by === "provider" ? "Failed" : "Escalated",
    )}</span></div><div><b>${esc(e.title)}</b>${e.rationale ? `<p><i>“${esc(e.rationale)}”</i></p>` : ""}<ul>${e.reasons
      .map((r) => `<li>${esc(r)}</li>`)
      .join("")}</ul><small>${esc([e.changeId, e.submittedBy && `by ${e.submittedBy}`].filter(Boolean).join(" · "))}</small></div></li>`;
  }
  const deltas = e.deltas
    .map(
      (d) =>
        `<li class="${d.kind}">${esc(d.text)}${d.devStub ? ` <span class="tag warn">DEV STUB${d.provider ? ` · for ${esc(d.provider)}` : ""}</span>` : ""}${
          d.operatorAttached ? ' <span class="tag warn">attached by operator</span>' : ""
        }</li>`,
    )
    .join("");
  return `<li><div class="rev">r${e.revision}<span class="tag">${esc(e.label)}</span><small>${e.activeRoles} roles</small></div><div><b>${esc(
    e.cause,
  )}</b>${e.triggers.length ? `<p>because ${esc(e.triggers.join("; "))}</p>` : ""}<ul class="deltas">${deltas}</ul><small>${esc(
    [e.changeId, e.submittedBy && `by ${e.submittedBy}`, e.plannerModel && `planner ${e.plannerModel}`].filter(Boolean).join(" · "),
  )}</small></div></li>`;
}

function auditHtml(a: HistoryAudit | null): string {
  if (!a) return "<h2>Kernel audit</h2><p>The kernel audit was not available when this page was exported.</p>";
  const head =
    a.violation_count === 0
      ? `every stored revision r${a.first_revision ?? 0}–r${a.last_revision ?? 0} passes ${a.invariants.length} invariants`
      : `${a.violation_count} violation(s) in stored history`;
  const list = a.violations
    .map((v) => `<li class="bad">r${v.revision} <b>${esc(ruleName(v.invariant))}</b> — ${esc(v.detail)}</li>`)
    .join("");
  return `<h2>Kernel audit: ${esc(head)}</h2>
<p>${a.snapshots_checked} snapshots and ${a.events_checked} events checked by genesis_core::audit. Rules: ${a.invariants
    .map((n) => esc(ruleName(n)))
    .join(", ")}.</p>${list ? `<ul class="audit">${list}</ul>` : ""}
<p><small>A clean audit means every stored state is well formed and every step was a legal move; it does not show that providers acted or that evidence is true.</small></p>`;
}

export function storyHtml(
  state: OrgState,
  changes: OrgChangeRecord[],
  events: DomainEvent[],
  audit: HistoryAudit | null,
  generatedAt: Date = new Date(),
): string {
  const entries = timeline(events, changes, state);
  const roles = Object.values(state.roles ?? {}).sort((a, b) => a.created_in_revision - b.created_in_revision);
  const devStubs = roles.some((r) => r.binding?.provider === "dev_stub");
  const roleHtml = roles
    .map((r) => {
      const p = roleProvenance(r.role_id, state, changes, events);
      const steps = [...p.cause, ...p.since]
        .map(
          (s) =>
            `<li><span class="k">${esc(s.kind)}</span> ${esc(s.title)}${s.devStub ? ' <span class="tag warn">dev stub, not live</span>' : ""}${
              s.detail ? ` — ${esc(s.detail)}` : ""
            }</li>`,
        )
        .join("");
      return `<section class="role${r.state === "RETIRED" ? " retired" : ""}"><h3>${esc(r.name)} <small>${esc(r.state)} · r${
        r.created_in_revision
      }${r.retired_in_revision ? `–r${r.retired_in_revision}` : ""}</small></h3><ol>${steps || "<li>No recorded cause.</li>"}</ol></section>`;
    })
    .join("");
  return `<!doctype html><html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1">
<title>${esc(state.name)} — evolution to r${state.revision}</title>
<style>
:root{--ink:#1c1f24;--ink2:#4b5059;--ink3:#7c818a;--rule:#e2ddd3;--paper:#fffdf9;--accent:#0f5f63;--ok:#2f7a43;--warn:#a8640a;--bad:#b03a2e}
@media (prefers-color-scheme:dark){:root{--ink:#ece8e0;--ink2:#b8b3aa;--ink3:#868a91;--rule:#2c3036;--paper:#1c1f24;--accent:#5fb7ba;--ok:#6cc183;--warn:#e2a64b;--bad:#e5776b}}
body{margin:0;background:var(--paper);color:var(--ink);font:15px/1.5 ui-sans-serif,system-ui,sans-serif}
main{max-width:860px;margin:0 auto;padding:32px 16px 64px}
h1{margin:0 0 4px;font-size:26px}h2{margin:36px 0 12px;font-size:13px;letter-spacing:.08em;text-transform:uppercase;color:var(--ink3)}
h3{margin:0 0 6px;font-size:15px}small{color:var(--ink3);font-weight:400}
.notice{border:1px solid var(--warn);color:var(--warn);padding:8px 12px;border-radius:6px;margin:16px 0}
ol.tl{list-style:none;margin:0;padding:0}ol.tl>li{display:grid;grid-template-columns:110px minmax(0,1fr);gap:12px;padding:12px 0;border-top:1px solid var(--rule)}
.rev{font:600 15px ui-monospace,monospace;display:grid;gap:4px;align-content:start}
.tag{display:inline-block;font:600 11px ui-sans-serif,system-ui;padding:1px 6px;border-radius:4px;border:1px solid var(--rule);color:var(--accent)}
.tag.warn{color:var(--warn);border-color:var(--warn)}.tag.bad{color:var(--bad);border-color:var(--bad)}
li.refusal>div:last-child{border-left:2px solid var(--bad);padding-left:10px}li.refusal ul{color:var(--bad)}
ul{margin:6px 0;padding-left:18px}p{margin:4px 0;color:var(--ink2)}
.deltas li.role_added::marker{content:"+ ";color:var(--ok)}.deltas li.role_retired{text-decoration:line-through}.deltas li.role_retired::marker{content:"− ";color:var(--bad)}
.audit li.ok::marker{content:"✓ ";color:var(--ok)}.audit li.bad::marker{content:"✗ ";color:var(--bad)}
.role{border:1px solid var(--rule);border-radius:6px;padding:10px 12px;margin:10px 0}.role.retired{opacity:.7}
.role ol{margin:0;padding-left:18px;font-size:14px}.k{font:11px ui-monospace,monospace;color:var(--ink3);text-transform:uppercase}
footer{margin-top:40px;font-size:12px;color:var(--ink3)}
</style></head><body><main>
<h1>${esc(state.name)}</h1>
<p>How the organization rewrote itself, from founding to r${state.revision}. Every line cites recorded kernel evidence.</p>
${devStubs ? '<div class="notice">Development stubs stood in for some providers. Roles labeled DEV STUB are not live Elastic, Fetch or CREAO agents.</div>' : ""}
<h2>Evolution</h2><ol class="tl">${entries.map(entryHtml).join("") || "<li>No structural changes.</li>"}</ol>
${auditHtml(audit)}
<h2>Why each role exists</h2>${roleHtml}
<footer>Exported from the GENESIS console at ${esc(generatedAt.toISOString())} · org ${esc(state.org_id)} · r${state.revision}</footer>
</main></body></html>`;
}

export function downloadStory(
  state: OrgState,
  changes: OrgChangeRecord[],
  events: DomainEvent[],
  audit: HistoryAudit | null,
): void {
  const blob = new Blob([storyHtml(state, changes, events, audit)], { type: "text/html" });
  const url = URL.createObjectURL(blob);
  const a = document.createElement("a");
  a.href = url;
  a.download = `genesis-${state.org_id}-r${state.revision}.html`;
  document.body.appendChild(a);
  a.click();
  a.remove();
  setTimeout(() => URL.revokeObjectURL(url), 1000);
}
