import { createContext, useContext, useEffect, useMemo, useRef, useState } from "react";
import type { Api } from "../lib/api";
import type { DomainEvent, OrgChangeRecord, OrgState } from "../lib/contracts";
import { buildTree, executedBy, ownedBy, pendingRoles, RoleNode, short } from "../lib/graph";
import { HistoryEntry, historyView } from "../lib/snapshot";
import { ProviderBadge, StateChip } from "./Chips";
import { WhyDrawer } from "./WhyDrawer";
import { EvolutionView } from "./EvolutionView";
import { generations, timeline } from "../lib/evolution";
import { AuditStrip } from "./AuditStrip";
import { downloadStory } from "../lib/story";
import type { HistoryAudit } from "../lib/audit";
import { gapsForRole } from "../lib/gaps";
import { shortLabel } from "../lib/capabilities";
import { GapIntakeContext } from "../lib/gapIntake";
import { GapFinalizationContext } from "../lib/gapFinalization";
import { GapAdoptionContext } from "../lib/gapAdoption";

/** Records the "Why?" drawer explains a role from. Absent → no drawer. */
const Provenance = createContext<{ changes: OrgChangeRecord[]; events: DomainEvent[] } | null>(null);

function WhyButton({ open, onToggle }: { open: boolean; onToggle: () => void }) {
  if (!useContext(Provenance)) return null;
  return (
    <button
      className="btn why-btn"
      aria-expanded={open}
      aria-label={open ? "Hide why this role exists" : "Why does this role exist?"}
      onClick={onToggle}
    >
      {open ? "Hide why" : "Why?"}
    </button>
  );
}

function Why({ open, roleId, state }: { open: boolean; roleId: string; state: OrgState }) {
  const records = useContext(Provenance);
  if (!open || !records) return null;
  return <WhyDrawer roleId={roleId} state={state} changes={records.changes} events={records.events} />;
}

function RequestedCapabilities({ state, roleId }: { state: OrgState; roleId: string }) {
  const intake = useContext(GapIntakeContext);
  const finalization = useContext(GapFinalizationContext);
  const adoption = useContext(GapAdoptionContext);
  const gaps = gapsForRole(state, roleId, intake, finalization, adoption);
  if (gaps.length === 0) return null;
  return (
    <ul className="duties gaps">
      {gaps.map((g) => (
        <li key={g.gapId} title={`Requested by this organization's change; sources: ${g.sources.join(", ")}`}>
          <span
            className="chip provider dashed"
            style={{ ["--pc" as string]: "var(--p-apify)" }}
            title={g.version ?? g.capabilityId}
          >
            {shortLabel(g.version ?? g.capabilityId)}
          </span>
          <span className={`chip ${g.tone}`}>{g.label}</span>
          <span>{g.purpose}</span>
          {g.detail && <span className="muted">{g.detail}</span>}
        </li>
      ))}
    </ul>
  );
}

function Duties({ state, roleId }: { state: OrgState; roleId: string }) {
  const owned = ownedBy(state, roleId);
  const executing = executedBy(state, roleId).filter((r) => !owned.includes(r));
  if (owned.length === 0 && executing.length === 0) return null;
  return (
    <ul className="duties">
      {owned.map((r) => {
        const contract = r.executor?.kind === "contractor" ? state.contracts?.[r.executor.id] : undefined;
        return (
          <li key={r.responsibility_id}>
            <span className="chip accent" title="Accountable owner">OWNS</span>
            <span>{r.purpose}</span>
            <span className="epoch">epoch {r.epoch}</span>
            {contract && (
              <div className="contractor" style={{ width: "100%" }}>
                <span className="chip provider" style={{ ["--pc" as string]: "var(--p-fetch)" }}>
                  fetch specialist
                </span>
                <span className="mono">{contract.specialist_address}</span>
                <StateChip state={contract.state} />
                <span className="muted">{contract.scope}</span>
              </div>
            )}
          </li>
        );
      })}
      {executing.map((r) => (
        <li key={r.responsibility_id}>
          <span className="chip neutral" title="Delegated executor; accountability stays with the owner">
            EXECUTES
          </span>
          <span>{r.purpose}</span>
        </li>
      ))}
    </ul>
  );
}

