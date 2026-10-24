import { useEffect, useState } from "react";
import type { Api } from "../lib/api";
import { EvidenceResults, lagging, ProjectionStatus, recordId, searchPath } from "../lib/evidenceSearch";

/** Search this organization's Elastic evidence index. Hidden when Elastic is not configured. */
export function EvidenceSearch({ api, revision }: { api: Api; revision: number | undefined }) {
  const [status, setStatus] = useState<ProjectionStatus | null>(null);
  const [query, setQuery] = useState("");
  const [results, setResults] = useState<EvidenceResults | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    let live = true;
    api
      .get<ProjectionStatus>("/projection")
      .then((s) => live && setStatus(s))
      .catch(() => live && setStatus(null));
    return () => {
      live = false;
    };
  }, [api, revision]);

  if (!status?.configured) return null;

  const run = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!query.trim()) return;
    setBusy(true);
    setError(null);
    try {
      setResults(await api.get<EvidenceResults>(searchPath(query)));
    } catch (err) {
      setResults(null);
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="kv">
      <h3>Search evidence (Elastic)</h3>
      <form className="search-row" onSubmit={run}>
        <input
          type="search"
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          placeholder="e.g. venue closed"
          aria-label="Search this organization's evidence"
        />
        <button className="btn" type="submit" disabled={busy}>
          Search
        </button>
      </form>
      <div className="muted" style={{ fontSize: 12 }}>
        Index at r{status.projected_revision ?? "–"} · {status.documents} records
        {lagging(status.projected_revision, revision) && " · catching up"}
        {status.last_error && <span className="bad-text"> · last projection failed: {status.last_error}</span>}
      </div>
      {error && <div className="notice bad">{error}</div>}
      {results && results.hits.length === 0 && <p className="empty">No matching records.</p>}
      {results && results.hits.length > 0 && (
        <ul className="timeline">
          {results.hits.map((h) => (
            <li key={h.doc.doc_id}>
              <div className="row">
                <span className="chip neutral">{h.doc.kind}</span>
                {h.doc.status && <span className="chip faded">{h.doc.status}</span>}
                <span className="mono muted">{recordId(h.doc.doc_id)}</span>
              </div>
              <div>{h.doc.title}</div>
              {h.doc.body && <div className="muted" style={{ fontSize: 12 }}>{h.doc.body}</div>}
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}
