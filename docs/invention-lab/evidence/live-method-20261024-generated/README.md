# First live generated method package: source handoff

The third separately declared attempt on October 24, 2026 produced GENESIS's first
live model-generated method source package. Operation
`live-a-method-20261024t160859z-75cdf274` ran on source
`07c098ca56b1f87c29a0d9d0e12365be8519fd69` after the thinking-prefix parser fix.
The [first failure](../live-method-20261024/README.md) and
[second failure](../live-method-20261024-diagnostic/README.md) remain unchanged.

| Observation | Result |
| --- | --- |
| Adapter-reported endpoint/model | `anthropic_https` / `claude-sonnet-5-5` |
| Terminal state | `generated`; canonical package exported |
| Package | `generated-method` version `0.1.0`, entrypoint `method.py` |
| Source | 4027 UTF-8 bytes, 137 lines; Python AST parse passes |
| Observed imports | `json`, `math`, `sys` |
| Declared cap | 1 call, 8192 output tokens, 120000ms request timeout |
| Admission after completion | One reservation, zero remaining attempts |
| Execution / scoring / adoption / inheritance | Not performed |

| Observed cost | Calls | Input tokens | Output tokens | Request wall time (ms) |
| --- | ---: | ---: | ---: | ---: |
| First attempt, failed | 1 | 3971 | 8192 | 62131 |
| Second attempt, failed | 1 | 4028 | 4892 | 36790 |
| Third attempt, generated | 1 | 4028 | 5231 | 42198 |
| Arithmetic total | 3 | 12027 | 18315 | 141119 |

Token counts are provider-reported; calls and request time are locally recorded.
The total includes both failures and is arithmetic across separate declarations,
not a predeclared combined budget, elapsed experiment time or comparative result.
Input-token and aggregate wall grants remain accounting limits, not provider
billing or universal enforcement. `provider_default` sampling is not fixed-settings
G4 evidence. These retained host records are not provider-signed proof.

## Exact package and receiver handoff

- Package SHA256: `8215c3679a06724069759bcbdaa576b64ec0ef1251c5238a797c0cb34962b944`.
- Source SHA256: `68e990eaebff8a4c69e152aa98743a1f50fd318d031f57b83b78170707b1ab31`.
- Final checkpoint: `9d8c13540453fdff91bf7781a78eb1080d0795d0a334d10809c4a3b40de1ecc9`.

`method-package.json` is one canonical JSON line with no trailing newline, copied
unchanged from the export. The source was not edited. Its pinned runtime is
`0b21e795ac528c0f9cbe99e28ca1125af0c4dc5b87f53224abfe2a2ae5a69491` and protocol is
`d3b90c36b32e89b4e3d191dc018717301a4afc386ac915123cf5cf86250c550d`.
Henry owns checking those exact bindings before using the existing runner and
independent acceptance path. Do not retag the runtime to fit another host; a
mismatch must remain explicit. Runtime compatibility, scoring, adoption and
inheritance have not been demonstrated. G0–G4 remain open.

`static-inspection.json` reports syntax and observed imports only. Static parsing
does not establish behavior, safety, enforcement or task success. No model source
was executed during generation, inspection or publication of this bundle.

## Inspect without another provider request

From this directory, with `genesis-forge` built at the recorded source:

```sh
shasum -a 256 -c SHA256SUMS
genesis-forge method-generate-status --checkpoint generation.json
genesis-forge method-admission-status --admission admission.json
genesis-forge method-roster-assess --manifest snapshots.json --roster roster.json
```

These commands are read-only. All eleven JSON files match the retained originals
byte for byte; SHA256SUMS covers each. The bundle contains public request inputs,
the returned source and exact package, with no credentials or thinking payloads.
It is historical evidence, not an operating journal. Do not reset or replay any
of the three spent operations. The next step is Henry's bounded runtime/acceptance
handoff of these exact bytes, with all generation costs retained separately.
