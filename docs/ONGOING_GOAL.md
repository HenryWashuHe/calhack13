# GENESIS ongoing goal

**Networked recursive capability improvement with fixed model weights.**

Status: ongoing work resumed by the user on October 23, 2026 (Pacific time).
The existing hourly continuation is active. This is the durable repository
objective and completion contract, read alongside current [ownership](AGENT_HANDOFF.md#current-ownership-and-next-work)
and [issue #4](https://github.com/HenryWashuHe/genesis/issues/4).

The older app Goal is still an unfinished paused record at this checkpoint.
Its tool cannot replace it until the user clears it; do not falsely complete it.
That app-state limitation does not revoke the user's new authorization or the
active recurring task. If the user clears that record, create the revised app Goal from the objective below under their existing request, with no invented token budget. Record the replacement accurately when it occurs.

## Objective

Implement and independently evaluate a bounded network in which organization A
develops an executable capability-development method, organization B selectively
adopts and improves it, and A independently accepts and executes the frozen revision while developing a
new capability on a fresh task in at least one recorded transfer chain. Keep foundation model weights fixed, preserve
each organization's authority and acceptance decisions, and measure the effect
against declared controls at comparable total resource budgets.

The [invention-lab simulation](INVENTION_LAB_DEMO.md) is the current proposed
demonstration. It supplies a visible task world, not evidence that the mechanism
already exists. The product remains infrastructure for organizations to retain,
exchange and improve executable capabilities and their development methods.
Event organization is not the product, demo requirement or completion gate.

## Completion gates and evidence

| Gate | Required result | Evidence before marking complete | Current state |
| --- | --- | --- | --- |
| G0: shared contracts | A versioned method interface, exact artifact/dependency identity, evaluator/task contract, resource accounting and separate execution/evaluation/adoption records | Reviewed contracts and compatibility/unknown-outcome cases agreed by affected owners; selected runtime/evaluator documented | Open: #124, #126; adoption proposal #129 is input |
| G1: executable development | A develops or improves an executable method through the governed process and uses it to develop a capability/design | Generated/changed method source and derivation, runnable executor, independently scored outputs, enforced limits, receipts and meaningful failure/restart tests; a supplied seed alone is a control | Open; existing extraction Actors are a foundation, not this general executor |
| G2: independent inheritance | B discovers, locally checks and uses A's exact method on a distinct task, with justified organizational responsibility | Separate state/grants, exact invocation/acceptance evidence, a justified role change and an existing-owner control, appropriate non-reuse with scored task outcome | Open for methods; task-capability inheritance has separate existing fixture evidence |
| G3: recursive return | B develops an executable revision; in at least one chain A independently accepts and executes that frozen revision to develop a later capability on a fresh return task | Genuine method changes/derivation, pinned versions, local acceptance and execution evidence closing A → B → A; declines remain valid controls but do not close this gate; positive improvement is not required | Open |
| G4: controlled evaluation | Reproducible comparison of no accumulation, task-capability accumulation and development-method accumulation, plus a fair local-history control for network benefit | Fixed model/settings and trusted evaluators, isolated state/holdouts, predeclared criteria, repeated task orders, costs/latency/interventions/failures and uncertainty reported | Open; no positive recursive-benefit result exists |
| Delivery | Reviewable, runnable work and an honest demonstration | Focused merged PRs, reproducible commands, source revisions, updated implementation inventory and explicitly labeled fixture/native/cloud evidence | Open |

Completion means the mechanism and evaluation gates are fulfilled and their
results are reported honestly. It does not require manufacturing a positive
result. A **positive recursive-improvement claim** stays open unless its
predeclared quality/cost criteria pass. Missing cloud credentials are a separate
blocker for live sponsor claims; they do not justify stopping independent core
work or presenting fixtures as live. Simulation does not prove real engineering
reliability or universal transfer.

## Working cycle

At the start of every resumed cycle, and again before a merge or new assignment:

1. Read this goal, the current handoff, Git status including uncommitted work,
   remote main, exact open PR heads/reviews, and the latest issue #4/task comments.
   Prefer current evidence over old ownership or status snapshots.
2. Select a concrete next step that advances a gate or removes its dependency.
   Record the gate, deliverable, owner, files/interfaces, dependency and validation
   before expanding shared scope. Work on implementation as well as review.
3. Check whether another session already owns or has completed it. Give available
   collaborators bounded non-overlapping work; preserve active claims. Distinguish
   assigned, acknowledged, active, blocked and done. Ines remains unassigned until
   the user changes that instruction.
4. Implement or review the scoped change, run appropriate checks, preserve
   independent regressions, and commit/push working increments. Open focused PRs
   and merge ready exact heads under the user's existing authorization. Do not
   infer readiness from author-reported tests alone.
5. Update the evidence ledger and handoff with source/head, checks, result,
   limitations and next step. Check that the work advanced its chosen gate.
   If it did not, stop expanding that branch and select a better next step.

Use the existing hourly heartbeat for continuation when idle; during active work,
perform this check at milestone boundaries. Refresh assignments when ownership,
progress or blockers change, not on every timer tick. Avoid duplicate tasks and
repetitive no-change comments. Notify for completed milestones, significant
failures or input that is actually required.

## Constraints that prevent drift

- Preserve accumulation, reuse/composition and improved future development as
  distinct claims. More tools, more roles, or shorter traces do not prove progress.
- A receiving organization can reject a capability. Unknown discovery/execution
  is not an empty result or permission to repeat an external effect.
- Candidates cannot change their trusted evaluator, hidden cases, resource
  ceilings or kernel authority. Exact dependencies and installations stay pinned.
- Extend existing organization/registry foundations deliberately. Do not add a
  parallel controller, broad framework rewrite, unrelated infrastructure, or a
  new product story merely because it is easier to implement.
- Preserve colleague files and local repair WIP. Deployed repair is a supporting
  milestone to resume only with a relevant scoped claim and its unfinished
  validation; it must not displace the method-development path by default.
- Keep implemented, unmerged, proposed and measured claims visible separately.
  Evidence categories are loopback, native local process, authenticated cloud,
  and independent comparison. None implies the others.
- If one path is externally blocked, continue an independent authorized gate.
  If every meaningful path requires user input, state the exact blocker and
  preserve state. Stop recurring work on user pause/cancellation or completion.

## Current milestone ledger

| Milestone | Owner and evidence | Next action |
| --- | --- | --- |
| Canonical framing and status separation | #127 merged at `844f37f`; documentation review, 193 local link/anchor checks | Maintain claim/evidence distinctions as code lands |
| Method and comparison contract, G0 | Codex #124; runtime not implemented | Draft the contract pack using #129's requirements and review corrections: separate invocation/acceptance/comparison purposes, durable acceptance versus job authority, and merged versus pending fields; obtain affected-owner feedback before wiring |
| Selective adoption foundation | Core Claude #122 acknowledged; #116 correction `78a18c0`, #128 classifier `546fb65` stacked on #116, #129 adoption proposal `d8fa531` | Review #116 before #128 and their interaction with #121; keep implementation with its owner |
| Constitution audit and evidence UI | Eric #123; #120 `0cf383b7` open; new UI assignment not yet acknowledged | Review audit; request bounded UI contract feedback without taking Eric's files |
| ACP correctness | ACP session #125 acknowledged; planned `claude/acp-regressions` not yet pushed; prior seven regressions preserved | Confirm current head, correct/test generic defects, then ACP contract; review #130 as a proposal, not authority to change product scope |
| Simulator/evaluator, G0/G1 | #126 unassigned; Ines unassigned | Record a named bounded owner before implementation; do not invent one from a collaborator handle |
| Deployed repair foundation | Pushed forge `1795fb7`, local server/test WIP; happy path only | Preserve until a separately scoped supporting milestone; full revised 12-case target/final checks remain open |

This ledger is a checkpoint, not a substitute for current GitHub state. Record
subsequent material milestones here with exact evidence; retain historical
details in the handoff instead of copying an ever-growing transcript.
