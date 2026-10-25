// Eric's Claude: method exchange records in the lineage view
// (docs/invention-lab/ui.md#method-exchange-records). One row per
// publisher-qualified exact method, with discovery, fetched bytes and local
// decisions in separate columns. Read-only: no registry call, no control that
// publishes, fetches or adopts.

import { Chip } from "./AttemptCard";
import { adoptionLabel } from "./claims";
import type { Exchange, ExchangeEntry } from "./exchange";

export const EXCHANGE_LIMIT =
  "Each file is a registry response an organization kept: a record of what was returned, not proof of who sent it. " +
  "The publisher is the registry account that submitted the bytes, not their author. Being discovered, being fetched " +
  "and a local decision are separate; none implies the next, and none is an improvement claim.";

function Discovered({ e }: { e: ExchangeEntry }) {
  if (e.discovered.length === 0) return <>not in a supplied search result</>;
  const c = e.discovered[0]!;
  return (
    <>
      search hit (metadata only, no source): “{c.intent}”; for {c.applicability.join("; ") || "no stated applicability"}; runtime{" "}
      <code>{c.runtime.id}</code>, protocol <code>{c.protocol.id}</code>, {c.dependencies.length} dependencies
    </>
  );
}

function Fetched({ e }: { e: ExchangeEntry }) {
  if (e.fetched.length === 0) return <>no offer supplied</>;
  return (
    <>
      {e.fetched.map((f, i) =>
        f.package ? (
          <div key={i}>
            bytes are exactly this reference (checked in this browser): entrypoint <code>{f.package.entrypoint}</code>,{" "}
            {f.package.files.length} file{f.package.files.length === 1 ? "" : "s"}
          </div>
        ) : (
          <div key={i}>bytes not linked: {f.mismatch}</div>
        ),
      )}
    </>
  );
}

function InRecords({ e }: { e: ExchangeEntry }) {
  if (!e.fetched.some((f) => f.package)) return <>not linked (no matching fetched bytes)</>;
  return e.node ? <>same exact method as in the lineage above</> : <>no other supplied record names this method</>;
}

export function ExchangePanel({ exchange }: { exchange: Exchange }) {
  if (exchange.entries.length === 0 && exchange.searches === 0) return null;
  return (
    <section className="lab-exchange">
      <h3>Method exchange records</h3>
      <p className="lab-limit">{EXCHANGE_LIMIT}</p>
      {exchange.searches > 0 && (
        <p className="lab-limit">
          {exchange.searches} search result{exchange.searches === 1 ? "" : "s"} supplied
          {exchange.truncated > 0 ? `, ${exchange.truncated} truncated (more matches existed)` : ""}. An untruncated result
          is not proof that nothing else matches.
        </p>
      )}
      {exchange.entries.length > 0 && (
        <table>
          <thead>
            <tr>
              <th scope="col">Publisher and exact method</th>
              <th scope="col">Discovered</th>
              <th scope="col">Fetched</th>
              <th scope="col">In these records</th>
              <th scope="col">Local decisions</th>
            </tr>
          </thead>
          <tbody>
            {exchange.entries.map((e) => (
              <tr key={`${e.publisher} ${e.reference.sha256} ${e.reference.id} ${e.reference.version}`}>
                <th scope="row">
                  {e.publisher}: <code>{`${e.reference.id}@${e.reference.version} ${e.reference.sha256.slice(0, 12)}`}</code>
                </th>
                <td>
                  <Discovered e={e} />
                </td>
                <td>
                  <Fetched e={e} />
                </td>
                <td>
                  <InRecords e={e} />
                </td>
                <td>
                  {e.decisions.length === 0
                    ? "none recorded"
                    : e.decisions.map((d, i) => (
                        <div key={i}>
                          {d.org_id}: <Chip label={adoptionLabel(d.outcome)} />
                        </div>
                      ))}
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
      {exchange.problems.length > 0 && (
        <>
          <h4>Problems in the exchange records</h4>
          <ul>
            {exchange.problems.map((p) => (
              <li key={p}>{p}</li>
            ))}
          </ul>
        </>
      )}
    </section>
  );
}
