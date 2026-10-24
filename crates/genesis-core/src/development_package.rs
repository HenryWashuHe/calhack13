//! Exact-byte source transfer for development methods. No execution or authority.
use crate::development_method::{ContractError, DevelopmentRunBinding, ExactContentRef};
use crate::digest::canonical_json;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;

const DOMAIN: &str = "genesis.development_method_source";
const SCHEMA: &str = "development-method-source/1";
pub const MAX_PACKAGE_BYTES: usize = 8 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MethodSourceFile {
    pub path: String,
    pub content: String,
}

/// Immutable after construction/loading. Runtime and protocol identify exact
/// host-supported artifacts, not a shell command or permission to fetch them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MethodPackage {
    id: String,
    version: String,
    runtime: ExactContentRef,
    protocol: ExactContentRef,
    entrypoint: String,
    dependencies: Vec<ExactContentRef>,
    files: Vec<MethodSourceFile>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Envelope {
    domain: String,
    version: u8,
    package: MethodPackage,
}

fn invalid(field: &'static str) -> ContractError {
    ContractError::Invalid(field)
}

fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn valid_path(path: &str) -> bool {
    !path.is_empty()
        && path.len() <= 256
        && path
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"._-/".contains(&c))
        && path
            .split('/')
            .all(|part| !part.is_empty() && part != "." && part != "..")
}

impl MethodPackage {
    pub fn new(
        id: String,
        version: String,
        runtime: ExactContentRef,
        protocol: ExactContentRef,
        entrypoint: String,
        dependencies: Vec<ExactContentRef>,
        mut files: Vec<MethodSourceFile>,
    ) -> Result<Self, ContractError> {
        files.sort_by(|a, b| a.path.cmp(&b.path));
        let package = Self {
            id,
            version,
            runtime,
            protocol,
            entrypoint,
            dependencies,
            files,
        };
        // Include the encoded-size bound, not just unescaped source lengths.
        package.artifact_json()?;
        Ok(package)
    }

    fn reference_with_hash(&self, sha256: String) -> ExactContentRef {
        ExactContentRef {
            domain: "genesis".into(),
            schema: SCHEMA.into(),
            id: self.id.clone(),
            version: self.version.clone(),
            sha256,
        }
    }

    fn validate(&self) -> Result<(), ContractError> {
        self.reference_with_hash("0".repeat(64)).validate()?;
        self.runtime.validate()?;
        self.protocol.validate()?;
        if self.dependencies.len() > 128 {
            return Err(invalid("method dependency count"));
        }
        let mut dependencies = BTreeSet::new();
        for dependency in &self.dependencies {
            dependency.validate()?;
            if !dependencies.insert((
                &dependency.domain,
                &dependency.schema,
                &dependency.id,
                &dependency.version,
            )) {
                return Err(invalid("duplicate method dependency"));
            }
        }
        if self.files.is_empty() || self.files.len() > 256 {
            return Err(invalid("method file count"));
        }
        let mut names = BTreeSet::new();
        let mut total = 0;
        for file in &self.files {
            if !valid_path(&file.path)
                || file.content.len() > 2 * 1024 * 1024
                || !names.insert(file.path.to_ascii_lowercase())
            {
                return Err(invalid("method source file"));
            }
            total += file.content.len();
            if total > 4 * 1024 * 1024 {
                return Err(invalid("method source bytes"));
            }
        }
        for name in &names {
            for (index, _) in name.match_indices('/') {
                if names.contains(&name[..index]) {
                    return Err(invalid("method file/directory collision"));
                }
            }
        }
        if !self
            .files
            .windows(2)
            .all(|pair| pair[0].path < pair[1].path)
            || !self.files.iter().any(|file| file.path == self.entrypoint)
        {
            return Err(invalid("method ordering or entrypoint"));
        }
        Ok(())
    }

    /// These exact UTF-8 bytes, including source whitespace, are the artifact.
    pub fn artifact_json(&self) -> Result<String, ContractError> {
        self.validate()?;
        let encoded = canonical_json(&serde_json::json!({
            "domain": DOMAIN, "version": 1, "package": self,
        }));
        if encoded.len() > MAX_PACKAGE_BYTES {
            return Err(invalid("method package bytes"));
        }
        Ok(encoded)
    }

    pub fn reference(&self) -> Result<ExactContentRef, ContractError> {
        Ok(self.reference_with_hash(hash(self.artifact_json()?.as_bytes())))
    }

    /// The host supplies frozen state and runtime/protocol selected independently
    /// from its trusted environment configuration. Never select them from bytes.
    /// Dependency references are checked; resolving their bytes remains host work.
    pub fn load_for(
        bytes: &[u8],
        expected: &DevelopmentRunBinding,
        runtime: &ExactContentRef,
        protocol: &ExactContentRef,
    ) -> Result<Self, ContractError> {
        expected.validate()?;
        Self::load_source_for(
            bytes,
            &expected.method,
            &expected.dependencies,
            runtime,
            protocol,
        )
    }

    /// Inspect exact source bytes without inventing an execution grant. This
    /// confers no execution, installation or adoption authority.
    pub fn load_source_for(
        bytes: &[u8],
        method: &ExactContentRef,
        dependencies: &[ExactContentRef],
        runtime: &ExactContentRef,
        protocol: &ExactContentRef,
    ) -> Result<Self, ContractError> {
        method.validate()?;
        runtime.validate()?;
        protocol.validate()?;
        if bytes.len() > MAX_PACKAGE_BYTES {
            return Err(invalid("method package bytes"));
        }
        if method.domain != "genesis" || method.schema != SCHEMA || hash(bytes) != method.sha256 {
            return Err(ContractError::IdentityMismatch);
        }
        let envelope: Envelope =
            serde_json::from_slice(bytes).map_err(|_| invalid("method package encoding"))?;
        if envelope.domain != DOMAIN || envelope.version != 1 {
            return Err(invalid("method package format"));
        }
        let package = envelope.package;
        if package.artifact_json()?.as_bytes() != bytes {
            return Err(invalid("noncanonical method package"));
        }
        if package.id != method.id
            || package.version != method.version
            || package.dependencies != dependencies
            || &package.runtime != runtime
            || &package.protocol != protocol
        {
            return Err(ContractError::IdentityMismatch);
        }
        Ok(package)
    }

    pub fn entrypoint(&self) -> &str {
        &self.entrypoint
    }
    pub fn files(&self) -> &[MethodSourceFile] {
        &self.files
    }
    pub fn dependencies(&self) -> &[ExactContentRef] {
        &self.dependencies
    }
    pub fn runtime(&self) -> &ExactContentRef {
        &self.runtime
    }
    pub fn protocol(&self) -> &ExactContentRef {
        &self.protocol
    }
}
