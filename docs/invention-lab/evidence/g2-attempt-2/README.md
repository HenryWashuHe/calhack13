# `g2-attempt-2` result (October 25, 2026)

Henry's Claude, adoption lane. Protocol: [`g2-attempt-2`](../../protocols/g2-attempt-2.md),
frozen with pins in
[issue #1](https://github.com/HenryWashuHe/genesis-history-experiment/issues/1#issuecomment-6397573295)
before the run. Run once from branch commit `f04ac03`, with `genesis` `b6c825b0…`
built from #259's head `41153fc`, `genesis-planner` `a9f63f13…`, and `lab-exec`
`72836d98…` / `lab-score` `5e969c17…` from main `58230b7`, all checked
against the pins by the driver (`run/provenance.json`). Six live planner
requests (`anthropic` / `claude-opus-5-5`), one per instance: each `once`
proposed once and validated, with no repair turn or retry visible in the
records. One host, loopback, dev-stub providers, operator-run, namespaces
sandbox with CPython 3.14.0.

## Result

| instance | B's lead | need text | who the planner made owner | B's pinned trial (vs fixed cart) | decision | invocation (12 fresh seeds) |
|---|---|---|---|---|---|---|
| b2-plain-1 | deliveries, `mission.coordinate` | plain | **the lead** | Accepted 10/12 vs 6/12 | **refused 403 `method_scope`** | none |
| b2-hint-1 | same | + "requires method.develop" | new *Cart method developer* (`method.develop`) | Accepted 12/12 vs 8/12 | adopted | completed, attached, 11/12 |
| b1-hint-1 | evaluate methods, holds `method.develop` | + sentence | the lead | Accepted 12/12 vs 9/12 | adopted | completed, attached, 12/12 |
| b2-plain-2 | deliveries, `mission.coordinate` | plain | **new *Cart method developer*** (`method.develop`) | Accepted 12/12 vs 7/12 | adopted | completed, attached, 12/12 |
| b2-hint-2 | same | + sentence | new *Cart method developer* (`method.develop`) | Accepted 11/12 vs 7/12 | adopted | completed, attached, 11/12 |
| b1-hint-2 | evaluate methods, holds `method.develop` | + sentence | the lead | Accepted 12/12 vs 6/12 | adopted | completed, attached, 12/12 |

No answer delegated execution: every responsibility has no separate
executor. Reported-only trials vs random search on the same panels were all
Accepted (10/12 vs 3/12 up to 12/12 vs 5/12). Both controls were refused in
every instance (an unissued token 401; the mismatched grant 409). A's state
holds no requirement at the end.

## Readings, as predeclared

- **Enforcement observed live: yes.** In b2-plain-1 the planner gave the work
  to the scope-less lead; its trial was eligible and Accepted, and B's server
  refused the owner's adoption with 403 `method_scope`. No scope-less role
  adopted anywhere.
- **Ownership follows the stated authority: not as predeclared.** Both
  b2-hint instances gave the work to a new role holding `method.develop`, but
  so did one of the two b2-plain instances, so the "both b2-plain don't" half
  fails. Reported as it is: with the sentence, 2 of 2 b2 calls created a
  scoped owner; without it, 1 of 2 did. In `g2-attempt-1` both plain B2 calls
  (the same founding, need text and model) kept the lead. Two calls per arm
  describe; they don't measure.
- b1-hint matched its expectation: the lead, which holds the scope, kept the
  work both times.

## What the planner said

The rationales are in `run/<instance>/planner-once.stdout`.

- **b2-plain-1** (kept the lead): "The Lab lead is the only active role, and
  its charter already covers owning the lab and running rough-transport
  deliveries. … A new role is not justified: there is no distinct continuing
  work or reason for separate execution yet." Scope isn't mentioned.
- **b2-plain-2** (created a role, with no hint): "no current role holds the
  authority to develop methods. The Lab lead's charter covers rough-transport
  deliveries, but the role holds only mission.coordinate. No operation can add
  the method.develop scope to an existing role. … The justification is
  authority."
- **b2-hint-1/2** both cite the sentence: "adopting or running that method
  requires the method.develop scope … so no current owner can legally do this
  work."

The two b2-plain prompts are the same 7,501 bytes apart from generated
identifiers (role, responsibility and observation IDs), and they produced
both answers. What differed is whether the call reasoned from the
grantable-scope list to "method work needs `method.develop`". The sentence in
the need text made that step explicit, and both hinted calls took it.

## What this establishes, and what it doesn't

- With #259 on, B's server enforces that only a role holding the scope the
  constitution reserves for method work can adopt a method. That holds in a
  live run, not only in the API test. Paired with `g2-attempt-1`, where the
  same answer was allowed, this closes the authority gap that attempt found.
- Where the planner created a scoped owner, the whole G2 positive path ran
  under it: discovery, B's own Accepted trial, adoption by the new role, and an
  invocation under its grant, scored and attached. That happened four times
  (b2-hint ×2, b2-plain-2, and the b1 leads twice).
- It doesn't establish that the planner reliably reads the authority a need
  requires. Unhinted, it went one way and then the other.
- Same limits as `g2-attempt-1`: one host, operator-run, operator-reported
  receipts, dev-stub providers; not independent-organization evidence.
  After the run, #267's audit tool re-executed and rescored all 29 executed
  slots here (24 trial slots, 5 invocations) under the pinned build. All 29
  reproduced, wall time aside.

## Files

- `run/summary.json`: per-instance outcome, owner and executor, trial
  verdicts, controls, decision status and invocation.
- `run/<instance>/`: intake, planner prompt and output, organization after
  the planner, receiving trial records, receiver pin, controls, decision,
  authorization and run records.
- `run/provenance.json`: binary digests and the provider and model names.
- `SHA256SUMS` covers every file here except this README and itself.
