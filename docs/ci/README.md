# Capability core checks

Run `bash scripts/check-capability-core.sh` from a checkout with Rust 1.94 or newer,
rustfmt, Clippy and Node.js 24 or newer. It checks formatting, tests all three crates, rejects lint warnings,
and reruns acceptance with `serde_json/preserve_order` to cover workspace feature
unification. It also installs the Actor's locked dependencies and runs its
TypeScript/SDK tests. The tests use local fixtures and loopback HTTP, with no
provider keys. Dependency installation can use the network.

`node scripts/demo-local-capability.mjs` additionally prepares a real source
package through the forge CLI, compiles it in a fresh directory, runs the baseline
parser with actual SDK local storage, and invokes the Rust verifier on A/B receipts.
It checks both semantic rejection and rejection of A's receipts for B's new input.
This is a hand-authored local baseline with explicitly synthetic provider IDs.

`capability-core.yml.example` is a proposed GitHub Actions workflow. It is kept
here because the authenticated GitHub token could push code but was denied
permission to create workflow files (missing `workflow` scope). No hosted CI is
installed or claimed to have passed. Once an authorized account can create
workflows, place the template at `.github/workflows/capability-core.yml`.

The template pins Rust 1.94.0 and the verified `actions/checkout` v4 commit. It
tests the three crates separately while Claude's kernel workspace is under
development. If a root workspace adopts them, update its member list/lockfile
before running the same checks.
