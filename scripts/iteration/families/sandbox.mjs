// Where generated rules run. `node:vm` is not a security boundary: a rule's top-level `this` is a
// contextified host object, so a rule can reach the host realm. Every rule is therefore scored in a
// separate child process started with an EMPTY environment (no API key, no .env), an empty working
// directory, a wall-clock timeout and SIGKILL; the parent never evaluates model text itself, it only
// passes it as JSON. A breach reaches a process that holds nothing. Residual: a rule could still burn the
// child's CPU until the wall timeout, or spawn grandchildren that outlive the SIGKILL of the child alone;
// the namespace sandbox behind `lab-exec` (no network, chroot, nobody) is the stronger boundary and is
// where lab methods run; this is the cheapest boundary that keeps the key out of reach.
import { spawnSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

const CHILD = fileURLToPath(new URL("./rule-child.mjs", import.meta.url));
let emptyDir = null;
const workdir = () => (emptyDir ||= fs.mkdtempSync(path.join(os.tmpdir(), "genesis-rule-")));

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
 * Scores one rule on one instance in a child process. `family` names the module the child may load
 * (allow-listed there); `options` are the family's own numeric settings; `timeoutMs` is the per-phase vm
 * timeout the child applies and the basis of the parent's wall timeout. Synchronous; never throws.
 */
export function scoreInChild({ family, source, instance, options, timeoutMs }) {
  const wall = Math.max(250, 2 * timeoutMs + 150);
  let r;
  try {
    r = spawnSync(process.execPath, ["--no-warnings", CHILD], {
      input: JSON.stringify({ family, source: String(source), instance, options, timeoutMs }),
      env: {}, cwd: workdir(), encoding: "utf8", timeout: wall, killSignal: "SIGKILL", maxBuffer: 1 << 20, windowsHide: true,
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
