// Review guard for #216 at 5fcb126 (Henry's Claude): the empty-environment
// child keeps the key out of process.env, but a rule that escapes node:vm into
// the child's own realm still runs as the runner's user with the whole
// filesystem, so it can read the runner's .env (run-jobshop.mjs loads the key
// from it) and write anywhere. This probe uses a dummy secret file only.
// Run: node --test scripts/iteration/families/review_child_filesystem.test.mjs
import test from "node:test";
import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { jobshopFamily } from "./jobshop.mjs";
import { binpackFamily } from "./binpack.mjs";

for (const [name, family] of [
  ["jobshop", jobshopFamily({ id: "jobshop", seed: "s" })],
  ["binpack", binpackFamily({ id: "binpack", seed: "s" })],
]) {
  test(`${name}: a rule cannot read a host file and copy it out`, () => {
    const dir = fs.mkdtempSync(path.join(os.tmpdir(), "review216-"));
    const secret = path.join(dir, ".env");
    const copy = path.join(dir, "copied");
    fs.writeFileSync(secret, "GENESIS_LLM_API_KEY=dummy-not-a-key\n");
    const source = `(() => {
      const p = this.constructor.constructor('return process')();
      const fs = p.getBuiltinModule('fs');
      fs.writeFileSync(${JSON.stringify(copy)}, fs.readFileSync(${JSON.stringify(secret)}));
      return (x) => 0;
    })()`;
    try {
      const [i] = family.instances("fs", 1);
      family.score({ source }, i);
      assert.equal(fs.existsSync(copy), false, "the rule read a host file and wrote it elsewhere");
    } finally {
      fs.rmSync(dir, { recursive: true, force: true });
    }
  });
}
