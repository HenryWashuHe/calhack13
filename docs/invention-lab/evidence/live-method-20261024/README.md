# First live method-generation attempt: retained failure

On October 24, 2026, the operator invoked one admitted Anthropic generation
attempt on source `be72815321ce784c1d487dbf94b9c1521b72325d`.
Operation: `live-a-method-20261024t152819z-2a3e0b5a`.
The project key authenticated successfully; no credentials are in this bundle.

| Observation | Result |
| --- | --- |
| Endpoint/model reported by the adapter | `anthropic_https` / `claude-sonnet-5-5` |
| Terminal state | `failed`, completion reason `protocol` |
| Provider-reported calls/input/output | 1 / 3971 / 8192 tokens |
| Locally measured request wall time | 62131ms |
| Declared cap | 1 call, 8192 output tokens, 120000ms request timeout |
| Admission after completion | One reservation, zero remaining attempts |
| Generated package / execution / adoption | None / not performed / not performed |

The output allowance was fully used. The adapter did not retain the response's
stop/content diagnostics or raw response, so truncation is **not a proven cause**.
The response hash cannot reconstruct those omitted fields. There was no retry,
refund or manufactured source package. Generation costs include this failed call.
The input grant of 32768 tokens and aggregate wall grant are accounting limits;
only the participating request/output limits and transport timeout bound this path.

These are operator-observed cloud records plus independently checked local
consistency, not provider-signed evidence. Hashes do not authenticate the provider
or chronology. `provider_default` sampling does not establish fixed-settings G4
comparison. G1 still lacks genuine generated-method execution; G0–G4 remain open.

The plan uses the existing executor's public `ravine-crossing` task, protocol and
design-language descriptions. An added instruction requests flushing/reply handling,
finite experiment count, and no network/subprocess/external file access. Those are
prompt instructions, not execution enforcement. No handwritten solution, hidden
case or secret was submitted in the prompt.

## Inspect without another provider request

From this directory, with the main-workspace `genesis-forge` binary on PATH:

```sh
shasum -a 256 -c SHA256SUMS
genesis-forge method-generate-status --checkpoint generation.json
genesis-forge method-admission-status --admission admission.json
genesis-forge method-roster-assess --manifest snapshots.json --roster roster.json
```

All three commands are read-only. The last reconstructs the original roster's
cost assessment, including this failed attempt. `final-status.json` is the retained
status output. The original Prepared and final checkpoint/journal bytes are copied
unchanged; public documents and complete submitted request are embedded in them.
The plan also pins this host's Python runtime, which a later receiver must match.
Never reset or replay this operation. This directory is historical evidence, not
a live operating journal or a runnable method package.

Next: add allowlisted stop/rejection diagnostics while preserving old checkpoints,
then declare a fresh bounded attempt with its own evidence. Henry retains runtime,
scoring and adoption ownership; there is no executable artifact to hand off yet.
