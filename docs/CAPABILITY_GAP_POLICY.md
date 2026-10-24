# Trusted inputs for capability gaps

`genesis_server::capability_gap_policy` converts a committed capability gap into
the existing `CreateAcquisition` request. The operator supplies the policy;
the planner supplies only the need recorded by `RequestCapability`.

The gap driver is an explicit server startup option. It claims committed gaps,
creates durable acquisition intents and projects independently verified results
back to `Acquired`. The existing acquisition worker owns model calls, Actor
builds and verification. Calling the policy's `materialize` helper alone still
does not create an acquisition, authorize work or contact a provider.

Enable it for one organization with all three acquisition options:

```sh
genesis org --org-id org_a --name "Org A" \
  --oracle-suite config/trusted-suite.json \
  --acquisition-policy config/acquisition.json --auto-acquire \
  --gap-acquisition-policy config/gaps.json
```

These are operator-owned local files. Supply the ordinary organization tokens
and `ANTHROPIC_API_KEY` / `APIFY_TOKEN` through the existing environment setup.
The CLI requires both `--auto-acquire` and `--acquisition-policy` when the gap
policy flag is present. Startup validates the rules and requires configured
model and Apify clients; it makes no provider calls during configuration.
Without the gap policy flag, automatic acquisition only advances already
authorized acquisition intents. `genesis dev` does not enable the gap driver.

Gap policy input is bounded to 4 MiB, must be a regular JSON file, and rejects
duplicate keys. Decode/validation errors and configuration Debug output omit
example HTML. The private file and its frozen storage records must still be
treated as trusted backend configuration.

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

The durable driver freezes the exact request, policy digest, model policy, suite
digest, absolute deadline and assignment identity. Later policy changes apply to
new gaps; they cannot rewrite an existing binding. A missing or changed suite
leaves the frozen binding in attention. A role awaiting activation can proceed
once active under the same assignment, epoch and authority.

The driver verifies the committed requirement and current assignment authority,
and recovers the same intent after a lost response. Child creation, quota debit,
acquisition history and the durable parent link commit together. A failed gap
status write can then be retried without creating another child. The existing acquisition runtime remains responsible
for quotas, deadlines, provider effects and independent verification. An unknown
provider outcome requires attention; it is not permission to create another
intent or replay a provider submission. Failed reads retain the existing explicit
retry requirement.

Operators can inspect redacted progress at
`GET /api/v1/capability-gaps/{id}/acquisition`. This endpoint requires the
organization's Operator token and does not expose frozen HTML or oracle answers.
`Acquired` means a verified exact version, not publication or installation.
This driver does not publish a manifest, grant provider access, accept an
installation or attach a capability to a role. Those remain separate steps.

Tests cover strict decoding, bounds, source narrowing, missing examples,
deterministic identities across serialization/restart, independent organization
and repair identities, and exclusion of planner-controlled instruction text.
Startup/CLI checks also cover explicit opt-in, prerequisite configuration, bounded
file loading and redacted diagnostics. Driver tests cover concurrent intake,
transaction rollback, restart recovery, changed requirements and assignments,
unknown outcomes and paused reads. A configured server also advances a committed
gap to `Acquired` using loopback model/Actor responses without manual step calls.
These are local checks; no live provider evidence is claimed.
