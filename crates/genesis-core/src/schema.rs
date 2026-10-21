//! JSON Schema export for the shared contracts.
//!
//! The Rust structs are the single source of truth. `cargo run -p
//! genesis-server -- export-schemas` writes these to `schemas/`, from which the
//! TypeScript UI and the Python Fetch bridge generate or validate their types.

use schemars::schema::RootSchema;
use schemars::schema_for;
use std::collections::BTreeMap;

/// Named root schemas for every boundary type.
pub fn all() -> BTreeMap<&'static str, RootSchema> {
    let mut m = BTreeMap::new();
    m.insert("OrgState", schema_for!(crate::model::OrgState));
    m.insert(
        "OrgChangeProposal",
        schema_for!(crate::change::OrgChangeProposal),
    );
    m.insert(
        "OrgChangeRecord",
        schema_for!(crate::change::OrgChangeRecord),
    );
    m.insert("DomainEvent", schema_for!(crate::events::DomainEvent));
    m.insert(
        "MissionBootstrap",
        schema_for!(crate::engine::MissionBootstrap),
    );
    m.insert(
        "ValidationError",
        schema_for!(crate::engine::ValidationError),
    );
    m.insert("DispatchError", schema_for!(crate::engine::DispatchError));
    m.insert("Job", schema_for!(crate::jobs::Job));
    m.insert("DispatchRequest", schema_for!(crate::jobs::DispatchRequest));
    m.insert(
        "JobResultSubmission",
        schema_for!(crate::jobs::JobResultSubmission),
    );
    m.insert(
        "CapabilityManifest",
        schema_for!(crate::capability::CapabilityManifest),
    );
    m.insert(
        "VerificationReport",
        schema_for!(crate::capability::VerificationReport),
    );
    m
}

#[cfg(test)]
mod tests {
    #[test]
    fn every_schema_serializes() {
        for (name, schema) in super::all() {
            let json = serde_json::to_string(&schema).unwrap();
            assert!(json.len() > 20, "{name} schema looks empty");
        }
    }
}
