// Eric's Claude: every generation attempt with an arithmetic total
// (docs/invention-lab/ui.md#attempt-ledger). Presentational; not wired into
// the console.

import { AttemptCard } from "./AttemptCard";
import { phaseLabel, TIER_TEXT } from "./claims";
import { buildLedger, totalText } from "./ledger";
import type { Attempt } from "./lineage";
import { USAGE_DIMENSIONS } from "./records";

export function AttemptLedger({ attempts }: { attempts: Attempt[] }) {
  if (attempts.length === 0) return <p>No generation attempts recorded in the supplied files.</p>;
  const ledger = buildLedger(attempts);
  const phases = Object.entries(ledger.byPhase)
    .map(([phase, n]) => `${n} ${phaseLabel(phase).text}`)
    .join(" · ");
  return (
    <section className="lab-ledger">
      <h3>Generation attempts</h3>
      <p>
        {ledger.attempts} attempt{ledger.attempts === 1 ? "" : "s"} · {phases} · {ledger.packages} package
        {ledger.packages === 1 ? "" : "s"}
      </p>
      <p>evidence: {ledger.tiers.map((t) => TIER_TEXT[t]).join("; ")}</p>
      <table>
        <caption>Arithmetic total across separately declared attempts, including failures</caption>
        <tbody>
          {USAGE_DIMENSIONS.map((d) => (
            <tr key={d}>
              <th scope="row">{d}</th>
              <td>{totalText(ledger.totals[d])}</td>
            </tr>
          ))}
        </tbody>
      </table>
      <p className="lab-limit">Not a predeclared combined budget, a cost per success or a comparison.</p>
      {attempts.map((a) => (
        <AttemptCard key={a.status.operation_id} attempt={a} />
      ))}
    </section>
  );
}
