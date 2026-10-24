# Elastic integration

Status: implemented and tested against in-process fakes of the Elasticsearch
REST subset and the Kibana Agent Builder API. **No live Elastic project has
been called yet.** The live smoke test below is still required.

## What it does

| Piece | Code | Behavior |
| --- | --- | --- |
| Evidence projection | `genesis-server/src/projection.rs`, `genesis-elastic::org_docs` | After commits, writes the organization's missions, observations, requirements, responsibilities and roles to its own index `genesis-evidence-<org-namespace>`, where the namespace is a readable prefix plus a digest of the exact organization ID (so `Org_A` and `org_a` never share an index). Projection and Agent Builder use the same mapping (`genesis_elastic::org_namespace`). Deterministic document IDs; strict mappings; retries with backoff. Never blocks or rolls back a kernel change. |
| Evidence search | `GET /api/v1/search/evidence?q=&kinds=&mission_id=&limit=` | Operator/agent tokens. Searches only this organization's index and returns `projected_revision` with the hits. Hits are candidates: re-read the authoritative record before acting. |
| Projection status | `GET /api/v1/projection` | `configured`, `projected_revision`, `documents`, sanitized `last_error`. |
| Capability retrieval | `genesis-server/src/registry.rs` with `ElasticClient::{index_capabilities, search_capabilities}` | The registry projects each published manifest into the shared `genesis-capabilities` index (best effort; the SQLite store stays authoritative) and searches it, filtering to `PUBLISHED` manifests whose permissions the searcher can grant. Every hit is resolved against the canonical stored manifest, so an index entry without one is never a candidate. If Elastic fails, search falls back to keyword ranking and the next search re-projects all manifests. `genesis registry` and `genesis dev` enable it from the same environment variables. |
| Specialist provisioning | `genesis-elastic::AgentBuilder` (a `RoleProvisioner`) | A kernel-approved role with `provider: elastic` becomes an Agent Builder agent. Its only tool is an `index_search` tool scoped to the organization's evidence index. IDs derive from org, change and role key. A lost or timed-out create is reconciled by bounded `GET`s, never a second POST. An existing agent is adopted only if its effective configuration (name, description, labels, instructions and the exact tool set) matches; extra tools are rejected. Readiness requires the agent's effective configuration to match the binding digest, its only tool to be this organization's evidence tool, and that tool to be `index_search` over this organization's index. |
| Specialist jobs | `genesis-server/src/elastic_jobs.rs` (outbox `job_dispatch` for provider `elastic`) | One synchronous converse turn with the role's agent. Live authority (active role, open responsibility, current epoch, deadline) is checked before the call and again before the answer is applied; a late answer becomes `RESULT_REJECTED`. The answer is stored with `domain_verified: false` and the job moves to `VERIFYING`, never `SUCCEEDED`; no commitment is recorded and proposals in the answer are not executed. A round waiting for a human-in-the-loop prompt fails the job; GENESIS never answers provider prompts. |

The agent's instructions say it cannot change the organization and must
return proposals as JSON for the kernel to validate. Instructions are not an
authorization boundary: the kernel is.

## Configuration

Put these in `.env` (git-ignored), never in code or chat:

```
ELASTICSEARCH_URL=https://<project>.es.<region>.elastic.cloud
ELASTIC_DATA_API_KEY=<encoded key: read/write on genesis-* indices only>
ELASTIC_KIBANA_URL=https://<project>.kb.<region>.elastic.cloud
ELASTIC_MANAGEMENT_API_KEY=<encoded key: Agent Builder management>
# optional
ELASTIC_KIBANA_SPACE=genesis-demo
GENESIS_ELASTIC_INDEX_PREFIX=genesis
```

`genesis org`, `genesis registry` and `genesis dev` read these from the process
environment (load a `.env` with `set -a; source .env; set +a`); the smoke-test
binary and the planner also read `.env` directly. Either half can be
configured alone.

## Live smoke test (still to do)

With the four variables set (in `.env` or the shell), run:

```sh
cargo run -p genesis-elastic --bin genesis-elastic-smoke -- --org genesis-smoke --converse --cleanup --report smoke.json
```

It indexes and searches a unique evidence document and capability document,
creates an Agent Builder specialist, re-provisions it to confirm
reconciliation (no duplicate), checks readiness, holds one converse turn and
confirms the agent called its evidence tool, then deletes the agent. It prints
`PASS`/`FAIL` per step with IDs (never keys) and exits non-zero on failure.
The same binary passes against the in-process fakes
(`tests/smoke_binary.rs`). After it passes, also check in the console that a
`provider: elastic` role shows the Agent Builder agent ID, and confirm that
Org B's key cannot read Org A's evidence index.

Unverified assumptions to check during the smoke test: `index_search` accepts
`row_limit` on Serverless; Kibana answers an existing ID with 409 (the client
also reconciles on 400); the `flattened` field type is available.

Sources: [Agent Builder API tutorial](https://www.elastic.co/docs/explore-analyze/ai-features/agent-builder/agent-builder-api-tutorial),
[Kibana API overview](https://www.elastic.co/docs/explore-analyze/ai-features/agent-builder/kibana-api),
[index search tools](https://www.elastic.co/docs/explore-analyze/ai-features/agent-builder/tools/index-search-tools).
