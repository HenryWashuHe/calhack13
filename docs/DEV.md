# Developing the kernel and console

This covers the Rust organization kernel (`crates/`) and the React console (`apps/web/`). Provider integrations have their own guides; the table below distinguishes local evidence from outstanding live gates.

![Console showing Org A after relocation and handoff](img/console-org-a.png)

## Run it

You need Rust 1.94+ and Node 20+.

```bash
# Terminal 1: registry (7100) + Org A (7101) + Org B (7102), with dev stubs
cargo run -p genesis-server -- dev

# Terminal 2: the console on http://127.0.0.1:5173
cd apps/web && npm install && npm run dev

# Terminal 3 (optional): drive the full A -> B cycle against the dev stubs
node scripts/dev-scenario.mjs --delay 1200
```

State lives in `data/dev/*.db` (git-ignored). Delete that folder for a clean run.

## Integration status

| Integration | Guide | Status |
| --- | --- | --- |
| Elastic: evidence projection/search, capability search, Agent Builder specialists and jobs | [ELASTIC.md](ELASTIC.md), keys: [ELASTIC_SETUP.md](ELASTIC_SETUP.md) | Merged (#11); fake-tested. Run `genesis-elastic-smoke` against a real project |
| Mission planner (configured model, or an Elastic Agent Builder agent) | [PLANNER.md](PLANNER.md) | Merged (#12, #22); replay- and fake-tested; requires model configuration |
| CREAO asynchronous roles and jobs | [CREAO_RUNTIME.md](CREAO_RUNTIME.md), [ASYNC_PROVISIONING.md](ASYNC_PROVISIONING.md) | Kernel hook and env registration merged (#20). Gateway readiness (#26), operator attach (#27) and the CREAO readiness probe (#29) are open. Scripted-provider tests only; no live run |
| Apify capability acquisition, execution, acceptance and adoption | [CAPABILITY_EXECUTION.md](CAPABILITY_EXECUTION.md) | Merged; loopback-fixture tested |
| Fetch recruitment | [FETCH_RUNTIME.md](FETCH_RUNTIME.md) | Local multi-process proof |
| Console: capability evidence, Attention panel and failure drills | [CAPABILITY_EXECUTION.md](CAPABILITY_EXECUTION.md), [PR_REVIEW_QUEUE.md](PR_REVIEW_QUEUE.md) | Capability evidence merged (#21, integration #55); Attention panel and failure drills (#28) remain open |

The evidence listed here is local or loopback; live sponsor execution remains
a separate gate. `genesis dev` enables labeled fallback stubs, but configured
Elastic and CREAO integrations can still attach from the environment. For a
fully stubbed demo, leave those provider configurations unset. Review status of open PRs:
[PR_REVIEW_QUEUE.md](PR_REVIEW_QUEUE.md).

**Dev stubs are not integrations.** With stubs on, any role whose provider has no adapter gets a binding with `provider: "dev_stub"` and an ID like `devstub-elastic-…`. The console labels these `DEV STUB · for elastic`, and the `role_provisioned` event carries `dev_stub: true`. Run an organization without `--dev-stub` and unconfigured providers fail provisioning instead.

## Tests

```bash
cargo test --workspace          # engine invariants, store, HTTP end-to-end, provider fakes
cargo clippy --workspace --all-targets
cd apps/web && npm test && npm run build
```

The HTTP tests in `crates/genesis-server/tests/api.rs` start a real registry and separate organization servers on random ports. They cover the saga, failed provisioning, revision conflicts, stale-epoch fencing, idempotent results, the publication bar, A → B inheritance with zero synthesis, an isolation canary, incompatible adoption, SSE replay and restart from SQLite.

## Layout

| Path | What it is |
|---|---|
| `crates/genesis-core` | Pure domain: IDs, lifecycle tables, `OrgState`, change vocabulary, `engine` (validate/commit/dispatch/fencing), capability manifests, events. No I/O. |
| `crates/genesis-providers` | `RoleProvisioner` and `CapabilityExecutor` traits, `DevStub`, `FailingProvisioner`, `ProviderSet`. |
| `crates/genesis-server` | Axum API, SQLite store, change saga, outbox worker, SSE, shared registry, `genesis` CLI. |
| `schemas/` | JSON Schemas exported from the Rust types. A test fails if they drift. |
| `apps/web` | Console. `src/lib/contracts.ts` is generated from `schemas/`. |
| `scripts/dev-scenario.mjs` | Dev-only driver for the demo cycle. |

## How a change flows

1. An agent posts `POST /api/v1/org-changes` with `client_request_id` and a proposal citing observation IDs and `expected_org_revision`.
2. The kernel validates it against current state (`genesis_core::engine::validate`). Invalid proposals are stored as `REJECTED` with error codes; a stale revision returns 409.
3. Valid changes go `VALIDATED → PROVISIONING`. For each `CreateRole`, the saga calls the provider's `RoleProvisioner`, checks readiness, and persists the returned binding on the change record before moving on. A restart resumes from there.
4. `READY → COMMITTED` happens in one SQLite transaction: new snapshot at revision + 1, events, and outbox items for the newly responsible roles. New roles are never visible as active before this.
5. Any provisioning failure ends in `FAILED` and leaves the organization untouched.

## Contracts for other workstreams

Regenerate after changing any Rust type that crosses a boundary:

```bash
cargo run -p genesis-server -- export-schemas
cd apps/web && npm run gen:types
```

| Who | Calls | Token |
|---|---|---|
| Elastic mission agent | `POST /missions/{id}/observations`, `POST /missions/{id}/requirements`, `POST /org-changes`, `GET /organization` | agent |
| Elastic and CREAO specialists | Trusted backend provisioning/runtime and job collection; caller result POSTs cannot complete their jobs | backend only |
| Other caller-managed adapters | Report `POST /jobs/{id}/results` for jobs outside managed provider/capability paths | adapter |
| Fetch bridge | `POST /contracts/{id}/state` with `sender_address` (must match the contracted specialist) | adapter |
| Apify acquisition / verifier | `POST /capability-acquisitions`; publish through `POST /capabilities/publish` with a trusted forge or acquisition verification reference; the backend recomputes the evidence | operator |
| Org B | `POST /capability-adoptions` for durable discovery and independent acceptance; `POST /capability-runs` for an accepted, attached capability | operator |

Caller-managed job results must carry the job's `attempt_id` and `epoch`. A result with an old epoch is archived as `RESULT_REJECTED` (409 `stale_epoch`). Provider success with `domain_verified: false` leaves the job in `VERIFYING`. Repeating a result with the same `operation_id` produces one ledger row. Backend-managed capability jobs and Elastic/CREAO specialist jobs instead use their trusted collection paths; generic result submissions are rejected.

### Plugging in a real adapter

```rust
let providers = ProviderSet::new()
    .with_provisioner(Provider::Elastic, Arc::new(ElasticAdapter::new(cfg)))
    .with_dev_stub(DevStub::default()); // optional fallback for the rest
let app = genesis_server::build_org(&org_config, Some(providers))?;
```

## Runtime behavior and limits

- Registry search uses configured Elastic retrieval resolved against canonical manifests; missing or failed Elastic retrieval falls back to keyword ranking.
- Outbox items for providers without an adapter stay pending with `last_error` (visible at `GET /api/v1/outbox`) unless dev stubs are on.
- Capability runs use durable backend submission/collection and semantic finalization. Their evidence never enters through generic `/jobs/{id}/results`; see [CAPABILITY_EXECUTION.md](CAPABILITY_EXECUTION.md).
- Tokens are static bearer strings from env vars. Fine for the demo, not for production.
- SQLite access is serialized behind one mutex per organization. Fine at demo scale.
