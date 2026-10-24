# Experimental method source packages

`development_package::MethodPackage` implements byte-bearing source transfer and
loading for the existing execution binding. It does not execute code, create a
role, publish to the registry, approve adoption or verify a method's derivation.
No method language or subprocess protocol is selected by this library.

An author constructs a package containing immutable ID/version labels, exact
runtime and protocol references, an entrypoint, ordered dependency references and
UTF-8 source files. Files are sorted by path; content bytes (including whitespace
and Unicode encoding) are preserved. `artifact_json` returns the exact canonical
UTF-8 artifact with a `genesis.development_method_source`, version 1 envelope.
`reference` uses SHA-256 of **those bytes**, schema `development-method-source/1`.
This is additive: existing Apify `genesis-source-v1` hashes remain unchanged.

A receiving host calls `load_for(bytes, frozen_binding, runtime, protocol)`. The
binding must come from its own trusted plan, and runtime/protocol must be selected
independently from its trusted environment configuration, never from the received
package. Loading checks the received byte digest, canonical encoding, exact labels,
ordered dependencies, runtime/protocol and entrypoint before returning read-only
source views. Rehashing substituted runtime/dependency claims cannot change the
host's frozen choices. A source revision requires a new explicit method selection;
the loader neither accepts that revision for the organization nor proves benefit.

Limits: 256 files, 256-byte relative ASCII paths, 2 MiB per source, 4 MiB total
source, 128 dependency references, 8 MiB encoded artifact. Unsafe relative paths,
case-insensitive aliases, file/directory collisions, duplicate dependency identities,
missing entrypoints, unknown fields and noncanonical wire encodings are rejected.
The host must also bound its transport read **before** calling this in-memory API.

Runtime responsibilities remain: resolve and verify dependency/runtime/protocol
bytes, approve compatibility, materialize files into a fresh isolated workspace
without symlink/host-file aliasing, enforce grants, meter actual execution, and
emit receipts. No file is written or command run here. File views obtained through
ordinary deserialization are not trusted; use `load_for` at the execution boundary.
Neither hashes nor successful loading authenticate a publisher or prove generation,
execution, local acceptance, comparative improvement or a completed A-to-B-to-A loop.

Focused checks: `cargo test -p genesis-core --test development_package`. Transfer
and revision tests use labeled text fixtures; they are library tests, not native
method execution or model-written method evidence.
