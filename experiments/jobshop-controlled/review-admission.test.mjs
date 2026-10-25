// Review guards (Henry's Claude) for the key-file admission and the provider
// failure summary. Temporary files only; no key, provider, sandbox or family.
import test from "node:test";
import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { admitKeyFile, providerFailure } from "./run.mjs";

function tree() {
  const base = fs.mkdtempSync(path.join(os.tmpdir(), "jobshop-admission-"));
  for (const d of ["readable/sub", "elsewhere", "readable-sibling"]) fs.mkdirSync(path.join(base, d), { recursive: true });
  for (const f of ["readable/sub/.env", "elsewhere/.env", "readable-sibling/.env"]) fs.writeFileSync(path.join(base, f), "GENESIS_LLM_API_KEY=x\n");
  return base;
}

test("a key file under a sandbox read root is refused, also through a symlink", () => {
  const base = tree();
  const roots = [path.join(base, "readable")];
  assert.throws(() => admitKeyFile(path.join(base, "readable/sub/.env"), roots), /rule sandbox can read/);
  fs.symlinkSync(path.join(base, "readable/sub/.env"), path.join(base, "elsewhere/link.env"));
  assert.throws(() => admitKeyFile(path.join(base, "elsewhere/link.env"), roots), /rule sandbox can read/);
});

test("a key file outside every root is admitted, including a prefix-named sibling", () => {
  const base = tree();
  const roots = [path.join(base, "readable")];
  assert.equal(admitKeyFile(path.join(base, "elsewhere/.env"), roots), fs.realpathSync(path.join(base, "elsewhere/.env")));
  assert.ok(admitKeyFile(path.join(base, "readable-sibling/.env"), roots));
});

test("provider failure keeps only the HTTP status and error type", () => {
  const stderr = "file:///x/provider-call.mjs:25\nError: Claude API HTTP 529: overloaded_error\n    at file:///Users/someone/secret/path.mjs:25:30\n";
  assert.deepEqual(providerFailure(stderr), { http_status: 529, error_type: "overloaded_error" });
  assert.deepEqual(providerFailure("TypeError: fetch failed\n"), { http_status: null, error_type: null });
  assert.deepEqual(providerFailure(undefined), { http_status: null, error_type: null });
});
