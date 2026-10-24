# Finite chain ceilings

`method_chain::ChainAllocation::new` validates an ordered list of 1–64 stages
before participating work. Each stage names its organization, unique stage ID,
existing `StageLimits`, exact trial-model-settings reference and one work kind:

- `Generation`: exact generation settings, 1–64 maximum attempts and a complete
  generation grant. The grant must cover every declared call and output cap.
- `TrialOnly`: an inherited-method, return, control or decline trial without a
  generation roster or generated-method links. It does not manufacture a call.

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
writers and skipped checks remain outside this cooperative guarantee. Existing
generation admission and runtime behavior are unchanged. A declined, unexecuted
stage can remain unbound while its full ceiling stays reserved.
