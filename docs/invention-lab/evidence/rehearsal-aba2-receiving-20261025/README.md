# Rehearsal 5: the receiving half of `aba-attempt-2` (October 25, 2026)

Stages 2–6 of [`aba-attempt-2`](../../protocols/aba-attempt-2.md) with real
processes, so the harness has run before the real attempt freezes.
Predeclared in
[issue #1](https://github.com/HenryWashuHe/genesis-history-experiment/issues/1#issuecomment-6394948453)
at `0b8eba5` before it ran. Ran once, as root, sandbox namespaces, pinned
Python 3.14.0, 11 s. No generation: the child `4f455dc6` stands in for
`child2`. The planner is a scripted `ReplayClient`. Author-run, one host,
loopback, dev-stub providers. It closes no gate.

## Source

Branch `henry-claude/rehearse-aba2` off main `85073cc` (#65, #57 and #211
merged). Driver `crates/genesis-planner/tests/rehearse_aba2.rs`. Lab binaries
built from that main: `lab-exec` `72836d98…`, `lab-score` `5e969c17…`,
`lab-eval` `3adbc1fb…` (evaluator digest `1f03d1b6…` unchanged).

## What happened

| stage | result |
|---|---|
| 2. B receiving, rough-transport 48000..48011, vs pinned fixed cart | **Accepted, 11/12 vs 10/12** (reported only: vs random search 11/12 vs 4/12, Accepted) |
| B publishes | yes, under `org_example_b` |
| control: A's registry token publishing "as B" | client error (502 `registry_invalid_response`), **but see the finding below** |
| 3. A search (A's token) and intake; scripted planner | requirement assigned to A's **existing Designer role**; still one role, no specialist |
| 4. A receiving, ravine 48100..48111, vs A's own method (parent `8215c367`) | **Accepted, 3/12 vs 2/12** (reported only: vs random search **3/12 vs 6/12, Rejected**) |
| A receiver pins that plan; A restarts with it | done |
| 5. control: B's operator token on A | 401 |
| Designer decides | **adopted** (server recomputed acceptance 3 vs 2 of 12) |
| 6. one invocation, ravine 48200..48211 | completed, `output_generated`, 22,500 steps, within limits; scored **2/12**, result **attached** to A's requirement, responsibility, Designer role and adoption |
| B's state at the end | no requirement, nothing of A's |

I predicted the decline path at stage 5 (on #117 the child lost to the parent
on ravine). It went the other way on these seeds, by one instance.

## Findings worth acting on

1. **The existing-owner control can admit a method that random search
   beats.** A's pinned comparison is the stand-in against A's own current
   method. Both are weak on ravine (3/12 and 2/12), so margin one adopted a
   method that the reported-only random-search control beat 6/12 to 3/12 on
   the same panel. `aba-attempt-2` publishes the random comparison beside the
   pinned one but doesn't let it block adoption. Before freezing, it's worth
   deciding whether A's receiver should also require not losing to random
   search, or at least whether a G3-relevant reading should require it.
2. **The registry ignores the requested publisher instead of refusing.** A's
   token asked to publish B's exact package "as `org_example_b`". The registry
   didn't reject it: it stored a second row for the same method and version
   under **`org_example_a`** (the token's organization), and the typed client
   then reported an inconsistent response. So nobody can publish as someone
   else, but any publisher can republish another organization's exact bytes
   under its own name, and the caller sees an error after a successful write.
   That's Codex's registry; it may be intended (content-addressed identity),
   but the provenance question deserves an explicit decision.
3. The stand-in's invocation output scored 2/12 on fresh ravine seeds,
   consistent with the acceptance numbers: this harness works, the method
   isn't good on ravine.

## What this is not

Not the real attempt (no `child2`, no generation), not model judgment (scripted
planner), not independent operators, not a benefit claim. Seeds 48000..48211
are now used.

## Files

`run/` holds the driver's records, process logs and the invocation run.
`b-trial/` and `a-trial/` are `receive.py`'s output. SQLite files are not in
Git; scorer artifacts for the pinned trials and the invocation, and the
invocation's submitted design, are force-added. `SHA256SUMS` covers the
committed files.
