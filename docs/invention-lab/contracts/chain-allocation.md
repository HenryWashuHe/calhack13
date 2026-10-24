# Finite chain ceilings

`method_chain::ChainAllocation::new` validates an ordered list of 1–64 stages
before participating work. Each stage names its organization, unique stage ID,
existing `StageLimits`, exact trial-model-settings reference and one work kind:

- `Generation`: exact generation settings, 1–64 maximum attempts and a complete
  generation grant. The grant must cover every declared call and output cap.
  It may explicitly declare zero trial slots with all development/scoring grant
  dimensions zero; such a stage cannot bind a trial. Later evaluations belong in
  separately declared `TrialOnly` stages and do not repeat the generation cost.
- `TrialOnly`: an inherited-method, return, control or decline trial without a
  generation roster or generated-method links. It requires 1–64 trial slots and
  does not manufacture a call.

Checked arithmetic requires each full generation allowance plus every possible
trial slot's development/scoring grants to fit its stage ceiling. The sum of
**all full stage ceilings**, including unused or declined stages, must fit the
chain grant in all five dimensions. Repetitions require declared slots/stages;
unused capacity does not move elsewhere. Stage order contributes to the hash.
A decline without execution leaves its declared stage unbound and fully reserved.

`bind_generation` pins a later existing `StageAllocation` to one declared stage,
matching exact limits, settings, generation grant, organization and attempt cap.
Its `ChainStageBinding::validate_generation` rejects restored substitutions.
Before a generated-stage trial, `validate_trial` additionally requires the exact
existing `StageTrialBinding`, plan and scope, with the declared model reference
and organization. Existing stage serialization and hash domains are unchanged.

`bind_trial_only` pins the exact trial and cost-scope hash with matching grants,
ordered slots, model reference and organization, and no generation entries/links.
Validate a restored binding with `validate_trial(..., None, trial, scope)`.
Existing cost assessment retains missing evidence as unknown; unused declared
capacity is not measured zero spend. This increment adds no combined report.

These are pure structural checks. The host must retain the chain before A starts,
keep one authoritative binding per stage and consume it before work. The library
can construct multiple bindings; it adds no persistence, ledger or controller.
Copies, rollback, alternative paths, chronology, actual metering and exhaustive
history remain outside this guarantee. Per-stage settings are pinned, not forced
equal across stages; their equality, model provenance and fair controls require
the comparison protocol. Provider-default sampling is not fixed G4 evidence.

The three already observed generation calls cannot retroactively acquire this
declaration. Their package remains valid generation evidence, with all failures
retained. A future controlled chain needs its own prior declaration. No runtime,
scoring, acceptance, inheritance, authenticated organization or benefit is proved.


## Retained CLI files

The following commands are offline; no generation, admission, execution or scoring
command implicitly consumes the chain in this increment:

```sh
genesis-forge method-chain-prepare --manifest chain-input.json --chain chain.json
genesis-forge method-chain-bind --manifest stage-input.json --chain chain.json
genesis-forge method-chain-check --manifest stage-input.json --chain chain.json
```

`chain-input.json` is the serialized `ChainAllocation` declaration (`schema_version`,
`id`, `total_grant`, `stages`). Preparation validates its complete ceilings and
retains it create-only under the existing checkpoint lock. It cannot establish
when prior work happened; declare and retain it before participating work.

A stage manifest names one declared stage and paths relative to the manifest:

```json
{"schema":"genesis-chain-stage-input/1","stage_id":"develop-a","target":{"kind":"generation","allocation":"stage.json"}}
```

Generation binding checks the existing allocation's exact roster/settings/grants.
Add `"trial":{"plan":"trial.json","scope":"cost-scope.json"}` inside `target`
to additionally require the existing `stage.json.trial-binding.json` and validate
that exact trial. Without `trial`, success checks only the generation allocation.
For inherited/control/return work, use this target shape instead:

```json
{"schema":"genesis-chain-stage-input/1","stage_id":"return-a","target":{"kind":"trial_only","plan":"trial.json","scope":"cost-scope.json"}}
```

Binding holds the chain lock through validation and create-only save to the fixed
`chain.json.stage-<stage_id>.json` companion. Repeated binding cannot overwrite it,
even for the same inputs. An undeclared stage, wrong kind, changed grants/settings,
invalid schema or missing named input fails without a binding. Trial-only inputs
must contain zero generation entries and zero generated-package links.

Checking reads the exact companion and named records without locks, recovery or
writes; it leaves occupied/stale locks untouched. Exit 0 means that structural
binding matches this snapshot, not runtime permission, completed work or a verdict.
Before a generated-stage trial, include its `trial` paths in the check manifest.
The host must keep inputs immutable and use canonical chain/allocation paths:
copies, alternate paths with other companions, rollback, races with external
writers and skipped checks remain outside this cooperative guarantee. A declined,
unexecuted stage can remain unbound while its full ceiling stays reserved.

## Optional generation-step guard

```sh
genesis-forge method-generate-step --checkpoint generation.json \
  --admission admission.json --allocation stage.json --chain chain.json
```

