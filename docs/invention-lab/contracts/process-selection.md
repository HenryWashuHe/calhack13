# Development selection primitive

`genesis_forge::process_selection::select_development` implements the
[process-comparison/1 development selection rule](../protocols/process-comparison-1.md#selection-frozen-before-the-fresh-panel)
for **one arm, one family, exactly three round positions**. It is a pure library
function, not a command, scorer, controller, acceptance decision or grant.

The caller supplies independently retained expected TrialPlans, actual criterion
bytes and optional candidate TrialEvidence/metrics bytes. A `NoMethod` reason is
the trusted caller's absence statement, not validated generation history or zero
spend. Receipt fields cannot substitute for the expected plan. Family seeds must
be 0..5; criterion, complete cohort policy, control/dependencies, organization,
receipt identities, metrics identity/digest and ordered boolean instance results
are checked. Repeated logical trial/run/attempt/scoring identities are refused.
Distinct attempts producing the same package remain distinct observations.

All three rows survive. Missing execution, scoring or metrics remain absent,
never score zero. Failed/Interrupted/Unknown executions keep their observed
counts and usage but cannot be selected. Eligible zero is a real score. Selection
uses successes descending, measured simulation steps ascending, original round
ascending, then package digest. A null `selected_round` means no eligible
candidate; the exact method and plan remain in the selected row.

Eligibility follows the frozen protocol literally: OutputGenerated, all five
execution usage fields known, Scored within its grant and a validated count.
Known development overruns are separately visible; this report does not admit
an overrun for execution, scoring, adoption or a positive endpoint. All costs
still belong in the existing whole-chain accounting path.

The report hashes the normalized typed inputs, not original JSON formatting.
Metrics are bounded to 1 MiB and criterion bytes to 64 KiB. Input hashes establish
neither timing nor measurement origin. The caller still must bind the full six
operation/four-family roster, generation settings and budgets, generation status,
random-control package, source/selection freeze and fresh-panel endpoint. This
primitive's trial model-settings equality does not establish equal model settings
during generation. No live provider, generated source or evaluator runs here.

Cohort equality does not authenticate authoritative family/context bytes or
enforce the protocol's numeric 60,000 ms / 72,000-step development ceilings.
The report explicitly labels its scope `single_arm_family_development`;
the protocol name identifies the selection rule, not a completed global check.

Slot eligibility comes from the shared pure
`genesis_core::development_verdict::assess_process_slot(plan, slot_id, &SlotEvidence)`.
It rebuilds identities from the expected plan and returns a serializable
`ProcessSlotAssessment` with the exact plan/method/slot, observed outcomes and
usage, steps, overrun/within-grant flags, optional successes, eligibility and
reasons. Both candidate and control public slots are supported, including the
12-instance fresh panel. It rejects HiddenCommitment instead of inferring hidden
instance order. The selector still owns its three-round cohort/criterion checks;
the helper does not validate the full protocol or change `decide`'s acceptance rule.

## Operator capture and read-only check

`genesis-forge process-select-prepare --manifest selection-input.json --selection selection.json`
resolves paths relative to the manifest, loads all three original positions and
calls the same selector. It captures the typed plans/receipts and exact criterion/
metrics bytes with the derived report, under schema
`genesis-retained-process-selection/1`. Publication is create-only through the
existing private checkpoint storage; the destination directory must exist.
No package/source is loaded or executed. Example input (replace paths with the
independently retained expected plan and its evidence):

```json
{
  "schema": "genesis-process-selection-input/1",
  "arm": "workflow",
  "family": "rough-transport",
  "rounds": [
    {"kind":"trial", "plan":"round1/plan.json", "criterion":"round1/criterion.json",
     "evidence":"round1/trial-evidence.json", "metrics":"round1/metrics.json"},
    {"kind":"no_method", "reason":"generation failed"},
    {"kind":"no_method", "reason":"no_parent"}
  ]
}
```

`trial-evidence.json` is the existing `TrialEvidence` object with `slot_id`,
`execution` and optional `scoring`; it is not a scorer-provided pass/count flag.
Evidence or metrics may be null/missing, preserving unknown observations.
The other arm is `fresh`; all four declared family names are accepted.
Inputs are bounded regular files (manifest/plan/evidence 8 MiB, criterion 64 KiB,
metrics 1 MiB); the complete retained record must fit 8 MiB after encoding.

`genesis-forge process-select-check --selection selection.json` reads only this
self-contained capture, recomputes selection and checks the complete report.
It neither touches a leftover lock nor needs the original paths. Both commands
emit `genesis-process-selection-summary/1`: exact selected method/plan and round,
or `selected: null`, plus canonical SHA-256 of the selection, input and report.
These digests describe parsed, normalized JSON, not original file whitespace.
Exit 0 means a valid report, including no candidate; exit 2 rejects malformed or
inconsistent data. A failed stdout write may follow a successful capture: inspect
the retained path instead of choosing a new path and repeating preparation.

Publish the selection digest independently before fresh-panel execution. The
local create-only record and self-consistency check do not prove chronology or
authentic origin, and a host can rewrite both inputs and report. Consumers must
compare the externally pinned digest and selected full method reference, and
must separately bind the complete eight arm/family selections, controls, common
round methods, model/settings, numeric grants and generation history. No complete
experiment, receiver acceptance or execution grant is inferred here.
