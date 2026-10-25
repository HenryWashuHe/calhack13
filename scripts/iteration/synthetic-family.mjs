// SYNTHETIC family for the engine's tests only. It proves the protocol, not any task.
// A "package" is a JSON source `{ "guess": number }`; an instance is a hidden target drawn around
// the family's centre; success is |guess - target| <= tolerance. Everything is seeded and deterministic.
import { sha256 } from "./engine.mjs";

const unit = (s) => parseInt(sha256(s).slice(0, 8), 16) / 0xffffffff; // deterministic number in [0, 1)

export function syntheticFamily({ id, seed, target, tolerance = 3, jitter = 8 }) {
  const targetOf = (instance) => target + Math.round((unit(`${instance}:target`) * 2 - 1) * jitter);
  return {
    id,
    root: () => JSON.stringify({ guess: 0 }),
    instances: (instanceSeed, n) => Array.from({ length: n }, (_, i) => `${id}/${String(instanceSeed).slice(0, 8)}/${i}`),
    score: (pkg, instance) => {
      let guess = NaN;
      try { guess = Number(JSON.parse(pkg.source).guess); } catch { /* malformed source scores as a failure */ }
      const error = Number.isFinite(guess) ? targetOf(instance) - guess : NaN;
      const distance = Number.isFinite(error) ? Math.abs(error) : Infinity;
      return { success: distance <= tolerance, progress: Number.isFinite(error) ? Math.max(0, 1 - distance / 50) : 0, cost: 1, detail: { signed_error: Number.isFinite(error) ? error : null } };
    },
    seed,
  };
}

// Two fixed controls, as a real family would have a fixed design and a hand-written baseline.
export function syntheticControls(family) {
  return [
    { id: "fixed-zero", package: { sha256: sha256(family.root()), source: family.root(), origin: "control", parent: null, generation: 0 } },
    { id: "fixed-twenty-five", package: { sha256: sha256(JSON.stringify({ guess: 25 })), source: JSON.stringify({ guess: 25 }), origin: "control", parent: null, generation: 0 } },
  ];
}

// A mutator that reads attributed feedback (the signed error per instance) and proposes k variants
// around the correction; with no feedback it spreads symmetrically. Stands in for a model call.
export function syntheticMutator({ step = 4 } = {}) {
  return {
    propose: ({ parent, feedback, k }) => {
      const guess = Number(JSON.parse(parent.source).guess) || 0;
      const errors = (feedback?.instances || []).map((i) => i.detail?.signed_error).filter((e) => typeof e === "number");
      const mean = errors.length ? errors.reduce((a, b) => a + b, 0) / errors.length : 0;
      const offsets = errors.length ? [mean, mean / 2, mean * 1.5, mean / 4, mean + step, mean - step] : [step, -step, 2 * step, -2 * step, 4 * step, -4 * step];
      return offsets.slice(0, k).map((o, i) => ({ source: JSON.stringify({ guess: Math.round(guess + o) }), note: `variant ${i} (offset ${o.toFixed(1)})` }));
    },
  };
}
