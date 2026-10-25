// Where generated rules run. `node:vm` is not a security boundary: a rule's top-level `this` is a
// contextified host object, so a rule can reach the host realm. Every rule is therefore scored in a
// separate child process, and that child is confined as far as this host allows:
//   - empty environment (no API key, no .env), an empty working directory, a wall-clock kill;
//   - `--disallow-code-generation-from-strings`, so `Function(...)`/`eval` throw in every realm of the
//     child, which closes the constructor.constructor class of escapes outright;
//   - an OS sandbox when one is available: on macOS, `sandbox-exec` with a DEFAULT-DENY read policy
//     (`(deny file-read*)`) that allows reads only of the OS and Node runtime roots, the iteration scripts
//     (plus the `package.json` files on their ancestor path, which the module resolver probes) and the
//     child's own work directory; denies all network; and allows writes only in that work directory and
//     `/dev`. The policy file lives OUTSIDE the child's writable directory and is rewritten from trusted
//     bytes before every spawn; every score gets a fresh work directory. On Linux, `unshare -rn` removes
//     the network only, which is NOT filesystem confinement and does not admit live runs.
// The parent never evaluates model text; it passes it as JSON. `describeSandbox()` says which layers are
// in force and whether they are admissible for live work, so a run can record it and refuse otherwise.
import { spawnSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

const CHILD = fileURLToPath(new URL("./rule-child.mjs", import.meta.url));
const ITERATION_DIR = fs.realpathSync(path.dirname(path.dirname(CHILD))); // scripts/iteration: the only code the child may read
const NODE_FLAGS = ["--no-warnings", "--disallow-code-generation-from-strings"];
let policyDirCache = null;
const policyDir = () => (policyDirCache ||= fs.realpathSync(fs.mkdtempSync(path.join(os.tmpdir(), "genesis-policy-")))); // never writable by a child
const freshWorkdir = () => fs.realpathSync(fs.mkdtempSync(path.join(os.tmpdir(), "genesis-rule-")));

const sbplQuote = (p) => `"${String(p).replace(/["\\]/g, "\\$&")}"`;
/** The ancestors of the iteration directory, whose package.json the ESM resolver may probe (a specific file each, never a tree). */
function ancestorManifests() {
  const out = [];
  for (let d = path.dirname(ITERATION_DIR); d && d !== path.dirname(d); d = path.dirname(d)) out.push(path.join(d, "package.json"));
  return out;
}
/** Where the Node runtime lives: the install prefix of the real binary, and the Homebrew root when it is one of its formulae. */
function runtimeRoots() {
  const real = fs.realpathSync(process.execPath);
  const prefix = path.dirname(path.dirname(real));
  const roots = [prefix, path.dirname(process.execPath)];
  if (real.startsWith("/opt/homebrew/")) roots.push("/opt/homebrew");
  if (real.startsWith("/usr/local/")) roots.push("/usr/local");
  return [...new Set(roots)];
}
function darwinProfile(work) {
  const reads = ["/usr", "/System", "/Library", "/dev", "/private/etc", ...runtimeRoots(), ITERATION_DIR, work].map((p) => `(subpath ${sbplQuote(p)})`)
    .concat(ancestorManifests().map((p) => `(literal ${sbplQuote(p)})`), ['(literal "/")']);
  return [
    "(version 1)", "(allow default)", "(deny network*)",
    "(deny file-read*)", // default-deny: nothing below is readable unless named here
    "(allow file-read-metadata)",
    `(allow file-read* ${reads.join(" ")})`,
    "(deny file-write*)",
    `(allow file-write* (subpath ${sbplQuote(work)}) (subpath "/dev"))`,
  ].join("\n");
}

const hasExecutable = (p) => { try { fs.accessSync(p, fs.constants.X_OK); return true; } catch { return false; } };
const onPath = (name) => (process.env.PATH || "").split(path.delimiter).some((d) => d && hasExecutable(path.join(d, name)));

/**
 * Which confinement layers apply on this host. `os` is "sandbox-exec", "unshare" or "none"; `admissible` is true
 * only when reads, writes and network are all confined, which is what a live run requires. Network-only
 * confinement (Linux `unshare`) is reported as what it is and is not admissible.
 */
export function describeSandbox() {
  const os_ = process.platform === "darwin" && hasExecutable("/usr/bin/sandbox-exec") ? "sandbox-exec"
    : process.platform === "linux" && onPath("unshare") ? "unshare" : "none";
  const confined = os_ === "sandbox-exec";
  return { process: "child, empty environment, fresh empty cwd per score, wall timeout, SIGKILL", code_generation: "disallowed from strings", os: os_,
    filesystem: confined ? "default-deny reads; allowed: OS and Node runtime roots, iteration scripts, own work dir only; write: own work dir only" : "not confined",
    network: os_ === "none" ? "not confined" : "denied", admissible: confined, confined }; // `confined` is the same flag under the name the parallel commit used
}

/** The command for one child. The policy file is written fresh from trusted bytes, outside the child's writable directory, on every call. */
function command(work) {
  const sb = describeSandbox();
  if (sb.os === "sandbox-exec") {
    const profile = path.join(policyDir(), "profile.sb");
    fs.writeFileSync(profile, darwinProfile(work)); // never reused: a child cannot write here, and even so the bytes are regenerated each time
    return ["/usr/bin/sandbox-exec", ["-f", profile, process.execPath, ...NODE_FLAGS, CHILD]];
  }
  if (sb.os === "unshare") return ["unshare", ["-rn", "--", process.execPath, ...NODE_FLAGS, CHILD]];
  return [process.execPath, [...NODE_FLAGS, CHILD]];
}

/** A bounded, primitive diagnostic from a thrown value, without invoking getters or string conversion on it. */
export function safeMessage(err) {
  if (typeof err === "string") return err.split("\n")[0].slice(0, 160);
  if (err && (typeof err === "object" || typeof err === "function")) {
    const m = Object.getOwnPropertyDescriptor(err, "message");
    if (m && typeof m.value === "string") return m.value.split("\n")[0].slice(0, 160);
    const n = Object.getOwnPropertyDescriptor(err, "name");
    return `${n && typeof n.value === "string" ? n.value.slice(0, 40) : "error"} (message withheld: not a plain string)`;
  }
  return `thrown ${typeof err}`;
}

export const failure = (message) => ({ success: false, progress: 0, cost: 1, detail: { error: String(message).slice(0, 160) } });

/**
 * Scores one rule on one instance in a confined child process. `family` names the module the child may
 * load (allow-listed there); `options` are the family's own numeric settings; `timeoutMs` is the per-phase
 * vm timeout the child applies and the basis of the parent's wall timeout. Synchronous; never throws.
 */
export function scoreInChild({ family, source, instance, options, timeoutMs }) {
  const wall = Math.max(250, 2 * timeoutMs + 150) + (describeSandbox().os === "sandbox-exec" ? 150 : 0);
  let r, work = null;
  try {
    work = freshWorkdir();
    const [cmd, args] = command(work);
    r = spawnSync(cmd, args, {
      input: JSON.stringify({ family, source: String(source), instance, options, timeoutMs }),
      env: {}, cwd: work, encoding: "utf8", timeout: wall, killSignal: "SIGKILL", maxBuffer: 1 << 20, windowsHide: true,
    });
  } catch (err) { return failure(`sandbox spawn failed: ${safeMessage(err)}`); }
  finally { if (work) { try { fs.rmSync(work, { recursive: true, force: true }); } catch { /* a leftover temp dir is not a scoring error */ } } }
  if (r.error && r.error.code === "ETIMEDOUT") return failure(`rule exceeded the wall timeout (${wall} ms); process killed`);
  if (r.error) return failure(`sandbox error: ${safeMessage(r.error)}`);
  if (r.signal) return failure(`sandbox killed by ${r.signal}`);
  if (r.status !== 0) return failure(`sandbox exited ${r.status}: ${String(r.stderr || "").split("\n")[0].slice(0, 120)}`);
  try {
    const out = JSON.parse(r.stdout);
    return out && typeof out.success === "boolean" ? out : failure("sandbox returned no score");
  } catch { return failure("sandbox returned unreadable output"); }
}
