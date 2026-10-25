# Why generation 2 regressed: B's revised child against A's parent on rough-transport

Analysis of one retained comparison, built only from files on `main`. No model call, no rerun, no
new score. It explains the mechanism behind a verdict the predeclared trial already reached; it does
not change that verdict or claim anything about any other family.

**The comparison.** `docs/invention-lab/evidence/live-method-20261024-b-revision-receiver/vs-parent/`
(below, `R/`): plan `rough-transport-child-vs-parent`, criterion `success-count-margin` with
`min_margin` 1 on six `rough-transport/1/{0..5}` instances (`R/plan.json`, `R/criterion.json`).
Candidate: B's revised child, method `4f455dc6…` (192 lines, `docs/invention-lab/evidence/live-method-20261024-b-revision/method-package.json`).
Control: A's generated parent, method `8215c367…` (137 lines, `docs/invention-lab/evidence/live-method-20261024-generated/method-package.json`).
Verdict: `rejected`, "candidate 3/6 does not exceed control 6/6 by 1" (`R/verdict.json`).

## 1. What each method did, experiment by experiment

Rows are joined from `R/{candidate,control}/transcript.json` (one record per `experiment` call:
instance, design hash, step limit, measured `simulation_steps`) with the result artifact each record
names (`R/{candidate,control}/artifacts/<result sha256>`: `metrics.success`, `metrics.progress`).

**Parent (control): 6 experiments, 5,205 steps, one design, 6/6.**

| # | instance | design | steps | success |
| ---: | --- | --- | ---: | --- |
| 0 | 0 | `2483f08f` | 805 | yes |
| 1 | 1 | `2483f08f` | 581 | yes |
| 2 | 2 | `2483f08f` | 562 | yes |
| 3 | 3 | `2483f08f` | 860 | yes |
| 4 | 4 | `2483f08f` | 823 | yes |
| 5 | 5 | `2483f08f` | 1,574 | yes |

The parent sorted the instances by `gap + step_height`, screened one candidate on the first, saw a
success, confirmed on the other five and stopped at six for six (parent `method.py` lines 64 to 67
and 111 to 130). On rough-transport both of those fields are zero (`evaluator/src/family.rs` lines
110 to 111 and 137 to 151, per the receiver-lane review of this analysis), so the sort was a no-op
and the parent simply screened instance 0 first. This trial therefore does not show that
hardest-first screening caused the parent's result; it shows one screen, five confirmations.

**Child (candidate): 32 experiments, 39,778 steps, 10 distinct designs, submitted a design it had
run twice.**

| # | inst | design | steps | ok | | # | inst | design | steps | ok |
| ---: | :-: | --- | ---: | :-: | --- | ---: | :-: | --- | ---: | :-: |
| 0 | 1 | `b1225902` | 700 | 1 | | 16 | 1 | `bdc7417e` | 593 | 1 |
| 1 | 0 | `1cc0dd6e` | 2,400 | 0 | | 17 | 0 | `bdc7417e` | 1,033 | 1 |
| 2 | 5 | `acd6b5dc` | 928 | 1 | | 18 | 5 | `bdc7417e` | 1,029 | 1 |
| 3 | 2 | `bdc7417e` | 556 | 1 | | 19 | 3 | `bdc7417e` | 2,400 | 0 |
| 4 | 3 | `f96fadab` | 2,400 | 0 | | 20 | 4 | `bdc7417e` | 742 | 1 |
| 5 | 4 | `ad495176` | 847 | 1 | | 21 | 1 | `ad495176` | 746 | 1 |
| 6 | 1 | `8420c8f8` | 529 | 1 | | 22 | 0 | `ad495176` | 1,326 | 1 |
| 7 | 0 | `707fe09e` | 2,400 | 0 | | 23 | 5 | `ad495176` | 1,227 | 1 |
| 8 | 5 | `20d7bb40` | 2,400 | 0 | | 24 | 2 | `ad495176` | 694 | 1 |
| 9 | 2 | `6b4dcd55` | 654 | 1 | | 25 | 3 | `ad495176` | 2,400 | 0 |
| 10 | 0 | `b1225902` | 2,400 | 0 | | 26 | 0 | `8420c8f8` | 925 | 1 |
| 11 | 1 | `acd6b5dc` | 532 | 1 | | 27 | 5 | `8420c8f8` | 1,006 | 1 |
| 12 | 0 | `acd6b5dc` | 911 | 1 | | 28 | 2 | `8420c8f8` | 522 | 1 |
| 13 | 2 | `acd6b5dc` | 536 | 1 | | 29 | 3 | `8420c8f8` | 2,400 | 0 |
| 14 | 3 | `acd6b5dc` | 2,400 | 0 | | 30 | 4 | `8420c8f8` | 660 | 1 |
| 15 | 4 | `acd6b5dc` | 740 | 1 | | 31 | 1 | `6b4dcd55` | 742 | 1 |

Per instance (child): 

| instance | experiments | successes | steps | hit the 2,400 cap | first success at # |
| :-: | ---: | ---: | ---: | ---: | ---: |
| 0 | 7 | 4 | 11,395 | 3 | 12 |
| 1 | 6 | 6 | 3,842 | 0 | 0 |
| 2 | 5 | 5 | 2,962 | 0 | 3 |
| 3 | 5 | 0 | 12,000 | 5 | never |
| 4 | 4 | 4 | 2,989 | 0 | 5 |
| 5 | 5 | 4 | 6,590 | 1 | 2 |

