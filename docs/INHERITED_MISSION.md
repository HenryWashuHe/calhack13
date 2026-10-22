# Acquisition and inheritance across two organizations

`genesis scenario` connects the public organization APIs into one operator-directed
mission. The driver records a portal requirement and commits a specialist
responsibility. Organization A generates, builds and verifies a capability; Organization B discovers
that exact artifact, collects its own acceptance evidence and uses it. Both temporary
roles then retire with explicit responsibility handoffs. B's successor uses the
same accepted artifact without generating another tool.

This demonstrates a structural change and executable capability inheritance. The
scenario chooses its changes explicitly. Autonomous planning, live Elastic role
creation, authenticated Fetch recruitment and CREAO role activation remain separate
integration gates owned by Claude. A development binding is reported as such.

## Run the deterministic integration proof

```sh
cargo test --locked -p genesis-server --test capability_scenario
```

The fixture uses separate organization databases, operator tokens and trusted suites,
a shared registry, and the actual Rust generation and Apify HTTP clients pointed at
loopback providers. Canned model and provider replies make the evidence repeatable;
they do not execute a cloud build or prove model-generated parser quality. The
scenario has no direct database access and cannot insert verification receipts.
The existing manual fixture exercises one repair, B's independent acceptance checks,
initial and successor invocations, then reopens B from its SQLite database and
invokes again with the successor. B's runs share the repaired artifact and consume
no model or Actor-create calls. Both commitment ledgers remain empty.
Acceptance jobs remain `VERIFYING`, while
fresh invocation jobs become `SUCCEEDED` only after backend semantic verification.
A negative fixture returns incorrect fresh data after valid acceptance and confirms
the scenario stops before retirement despite successful provider transport.

Automatic-mode validation must run the actual `serve_org` workers for acquisition,
provider execution and semantic finalization. A router-only fixture driven through
explicit step or verify requests does not establish automatic progression. The
command above is the validation entry point; no live-provider or cloud-execution
claim follows from a passing loopback test.

## Run against configured organization servers

Start two **fresh** organization databases and a registry. Both organizations need
registry access, their own operator credentials and an Apify executor. Only A enables
`--acquisition-policy` and configures a model client. For the recommended automatic
example, A also enables `--auto-acquire`; this requires both its model and Apify
clients to be configured. Both load trusted oracle suites through `--oracle-suite`;
B's suite must contain the exact intended invocation input.
See [SERVER_ACQUISITION.md](SERVER_ACQUISITION.md) and
[CAPABILITY_EXECUTION.md](CAPABILITY_EXECUTION.md) for server setup.

Copy `crates/genesis-server/examples/scenario/mission.json` into your runtime folder.
It contains public examples and an invocation input, never expected oracle answers.
The example sets `"progression": "automatic"`. The optional `progression` field
accepts `"manual"` or `"automatic"`; omitting it preserves manual progression for
existing specs. Set `"progression": "manual"` to have the driver issue explicit
step and verification requests as before.

In automatic mode, run both organizations through `genesis org` or an embedding
that calls `serve_org`. The driver creates explicit authorized acquisition and run
intents, then polls the acquisition automation status, run state, finalization status
and persisted proofs. It never sends a `/step` or `/verify` request. A's
`/api/v1/health` field `automatic_acquisition_enabled: true` confirms configuration
only; it does not prove that a worker is running or making progress. A missing or
stalled worker is still subject to the driver's polling and elapsed-time bounds.
Automatic progression does not infer new acquisition intents from requirements or
choose organization changes; those remain explicit actions in this scenario.

The sample uses synthetic `.example` sources: host the public fixtures and update the
public example URLs, allowlist, invocation and trusted source identities consistently
before a cloud Actor run. Configure independently checked expected data in each
backend suite. Change candidate Actor names/version identifiers for a new run.

The sample's A suite is `offline-venue-suite-v1`; its B suite is
`inherited-venue-suite-b-v1`, with a 150-attendee table input. For a local synthetic
configuration, copy the existing trusted suite and change its `suite_id` and first
case's `input.attendees` to match. This is a distinct configuration and fresh local
verification, not a claim of broader unseen-source coverage. Keep expected answers
out of the scenario spec and generation examples.

