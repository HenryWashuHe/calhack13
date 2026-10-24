# Invention lab: adopting an executable development method

**Status: contract proposal (Henry's core Claude, lane genesis#122). Not
implemented.** It specifies how a receiving organization decides whether to
install and use an exact version of an executable development method. The
method identity, execution and scoring receipts are Codex's experimental
contracts (#18, #22; lane genesis#124, under `docs/invention-lab/contracts/**`
on those branches); the evaluator and simulator are genesis#126, assigned to
Henry's core Claude in this repository's issue #1 (contract #13). Where this document needs something from those
contracts it says so under [Required from the method contract](#required-from-the-method-contract)
instead of inventing their fields.

Throughout, three statuses are kept apart: **on main** (merged), **open PR**
(pushed with tests, not merged, so not yet main's wire contract) and
**proposed** (no wire value yet). Nothing here is a claim that a method has been adopted, executed or shown to
improve anything.

## What exists today, and what this extends

Adoption today handles one artifact kind: a task capability (an Apify Actor build)
described by a `CapabilityManifest`. Pieces this contract reuses:

| Piece | Where | Status | Wire values |
| --- | --- | --- | --- |
| Bounded discovery, own-publication and scope filtering, exact manifest match | `capability_adoption.rs` | on main | adoption attention codes `registry_unavailable`, `no_candidate`, `invalid_candidate`, ... |
| Separate execution purposes | `capability_runtime.rs` `ExecutionPurpose` | on main | `invocation`, `local_acceptance`, `adoption_acceptance` |
| Local acceptance by the receiver's own trusted suite, fresh inputs | `capability_adoption.rs`, `genesis-verification` | on main | installation `checking` → `accepted` / `rejected` |
| Committed acceptance is organization-owned evidence; current authority gates live jobs and the initial decision | `capability_authority.rs` | on main | n/a |
| Pinned installations; publication never changes one | kernel `begin_installation`, registry | on main | one `checking`/`accepted` installation per exact `capability_id@version` |
| Unknown submission never becomes empty or a retry | #86 | on main | `needs_reconciliation`, retry gate `adoption_checks_unresolved` |
| Per-candidate decision log, committed with the outcome it explains | legacy #116 → #8, `capability_adoption_candidates.rs` | on main | `selected`, `adopted`, `not_applicable`, `own_publication`, `missing_scope`, `invalid_manifest`, `unusable_state`, `manifest_unavailable`, `invalid_response`, `installation_conflict`, `not_evaluated` |
| Conclusion of an adopt-first gap | legacy #128 → #11, `capability_adoption_outcome.rs` | open PR | `pending`, `adopted`, `not_applicable`, `insufficient_evidence`, `no_candidate`, `unknown`, `stopped` |
| Confirmed terminal provider failure distinct from transport uncertainty | legacy #121 → #10 | on main | `ProviderError::RunFailed` |

Values from open PRs (#11) are pending dependencies of this proposal, not
main's current wire contract. #18 (execution) and #22 (scoring) are merged
experimental contracts; the pre-execution trial plan is draft #28 and the
experiment-call ledger draft #32.

The method case keeps all of these rules. It adds an artifact kind, an
environment check and a different acceptance experiment. It does not add a
registry, a second adoption coordinator or a second decision vocabulary where
the existing one fits.

## The rules

1. **Exact identity only.** The receiver adopts `method_id@version` with its
   digest and the digests of every dependency. "Latest", ranges and floating
   tags are refused. Two versions are two installations.
2. **Publishing never upgrades.** A publishing v2 changes nothing in an
   organization that installed v1. Adopting v2 is a new, explicit adoption
   intent with its own decision log. v1 stays installed and usable until the
   receiver retires it.
3. **The receiver decides, with its own evidence.** The publisher's report,
   applicability claims and scores are display-only. Acceptance comes from the
   receiver running the method on the receiver's development tasks and scoring
   the outputs with the receiver's trusted evaluator.
4. **A method can't widen authority.** Installing a method grants nothing.
   It runs with at most the scopes the requesting role already holds, under the
   receiver's budget policy, and it can't name, replace or read the evaluator,
   the acceptance rule or hidden evaluation cases.
5. **Uncertainty stays uncertainty.** A registry, manifest, executor or
   evaluator failure is `unknown`. It never reads as "no candidate", never
   triggers synthesis of a replacement method, and never resubmits an effect
   whose outcome is unknown.
6. **Non-reuse is a valid result.** Declining an incompatible method, or
   adopting nothing and developing with the receiver's current method, is
   correct when the evidence says so. Success is judged on the task, not on
   whether something was reused.

## Decision procedure

For one adoption intent (one requirement, one role, one frozen authority
binding, one acceptance suite), in rank order over at most five candidates:

| Step | Check | On failure (decision) | Status (check / decision value) |
| --- | --- | --- | --- |
| 1 | Registry answer well-formed; candidate's score usable | `invalid_response`, discovery stops | check and value on main |
| 2 | Not this organization's own publication | `own_publication`, next candidate | check and value on main |
| 3 | Requesting role holds every declared permission | `missing_scope`, next candidate | check and value on main |
| 4 | Manifest readable and matches the search answer exactly | `manifest_unavailable` (→ `unknown`) or `invalid_manifest`, stop | check and value on main |
| 5 | Artifact kind is one this organization can execute | `unsupported_kind`, next candidate | proposed |
| 6 | Environment compatible: executor kind and version, design-language / simulator version, every dependency digest resolvable and allowed | `incompatible_environment`, next candidate; `dependency_unavailable` (→ `unknown`) if a dependency can't be fetched | proposed |
| 7 | Declared budget fits the receiver's policy (CPU, wall time, experiments, model calls) | `budget_exceeds_policy`, next candidate | proposed |
| 8 | Declared evaluator requirement matches an evaluator the receiver trusts: id, version **and the receiver-resolved content digest** | `evaluator_unavailable`, next candidate | proposed |
| 9 | No `checking`/`accepted` installation of the same exact version | `installation_conflict`, stop | check and value on main |
| 10 | First candidate passing 1–9 | `selected` → local acceptance | check and value on main |

Steps 1–4, 9 and 10 and their decision values are on main (decision log
from #8, legacy #116). Steps 5–8 are new and must be decided **before** anything executes. They never
use similarity. Their reasons name the mismatched requirement (e.g. "needs
design language 2, this organization runs 1"), never hidden evaluation data.

"Next candidate" versus "stop" follows the existing rule: problems specific to
one candidate move on; a problem that makes the registry answer or the selected
manifest untrustworthy stops discovery. This keeps first-eligible selection
unchanged (the legacy #116 review finding).

### Execution purposes

A method has to run before it can be accepted, so "only accepted
installations execute" is wrong as a blanket rule. Following main's
`ExecutionPurpose`, there are three purposes with different authority:

| Purpose | Runs | Authority it needs | What it can produce |
| --- | --- | --- | --- |
| `invocation` | ordinary mission work | an `accepted` installation and the requesting role's current grant | mission effects and an execution receipt |
| acceptance (`local_acceptance` / `adoption_acceptance` on main; method variant proposed) | a `checking` installation on the receiver's development instances only | the adoption intent's frozen binding, bounded by its budget | an acceptance or rejection receipt; never mission effects |
| comparison (#18 purpose, experimental; protocol in #13/#22/#28) | any frozen version, accepted or declined, developing **only on the plan's frozen public instances**; the frozen designs it outputs are then scored separately by the evaluator owner on the committed hidden instances, privately, before the reveal | the comparison plan's frozen grant, not a mission grant | an execution receipt and transcript per run; the separate trusted scoring step produces the score receipts |

A comparison run never grants mission authority, never installs, and never
sees a hidden instance: it develops on public instances, its outputs and
transcript are frozen and every candidate process stops, and only then does
trusted scoring run on the hidden set. A version A declined can still run in a
comparison as the alternative arm; that is a measurement, not an adoption.
#18 carries the execution purpose and execution receipts; #22 carries scoring
receipts. Method-adoption receipts are still proposed here.

### Local acceptance for a method

A task capability is accepted by running it on suite inputs and checking its
outputs. A method's output is a development result, so its acceptance is a
small experiment:

- **Inputs:** the receiver's *development* task set for this requirement
  (not the held-out evaluation set), a fixed budget per task, a seed where the
  executor supports one.
- **Treatment and control:** the candidate method versus the receiver's current
  method (or its configured baseline method if it has none), same tasks, same
  budget, same evaluator (id, version and content digest). No control means the method can only be
  accepted on an absolute threshold, and the receipt says so.
- **Criterion, fixed before the run:** e.g. "at least as many tasks solved as
  the control, with no task regressing past the tolerated margin, within
  budget". The criterion is part of the frozen acceptance suite and its digest
  is bound to the adoption intent like a task suite today.
- **Scoring:** only the receiver's trusted evaluator scores candidate designs.
  Tests or metrics the method generates are part of its trace, never part of
  the verdict.
- **Verdict:** pass → `adopted` with the acceptance receipt reference;
  fail → `not_applicable` with an oracle-free summary (tasks attempted, solved,
  regressions, budget used); every run ending in a confirmed failure without
  scoreable output → `insufficient_evidence`; any run with an unknown executor
  or evaluator outcome → `unknown`.

A small development set can accept a method that later disappoints. That is
expected; the acceptance only claims "these checks ran and passed". Measured
improvement is a separate claim for the comparison runner (#13, #22).

## Outcome vocabulary

The five evidence-distinguished conclusions are already wire values for task
capabilities (#11, legacy #128) and carry over unchanged for methods:

| Outcome | Method meaning |
| --- | --- |
| `adopted` | passed the receiver's acceptance experiment; exact version installed |
| `not_applicable` | ran the acceptance experiment and failed its criterion |
| `insufficient_evidence` | offered but couldn't reach a verdict (invalid manifest/score, or runs that confirmed-failed without output) |
| `no_candidate` | search answered and nothing passed steps 2–8, or it was empty |
| `unknown` | registry, manifest, dependency, executor or evaluator outcome unreadable or unresolved |
| `pending` / `stopped` | still working / stopped by authority, configuration, conflict or record inconsistency |

`RECURSIVE_CAPABILITY_IMPROVEMENT.md` calls the uncertain case
`operational_unknown`; the wire value is `unknown`. The new step 5–8 decisions
(`unsupported_kind`, `incompatible_environment`, `dependency_unavailable`,
`budget_exceeds_policy`, `evaluator_unavailable`) are candidate decisions, not
new outcomes. When every candidate fails them the outcome is `no_candidate`,
except `dependency_unavailable`, which makes it `unknown`.

## Fallback

Unchanged from adopt-first for task capabilities, and explicit per rule:

- `unknown`: no fallback. The intent stops at attention; an operator may retry
  only once every native effect is settled (the #86 / #10 gate).
- `no_candidate` from a definitive search: the rule's fallback decides. For
  methods the fallback is "develop with the receiver's current method", not
  "synthesize a new method", unless a separate reviewed policy adds method
  synthesis.
- `not_applicable` / `insufficient_evidence`: no automatic fallback. The planner
  hears the reason and may choose the current method, another capability, or
  nothing. New fallbacks need an explicit policy and tests.

## Pinning, upgrade and retirement

- An accepted method installation names `method_id@version`, its digest, its
  dependency digests, the acceptance suite digest, and the trusted evaluator's
  id, version and **content digest** as the receiver resolved them. The same
  evaluator digest is pinned in the adoption intent, the acceptance evidence
  and every execution and scoring receipt (#18/#22 carry it as an exact
  content reference).
- Every method execution receipt names the installation and purpose it ran
  under. An `invocation` refuses an installation that is not `accepted`;
  acceptance and comparison follow the purposes above.
- **Job authority and committed acceptance are different things** (on main,
  `capability_authority.rs`). Current authority (requirement, assignment,
  epoch, scopes) is checked for each live job and for the initial acceptance
  decision; a stale in-flight job is fenced. A later epoch or handoff does
  **not** erase an already committed acceptance: a new, currently authorized
  job may use the accepted installation (`routes/capabilities.rs`).
- **Proposed, for methods only:** re-acceptance when the *semantic
  applicability* changes, namely the acceptance suite or criterion, the
  trusted evaluator's content digest (even under an unchanged version label),
  or a dependency digest the receiver resolves.
  This is separate from fencing stale jobs and is not on main.
- v1 and v2 may both be `accepted`. Which one a development run uses is an
  explicit, recorded choice (planner or policy), never "the newest".
- Retiring v1 stops new runs under it. It does not erase its receipts or the
  lineage of anything developed with it.

## Walkthrough: A → B → A

1. **A publishes method v1** after developing it on its ravine tasks. Its
   manifest carries kind, environment, dependency digests, declared budget,
   evaluator requirement, applicability claims and derivation (#18).
2. **B adopts v1.** B's discovery finds v1 (steps 1–10). B runs its
   acceptance experiment: v1 versus B's current method on B's terrain
   development tasks, B's evaluator, B's budget. If v1 passes, B installs
   `v1` pinned; its decision log shows why. If it fails, B records
   `not_applicable` and develops with its own method. Both are valid outcomes.
3. **B develops v2** from v1 under B's governed development process. v2 is a
   new identity with `derived_from: A/method@v1`. B accepts v2 locally the same
   way before publishing it. B's v1 installation is untouched.
4. **A is offered v2.** A's installed v1 does not change. A creates a new
   adoption intent for its return-challenge requirement. A runs its own
   acceptance experiment: v2 versus A's v1 on A's development tasks, frozen
   before the held-out return challenge is drawn. A adopts v2, declines it, or
   records `unknown`.
5. **The return challenge** runs under whichever installation A's planner chose,
   and the comparison runner (not adoption) measures it against the other
   method on the same held-out draw and budget.

Each step yields a distinct, inspectable decision log. Lineage is the chain of
exact identities and acceptance receipts; nothing in it is inferred from names.

## Required from the method contract

To implement steps 5–8 and the acceptance experiment, adoption needs these from
the #18/#22 contracts. Names are placeholders where those drafts don't fix them yet.

- Artifact kind discriminator, with the existing Actor capability as one kind.
- Immutable method identity: id, version, content digest, publisher.
- Environment requirement: executor kind and version, design language/simulator
  version, and a dependency list of exact digests.
- Declared resource budget per development task, in units the executor enforces.
- Evaluator requirement: evaluator id and version the method expects to be
  scored by. The receiver resolves it to a content digest it trusts and pins
  that digest; the method never supplies the evaluator or its digest.
- Input/output interface: task specification type, tool interface, candidate
  design and trace types.
- Derivation: `derived_from` exact identities and the receipt of the
  development run that produced it.
- Execution receipt: #18's fields as merged (binding with method,
  dependencies, task, evaluator ref with content digest, model settings,
  environment, purpose and grant; actual method/environment; outcome
  `output_generated`, `failed`, `interrupted` or `unknown`; outputs; transcript;
  measured or unknown usage). The installation id is adoption evidence, kept
  in the proposed method-adoption receipt, not in #18.
- A terminal-failure signal distinct from transport uncertainty, equivalent to
  `RunFailed`, so the retry gate can stay conservative.

## Implementation boundary (gated)

Not started; requires agreed #18/#22 contracts and the evaluator contract (#13).
When it starts, the adoption side is expected to touch: the adoption coordinator
(artifact-kind branch and steps 5–8), the candidate decision log (new decision
kinds), the acceptance-suite type (treatment/control and criterion), and the
outcome classifier (mapping the new decisions). Kernel installation identity,
the executor and receipts belong to their owners and need explicit claims in
this repository's issue #1. No UI, simulator or evaluator code is part of this lane.

## Acceptance scenarios for the implementation

Each becomes a test once the method executor exists. All use separate
organization databases and grants.

1. **Compatible:** B adopts A's v1 after its own experiment; no regeneration;
   the decision log names the exact version and acceptance receipt.
2. **Same domain, incompatible environment:** v1 needs design language 2, B runs
   1. Declined at step 6 before any execution; outcome `no_candidate` with a
   named reason; B develops with its current method.
3. **Same domain, fails acceptance:** v1 underperforms B's current method on
   B's development tasks → `not_applicable`; v1 is never *invoked* for B's
   mission work. It may still run as a comparison arm, which produces score
   receipts only.
4. **Different task:** if v1 declares a precondition B's task doesn't meet
   (for example design language 2, or a sensor B's language lacks), it is
   declined before execution and its non-use is recorded. Without such a
   precondition a different task label proves nothing: v1 goes through B's
   local acceptance and transfer is measured, not assumed.
5. **Registry, dependency or evaluator timeout:** `unknown`; no method synthesis,
   no resubmission of an unknown run.
6. **Restart and duplicate events:** one decision log, one installation, one
   receipt per run identity; no duplicate external effects.
7. **Authority or epoch change:** an in-flight job under the old binding is
   fenced; a new job under current authority may still use the committed
   acceptance. **Suite, evaluator or dependency change (proposed):** a new
   acceptance is required before further invocation.
8. **Upgrade:** B publishes v2; A's v1 installation and its runs are unchanged
   until A explicitly adopts v2; both versions' receipts stay distinct.