function Node({ node, state, freshRevision }: { node: RoleNode; state: OrgState; freshRevision: number }) {
  if (node.kind === "pending") {
    const p = node.pending;
    return (
      <li>
        <div className="role pending" aria-label={`${p.name}, not yet active`}>
          <div className="role-head">
            <span className="role-name">{p.name}</span>
            <StateChip state={p.change_status === "READY" ? "READY" : "PROVISIONING"} />
            <ProviderBadge provider={p.provider} binding={p.binding} />
          </div>
          <div className="role-charter">{p.charter}</div>
          {p.operation && (
            <div className="role-meta">
              <span className="chip neutral" title={`Provider operation ${p.operation.operation_id}`}>
                {p.operation.state.replace(/_/g, " ")}
              </span>
              {p.operation.run_id && <span className="mono">run {p.operation.run_id}</span>}
              {p.operation.native_status === "operator_attached" && (
                <span className="chip warn" title="Agent ID supplied by an operator; the provider never reported it">
                  attached by operator
                </span>
              )}
              {p.operation.attention && <span className="muted">{p.operation.attention}</span>}
              {p.operation.readiness_evidence && (
                <span className="chip ok" title="Backend-observed readiness evidence">
                  readiness {p.operation.readiness_evidence}
                </span>
              )}
            </div>
          )}
          <div className="role-meta">
            <span>Not executable until the change commits</span>
            <span className="mono">{short(p.change_id)}</span>
            {p.binding && <span className="mono">{p.binding.provider_agent_id}</span>}
          </div>
        </div>
        {node.children.length > 0 && (
          <ul>
            {node.children.map((c) => (
              <Node key={c.id} node={c} state={state} freshRevision={freshRevision} />
            ))}
          </ul>
        )}
      </li>
    );
  }
  const r = node.role;
  const retired = r.state === "RETIRED";
  const fresh = !retired && r.created_in_revision === freshRevision && freshRevision > 1;
  const scopes = r.authority.scopes ?? [];
  const [why, setWhy] = useState(false);
  return (
    <li>
      <div className={`role${retired ? " retired" : ""}${fresh ? " fresh" : ""}`}>
        <div className="role-head">
          <span className="role-name">{r.name}</span>
          <StateChip state={r.state} />
          <ProviderBadge binding={r.binding} />
          {fresh && <span className="chip accent">NEW IN r{r.created_in_revision}</span>}
          <WhyButton open={why} onToggle={() => setWhy((w) => !w)} />
        </div>
        <div className="role-charter">{r.charter}</div>
        <Duties state={state} roleId={r.role_id} />
        <RequestedCapabilities state={state} roleId={r.role_id} />
        {(r.capabilities ?? []).length > 0 && (
          <div className="role-meta">
            {(r.capabilities ?? []).map((c) => (
              <span
                key={`${c.capability_id}@${c.version}`}
                className="chip provider"
                style={{ ["--pc" as string]: "var(--p-apify)" }}
                title={`${c.capability_id}@${c.version}`}
              >
                {shortLabel(`${c.capability_id}@${c.version}`)}
              </span>
            ))}
          </div>
        )}
        <div className="role-meta">
          {scopes.length > 0 && <span className="mono">{scopes.join(" · ")}</span>}
          {(r.authority.budget_cents ?? 0) > 0 && (
            <span className="tabular">${((r.authority.budget_cents ?? 0) / 100).toLocaleString()}</span>
          )}
          {r.binding && <span className="mono" title="Provider agent ID">{r.binding.provider_agent_id}</span>}
          <span>
            r{r.created_in_revision}
            {r.retired_in_revision ? ` → retired r${r.retired_in_revision}` : ""}
          </span>
        </div>
        <Why open={why} roleId={r.role_id} state={state} />
      </div>
      {node.children.length > 0 && (
        <ul>
          {node.children.map((c) => (
            <Node key={c.id} node={c} state={state} freshRevision={freshRevision} />
          ))}
        </ul>
      )}
    </li>
  );
}

