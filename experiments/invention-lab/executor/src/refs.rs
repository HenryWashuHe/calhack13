//! Exact content references for lab artifacts. A reference names bytes; it
//! does not authenticate who made them.
use genesis_core::development_method::ExactContentRef;
use genesis_lab_eval::family::{instance, instance_digest, Family, Instance, FAMILY_VERSION};
use genesis_lab_eval::score::{evaluator_digest, EVALUATOR_ID, EVALUATOR_VERSION};
use sha2::{Digest, Sha256};

pub const DOMAIN: &str = "genesis-lab";
pub const EXECUTOR_VERSION: &str = env!("CARGO_PKG_VERSION");

pub fn sha256_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

pub fn content(schema: &str, id: &str, version: &str, bytes: &[u8]) -> ExactContentRef {
    ExactContentRef {
        domain: DOMAIN.into(),
        schema: schema.into(),
        id: id.into(),
        version: version.into(),
        sha256: sha256_hex(bytes),
    }
}

/// The in-process evaluator this executor links.
pub fn evaluator() -> ExactContentRef {
    ExactContentRef {
        domain: DOMAIN.into(),
        schema: "lab-evaluator/1".into(),
        id: EVALUATOR_ID.into(),
        version: EVALUATOR_VERSION.into(),
        sha256: evaluator_digest(),
    }
}

/// `"<family>/<version>/<seed>"`, for development seeds only.
pub fn instance_id(family: Family, seed: u64) -> String {
    format!("{}/{FAMILY_VERSION}/{seed}", family.name())
}

pub fn instance_ref(family: Family, seed: u64) -> ExactContentRef {
    ExactContentRef {
        domain: DOMAIN.into(),
        schema: "lab-instance/1".into(),
        id: instance_id(family, seed),
        version: FAMILY_VERSION.to_string(),
        sha256: instance_digest(&instance(family, seed)),
    }
}

/// Regenerates the world a reference names and checks its digest.
pub fn resolve_instance(reference: &ExactContentRef) -> Option<Instance> {
    let mut parts = reference.id.split('/');
    let family = Family::parse(parts.next()?)?;
    let version: u32 = parts.next()?.parse().ok()?;
    let seed: u64 = parts.next()?.parse().ok()?;
    if parts.next().is_some()
        || version != FAMILY_VERSION
        || genesis_lab_eval::score::is_hidden_seed(seed)
    {
        return None;
    }
    (instance_ref(family, seed) == *reference).then(|| instance(family, seed))
}

/// The method runtime this host provides: an interpreter at an exact version.
/// Selected by the host from its own configuration, never from a package.
pub fn runtime(interpreter_version: &str) -> ExactContentRef {
    content(
        "lab-runtime/1",
        "python3",
        "1",
        genesis_core::digest::canonical_json(&serde_json::json!({
            "interpreter": "python3", "version": interpreter_version,
        }))
        .as_bytes(),
    )
}

/// The JSON-line protocol between a method and this executor.
pub const PROTOCOL: &str = "genesis-lab-exec-lines/1: one JSON object per line on stdin/stdout; \
ops instances, experiment{instance,design}, submit{design}; replies carry ok and refused";

pub fn protocol() -> ExactContentRef {
    content(
        "lab-protocol/1",
        "genesis-lab-exec-lines",
        "1",
        PROTOCOL.as_bytes(),
    )
}

/// Executor sources, so the environment digest changes when they do.
const SOURCES: [(&str, &[u8]); 7] = [
    ("Cargo.toml", include_bytes!("../Cargo.toml")),
    ("src/lib.rs", include_bytes!("lib.rs")),
    ("src/refs.rs", include_bytes!("refs.rs")),
    ("src/sandbox.rs", include_bytes!("sandbox.rs")),
    ("src/store.rs", include_bytes!("store.rs")),
    ("src/run.rs", include_bytes!("run.rs")),
    ("src/session.rs", include_bytes!("session.rs")),
];

pub fn source_files() -> Vec<&'static str> {
    SOURCES.iter().map(|(n, _)| *n).collect()
}

/// The development environment a plan pins: executor source digest, sandbox
/// mode and interpreter. Running under a different mode is a mismatch.
pub fn environment(sandbox: crate::sandbox::Mode, interpreter_version: &str) -> ExactContentRef {
    let mut h = Sha256::new();
    for (name, bytes) in SOURCES {
        h.update((name.len() as u64).to_be_bytes());
        h.update(name.as_bytes());
        h.update((bytes.len() as u64).to_be_bytes());
        h.update(bytes);
    }
    let descriptor = serde_json::json!({
        "executor": format!("genesis-lab-exec@{EXECUTOR_VERSION}"),
        "executor_sources_sha256": format!("{:x}", h.finalize()),
        "sandbox": sandbox.describe(),
        "interpreter": interpreter_version,
    });
    content(
        "lab-environment/1",
        &format!("genesis-lab-exec/{}", sandbox.name()),
        EXECUTOR_VERSION,
        genesis_core::digest::canonical_json(&descriptor).as_bytes(),
    )
}
