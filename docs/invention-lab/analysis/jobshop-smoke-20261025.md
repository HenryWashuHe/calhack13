# Job-shop smoke runs, 2026-10-25: the loop against seeded random search

Scope: four small live runs of the iteration engine on the first real task family, written to
`scripts/iteration/results/jobshop-smoke-1/` through `jobshop-smoke-4/` (`result.json`, `ledger.json`,
`bundle.json`, sponsor exports). One family, one seed each, 20-instance panels, five model calls at most.
Runs 3 and 4 were added at the owner's request after the first two.
This is a smoke test of the mechanism, not evidence of recursive improvement, transfer between real
organizations, or anything about other families.

## Setup

- Family `jobshop-10x5` (`scripts/iteration/families/jobshop.mjs`): 10 jobs × 5 machines, processing times
  1–99, instances derived from seeded ids and drawn fresh for every plan. A package is a JavaScript
  dispatching rule; success on an instance is makespan ≤ 1.20 × lower bound.
- Mutator: `claude-sonnet-5-5` (the repository's configured model), effort medium, one request per
  generation, behind a 2× cascade screened on three separate instances. The prompt shows scored programs
  worst-first/best-last with id-free per-instance tables and a parent-versus-best-child reflection
  record; no strategy is prescribed.
- Controls: fixed SPT, MWKR, LPT; `random-search`, the best of 67 seeded linear rules over five features,
  selected on a private six-instance training panel (402 evaluations, the loop's own selection budget;
  it scored 6/6 on its training panel).
- Criterion: success-count margin ≥ 1 on the same sealed panel; at most one promotion per generation.

## Run 1: `smoke-1`, gate = incumbent and every control (the contract's default)

| gen | incumbent (SPT) | fixed-mwkr | random-search | candidates | promoted |
|---|---|---|---|---|---|
| 1 | 9/20 | 8/20 | 14/20 | 14, 11, 10, 9 | none |
| 2 | 9/20 | 14/20 | 16/20 | 15, 15, 15, 14 | none |
| 3 | 11/20 | 18/20 | 18/20 | 16, 18, 18, 18 | none |

Stalled after three generations with no frontier and therefore no confirmation. 540 engine
evaluations, 72 screening evaluations, 3 calls, 10,775 input / 1,667 output tokens. Ledger: 46 records,
verified. **Null result under the strict gate.** The model's rules beat the SPT incumbent every
generation but never cleared the random-search control by one instance on the same panel; the frontier
stayed at the root, so every generation's parent was still SPT and nothing accumulated.

## Run 2: `smoke-2`, gate = incumbent only; controls compared on the final fresh panel

| gen | incumbent | candidates | promoted |
|---|---|---|---|
| 1 | 11/20 (SPT) | 15, 14, 15, 13 | yes, 15/20 |
| 2 | 15/20 | 14, 15, 15, 15 | none |
| 3 | 18/20 | 15, 15, 15, 16 | none |
| 4 | 15/20 | 15, 15, 15, 15 | none |
| 5 | 19/20 | 17, 17, 18, 17 | none |

Frontier: the generation-1 promotion, `(op, ctx) => op.remainingWork / (1 + 0.02 * op.duration) + 0.6 *
op.machineRemainingLoad`, package `16318049…`. Confirmation on a fresh 20-instance panel never used for
selection (`confirmation` record): **19/20**. Report-only, every control on that same final panel:

| rule | final panel | mean makespan / lower bound |
|---|---|---|
| frontier (generation 1 model rule) | 19/20 | 1.095 |
| fixed-mwkr | 15/20 | 1.150 |
| random-search (best of 67) | 14/20 | 1.147 |
| root / fixed-spt | 5/20 | 1.269 |
| fixed-lpt | 1/20 | 1.389 |

500 engine evaluations, 120 screening evaluations, 5 calls, 20,800 input / 2,950 output tokens
(about seven cents at list price). Ledger: 60 records, verified; bundle exported; Fetch offer and
observation body written; Elastic bulk body written and posted, every document refused by the data key
(`indices:admin/auto_create` unauthorized), recorded verbatim in `result.json`.

## Run 3: `smoke-3`, a second seed, same configuration as run 2 (gate = incumbent only)

| gen | incumbent | candidates | promoted |
|---|---|---|---|
| 1 | 8/20 (SPT) | 9, 10, 9, 11 | yes, 11/20 |
| 2 | 12/20 | 14, 12, 13, 12 | yes, 14/20 |
| 3 | 14/20 | 17, 17, 14, 15 | yes, 17/20 |
| 4 | 11/20 | 12, 14, 12, 12 | yes, 14/20 |
| 5 | 15/20 | 15, 16, 15, 16 | yes, 16/20 |

A promotion in every generation, each beating its own parent on that generation's sealed panel (the
incumbent column is the previous frontier re-scored on the new panel, which is why it moves up and down).
Frontier after five generations: `(op, ctx) => op.remainingWork + 0.3 * op.machineRemainingLoad - 1.0 *
op.duration`, package `63aadd6a…`. Confirmation on a fresh panel: **17/20**. Report-only, on that same
final panel:

| rule | final panel | mean makespan / lower bound |
|---|---|---|
| frontier (generation 5 model rule) | 17/20 | 1.085 |
| random-search (best of 67) | 17/20 | 1.120 |
| fixed-mwkr | 15/20 | 1.130 |
| root / fixed-spt | 12/20 | 1.190 |
| fixed-lpt | 5/20 | 1.354 |

500 engine evaluations, 120 screening evaluations, 5 calls, 20,783 input / 2,819 output tokens.
Ledger: 64 records, verified; bundle, Fetch offer and Elastic bulk body written (the data key again
refused every document).

## Run 4: `smoke-4`, a third seed, same configuration (gate = incumbent only)

