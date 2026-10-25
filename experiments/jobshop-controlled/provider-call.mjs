// One non-retrying Claude request. Only this child receives the API key.
const chunks = [];
for await (const chunk of process.stdin) chunks.push(chunk);
const req = JSON.parse(Buffer.concat(chunks).toString("utf8"));
const key = process.env.GENESIS_LLM_API_KEY;
if (!key) throw new Error("missing local API key");
const body = {
  model: req.model,
  max_tokens: req.max_tokens,
  system: req.system,
  messages: [{ role: "user", content: req.user }],
  output_config: { effort: req.effort },
};
let response, json;
try {
  response = await fetch("https://api.anthropic.com/v1/messages", {
    method: "POST",
    headers: {
      "content-type": "application/json",
      "x-api-key": key,
      "anthropic-version": "2023-06-01",
    },
    body: JSON.stringify(body),
    signal: AbortSignal.timeout(170_000),
  });
  json = await response.json();
} catch (error) {
  // Never put the transport's message, JSON body or request headers in stderr.
  // A body-read failure still retains an HTTP error status already received.
  const timeout = error?.name === "TimeoutError";
  if (response && !response.ok) throw new Error(`Claude API HTTP ${response.status}: unknown${timeout ? " (timeout)" : ""}`);
  throw new Error(timeout ? "Claude API timeout" : "Claude API response unavailable");
}
if (!response.ok) {
  const type = json?.error?.type;
  const safeType = typeof type === "string" && /^[A-Za-z_]{1,48}$/.test(type) ? type : "unknown";
  throw new Error(`Claude API HTTP ${response.status}: ${safeType}`);
}
process.stdout.write(JSON.stringify({
  id: json.id,
  model: json.model,
  stop_reason: json.stop_reason,
  usage: json.usage,
  text: (json.content || []).filter((part) => part.type === "text").map((part) => part.text).join("\n"),
}));
