import { useMemo, useState } from "react";
import { Api, ApiError } from "../lib/api";
import type { DomainEvent, Job, OrgChangeRecord, OrgState } from "../lib/contracts";
import {
  attention,
  AttentionKind,
  counts,
  isOpen,
  ReconcileTarget,
  validAgentId,
} from "../lib/attention";

const LABEL: Record<AttentionKind, { text: string; tone: string; title: string }> = {
  action: { text: "NEEDS YOU", tone: "bad", title: "Blocked until an operator decides" },
  unknown: { text: "UNKNOWN", tone: "warn", title: "Outcome unknown; reconcile before retrying" },
  waiting: { text: "UNPROVEN", tone: "accent", title: "Progressing, but not proven yet" },
  refused: { text: "BLOCKED", tone: "neutral", title: "The kernel refused this request" },
  failed: { text: "FAILED", tone: "bad", title: "Ended FAILED; provider IDs are kept" },
};

/** Collapsed view keeps every open item plus a few recent history items. */
const RECENT = 4;

function Reconcile({ api, target }: { api: Api; target: ReconcileTarget }) {
  const [agent, setAgent] = useState("");
  const [reason, setReason] = useState("");
  const [confirming, setConfirming] = useState(false);
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState<{ bad: boolean; text: string } | null>(null);
  const base = `/org-changes/${encodeURIComponent(target.change_id)}/operations/${encodeURIComponent(target.role_key)}`;
  const agentId = agent.trim();
  const agentOk = validAgentId(agentId);
  const reasonOk = reason.trim().length > 0;

  async function send(path: string, body: unknown, done: string) {
    setBusy(true);
    setMessage(null);
    try {
      await api.post(base + path, body);
      setMessage({ bad: false, text: done });
    } catch (e) {
      setMessage({ bad: true, text: e instanceof ApiError ? `${e.code}: ${e.message}` : String(e) });
    } finally {
      setBusy(false);
      setConfirming(false);
    }
  }

  return (
    <form
      className="reconcile"
      onSubmit={(e) => {
        e.preventDefault();
        if (agentOk && reasonOk) {
          void send(
            "/attach",
            { provider_agent_id: agentId, reason: reason.trim() },
            "Attached. The role stays inactive until readiness is proven.",
          );
        }
      }}
    >
      <div className="reconcile-fields">
        <label>
          Agent ID found in {target.provider}
          <input
            value={agent}
            onChange={(e) => setAgent(e.target.value)}
            placeholder="provider agent ID"
            spellCheck={false}
            aria-invalid={agent !== "" && !agentOk}
          />
        </label>
        <label>
          Reason (recorded)
          <input value={reason} onChange={(e) => setReason(e.target.value)} placeholder="e.g. found in CREAO console" />
        </label>
      </div>
      <div className="row">
        <button className="btn" type="submit" disabled={busy || !agentOk || !reasonOk}>
          Attach agent
        </button>
        {confirming ? (
          <>
            <button
              className="btn danger"
              type="button"
              disabled={busy || !reasonOk}
              onClick={() =>
                void send(
                  "/abandon",
                  { reason: reason.trim() },
                  "Abandoned. The change failed; provider IDs were kept for cleanup.",
                )
              }
            >
              Confirm abandon
            </button>
            <button className="btn" type="button" onClick={() => setConfirming(false)}>
              Cancel
            </button>
          </>
        ) : (
          <button className="btn" type="button" disabled={busy || !reasonOk} onClick={() => setConfirming(true)}>
            Abandon…
          </button>
        )}
      </div>
      <span className="muted hint">
        Attaching never re-creates the agent or retries the lost request. Abandoning fails the change.
      </span>
      {message && (
        <span className={message.bad ? "reconcile-error" : "muted"} role="status">
          {message.text}
        </span>
      )}
    </form>
  );
}

export function AttentionPanel({
  api,
  changes,
  jobs,
  events,
  state,
}: {
  api: Api;
  changes: OrgChangeRecord[];
  jobs: Job[];
  events: DomainEvent[];
  state: OrgState | null;
}) {
  const items = useMemo(() => attention(changes, jobs, events, state), [changes, jobs, events, state]);
  const [expanded, setExpanded] = useState(false);
  if (items.length === 0) return null;
  const c = counts(items);
  const open = items.filter(isOpen).length;
  const shown = expanded ? items : items.slice(0, open + RECENT);
  const summary = [
    c.action && `${c.action} need${c.action === 1 ? "s" : ""} you`,
    c.unknown && `${c.unknown} unknown`,
    c.waiting && `${c.waiting} unproven`,
    c.refused && `${c.refused} blocked`,
    c.failed && `${c.failed} failed`,
  ]
    .filter(Boolean)
    .join(" · ");
  return (
    <section className="panel attention" aria-labelledby="attention-h">
      <header>
        <h2 id="attention-h">Attention</h2>
        <span className="muted tabular">{summary}</span>
        {items.length > open + RECENT && (
          <div className="right">
            <button className="btn" onClick={() => setExpanded((v) => !v)} aria-expanded={expanded}>
              {expanded ? "Show recent" : `Show all ${items.length}`}
            </button>
          </div>
        )}
      </header>
      <div className="panel-body">
        <ol className="events">
          {shown.map((i) => {
            const label = LABEL[i.kind];
            return (
              <li key={i.key}>
                <span className="seq">{i.seq ?? ""}</span>
                <div>
                  <div className="title">
                    <span className={`chip ${label.tone}`} title={label.title}>
                      {label.text}
                    </span>
                    {i.title}
                  </div>
                  {i.detail && <div className="detail">{i.detail}</div>}
                  {i.refs.length > 0 && (
                    <div className="refs">
                      {i.refs.map((r) => (
                        <code key={r}>{r}</code>
                      ))}
                    </div>
                  )}
                  {i.reconcile && <Reconcile api={api} target={i.reconcile} />}
                </div>
              </li>
            );
          })}
        </ol>
      </div>
    </section>
  );
}
