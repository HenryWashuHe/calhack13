# Provider contracts

Documentation reviewed: October 21, 2026; CREAO execution contract updated October 25, 2026. These are implementation contracts derived from public documentation, **not evidence of authenticated integration tests**. Account access, latency, quotas, and provider response shapes still need the smoke checks below.

The Rust core uses Axum/Tokio and owns provider adapters, credentials, jobs, and organizational state. Reuse asynchronous Rust HTTP clients for Elastic, Apify, CREAO, and Agentverse search. A separate, small Python process hosts only the Fetch uAgents SDK and exchanges typed messages with the core through a private HTTP bridge. The React/TypeScript UI renders committed events. Generated Apify Actors use TypeScript in a stable build scaffold.

Use typed Rust provider, operation, and lifecycle enums, while preserving provider identifiers and raw JSON responses alongside normalized results. Unknown provider statuses or fields must remain inspectable; an unrecognized status cannot become a successful job. Validate provider output before constructing domain commands. HTTP adapters need explicit timeouts, bounded retries, per-provider/account concurrency and rate limits, and incremental SSE parsing where applicable. Rust types do not establish provider-side idempotency or guarantee domain correctness.

## Responsibilities and credentials

Configuration names below are GENESIS conventions, not provider-defined environment variables.

| Provider | Required responsibility | Backend configuration | Boundary |
|---|---|---|---|
| Fetch.ai | Discover, recruit, and message independently running specialists | `AGENTVERSE_API_KEY` for Rust discovery and SDK registration setup; a separate secret seed held by each Python uAgent process; private bridge authentication | Agentverse discovery uses `Authorization: Bearer <key>`. The bridge carries typed messages; a discovered address is a candidate, not an accepted contract. |
| Elastic Agent Builder on Serverless | Create internal specialists and skills; retrieve organization evidence and shared capabilities | `ELASTIC_KIBANA_URL`, `ELASTICSEARCH_URL`; separate management/execution encoded API keys | Agent Builder uses the Kibana URL and `Authorization: ApiKey <encoded-key>`; writes require `kbn-xsrf: true`. Data indexing uses the Elasticsearch URL. |
| Apify | Create, build, and execute missing web capabilities | `APIFY_TOKEN` held only by the trusted backend | Actor creation/modification requires an unscoped forge token; scoped tokens cannot perform those operations. Never give the forge token to generated code or CREAO. |
| CREAO | Bind organizational roles to a published app and run specialized workflows | Server-side `CREAO_TRIGGER_API_KEY` (`capi_`), `CREAO_TRIGGER_APP_ID`; optional exact `CREAO_TRIGGER_RUNS_URL` | API Trigger only in production; no Developer fallback or cloud agent creation. Frozen role contracts accompany each job. |

The GENESIS kernel is the sole authority for roles, responsibilities, assignments, grants, leases, and retirement. Provider resources implement those decisions; they do not define the organization.

## Fetch.ai

| Operation | Documented interface | Adapter result |
|---|---|---|
| Discover | `POST https://agentverse.ai/v1/search/agents` | Candidates with addresses, advertised metadata, and discovery time |
| Register | `uagents_core.utils.registration.register_chat_agent(...)` with `RegistrationRequestCredentials` | Registered independent identity; its process must stay online |
| Communicate | `ctx.send(address, typed_message)` and matching `@agent.on_message(model=...)` handlers | Correlated application messages, not proof of completed work |

