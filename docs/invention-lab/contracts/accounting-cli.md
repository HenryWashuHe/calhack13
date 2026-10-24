# Offline cost assessment

`genesis-forge method-cost-assess --manifest cost-evidence.json` reads saved
evidence and emits one JSON report. No credentials are required. It does not
execute source, call providers, recover pending requests, lock or rewrite files.

With a retained generation journal, use the optional
[`--admission` assessment](admitted-costs.md#cli). That mode requires the full
original Prepared roster and joins later snapshots without changing the journal.
The exact-snapshot behavior below remains unchanged without the option.

The manifest is a trusted local file; paths are resolved relative to its directory
(absolute paths are also accepted):

```json
{
  "schema": "genesis-method-cost-input/1",
  "scope": "cost-scope.json",
  "generation_checkpoints": ["generation-a.json", "failed-generation-b.json"],
  "trial": { "plan": "trial.json", "evidence": "trial-evidence.json" }
}
```

`scope` contains the serialized `CostScope`; see the [accounting contract](accounting.md).
The caller must declare the full intended generation roster, exact selected
checkpoint/request/plan hashes, all trial slots and total grant. This command
does not construct or silently update that roster from whichever files happen
to be present. Generation checkpoints use `MethodSynthesisSession` snapshots.
Trial evidence is a JSON array of `TrialEvidence` records containing `slot_id`,
`execution` and optional `scoring` receipts.

Omit a checkpoint from the list or a slot from the evidence array to report its
declared evidence as missing/unknown. Use `"trial": null` when no trial evidence
is available. A named file that is absent, malformed or bound to another identity
is an input error, not unknown consumption. Duplicate supplied evidence is rejected.
Input files have the existing 8 MiB JSON limit; at most 64 generation files are
accepted. Use retained snapshots in a trusted directory. This reader does not
prove that the roster was declared before spending or is an exhaustive history.

Exit 0 means assessment completed, including reports with failures, unknown usage
or measured overruns. Exit 2 means invalid input or a local error. Neither the
exit code nor the report authorizes execution/adoption or establishes improvement.
Inspect stage and combined totals, including `unknown`, `overflow` and
`limit_exceeded`. A known lower bound of zero with `unknown: true` is not zero cost.
The report retains evidence hashes and generation transport, but does not print
generation prompts/source or raw trial outputs. It is local accounting evidence,
not authenticated provider metering or a completed controlled comparison.
