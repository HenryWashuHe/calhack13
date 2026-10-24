import { useEffect, useMemo, useState } from "react";
import { Api, orgsFromEnv, OrgEndpoint } from "./lib/api";
import { Health, useOrg } from "./lib/useOrg";
import { MissionPanel } from "./components/MissionPanel";
import { OrgPanel } from "./components/OrgPanel";
import { EvidencePanel } from "./components/EvidencePanel";
import { Deck } from "./components/Deck";
import { AttentionPanel } from "./components/AttentionPanel";

function OrgTab({ ep, selected, onSelect }: { ep: OrgEndpoint; selected: boolean; onSelect: () => void }) {
  // Lightweight health poll for the tab badge; the selected org has its own live stream.
  const [health, setHealth] = useState<Health | null>(null);
  const [ok, setOk] = useState<boolean | null>(null);
  useEffect(() => {
    const api = new Api(ep);
    let cancelled = false;
    const tick = () =>
      api
        .get<Health>("/health")
        .then((h) => !cancelled && (setHealth(h), setOk(true)))
        .catch(() => !cancelled && setOk(false));
    void tick();
    const id = window.setInterval(tick, 3000);
    return () => {
      cancelled = true;
      window.clearInterval(id);
    };
  }, [ep]);
  const dot = ok === null ? "connecting" : ok ? "live" : "offline";
  return (
    <button className="org-tab" role="tab" aria-selected={selected} onClick={onSelect}>
      <span className="label">
        <span className={`dot ${dot}`} aria-hidden />
        {ep.label}
      </span>
      <span className="sub">
        {health?.name ?? ep.url} · r{health?.revision ?? "–"}
      </span>
    </button>
  );
}

function Console({ ep }: { ep: OrgEndpoint }) {
  const view = useOrg(ep);
  const stubbed = useMemo(
    () => Object.values(view.state?.roles ?? {}).some((r) => r.binding?.provider === "dev_stub"),
    [view.state],
  );
  return (
    <>
      {(stubbed || view.health?.dev_stub_enabled) && (
        <div style={{ padding: "8px max(16px, env(safe-area-inset-left)) 0" }}>
          <div className="notice">
            Development stubs are enabled for {ep.label}. Bindings labeled DEV STUB are stand-ins, not live
            Elastic, Fetch or CREAO integrations.
          </div>
        </div>
      )}
      {view.error && (
        <div style={{ padding: "8px max(16px, env(safe-area-inset-left)) 0" }}>
          <div className="notice bad">
            Cannot reach {ep.label} at {ep.url}: {view.error}. Start the kernel with{" "}
            <span className="mono">cargo run -p genesis-server -- dev</span>.
          </div>
        </div>
      )}
      <main className="layout">
        <MissionPanel state={view.state} api={view.api} />
        <OrgPanel api={view.api} state={view.state} changes={view.changes} />
        <AttentionPanel api={view.api} changes={view.changes} jobs={view.jobs} events={view.events} state={view.state} />
        <EvidencePanel events={view.events} state={view.state} />
        <Deck changes={view.changes} jobs={view.jobs} state={view.state} events={view.events} ledger={view.ledger} />
      </main>
    </>
  );
}

export default function App() {
  const orgs = useMemo(() => orgsFromEnv(), []);
  const [selected, setSelected] = useState(orgs[0]?.id ?? "");
  const ep = orgs.find((o) => o.id === selected) ?? orgs[0];
  return (
    <>
      <nav className="topbar">
        <div className="brand">
          <img src="/genesis.svg" alt="" />
          GENESIS
        </div>
        <div className="org-tabs" role="tablist" aria-label="Organizations">
          {orgs.map((o) => (
            <OrgTab key={o.id} ep={o} selected={o.id === ep?.id} onSelect={() => setSelected(o.id)} />
          ))}
        </div>
        <span className="spacer" />
      </nav>
      {ep ? <Console key={ep.id} ep={ep} /> : <p className="empty">No organizations configured.</p>}
    </>
  );
}
