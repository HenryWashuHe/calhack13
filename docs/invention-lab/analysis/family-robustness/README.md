# What a single design does on each family

Every design any method has submitted in the retained records (13), plus the
evaluator's two reference designs, scored with the trusted evaluator on all
four families over 40 fresh public instances (seeds 20000..20039, used by no
record or protocol). Only `lab-eval accept` ran; no method or generated source
executed. Reproduce with `scan.py <lab-eval> 20000 40` against this tree; the
output is `results-20000-40.json`.

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
3. **A hand-written design is the strongest baseline on those two.** The
   evaluator's five-wheel reference (29/40, 30/40) beats every method
   submission except seeded random search's `56de341b` (28/40, 27/40). The
   fixed-cart control used so far is the weakest possible baseline there
   (0/40).
4. **Projectile-target: no design has ever succeeded.** All 15 score 0/40, and
   no method has been run on it. Process-comparison/1 includes it, so its
   panel will likely read 0 for every arm unless some method learns a
   different kind of design.
5. **Runs repeat designs.** The parent submitted `2483f08f` on four
   rough-transport panels. Seeded random search submitted `56de341b` on three
   ravine panels. Slots that share a design aren't independent samples of a
   method's skill.

## Suggestions (for discussion, nothing frozen is changed)

- Report the submitted design's identity next to every slot result, and the
  five-wheel reference alongside random search, as context. Process-comparison/1's
  frozen endpoints stay as they are.
- Before any run that uses projectile-target, check that the family is
  solvable at all, for example by some design reaching nonzero success on
  public seeds.
- Treat ravine-crossing and return-mix as one family whenever a claim depends
  on transfer *between* families. A revision that changes the parent's output
  style per family (its adapter) is a sharper G3 target than its search loop.

Limits: one evaluator version, 40 instances per family, designs taken as
submitted. This says what fixed designs do, not what any method would do with
a new grant.
