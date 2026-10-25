// A model mutator for the iteration engine: k candidate programs from the parent, an archive of scored
// programs (worst first, best last) and id-free per-instance tables. It prescribes no strategy: the
// prompt shows what scored how and asks for distinct alternatives. The engine never hands it an instance
// id; `observe` strips ids from the verdicts the runner feeds back.
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import { sha256 } from "../engine.mjs";

const CALL_SCRIPT = fileURLToPath(new URL("./claude-call.mjs", import.meta.url));
const BLOCK = /=== CANDIDATE \d+ ===\s*```(?:js|javascript)?\s*([\s\S]*?)```/g;
const FENCE = /```(?:js|javascript)\s*([\s\S]*?)```/g;

/** Default transport: the child process. `env` is the only environment the call child gets (the key lives there, nowhere else). Returns {text, stop_reason, usage, refusal?}; throws on failure. */
export function childCall(req, { env = process.env } = {}) {
  const r = spawnSync(process.execPath, [CALL_SCRIPT], { input: JSON.stringify(req), encoding: "utf8", timeout: (req.timeout_ms || 170000) + 10000, env: { ...env, PATH: process.env.PATH || "" }, maxBuffer: 8 * 1024 * 1024 });
  if (r.error) throw r.error;
  if (r.status !== 0) throw new Error(r.stderr || `model call exited ${r.status}`);
  return JSON.parse(r.stdout);
}

export function parseCandidates(text) {
  const blocks = [...String(text || "").matchAll(BLOCK)].map((m) => m[1].trim());
  const fenced = blocks.length ? blocks : [...String(text || "").matchAll(FENCE)].map((m) => m[1].trim());
  return fenced.filter((s) => s.length > 0);
}

const idFree = (scores) => (scores || []).map(({ instance, ...rest }, index) => ({ index, ...rest }));
const meanProgress = (rows) => { const p = rows.filter((r) => typeof r.progress === "number"); return p.length ? p.reduce((a, r) => a + r.progress, 0) / p.length : 0; };
const better = (a, b) => b.success_count - a.success_count || b.mean_progress - a.mean_progress;

function table(rows) {
  const head = "| # | success | progress | detail |\n|---|---|---|---|";
  const body = rows.map((r) => `| ${r.index} | ${r.outcome === "scored" ? (r.success ? "yes" : "no") : r.outcome} | ${typeof r.progress === "number" ? r.progress.toFixed(3) : "-"} | ${r.detail ? JSON.stringify(r.detail) : ""} |`);
  return [head, ...body].join("\n");
}

function renderProgram(entry, i) {
  return `### Program ${i + 1}: ${entry.success_count}/${entry.of} instances succeeded, mean progress ${entry.mean_progress.toFixed(3)}${entry.role ? ` (${entry.role})` : ""}\n\`\`\`js\n${entry.source}\n\`\`\`\n${entry.rows ? table(entry.rows) : ""}`;
}

function renderReflection(r) {
  if (!r) return "";
  return `## Last generation\nParent scored ${r.parent.success_count}/${r.of}; the best child scored ${r.child.success_count}/${r.of} (${r.child.verdict}). Per-instance, the child ${r.child_won} times did better and ${r.parent_won} times worse than the parent.\n\nParent:\n\`\`\`js\n${r.parent.source}\n\`\`\`\nChild:\n\`\`\`js\n${r.child.source}\n\`\`\``;
}

/**
 * Builds the mutator. `task` describes the family in plain words; `featureDoc` is the family's own field
 * reference; `call` is injectable for tests. `observe` receives the engine's generation result so the
 * archive and the reflection record stay current; both hold sources and id-free rows only. `parentScores` is the
 * incumbent arm's scores from the ledger (slot "incumbent" of the generation's plan).
 */
