// Eric's Claude: one generation attempt, as recorded (docs/invention-lab/ui.md).
// Presentational only and not yet wired into the console. Shows cost for every
// attempt, including failed ones, and the record's own authority sentence.

import { diagnosticLabel, phaseLabel, TIER_TEXT, type Label } from "./claims";
import type { Attempt } from "./lineage";
import { USAGE_DIMENSIONS, type Usage } from "./records";

export function Chip({ label }: { label: Label }) {
  return (
    <span className={`chip ${label.tone}`} title={label.limit}>
      {label.text}
    </span>
  );
}

export function usageText(u: Usage): string {
  switch (u.kind) {
    case "measured":
      return String(u.value);
    case "unknown":
      return `unknown (${u.reason})`;
    case "not_recorded":
      return "not recorded";
    case "unrecognized":
      return `unrecognized: ${u.raw}`;
  }
}

export function AttemptCard({ attempt }: { attempt: Attempt }) {
  const { status, outcome } = attempt;
  const phase = phaseLabel(status.phase);
  const s = status.settings;
  return (
    <article className="lab-attempt" data-operation={status.operation_id}>
      <header>
        <strong>{status.org_id}</strong> <code>{status.operation_id}</code> <Chip label={phase} />
      </header>
      <p className="lab-limit">{phase.limit}</p>
      {outcome && (
        <p>
          outcome: <code>{outcome.kind}</code>
          {outcome.reason && (
            <>
              {" "}
              / <code>{outcome.reason}</code>
            </>
          )}
        </p>
      )}
      {status.diagnostic && (
        <p className="lab-diagnostic">
          <Chip label={diagnosticLabel(status.diagnostic.rejection, status.diagnostic.stop_reason)} />{" "}
          {diagnosticLabel(status.diagnostic.rejection, status.diagnostic.stop_reason).limit}
        </p>
      )}
      {status.unrecognized_fields.length > 0 && (
        <p>unrecognized fields (not interpreted): {status.unrecognized_fields.join(", ")}</p>
      )}
      <p>evidence: {TIER_TEXT[attempt.tier]}</p>
      <p>
        model {status.actual_model ?? s.model} · sampling {s.sampling} · max output {s.max_tokens}
      </p>
      <table>
        <tbody>
          {USAGE_DIMENSIONS.map((d) => (
            <tr key={d}>
              <th scope="row">{d}</th>
              <td>{status.usage ? usageText(status.usage[d]) : "no completion recorded"}</td>
            </tr>
          ))}
        </tbody>
      </table>
      {attempt.reachedOutputCap && <p>reached the output cap (not by itself a proven failure cause)</p>}
      <p>
        package:{" "}
        {status.package ? (
          <code>
            {status.package.id}@{status.package.version} {status.package.sha256.slice(0, 12)}
          </code>
        ) : (
          "none"
        )}
      </p>
      <p className="lab-authority">{status.authority}</p>
    </article>
  );
}
