# Capability source synthesis

GENESIS can turn a missing venue-extraction requirement and public HTML examples
into a parser candidate. The forge records a generation intent, calls the
Anthropic Messages API once, saves the exact parser and response metadata, and
packages that source for the existing Apify build and independent verification
pipeline. A generated candidate does not activate a role, publish a capability,
or establish that any venue data is correct.

This is the capability-evolution half of a future planner action. Claude retains
the planner, role activation, Elastic and UI integration. The generator receives
no verification suite, expected answers, Apify checkpoint, filesystem access or
tool definitions. The caller must select genuinely public examples: placing
private data in the request would send it to the model.

## API and model configuration

The initial adapter uses the official [Messages API](https://platform.claude.com/docs/en/api/messages/create):
`POST https://api.anthropic.com/v1/messages`, `x-api-key`, the pinned
`anthropic-version: 2023-06-01` protocol, explicit model/max_tokens, a system prompt
and one user message. Only one complete text block ending with `end_turn` and
containing the strict JSON parser result is accepted. Tool calls, refusals,
incomplete output, redirects and malformed or oversized responses fail closed.
The API key is environment-only; provider error bodies are discarded.

Choose an available **concrete model ID** in your account. The returned model must
equal the requested ID, so aliases that resolve to a different model are rejected.
There is no default model or silent fallback. Token counts are usage evidence,
not a dollar-cost estimate or an account-wide spending limit.

No paid model call or cloud Actor creation was performed while implementing this
path. Loopback tests exercise the actual HTTP client and command-line handoff.
The supplied examples are synthetic public fixtures, not live venue availability.

## Run

Requirements: the existing Rust toolchain, a private writable runtime directory,
and a model ID/key for the explicit generation step. Prepare is offline:

```sh
mkdir -p runtime
# Create runtime/model.json with your concrete model ID, for example:
# {"model":"YOUR_CONCRETE_MODEL_ID","max_tokens":8192}

cargo run --locked -p genesis-forge -- generate-prepare \
  --request crates/genesis-forge/examples/synthesis/public-request.json \
  --model-config runtime/model.json \
  --checkpoint runtime/generation.json

cargo run --locked -p genesis-forge -- generate-status \
  --checkpoint runtime/generation.json
```

The public request binds an operation ID, capability/version, Actor name/source
version, requirement, explicit allowed source URLs and one to eight examples.
Every example URL must be allowlisted. Each HTML example is limited to 128 KiB;
the serialized request is limited to 512 KiB. The prompt contains the fixed
TypeScript extraction contract and treats example contents as data. Prompt rules
help guide the model; they do not prove generated code follows them.

Supply `ANTHROPIC_API_KEY` through your normal environment/secrets mechanism:

```sh
cargo run --locked -p genesis-forge -- generate-step \
  --checkpoint runtime/generation.json
```

This saves and fsyncs `requesting` before sending the request. It makes at most
one POST, has a 120-second timeout and 1 MiB response limit, and never automatically
retries. It emits compact status without examples or source. The checkpoint
contains source and prompts and uses private file permissions. Repeated steps on
completed generation reuse the saved result without another model call. An
interrupted pending request becomes `needs_reconciliation` without using
credentials or sending anything. Failure is terminal for that operation; inspect
the account outcome before intentionally preparing a fresh operation.

After `phase: generated`, prepare the existing forge **offline**:

```sh
cargo run --locked -p genesis-forge -- prepare-generated \
  --candidate crates/genesis-forge/examples/synthesis/candidate.json \
  --generation runtime/generation.json \
  --suite crates/genesis-verification/examples/fixtures/trusted/suite.json \
  --checkpoint runtime/forge.json
```

Candidate fields must exactly match the saved request; every suite URL must be
within its public allowlist. The suite does not rewrite that allowlist. The exact
parser, fixed production scaffold/lockfile and `genesis-synthesis.json` provenance
become one package. Provenance links model message, usage, request digest, parser
digest and scaffold source digest, without example HTML or expected answers.
The resulting ForgeSession freezes the complete source-package digest and identity.

The `.example` URLs are offline placeholders. For an authenticated Apify run,
first host the public fixture pages and consistently update the request, allowed
URLs and independent suite before preparing either checkpoint. Then follow
[FORGE_INTEGRATION.md](FORGE_INTEGRATION.md) for `step` and `export`. Do not run
generated TypeScript directly on the host to inspect whether it works; compilation
and execution belong in the separately controlled Actor build/runtime.

## Library and planner handoff

`SynthesisSession` owns the immutable public request and generation state.
`prompt()` exposes the bounded prompt; `begin()` marks the one allowed attempt.
Persist that state before invoking `AnthropicGenerator::generate`, then call
`complete` and persist the result. `complete(Err(provider_error))` records terminal
failure/reconciliation and returns `Ok(())` so callers save it. A state/receipt
mismatch returns an error without accepting a foreign completion. On recovery,
call `recover_interrupted` before considering any provider request.

`genesis_forge::generated_candidate(&session)` packages only a validated completed
generation. Pass its `CandidateSpec` to `ForgeSession::new` with an independent
suite, persist actions before effects, and drive build/run/verification. This
helper has no oracle or organization authority; the backend must check suite
coverage, live assignment/grant/lease and budget before effects. The CLI additionally
checks suite URL coverage during `prepare-generated`.

Claude's current planner proposes organization changes but has no acquisition
action yet. Integration should persist a capability-acquisition intent linked to
the mission's new specialist responsibility, call this path outside the kernel
lock, and advance on independently verified artifact evidence. The prompt is
exposed for future planner-backend integration; the initial receipt adapter is
specific to Messages API metadata. Reuse the existing forge repair lineage for
revised candidates; this change does not automatically retry generation or feed
oracle answers to a model for repair.

## Trust and evidence limits

- Checkpoint hashes detect accidental drift and mismatched completions. They are
  not provider signatures and cannot protect against an attacker who can rewrite
  trusted storage and recompute them.
- Generation binds the parser and contract prompt. Packaging uses the currently
  embedded scaffold. Exact whole-package reproducibility begins at ForgeSession
  creation; an upgraded forge binary can package different scaffold bytes.
- A model response is untrusted source. The Actor runs it in Node; the scaffold's
  URL checks are not arbitrary-code containment.
- Independent verification needs held-out cases and source variations. Passing
  the supplied public samples alone does not establish generalization.
- Organization B still needs its own acceptance runs for the pinned build.
  Model provenance does not replace publication, compatibility or installation.
- Account credentials, model output quality, live Actor builds/runs and mission
  integration remain live demonstration gates.

Focused checks: `cargo test --locked -p genesis-forge`. Tests cover state/receipt
substitution, HTTP bounds and sanitized failures, incomplete/ambiguous responses,
recovery without duplicate requests, private files, metadata/allowlist binding,
exact source packaging, and omission of the oracle from model input.
