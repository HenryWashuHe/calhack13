# Elastic project and API keys

Steps to give GENESIS a real Elastic Serverless project. Keys go in a
git-ignored `.env`; never commit or paste them into chat. On Serverless,
create keys in **Console** or the **API keys** UI (username/password
authentication is not supported for the curl form).

## 1. Names to grant

Index and tool names include a digest of the exact organization ID. Print them
offline (no keys or network needed):

```sh
cargo run -p genesis-elastic --bin genesis-elastic-smoke -- --names --org org_a
cargo run -p genesis-elastic --bin genesis-elastic-smoke -- --names --org org_b
```

Each prints `evidence_index` (one per organization), `capability_index`
(shared, `genesis-capabilities`) and `evidence_tool`.

## 2. Keys

Use one data key and one Agent Builder key **per organization**, so Org B's
keys cannot read Org A's evidence. Agent Builder tools run with the index
privileges of the key that calls converse, so the Agent Builder key's index
grant is the isolation boundary. Replace `<ORG_A_EVIDENCE_INDEX>` with the
printed name.

**Org A data key** (`ELASTIC_DATA_API_KEY` for `genesis org --org-id org_a`):
writes its evidence projection and reads shared capabilities.

```
POST /_security/api_key
{
  "name": "genesis-org-a-data",
  "expiration": "30d",
  "role_descriptors": {
    "genesis-org-a-data": {
      "indices": [
        { "names": ["<ORG_A_EVIDENCE_INDEX>"],
          "privileges": ["create_index", "write", "read", "view_index_metadata"] },
        { "names": ["genesis-capabilities"],
          "privileges": ["read", "view_index_metadata"] }
      ]
    }
  }
}
```

**Registry data key** (`ELASTIC_DATA_API_KEY` for `genesis registry`): the only
writer of the shared capability index.

```
POST /_security/api_key
{
  "name": "genesis-registry-data",
  "expiration": "30d",
  "role_descriptors": {
    "genesis-registry-data": {
      "indices": [
        { "names": ["genesis-capabilities"],
          "privileges": ["create_index", "write", "read", "view_index_metadata"] }
      ]
    }
  }
}
```

**Org A Agent Builder key** (`ELASTIC_MANAGEMENT_API_KEY`): manages agents and
tools; index access read-only and limited to Org A's evidence (adapted from
Elastic's management-key example).

```
POST /_security/api_key
{
  "name": "genesis-org-a-agent-builder",
  "expiration": "30d",
  "role_descriptors": {
    "genesis-org-a-agent-builder": {
      "indices": [
        { "names": ["<ORG_A_EVIDENCE_INDEX>"],
          "privileges": ["read", "view_index_metadata"] }
      ],
      "applications": [
        { "application": "kibana-.kibana",
          "privileges": ["feature_agentBuilder.all"],
          "resources": ["space:default"] }
      ]
    }
  }
}
```

If the agents' model uses an inference endpoint or connector, add
`"cluster": ["monitor_inference"]` and/or `feature_actions.read` as Elastic's
guide describes. For a non-default space, change `space:default` and set
`ELASTIC_KIBANA_SPACE`. Repeat the org keys for Org B with its own names.

Use the **encoded** value returned by the create call (not the separate `id`
or `api_key`).

## 3. Environment and smoke test

```
ELASTICSEARCH_URL=https://<project>.es.<region>.elastic.cloud
ELASTIC_DATA_API_KEY=<org data key>
ELASTIC_KIBANA_URL=https://<project>.kb.<region>.elastic.cloud
ELASTIC_MANAGEMENT_API_KEY=<org agent builder key>
```

```sh
cargo run -p genesis-elastic --bin genesis-elastic-smoke -- --org org_a --skip-capabilities --converse --cleanup --report smoke-org-a.json
```

`--skip-capabilities` skips writing a capability document, which the
restricted org data key cannot do; run the smoke test once without it using
the registry data key to cover the shared index. Then confirm isolation: with Org B's
Agent Builder key, a converse turn asking for Org A's smoke token must not
return it.

Source: [Create API keys for Elastic Agent Builder](https://www.elastic.co/docs/explore-analyze/ai-features/agent-builder/api-keys).
