// Where generated rules run. `node:vm` is not a security boundary: a rule's top-level `this` is a
// contextified host object, so a rule can reach the host realm. Every rule is therefore scored in a
// separate child process, and that child is confined as far as this host allows:
//   - empty environment (no API key, no .env), an empty working directory, a wall-clock kill;
//   - `--disallow-code-generation-from-strings`, so `Function(...)`/`eval` throw in every realm of the
//     child, which closes the constructor.constructor class of escapes outright;
//   - an OS sandbox when one is available. On macOS, `sandbox-exec` with a profile whose read policy is
//     default-deny: every file read is refused except the declared runtime roots (the system roots `/usr`,
//     `/bin`, `/sbin`, `/System`, `/Library`, `/private/etc`, `/private/var/db`, `/private/var/select`,
//     `/dev`, the root directory entry, and the prefix the node binary lives under), the iteration scripts
//     and the child's own work directory; writes are allowed only in that work directory; all network is
//     denied. Process-level operations (mach, sysctl, signals) stay allowed so the runtime can start.
//     The profile is written outside the child's writable directory and regenerated for every child, so a
//     child cannot alter the policy the next child runs under. On Linux, `unshare -rn` removes the network
//     only: that is NOT filesystem confinement, `describeSandbox()` says so, and the runner refuses live
//     runs there unless told otherwise.
// The parent never evaluates model text; it passes it as JSON. `describeSandbox()` says which layers are
// in force, with `confined` true only when both filesystem and network are confined, so a run can record
// it and refuse to proceed without a full OS boundary.
import { spawnSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

const CHILD = fileURLToPath(new URL("./rule-child.mjs", import.meta.url));
const ITERATION_DIR = fs.realpathSync(path.dirname(path.dirname(CHILD))); // scripts/iteration: the only code the child may read
const NODE_FLAGS = ["--no-warnings", "--disallow-code-generation-from-strings"];
let emptyDir = null;
const workdir = () => (emptyDir ||= fs.realpathSync(fs.mkdtempSync(path.join(os.tmpdir(), "genesis-rule-"))));
// Policy material lives in its own directory, which the profile never makes writable.
let policyDirPath = null;
const policydir = () => (policyDirPath ||= fs.realpathSync(fs.mkdtempSync(path.join(os.tmpdir(), "genesis-rule-policy-"))));

/** System roots every process needs to read, plus the prefix the node binary was installed under. */
/** Every path a confined child may read, beside its own work directory. Callers use it to keep secrets out of reach. */
export function sandboxReadRoots() {
  return [...runtimeReadRoots(), ITERATION_DIR];
}

function runtimeReadRoots() {
  const nodeReal = fs.realpathSync(process.execPath);
  const roots = ["/usr", "/bin", "/sbin", "/System", "/Library", "/private/etc", "/private/var/db", "/private/var/select", "/dev", path.dirname(path.dirname(nodeReal))];
  for (const prefix of ["/opt/homebrew", "/usr/local"]) if (nodeReal.startsWith(prefix + "/")) roots.push(prefix);
  return [...new Set(roots)];
}

const sbplQuote = (p) => `"${String(p).replace(/["\\]/g, "\\$&")}"`;
const subpaths = (paths) => paths.map((p) => `(subpath ${sbplQuote(p)})`).join(" ");
/** Default-deny reads and writes with declared allowances; network denied outright. The loader needs the root directory entry itself. */
function darwinProfile(work) {
  return [
    "(version 1)", "(allow default)", "(deny network*)",
    "(deny file-read*)",
    "(allow file-read-metadata)",
    `(allow file-read* (literal "/") ${subpaths([...runtimeReadRoots(), ITERATION_DIR, work])})`,
    "(deny file-write*)",
    `(allow file-write* (subpath ${sbplQuote(work)}) (literal "/dev/null"))`,
  ].join("\n");
}

const hasExecutable = (p) => { try { fs.accessSync(p, fs.constants.X_OK); return true; } catch { return false; } };
const onPath = (name) => (process.env.PATH || "").split(path.delimiter).some((d) => d && hasExecutable(path.join(d, name)));

/**
 * Which confinement layers apply on this host; `os` is "sandbox-exec", "unshare", or "none". `confined` is
 * true only when both the filesystem and the network are confined; a network-only wrapper is reported as
 * such and does not count as a sandbox for live runs.
 */
export function describeSandbox() {
  const os_ = process.platform === "darwin" && hasExecutable("/usr/bin/sandbox-exec") ? "sandbox-exec"
    : process.platform === "linux" && onPath("unshare") ? "unshare" : "none";
  const filesystem = os_ === "sandbox-exec" ? "read: declared runtime roots, iteration scripts and own work dir only (default deny); write: own work dir only" : "not confined";
  const network = os_ === "none" ? "not confined" : "denied";
  return { process: "child, empty environment, empty cwd, wall timeout, SIGKILL", code_generation: "disallowed from strings", os: os_, filesystem, network,
    confined: filesystem !== "not confined" && network === "denied" };
}

function command(work) {
  const sb = describeSandbox();
  if (sb.os === "sandbox-exec") {
    // Outside the child's writable directory, and rewritten for every child: a previous child cannot
    // have altered the policy the next one runs under.
    const profile = path.join(policydir(), "profile.sb");
    fs.writeFileSync(profile, darwinProfile(work));
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
  let r;
  try {
    const work = workdir();
    const [cmd, args] = command(work);
    r = spawnSync(cmd, args, {
      input: JSON.stringify({ family, source: String(source), instance, options, timeoutMs }),
      env: {}, cwd: work, encoding: "utf8", timeout: wall, killSignal: "SIGKILL", maxBuffer: 1 << 20, windowsHide: true,
    });
  } catch (err) { return failure(`sandbox spawn failed: ${safeMessage(err)}`); }
  if (r.error && r.error.code === "ETIMEDOUT") return failure(`rule exceeded the wall timeout (${wall} ms); process killed`);
  if (r.error) return failure(`sandbox error: ${safeMessage(r.error)}`);
  if (r.signal) return failure(`sandbox killed by ${r.signal}`);
  if (r.status !== 0) return failure(`sandbox exited ${r.status}: ${String(r.stderr || "").split("\n")[0].slice(0, 120)}`);
  try {
    const out = JSON.parse(r.stdout);
    return out && typeof out.success === "boolean" ? out : failure("sandbox returned no score");
  } catch { return failure("sandbox returned unreadable output"); }
}
