// Evaluation cascade outside the engine: ask the inner mutator for `factor × k` proposals, screen them on a
// separate seeded screening panel (never the sealed plan panel), hand the engine the top k. Screening
// evaluations are counted here and reported by the runner; they are not selection evidence.
import { sha256, canon } from "./engine.mjs";

const successes = (rows) => rows.filter((r) => r && r.success === true).length;
const meanProgress = (rows) => rows.length ? rows.reduce((a, r) => a + (typeof r.progress === "number" ? r.progress : 0), 0) / rows.length : 0;

function screenOne(family, instances, source) {
  const rows = instances.map((instance) => { try { return family.score({ sha256: sha256(source), source }, instance) || {}; } catch (err) { return { success: false, progress: 0, error: err.message }; } });
  return { source, success_count: successes(rows), mean_progress: meanProgress(rows) };
}

export function cascade(mutator, { family, screenSeed, m = 3, factor = 2 } = {}) {
  if (!family || !screenSeed) throw new Error("cascade needs a family and a screenSeed");
  const state = { evaluations: 0, rounds: [] };
  return {
    usage: mutator.usage,
    counts: state,
    observe: (...args) => (mutator.observe ? mutator.observe(...args) : undefined),
    propose({ parent, feedback, k, generation }) {
      const proposed = mutator.propose({ parent, feedback, k: Math.max(1, Math.round(k * factor)), generation }) || [];
      if (proposed.length <= k) return proposed;
      const instances = family.instances(sha256(canon(["screen", String(screenSeed), family.id, generation])), m);
      const screened = proposed.map((c) => ({ ...c, screen: screenOne(family, instances, c.source) }));
      state.evaluations += proposed.length * instances.length;
      const ranked = [...screened].sort((a, b) => b.screen.success_count - a.screen.success_count || b.screen.mean_progress - a.screen.mean_progress);
      state.rounds = [...state.rounds, { generation, proposed: proposed.length, kept: k, panel: instances.length, dropped: ranked.slice(k).map((c) => c.screen) }];
      return ranked.slice(0, k).map(({ screen, ...c }) => ({ ...c, note: `${c.note || "candidate"}; screened ${screen.success_count}/${instances.length}` }));
    },
  };
}
