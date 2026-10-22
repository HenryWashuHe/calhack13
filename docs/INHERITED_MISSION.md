# Acquisition and inheritance across two organizations

`genesis scenario` connects the public organization APIs into one operator-directed
mission. A newly recorded portal requirement creates a specialist responsibility;
Organization A generates, builds and verifies a capability; Organization B discovers
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

## Run against configured organization servers

Start two **fresh** organization databases and a registry. Both organizations need
registry access, their own operator credentials and an Apify executor. Only A enables
`--acquisition-policy` and configures a model client. Both load trusted oracle suites
through `--oracle-suite`; B's suite must contain the exact intended invocation input.
See [SERVER_ACQUISITION.md](SERVER_ACQUISITION.md) and
[CAPABILITY_EXECUTION.md](CAPABILITY_EXECUTION.md) for server setup.

Copy `crates/genesis-server/examples/scenario/mission.json` into your runtime folder.
It contains public examples and an invocation input, never expected oracle answers.
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
initial and successor invocation IDs, responsibility owners and actual provider
bindings. B's zero-synthesis result is scoped to this scenario and the server's
disabled acquisition configuration; it is not an account-wide billing audit.
The report omits generated source, HTML, oracle answers, execution outputs and tokens.

## Workflow and limits

1. Refuse identical organizations, nonempty state, missing registry configuration,
   missing constitution scopes, acquisition enabled on B, or unauthorized dev roles.
2. Create A's mission, observation and requirement. Commit a specialist role and
   requirement-backed responsibility before requesting acquisition.
3. Advance the durable acquisition through generation, Actor creation/build and all
   verification cases. If explicitly configured, allow one semantic repair with a
   new version and Actor. Stop on reconciliation or terminal failure.
4. Publish through `acquisition:<id>`. The server recomputes verification from its
   own proof; the driver supplies no affirmative verification assertion.
5. Create B's mission and responsibility, search the registry and install the exact
   published artifact. Collect B's own acceptance jobs and commit local acceptance.
6. Attach and invoke the capability. Retire A's temporary specialist with a handoff,
   then retire B's verifier with a handoff to a persistent monitor. Attach the
   accepted capability to the monitor and collect another invocation.
7. Confirm acceptance and the artifact pin survived retirement, and A incurred no
   additional acquisition effects during B's inheritance.

Every request, response and polling loop is bounded. Redirects and automatic POST
retries are disabled. On timeout or an ambiguous effect, inspect the server's durable
organization/acquisition/run records before continuing. The driver does not resume
a partial narrative; rerunning against nonempty organizations is refused. Retain
the databases for inspection instead of deleting evidence to force a rerun. Existing
acquisition and capability runtimes own restart recovery and effect reconciliation.

Invocation currently accepts only an exact input in B's configured suite. Collected
output is verification evidence, not a booking, external commitment or generic
successful job result. This mission proves inherited lookup and continuing ownership.

## Authority after handoff

Fresh capability checks and invocations require a current assignment, an unexpired
lease, an active mission and an unexpired mission/job deadline. Provider completion
rechecks the same grant and assignment epoch before accepting a result. Initial local
acceptance also requires its proof jobs to retain authority.

Once acceptance commits, its verification evidence belongs to the organization.
Retiring the verifier does not erase it. A successor can reuse that accepted proof
with its own current assignment and capability attachment. The producer's retirement
also cannot revoke B's already accepted local evidence implicitly.
