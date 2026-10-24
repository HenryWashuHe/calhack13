# Return to A: parent and child on return-mix

`org_example_a` evaluated both the method it originally generated (the parent,
`8215c367…`) and B's revised child (`4f455dc6…`) on a third family,
**return-mix**. There were three plans,
[declared before execution](https://github.com/HenryWashuHe/genesis-history-experiment/issues/1#issuecomment-6388057984).
Every slot ran once, with no reruns, no selection and no edits.

## Result: neither generated method beats seeded random search

| Plan | Candidate | Control | Verdict |
| --- | ---: | ---: | --- |
| Child vs seeded random search | 0/6 | 3/6 | `rejected` |
| Child vs parent | 0/6 | 1/6 | `rejected` |
| Parent vs seeded random search | 1/6 | 3/6 | `rejected` |

| Per instance (success, progress) | /0 | /1 | /2 | /3 | /4 | /5 |
| --- | --- | --- | --- | --- | --- | --- |
| Child | 0, 0.65 | 0, 0.40 | 0, 0.43 | 0, 0.41 | 0, 0.68 | 0, 0.45 |
| Parent | 0, 0.80 | 1 | 0, 0.80 | 0, 0.42 | 0, 0.61 | 0, 0.47 |
| Random search | 0, 0.77 | 1 | 0, 0.76 | 1 | 0, 0.81 | 1 |

Each method's run is identical across the plans it appears in. Model calls in
every slot are measured 0, and `lab-exec check` reports `WithinLimits` for all
six slots.

## Across the three families so far

| Family (6 instances each) | Parent | B's child | Random search | Fixed-cart |
| --- | ---: | ---: | ---: | ---: |
| ravine-crossing ([receiver](../live-method-20261024-receiver/README.md)) | 1 | not run | 2 | 0 |
| rough-transport ([B receiver](../live-method-20261024-b-revision-receiver/README.md)) | **6** | 3 | 4 | 2 |
| return-mix (this bundle) | 1 | 0 | 3 | not run |

**Read:** the hand-written seeded random search is the most reliable method
across families. The model-generated parent wins clearly on one family and
loses on the other two. B's feedback-driven revision is below its parent on
both families it ran. So this first A → B → A pass did not produce an improved
method. That's an honest null result for the loop, not a G3 or G4 success. The
mechanism itself (generation, receiving-side execution, feedback, revision and
return evaluation with known costs) does run end to end.

`candidate-feedback.json` in each plan directory is the checked public feedback
for that candidate's run.

## Limits

- Six instances per family, one run per slot, deterministic methods. The
  sample is small.
- Same runtime build, unmerged #65 sandbox and host as the earlier receiver
  bundles. The organizations are labels on one operator's host, not
  independently authenticated parties.
- Provider-default sampling and an exploratory, operator-assigned history:
  not fixed-settings G4 evidence. No installation, promotion or improvement
  claim. G0 to G4 stay open.

## Costs

Each `cost-report.json` comes from `genesis-forge method-cost-assess` and has
the package-to-slot link matched. All values are known.

- `child-vs-random-search/` joins the child's revision generation: 1 call,
  20798/5741 tokens, 101151 ms, 75636 steps combined.
- `parent-vs-random-search/` joins the parent's generation: 1 call, 4028/5231
  tokens, 76265 ms, 44140 steps combined.

The child-vs-parent plan repeats both runs and is retained without a separate
report. Earlier attempts and earlier receiver trials are outside these scopes.

## Reproduce offline

```sh
shasum -a 256 -c SHA256SUMS
for t in child-vs-random-search child-vs-parent parent-vs-random-search; do
  (cd $t && lab-score verdict plan.json criterion.json candidate candidate-score control control-score)
done
for t in child-vs-random-search parent-vs-random-search; do
  (cd $t && genesis-forge method-cost-assess --manifest cost-manifest.json | cmp - cost-report.json)
done
```