Instances 0 and 3 took 23,395 steps: 58.8% of the 39,778 development steps the child used, and
32.5% of the 72,000-step development grant in `R/plan.json`. Instance 3 failed in all five attempts,
each at the 2,400-step cap; the parent passed it in 860 steps.

## 2. The submission: a design tested on two of six instances

The child's stage 1 (experiments 0 to 9) ran ten different designs once each; six succeeded on
easy instances, four hit the cap. Stage 2 (10 to 31) extended designs by the estimator
`est(S) = ((successes + 0.3) / (tested + 1), mean progress)` and always picked the best
partially tested design (`child.py` lines 147 to 170). That drove four designs to full coverage,
each ending 5/6 with instance 3 as the only failure: `acd6b5dc`, `bdc7417e`, `ad495176`,
`8420c8f8`. The method's own request cap (`MAXREQ = 32`, self-imposed; the step grant was not
exhausted) was reached with `6b4dcd55` tested twice, both successes.

Final selection used the same estimator over all designs (`child.py` lines 174 to 183):

| design | tested | successes | `(s + 0.3) / (t + 1)` |
| --- | ---: | ---: | ---: |
| `6b4dcd55` | 2 | 2 | 0.767 |
| `acd6b5dc`, `bdc7417e`, `ad495176`, `8420c8f8` | 6 | 5 | 0.757 |

So the child submitted `6b4dcd55`, the two-test design, over the four fully tested contenders
(the receiver README's "submitted design `6b4dcd55…`" agrees; the four designs that failed their
single probe were also tested once, so it was not the least-tested design overall). It had been
developed on instances 1 and 2 only, leaving four unseen: 0, 3, 4 and 5. In final scoring it
succeeded on 1, 2 and 4 and failed on 0, 3 and 5 with progress 0.46, 0.40 and 0.49 (`R/../README.md`,
per-instance table): three of the four unseen instances failed, and unseen instance 4 succeeded.
Every submitted design, this one included, was then scored on all six frozen instances by the
trial; partial coverage was a property of the method's development, not of the comparison. The
parent scored 6/6.

## 3. The observed mechanism, then plausible contributors

Only the first item is observed in the retained records. The others are plausible contributors;
no ablation was run, so none is established as an isolated cause.

1. **A selection rule that rewards under-testing (observed).** The additive smoothing `(s + 0.3) / (t + 1)`
   ranks a perfect 2-of-2 above 5-of-6 (child `method.py` lines 147 to 149 and 176 to 185). This
   is the direct mechanism of the 3/6 submission. Had the rule required full coverage, one of the
   four 5/6 designs would have been submitted; their final scores were never run, so no number is
   claimed for them.
2. **The prescribed strategy replaced a working one (plausible).** The revision prompt asked for "sampling
   across instances rather than waiting for one difficult case to succeed"
   (`docs/invention-lab/evidence/live-method-20261024-b-revision/README.md`, "Before the request").
   The child did exactly that: ten one-off probes that produced no ranking signal, then
   confirmations that spent 23,395 steps on two instances. The parent's screen-one-then-confirm
   loop had solved the same family in 5,205 steps. Whether the strategy or the grid below did the
   damage cannot be separated from these files.
3. **The parameter grid was replaced, not extended (plausible).** Parent: radius {0.6, 0.8, 0.45, 1.0},
   speed {-6, -10, -3}, length {3.0, 3.8} (parent `method.py` lines 69 to 72). Child: radius
   {0.5, 0.8}, speed {-4, -7, -10}, length {2.4, 3.4}, plus a walls flag (child `method.py` lines
   88 to 92). The parent's winning region is absent, and no child design solved instance 3 (0 for 5).
4. **Feedback from a different family (plausible, and itself evidence).** The revision was prompted
   with the ravine-crossing receiver feedback, where the parent had scored 1/6
   (`…/b-revision/README.md`, `public-feedback.json`, 24 experiments), and examined on
   rough-transport, where the parent was already 6/6. For a transfer project that is not an
   invalid test: it is negative-transfer evidence, recording that feedback from one family did not
   carry to another.
5. **Population of one, six instances, margin of one (context).** One child per generation and a six-instance
   panel mean a single estimator quirk decides a generation; across three families the same parent
   scores 1, 6 and 1 (`docs/invention-lab/evidence/live-method-20261024-a-return/README.md`).

## 4. What the trial got right, and what the engine proposal changes

The predeclared comparison caught the regression: child and parent ran on the same frozen
instances, once each, every submitted design was scored on all six, and the child lost. Nothing
here revises that. The proposed iteration contract (`docs/invention-lab/contracts/iteration.md`,
PR #144, a proposal under review) would change the outer layer only. It scores every arm on every
instance of a frozen plan and rejects an arm that loses, which is what this trial already did; it
cannot stop a method's internal estimator from submitting a partially tested design, and it does
not claim to. What it would add, per family, is several candidates per generation instead of one,
the incumbent kept as an arm so a replaced grid is judged against what it replaced, feedback carried
only within a family, larger regenerated panels and a final fresh confirmation panel. Whether those
reduce regressions like this one is an empirical question for the next generations; this analysis
does not establish it.
