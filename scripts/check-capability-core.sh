#!/usr/bin/env bash
set -euo pipefail

repo_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_dir"

# Use a Rust >=1.94 toolchain with rustfmt and clippy on PATH.
# No provider credentials or network API calls are needed by these tests.
for crate in genesis-verification genesis-apify; do
  manifest="crates/$crate/Cargo.toml"
  cargo fmt --manifest-path "$manifest" --check
  cargo test --locked --manifest-path "$manifest"
  cargo clippy --locked --manifest-path "$manifest" --all-targets -- -D warnings
done

# A kernel workspace may unify serde_json features. Digests must remain stable.
cargo test --locked --manifest-path crates/genesis-verification/Cargo.toml \
  --features serde_json/preserve_order
