# Mission planner

The planner is the organization's decision-maker. When new evidence arrives it asks a language model whether the organization must change, and submits a structured proposal to the kernel with the restricted **agent** token. The kernel validates and decides. The planner can never commit a change on its own.

```
observation_recorded ──► planner reads mission, roles, responsibilities,
                         constitution, installed capabilities, the evidence
                    ──► model returns JSON: assessment, verdict, requirement, operations
                    ──► planner records the requirement, fills revision + evidence IDs,
                        submits POST /org-changes
                    ──► kernel accepts  → saga provisions and commits
                        kernel rejects  → exact error codes go back to the model (max 3 calls)
                    ──► planner_decision event in the evidence log either way
```

## Status

- Model choice is **pending**. The code supports Anthropic, OpenAI, Gemini (OpenAI-compatible endpoint) and any OpenAI-compatible server, selected by environment variables. Elastic Agent Builder can be added as another `LlmClient` implementation.
- Tests use `ReplayClient`, which returns scripted replies and records itself as model `replay`. The kernel, validation and saga in those tests are real.
- External specialists come from a JSON file (`--specialists`). Until Fetch discovery is wired, the list is empty and the prompt forbids `RecruitSpecialist`.

## Run

```bash
cp .env.example .env            # then fill in GENESIS_LLM_* once a model is chosen
cargo run -p genesis-server -- dev                      # kernel stack
cargo run -p genesis-planner -- run                     # watch Org A and plan
cargo run -p genesis-planner -- prompt --observation obs_…   # see the exact prompt; works with no model
cargo run -p genesis-planner -- once --observation obs_…     # plan a single observation
```

With no model configured, `run` and `once` exit with a clear message; `prompt` still works.

## Why the result is not hard-coded

- The system prompt (`crates/genesis-planner/src/prompt.rs`) describes only the mutation vocabulary and the TECH_SPEC §5 decision rules. A test fails if scenario words such as "venue" or "relocation" appear in it.
- Role names, charters, scopes and responsibilities come from the model's reading of the evidence and state.
- The model is told to prefer no change, then reusing an existing role, then attaching a capability, then recruiting, and only then creating a role with a stated justification.
- The model never supplies the expected revision or evidence IDs; the planner fills them from the kernel. Unknown IDs, scopes outside the constitution, budget overruns and missing owners are rejected by the kernel, and those exact errors are what the model sees on its next attempt.

For the demo, show a second disruption that should **not** create a role (for example a capacity change that the existing monitoring role owns). Two different pieces of evidence producing two different organizational responses is the evidence that the planner reasons rather than follows a script.

## Elastic Agent Builder as the planner

Set `GENESIS_LLM_PROVIDER=elastic` (with `ELASTIC_KIBANA_URL` and
`ELASTIC_MANAGEMENT_API_KEY`) to run the planner's model as the
organization's own mission-planner agent on Elastic Agent Builder
(`src/elastic.rs`). The agent's instructions are the planner's system prompt
plus a direction to consult the evidence first; its only tool is the
organization's `index_search` evidence tool (see [ELASTIC.md](ELASTIC.md)).
Each completion is one fresh conversation. The reply is parsed and validated
exactly like any other model's: the agent proposes, the kernel decides.
Changed instructions create a new agent (its ID includes their digest)
instead of mutating an existing one. Provider error text never reaches
decisions. Test: `tests/elastic_planner.rs` (fake Kibana, real kernel).

## Recovery and trust

- **No duplicate effects after lost responses or restarts.** A requirement is
  reused if one was already recorded from the same observation. Each change
  submission uses a deterministic request ID (`planner:<observation>:a<attempt>:s<n>`,
  independent of the revision). After an ambiguous kernel response (transport
  error or 5xx) the planner reconciles by reading
  (`GET /api/v1/org-changes/requests/{id}`, the organization state) and never
  resubmits an unknown outcome. On start, a planner that finds an accepted
  change for the observation records the decision without a new model call.
  Only a definite 4xx rejection enters model repair.
- **Decisions are idempotent.** The kernel records at most one terminal
  planner decision per observation; a retried decision returns the existing one.
- **Model HTTP.** Model clients never follow redirects (a redirect would resend
  the API key and prompt to another origin) and do not retry. Provider error
  bodies are reduced to the HTTP status and a short error type; their text
  never reaches errors, logs or `planner_decision` events.

Regression tests: `tests/review_recovery.rs`, `tests/review_http.rs` (from the
Codex review of PR #12) and the restart test in `tests/planner.rs`.

## Closed missions

Evidence for a mission that is no longer `Active` (succeeded or failed) or is
past its deadline is recorded, but not planned: the planner records a
`no_change` decision with the reason and makes no model call, so the runner
treats it as decided. A change already accepted by an interrupted run is still
recovered first. This covers late portal or operator evidence and the
kernel's own note after a capability gap closes (#57 also stops recording that
note for closed missions).

## Files

| Path | Purpose |
|---|---|
| `src/llm.rs` | `LlmClient` trait; Anthropic, OpenAI-compatible and Replay clients; `from_env()` |
| `src/prompt.rs` | Domain-neutral system prompt and the state/evidence message |
| `src/output.rs` | Parse untrusted model JSON into typed `OrgOp`s; `$REQUIREMENT` placeholder |
| `src/agent.rs` | One planning pass with bounded repair and decision events |
| `src/runner.rs` | Polls the event log; plans each observation once, also after restarts |
| `tests/planner.rs` | End-to-end against an in-process kernel |
