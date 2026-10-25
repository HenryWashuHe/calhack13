# B's revised method: receiving-side trials on rough-transport

`org_example_b` ran the exact revised child from
[B's feedback-driven revision](../live-method-20261024-b-revision/README.md)
(`4f455dc6…`) on the distinct **rough-transport** task. It was evaluated against
three controls, in three plans
[declared before execution](https://github.com/HenryWashuHe/genesis-history-experiment/issues/1#issuecomment-6387983350).
Every slot ran once, with no reruns, no selection among runs and no edits.

## Result: the revision did not help

| Plan | Child | Control | Verdict |
| --- | ---: | ---: | --- |
| **Primary:** vs seeded random search | 3/6 | 4/6 | `rejected` |
| Secondary: vs exact parent `8215c367…` | 3/6 | **6/6** | `rejected` |
| Reference: vs fixed-cart | 3/6 | 2/6 | `accepted` |

| Per instance (success, progress) | /0 | /1 | /2 | /3 | /4 | /5 |
| --- | --- | --- | --- | --- | --- | --- |
| Child (all three plans) | 0, 0.46 | 1 | 1 | 0, 0.40 | 1 | 0, 0.49 |
| Parent | 1 | 1 | 1 | 1 | 1 | 1 |
| Random search | 1 | 1 | 1 | 0, 0.22 | 1 | 0, 0.19 |
| Fixed-cart | 0, 0.38 | 0, 0.33 | 1 | 0, 0.07 | 1 | 0, 0.31 |

The child's runs are identical in all three plans: 32 experiments, 39778
development steps and the same submitted design (`6b4dcd55…`). Model calls in
every slot are measured 0, and `lab-exec check` reports `WithinLimits` for all
six slots.

**Read:** A's original generated method (the parent), developed against the
ravine-crossing description, solves all six rough-transport instances. It uses
6 experiments and 5205 steps. B's feedback-driven revision reaches 3/6 with 32
experiments and 39778 steps. On this task the revision is a regression from
its parent and worse than seeded random search. It beats only the fixed design.
This is an honest negative result for this one exploratory revision. It does
not show that revision can't help, and it is not a G3 success.

Two observations worth keeping for the next attempt:
- The parent's prior rejection on ravine-crossing (1/6 vs random search 2/6)
  didn't predict its rough-transport behaviour. Selection on one family's
  verdict would have discarded the better method here.
- `vs-random-search/candidate-feedback.json` (`lab-score feedback`,
  `f9759134…`) is the checked public feedback for the child's primary run, for
  any further revision.

## Limits

- One run per slot. The methods are deterministic here, so the comparison is
  exact, but six instances is a small sample.
- The runtime, sandbox and executor are the same as in
  [the first receiver bundle](../live-method-20261024-receiver/README.md):
  declared `Python 3.14.0` on a Linux standalone build, unmerged #65
  `namespaces` mode on one Linux host, experimental isolation. No ref was retagged.
- Provider-default sampling and an operator-assigned exploratory history: this
  is not fixed-settings G4 evidence. No adoption, installation, return to A or
  improvement claim. G0 to G4 stay open.

## Costs

`vs-random-search/cost-report.json`, from `genesis-forge method-cost-assess`,
joins the revision's generation (operation
`live-b-revision-20261024t182242z-80adc80d`) to the primary trial. The child
package's link to the candidate slot matches. All values are known: 1 call,
20798 input / 5741 output tokens, 140640 ms and 123113 simulation steps
combined. The secondary and reference plans each repeat the child's run and
cost about the same execution again. They are retained but not in this report.
Earlier generation attempts are outside this scope.

## Reproduce offline

From this directory, with `lab-exec`, `lab-score` and `genesis-forge` built:

```sh
shasum -a 256 -c SHA256SUMS
for t in vs-random-search vs-parent vs-fixed-cart; do
  (cd $t && lab-score verdict plan.json criterion.json candidate candidate-score control control-score)
done
(cd vs-random-search && genesis-forge method-cost-assess --manifest cost-manifest.json | cmp - cost-report.json)
```

`lab-exec check <plan> <slot> <state> <score>` validates each slot. Re-running
a slot needs root, the namespaces sandbox and a `python3` reporting exactly
`Python 3.14.0`, and must use a new state directory.
