# Strategy / adapter split (exploratory)

A hand split of the first model-generated method, `8215c367…`
(`docs/invention-lab/evidence/live-method-20261024-generated/method-package.json`),
into a domain-free search strategy and a domain adapter. It is not generated
and not an improvement: it must behave exactly like the parent. It exists to
test whether a method can be separated into a part that could transfer between
task families (strategy) and a part that cannot (adapter).

| File | Holds | Must not hold |
| --- | --- | --- |
| `parent-split/strategy.py` | `order`, `propose`, `allocate`, `stop`, and `search` composing them | domain nouns, protocol I/O |
| `parent-split/adapter.py` | line protocol, instance features, candidate space, design builder | search loops |
| `parent-split/method.py` | glue, entry point | logic |

The three files fit the existing `development-method-source/1` package (up to
256 files, sorted by path; `docs/invention-lab/contracts/packages.md`). The
executor copies every file into the run directory and starts `method.py`
there, so no forge, executor or core change is needed. The strategy and adapter
identities are the SHA-256 of their file bytes, derived from the package.

## Acceptance

`python3 -m unittest discover -s experiments/strategy-adapter/harness`
(standard library only, no simulator, no model call):

- the six `rough-transport/1/{0..5}` worlds rebuilt by `harness/labworld.py`
  match the digests in `…/b-revision-receiver/vs-parent/plan.json`;
- the harness reproduces both retained runs in that plan from their artifacts:
  the parent (6 experiments, 5,205 steps) and B's child (32, 39,778);
- **the split reproduces the parent's retained run request for request:**
  same instances, same normalized design `2483f08f…` each time, 6 of 6
  successes, 5,205 steps, and the same submitted design `2483f08f…`, whose
  retained score is 6/6 (`vs-parent/verdict.json`);
- a deliberately reordered strategy is detected;
- parent and split send byte-identical requests on 200 synthetic oracle cases
  (`replay.py diff`; synthetic outcomes, never scores);
- static checks for the two "must not hold" columns above.

A live run through `lab-exec` / `lab-score` is the remaining direct check.

## One finding the split made visible

The parent's "screen on the hardest instance" orders by `gap + step_height`,
which are ravine fields. On rough-transport both are 0.0 for every instance,
so the order is a no-op: the parent screened on instance 0 because it was
first, and its 6/6 came from its first admissible grid point passing (radius
0.6, speed -6, length 3.0, wood; the rubber variant costs 5.12, over the
smallest budget, 4.319). The screen-then-confirm loop worked; the adapter's
difficulty feature was blind on this family.
