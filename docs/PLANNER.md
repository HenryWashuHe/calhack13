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

## Files

| Path | Purpose |
|---|---|
| `src/llm.rs` | `LlmClient` trait; Anthropic, OpenAI-compatible and Replay clients; `from_env()` |
| `src/prompt.rs` | Domain-neutral system prompt and the state/evidence message |
| `src/output.rs` | Parse untrusted model JSON into typed `OrgOp`s; `$REQUIREMENT` placeholder |
| `src/agent.rs` | One planning pass with bounded repair and decision events |
| `src/runner.rs` | Polls the event log; plans each observation once, also after restarts |
| `tests/planner.rs` | End-to-end against an in-process kernel |
