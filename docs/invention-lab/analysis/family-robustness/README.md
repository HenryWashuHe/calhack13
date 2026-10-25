# Fixed-design and hand-written-control observations

This is an exploratory analysis of author-run evaluator results. It retains
15 designs (13 distinct method submissions and two evaluator references) scored
on four families over 40 public instances, seeds 20000..20039. Those seeds were
outside the retained trials and frozen process-comparison protocol. Later
sections examine sampled launchers and hand-written search controls. These
results do not execute the frozen comparison or establish recursive improvement.

A separate static review checked the JSON arithmetic, design digests and
committed probe bytes. It did not rerun the evaluator or the controls. The
reported execution history below is the author's account; these aggregate
files are not bound execution receipts or organization-local acceptance records.

The author used `scan.py <lab-eval> 20000 40` for
`results-20000-40.json`, and these commands for the launcher samples:

```sh
launcher_search.py <lab-eval> 7 20000 20001 20002 20003 20004 20005
panel_cap.py <lab-eval> 1 4000 30000 30006 30012
```

`calibration.json` reports `calibrate.py` runs on seeds 30000..30017;
`headroom.py <lab-eval>` produces the headroom comparison. These are standalone
research scripts, not the bounded executor. `calibrate.py` starts a supplied
Python control directly, without an OS sandbox. Only trusted, hand-written
controls were reported used here; no generated method source was reported run
for this analysis. The harness's failure accounting and process-lifecycle
fixes are described below. The commands are not an instruction to execute
untrusted source or overwrite the retained results.

Successes out of 40 (selected rows; all 15 are in the JSON):

| Design | Submitted by (family developed on) | ravine | rough | return-mix | projectile |
| --- | --- | ---: | ---: | ---: | ---: |
| `2483f08f` | A's parent (rough-transport) | 3 | **37** | 5 | 0 |
| `cd7ce079` | A's parent (ravine, hidden runs) | 8 | 35 | 7 | 0 |
| `05eaa4c6` | A's parent (ravine, public) | 6 | 33 | 9 | 0 |
| `9a7ca944` | A's parent (ravine) | 7 | 32 | 7 | 0 |
| `adede12c` | A's parent (return-mix) | 3 | 35 | 4 | 0 |
| `6b4dcd55` | B's revision (rough-transport) | 1 | 31 | 1 | 0 |
| `56de341b` | seeded random search (ravine, all 3 runs) | 28 | 3 | 27 | 0 |
| `244fa07e` | evaluator reference, five-wheel | **29** | 5 | **30** | 0 |
| `7a03fd84` | fixed cart control | 0 | 22 | 0 | 0 |

## Observations from these samples

1. **The parent's five retained designs have a similar performance profile.**
   They score 32–37 on rough-transport and at most 9 on ravine or return-mix,
   including designs developed on those other families. This suggests a
   cart-type bias in the retained outputs. It does not mean the outputs or
   search paths are identical across families. The repeated rough-transport
   output in #165 is evidence of selective transfer in that experiment, not
   a causal explanation of why every run selected it. B's shown revision has
   a similar rough-favoring profile.
2. **Ravine and return-mix outcomes overlap strongly for these 15 designs.**
   Their success counts correlate at r = 0.994, while both correlate
   negatively with rough-transport (−0.68 and −0.64). This is a possible
   coverage confound for interpreting cross-family transfer. It does not
   establish that the task families are equivalent or that every future
   method would have correlated results. The four frozen families and their
   separate endpoints remain unchanged.
3. **The five-wheel reference beats every submitted design on these two
   panels.** Its 29/40 ravine and 30/40 return-mix exceed all method
   submissions, including the closest, random search's `56de341b` at 28/40
   and 27/40. The evaluator calls it a sanity design, not a baseline. The
   fixed-cart control used in the retained trials scores 0/40 there.
4. **The sampled launchers succeed rarely; the search does not establish a
   ceiling.** All 15 retained designs score 0/40 on projectile-target. The
   separate single-arm search tried 20,000 designs per instance and found
   1–19 hits among 3,372–10,478 valid designs on each of five instances.
   The first hit selected for each instance scores 2–8/40 on the public panel
   (`launchers.json`); these are not results for every successful launcher.
   In another sample of 4,000 launchers on each of three six-instance panels,
   the best sampled count is 2/6 and 11,972/12,000 evaluations (99.77%) score
   zero (`panel-cap.json`). A finite random sample is not an upper bound,
   even for that launcher shape. A trial currently scores one submitted
   design across its panel. That output contract does not express separate
   per-instance submissions, but it does not prove a universal design cannot
   solve more instances.
5. **Several runs repeat a final design.** The parent submitted `2483f08f`
   on four rough-transport panels; random search submitted `56de341b` on
   three ravine panels. Count those shared outputs explicitly when reasoning
   about method diversity. Repeated output alone does not establish identical
   search histories or eliminate uncertainty across task panels.
