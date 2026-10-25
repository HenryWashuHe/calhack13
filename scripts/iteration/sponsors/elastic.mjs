// Elastic: the iteration ledger as searchable evidence. `toBulk` renders a verified ledger as one
// Elasticsearch bulk body (one document per record, _id = the record's chain hash, so re-indexing is
// idempotent). `post` sends it only when a URL and a data key are present; nothing is printed about the
// key. Elastic is "the organization's memory" here: every plan, score and verdict becomes a query target.
import { verifyLedger } from "../engine.mjs";

export function toBulk(ledger, { index = "genesis-iteration-ledger", lab, exported_at } = {}) {
  const v = verifyLedger(ledger);
  if (!v.ok) throw new Error(`refusing to export an unverified ledger: ${v.reason || "verification failed"}`);
  const lines = ledger.records.flatMap((r) => [
    JSON.stringify({ index: { _index: index, _id: r.sha256 } }),
    JSON.stringify({ ...r, ledger_head: ledger.head, lab: r.lab || lab, exported_at: exported_at || null }),
  ]);
  return { body: lines.join("\n") + "\n", documents: ledger.records.length, index, head: ledger.head };
}

/** Field mapping that keeps hashes exact and leaves free-text reasons searchable. */
export const mapping = {
  mappings: { dynamic: true, properties: { type: { type: "keyword" }, family: { type: "keyword" }, plan: { type: "keyword" }, package: { type: "keyword" }, sha256: { type: "keyword" }, prev: { type: "keyword" }, ledger_head: { type: "keyword" }, lab: { type: "keyword" }, verdict: { type: "keyword" }, reason: { type: "text" }, generation: { type: "integer" }, index: { type: "integer" } } },
};

export async function post(bulk, { url = process.env.ELASTICSEARCH_URL, apiKey = process.env.ELASTIC_DATA_API_KEY, fetchImpl = globalThis.fetch } = {}) {
  if (!url || !apiKey) return { sent: false, reason: "ELASTICSEARCH_URL or ELASTIC_DATA_API_KEY not set; bulk body written locally only" };
  const res = await fetchImpl(`${url.replace(/\/$/, "")}/_bulk`, { method: "POST", headers: { "content-type": "application/x-ndjson", authorization: `ApiKey ${apiKey}` }, body: bulk.body });
  const json = await res.json().catch(() => ({}));
  if (!res.ok) return { sent: false, status: res.status, reason: (json.error && json.error.reason) || res.statusText };
  const failures = (json.items || []).filter((i) => i.index && i.index.error);
  const first = failures[0] ? failures[0].index.error : null;
  return { sent: true, status: res.status, indexed: (json.items || []).length - failures.length, failed: failures.length, errors: !!json.errors, first_error: first ? { type: first.type, reason: String(first.reason || "").slice(0, 200) } : null };
}