export function OrgPanel({
  api,
  state,
  changes,
  events,
}: {
  api: Api;
  state: OrgState | null;
  changes: OrgChangeRecord[];
  /** Full event history; enables the per-role "Why?" explanation. */
  events?: DomainEvent[];
}) {
  const [showRetired, setShowRetired] = useState(true);
  const [viewRevision, setViewRevision] = useState<number | null>(null);
  const [entry, setEntry] = useState<HistoryEntry | null>(null);
  const [mode, setMode] = useState<"tree" | "evolution">("tree");
  /** Index into `gens` while replaying the organization's evolution. */
  const [replay, setReplay] = useState<number | null>(null);
  const live = viewRevision === null;
  const gens = useMemo(() => (events ? generations(events, changes, state) : []), [events, changes, state]);
  const entries = useMemo(
    () => (events && mode === "evolution" ? timeline(events, changes, state) : []),
    [events, changes, state, mode],
  );
  const replaying = replay !== null ? gens[replay] : undefined;
  // The replay timer is keyed to the step being shown, not to the generation
  // array or sequence: ordinary event traffic re-derives `gens`, and new
  // structural generations append to it, and neither may restart the step's
  // deadline. When the timer fires it reads the latest generations, so replay
  // continues into generations that arrived while it was running.
  const gensRef = useRef(gens);
  gensRef.current = gens;
  const stepRevision = replaying?.revision;

  useEffect(() => {
    if (replay === null) return;
    if (stepRevision === undefined) {
      setReplay(null);
      return;
    }
    setViewRevision(stepRevision);
    const t = setTimeout(() => {
      if (replay + 1 < gensRef.current.length) setReplay(replay + 1);
      else {
        setReplay(null);
        setViewRevision(null);
      }
    }, 2400);
    return () => clearTimeout(t);
  }, [replay, stepRevision]);

  const stopReplay = () => setReplay(null);

  useEffect(() => {
    // Drop the previous revision's snapshot immediately; it is never shown
    // under another revision's label.
    setEntry(null);
    if (viewRevision === null) return;
    let cancelled = false;
    api
      .get<OrgState>(`/organization/revisions/${viewRevision}`)
      .then((s) => {
        if (!cancelled) setEntry({ revision: viewRevision, state: s });
      })
      .catch((e: unknown) => {
        if (!cancelled) setEntry({ revision: viewRevision, error: e instanceof Error ? e.message : String(e) });
      });
    return () => {
      cancelled = true;
    };
  }, [api, viewRevision]);

  const history = historyView(viewRevision, entry);
  const shown = history.kind === "live" ? state : history.kind === "ready" ? history.state : null;
  const pending = useMemo(() => (live ? pendingRoles(changes, state) : []), [live, changes, state]);
  const tree = useMemo(
    () => (shown ? buildTree(shown, pending, { showRetired }) : []),
    [shown, pending, showRetired],
  );
  const current = state?.revision ?? 0;
  const rolesCount = Object.values(shown?.roles ?? {}).filter((r) => r.state === "ACTIVE").length;

  return (
    <section className="panel" aria-labelledby="org-h">
      <header>
        <h2 id="org-h">Organization</h2>
        <span className="muted tabular">
          r{shown?.revision ?? "–"} · {rolesCount} active roles
        </span>
        <div className="right org-tools">
          {events && (
            <div className="seg" role="group" aria-label="Organization view">
              <button className="btn" aria-pressed={mode === "tree"} onClick={() => setMode("tree")}>
                Tree
              </button>
              <button className="btn" aria-pressed={mode === "evolution"} onClick={() => setMode("evolution")}>
                Evolution
              </button>
            </div>
          )}
          {gens.length > 1 && (
            <button
              className="btn"
              aria-pressed={replaying !== undefined}
              onClick={() => {
                if (replaying) {
                  stopReplay();
                  setViewRevision(null);
                } else {
                  setMode("tree");
                  setReplay(0);
                }
              }}
            >
              {replaying ? "■ Stop" : "▶ Replay"}
            </button>
          )}
          {events && state && (
            <button
              className="btn"
              title="Download a self-contained HTML record of this organization's evolution"
              onClick={async () => {
                const audit = await api.get<HistoryAudit>("/organization/audit").catch(() => null);
                downloadStory(state, changes, events, audit);
              }}
            >
              Export story
            </button>
          )}
          <label>
            <input type="checkbox" checked={showRetired} onChange={(e) => setShowRetired(e.target.checked)} />
            Show retired
          </label>
        </div>
      </header>
      <div className="panel-body">
        <div className="org-tools">
          <div className="scrubber">
            <span className="muted" style={{ fontSize: 12 }}>
              Revision
            </span>
            <input
              type="range"
              min={0}
              max={Math.max(current, 0)}
              value={viewRevision ?? current}
              onChange={(e) => {
                stopReplay();
                const v = Number(e.target.value);
                setViewRevision(v === current ? null : v);
              }}
              aria-label="Organization revision"
            />
            <span className="mono tabular">r{viewRevision ?? current}</span>
          </div>
          <button
            className="btn"
            aria-pressed={live}
            onClick={() => {
              stopReplay();
              setViewRevision(null);
            }}
          >
            Live
          </button>
        </div>
        {!live && replaying && (
          <div className="history-banner replay" aria-live="polite">
            <strong>
              r{replaying.revision} · {replaying.label}
            </strong>{" "}
            {replaying.cause}
            {replaying.triggers.length > 0 && <span className="muted"> — because {replaying.triggers.join("; ")}</span>}
          </div>
        )}
        {!live && !replaying && (
          <div className="history-banner">
            Viewing revision r{viewRevision} as it was committed. In-flight changes are hidden.
          </div>
        )}
        {/* Mounted on `state`, not `shown`: `shown` is null while a historical
            revision loads, and a remount would rerun the whole-history audit on
            every replay step or slider move. */}
        {state && (
          <AuditStrip
            api={api}
            shown={shown?.revision ?? viewRevision ?? current}
            current={current}
            onSelect={(rev) => {
              stopReplay();
              setViewRevision(rev);
            }}
          />
        )}
        {mode === "evolution" && (
          <EvolutionView
            entries={entries}
            selected={viewRevision}
            onSelect={(rev) => {
              stopReplay();
              setViewRevision(rev === current ? null : rev);
              setMode("tree");
            }}
          />
        )}
        {history.kind === "loading" && <p className="empty">Loading revision r{history.revision}…</p>}
        {history.kind === "error" && (
          <div className="notice bad" role="alert">
            Could not load revision r{history.revision}: {history.message}
          </div>
        )}
        {mode === "tree" && shown && tree.length === 0 && <p className="empty">No roles yet.</p>}
        {mode === "tree" && shown && (
          <Provenance.Provider value={live && events ? { changes, events } : null}>
            <ul className="tree">
              {tree.map((n) => (
                <Node key={n.id} node={n} state={shown} freshRevision={viewRevision ?? current} />
              ))}
            </ul>
          </Provenance.Provider>
        )}
      </div>
    </section>
  );
}
