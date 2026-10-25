import test from "node:test";
import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import { frozenPanels, usageCharge } from "./run.mjs";

test("paired panel commitments reproduce and final panel differs from every selection panel", () => {
  const a = frozenPanels("judge-20261025-01");
  assert.deepEqual(a, frozenPanels("judge-20261025-01"));
  assert.equal(a.selection.length, 5);
  assert.equal(new Set([...a.selection, a.final]).size, 6);
  assert.notDeepEqual(a, frozenPanels("judge-20261025-02"));
});

test("cost settlement accepts bounded actual usage and refuses unknown or unpriced cache use", () => {
  assert.equal(usageCharge({ input_tokens: 1000, output_tokens: 100 }), 3000);
  assert.throws(() => usageCharge(null), /unknown/);
  assert.throws(() => usageCharge({ input_tokens: 1, output_tokens: 1, cache_read_input_tokens: 1 }), /cache/);
  assert.throws(() => usageCharge({ input_tokens: 62049, output_tokens: 1 }), /reserved/);
  assert.throws(() => usageCharge({ input_tokens: 1, output_tokens: 8001 }), /reserved/);
});

test("complete-dollar admission refuses a paid run before reading a key or creating an artifact", () => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "genesis-budget-test-"));
  const output = path.join(root, "run");
  const script = fileURLToPath(new URL("./run.mjs", import.meta.url));
  const result = spawnSync(process.execPath, [script, "--output-dir", output, "--key-file", path.join(root, "missing.env"), "--prior-project-reserve-usd", "50"], { encoding: "utf8" });
  assert.notEqual(result.status, 0);
  assert.match(result.stderr, /reserve exceeds a dollar ceiling/);
  assert.equal(fs.existsSync(output), false);
  fs.rmSync(root, { recursive: true, force: true });
});
