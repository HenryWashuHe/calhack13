// One app-scoped CREAO API Trigger run. The journal is written before POST so
// an uncertain response can never be retried as a second credit-consuming run.
import { createHash, randomUUID } from "node:crypto";
import { closeSync, existsSync, fsyncSync, openSync, readFileSync, renameSync, statSync, unlinkSync, writeFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { parseArgs } from "node:util";
import { fileURLToPath } from "node:url";

const HOST = "https://agent.creao.ai";
// Cloudflare currently rejects Node's default undici User-Agent on the run
// status route (403 / error 1010), while accepting a browser User-Agent.
const USER_AGENT = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/130.0.0.0 Safari/537.36";
const UUID = /^[0-9a-f]{8}-[0-9a-f]{4}-[1-8][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i;
export function triggerRunsUrl(appId, configured) {
  if (!UUID.test(appId ?? "")) throw new Error("CREAO_TRIGGER_APP_ID must be a UUID");
  const official = `${HOST}/api/v1/apps/${appId}/runs`;
  // Literal equality also rejects query credentials, fragments, userinfo, alien
  // hosts, ports, encoded path substitutions and another app's URL.
  if (configured !== undefined && configured !== official) {
    throw new Error("CREAO_TRIGGER_RUNS_URL must exactly match the configured app's official runs URL");
  }
  return official;
}

const TERMINAL = new Set(["completed", "failed", "cancelled"]);
const sleep = (ms) => new Promise((done) => setTimeout(done, ms));

function digest(value) {
  return createHash("sha256").update(JSON.stringify(value)).digest("hex");
}

function syncDirectory(path) {
  const fd = openSync(path, "r");
  try { fsyncSync(fd); }
  finally { closeSync(fd); }
}

function save(path, record, first = false) {
  const body = `${JSON.stringify(record, null, 2)}\n`;
  const target = first ? path : `${path}.${randomUUID()}.tmp`;
  const fd = openSync(target, "wx", 0o600);
  try {
    writeFileSync(fd, body);
    fsyncSync(fd);
  } finally {
    closeSync(fd);
  }
  if (!first) renameSync(target, path);
  syncDirectory(dirname(path));
}

async function jsonResponse(response) {
  if (response.type === "opaqueredirect" || response.status >= 300 && response.status < 400) {
    throw new Error("redirect_rejected");
  }
  const limit = 256 * 1024;
  const length = Number(response.headers.get("content-length"));
  if (length > limit) {
    await response.body?.cancel();
    throw new Error("response_too_large");
  }
  if (!response.body) throw new Error("invalid_json");
  const reader = response.body.getReader();
  const chunks = [];
  let size = 0;
  try {
    while (true) {
      const { done, value } = await reader.read();
      if (done) break;
      size += value.byteLength;
      if (size > limit) {
        await reader.cancel();
        throw new Error("response_too_large");
      }
      chunks.push(value);
    }
  } finally {
    reader.releaseLock();
  }
  const raw = Buffer.concat(chunks, size).toString("utf8");
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
    result_check_id: record.result_check_id ?? null,
    result_check: record.result_check ?? null,
    reason: record.reason ?? null,
  };
}

/** No production-host override in the CLI; tests inject a loopback transport. */
export async function runTrigger({ appId, inputs, out, allowCredits = false, key,
  fetchImpl = fetch, baseUrl = HOST, pollMs = 2000, maxPolls = 60, runsUrl, checkId = null, checkResult = null }) {
  triggerRunsUrl(appId, runsUrl);
  if ((checkId === null) !== (checkResult === null) || (checkId !== null &&
      (typeof checkId !== "string" || !/^[a-z0-9._/-]{1,128}$/i.test(checkId) || typeof checkResult !== "function"))) {
    throw new Error("result check requires a fixed contract ID and a function");
  }
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
  // Require an already existing journal directory. Creating ancestors here
  // would need each new directory entry persisted before the crediting POST.
  const directory = dirname(path);
  if (!statSync(directory, { throwIfNoEntry: false })?.isDirectory()) {
    throw new Error("journal directory must already exist before --allow-credits");
  }
  // One live invocation per journal. A stale lock after a process crash fails
  // closed until the operator checks the journal and clears the lock.
  const lockPath = `${path}.lock`;
  let lockFd;
  try { lockFd = openSync(lockPath, "wx", 0o600); }
  catch { throw new Error("journal lock exists or cannot be created; check for another run"); }
  try {
    closeSync(lockFd);
    let record;
    if (existsSync(path)) {
      try { record = JSON.parse(readFileSync(path, "utf8")); }
      catch { throw new Error("existing journal is unreadable; do not replay the POST"); }
      if (record.schema !== "genesis-creao-trigger-smoke/1" || record.app_id !== appId ||
        record.input_sha256 !== inputSha || (record.result_check_id ?? null) !== checkId) {
        throw new Error("existing journal does not match this app and exact input");
      }
      if (TERMINAL.has(record.phase) || record.phase === "needs_reconciliation") return recordView(record);
      if (!UUID.test(record.run_id ?? "")) {
        return recordView({ ...record, phase: "needs_reconciliation", reason: "post_outcome_unknown" });
      }
    } else {
      record = { schema: "genesis-creao-trigger-smoke/1", started_at: new Date().toISOString(), app_id: appId,
        input_sha256: inputSha, result_check_id: checkId, phase: "post_outcome_unknown", run_id: null };
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
          headers: { Authorization: `Bearer ${key}`, "User-Agent": USER_AGENT },
        });
        if (response.status !== 200) throw new Error("get_failed");
        body = await jsonResponse(response);
        // The live GET receipt omits appId, although POST includes it. The
        // exact run ID and app-scoped URL still bind this read to the journal.
        if (body.id !== record.run_id || (body.appId !== undefined && body.appId !== appId) ||
          !["pending", "running", "completed", "failed", "cancelled"].includes(body.status)) {
          throw new Error("invalid_get_receipt");
        }
      } catch {
        return recordView({ ...record, reason: "poll_failed; rerun with the same journal to retry GET only" });
      }
      let resultCheck = null;
      if (body.status === "completed" && checkResult) {
        try { resultCheck = { passed: checkResult(body.result) === true }; }
        catch { resultCheck = { passed: false }; }
      }
      record = { ...record, status: body.status, phase: body.status, result_check: resultCheck,
        result_sha256: body.result === undefined ? null : digest(body.result),
        credits_used: typeof body.usage?.credits === "number" && Number.isFinite(body.usage.credits) && body.usage.credits >= 0
          ? body.usage.credits : null, reason: null };
      save(path, record);
      if (TERMINAL.has(body.status)) return recordView(record);
      if (i + 1 < maxPolls) await sleep(pollMs);
    }
    return recordView({ ...record, reason: "poll_limit; rerun with the same journal to continue GET only" });
  } finally {
    unlinkSync(lockPath);
  }
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
      allowCredits: values["allow-credits"], key: process.env.CREAO_TRIGGER_API_KEY, runsUrl: process.env.CREAO_TRIGGER_RUNS_URL });
    process.stdout.write(`${JSON.stringify(report, null, 2)}\n`);
    if (report.reason || ["failed", "cancelled", "needs_reconciliation"].includes(report.phase)) process.exitCode = 1;
  } catch (error) {
    console.error(`CREAO trigger smoke: ${error.message}`);
    process.exitCode = 2;
  }
}

if (process.argv[1] && fileURLToPath(import.meta.url) === resolve(process.argv[1])) main();