On `method-generate-step`, `--chain` requires `--allocation`, which requires
`--admission`. After validating the allocation against that admission journal,
the command reads the chain and its fixed
`chain.json.stage-<allocation.limits.id>.json` companion. The retained binding
must match the exact chain and generation allocation. There is no separate
stage selector or caller-chosen binding path.

These checks precede session recovery, credentials, reservation and provider
effects. A missing, invalid or substituted chain/binding leaves the generation
checkpoint and admission journal unchanged, including a pending attempt. With
valid records, existing recovery converts an interrupted request to unknown
without replay or refund. Prepared attempts retain the existing durable
reservation and write-before-effect ordering.

The option is intentionally optional; generation steps without it keep their
existing behavior and do not establish a chain guard. It adds no controller,
trial-runtime gate, source execution, automatic retry or proof of chronology.
Canonical paths and immutable retained inputs remain host responsibilities.

## Whole-chain observed costs

`method_chain_costs::assess_chain_costs(chain, evidence)` recomputes costs from
original admitted generation snapshots and bound trial evidence. It accepts no
caller-authored totals. `ChainStageEvidence::Generation` supplies the retained
chain binding, allocation, admission, original scope and current snapshots, plus
an optional exact stage/trial binding and evidence. `TrialOnly` supplies its
chain binding, trial, scope and evidence without inventing a generation.

Every declared stage appears in declaration order, even if inputs arrive in a
different order or a stage is omitted. Each retains its full reserved ceiling,
binding, nested observations and original individual budget assessments. The
chain totals sum the recomputed combined costs once, without adding the nested
generation report again. Later use of an already generated method belongs in a
TrialOnly stage. Distinct attempts producing identical packages still cost money;
package identity is never used to deduplicate spending.

Duplicate stage inputs, generation operations/requests/prepared identities,
logical trial identities (including changed bodies under the same name/version),
organization-scoped run IDs or attempt IDs, or receipt identities across stages are
errors. The caller must give each effect one cost owner; shared comparison
references do not authorize charging it twice. Foreign chain/allocation/trial
bindings fail before aggregation. Existing accounting still rejects substituted
or malformed evidence and preserves known overruns and unsuccessful attempts.
The run/attempt rule extends TrialPlan's existing uniqueness policy across the
chain; changing one label cannot reuse the other within the same organization.
It rejects ambiguous retained identities, not proof of duplicate physical work.

A generation observation without a trial retains `unresolved_trial` when its
declaration has nonzero trial slots, even when their grants are zero. For an
explicit zero-slot Generation stage, validated binding/allocation/admission and
generation evidence have no future trial to resolve. Complete measured generation
can yield known totals; failed attempts still cost resources, and missing,
Prepared, pending or unknown generation evidence remains unknown as before.
The unused trial-model-settings reference remains declared but supplies no costs.
An omitted or declined-without-evidence stage is also unknown, never zero spend.
A bound trial still reports missing execution/scoring evidence through the existing
cost rows. Known lower bounds, unknown flags, saturating overflow and chain-limit
excess are separate; nested generation and individual limits remain visible even
when a larger chain ceiling is not exceeded. Summed request/run wall times are
not elapsed study duration.

This is read-only accounting over trusted retained records, not a runtime
guard, global enforcement, exhaustive provider billing, chronological proof or
acceptance/improvement verdict. It neither authenticates organizations nor proves
that omitted work did not happen. Adaptive later rounds must prepare their exact
requests only once feedback exists, within previously retained chain ceilings;
no future prompt hash is fabricated before those inputs are available.

### Read-only chain cost CLI

```sh
genesis-forge method-chain-cost-assess --chain chain.json --manifest chain-costs.json
```

The strict `genesis-chain-cost-input/1` manifest selects up to 64 declared stages:

```json
{"schema":"genesis-chain-cost-input/1","stages":[
  {"stage_id":"develop-a","target":{"kind":"generation","cost_manifest":"a/costs.json","allocation":"a/stage.json","admission":"a/admission.json"}},
  {"stage_id":"return-a","target":{"kind":"trial_only","cost_manifest":"return/costs.json"}}
]}
```

Outer paths resolve relative to this manifest. Each nested manifest uses the
existing [cost-input format](accounting-cli.md) and resolves its own scope,
checkpoint and trial paths relative to its own directory. The command reads the
fixed `chain.json.stage-<stage_id>.json` companion for every selected stage.
Generation stages with a trial also require the fixed
`stage.json.trial-binding.json` companion; generation-only observations do not.
It never constructs replacement bindings or accepts supplied totals.

TrialOnly inputs reject allocation/admission fields and generation snapshots,
expectations or links. Duplicate/undeclared stage IDs and malformed schemas fail.
Omitted stages remain unknown; any named missing, invalid or foreign file fails
with exit 2 and no report. An empty stage list is valid and reports every declared
stage as unknown. Inputs are read once per use from trusted host storage, without
locking or a cross-file atomic snapshot; retain immutable files for consistency.

Exit 0 means report construction, including unknowns or overruns, not compliance
or acceptance. The command does not read credentials, recover pending requests,
reserve capacity, write/lock files or invoke providers/execution/scoring. Existing
`method-cost-assess` behavior remains unchanged.
