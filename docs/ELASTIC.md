# Elastic integration

Status: implemented and tested against in-process fakes of the Elasticsearch
REST subset and the Kibana Agent Builder API. **No live Elastic project has
been called yet.** The live smoke test below is still required.

## What it does

| Piece | Code | Behavior |
| --- | --- | --- |
| Evidence projection | `genesis-server/src/projection.rs`, `genesis-elastic::org_docs` | After commits, writes the organization's missions, observations, requirements, responsibilities and roles to its own index `genesis-evidence-<org>`. Deterministic document IDs; strict mappings; retries with backoff. Never blocks or rolls back a kernel change. |
| Evidence search | `GET /api/v1/search/evidence?q=&kinds=&mission_id=&limit=` | Operator/agent tokens. Searches only this organization's index and returns `projected_revision` with the hits. Hits are candidates: re-read the authoritative record before acting. |
| Projection status | `GET /api/v1/projection` | `configured`, `projected_revision`, `documents`, sanitized `last_error`. |
| Capability retrieval | `ElasticClient::{index_capabilities, search_capabilities}` | Shared `genesis-capabilities` index; only `PUBLISHED` manifests whose required permissions the searcher can grant. **Not yet wired to the registry** (registry publish/search changes wait for PR #5 to merge). |
| Specialist provisioning | `genesis-elastic::AgentBuilder` (a `RoleProvisioner`) | A kernel-approved role with `provider: elastic` becomes an Agent Builder agent. Its only tool is an `index_search` tool scoped to `genesis-evidence-<org>`. IDs derive from org, change and role key, so retries and lost responses reconcile by `GET` instead of creating a second agent; an existing agent with a different configuration is rejected. Readiness = agent exists and still has the evidence tool. |
| Conversation | `AgentBuilder::converse` | Synchronous `POST /api/agent_builder/converse`. A round waiting for a human-in-the-loop prompt is reported as rejected; GENESIS never answers provider prompts. Not yet called by job dispatch. |

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

`genesis org` picks these up through `attach_elastic_from_env`. Either half can
be configured alone.

## Live smoke test (still to do)

1. With both keys set, start one organization and create a mission and an
   observation; `GET /api/v1/projection` should report the current revision.
2. `GET /api/v1/search/evidence?q=<words from the observation>` returns it.
3. Commit a change with a `provider: elastic` role; the role's binding shows
   the Agent Builder agent ID, and the agent and tool exist in Kibana.
4. Converse with the agent and confirm it called the evidence tool.
5. Confirm Org B's key cannot read `genesis-evidence-org_a`.

Unverified assumptions to check during the smoke test: `index_search` accepts
`row_limit` on Serverless; Kibana answers an existing ID with 409 (the client
also reconciles on 400); the `flattened` field type is available.

Sources: [Agent Builder API tutorial](https://www.elastic.co/docs/explore-analyze/ai-features/agent-builder/agent-builder-api-tutorial),
[Kibana API overview](https://www.elastic.co/docs/explore-analyze/ai-features/agent-builder/kibana-api),
[index search tools](https://www.elastic.co/docs/explore-analyze/ai-features/agent-builder/tools/index-search-tools).
