# Capability core checks

Run `bash scripts/check-capability-core.sh` from a checkout with Rust 1.94 or newer,
rustfmt and Clippy. It checks formatting, tests both crates, rejects lint warnings,
and reruns acceptance with `serde_json/preserve_order` to cover workspace feature
unification. The tests use local fixtures and loopback HTTP, with no provider keys.

`capability-core.yml.example` is a proposed GitHub Actions workflow. It is kept
here because the authenticated GitHub token could push code but was denied
permission to create workflow files (missing `workflow` scope). No hosted CI is
installed or claimed to have passed. Once an authorized account can create
workflows, place the template at `.github/workflows/capability-core.yml`.

The template pins Rust 1.94.0 and the verified `actions/checkout` v4 commit. It
tests the two crates separately while Claude's kernel workspace is under
development. If a root workspace adopts them, update its member list/lockfile
before running the same checks.
