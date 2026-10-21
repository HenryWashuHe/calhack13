//! Strongly typed identifiers.
//!
//! Every domain identifier is its own newtype so that a `RoleId` can never be
//! passed where an `AssignmentId` is expected. Provider identifiers (Elastic
//! agent IDs, Apify build IDs, ...) are plain strings stored on bindings and
//! never replace these domain IDs.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::fmt;

macro_rules! define_id {
    ($(#[$meta:meta])* $name:ident, $prefix:literal) => {
        $(#[$meta])*
        #[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema)]
        #[serde(transparent)]
        pub struct $name(pub String);

        impl $name {
            /// Prefix used for newly generated identifiers.
            pub const PREFIX: &'static str = $prefix;

            /// Generate a fresh random identifier such as `role_3f2a...`.
            pub fn generate() -> Self {
                Self(format!("{}_{}", $prefix, uuid::Uuid::new_v4().simple()))
            }

            /// Wrap an existing identifier string.
            pub fn new(value: impl Into<String>) -> Self {
                Self(value.into())
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&self.0)
            }
        }

        impl From<&str> for $name {
            fn from(value: &str) -> Self {
                Self(value.to_owned())
            }
        }

        impl From<String> for $name {
            fn from(value: String) -> Self {
                Self(value)
            }
        }
    };
}

define_id!(
    /// An independent organization runtime (Org A, Org B, ...).
    OrgId, "org"
);
define_id!(MissionId, "mis");
define_id!(ObservationId, "obs");
define_id!(RequirementId, "req");
define_id!(ResponsibilityId, "rsp");
define_id!(RoleId, "role");
define_id!(AssignmentId, "asg");
define_id!(ContractId, "ctr");
define_id!(
    /// Stable identity of a capability across versions.
    CapabilityId, "cap"
);
define_id!(InstallationId, "inst");
define_id!(JobId, "job");
define_id!(AttemptId, "att");
define_id!(ChangeId, "chg");
define_id!(EventId, "evt");
define_id!(CorrelationId, "cor");
define_id!(
    /// Logical operation ID used to make ledger effects idempotent.
    OperationId, "op"
);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_ids_have_prefix_and_are_unique() {
        let a = RoleId::generate();
        let b = RoleId::generate();
        assert!(a.as_str().starts_with("role_"));
        assert_ne!(a, b);
    }

    #[test]
    fn ids_serialize_as_plain_strings() {
        let id = MissionId::new("mis_demo");
        assert_eq!(serde_json::to_string(&id).unwrap(), "\"mis_demo\"");
        let back: MissionId = serde_json::from_str("\"mis_demo\"").unwrap();
        assert_eq!(back, id);
    }
}
