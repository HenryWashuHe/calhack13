// Demo of the engine on the SYNTHETIC family: a few generations in lab A, then lab B adopts A's frontier
// and re-verifies it on its own instances. Prints the ledger summary. Proves the protocol, not any task.
import { createState, run, exportBundle, adopt, verifyLedger } from "./engine.mjs";
import { syntheticFamily, syntheticMutator, syntheticControls } from "./synthetic-family.mjs";

const criterion = { rule: "success-count-margin", min_margin: 1 };
const famA = syntheticFamily({ id: "synthetic-line", seed: 5, target: 60 });
const a = createState({ lab: "lab_a", seed: 5 });
const outA = run(a, { family: famA, mutator: syntheticMutator(), k: 4, controls: syntheticControls(famA), criterion, heldout: 6, generations: 6, stall: 3 });

const short = (h) => String(h).slice(0, 10);
for (const r of a.ledger.records) {
  if (r.type === "plan_frozen") console.log(`gen ${r.generation}  plan ${short(r.plan)}  arms ${r.arms.length}  instances ${r.instances.length}`);
  if (r.type === "verdict") console.log(`         ${r.slot.padEnd(12)} ${short(r.package)}  ${r.verdict.padEnd(8)} ${r.reason}`);
  if (r.type === "promotion") console.log(`         >> frontier ${short(r.package)} (${r.success_count}/${r.of} vs incumbent ${r.incumbent_count}/${r.of})`);
  if (r.type === "run_stopped") console.log(`stopped: ${r.reason} after ${r.generations} generations, ${r.promotions} promotions, ${r.evaluations} evaluations, ${r.calls} mutator calls`);
}
console.log(`ledger ${a.ledger.records.length} records, head ${short(a.ledger.head)}, verify: ${verifyLedger(a.ledger).ok}`);

if (outA.promotions) {
  const bundle = exportBundle(a, famA.id);
  console.log(`\nlab_a exports ${short(bundle.package.sha256)} with lineage ${bundle.lineage.map((l) => short(l.sha256)).join(" <- ")}`);
  for (const [lab, target] of [["lab_b (different task)", 120], ["lab_c (same task)", 60]]) {
    const fam = syntheticFamily({ id: "synthetic-line", seed: 21, target });
    const s = createState({ lab, seed: 21 });
    const v = adopt(s, { bundle, family: fam, controls: syntheticControls(fam), criterion, heldout: 6 });
    console.log(`${lab}: ${v.verdict} — ${v.reason}`);
  }
}
