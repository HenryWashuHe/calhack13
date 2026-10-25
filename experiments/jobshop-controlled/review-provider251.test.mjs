// Independent source-level transport guards. All execution dependencies are
// inert: no runner entrypoint, family, rule, sandbox, child or network is run.
import test from "node:test";
import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import vm from "node:vm";
import { fileURLToPath } from "node:url";

const runnerUrl = new URL("./run.mjs", import.meta.url);
const runnerText = fs.readFileSync(runnerUrl, "utf8");
const runner = runnerText.replace(/^import .*;\n/gm, "").replace(/^export /gm, "")
  .replaceAll("import.meta.url", JSON.stringify(runnerUrl.href));
assert.doesNotMatch(runner, /^import /m);
const helpers = { path, fileURLToPath, process: { argv: [] }, fs: {} };
vm.runInNewContext(`${runner}\nglobalThis.helper = { admitKeyFile, providerFailure };`, helpers, { timeout: 1000 });
const plain = (value) => JSON.parse(JSON.stringify(value));

const providerSource = fs.readFileSync(new URL("./provider-call.mjs", import.meta.url), "utf8");
assert.doesNotMatch(providerSource, /\bimport\s/);
async function fakeProvider(fetch) {
  let output = "";
  const process = { env: { GENESIS_LLM_API_KEY: "inert-not-a-key" },
    stdin: (async function* () { yield Buffer.from(JSON.stringify({ model: "fixture", max_tokens: 1, system: "fixture", user: "fixture", effort: "medium" })); })(),
    stdout: { write: (value) => { output += value; } } };
  try {
    await vm.runInNewContext(`(async () => { ${providerSource} })()`, {
      process, Buffer, fetch, AbortSignal: { timeout: () => ({ inert: true }) },
    }, { timeout: 1000 });
    return { status: 0, stdout: output, stderr: "" };
  } catch (error) {
    return { status: 1, stdout: output, stderr: `${error.name}: ${error.message}\n` };
  }
}

// Exercise the actual runner's existing failed-child branch, not main().
const failureBranch = runnerText.match(/      if \(result\.error \|\| result\.status !== 0\) \{([\s\S]*?)\n      \}\n      const response/);
assert.ok(failureBranch, "locate the failed-child branch for this exact source");
function stoppedProvider(result) {
  return plain(vm.runInNewContext(`(() => { try { ${failureBranch[1]} } catch (error) { return error.provider; } })()`, {
    result, providerFailure: helpers.helper.providerFailure, PER_CALL_MICRO_USD: 204096,
  }, { timeout: 1000 }));
}

test("ordinary JSON HTTP errors retain only status/type and their unresolved reserve", async () => {
  const result = await fakeProvider(async () => ({ ok: false, status: 529, json: async () => ({ error: { type: "overloaded_error", message: "private diagnostic" } }) }));
  assert.deepEqual(stoppedProvider(result), { http_status: 529, error_type: "overloaded_error", timeout: false, reserved_micro_usd: 204096 });
  assert.doesNotMatch(result.stderr, /private diagnostic/);
});

test("a known HTTP status survives an unreadable proxy error body", async () => {
  const result = await fakeProvider(async () => ({ ok: false, status: 503, json: async () => { throw new SyntaxError("not JSON"); } }));
  assert.equal(stoppedProvider(result).http_status, 503);
});

test("the provider child's own timeout is retained as a timeout", async () => {
  const result = await fakeProvider(async () => { const error = new Error("request timed out"); error.name = "TimeoutError"; throw error; });
  assert.equal(stoppedProvider(result).timeout, true);
});

test("the parent child-process timeout remains a timeout", () => {
  assert.equal(stoppedProvider({ error: { code: "ETIMEDOUT" }, status: null, stderr: "" }).timeout, true);
});

test("key admission refuses resolved descendants, including symlinks, but admits siblings", () => {
  const resolved = { "/read": "/read", "/outside/link": "/read/key", "/read/key": "/read/key", "/read-sibling/key": "/read-sibling/key" };
  helpers.fs.realpathSync = (name) => { assert.ok(name in resolved); return resolved[name]; };
  assert.throws(() => helpers.helper.admitKeyFile("/read/key", ["/read"]), /sandbox can read/);
  assert.throws(() => helpers.helper.admitKeyFile("/outside/link", ["/read"]), /sandbox can read/);
  assert.equal(helpers.helper.admitKeyFile("/read-sibling/key", ["/read"]), "/read-sibling/key");
});
