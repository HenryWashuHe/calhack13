import type { Delta, Generation } from "../lib/evolution";

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

/**
 * The organization's history as a sequence of generations: each revision in
 * which its shape changed, why, and what changed. Selecting one shows the
 * tree as committed at that revision.
 */
export function EvolutionView({
  gens,
  selected,
  onSelect,
}: {
  gens: Generation[];
  selected: number | null;
  onSelect: (revision: number) => void;
}) {
  if (gens.length === 0) return <p className="empty">No structural changes yet.</p>;
  const peak = Math.max(1, ...gens.map((g) => g.activeRoles));
  return (
    <ol className="evo" aria-label="Organization evolution">
      {gens.map((g) => (
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
              <button className="btn" onClick={() => onSelect(g.revision)} aria-label={`Show the organization at r${g.revision}`}>
                View r{g.revision}
              </button>
            </div>
          </div>
        </li>
      ))}
    </ol>
  );
}
