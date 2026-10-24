# Selective transfer: B confirms, on fresh instances, a method rejected elsewhere

A's first generated method (`8215c367…`) was generated against ravine-crossing.
Against seeded random search it was rejected there, 1/6 vs 2/6 in
[the first receiving trial](../live-method-20261024-receiver/README.md), and
again on return-mix, 1/6 vs 3/6 in
[A's return trial](../live-method-20261024-a-return/README.md). In
[B's revision trials](../live-method-20261024-b-revision-receiver/README.md) the
same unchanged method happened to be a control and solved 6/6 rough-transport
instances. That was a post-hoc observation on six public seeds, so it was not a
result.

This bundle tests it on instances nobody had run. The spec was
[predeclared](https://github.com/HenryWashuHe/genesis-history-experiment/issues/1#issuecomment-6390339775)
(SHA-256 `9d23de58…`, pushed as `220f764`) before execution, with the reading
fixed in advance. Each slot ran once, with no reruns, no selection and no edits.

## Result: accepted on B's task, rejected on A's

| Trial | Parent | Random search | Verdict |
| --- | ---: | ---: | --- |
| **Primary:** rough-transport, seeds 3000..3011 | **12/12** | 7/12 | `accepted` |
| Selectivity check: ravine-crossing, seeds 3000..3011 | 4/12 | **10/12** | `rejected` |

| Per instance (success, or progress) | 3000 | 3001 | 3002 | 3003 | 3004 | 3005 | 3006 | 3007 | 3008 | 3009 | 3010 | 3011 |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| rough-transport, parent | 1 | 1 | 1 | 1 | 1 | 1 | 1 | 1 | 1 | 1 | 1 | 1 |
| rough-transport, random | 1 | 0.24 | 1 | 1 | 1 | 0.16 | 0.16 | 1 | 0.06 | 1 | 0.15 | 1 |
| ravine-crossing, parent | 0.52 | 0.50 | 0.77 | 0.79 | 0.90 | 1 | 1 | 1 | 0.98 | 0.84 | 1 | 0.95 |
| ravine-crossing, random | 1 | 1 | 1 | 0.78 | 1 | 1 | 1 | 1 | 1 | 0.85 | 1 | 1 |

All four slots are eligible under process-comparison/1's rule: every execution
is `output_generated`, `lab-exec check` reports `WithinLimits`, and all usage
is known. Model calls are measured 0 in every slot.

| Slot | Experiments | Simulation steps | Wall ms |
| --- | ---: | ---: | ---: |
| rough-transport, parent | 12 | 8,961 | 493 |
| rough-transport, random | 42 | 70,142 | 1,283 |
| ravine-crossing, parent | 18 | 25,699 | 680 |
| ravine-crossing, random | 56 | 38,532 | 1,176 |

**Read, as fixed before the run.** The primary is accepted, every slot is
eligible and ravine-crossing is not accepted, so the predeclared
selective-transfer reading holds. Selection on its ravine-crossing or return-mix verdicts would have
discarded a method that, on B's task, beats seeded random search on fresh
instances with about an eighth of its simulation steps. On rough-transport the
parent converges to one design that solves all twelve instances.

## What this is not

- **Not adoption.** B has a retained, eligible, accepted trial. Turning that
  into an adoption record and an invocation grant is the receiver-pinned path in
  #155/#159, which are unmerged. If they merge, this trial's exact plan
  reference is what a receiver `acceptance_plans` pin would name. That is a
  separate decision.
- **Not improvement.** The method is unchanged from A's generation. Nothing
  here shows a revised method beating its parent, and the one revision so far
  (B's child) regressed. G3 and G4 stay open.
- **Public instances.** Seeds 3000..3011 are a fresh public range, not a hidden
  holdout. The method was generated long before they existed and can't adapt
  to them between runs. It develops on these same instances inside its grant,
  as every receiving trial here does.
- **Hypothesis from earlier data.** The rough-transport hypothesis came from a
  post-hoc look at #116. This is its first confirmation on new data, with a
  single family pair and 12 instances each.
- **Experimental runtime.** Unmerged #65 executor (`6636596`) and unmerged #163
  `receive.py` (`66118d2`), on one Linux host. The declared Python 3.14.0
  runtime was used as is; nothing was retagged. The scorer's instance set is
  labelled `development` because example plans develop and score on one public
  panel. The metrics name each instance by seed.

process-comparison/1 is untouched. Its plan (4), parent vs random search on
1000..1011, still runs as frozen.

## Reproduce offline

From this directory, with `lab-exec` and `lab-score` built from `6636596`:

```sh
shasum -a 256 -c SHA256SUMS
for t in st-rough-transport st-ravine-crossing; do
  (cd run/$t && lab-score verdict plan.json criterion.json candidate candidate-score control control-score | cmp - verdict.json)
  for s in candidate control; do lab-exec check run/$t/plan.json $s run/$t/$s run/$t/$s-score; done
done
```

Re-running a slot needs root, the namespaces sandbox and a `python3` that
reports exactly `Python 3.14.0`, with a new state directory.
