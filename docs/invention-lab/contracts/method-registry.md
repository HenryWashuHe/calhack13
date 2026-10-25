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

## Operator CLI

The existing `genesis` binary exposes `method publish`, `method search` and
`method fetch`. Set `GENESIS_REGISTRY_URL` to the registry origin and
`GENESIS_REGISTRY_TOKEN` locally; there is no token command-line argument or
automatic `.env` loading. `--registry-url` before the subcommand overrides the
URL. The input file is strict JSON, limited to 1 MiB, and is never modified.

```sh
genesis method publish --expected-publisher org-a --input publication.json
genesis method search --input query.json
genesis method fetch --input selected-candidate.json
```

Inputs are the existing `MethodPublication`, `MethodSearch` and `MethodCandidate`
records, respectively. A query can be `{"query":"transport","exclude_publisher":"org-b"}`.
Select one complete candidate from the search result under the receiver's own
policy and save it as `selected-candidate.json`; fetch never chooses for you.
Successful stdout contains only the corresponding `MethodOffer` or
`MethodSearchResults` JSON. Retain it locally without overwriting earlier
evidence. A redirected file by itself does not authenticate origin, execution
or acceptance; the command provides no durable receipt journal.

Each command invokes the existing bounded client once. Failure prints sanitized
diagnostics and exits nonzero, with no successful record. Publication may already
have committed when any error occurs, including a response-identity or stdout
write error. Reconcile before another attempt; do not automatically retry or
interpret missing output as no publication. These commands transfer public source;
they never execute it, prepare a revision, grant authority or adopt a method.