6. **Two tested controls score zero on the sampled projectile panels.**
   The existing random-search control samples carts. The hand-written
   `mixed_search.py` samples carts and launchers equally. Both report 0/18
   under a 72,000-step grant on three projectile panels; on the other three
   families their totals are 11–14/18 (`calibration.json`). This is an offline
   emulation of the executor protocol, not a retained trial. It does not
   establish that every random control fails. A margin-one win over a zero
   control on such a panel would meet that count rule, but would leave the
   breadth and strength of the improvement to further evaluation.

## A hand-written probe that selects among cart types

`portfolio_search.py` screens six starting carts on two instances (two large
wooden wheels, five small rubber wheels, and four random carts from a wider
range than `random-search`), tests the best two on the other four instances,
then mutates the leader. It does not read the family name. This is a
hand-written research probe, not B's generated revision.

The author reports tuning only on seeds 30000..30017. Commit `7ec9a24`
predeclares six panels per family on seeds 31000..31035, a 72,000-step grant,
random search and two fixed references, with no further probe changes.
`portfolio_search.py` and `calibrate.py` bytes are unchanged from that commit;
`headroom.py` and the result JSON were added in `be9e3bc`. This records a
versioned declaration and unchanged probe bytes, not independent proof of
execution timing. The author-run comparison uses `calibrate.py`'s emulation;
the fixed designs use `lab-eval accept`. Neither is the executor or an
organization-local adoption decision.

| Successes /36 (six panels) | ravine | rough | return-mix | total /108 |
| --- | ---: | ---: | ---: | ---: |
| `portfolio_search` | 20 | **36** | 26 | **82** |
| `random_search` (the trial control) | 19 | 19 | **27** | 65 |
| parent's fixed cart `2483f08f` | 2 | 31 | 5 | 38 |
| five-wheel reference | **21** | 7 | 26 | 54 |

7. **The probe's largest observed advantage is on rough-transport.** It
   reports 36/36 there versus random search's 19/36. On ravine and return-mix
   it is within one success of random search and the five-wheel reference.
   Those totals do not imply a tie on every panel or equivalence of methods.
8. **The sampled ravine counts leave limited separation among these
   controls.** The largest total in this comparison is the reference's
   21/36. That is the best observed result in this table, not a limit for
   carts or future development methods.
9. **Panel-wise margins depend on the family and control.** With a
   margin-one rule, the probe beats random search on 6/6 rough panels,
   1/6 ravine panels and 1/6 return-mix panels. Random search wins on 2/6
   return-mix panels. The remaining panels tie (five ravine and three
   return-mix). These observations cannot determine the probability of an
   accepted A return or show that A's tasks cannot demonstrate improvement.

A generated revision that experiments with more than one cart type is a
concrete hypothesis for the next development-method trial. The hand-written
probe's outcome does not predict that revision's result: B must actually
develop the revision, retain its evidence, and return it for a fresh,
predeclared comparison and A's local decision. G3/G4 remain open.

## Suggestions for future experiments

- Report each submitted design's full identity next to its slot result.
  Keep process-comparison/1's frozen endpoints and all unsuccessful outcomes.
- Report the observed ravine/return correlation as a coverage limitation,
  while retaining the distinct families and results. Test broader transfer
  in a separately declared protocol rather than collapsing frozen endpoints.
- Distinguish sample hit rates from performance ceilings. A prospective
  per-instance output protocol could test adaptation directly, but would
  change the current output contract and require its own declaration.
- Treat design-class selection as a revision hypothesis. Its value must be
  demonstrated through generated development, selective organization-local
  reuse and a fresh accepted return, not inferred from this hand-written probe.

## Limits and reproduction status

The JSON covers one evaluator version, fixed public seed ranges, selected
retained outputs and particular hand-written controls/search distributions.
It does not bound what another method or design could achieve with a new grant.
The aggregate results lack immutable execution bindings and full evaluator
failure records; they cannot establish measured model usage, adoption,
execution authority, a controlled G4 gate or recursive benefit.

Static review found that `calibrate.py` could report a failed or missing
experiment receipt as an `ok` response with zero charged steps, that it and
`headroom.py` summed missing acceptance results as zero successes, that
`panel_cap.py` left failed evaluator calls out of its histogram, and that the
harness had no deadline on its protocol reads and no guaranteed child
cleanup. Those are fixed (PR after #184). An evaluator call that fails, times
out or prints no valid count makes a calibration panel *unavailable*, with the
reason, and its successes `null`. Any unavailable panel makes the total
`null`. `panel_cap.py` refuses to print a partial histogram. Each evaluator
call has a 120 s limit and each panel a 900 s deadline, and the control
process is always stopped and reaped. `test_harness.py` covers these with a
fake evaluator. Re-running `headroom.py` and every `calibrate.py` entry after
the fix reproduced the checked-in `headroom.json` and `calibration.json`
byte for byte (the retained runs had no failures). `panel-cap.json` was not
regenerated. These are still author-run analysis scripts for hand-written
controls, not the executor or a sandbox.

GENESIS's goal remains networked recursive capability improvement with fixed
model weights. This analysis informs that work; it is not a substitute for
networked execution, selective adoption or the full declared evaluation.
