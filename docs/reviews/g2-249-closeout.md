# G2 attempt 1 closeout review

Reviewed [PR #249](https://github.com/HenryWashuHe/genesis-history-experiment/pull/249)
at `479c35e617f34c6e2e7c722e7a7188b789da1116`. Its historical run used the
unchanged driver/protocol from `37706b15cb53380eea952374f8d55867b03f2140`.
The fixes below are prospective. The protocol, input specs, archive README,
records and manifest remain byte-identical; they do not inherit later fixes.

## Driver corrections

Ordinary port-allocation, organization/planner/helper launch and process-log
open errors now reach the existing per-instance stop summary. A stop preserves
prior recorded fields and permits the next isolated instance to proceed.
Evidence-storage failures and top-level setup can still terminate the driver;
this is not a claim of crash-safe or universally panic-free execution.

A settled trial verdict alone did not implement the protocol's process
eligibility exclusion: the general verdict function can accept a Failed or
Interrupted execution with scoreable output. Before installing the receiver
or deciding, the driver now validates both retained slots against their exact
typed plan with the existing `assess_process_slot` rule. It retains the
assessment separately. An eligible rejection remains a legitimate non-reuse
outcome. Missing, foreign, tampered or ineligible records cannot authorize the
decision. No shared executor, scoring rule or receiver policy changed.

Historical driver SHA-256:
`9885de59f9be9aa7a4918f60763d8c6ddf860453e2cd5e2c47c0dbfc96feacf4`.
Corrected driver SHA-256:
`b458a6d27c444b28136b39ceb29de70395d4e0ef89f58e44f9e8e8414a6ef054`.

## Historical evidence and limits

The independent audit uses committed bytes, not a repeat of the run. All 824
manifest entries cover the 825 archived files except the manifest itself;
all 454 content-addressed artifacts match their filenames. Manifest SHA-256:
`9d2ab6385531db1ad395932cb2aac8f7320619591e09ec4b5b6fd5f49f6fce34`.

All 18 retained execution/scoring pairs (16 comparative slots and two
invocations) have matching binding/request hashes, method, environment,
context, purpose and ordered 12-instance metrics. Their measured usage is
within the recorded grants. All are `output_generated` and scored; the
prospective Failed/Interrupted eligibility bug is not encountered in these
records. The pinned candidate/control scores are 12/4, 7/7, 11/7 and 8/8;
the two fresh invocation scores are 10/12 and 11/12. This validates retained
record consistency, not externally authenticated execution or provider costs.

The historical README's phrase “organizational authority isn't enforced” is
too broad. Unknown tokens and mismatched grants are refused in the retained
controls. The narrower gap is that adoption/invocation do not require the
selected role to hold `method.develop`; B2's lead only holds
`mission.coordinate`. This review does not change that policy or claim the
B2 specialist expectation passed. The original hashed README is preserved.

The author's four planner requests are not independently authenticated billing
or token measurements. This is one host/operator with stub provider
provisioning; the two B configurations vary charter and scope together.
Same-cart ties are refusal plumbing controls, not a planner-quality experiment.
The retained observation does not establish independent organizations,
recursive improvement or completion of G0–G4.

## Validation

Five pure retained-record regressions pass using the `review_249::` filter in
`g2_attempt_1`; both ignored live drivers/helpers are filtered out. They retain
accepted and rejected eligible outcomes, reproduce generic acceptance with
Failed/Interrupted receipts before the stricter check rejects it, reject
unknown/unsettled/foreign/tampered evidence, and preserve earlier fields on
operation failure. Scoped strict Clippy, Rust formatting and whitespace checks
pass. No driver, generated method, provider, evaluator, scorer or namespace
was executed by this review.
