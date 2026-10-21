// Live view of one organization: authoritative snapshot plus the ordered event
// log over SSE. The UI only renders persisted state; nothing is animated into
// existence before the backend reports it.

import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { Api, OrgEndpoint } from "./api";
import type { DomainEvent, Job, OrgChangeRecord, OrgState } from "./contracts";

export type Connection = "connecting" | "live" | "offline";

export interface Health {
  org_id: string;
  name: string;
  revision: number;
  dev_stub_enabled: boolean;
  configured_providers: string[];
  registry_configured: boolean;
}

export interface Commitment {
  operation_id: string;
  job_id: string;
  payload: unknown;
  created_at: string;
}

export interface OrgView {
  api: Api;
  health: Health | null;
  state: OrgState | null;
  changes: OrgChangeRecord[];
  jobs: Job[];
  ledger: Commitment[];
  events: DomainEvent[];
  connection: Connection;
  error: string | null;
  refresh: () => void;
}

export function mergeEvents(existing: DomainEvent[], incoming: DomainEvent[]): DomainEvent[] {
  if (incoming.length === 0) return existing;
  const bySeq = new Map<number, DomainEvent>();
  for (const e of existing) bySeq.set(e.seq, e);
  for (const e of incoming) bySeq.set(e.seq, e);
  return [...bySeq.values()].sort((a, b) => a.seq - b.seq);
}

export function useOrg(ep: OrgEndpoint): OrgView {
  const api = useMemo(() => new Api(ep), [ep]);
  const [health, setHealth] = useState<Health | null>(null);
  const [state, setState] = useState<OrgState | null>(null);
  const [changes, setChanges] = useState<OrgChangeRecord[]>([]);
  const [jobs, setJobs] = useState<Job[]>([]);
  const [ledger, setLedger] = useState<Commitment[]>([]);
  const [events, setEvents] = useState<DomainEvent[]>([]);
  const [connection, setConnection] = useState<Connection>("connecting");
  const [error, setError] = useState<string | null>(null);
  const timer = useRef<number | null>(null);

  const load = useCallback(async () => {
    try {
      const [h, s, c, j, l] = await Promise.all([
        api.get<Health>("/health"),
        api.get<OrgState>("/organization"),
        api.get<OrgChangeRecord[]>("/org-changes"),
        api.get<Job[]>("/jobs"),
        api.get<Commitment[]>("/ledger"),
      ]);
      setHealth(h);
      setState(s);
      setChanges(c);
      setJobs(j);
      setLedger(l);
      setError(null);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  }, [api]);

  const scheduleLoad = useCallback(() => {
    if (timer.current !== null) window.clearTimeout(timer.current);
    timer.current = window.setTimeout(() => {
      timer.current = null;
      void load();
    }, 120);
  }, [load]);

  useEffect(() => {
    setHealth(null);
    setState(null);
    setEvents([]);
    setConnection("connecting");
    void load();
    const source = new EventSource(api.eventsUrl(0));
    source.onopen = () => setConnection("live");
    source.onerror = () => setConnection(source.readyState === EventSource.CLOSED ? "offline" : "connecting");
    source.addEventListener("domain_event", (msg) => {
      const ev = JSON.parse((msg as MessageEvent<string>).data) as DomainEvent;
      setEvents((prev) => mergeEvents(prev, [ev]));
      scheduleLoad();
    });
    return () => {
      source.close();
      if (timer.current !== null) window.clearTimeout(timer.current);
    };
  }, [api, load, scheduleLoad]);

  return { api, health, state, changes, jobs, ledger, events, connection, error, refresh: load };
}
