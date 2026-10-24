// Minimal client for one organization kernel.

export interface OrgEndpoint {
  id: string;
  label: string;
  url: string;
  token: string;
}

const DEFAULT_ORGS: OrgEndpoint[] = [
  { id: "org_a", label: "Org A", url: "http://127.0.0.1:7101", token: "org_a-operator" },
  { id: "org_b", label: "Org B", url: "http://127.0.0.1:7102", token: "org_b-operator" },
];

export function orgsFromEnv(raw: string | undefined = import.meta.env?.VITE_GENESIS_ORGS): OrgEndpoint[] {
  if (!raw) return DEFAULT_ORGS;
  try {
    const parsed = JSON.parse(raw) as OrgEndpoint[];
    if (Array.isArray(parsed) && parsed.every((o) => o.id && o.url && o.token)) return parsed;
  } catch {
    // fall through
  }
  console.warn("VITE_GENESIS_ORGS is not valid JSON; using dev defaults");
  return DEFAULT_ORGS;
}

export class ApiError extends Error {
  constructor(
    public status: number,
    public code: string,
    message: string,
    public details?: unknown,
  ) {
    super(message);
  }
}

export class Api {
  constructor(private ep: OrgEndpoint) {}

  private async request<T>(method: string, path: string, body?: unknown): Promise<T> {
    const res = await fetch(`${this.ep.url}/api/v1${path}`, {
      method,
      headers: {
        Authorization: `Bearer ${this.ep.token}`,
        ...(body === undefined ? {} : { "Content-Type": "application/json" }),
      },
      body: body === undefined ? undefined : JSON.stringify(body),
    });
    const text = await res.text();
    const data = text ? JSON.parse(text) : null;
    if (!res.ok) {
      throw new ApiError(res.status, data?.error ?? "http_error", data?.message ?? res.statusText, data?.details);
    }
    return data as T;
  }

  get<T>(path: string): Promise<T> {
    return this.request<T>("GET", path);
  }

  post<T>(path: string, body: unknown): Promise<T> {
    return this.request<T>("POST", path, body);
  }

  eventsUrl(after: number): string {
    const u = new URL(`${this.ep.url}/api/v1/events`);
    u.searchParams.set("after", String(after));
    u.searchParams.set("token", this.ep.token);
    return u.toString();
  }
}
