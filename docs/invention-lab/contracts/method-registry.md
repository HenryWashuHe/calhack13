# Exact development-method exchange

The existing authenticated registry process now has a source-exchange resource
alongside Actor manifests. It uses the same SQLite database, publisher tokens
and reader tokens; organization state and installations remain separate.

| POST endpoint | Request | Result |
| --- | --- | --- |
| `/registry/v1/methods` | `MethodPublication`: full `reference`, exact canonical UTF-8 `package_json`, `intent`, `applicability` | `MethodOffer`: token-derived `publisher` and nested `publication`; 201 new, 200 identical retry, 409 conflicting immutable version |
| `/registry/v1/methods/search` | `query`, optional `limit` (default 10), exact `runtime` / `protocol`, `exclude_publisher` | Source-free `candidates` plus explicit `truncated` |
| `/registry/v1/methods/fetch` | `publisher` and full exact `reference` | Original offer; 404 absent, 409 different identity |

Reader tokens cannot publish. Publisher tokens can read. Immutable keys include
publisher, method ID and version: two organizations' publications never alias
solely because their IDs or hashes match. Publishing another version cannot
change the original. Retries must preserve metadata as well as exact source.

Source identity is checked with the existing `MethodPackage` canonical loader;
this never executes source, resolves dependencies or trusts the declared runtime.
The exchange subset is 128 KiB of canonical package bytes, at most 16 dependency
references, reference fields up to 256 bytes, intent up to 1,024 bytes, and up to
16 applicability strings of 256 bytes. Search candidates are bounded to 8 KiB.
These are transport limits, not new limits on the core package format.

Search uses a case-folded substring of public intent/applicability, with literal
SQL wildcard characters, exact optional environment filters and deterministic
publisher/ID/version ordering. Query length is 1–256 bytes and limit is 1–50.
An extra match sets `truncated`; this is not complete semantic discovery or
Elastic method retrieval. An error is never an empty candidate list.

The receiver must fetch the selected exact publisher/reference and pass the
unchanged `package_json` bytes to its existing `Offer` or revision preparation.
Publications contain source and deliberately public descriptions only: do not
upload generation checkpoints, full prompts, private feedback or credentials.
Token-bound publisher identity attests who submitted bytes to this configured
registry, not who authored them or whether they are verified, compatible, useful
or independently evaluated. Search never grants acceptance or invocation rights.
Receiver-owned selection, acceptance, current role/grant authority and pinned
installations remain required; publication alone closes none of G0–G4.

## Typed organization client

The existing `RegistryClient` exposes `publish_method(expected_publisher,
publication)`, `search_methods(query)`, `method_offer(lookup)` and
`fetch_method_candidate(candidate)`. These reuse its fixed-origin HTTP transport,
15-second timeout, 1 MiB bounded strict JSON decoder, disabled redirects and
disabled retries. Local malformed input is refused before the request. Remote
errors and inconsistent successful responses remain errors, with no remote body
or credentials reflected in diagnostics.

After receiver-owned selection, use `fetch_method_candidate`: it compares the
complete search metadata, runtime, protocol and dependencies against the fetched
canonical package. Plain `method_offer` binds the exact publisher/reference and
checks the publication, without asserting agreement with an earlier search hit.
Search checks bounds, filters, ordered unique identities and coherent truncation;
the registry's `truncated: false` is not independent proof of exhaustive absence.

`expected_publisher` checks the publication response; it cannot prevent a
misconfigured token from writing under its actual account. Any publication error,
including an identity mismatch, may follow a committed write. Reconcile exact
identity before deciding on another attempt; the client never retries.

The loopback client fixture publishes A's inert source, lets B discover/fetch it,
then passes the unchanged bytes to `MethodRevisionSpec::prepare` under B's new
task and operation. It stops at `Prepared`: no model request, source execution,
acceptance, role creation or recursive benefit is demonstrated.
