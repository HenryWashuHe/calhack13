# Generation roster before results

`GenerationRoster::freeze` captures 1–64 existing **Prepared** method-generation
sessions, their order, a common host-selected `TextModelSettings`, and a declared
generation-only `ResourceGrant`. It refuses started/completed sessions, mixed
settings, duplicate identities, and a grant unable to cover the planned request
count or sum of requested maximum output tokens. Ordinary and parent-bound
revision sessions use the same interface.

The source-free record contains org/operation identities and exact plan,
request and initial checkpoint hashes. Its digest includes order, settings and
grant. Save it before calling generation step and retain that exact identity
with the experiment. The roster does not include prompts, source or secrets.

`assess(snapshots)` accepts only snapshots matching declared identities and
settings. Each matching attempt can advance from its initial checkpoint to a
later saved result. The initial checkpoint identity is rederived from that
snapshot's saved historical request, without resetting its phase or permitting
another call. A substituted initial hash is rejected even for advanced results.
The existing cost assessor still validates exact selected
snapshots and usage. Every declared attempt contributes a row even if its
snapshot is missing, still Prepared, failed, rejected or uncertain. Missing and
unused observations retain the existing Unknown accounting behavior. Supplied
foreign, duplicate or substituted snapshots fail; omitting an unsuccessful
attempt does not remove its row. Known overruns remain visible.

The report binds the roster digest and derived exact-snapshot cost scope. It is
generation-only; execution/scoring reports and their independent trial plans
remain separate. This does not replace `method-cost-assess` or establish a full
fair comparison, adoption or recursive improvement.

For an adaptive A → B → A chain, each stage can be declared once its exact
parent/input bytes exist and before its own requests. This is not a preregistered
allocation policy for the entire adaptive chain.

The API enforces a pre-result phase condition, not authenticated chronology.
A trusted host can reconstruct a Prepared session, rewrite a declaration, run
undeclared work or copy a checkpoint. Retained immutable storage and experiment
procedure are responsible for preventing that; the digest is not a signature.
This roster does not gate provider calls, reserve shared spend, enforce input
token/wall quotas or prove exhaustive history. Requested output caps are not
proof of observed cost compliance. Common recorded settings do not freeze
provider-default sampling or provider internals. No request, execution, recovery
or external effect occurs during freeze or assessment.
