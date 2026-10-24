# Acquisition and inheritance across two organizations

`genesis scenario` connects the public organization APIs into one operator-directed
mission. The driver records a portal requirement and commits a specialist
responsibility. Organization A generates, builds and verifies a capability. In automatic
mode, Organization B receives a durable adoption intent bound to its own requirement,
discovers a foreign capability, collects its own acceptance evidence and uses it.
Both temporary roles then retire with explicit responsibility handoffs. B's successor uses the
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
The fixture runs both organizations through the actual `serve_org` workers, behind
recording HTTP proxies that reject every `/step` and `/verify` POST, as well as the
legacy search, installation, checks and acceptance writes. Automatic adoption uses
one intent creation request and status reads. The full scenario test requires one
semantic repair (two model calls, Actors and builds; six A verification runs),
three B acceptance checks, initial and successor invocations, then a new successor invocation after reopening B's SQLite database.
Its six B runs share the repaired artifact. B has no acquisition or model client;
recorded provider traffic contains no B model, Actor-create or build calls. Both
commitment ledgers remain empty. Acceptance jobs remain `VERIFYING`,
while fresh invocation jobs become `SUCCEEDED` only after backend semantic verification.

Negative fixtures confirm incorrect fresh data stops the scenario before handoffs,
an ambiguous Actor creation remains unreplayed after restarting A, and disabled
automation is refused before mission creation. The report is checked for private
source, oracle and credential leakage. These tests establish automatic progression
through the real Rust server workers with canned HTTP provider replies; they do not
establish live-provider execution or native sponsor role activation. Canned driver
HTTP tests additionally require adoption attention, polling exhaustion and mismatched
accepted bindings to stop with the intent ID, without legacy fallback or another
creation request.

## Run against configured organization servers

Start two **fresh** organization databases and a registry. Both organizations need
registry access, their own operator credentials and an Apify executor. Only A enables
`--acquisition-policy` and configures a model client. For the recommended automatic
example, A also enables `--auto-acquire`; this requires both its model and Apify
clients to be configured. Both load trusted oracle suites through `--oracle-suite`;
B's suite must contain the exact intended invocation input.
See [SERVER_ACQUISITION.md](SERVER_ACQUISITION.md) and
[CAPABILITY_EXECUTION.md](CAPABILITY_EXECUTION.md) for server setup, and
[CAPABILITY_ADOPTION.md](CAPABILITY_ADOPTION.md) for B's durable adoption contract.

Copy `crates/genesis-server/examples/scenario/mission.json` into your runtime folder.
It contains public examples and an invocation input, never expected oracle answers.
The example sets `"progression": "automatic"`. The optional `progression` field
accepts `"manual"` or `"automatic"`; omitting it preserves manual progression for
existing specs. Set `"progression": "manual"` to have the driver issue explicit
step and verification requests as before, and to use the legacy explicit
search/install/checks/acceptance sequence for B.

In automatic mode, run both organizations through `genesis org` or an embedding
that calls `serve_org`. The driver creates explicit authorized acquisition, adoption
and run intents, then polls acquisition automation, adoption and run state, finalization
status and persisted proofs. B's server derives discovery from its committed requirement,
selects a foreign registry candidate and owns the independent acceptance batch. The
driver never sends `/step`, `/verify`, legacy search, install, checks or acceptance
requests in automatic mode. It supplies neither A's capability identity nor a search
query to B's adoption request. Only after B selects and accepts a candidate does the
driver compare its artifact with A's publication to establish this scenario's inheritance.
A's `/api/v1/health` field `automatic_acquisition_enabled: true` confirms configuration
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

The successful JSON report records the selected progression mode, acquisition ID
and counters, B's adoption ID and selected publisher in automatic mode, exact
source/Actor/build pin, B's independently collected acceptance job IDs and reference,
initial and successor invocation IDs and proof references, responsibility owners and
actual provider bindings. B's zero-synthesis result is scoped to this scenario and
the server's disabled acquisition configuration. The deterministic fixture additionally
checks recorded provider calls; a published manifest is not evidence of zero generation.
The report is not an account-wide billing audit.
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
5. Create B's mission and responsibility. In automatic mode, submit one requirement-bound
   adoption intent and observe its durable state while B's server discovers, installs
   and independently verifies a candidate. Require the accepted installation, exact
   selected artifact and acceptance jobs to agree with that intent. In manual mode,
   keep the explicit registry search, installation, checks and acceptance requests.
6. Attach, invoke and require a fresh supported result with a persisted semantic
   proof. In automatic mode, server workers collect and finalize the invocation;
   the driver reads their status and proof. Retire A's temporary specialist with a
   handoff, then retire B's verifier with a handoff to a persistent monitor. Attach the
   accepted capability to the monitor and verify another invocation under its grant.
7. Confirm acceptance and the artifact pin survived retirement, and A incurred no
   additional acquisition effects during B's inheritance.

Every request, response and polling loop is bounded. Defaults allow up to 4,096
polls at 500 ms intervals and impose a 30-minute deadline on the entire scenario;
cheap observation GETs do not inherit the old 128-second effective polling window.
Redirects and automatic POST
retries are disabled. A driver timeout or disconnect does **not** cancel authorized
server work: acquisition, adoption, execution and finalization workers may continue
while their grants, deadlines and quotas permit. Inspect durable organization, acquisition,
automation, adoption, run, finalization and proof records before taking further action.
Adoption attention, rejection or fencing stops this driver without creating another
intent or falling back to manual requests. An unknown provider POST can still have
taken effect; a failed read also requires explicit operator attention. Neither
condition causes automatic resubmission or a replacement acceptance batch.
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