export function claudeMutator({ task, featureDoc, subject = "a dispatching rule for job-shop scheduling", model = process.env.GENESIS_LLM_MODEL || "claude-sonnet-5-5", effort = "medium", call = childCall, archiveSize = 4, log = () => {} } = {}) {
  if (!task || !featureDoc) throw new Error("claudeMutator needs task and featureDoc");
  const state = { archive: {}, reflection: null, usage: { input_tokens: 0, output_tokens: 0, calls: 0, failures: 0 } };
  const remember = (source, success_count, of, rows, role) => {
    state.archive = { ...state.archive, [sha256(source)]: { source, success_count, of, mean_progress: meanProgress(rows), rows, role } };
  };
  const systemFor = (k) => `You improve ${subject}, one small JavaScript function at a time.\n\n${task}\n\n${featureDoc}\n\nYou will see programs that were already scored on hidden instances, worst first and best last, with per-instance tables. Instances change every generation, so a rule that fits one table will not carry. Write ${k} distinct candidate programs that you expect to beat the best one shown. Each candidate must be a single JavaScript arrow function expression returning a finite number, taking exactly the arguments the field reference describes, with no comments outside the function, no imports, no side effects.\n\nAnswer with exactly this layout and nothing else:\n=== CANDIDATE 1 ===\n\`\`\`js\n(...) => ...\n\`\`\`\n=== CANDIDATE 2 ===\n...`;
  return {
    usage: state.usage,
    propose({ parent, feedback, k, generation }) {
      const parentRows = idFree(feedback && feedback.instances);
      const parentCount = parentRows.filter((r) => r.outcome === "scored" && r.success).length;
      if (parentRows.length) remember(parent.source, parentCount, parentRows.length, parentRows, "current frontier");
      const entries = Object.values(state.archive).sort(better).slice(0, archiveSize).reverse(); // best last
      const shown = entries.length ? entries : [{ source: parent.source, success_count: 0, of: 0, mean_progress: 0, rows: null, role: "starting point, not yet scored" }];
      const user = `# Generation ${generation}\n\n## Scored programs (worst first, best last)\n${shown.map(renderProgram).join("\n\n")}\n\n${renderReflection(state.reflection)}\n\nWrite ${k} distinct candidates.`;
      state.usage.calls += 1;
      let out;
      try { out = call({ system: systemFor(k), user, model, max_tokens: 8000, effort }); }
      catch (err) { state.usage.failures += 1; log({ generation, error: err.message }); return []; }
      if (out.usage) { state.usage.input_tokens += out.usage.input_tokens || 0; state.usage.output_tokens += out.usage.output_tokens || 0; }
      if (out.refusal) { log({ generation, refusal: out.refusal }); return []; }
      const sources = parseCandidates(out.text);
      log({ generation, stop_reason: out.stop_reason, model: out.model, candidates: sources.length, usage: out.usage });
      return sources.slice(0, k).map((source, i) => ({ source, note: `model candidate ${i} (${out.model || model}, generation ${generation})` }));
    },
    observe({ generation, parent, parentScores, verdicts, of }) {
      if (parent && Array.isArray(parentScores) && parentScores.length) remember(parent.source, parentScores.filter((r) => r.outcome === "scored" && r.success).length, of, idFree(parentScores), "incumbent");
      const scored = (verdicts || []).filter((v) => v.package && typeof v.package.source === "string");
      for (const v of scored) remember(v.package.source, v.success_count, of, idFree(v.scores), `generation ${generation} candidate, ${v.verdict}`);
      const best = [...scored].sort((a, b) => b.success_count - a.success_count)[0];
      const parentEntry = parent && state.archive[sha256(parent.source)];
      if (!best || !parentEntry || !parentEntry.rows) return;
      const pairs = idFree(best.scores).map((r, i) => [r, parentEntry.rows[i]]).filter(([c, p]) => c && p && typeof c.progress === "number" && typeof p.progress === "number");
      state.reflection = { of, parent: { source: parent.source, success_count: parentEntry.success_count }, child: { source: best.package.source, success_count: best.success_count, verdict: best.verdict },
        child_won: pairs.filter(([c, p]) => c.progress > p.progress).length, parent_won: pairs.filter(([c, p]) => c.progress < p.progress).length };
    },
  };
}
