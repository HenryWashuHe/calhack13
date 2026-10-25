// One Messages API request in a child process, so the synchronous engine can call a model without
// becoming asynchronous. Reads {system, user, model, max_tokens, effort} as JSON on stdin; writes
// {text, stop_reason, model, usage} as JSON on stdout. Raw HTTP on purpose: scripts/iteration has no
// dependencies. The key is read from the environment and never printed.
const FALLBACK_BETA = "server-side-fallback-2026-07-01";

function readStdin() {
  return new Promise((resolve, reject) => {
    const chunks = [];
    process.stdin.on("data", (c) => chunks.push(c));
    process.stdin.on("end", () => resolve(Buffer.concat(chunks).toString("utf8")));
    process.stdin.on("error", reject);
  });
}

function textOf(content) {
  return (Array.isArray(content) ? content : []).filter((b) => b && b.type === "text").map((b) => b.text).join("\n");
}

async function main() {
  const req = JSON.parse(await readStdin());
  const key = process.env.GENESIS_LLM_API_KEY || process.env.ANTHROPIC_API_KEY;
  if (!key) throw new Error("GENESIS_LLM_API_KEY (or ANTHROPIC_API_KEY) is not set");
  const base = (process.env.GENESIS_LLM_API_BASE || process.env.ANTHROPIC_BASE_URL || "https://api.anthropic.com").replace(/\/$/, "");
  const body = {
    model: req.model,
    max_tokens: req.max_tokens || 8000,
    system: req.system,
    messages: [{ role: "user", content: req.user }],
    output_config: { effort: req.effort || "medium" },
    fallbacks: "default", // a safety-classifier refusal is re-run server-side on the recommended model
  };
  const res = await fetch(`${base}/v1/messages`, {
    method: "POST",
    headers: { "content-type": "application/json", "x-api-key": key, "anthropic-version": "2023-06-01", "anthropic-beta": FALLBACK_BETA },
    body: JSON.stringify(body),
    signal: AbortSignal.timeout(req.timeout_ms || 170000),
  });
  const json = await res.json().catch(() => ({}));
  if (!res.ok) throw new Error(`messages API ${res.status}: ${(json.error && json.error.message) || res.statusText}`);
  const out = { text: textOf(json.content), stop_reason: json.stop_reason, model: json.model, usage: json.usage || null };
  if (json.stop_reason === "refusal") out.refusal = (json.stop_details && json.stop_details.category) || "unspecified";
  process.stdout.write(JSON.stringify(out));
}

main().catch((err) => { process.stderr.write(String(err && err.message ? err.message : err)); process.exit(1); });
