# `mixed-terrain/1` (research family, calibration only)

Henry's Claude, evaluator lane. Implements
[proposal #195](../../../docs/invention-lab/proposals/mixed-terrain.md):
a separately versioned family where the cart first crosses a bump field and
then a gap. Nothing here changes v1. Generated methods do not run on this
family before the confirmation is published, and executor runs come only
after that.

## What's here

- `src/family.rs`: the generator. Bumps from x = 0.5 to `bump_end` ∈
  [2.5, 4.5] m (height [0.08, 0.3] m, spacing [0.6, 1.4] m), a 0.5 m flat
  run-up, then a gap of width [0.6, 1.8] m with the far bank offset by
  [−0.15, 0.15] m. Friction [0.4, 0.9], 0–2 pushes as in return-mix, build
  zone, budget and payload ranges as in v1. Its own generator salt.
- `src/sim.rs`: the episode. Terrain and success rule are new; body, joint,
  payload and stepping code is copied from v1's `sim.rs`, which is not
  edited. Success is ravine's rule: the payload held beyond the far edge
  plus 1 m for one continuous second and never lost (below y = −1.5).
  Episode 20 s, 2,400 steps.
- `src/score.rs`, `src/bin/lab-eval-mixed.rs`: v1's receipt shape and
  `instance` / `experiment` / `accept` commands for this family only, plus
  `identity`. No hidden comparison.
- Identity: `genesis-lab-eval-mixed@0.1.0`, a digest over this crate's
  sources and lockfile with the v1 evaluator digest folded in. A test pins
  v1's digest `1f03d1b6…`, so an accidental v1 edit fails here.
- `calibration/`: `run.py` (one calibration run over seeds 50000..50035),
  `confirm.py` (the single confirmation run with the predeclared reading),
  `freeze.py` (the freeze manifest) and `test_confirm.py`. All drive the
  reviewed `docs/invention-lab/analysis/family-robustness/calibrate.py`
  offline emulation. That's not the executor and not trial evidence.

## Calibration so far

Every run is kept under `calibration/runs/`, including repeats.

| run | settings | portfolio | random | parent `2483f08f` | five-wheel | portfolio vs random |
|---|---|---|---|---|---|---|
| 01 | initial ranges | 6 5 3 5 4 4 (27) | 3 1 2 2 1 2 (11) | 4 4 2 2 3 2 (17) | 2 1 0 2 0 0 (5) | 6 wins, 0 losses of 6 |
| 02 | same, repeated | identical | identical | identical | identical | identical |

Every panel was available for every arm. Run 02 reproduces run 01 exactly,
panel by panel, including experiment counts and steps used.

Reading of calibration (finite, six panels, not the confirmation): on these
ranges no fixed cart we have comes close to solving the family, the two
search controls separate, and only one portfolio panel hit 6/6. That's what
the proposal asked of the generator, so I'm not adjusting the ranges.
Tuning them further after seeing a 6-of-6 separation would only fit the
calibration seeds.

## Confirmation

Frozen at `e2ade73` (`confirmation/freeze.json`, sha256 `38f26861…`) and
announced on issue #1 before it ran. Run once, seeds 51000..51071, twelve
panels of six (`confirmation/result.json`). The frozen confirmation CLI requires
both the evaluator binary path (`argv[1]`) and the predeclared evaluator identity
SHA-256 (`argv[2]`):

```sh
python3 experiments/invention-lab/evaluator-mixed/calibration/confirm.py \
  /path/to/lab-eval-mixed \
  316908f707b071bbb0bc51cc8fb90e1d51393fe0a1b53528b76e0822f17c4ed9
```

The second argument is `evaluator.sha256` in `confirmation/freeze.json`, not
`evaluator.binary.sha256`. The script checks the binary's `identity` response
against it before calibration/scoring work. This documents the retained run's
invocation contract; it does not request a repeat or replacement of its evidence.
The frozen script and its historical usage docstring remain byte-identical.

Retained results:

| arm | per panel | total of 72 |
|---|---|---|
| portfolio_search | 3 5 4 5 6 5 4 3 5 6 6 6 | 58 |
| random_search | 1 1 2 2 1 1 2 1 2 2 1 2 | 18 |
| parent `2483f08f` | 1 2 4 3 4 1 3 1 3 4 5 4 | 35 |
| five-wheel | 2 1 0 2 0 1 1 3 0 2 1 1 | 14 |

Every panel was available for every arm. Portfolio beats random by margin
one on 12 of 12 panels with no losses, so the predeclared reading is
**discriminating**.

What that means, narrowly: on this family the two hand-written search
controls separate, in the offline emulation, under the v1 grant. It says
nothing yet about generated methods, the executor or any benefit. Two
limits worth knowing before anyone uses it: portfolio hits 6/6 on four of
the twelve panels, so a method better than portfolio has little room on
those; and random search is weak here (never above 2/6), so beating it is
a low bar. A useful comparison of developed methods on this family would
pin portfolio, not random search, as the control.

## Not covered

Executor or `lab-score` builds against this evaluator, generated methods,
hidden comparison, per-instance submissions, any v1 reading. See the
proposal's open question on executor builds.
