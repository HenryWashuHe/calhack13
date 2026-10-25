// Where generated rules run. `node:vm` is not a security boundary: a rule's top-level `this` is a
// contextified host object, so a rule can reach the host realm. Every rule is therefore scored in a
// separate child process, and that child is confined as far as this host allows:
//   - empty environment (no API key, no .env), an empty working directory, a wall-clock kill;
//   - `--disallow-code-generation-from-strings`, so `Function(...)`/`eval` throw in every realm of the
//     child, which closes the constructor.constructor class of escapes outright;
//   - an OS sandbox when one is available: on macOS, `sandbox-exec` with a profile that denies all
//     network, denies reading the home directory, the temp trees and other volumes (so the repository's
//     `.env` and sibling temp files are unreadable), allows reading only the iteration scripts and the
//     child's own work directory, and allows writing only there; on Linux, `unshare -rn` for no network
//     when present (no filesystem confinement; the runner refuses live runs there unless told otherwise).
// The parent never evaluates model text; it passes it as JSON. `describeSandbox()` says which layers are
// in force so a run can record it and refuse to proceed without an OS boundary.
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

const sbplQuote = (p) => `"${String(p).replace(/["\\]/g, "\\$&")}"`;
function darwinProfile(work) {
  const home = os.homedir();
  return [
    "(version 1)", "(allow default)", "(deny network*)",
    `(deny file-read* (subpath ${sbplQuote(home)}) (subpath "/private/tmp") (subpath "/tmp") (subpath "/private/var/folders") (subpath "/Volumes"))`,
    "(allow file-read-metadata)",
    `(allow file-read* (subpath ${sbplQuote(ITERATION_DIR)}) (subpath ${sbplQuote(work)}) (regex #"/package\\.json$"))`,
    "(deny file-write*)",
    `(allow file-write* (subpath ${sbplQuote(work)}) (subpath "/dev"))`,
  ].join("\n");
}

const hasExecutable = (p) => { try { fs.accessSync(p, fs.constants.X_OK); return true; } catch { return false; } };
const onPath = (name) => (process.env.PATH || "").split(path.delimiter).some((d) => d && hasExecutable(path.join(d, name)));

/** Which confinement layers apply on this host; `os` is "sandbox-exec", "unshare", or "none". */
export function describeSandbox() {
  const os_ = process.platform === "darwin" && hasExecutable("/usr/bin/sandbox-exec") ? "sandbox-exec"
    : process.platform === "linux" && onPath("unshare") ? "unshare" : "none";
  return { process: "child, empty environment, empty cwd, wall timeout, SIGKILL", code_generation: "disallowed from strings", os: os_,
    filesystem: os_ === "sandbox-exec" ? "read: iteration scripts and own work dir only; write: own work dir only" : "not confined",
    network: os_ === "none" ? "not confined" : "denied" };
}

function command(work) {
  const sb = describeSandbox();
  if (sb.os === "sandbox-exec") {
    const profile = path.join(work, "profile.sb");
    if (!fs.existsSync(profile)) fs.writeFileSync(profile, darwinProfile(work));
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
