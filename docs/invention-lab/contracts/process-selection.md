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
