// Types and helpers for the Elastic evidence search API
// (`GET /api/v1/projection`, `GET /api/v1/search/evidence`).

export interface ProjectionStatus {
  configured: boolean;
  projected_revision: number | null;
  documents: number;
  last_error: string | null;
}

export interface EvidenceDoc {
  doc_id: string;
  org_id: string;
  kind: "mission" | "observation" | "requirement" | "responsibility" | "role";
  title: string;
  body: string;
  mission_id: string | null;
  status: string | null;
  revision: number;
  refs?: Record<string, string>;
}

export interface EvidenceResults {
  projected_revision: number | null;
  hits: { score: number; doc: EvidenceDoc }[];
}

export function searchPath(query: string, kinds: string[] = [], limit = 10): string {
  const p = new URLSearchParams({ q: query.trim(), limit: String(limit) });
  if (kinds.length > 0) p.set("kinds", kinds.join(","));
  return `/search/evidence?${p.toString()}`;
}

/** Whether the index may be behind the kernel's current revision. */
export function lagging(projected: number | null, current: number | undefined): boolean {
  return current !== undefined && (projected === null || projected < current);
}

/** The record ID inside a deterministic document ID (`org:kind:id`). */
export function recordId(docId: string): string {
  const parts = docId.split(":");
  return parts.length >= 3 ? parts.slice(2).join(":") : docId;
}

/** An unverified Elastic specialist answer stored on a job, if any. */
export function elasticAnswer(result: unknown): { message: string; tools: string[] } | null {
  if (!result || typeof result !== "object") return null;
  const r = result as Record<string, unknown>;
  if (r.kind !== "elastic_agent_answer" || typeof r.message !== "string") return null;
  return {
    message: r.message,
    tools: Array.isArray(r.tool_calls) ? r.tool_calls.filter((t): t is string => typeof t === "string") : [],
  };
}
