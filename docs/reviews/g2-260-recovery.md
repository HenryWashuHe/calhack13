# G2 attempt 2: partial publication recovery

This is a static audit of [PR #260](https://github.com/HenryWashuHe/calhack13/pull/260)
at original head `2ba046c8c66d38943b93c6d11cce49a7be108742`. It restores only
bytes matching the existing archive manifest. No driver, method, evaluator,
scorer, namespace, experiment or provider request was executed.

## Publication status

The original [SHA256SUMS](../invention-lab/evidence/g2-attempt-2/SHA256SUMS)
contains 1,450 distinct file paths. Initially 596 were published and matched;
854 were absent. This correction restores **184 paths / 39 unique digests**.
All **780 currently present manifested files** match their original SHA256.
**670 remain absent: 638 content-addressed artifacts and 32 logs/SQLite files.**
The full archive still cannot pass its manifest check or support complete
independent reproduction. This remains an incomplete publication, not a cleared
evidence package or a completed G2 gate.

- [Recovery provenance](g2-260-recovery.tsv) lists every added path, expected
  digest, original Git blob or embedded JSON source, and encoding where relevant.
- [Remaining missing files](g2-260-missing.sha256) preserves the original expected
  digests. Paths are relative to the evidence directory, not this note.

Of the restored paths, 174 came from 34 historical Git blobs. Ten came from five
scoring objects embedded in existing `candidate-feedback.json` files. The latter
were serialized using the recorded encoding and accepted only when the resulting
bytes exactly matched the preexisting manifest digest. No metric, receipt or
experimental outcome was recomputed or substituted.

The search covered 3,694 historical documentation/experiment/script blobs,
2,254 other Git blobs, and 20,831 local evidence/live-method files across existing
worktrees. The local file search and other-blob scan yielded no further matches.
This does not establish that the missing bytes are absent from the author's
original external machine. Restoring those exact files is still required.

## Historical claims and limits

The original README body and every originally published evidence file are
byte-identical; only a clearly separated publication-status banner was added to
the README. The manifest was not shortened or regenerated. Recovery inventories
live outside the historical archive.

The historical report describes six planner calls, five adopted/invoked paths,
and a separate operator-reported reproduction of 29 slots. Those statements
remain author reports. This review did not authenticate provider responses,
reproduce execution, establish Linux isolation, or confirm that reproduction
against a complete local archive. The report's phrase about five paths under a
new owner includes **three new-role paths and two existing-lead paths**, as its
own table shows; it is not evidence of five newly created specialists.

Static checksum agreement establishes byte identity for available records. It
does not fill missing content, establish chronology or independent organizations,
or prove model improvement. A credential-pattern scan of the restored bytes
found no matching token/private-key patterns; it is not a general security audit.

## Recheck without execution

From `docs/invention-lab/evidence/g2-attempt-2`, `shasum -a 256 -c SHA256SUMS`
will verify the 780 available entries and report the 670 listed missing files.
Those missing-file failures are expected and must not be interpreted as a
successful full-archive check. Comparing the recovery inventory against the
original manifest checks that no replacement digest was introduced.
