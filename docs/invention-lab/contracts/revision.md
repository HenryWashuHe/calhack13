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
