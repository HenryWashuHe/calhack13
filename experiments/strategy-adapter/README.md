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

Two commands, because only one of them executes method source.

**Static and controller checks** run by default and start no retained or
generated method:

```sh
python3 -m unittest discover -s experiments/strategy-adapter/harness
```

- the six `rough-transport/1/{0..5}` worlds rebuilt by `harness/labworld.py`
  match the digests in `…/b-revision-receiver/vs-parent/plan.json`;
- rough-transport sets neither ravine feature (`gap`, `step_height`);
- static checks for the two "must not hold" columns above (lexical
  heuristics, not proofs);
- `test_replay.py`: the replay controller's deadline, bounds and process-group
  cleanup, exercised only with small hand-written fixture methods.

**Generated-source acceptance** executes the retained model-generated parent,
B's generated child and the split. It is never discovered automatically and
must name its execution boundary:

```sh
python3 experiments/strategy-adapter/harness/acceptance_split.py --isolation namespaces
```

`namespaces` is the boundary lab-exec uses for its `namespaces` runs. The
setup is copied from `executor/src/sandbox.rs` and needs root. The method runs
as `nobody` with no network, read-only mounts where the kernel allows them, and
private `/home`, `/root`, `/tmp` and `/run`. The whole PID namespace ends with
each run. It does not stop reads of the rest of the read-only system, syscalls
or filesystem Unix sockets, and it does not show that no model was used.
`--isolation host` runs the generated source directly on this machine with no
isolation. Use it only after reading that source; it is not namespace
assurance. Every run is bounded by one deadline and caps on line size, request
count and output, and its process group is killed on every exit path.

The acceptance assertions:

- the harness reproduces both retained runs in that plan from their artifacts:
  the parent (6 experiments, 5,205 steps) and B's child (32, 39,778);
- **the split reproduces the parent's retained run request for request:**
  same instances, same normalized design `2483f08f…` each time, 6 of 6
  successes, 5,205 steps, and the same submitted design `2483f08f…`, whose
  retained score is 6/6 (`vs-parent/verdict.json`);
- a deliberately reordered strategy is detected;
- parent and split send byte-identical requests on 200 synthetic oracle cases
  (`replay.py diff`; synthetic outcomes, never scores).

## Live check

The same acceptance through the real executor and scorer, from
`experiments/invention-lab/executor` after `cargo build`:

```sh
B=target/debug/lab-exec; O=/tmp/split
$B package ../../strategy-adapter/parent-split parent-split 0.1.0 method.py $O.package.json
$B example-plan namespaces 6 72000 60000 $O rough-transport $O.package.json 0
$B run $O/plan.json candidate $O.package.json $O/candidate namespaces
(cd ../scoring && cargo run -q --bin lab-score -- score $O/plan.json candidate $O/candidate $O/candidate-score)
$B check $O/plan.json candidate $O/candidate $O/candidate-score
```

lab-exec's `none` mode is host execution with no isolation at all. It is not a
substitute for `namespaces`, and a `none` run must be reported as host
execution. Expected: 6 experiments, 5,205 steps, output `2483f08f…`, 6 of 6, and
the same instance, design, step count and result artifact for each experiment as
`…/vs-parent/control/transcript.json`. A run of this is an author-run local
process, not retained evidence. Its model calls and tokens are recorded as
unknown in every mode.

## One finding the split made visible

The parent's "screen on the hardest instance" orders by `gap + step_height`,
which are ravine fields. On rough-transport both are 0.0 for every instance,
so the order is a no-op: the parent screened on instance 0 because it was
first, and its 6/6 came from its first admissible grid point passing (radius
0.6, speed -6, length 3.0, wood; the rubber variant costs 5.12, over the
smallest budget, 4.319). The screen-then-confirm loop worked; the adapter's
difficulty feature was blind on this family.