| gen | incumbent | candidates | promoted |
|---|---|---|---|
| 1 | 8/20 (SPT) | 18, 14, 14, 13 | yes, 18/20 |
| 2 | 17/20 | 14, 16, 17, 15 | none |
| 3 | 16/20 | 16, 14, 14, 13 | none |
| 4 | 16/20 | 15, 16, 16, 16 | none |
| 5 | 16/20 | 17, 19, 17, 17 | yes, 19/20 |

Frontier after five generations: `(op, ctx) => 2 * op.remainingWork + op.machineRemainingLoad - 2.5 *
op.duration`, package `d807549f…`. Confirmation on a fresh panel: **18/20**. Same final panel, report-only:

| rule | final panel | mean makespan / lower bound |
|---|---|---|
| frontier (generation 5 model rule) | 18/20 | 1.086 |
| fixed-mwkr | 15/20 | 1.139 |
| random-search (best of 67) | 12/20 | 1.163 |
| root / fixed-spt | 10/20 | 1.194 |
| fixed-lpt | 2/20 | 1.339 |

500 engine evaluations, 120 screening evaluations, 5 calls, 20,789 input / 2,782 output tokens. Ledger: 61
records, verified; bundle and sponsor exports written (Elastic refused again by the data key).

## Second family: online bin packing, `binpack-smoke-1` (sizes 1–50, target 1.02, gate = incumbent only)

The same engine, mutator, cascade, runner and exports, with only the family swapped
(`scripts/iteration/families/binpack.mjs`, `--family binpack`): 100 items of size 1–50 into capacity-100
bins, a package is a bin-choice rule over the open bins that fit, success is bins ≤ 1.02 × the volume lower
bound, which at this size means hitting the bound exactly.

| gen | incumbent (first-fit) | candidates | promoted |
|---|---|---|---|
| 1 | 7/20 | 8, 8, 8, 8 | yes, 8/20 |
| 2–5 | 7, 7, 8, 8 | never ≥ incumbent + 1 | none |

The generation-1 promotion is literally best-fit, `(item, bin) => -bin.fits`. Confirmation on a fresh
panel: 8/20. Same panel: first-fit 8/20, best-fit 8/20, random-search 8/20, worst-fit 0/20, all four at the
identical mean ratio 1.0225. **A ceiling, not a result**: with sizes in [1, 50] every reasonable rule hits
the volume bound on the same instances and nothing can be told apart. The mechanism ran unchanged on a second
domain, which is what this run shows; the family as parameterised cannot discriminate. 5 calls, 19,689
input / 4,490 output tokens; ledger 60 records, verified. A second bin-packing run with sizes 20–70 and
target 1.08 (calibration on 20 fresh instances: first-fit 0.15, best-fit 0.25, random-search-of-24 0.25)
follows below.

## What the four runs say, and what they do not

1. **The loop runs end to end on a real family with a real model**: sealed panels, frozen plans,
   verdicts for every candidate, one promotion, a fresh-panel confirmation, a sealed bundle, and the two
   sponsor shapes, all in one verified ledger. That is the first time this engine has done anything but
   synthetic work.
2. **Against seeded random search on a fresh panel: two wins, one tie.** Run 2's rule beat it 19 to 14
   (fixed-mwkr 15); run 3's rule tied it 17 to 17 (fixed-mwkr 15) with a better mean ratio (1.085 against
   1.120); run 4's rule beat it 18 to 12 (fixed-mwkr 15). The model rule beat every fixed rule on all three
   panels and had the best mean makespan ratio on all three (1.095, 1.085, 1.086). Three seeds, sixty
   fresh instances, one family: a consistent edge over hand-written rules and over random search on this
   family, at a sample size that still calls for a second family before anyone generalizes.
3. **Whether later generations add anything depends on the seed.** In run 2 the gain came from generation
   1 and nothing after it: generations 2–5 never beat the incumbent by one instance. In run 3 every
   generation promoted, each child beating its parent on a sealed panel, and the frontier moved from a
   starting 8/20 to a confirmed 17/20; its final rule is a most-work-remaining variant with a machine-load
   term and a duration penalty. Run 4 sits between: a strong generation-1 rule (18/20), three flat
   generations, then a generation-5 improvement (19/20). Three seeds, three shapes: the recursive part can
   contribute, and it is not guaranteed to.
4. **The contract's gate is stricter than the literature's bar.** Beating random search by a margin on
   every generation's panel (run 1) is a different test from beating it on a final fresh panel (run 2),
   and under run 1's gate a frontier that cannot beat the control cannot move, so the mutator never gets
   to build on its own best candidate. Recommendation for the contract (not applied here): select against
   the incumbent per generation and compare against controls at confirmation, on the fresh panel, as run
   2 does; keep the per-generation control arms in the plan as retained evidence.
5. **Random search is a strong, cheap control**: 14/20 on the final panel after 402 training
   evaluations, and 18/20 on run 1's third panel. Any later claim must keep it.

Not established: an advantage over random search beyond this family (two wins and a tie on three
seeds), transfer between organizations (the bundles were exported but adopted by nobody), results on any
other family, and anything about the forge lane's real method-generation path, which this engine does not
touch. Improvement after generation 1 was observed in two of three seeds.

## Files

`scripts/iteration/results/jobshop-smoke-1/{result,ledger}.json`;
`scripts/iteration/results/jobshop-smoke-2/`, `jobshop-smoke-3/` and `jobshop-smoke-4/`: `{result,ledger,bundle}.json`,
`fetch-offer.txt`, `fetch-observation.json`, `elastic-bulk.ndjson`. Reproduce the mechanism without model calls with
`node scripts/iteration/run-jobshop.mjs --mock`; the live runs are not deterministic across model calls.
