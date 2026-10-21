use genesis_apify::{ActorDefinition, SourceFile};
use genesis_verification::json_sha256;
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;

/// Exact UTF-8 bytes uploaded for a candidate. The manifest hashes paths and
/// bytes, including the scaffold/lockfile, not merely generated parser code.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourcePackage {
    files: Vec<SourceFile>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct SourceEntry {
    pub path: String,
    pub byte_length: usize,
    pub sha256: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct SourceManifest {
    pub format: String,
    pub files: Vec<SourceEntry>,
}

impl SourcePackage {
    pub fn new(mut files: Vec<SourceFile>) -> Result<Self, ForgeStateError> {
        files.sort_by(|a, b| a.name.cmp(&b.name));
        let package = Self { files };
        package.validate()?;
        Ok(package)
    }

    pub fn files(&self) -> &[SourceFile] {
        &self.files
    }

    pub fn validate(&self) -> Result<(), ForgeStateError> {
        if self.files.is_empty() || self.files.len() > 256 {
            return Err(ForgeStateError::InvalidSource);
        }
        let mut names = BTreeSet::new();
        let mut bytes = 0usize;
        for file in &self.files {
            if file.name.is_empty()
                || file.name.len() > 256
                || !file
                    .name
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || b"._-/".contains(&c))
                || file
                    .name
                    .split('/')
                    .any(|part| part.is_empty() || part == "." || part == "..")
                || !names.insert(&file.name)
                || file.content.len() > 2 * 1024 * 1024
            {
                return Err(ForgeStateError::InvalidSource);
            }
            bytes += file.content.len();
            if bytes > 4 * 1024 * 1024 {
                return Err(ForgeStateError::InvalidSource);
            }
        }
        for path in &names {
            for (index, _) in path.match_indices('/') {
                if names.contains(&path[..index].to_string()) {
                    return Err(ForgeStateError::InvalidSource);
                }
            }
        }
        Ok(())
    }

    pub fn manifest(&self) -> Result<SourceManifest, ForgeStateError> {
        self.validate()?;
        let mut files: Vec<_> = self
            .files
            .iter()
            .map(|file| SourceEntry {
                path: file.name.clone(),
                byte_length: file.content.len(),
                sha256: format!("{:x}", Sha256::digest(file.content.as_bytes())),
            })
            .collect();
        files.sort_by(|a, b| a.path.cmp(&b.path));
        Ok(SourceManifest {
            format: "genesis-source-v1".into(),
            files,
        })
    }

    pub fn sha256(&self) -> Result<String, ForgeStateError> {
        let value =
            serde_json::to_value(self.manifest()?).map_err(|_| ForgeStateError::InvalidSource)?;
        Ok(json_sha256(&value))
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CandidateSpec {
    pub operation_id: String,
    pub capability_id: String,
    pub capability_version: String,
    pub actor_name: String,
    pub source_version: String,
    pub source: SourcePackage,
}

impl CandidateSpec {
    pub fn definition(&self) -> ActorDefinition {
        ActorDefinition {
            name: self.actor_name.clone(),
            version_number: self.source_version.clone(),
            source_files: self.source.files.clone(),
        }
    }

    pub fn validate(&self) -> Result<(), ForgeStateError> {
        if [
            &self.operation_id,
            &self.capability_id,
            &self.capability_version,
        ]
        .iter()
        .any(|s| {
            s.is_empty()
                || s.len() > 256
                || s.trim() != s.as_str()
                || s.chars().any(char::is_control)
        }) {
            return Err(ForgeStateError::InvalidCandidate);
        }
        self.source.validate()?;
        // Match the documented Actor name and major.minor source version contract.
        if self.actor_name.is_empty()
            || self.actor_name.len() > 64
            || !self
                .actor_name
                .bytes()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-')
            || self.actor_name.starts_with('-')
            || self.actor_name.ends_with('-')
        {
            return Err(ForgeStateError::InvalidCandidate);
        }
        if self.source_version.len() > 5 {
            return Err(ForgeStateError::InvalidCandidate);
        }
        let parts: Vec<_> = self.source_version.split('.').collect();
        if parts.len() != 2
            || parts.iter().any(|s| {
                s.is_empty()
                    || s.len() > 2
                    || !s.bytes().all(|c| c.is_ascii_digit())
                    || (s.len() > 1 && s.starts_with('0'))
            })
        {
            return Err(ForgeStateError::InvalidCandidate);
        }
        Ok(())
    }

    pub(crate) fn fingerprint(&self) -> Result<String, ForgeStateError> {
        self.validate()?;
        Ok(json_sha256(
            &json!({"format":"genesis-candidate-v1", "operation_id":self.operation_id,
            "capability_id":self.capability_id,"capability_version":self.capability_version,
            "actor_name":self.actor_name,"source_version":self.source_version,"source_sha256":self.source.sha256()?}),
        ))
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ForgeStateError {
    InvalidSource,
    InvalidCandidate,
    InvalidOracle,
    CorruptCheckpoint,
    SuiteChanged,
    OutcomeMismatch,
    NeedsReconciliation,
    Terminal,
    RepairNotAllowed,
    NotVerified,
}
impl std::fmt::Display for ForgeStateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "forge state: {self:?}")
    }
}
impl std::error::Error for ForgeStateError {}
