// Runner orchestration only: all method, evaluator, provider and engine dependencies are inert.
// Never imports or executes the job-shop family or a generated program.
import test from "node:test";
import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import vm from "node:vm";

const runner = fs.readFileSync(new URL("./run-jobshop.mjs", import.meta.url), "utf8")
  .replace(/^import .*;\n/gm, "")
  .replace(/main\(\)\.catch\([\s\S]*$/, "main();");

async function nullRun(out) {
  const state = { generation: 0, frontier: {}, packages: {}, ledger: { records: [], head: "fixture" }, calls: 0, evaluations: 0 };
  const context = {
    fs, path,
    process: { argv: ["node", "fixture", "--mock", "--generations", "1", "--out", out], env: {}, stdout: { write() {} } },
    createState: () => state,
    runGeneration: () => ({ refused: "inert null-result fixture" }),
    verifyLedger: () => ({ ok: true }),
    sha256: () => "0".repeat(64),
    jobshopFamily: () => ({ id: "fixture" }),
    jobshopControls: () => [],
    cascade: (inner) => ({ ...inner, counts: { evaluations: 0 } }),
    toBulk: () => ({ body: "fixture\n" }),
    post: async () => ({ sent: false, reason: "inert transport" }),
  };
  // Prevent even .env reads in this orchestration fixture.
  context.fs = { ...fs, existsSync: (p) => p === ".env" ? false : fs.existsSync(p) };
  await vm.runInNewContext(runner, context, { timeout: 1000, filename: "run-jobshop.mjs" });
}

test("control: a fresh output directory retains the null result", async () => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "genesis-run-dir-"));
  try {
    const out = path.join(root, "new-run");
    await nullRun(out);
    assert.equal(JSON.parse(fs.readFileSync(path.join(out, "result.json"))).confirmation, null);
    assert.equal(fs.existsSync(path.join(out, "bundle.json")), false);
  } finally { fs.rmSync(root, { recursive: true, force: true }); }
});

test("an occupied output directory refuses a new run without changing retained evidence", async () => {
  const out = fs.mkdtempSync(path.join(os.tmpdir(), "genesis-run-dir-"));
  const files = ["result.json", "ledger.json", "bundle.json", "fetch-offer.txt", "fetch-observation.json", "elastic-bulk.ndjson"];
  const original = new Map(files.map((name) => [name, `${name}: previous accepted run\n`]));
  try {
    for (const [name, body] of original) fs.writeFileSync(path.join(out, name), body);
    let error;
    try { await nullRun(out); } catch (err) { error = err; }
    const changed = files.filter((name) => fs.readFileSync(path.join(out, name), "utf8") !== original.get(name));
    assert.ok(error, `occupied run was accepted; overwritten=${changed.join(",")}; stale bundle and offers remain`);
    assert.deepEqual(changed, [], "refusal must precede writes to the retained run");
  } finally { fs.rmSync(out, { recursive: true, force: true }); }
});
