# Experimental method identity and execution contracts

This implements an **experimental, proposed partial G0 interface**, in the pure
[`genesis-core` module](../../../crates/genesis-core/src/development_method.rs).
It does not implement a simulator, method executor, registry manifest, acceptance
policy, comparison runner, or evidence authentication. Affected-owner agreement
is still pending. The
[ongoing goal](../../ONGOING_GOAL.md) remains open. No runtime or model is selected.

## Freeze before execution

`DevelopmentRunBinding` fixes organization, run and attempt IDs, exact method and
ordered dependencies, public task, evaluator, model settings, execution environment,
purpose and resource grant. Model-settings content must identify the model/version
and settings; environment content must identify the actual executable/dependency
configuration. Their future schemas are not defined by these opaque references.
Holdout cases and seeds must stay outside public task content. A hash of a public
reference is not a confidentiality mechanism.

Dependencies must have distinct `(domain, schema, id, version)` identities: neither
an exact duplicate nor a conflicting digest for the same identity is accepted.
Ordering of distinct dependencies remains part of the binding.

`ExactContentRef` contains nonblank `domain`, `schema`, `id`, `version` and lowercase
64-character SHA-256. The digest addresses exact bytes; a version is an immutable
label, never a request to resolve a moving tag. Known floating aliases and range
selectors are rejected. These validators check the reference, not the referenced
bytes, compatibility or provenance; a future trusted loader must verify those.

The binding hash is SHA-256 of existing `canonical_json` for:

```json
{"domain":"genesis.development_run_binding","version":1,"binding":{}}
```

Here `binding` is the entire serialized typed binding. Array order matters.
The v1 envelope separates this digest from other records; it does not change the
repository's canonicalizer. Protocol numbers are integers, so no new floating
point canonicalization rule is introduced. Equal hashes establish identity,
not authentication, authority or proof that an execution happened.

## Purposes and budgets

Purposes are `ordinary`, `acceptance` (exact plan and `candidate`/`control` role),
or `comparison` (exact plan, trial ID and arm ID). A receipt from one purpose cannot
be relabeled as another. A comparison plan reference does not itself establish a
fair comparison or define the expected roster of trials; that contract is pending.

The grant has independent unsigned integer ceilings for model calls, input tokens,
output tokens, wall-clock milliseconds and simulation steps. Zero means no usage
is granted. Units cannot be exchanged. Each usage field is explicitly `measured`
with a value, or `unknown` with a reason. Unknown is never converted to zero.
An over-limit measured dimension rejects the receipt even if another is unknown.
Future executors must meter and enforce grants; these pure checks only assess
reported usage and cannot detect a dishonest meter or enforce a process deadline.

## Validate a receipt against trusted state

`ExecutionReceipt::validate_against` takes the caller's previously frozen binding.
It requires the receipt's full binding and declared hash to match that value,
and actual method/environment references to match the expected identities.
Recomputing a substituted receipt's hash does not bypass the expected binding.
Transcripts are required. `output_generated` requires at least one output ref;
failed, interrupted and unknown executions can have empty or partial outputs.

The return is explicitly `BudgetAssessment::WithinLimits` or `Unknown`, or a
validation error. Neither successful structural validation nor `WithinLimits`
means the execution succeeded: read `outcome` separately. `output_generated`
means a process produced an artifact, not that the design met a target, passed
evaluation, was adopted, or demonstrated recursive benefit. Separate evaluation
and adoption evidence remains necessary.

Serde rejects unknown fields and wrong wire types; callers must also invoke the
semantic validator. Identifiers, digests and bounds are checked before hashing or
accepting a receipt. No scopes, roles, grants or installed versions are mutated.
No receipt here authenticates its issuer. A forged but internally consistent
record requires trusted transport/issuer verification before use as evidence.

Focused tests: `cargo test -p genesis-core --test development_method`.
These are local contract tests, not live-provider or invention-lab results.
