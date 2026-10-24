import { useState } from "react";
import type { Delta, Generation, Refusal, TimelineEntry } from "../lib/evolution";

const MARK: Record<Delta["kind"], string> = {
  role_added: "+",
  role_retired: "−",
  duty_moved: "⇄",
  capability: "◆",
  contractor: "↗",
  reporting: "↳",
};

function DeltaChip({ d }: { d: Delta }) {
  return (
    <li className={`evo-delta evo-${d.kind}`}>
      <span className="evo-mark" aria-hidden="true">
        {MARK[d.kind]}
      </span>
      <span>{d.text}</span>
      {d.devStub && <span className="chip warn">DEV STUB{d.provider ? ` · for ${d.provider}` : ""}</span>}
      {!d.devStub && d.provider && d.kind === "role_added" && <span className="muted">{d.provider}</span>}
    </li>
  );
}

const REFUSED_LABEL: Record<Refusal["by"], string> = { kernel: "Refused", provider: "Failed", planner: "Escalated" };

function RefusalRow({ r }: { r: Refusal }) {
  return (
    <li className={`evo-gen evo-refusal by-${r.by}`}>
      <div className="evo-rail">
        <span className="mono">r{r.revision}</span>
        <span className={`chip ${r.by === "kernel" ? "bad" : "warn"}`}>{REFUSED_LABEL[r.by]}</span>
        <span className="muted">stayed at r{r.revision}</span>
      </div>
      <div className="evo-body">
        <div className="evo-cause">{r.title}</div>
        {r.rationale && <div className="evo-why">“{r.rationale}”</div>}
        {r.reasons.length > 0 && (
          <ul className="evo-reasons">
            {r.reasons.map((x) => (
              <li key={x}>{x}</li>
            ))}
          </ul>
        )}
        <div className="evo-foot">
          {r.changeId && <span className="mono">{r.changeId}</span>}
          {r.submittedBy && <span>by {r.submittedBy}</span>}
          {r.operations.length > 0 && <span className="mono">{r.operations.join(" · ")}</span>}
        </div>
      </div>
    </li>
  );
}

/**
 * The organization's history as a sequence of generations: each revision in
 * which its shape changed, why, and what changed. Selecting one shows the
 * tree as committed at that revision.
 */
export function EvolutionView({
  entries,
  selected,
  onSelect,
}: {
  entries: TimelineEntry[];
  selected: number | null;
  onSelect: (revision: number) => void;
}) {
  const [showRefusals, setShowRefusals] = useState(true);
  const gens = entries.filter((e): e is { kind: "generation" } & Generation => e.kind === "generation");
  const refusals = entries.length - gens.length;
  if (entries.length === 0) return <p className="empty">No structural changes yet.</p>;
  const peak = Math.max(1, ...gens.map((g) => g.activeRoles));
  return (
    <>
      <div className="evo-summary">
        <span>
          {gens.length} generation{gens.length === 1 ? "" : "s"}
          {refusals > 0 && ` · ${refusals} change${refusals === 1 ? "" : "s"} that did not happen`}
        </span>
        {refusals > 0 && (
          <label>
            <input type="checkbox" checked={showRefusals} onChange={(e) => setShowRefusals(e.target.checked)} />
            Show refused and failed changes
          </label>
        )}
      </div>
      <ol className="evo" aria-label="Organization evolution">
        {entries.map((e) =>
          e.kind === "refusal" ? (
            showRefusals ? (
              <RefusalRow key={`x${e.seq}-${e.changeId ?? ""}`} r={e} />
            ) : null
          ) : (
            <GenerationRow key={e.revision} g={e} peak={peak} selected={selected} onSelect={onSelect} />
          ),
        )}
      </ol>
    </>
  );
}

function GenerationRow({
  g,
  peak,
  selected,
  onSelect,
}: {
  g: Generation;
  peak: number;
  selected: number | null;
  onSelect: (revision: number) => void;
}) {
  return (
    <li key={g.revision} className={`evo-gen${selected === g.revision ? " selected" : ""}`}>
      <div className="evo-rail">
        <span className="mono">r{g.revision}</span>
        <span className={`chip evo-label ${g.label.toLowerCase()}`}>{g.label}</span>
        <span
          className="evo-bar"
          style={{ ["--w" as string]: `${(g.activeRoles / peak) * 100}%` }}
          title={`${g.activeRoles} active roles after r${g.revision}`}
        >
          <span />
        </span>
        <span className="muted tabular">{g.activeRoles} roles</span>
      </div>
      <div className="evo-body">
        {g.cause && <div className="evo-cause">{g.cause}</div>}
        {g.triggers.length > 0 && (
          <div className="evo-why">
            <span className="muted">because</span> {g.triggers.join("; ")}
          </div>
        )}
        <ul className="evo-deltas">
          {g.deltas.map((d, i) => (
            <DeltaChip key={i} d={d} />
          ))}
        </ul>
        <div className="evo-foot">
          {g.changeId && <span className="mono">{g.changeId}</span>}
          {g.submittedBy && <span>by {g.submittedBy}</span>}
          {g.plannerModel && <span>planner {g.plannerModel}</span>}
          <button
            className="btn"
            onClick={() => onSelect(g.revision)}
            aria-label={`Show the organization at r${g.revision}`}
          >
            View r{g.revision}
          </button>
        </div>
      </div>
    </li>
  );
}
