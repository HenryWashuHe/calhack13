// Source-derived orchestration/policy checks only. Every loaded module import is
// removed and its dependencies replaced with inert mocks. No rule, family,
// evaluator, provider, process, sandbox command or filesystem probe is executed.
import test from "node:test";
import assert from "node:assert/strict";
import fs from "node:fs";
import vm from "node:vm";

const sandboxSource = fs.readFileSync(new URL("./families/sandbox.mjs", import.meta.url), "utf8");
const runnerSource = fs.readFileSync(new URL("./run-jobshop.mjs", import.meta.url), "utf8");
const WORK = "/private/tmp/review-work";
const pathMock = {
  delimiter: ":",
  join: (...parts) => parts.join("/"),
  dirname: (value) => value.slice(0, value.lastIndexOf("/")),
};
function policy(platform) {
  const files = new Map();
  const context = {
    URL,
    process: { platform, env: { PATH: "/mock/bin" }, execPath: "/mock/bin/node" },
    path: pathMock,
    os: { homedir: () => "/Users/reviewer", tmpdir: () => "/private/tmp" },
    fileURLToPath: (url) => String(url).replace(/^file:\/\//, ""),
    fs: {
      constants: { X_OK: 1 },
      realpathSync: (p) => p,
      mkdtempSync: () => WORK,
      accessSync: (p) => {
        if (p !== "/usr/bin/sandbox-exec" && p !== "/mock/bin/unshare") throw new Error("not executable");
      },
      existsSync: (p) => files.has(p),
      writeFileSync: (p, body) => files.set(p, body),
    },
    spawnSync: () => { throw new Error("subprocess execution forbidden in this test"); },
  };
  const source = sandboxSource.replace(/^import .*;\n/gm, "")
    .replace(/^export /gm, "")
    .replaceAll("import.meta.url", JSON.stringify("file:///opt/fixture/scripts/iteration/families/sandbox.mjs"));
  const api = vm.runInNewContext(`${source}\n({ describeSandbox, darwinProfile, command });`, context, { timeout: 1000 });
  return { api, files };
}

async function admission(sandbox) {
  let familyEntered = false;
  const context = {
    fs: { existsSync: () => false, mkdirSync() {} },
    path: pathMock,
    process: { argv: ["node", "runner", "--out", "/mock/out"], env: {} },
    describeSandbox: () => sandbox,
    jobshopFamily: () => { familyEntered = true; throw new Error("inert family boundary reached"); },
    jobshopControls: () => { throw new Error("control execution forbidden"); },
  };
  const source = runnerSource.replace(/^import .*;\n/gm, "")
    .replace(/main\(\)\.catch\([\s\S]*$/, "main();");
  let error;
  try { await vm.runInNewContext(source, context, { timeout: 1000 }); } catch (caught) { error = caught; }
  return { familyEntered, error };
}

test("control: absent OS confinement refuses live work before the mocked family", async () => {
  const result = await admission({ os: "none", filesystem: "not confined", network: "not confined" });
  assert.equal(result.familyEntered, false);
  assert.match(String(result.error), /no OS sandbox/);
});

test("Linux network-only confinement does not admit live work by default", async () => {
  const { api } = policy("linux");
  const sandbox = api.describeSandbox();
  assert.equal(sandbox.os, "unshare");
  assert.equal(sandbox.filesystem, "not confined");
  const result = await admission(sandbox);
  assert.equal(result.familyEntered, false, "network-only policy admitted live work without --allow-unsandboxed");
});

test("a profile described as read-confined has a global read denial", () => {
  const { api } = policy("darwin");
  assert.match(api.describeSandbox().filesystem, /only/);
  const lines = api.darwinProfile(WORK).split("\n");
  // Explicitly source-policy assurance only, not a claim that Seatbelt enforces
  // a generated profile. Selective path denials cannot close an allow default.
  assert.ok(lines.includes("(deny default)") || lines.includes("(deny file-read*)"),
    "read-confined claim has allow default plus only selected-tree read denials");
});

test("a subsequent child never reuses a policy altered in its writable workdir", () => {
  const { api, files } = policy("darwin");
  const first = api.command(WORK);
  const profile = first[1][first[1].indexOf("-f") + 1];
  assert.ok(files.has(profile));
  assert.ok(files.get(profile).includes(`(allow file-write* (subpath "${WORK}")`));
  // In-memory state substitution only: no child or filesystem operation occurs.
  if (profile.startsWith(`${WORK}/`)) files.set(profile, "(version 1)\n(allow default)");
  const next = api.command(WORK);
  const nextProfile = next[1][next[1].indexOf("-f") + 1];
  assert.notEqual(files.get(nextProfile), "(version 1)\n(allow default)",
    "the child-writable profile is reused as the next child's confinement policy");
});