Acquired Actors are private. Manifest publication does **not** grant another Apify
account permission to run one. Configure B with separately managed credentials that
are already authorized to execute the selected Actor, and then set
`provider_access_acknowledged` to `true`. The driver transfers no provider secrets.
Two logical organizations using an explicitly permitted shared provider resource
prove organizational isolation, not independent-account access brokerage.

The current CLI can run Apify while using explicitly labeled development role
provisioners. The example sets `allow_development_roles: true` for this case; the
type defaults to `false`. Without the opt-in, preflight refuses enabled development
provisioning before creating missions or triggering model work. Production sponsor
claims require the actual role adapters and their readiness integration.

Provide `GENESIS_ORG_A_OPERATOR_TOKEN` and `GENESIS_ORG_B_OPERATOR_TOKEN` in the
driver's environment. Model and Apify credentials remain in the organization server
environments. The driver accepts HTTPS origins or literal loopback HTTP origins;
use `127.0.0.1`, not a hostname, for local HTTP.

```sh
cargo run --locked -p genesis-server -- scenario \
  --spec runtime/mission.json \
  --org-a http://127.0.0.1:7101 \
  --org-b http://127.0.0.1:7102
```

The successful JSON report contains the acquisition ID and counters, exact
source/Actor/build pin, B's independently collected acceptance job IDs and reference,
initial and successor invocation IDs and proof references, responsibility owners and
actual provider bindings. B's zero-synthesis result is scoped to this scenario and
the server's disabled acquisition configuration; it is not an account-wide billing audit.
The report omits generated source, HTML, oracle answers, execution outputs and tokens.

## Workflow and limits

1. Refuse identical organizations, nonempty state, missing registry configuration,
   missing constitution scopes, acquisition enabled on B, or unauthorized dev roles.
2. Create A's mission, observation and requirement. Commit a specialist role and
   requirement-backed responsibility before requesting acquisition.
3. Create the explicit acquisition intent. In automatic mode, wait for A's worker
   to advance generation, Actor creation/build and all verification cases. In manual
   mode, request each step explicitly. If configured, allow one semantic repair with
   a new version and Actor. Stop when operator attention or reconciliation is
   required, or on terminal failure.
4. Publish through `acquisition:<id>`. The server recomputes verification from its
   own proof; the driver supplies no affirmative verification assertion.
5. Create B's mission and responsibility, search the registry and install the exact
   published artifact. Collect B's own acceptance jobs and commit local acceptance.
6. Attach, invoke and require a fresh supported result with a persisted semantic
   proof. In automatic mode, server workers collect and finalize the invocation;
   the driver reads their status and proof. Retire A's temporary specialist with a
   handoff, then retire B's verifier with a handoff to a persistent monitor. Attach the
   accepted capability to the monitor and verify another invocation under its grant.
7. Confirm acceptance and the artifact pin survived retirement, and A incurred no
   additional acquisition effects during B's inheritance.

Every request, response and polling loop is bounded. Redirects and automatic POST
retries are disabled. A driver timeout or disconnect does **not** cancel authorized
server work: the acquisition, execution and finalization workers may continue while
their grants, deadlines and quotas permit. Inspect durable organization, acquisition,
automation, run, finalization and proof records before taking further action.
The driver does not resume a partial narrative or automatically rerun it; rerunning
against nonempty organizations is refused. Retain the databases for inspection
instead of deleting evidence to force a rerun. Existing
acquisition and capability runtimes own restart recovery and effect reconciliation.

Invocation currently accepts only an exact input in B's configured suite. Collected
output remains evidence until the separate backend verifier issues an immutable
proof. The driver checks exact job/request/input/artifact/acceptance/suite bindings,
requires supported extraction, and reads the same persisted proof back. This
mission proves inherited lookup and continuing ownership; it makes no booking or
external commitment.

## Authority after handoff

Fresh capability checks and invocations require a current assignment, an unexpired
lease, an active mission and an unexpired mission/job deadline. Provider completion
rechecks the same grant and assignment epoch before accepting a result. Initial local
acceptance also requires its proof jobs to retain authority.

Once acceptance commits, its verification evidence belongs to the organization.
Retiring the verifier does not erase it. A successor can reuse that accepted proof
with its own current assignment and capability attachment. The producer's retirement
also cannot revoke B's already accepted local evidence implicitly.
