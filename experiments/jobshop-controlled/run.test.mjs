import test from "node:test";
import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import { frozenPanels, usageCharge, independentSampler, admitSandbox } from "./run.mjs";
import { sha256 } from "../../scripts/iteration/engine.mjs";
import { claudeMutator } from "../../scripts/iteration/mutators/claude.mjs";

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

test("execution admission rejects network-only and absent OS sandboxes before work", () => {
  assert.throws(() => admitSandbox({ os: "unshare", network: "denied" }, "linux"), /requires/);
  assert.throws(() => admitSandbox({ os: "none", network: "not confined" }, "darwin"), /requires/);
  assert.deepEqual(admitSandbox({ os: "sandbox-exec", network: "denied" }, "darwin"), { os: "sandbox-exec", network: "denied" });
});

test("independent generation uses the frozen root as prompt and package parent after promotion", () => {
  const prompts = [];
  const root = "(op) => -op.duration";
  const sampler = independentSampler(() => claudeMutator({ task: "job shop", featureDoc: "op.duration is an integer",
    call: (req) => { prompts.push(req.user); return { text: "=== CANDIDATE 1 ===\n```js\n(op) => op.duration\n```", model: "claude-sonnet-5-5", usage: { input_tokens: 10, output_tokens: 10 } }; } }), root);
  const result = sampler.propose({ parent: { source: "(op) => op.remainingWork", sha256: sha256("(op) => op.remainingWork") },
    feedback: { instances: [{ success: true, secret_marker: "must-not-appear" }] }, k: 1, generation: 2 });
  assert.equal(result.length, 1);
  assert.equal(result[0].parent, sha256(root));
  assert.match(prompts[0], /-op\.duration/);
  assert.doesNotMatch(prompts[0], /remainingWork|must-not-appear/);
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
