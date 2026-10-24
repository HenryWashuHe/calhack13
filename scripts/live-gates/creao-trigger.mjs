// One app-scoped CREAO API Trigger run. The journal is written before POST so
// an uncertain response can never be retried as a second credit-consuming run.
import { createHash, randomUUID } from "node:crypto";
import { closeSync, existsSync, fsyncSync, mkdirSync, openSync, readFileSync, renameSync, writeFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { parseArgs } from "node:util";
import { fileURLToPath } from "node:url";

const HOST = "https://agent.creao.ai";
const UUID = /^[0-9a-f]{8}-[0-9a-f]{4}-[1-8][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i;
const TERMINAL = new Set(["completed", "failed", "cancelled"]);
const sleep = (ms) => new Promise((done) => setTimeout(done, ms));

function digest(value) {
  return createHash("sha256").update(JSON.stringify(value)).digest("hex");
}

function save(path, record, first = false) {
  const body = `${JSON.stringify(record, null, 2)}\n`;
  mkdirSync(dirname(path), { recursive: true, mode: 0o700 });
  const target = first ? path : `${path}.${randomUUID()}.tmp`;
  const fd = openSync(target, "wx", 0o600);
  try {
    writeFileSync(fd, body);
    fsyncSync(fd);
  } finally {
    closeSync(fd);
  }
  if (!first) renameSync(target, path);
}

async function jsonResponse(response) {
  if (response.type === "opaqueredirect" || response.status >= 300 && response.status < 400) {
    throw new Error("redirect_rejected");
  }
  const length = Number(response.headers.get("content-length"));
  if (length > 256 * 1024) throw new Error("response_too_large");
  const raw = await response.text();
  if (Buffer.byteLength(raw) > 256 * 1024) throw new Error("response_too_large");
  try { return JSON.parse(raw); } catch { throw new Error("invalid_json"); }
}

function recordView(record) {
  return {
    schema: record.schema,
    started_at: record.started_at,
    app_id: record.app_id,
    input_sha256: record.input_sha256,
    phase: record.phase,
    run_id: record.run_id ?? null,
    status: record.status ?? null,
    result_sha256: record.result_sha256 ?? null,
    credits_used: record.credits_used ?? null,
    reason: record.reason ?? null,
  };
}

/** No production-host override in the CLI; tests inject a loopback transport. */
export async function runTrigger({ appId, inputs, out, allowCredits = false, key,
  fetchImpl = fetch, baseUrl = HOST, pollMs = 2000, maxPolls = 60 }) {
  if (!UUID.test(appId ?? "")) throw new Error("app ID must be a UUID");
  if (!inputs || typeof inputs !== "object" || Array.isArray(inputs)) {
    throw new Error("inputs must be a JSON object");
  }
  if (Buffer.byteLength(JSON.stringify({ inputs })) > 256 * 1024) {
    throw new Error("inputs exceed the 256 KiB smoke limit");
  }
  if (!out) throw new Error("--out is required for a durable run journal");
  if (baseUrl !== HOST && !/^http:\/\/(?:127\.0\.0\.1|localhost)(?::\d+)?$/.test(baseUrl)) {
    throw new Error("provider URL must be CREAO or literal loopback");
  }
  if (!Number.isSafeInteger(maxPolls) || maxPolls < 1 || maxPolls > 600) {
    throw new Error("maxPolls must be 1..600");
  }
  const path = resolve(out);
  const inputSha = digest(inputs);
  if (!allowCredits) {
    return { schema: "genesis-creao-trigger-smoke/1", phase: "preflight", app_id: appId,
      input_sha256: inputSha, key_present: typeof key === "string" && key.startsWith("capi_"),
      note: "No provider call. A credit-consuming run requires --allow-credits." };
  }
  if (typeof key !== "string" || !key.startsWith("capi_")) {
    throw new Error("CREAO_TRIGGER_API_KEY must be a capi_ key");
  }
  let record;
  if (existsSync(path)) {
    try { record = JSON.parse(readFileSync(path, "utf8")); }
    catch { throw new Error("existing journal is unreadable; do not replay the POST"); }
    if (record.schema !== "genesis-creao-trigger-smoke/1" || record.app_id !== appId ||
      record.input_sha256 !== inputSha) {
      throw new Error("existing journal does not match this app and exact input");
    }
    if (TERMINAL.has(record.phase) || record.phase === "needs_reconciliation") return recordView(record);
    if (!UUID.test(record.run_id ?? "")) {
      return recordView({ ...record, phase: "needs_reconciliation", reason: "post_outcome_unknown" });
    }
  } else {
    record = { schema: "genesis-creao-trigger-smoke/1", started_at: new Date().toISOString(), app_id: appId,
      input_sha256: inputSha, phase: "post_outcome_unknown", run_id: null };
    save(path, record, true);
    let response;
    try {
      response = await fetchImpl(`${baseUrl}/api/v1/apps/${appId}/runs`, {
        method: "POST", redirect: "manual", signal: AbortSignal.timeout(15000),
        headers: { Authorization: `Bearer ${key}`, "Content-Type": "application/json" },
        body: JSON.stringify({ inputs }),
      });
      if (response.status !== 202) throw new Error("post_not_accepted");
      const body = await jsonResponse(response);
      if (!UUID.test(body.id ?? "") || body.appId !== appId ||
        !["pending", "running", "completed", "failed", "cancelled"].includes(body.status)) {
        throw new Error("invalid_post_receipt");
      }
      record = { ...record, phase: "running", run_id: body.id, status: body.status };
      save(path, record);
    } catch {
      // A failed request may have reached CREAO. Preserve the pre-POST journal.
      return recordView({ ...record, phase: "needs_reconciliation", reason: "post_outcome_unknown" });
    }
  }
  for (let i = 0; i < maxPolls; i++) {
    let body;
    try {
      const response = await fetchImpl(`${baseUrl}/api/v1/apps/${appId}/runs/${record.run_id}`, {
        method: "GET", redirect: "manual", signal: AbortSignal.timeout(15000),
        headers: { Authorization: `Bearer ${key}` },
      });
      if (response.status !== 200) throw new Error("get_failed");
      body = await jsonResponse(response);
      if (body.id !== record.run_id || body.appId !== appId ||
        !["pending", "running", "completed", "failed", "cancelled"].includes(body.status)) {
        throw new Error("invalid_get_receipt");
      }
    } catch {
      return recordView({ ...record, reason: "poll_failed; rerun with the same journal to retry GET only" });
    }
    record = { ...record, status: body.status, phase: body.status,
      result_sha256: body.result === undefined ? null : digest(body.result),
      credits_used: typeof body.usage?.credits === "number" && Number.isFinite(body.usage.credits)
        ? body.usage.credits : null, reason: null };
    save(path, record);
    if (TERMINAL.has(body.status)) return recordView(record);
    if (i + 1 < maxPolls) await sleep(pollMs);
  }
  return recordView({ ...record, reason: "poll_limit; rerun with the same journal to continue GET only" });
}

async function main() {
  const { values } = parseArgs({ options: {
    "app-id": { type: "string" }, inputs: { type: "string" }, out: { type: "string" },
    "allow-credits": { type: "boolean", default: false },
  } });
  try {
    if (!values.inputs) throw new Error("--inputs is required");
    let inputs;
    try { inputs = JSON.parse(readFileSync(values.inputs, "utf8")); }
    catch { throw new Error("inputs file must contain valid JSON"); }
    const report = await runTrigger({ appId: values["app-id"], inputs, out: values.out,
      allowCredits: values["allow-credits"], key: process.env.CREAO_TRIGGER_API_KEY });
    process.stdout.write(`${JSON.stringify(report, null, 2)}\n`);
    if (report.reason || ["failed", "cancelled", "needs_reconciliation"].includes(report.phase)) process.exitCode = 1;
  } catch (error) {
    console.error(`CREAO trigger smoke: ${error.message}`);
    process.exitCode = 2;
  }
}

if (process.argv[1] && fileURLToPath(import.meta.url) === resolve(process.argv[1])) main();
