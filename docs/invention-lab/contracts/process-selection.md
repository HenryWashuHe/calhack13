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

## Whole development roster (library)

`process_cohort::assess_cohort` consumes a trusted `CohortDeclaration` and eight
`process_selection_record::RetainedSelection` records. The retained-selection
wire format and canonical digest are unchanged from the CLI above. The declaration
schema is `genesis-process-cohort-declaration/1`; its eight `SelectionPin` entries
name arm/family, the independently retained canonical `selection_sha256`, and
three expected positions, either `trial { plan: ExactContentRef }` or
`no_method { reason }`. Missing trials must remain declared missing; never remove
or renumber their positions. This is a final development declaration before the
fresh panel, not the earlier six-operation generation admission.

The check recomputes all eight selections and binds every input trial to its full
expected reference. It requires exactly W/F × four families, the same method and
ordered dependencies (or the same absence reason) across each arm/round's four
families, a common control/settings/environment, and one family-specific task,
criterion and instance policy across both arms. Trial identities and each
organization's run/attempt/scoring IDs cannot repeat anywhere in the roster.

Every development grant must be exactly 72,000 steps / 60,000 ms, with zero
model calls/input/output tokens. Every scoring grant is six × family maximum
steps (7,200 projectile; 14,400 otherwise), 600,000 ms and zero model/token
capacity. These are declared numeric grants, not runtime enforcement or actual
costs. Family-specific generator/scoring-rule/criterion/instance refs may differ.
The implementation does not fetch their bytes or assert that caller-supplied
references identify the trusted deployed evaluator.

The report keeps all 24 positions, original outcomes and full selection refs,
including valid zero scores and `no_candidate`. Evidence arrival order is irrelevant.
The caller must bound serialized input sizes and independently pin the declaration
before fresh-task preparation. Rewriting both declaration and evidence defeats
this cooperative consistency check; hashes do not prove origin, chronology or
complete history. No CLI, generation-settings/feedback provenance, whole-chain
cost link, fresh-panel spec generation, execution or adoption is added here.

## Whole-roster capture and recheck

```text
genesis-forge process-cohort-prepare --manifest cohort-input.json --cohort cohort.json
genesis-forge process-cohort-check --cohort cohort.json
```

The strict manifest is `{"schema":"genesis-process-cohort-input/1",
"declaration":"declaration.json","selections":[...eight selection paths...]}`.
Paths resolve relative to the manifest. Supply the independent declaration above
and the eight original `process-select-prepare` captures, in any arrival order.
Every input must be a bounded regular JSON file (8 MiB maximum). Empty paths,
unknown fields, repeated/missing pairs, conflicting pins or inconsistent original
reports fail before publication.

Preparation creates a private, durable, create-only `genesis-retained-process-cohort/1`
snapshot containing the declaration, all eight selections and the recomputed cohort
report. The complete encoded snapshot is also limited to 8 MiB; individually valid
large inputs can exceed this aggregate storage limit and be refused. Checking needs
only that snapshot and writes nothing, even if a stale lock exists. It revalidates
all original selection records and compares the complete derived report.

Both commands emit `genesis-process-cohort-summary/1`: canonical parsed-JSON
`cohort_sha256`, `declaration_sha256`, `report_sha256`, plus all eight arm/family
selection digests and full selected method/plan/round or `null`. A valid all-absent
roster succeeds and retains unknown costs; it is not a successful experiment.
Independently retain/publish these pins before downstream fresh-panel work.
As with the selection CLI, an output-stream failure can follow successful capture:
inspect/recheck the retained destination instead of inventing a new destination.
The preceding trust, chronology, authority and evidence limitations still apply.

## Generation origins (library)

`process_generation_origins::assess_generation_origins` joins the validated cohort
with `genesis-process-generation-origins/1`: one `base` MethodSynthesisPlan and
exactly six `positions`, keyed by `arm` and one-based `round`. Each position holds
an independently retained `prepared_checkpoint_sha256`, optional original
`snapshot`, optional original `revision` (MethodRevisionInput), and an explicit
`no_method_reason` when no source was generated. A never-prepared position can
have no pin; an observed snapshot always requires its original Prepared pin.

The function rechecks cohort pins and complete original sessions. It derives each
Generated package and matches its full reference and ordered dependencies to all
four corresponding development trials. Generated source cannot disappear as
NoMethod, and failed, rejected, pending, unknown or missing attempts cannot supply
a method. It retains those phases and nullable usage instead of inventing zeros.
Attempts and Prepared pins cannot be reused across positions.

The base requires the protocol's Sonnet 5.5 adaptive/high settings. W1/F1/F2/F3
must match its complete plan except operation and target names/version. W2/W3 use
the existing exact-parent revision input and must bind the immediately preceding
generated package; their base documents, runtime, dependencies and settings stay
fixed. Additional revision feedback and instructions are retained but their
provenance and completeness are **not** validated by this check. Trial-level
model-settings references remain separate from source-generation settings.

The source-free report names all six origins, methods, parent references and
observed phases/usage. It expressly leaves feedback provenance, absence-reason
causality, chain admission/cost binding, chronology/exhaustive history, provider
sampling and runtime/adoption authority unverified. Hashes describe consistency
under trusted host storage, not authenticated authorship. This is neither a
complete process-comparison validation nor an executed comparison or G4 result.
