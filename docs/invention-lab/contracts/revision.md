# Parent-bound method revision (source-only)

`genesis_forge::method_revision::MethodRevisionSpec` connects an exact parent
source package to one ordinary method-generation attempt. It contains
`generation` (the existing `MethodSynthesisPlan`) and `parent` (an
`ExactContentRef`). The host independently chooses runtime, protocol and ordered
dependencies and authorizes the parent's public disclosure to the model.

`prepare(parent_bytes)` validates canonical package bytes against that reference
and those host requirements. It appends the complete parent package as the final
public input, prefixes a versioned revision instruction and creates the existing
immutable generation checkpoint. It executes no source and makes no provider
request. The checkpoint continues through the existing single-attempt
generation path, including its saved request, interruption and no-replay rules.
Changing the parent, org, task, settings or revision instruction requires a new
declared attempt, not rewriting an earlier checkpoint.

Revision v1 supports one parent source file and one generated output file. The
encoded parent must fit the existing 128 KiB public-document limit; existing
input-count, total-plan and prompt limits apply after inclusion. Runtime,
protocol and dependencies must match; environment-changing revisions require a
separately designed contract. Target id/version must differ from the parent's
pair. This prevents reusing the parent identity but does not establish global
name uniqueness. An identical parent already in the supplied public inputs is
refused rather than appended twice.

`observe(parent_bytes, session)` validates the saved session and its entire
prepared plan before inspecting a generated child. It produces a
`genesis-method-revision-observation/1` record with the revision-spec digest,
parent/child refs, declared org/operation, saved request/checkpoint hashes,
settings, transport and both source digests. Renaming, reversioning or moving
identical source to a new entrypoint leaves `source_changed` false. Even a true
flag establishes only different source bytes; comments/whitespace can change
without affecting behavior. The observation omits the private source/prompt.

Prepared, failed, rejected or uncertain attempts cannot yield a generated-child
observation. Their generation checkpoints remain intact for the normal
[cost assessment](accounting.md); absence of a child is never zero cost.

These integrity links depend on trusted host storage. They do not authenticate
an organization's identity, prove it adopted or executed the parent, verify
dependency contents, grant execution/installation authority or prove improved
behavior. A fixture transport remains a fixture. The actual B-developed and
A-accepted executable return, independent evaluation and G3/G4 results remain
open. Keep each source generation, execution, scoring and adoption record
distinct.

## CLI

Save a revision spec (`generation` plus `parent`) and the exact canonical parent
package locally. These commands are offline:

```sh
cargo run -p genesis-forge -- method-revise-prepare \
  --plan revision.json --parent parent.package.json --checkpoint generation.json
cargo run -p genesis-forge -- method-revise-observe \
  --plan revision.json --parent parent.package.json --checkpoint generation.json
```

Preparation refuses to overwrite an existing checkpoint and emits only status
and identity hashes. The checkpoint contains the authorized public parent source
and prompt. Use the existing `method-generate-step --checkpoint generation.json`
for the single configured provider request, then `method-generate-export` to
export the child. Preparation itself makes no provider call. The new attempt
belongs in the normal cost roster, including when unsuccessful.

Observation reads the saved checkpoint without locking, recovering, writing or
calling a provider. It requires a validated generated child; a prepared, pending,
failed or uncertain checkpoint returns exit 2 with no observation and remains
unchanged. It also refuses an unrelated spec or parent. Existing lock files are
left untouched. Read-only observation is a snapshot, not an exclusive execution
or acceptance transaction. Exit 0 means the source observation was constructed,
not that its `source_changed` flag is true or the revision is better.

### Bundled input

`MethodRevisionInput` is a strict `genesis-method-revision-input/1` envelope with
`spec` (the ordinary revision spec) and `parent_package_json` (the exact canonical
UTF-8 parent). It is bounded to 1 MiB, with the same 128 KiB parent and existing
post-inclusion generation-plan limits. Its `prepare` and `observe` methods call
the existing revision logic; it introduces no separate session or ledger.

Both commands alternatively accept `--input received-revision.json` alongside
`--checkpoint`, mutually exclusive with `--plan` and `--parent`. Preparation
validates the whole input before creating a checkpoint or lock; observation
remains read-only and cannot recover a pending attempt. Unknown fields, schema
changes, source substitutions and oversized inputs are refused. Existing
plan/parent invocations and their checkpoint identities are unchanged.

The [registry CLI](method-registry.md#selected-parent-to-revision-input) produces
this bundle from a deliberately selected fetched parent, with a publisher-qualified
selection document inside the augmented spec. Retain that exact bundle for
`method-revise-observe --input received-revision.json --checkpoint generation.json`.
The observation's spec/request hashes bind the augmented spec, not the original
pre-fetch spec; it cannot silently discard or substitute the selection. A caller
can also construct the generic bundle locally, so its presence never proves a
network fetch, authenticated identity, adoption or behavioral improvement.