Search accepts `search_text`, `filters`, `sort`, `direction`, `offset`, and `limit`. Filters can select active uAgents and protocol compatibility. Rust calls this HTTP API directly. Only the Fetch sidecar uses Python 3.10–3.13 with a tested, pinned uAgents release compatible with the current `>=0.25.5` tutorials. Mailbox operation is documented; test its account registration and reachability before relying on it. [Search](https://innovationlab.fetch.ai/resources/docs/agentverse/searching), [registration](https://innovationlab.fetch.ai/resources/docs/agent-creation/uagent-creation), [communication](https://innovationlab.fetch.ai/resources/docs/agent-communication/uagent-uagent-communication)

GENESIS defines `RecruitRequest`, `Offer`, `ContractAccepted`, `TaskAccepted`, `TaskProgress`, `TaskResult`, and `ReleaseContract`. Both endpoints must share their schemas. Include organization, mission, task, attempt, contract, correlation, idempotency, and deadline identifiers. Validate the expected sender and live contract before accepting a result. Releasing Org A's contract must not stop a contractor serving Org B.

The private Rust–Python bridge is a GENESIS interface, not a Fetch endpoint. Its contract needs outbound message submission, inbound delivery with the actual transport sender identity, and sidecar health/identity readiness. Version its JSON envelopes and validate them at both ends. HTTP acceptance means the bridge accepted a message, not that a specialist accepted or completed work. The Rust core owns the durable outbox/inbox, retries, deduplication, contracts, and organizational authorization; the sidecar owns SDK identity and message transport. Authenticate the bridge, keep it private, and do not place Elastic, Apify, or CREAO credentials in it. Sidecar restarts must preserve agent identity and cannot create a second organizational authority.

## Elastic Agent Builder

All paths below are relative to the Serverless project's **Kibana URL**. For a non-default space, prepend `/s/{space_id}`.

| Operation | Documented path | Contract |
|---|---|---|
| Create specialist | `POST /api/agent_builder/agents` | Supply `id`, `name`, `description`, and `configuration`; persist returned identity and configuration hash |
| Inspect/change/remove | `GET`, `PUT`, `DELETE /api/agent_builder/agents/{id}` | Reconcile uncertain creates before retrying; deletion is optional cleanup |
| Create skill | `POST /api/agent_builder/skills` | `id`, `name`, `description`, `content`, `tool_ids`; bind through agent `configuration.skill_ids` |
| Create retrieval tool | `POST /api/agent_builder/tools` | MVP: `type: "index_search"`, `configuration: {"pattern": "<allowed-index-pattern>"}` |
| Invoke specialist | `POST /api/agent_builder/converse/async` | Supply `agent_id` and `input`; retain conversation ID and consume SSE through `round_complete` |
| Reconcile conversation | `GET /api/agent_builder/conversations/{conversation_id}` | Inspect persisted state after an interrupted stream; do not blindly replay side effects |

Runtime specialist creation is documented. **It is not the complete role lifecycle.** GENESIS must separately activate responsibility ownership, control subsequent dispatch, drain work, transfer obligations, and retire the role. Elastic's `subagent_ids` allowlist already supports subagent delegation; a changed call tree alone is not organizational evolution. [Agent creation](https://www.elastic.co/docs/api/doc/serverless/operation/operation-post-agent-builder-agents), [agent/skill/tool APIs](https://www.elastic.co/docs/explore-analyze/ai-features/agent-builder/kibana-api)

The MVP uses native retrieval plus schema-validated command proposals executed by the kernel. External MCP tools are supported, but their connector/import configuration is not yet a verified implementation contract. Keep experimental task-manager execution out of the critical path. SSE provides progress, not GENESIS scheduling or recovery semantics. Serialize turns within a conversation; parallel roles use separate conversations. [Streaming API](https://www.elastic.co/docs/api/doc/serverless/operation/operation-post-agent-builder-converse-async)

Separate management privileges from execution and index access. `feature_agentBuilder.all` grants component management but does not itself grant index writes. Index patterns and instructions are not authorization boundaries. Avoid interactive approval dependencies: subagents cannot collect prompt responses, and A2A cannot answer pending prompts or stream results. [API keys](https://www.elastic.co/docs/explore-analyze/ai-features/agent-builder/api-keys), [limitations](https://www.elastic.co/docs/explore-analyze/ai-features/agent-builder/limitations-known-issues)

## Apify

Paths are relative to `https://api.apify.com`.

| Operation | Documented path | Required retained evidence |
|---|---|---|
| Create Actor with source | `POST /v2/actors` | Actor ID and generated source digest |
| Add repaired version | `POST /v2/actors/{actorId}/versions` | Version number and revised source digest |
| Build | `POST /v2/actors/{actorId}/builds?version={version}&useCache=true` | Build ID and exact build number |
| Poll build | `GET /v2/actor-builds/{buildId}` | Terminal build status |
| Validate input | `POST /v2/actors/{actorId}/validate-input?build={build-number}` | Input-schema validation result |
| Execute pinned artifact | `POST /v2/actors/{actorId}/runs?build={build-number}` | Run ID; never substitute `latest` |
| Poll run | `GET /v2/actor-runs/{runId}` | Terminal run status and default dataset ID |
| Read output | `GET /v2/datasets/{datasetId}/items?format=json` | Dataset output and independent acceptance report |

Creation supports `versions` containing `versionNumber`, `sourceType: "SOURCE_FILES"`, and `sourceFiles`. Package generated TypeScript with the tested compilation/build scaffold and input schema. Input validation checks shape, not extraction accuracy or mission success. Accept the capability only after independent held-out checks pass. [Create Actor](https://docs.apify.com/api/v2/actors-post), [build](https://docs.apify.com/api/v2/actors-builds-post), [run](https://docs.apify.com/api/v2/actors-runs-post), [input validation](https://docs.apify.com/api/v2/actor-validate-input-post)

Request `forcePermissionLevel=LIMITED_PERMISSIONS` for generated runs and explicit runtime, memory, and cost limits. This is not a complete arbitrary-code sandbox or outbound-network policy. Use public synthetic inputs and no sensitive runtime credentials. Build caches and `waitForFinish` do not guarantee completion inside the presentation window. [Token scopes](https://docs.apify.com/integrations/api), [Actor permissions](https://docs.apify.com/actors/development/permissions), [build behavior](https://docs.apify.com/actors/development/builds-and-runs/builds)

## CREAO

Production uses app-scoped [API Trigger](https://docs.creao.ai/pro/api-trigger):

| Operation | Documented path | Contract |
|---|---|---|
| Bootstrap a local role / execute its job | `POST https://agent.creao.ai/api/v1/apps/{appId}/runs` | App-scoped Bearer key and `{"inputs":{...}}`; retain the exact app/run IDs |
| Poll execution | `GET https://agent.creao.ai/api/v1/apps/{appId}/runs/{runId}` | Require exact retained identity, terminal completion and independent output validation |

API Trigger does not create/edit cloud agents, install skills or expose Developer
memory/agent APIs. Publish/configure the app first. The kernel continues creating
organizational roles, with a durable bootstrap and separate gateway readiness.
Each job resends its frozen role contract, task and complete authority context;
shared app identity is not physical isolation. Historical v1/v2 records and
explicit library fixtures are retained, but backend entry points refuse those
protocols and keys. See [runtime configuration and migration](CREAO_RUNTIME.md).

The runtime persists intent before one POST, resumes known runs by GET, refuses
ambiguous resubmission, and shares a conservative local quota/cooldown ledger.
Run completion and `result.text` do not prove a domain action or current authority.
Readiness requires a challenge-bound gateway callback; receipts require separate
domain validation. Read-only authentication, a paid run, readiness and sponsor
core-work evidence are different claims. The [API path comparison](https://docs.creao.ai/developer-api/api-paths)
explains the separate credentials and scopes.

## Shared lifecycle and independence

- Keep stable GENESIS IDs separate from provider IDs. Persist attempts, configuration hashes, raw receipts, deadlines, and correlation IDs.
- Provision using a durable staged operation: propose → provision → verify readiness → activate ownership. Retain the prior accountable owner until handoff commits.
- Role lifecycle: `PROPOSED → PROVISIONING → READY → ACTIVE → DRAINING → RETIRED`, with explicit provisioning failure. Retirement must affect future dispatch and outstanding obligations.
- Jobs distinguish accepted work from completed work. A timeout or dropped stream can yield `UNKNOWN`; do not label work cancelled without evidence that execution stopped.
- Retry transient errors with bounded backoff and provider rate-limit handling. Reconcile uncertain creation before retrying. Use durable outbox/inbox deduplication; do not claim exactly-once provider execution.
- Org A and Org B have separate processes, stores, credentials, conversations, grants, and task queues. Elastic holds searchable projections; the kernel store owns atomic organizational changes.
- The shared registry publishes immutable capability manifests, not private conversations or connector credentials. Org B performs local acceptance and creates its own installation and role bindings before dispatching the pinned artifact.
- Derive the organization from authenticated caller identity at the execution gateway; do not trust a caller-supplied `org_id`. Shared provider accounts and public synthetic fixtures demonstrate the flow, not production tenant isolation.

## Required smoke checks before integration claims

- [ ] Fetch: register two independent identities; verify Rust discovery and the Rust–Python bridge's typed offer/accept/task/result round trips; measure latency; reject a late or wrong-sender result. Restart the sidecar and verify identity preservation and inbox/outbox deduplication.
- [ ] Elastic: on Serverless, create a native retrieval tool, skill, and specialist; invoke it through SSE; verify effective bindings and separate-org index authorization. Reconcile a simulated lost creation response.
- [ ] Apify: create a new source-bearing TypeScript Actor, build it, execute its exact build number, retrieve output, and fail an intentionally incorrect candidate against the oracle. Record cold-build latency and limits.
- [ ] CREAO: verify app-scoped authentication/credits; bootstrap a local role and poll its published app; execute distinct jobs with frozen role contracts; verify gateway readiness and independently check outputs. Record genuine run share evidence.
- [ ] Cross-provider: record provider IDs proving a role and executable were absent before the requirement and created afterward. Activate ownership only after readiness checks.
- [ ] Independence: Org B discovers the shared manifest, runs local acceptance, and executes the pinned capability without synthesis or access to Org A's private state.
- [ ] Retirement/recovery: retire a role with unfinished work, verify handoff and no later dispatch, then replay duplicate events/results without duplicate effects.

For context and research rationale, see [the research report](../GENESIS-organization-evolution-research.md).
