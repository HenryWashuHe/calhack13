# Single-attempt method source generation (experimental)

The forge can generate one UTF-8 source file and package it with the host's exact
`MethodPackage` metadata. This fills the source-generation boundary of G1. It
neither runs the method nor scores, publishes or adopts it. G1 still requires a
working isolated executor, independently scored outputs and enforced run limits.
Fixtures are not live model calls, and generated source is not an improvement result.

## Host inputs

`MethodSynthesisPlan` fixes an operation and organization label, target id/version,
entrypoint, runtime reference, protocol document/reference, ordered dependencies,
public task document, public input documents, instruction and model settings.
Every document's reference hashes its exact UTF-8 bytes. The host supplies trusted
references and public classification; a hash does not establish that the material
is public or authentic. Organization labels are host assertions, not kernel grants.
Callers retain responsibility for organization authority, budgets and trusted storage.

Use the executor owner's selected runtime and IPC protocol. The protocol document
contains the exact bytes identified by that protocol reference; longer public
schema/examples can be separate `public_inputs`. This module defines no new method
ABI and does not substitute a generated runtime or dependency. The model returns
exactly `{"source":"..."}`; all package metadata remains host-controlled. Source
whitespace is preserved. The source is capped at 128 KiB, plans at 256 KiB encoded,
public documents at 128 KiB each, with at most sixteen extra public inputs.

## Settings and accounting

`settings` requires `model`, `max_tokens` (1–16384), `timeout_ms` (1–120000), and
`sampling: "provider_default"`. The adapter sends one non-streaming Anthropic
Messages request, API version `2023-06-01`, with no tools, thinking configuration
or sampling parameters. Unsupported sampling variants fail during deserialization.
This explicit provider-default policy **does not pin sampling behavior or prove
fixed model weights**, even when the returned model identifier matches. It is not
sufficient for G4 fixed-settings claims; select and review a supported explicit
sampling policy before a controlled comparison. No alias-to-version substitution
is accepted in the returned model field.

The request record hashes the exact submitted JSON bytes plus the prompt,
settings and API version. Completion evidence records the observed model/message
id, response hash, wall time and provider-reported input/output tokens. The actual
client labels its endpoint `anthropic_https` or `loopback_fixture`; restored pending
requests are `unobserved`. This is a trusted-host observation, not signed or
independently authenticated provider proof. Known usage
survives malformed source, truncation, tool output or a model mismatch. Missing or
ambiguous usage remains `Unknown`, never zero. Duplicate or malformed envelopes
are rejected and their ambiguous usage is unknown. Token counters describe the
provider's reported fields, not a bill or independent metering proof.

There is one attempted POST, a transport deadline and output-token ceiling;
there is no hard input-token ceiling beyond the prompt's byte bound. A generated
package can coexist with unknown usage and does not establish budget compliance.
Generation costs are separate from subsequent method-execution and scoring grants.
Total comparison accounting must explicitly include them; that integration is not
implemented here.

## Durable state and CLI

Prepare is offline and refuses to overwrite a checkpoint. It freezes the complete
request and its identity, including the exact system/user prompt and settings.
Restart uses those saved bytes, never a newly reconstructed prompt. Unsupported
output-contract/API identities fail closed instead of relabeling old intent.
Step commits `requesting`
with the existing exclusive checkpoint lock, file fsync, atomic replacement and
parent-directory fsync before the POST. Completion is then saved. A restored
`requesting` record becomes `needs_reconciliation` before credentials are read.
No failed, rejected, generated or uncertain attempt automatically resubmits.
Source-invalid completions remain `rejected_source` with their observed usage.
Checkpoints retain private inputs/source and require trusted, exclusive storage;
checksums detect corruption, not a writer who can recompute them.

```sh
cargo run -p genesis-forge -- method-generate-prepare --plan method-plan.json --checkpoint generation.json
cargo run -p genesis-forge -- method-generate-step --checkpoint generation.json
cargo run -p genesis-forge -- method-generate-status --checkpoint generation.json
cargo run -p genesis-forge -- method-generate-export --checkpoint generation.json > method-package.json
```

The caller provides environment variables; the CLI does not implicitly load `.env`.
If any `GENESIS_LLM_PROVIDER`, `GENESIS_LLM_MODEL` or `GENESIS_LLM_API_KEY` is present,
all are required: provider must be `anthropic` and model must exactly equal the
saved plan. The project key wins when both project and legacy keys exist. Only when
all three project variables are absent does the CLI use `ANTHROPIC_API_KEY`.
Configuration conflicts fail before intent or POST; no secret enters a checkpoint
or error. Production has no endpoint override. Loopback endpoints are library test
constructors only.

Export writes canonical package bytes with **no trailing newline**. A consumer
must use `MethodPackage::load_for` with its own frozen binding and trusted
runtime/protocol references. The saved session and `derivation_json()` link the
plan, exact request/settings, provider completion and output package. They are
source-generation evidence, not an `ExecutionReceipt`, authenticated lineage,
local acceptance or evidence of a completed A → B → A loop.

## Offline validation

Loopback tests cover exact wire bytes/settings, package transfer/loading,
malformed source with retained usage, missing usage, duplicate content fields,
foreign models/completions, truncation/tools, transport ambiguity and no retry.
CLI tests cover persisted intent before POST, restart recovery before credentials,
project-model conflicts, exclusive prepare and exact export without executing
source. Existing parser generation tests protect its unchanged request, receipt,
checkpoint and CLI contracts.
