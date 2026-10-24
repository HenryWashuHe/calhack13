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
