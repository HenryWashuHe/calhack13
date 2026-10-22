# Trusted inputs for capability gaps

`genesis_server::capability_gap_policy` converts a committed capability gap into
the existing `CreateAcquisition` request. The operator supplies the policy;
the planner supplies only the need recorded by `RequestCapability`.

This module is the policy component of the gap driver. It is not yet connected
to server startup, a CLI flag or a background worker. Calling `materialize`
does not create an acquisition, authorize work or contact a provider.

Each rule selects a capability ID, configured suite ID, Actor name prefix,
exact source allowlist, fixed public HTML examples and whether one repair is
allowed. Unknown JSON fields, duplicate capability rules, invalid synthesis
inputs and oversized policies are rejected. The policy schema has no oracle,
credential, provider-endpoint or model-configuration fields; the existing
independent oracle and acquisition policy retain those responsibilities. Operators
must supply public HTML examples without credentials or private oracle answers.
Validation bounds that HTML but cannot establish its provenance.

`materialize(&org_id, &gap)` requires every requested source to appear in the
matching rule. It narrows both URLs and examples to that requested set, requires
at least one matching example, and preserves the gap's committed mission,
requirement, role and responsibility IDs. It never fetches a URL or copies the
gap's purpose into synthesis instructions. The acquisition store loads the
current authoritative requirement separately.

The organization and gap IDs determine a stable candidate identity. Primary and
repair Actors have distinct names and capability versions. Source versions are
deterministic and scoped to their individual Actor; they are not globally unique.
The gap ID is the acquisition idempotency key. The suite must be compatible with
the narrowed URLs; a materialized policy is not evidence that the suite will pass.

The upcoming durable driver must persist the exact request and policy digest
before creation, verify the frozen requirement and current assignment authority,
and recover the same intent after a lost response. The existing acquisition
runtime remains responsible for quotas, deadlines, provider effects and independent
verification. Publication, local acceptance and attachment remain separate steps.

Tests cover strict decoding, bounds, source narrowing, missing examples,
deterministic identities across serialization/restart, independent organization
and repair identities, and exclusion of planner-controlled instruction text.
These are local policy tests; no live provider evidence is claimed.
