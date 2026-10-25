# What a single design does on each family

Every design any method has submitted in the retained records (13), plus the
evaluator's two reference designs, scored with the trusted evaluator on all
four families over 40 fresh public instances (seeds 20000..20039, used by no
record or protocol). Only the evaluator (`lab-eval`) and, in `calibrate.py`, the two hand-written
controls ran; no generated source executed. Reproduce with `scan.py <lab-eval> 20000 40` against this tree; the
output is `results-20000-40.json`. `launcher_search.py <lab-eval> 7 20000 20001 20002 20003 20004 20005`
reproduces `launchers.json`, and `panel_cap.py <lab-eval> 1 4000 30000 30006 30012` reproduces
`panel-cap.json`. `calibration.json` comes from `calibrate.py` on seeds 30000..30017.

Successes out of 40 (selected rows; all 15 are in the JSON):

| Design | Submitted by (family developed on) | ravine | rough | return-mix | projectile |
| --- | --- | ---: | ---: | ---: | ---: |
| `2483f08f` | A's parent (rough-transport, every time) | 3 | **37** | 5 | 0 |
| `cd7ce079` | A's parent (ravine, hidden runs) | 8 | 35 | 7 | 0 |
| `05eaa4c6` | A's parent (ravine, public) | 6 | 33 | 9 | 0 |
| `adede12c` | A's parent (return-mix) | 3 | 35 | 4 | 0 |
| `6b4dcd55` | B's revision (rough-transport) | 1 | 31 | 1 | 0 |
| `56de341b` | seeded random search (ravine, all 3 runs) | 28 | 3 | 27 | 0 |
| `244fa07e` | evaluator reference, five-wheel | **29** | 5 | **30** | 0 |
| `7a03fd84` | fixed cart control | 0 | 22 | 0 | 0 |

## Readings

1. **The parent builds the same kind of cart on every family.** All four of its
   designs score 33–37 on rough-transport and at most 9 on ravine or
   return-mix, whatever family it developed on. Its "selective transfer" to
   rough-transport (#165) is this: its output doesn't depend on the family,
   and the cart it makes happens to suit rough-transport. B's revision has
   the same profile.
2. **Ravine-crossing and return-mix are one family for transfer purposes.**
   Across all 15 designs their success counts correlate at r = 0.994, and both
   are negatively correlated with rough-transport (−0.68, −0.64). A result on
   one is close to a replicate of the other, not an independent family. This
   includes A's own trial (#117, return-mix) and B's receiving trial on A's
   family (ravine).
3. **A hand-written design beats almost everything on those two.** The
   evaluator's five-wheel reference (29/40, 30/40) beats every method
   submission except seeded random search's `56de341b` (28/40, 27/40). The
   evaluator README calls it a sanity design, not a baseline; it is still a
   useful yardstick. The fixed-cart control used so far scores 0/40 there.
4. **Projectile-target is solvable, but capped near 2 of 6 per panel.** All
   15 retained designs score 0/40, because none is a launcher. A random search
   over single-arm launchers (`launcher_search.py`, `launchers.json`) finds
   hits on each of five instances: 1–19 hits per 3,000–10,000 valid designs.
   Each hit solves only 2–8 of 40 instances. A trial scores *one* submitted
   design on every panel instance, and each instance's target sits somewhere
   in 4–9 m. So the best launcher among 4,000 per panel fits only **2 of 6**,
   and 99.7% fit none (`panel_cap.py`, `panel-cap.json`). Under that scoring
   the family can't show per-instance adaptation. It rewards finding the
   panel's best compromise launcher, a rare event, and caps counts low.
5. **Runs repeat designs.** The parent submitted `2483f08f` on four
   rough-transport panels. Seeded random search submitted `56de341b` on three
   ravine panels. Slots that share a design aren't independent samples of a
   method's skill.
6. **No random control competes on projectile-target within the grant.**
   The seeded random search used in every trial samples only carts. A probe
   that samples carts and launchers equally (`mixed_search.py`, analysis only)
   does no better. Under the 72,000-step grant both read **0/18** on three
   projectile panels, while matching each other on the other families
   (11–14/18; `calibrate.py`, `calibration.json`). That uses an offline
   emulation of the executor protocol, not the executor, and isn't trial
   evidence. Any method that lands even one target would beat these controls
   by the margin-one rule. That would be a weak win.

## Suggestions (for discussion, nothing frozen is changed)

- Report the submitted design's identity next to every slot result.
  Process-comparison/1's frozen endpoints stay as they are.
- Treat ravine-crossing and return-mix as one family whenever a claim depends
  on transfer *between* families.
- Read projectile-target results with the cap in mind. Expect both arms near
  0, and treat a 1–0 margin there as weak. Showing per-instance adaptation
  would need per-instance submissions, which the current trial model doesn't
  have.
- A G3 target that follows from reading 1: a revision whose adapter changes
  the *kind* of design per family. The parent makes carts everywhere, so it
  can't move on projectile-target or improve much on ravine.

Limits: one evaluator version, 40 instances per family, designs taken as
submitted, and one launcher shape in the search; hit rates are for that shape
only. This says what fixed designs do, not what any method would do with
a new grant.
