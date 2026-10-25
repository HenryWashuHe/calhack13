# Smoke runs, 2026-10-25: the loop against seeded random search on two families

Scope: seven small live runs of the iteration engine on two real task families, written to
`scripts/iteration/results/jobshop-smoke-1/` through `jobshop-smoke-4/` and `binpack-smoke-1/` through
`binpack-smoke-3/` (`result.json`, `ledger.json`, `bundle.json`, sponsor exports). One seed per run,
20-instance panels, five model calls at most. Runs after the first two were added at the owner's request.
This is a smoke test of the mechanism, not evidence of recursive improvement, transfer between real
organizations, or anything about other families.

## Setup

**Where the generated rules ran (correction, 2026-10-25 07:00Z).** In every run recorded here the model-written
rules were evaluated with `node:vm` inside the runner's own process, which had loaded `.env` and therefore
held the model API key; `node:vm` is not a security boundary, and a rule's top-level `this` reaches the host
realm. Henry's Claude demonstrated the escape with a hand-written probe. There is no indication that any
generated rule did anything of the kind; the point is that the boundary did not exist while these results were
produced. A second review showed that an empty-environment child alone is not a filesystem or network
boundary either: the child ran as the same user and could read the repository's `.env`. Since
`families/sandbox.mjs` (second revision) every rule is scored in a child with an empty environment, code
generation from strings disabled, and, on this macOS host, a `sandbox-exec` profile that denies all network
and all reads outside the iteration scripts and the child's own work directory; the runner no longer loads
the key into its own environment and refuses live runs on hosts without an OS sandbox. A third revision, after
Codex's policy review, made that read policy default-deny with declared runtime roots (the second revision's
deny-list left any checkout under an undeclared root readable), moved the profile out of the child's writable
directory and regenerates it per child, and stopped counting Linux `unshare -rn` (network only) as a sandbox
for live admission. The recorded results are unchanged by all of this; they were produced without those
boundaries, and every run here predates them.


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

## Second family, `binpack-smoke-2` (sizes 20–70, target 1.08, gate = incumbent only)

| gen | incumbent | candidates | promoted |
|---|---|---|---|
| 1 | 4/20 (first-fit) | 8, 8, 8, 3 | yes, 8/20 |
| 2 | 12/20 | 13, 12, 8, 8 | yes, 13/20 |
| 3 | 8/20 | 6, 6, 4, 6 | none |
| 4 | 7/20 | 6, 7, 9, 7 | yes, 9/20 |
| 5 | 6/20 | 5, 5, 5, 5 | none |

Frontier after five generations: `(item, bin) => bin.fits === 0 ? 1000 : -bin.fits - (bin.fits < 20 ? 10 :
0) - bin.index * 0.001`, best-fit with a perfect-fit bonus and a penalty for leaving a gap too small for
any item. Confirmation on a fresh panel: **10/20**. Same panel, report-only:

| rule | final panel | mean bins / lower bound |
|---|---|---|
| frontier (generation 4 model rule) | 10/20 | 1.0805 |
| fixed-best-fit | 9/20 | 1.0829 |
| random-search (best of 67) | 8/20 | 1.0850 |
| root / fixed-first-fit | 3/20 | 1.0959 |
| fixed-worst-fit | 0/20 | 1.1834 |

5 calls, 20,965 input / 5,148 output tokens; ledger 62 records, verified. One instance over best-fit and two
over random search on twenty: the right direction and the right shape of rule, at a margin a single seed
cannot support. The model rediscovered best-fit in generation 1 of the saturated run and here found the
standard refinement of it; that is a fair description of what one call per generation buys on this family.

## Second family, `binpack-smoke-3` (a second seed at sizes 20–70, target 1.08)

| gen | incumbent | candidates | promoted |
|---|---|---|---|
| 1 | 6/20 (first-fit) | 5, 5, 6, 6 | none |
| 2 | 8/20 | 11, 12, 12, 12 | yes, 12/20 |
| 3–5 | 5, 9, 6 | never ≥ incumbent + 1 | none |

Frontier: `(item, bin) => -bin.fits + (bin.fits >= 20 ? 15 : 0) + (bin.fits === 0 ? 500 : 0) - bin.index *
0.001`, best-fit with a perfect-fit bonus and a preference for leaving room for another item. Confirmation
on a fresh panel: **10/20**, below its 12/20 selection score. Same panel: best-fit 11/20, random-search
11/20, first-fit 6/20, worst-fit 0/20. **A loss by one instance** against both best-fit and random
search, with a marginally worse mean ratio (1.0833 against 1.0823). 5 calls, 21,226 input / 4,082 output
tokens; ledger 60 records, verified.

## What the seven runs say, and what they do not

1. **The loop runs end to end on two real families with a real model, with only the family swapped**:
   sealed panels, frozen plans, verdicts for every candidate, promotions, fresh-panel confirmations, sealed
   bundles, and the two sponsor shapes, all in verified ledgers. Same engine, mutator, cascade, runner and
   exports for job-shop dispatching and online bin packing; the domain supplied an instance generator, a
   scorer and its controls, nothing else.
2. **Against seeded random search on a fresh panel: two wins, one tie.** Run 2's rule beat it 19 to 14
   (fixed-mwkr 15); run 3's rule tied it 17 to 17 (fixed-mwkr 15) with a better mean ratio (1.085 against
   1.120); run 4's rule beat it 18 to 12 (fixed-mwkr 15). The model rule beat every fixed rule on all three
   panels and had the best mean makespan ratio on all three (1.095, 1.085, 1.086). Three seeds, sixty
   fresh instances on job-shop: a consistent edge over hand-written rules and over random search. On bin
   packing the two discriminating seeds split: one instance above best-fit and two above random search in
   run 2, one instance below both in run 3; the saturated first run separated nothing. Two families, five
   informative seeds: a clear edge on job-shop, parity with best-fit on bin packing, and margins on the
   second family that are inside the noise of a 20-instance panel. The honest summary is that the loop
   rediscovers the strong classical rule on bin packing and refines it without reliably beating it.
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

Not established: a statistically supported advantage over random search or best-fit (job-shop: two wins
and a tie on three seeds; bin packing: one win and one loss by one instance), transfer between organizations
(the bundles were exported but adopted by nobody), results on any further family, and anything about the
forge lane's real method-generation path, which this engine does not touch. Improvement after generation 1
was observed in four of five informative seeds (counting run 3's generation-2 promotion).

## Files

`scripts/iteration/results/jobshop-smoke-1/{result,ledger}.json`;
`scripts/iteration/results/jobshop-smoke-2/`, `jobshop-smoke-3/`, `jobshop-smoke-4/`, `binpack-smoke-1/`,
`binpack-smoke-2/` and `binpack-smoke-3/`: `{result,ledger,bundle}.json`,
`fetch-offer.txt`, `fetch-observation.json`, `elastic-bulk.ndjson`. Reproduce the mechanism without model calls with
`node scripts/iteration/run-jobshop.mjs --mock`; the live runs are not deterministic across model calls.
