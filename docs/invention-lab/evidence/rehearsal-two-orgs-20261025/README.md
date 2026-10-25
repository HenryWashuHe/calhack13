# Rehearsal: two organizations in separate processes (October 25, 2026)

The third local rehearsal. Rehearsal 2 ran B in-process next to an in-memory
registry. Here the registry, A and B are three separate `genesis` processes,
each with its own SQLite file, tokens and an environment cleared except
`PATH`. It also adds the last step: the invocation's scored output is posted
to the run and attached to B's requirement (#211).

Predeclared in
[issue #1](https://github.com/HenryWashuHe/genesis-history-experiment/issues/1#issuecomment-6394224004)
at `0beefba` before anything ran: spec sha256 `1cfa35c2…`, invocation template
`91a7cd28…`, driver `crates/genesis-planner/tests/rehearse_two_orgs.rs`
(ignored by default). It ran once, as root, sandbox namespaces, pinned
Python 3.14.0, 3.9 s wall. Author-run, one host, loopback, scripted planner.
It closes no gate.

## Source

Branch `henry-claude/rehearse-two-orgs`: rehearsal 2's branch (#186/#179 and
#65) merged with #211 `d07b4c2` (unmerged). `genesis` built from that tree.
`lab-exec` `9a112687…`, `lab-score` `d384d1db…`, `lab-eval` `3adbc1fb…`, the
same binaries as rehearsal 2; no executor, evaluator or scorer source changed
in between (only an evaluator README and a scoring example).

## What ran

1. **Processes.** Registry with A's token as the only publisher (for
   `org_example_a`) and B's token as reader. A and B each started from one
   constitution file (`mission.coordinate`, `method.develop`), `--dev-stub`
   (provider bindings labeled `devstub-*`), own database and tokens.
2. **Publication.** A created its own mission. A published child `4f455dc6`
   with A's registry token. **Control:** B's reader token publishing as A got
   401 (`b-publish-as-a.json`).
3. **Selection and responsibility.** B searched with its own token (one
   candidate, `search.json`) and posted the intake. A `ReplayClient` planner
   (scripted reply, not a model) talked to B's process over HTTP with B's agent
   token; the kernel committed "Method specialist" plus a responsibility from a
   new requirement (`org-change.json`).
4. **Receiving trial** (`trial/`, only after that commit). Child vs the fixed
   cart, rough-transport, seeds 45000..45005, margin one: **Accepted, 6/6 vs
   2/6.**
5. **Receiver and restart.** B's operator wrote `state/b-receiver.json`,
   pinning that trial's plan to the new requirement, and B's process restarted
   with `--method-receiver`. A never got a receiver file.
6. **Adoption.** **Controls:** A's operator token on B's API got 401; B's lab
   lead, not the requirement's owner, got 409 `method_authority`
   (`refused-attempts.json`). The specialist adopted (`owner-decision.json`),
   with acceptance recomputed by the server from the retained records.
7. **Invocation and result.** One run under the specialist's grant on seeds
   45100..45105 via `invoke.py prepare/run/score`: **completed**,
   `output_generated`, 29,532 simulation steps, 785 ms, zero model use. The
   submitted design `7416ab4f…` was scored by `lab-score`: 6/6 successes,
   scoring budget `within_limits`. The server accepted the evaluation and
   recorded **attachment `attached`** to B's requirement, responsibility,
   specialist role and adoption (`run-record.json`, `score.stdout`).
   `lab-eval accept` on the same design and seeds also gives 6/6
   (`design-accept-45100.json`, informational).
8. **A's state at the end.** No requirements, one role, one responsibility, no
   trace of B's specialist; B's run id is 404 in A (`a-organization-at-end.json`,
   `a-run-lookup.json`).

## What this is not

- Not independent operators. One person's harness started all three
  processes on one host and knows every token. What's separate is process
  state, databases, receiver configuration and token checks.
- Not model judgment: the planner reply is scripted.
- Not a benefit claim. 6/6 on a fresh public range for one method run is a
  record that the attached output scores, not evidence that adopting the
  method helps B develop better designs than it would otherwise.
- Not live providers. `--dev-stub` bindings, loopback registry.
- No gate closes. #211 and #65 are unmerged.

## Files

`orgs/` holds the driver's records, process logs and the `invoke.py` run
directory. The SQLite files under `orgs/state/` are not in Git (`.gitignore`);
`constitution.json` and `b-receiver.json` are. `trial/` is `receive.py`'s
output. Exploration artifacts are left out (`artifacts/` is ignored); the
scorer artifacts for both trial slots and the invocation, the invocation's
submitted design `7416ab4f…` and the three process logs are force-added.
`SHA256SUMS` covers the committed files.
