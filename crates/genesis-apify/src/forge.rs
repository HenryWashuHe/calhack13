//! Source-bearing Actor creation and resumable builds. Tickets are trusted
//! backend recovery records, not credentials or cryptographic attestations.
use crate::{valid_id, ApifyClient, BoundedWriter, Envelope, ErrorKind, Stage};
use genesis_verification::{decode_json, json_sha256};
use reqwest::{Method, Url};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::fmt;
use tokio::time::{sleep, timeout_at, Instant};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceFile {
    pub name: String,
    pub content: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ActorDefinition {
    pub name: String,
    pub version_number: String,
    pub source_files: Vec<SourceFile>,
}

impl ActorDefinition {
    /// Local structural preflight with an 8 MiB serialized payload ceiling. The
    /// client's configured upload cap can be lower, and is enforced again there.
    pub fn validate(&self) -> Result<(), ForgeError> {
        definition_payload(self, 8 * 1024 * 1024).map(|_| ())
    }

    /// Canonical SHA-256 of the exact create-request meaning, including fixed
    /// private/permission/environment settings and source files sorted by path.
    /// This digest is also stored in ActorTicket, BuildTicket, and BuiltActor.
    pub fn definition_sha256(&self) -> Result<String, ForgeError> {
        definition_payload(self, 8 * 1024 * 1024).map(|(_, digest)| digest)
    }
}

/// Persist only in trusted backend storage. Do not accept this from an Actor.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ActorTicket {
    schema_version: u32,
    actor_id: String,
    version_number: String,
    definition_sha256: String,
}
impl ActorTicket {
    /// Revalidate a restored trusted-storage ticket without network access.
    /// This checks structure/request binding, not provider authenticity.
    pub fn validate_for(&self, definition: &ActorDefinition) -> Result<(), ForgeError> {
        let fingerprint = definition.definition_sha256()?;
        if self.schema_version != 1
            || !valid_id(&self.actor_id)
            || self.version_number != definition.version_number
            || self.definition_sha256 != fingerprint
        {
            return Err(failure(
                ForgeErrorKind::TicketMismatch,
                ForgeStage::Configuration,
            ));
        }
        Ok(())
    }

    pub fn actor_id(&self) -> &str {
        &self.actor_id
    }
    pub fn version_number(&self) -> &str {
        &self.version_number
    }
    pub fn definition_sha256(&self) -> &str {
        &self.definition_sha256
    }
}

/// A known build can be polled again without another build submission.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BuildTicket {
    schema_version: u32,
    actor_id: String,
    build_id: String,
    build_number: String,
    version_number: String,
    definition_sha256: String,
}
impl BuildTicket {
    /// Includes Actor-ticket validation and the source-version/build-number
    /// relationship, so a corrupt checkpoint cannot relabel a different build.
    pub fn validate_for(
        &self,
        definition: &ActorDefinition,
        actor: &ActorTicket,
    ) -> Result<(), ForgeError> {
        actor.validate_for(definition)?;
        if self.schema_version != 1
            || self.actor_id != actor.actor_id
            || self.definition_sha256 != actor.definition_sha256
            || self.version_number != actor.version_number
            || !valid_id(&self.build_id)
            || !build_number(&self.build_number, &actor.version_number)
        {
            return Err(failure(
                ForgeErrorKind::TicketMismatch,
                ForgeStage::Configuration,
            ));
        }
        Ok(())
    }

    pub fn actor_id(&self) -> &str {
        &self.actor_id
    }
    pub fn build_id(&self) -> &str {
        &self.build_id
    }
    pub fn build_number(&self) -> &str {
        &self.build_number
    }
    pub fn version_number(&self) -> &str {
        &self.version_number
    }
    pub fn definition_sha256(&self) -> &str {
        &self.definition_sha256
    }
}

/// Successful provider build whose source snapshot matched the submitted files.
/// This is not semantic capability acceptance or execution authority.
#[derive(Clone, Debug, Serialize)]
pub struct BuiltActor {
    actor_id: String,
    build_id: String,
    build_number: String,
    version_number: String,
    definition_sha256: String,
}
impl BuiltActor {
    pub fn actor_id(&self) -> &str {
        &self.actor_id
    }
    pub fn build_id(&self) -> &str {
        &self.build_id
    }
    pub fn build_number(&self) -> &str {
        &self.build_number
    }
    pub fn version_number(&self) -> &str {
        &self.version_number
    }
    pub fn definition_sha256(&self) -> &str {
        &self.definition_sha256
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ForgeStage {
    Configuration,
    CreateActor,
    ReadVersion,
    StartBuild,
    PollBuild,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ForgeErrorKind {
    InvalidDefinition,
    TicketMismatch,
    Transport,
    HttpStatus(u16),
    DeadlineExceeded,
    ResponseTooLarge,
    MalformedResponse,
    ProvenanceMismatch,
    BuildFailed,
    UnknownBuildStatus,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ForgeError {
    pub kind: ForgeErrorKind,
    pub stage: ForgeStage,
    pub actor_id: Option<String>,
    pub build_id: Option<String>,
    /// The operation's POST may have succeeded. Reconcile before retrying it.
    pub submission_may_have_occurred: bool,
}
impl fmt::Display for ForgeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Apify forge {:?}: {:?}", self.stage, self.kind)
    }
}
impl std::error::Error for ForgeError {}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CreatedActor {
    id: String,
    name: String,
    is_public: bool,
    actor_permission_level: Option<String>,
    versions: Vec<VersionSnapshot>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct VersionSnapshot {
    version_number: String,
    source_type: String,
    source_files: Option<Vec<RemoteFile>>,
    env_vars: Option<Vec<Value>>,
    apply_env_vars_to_build: Option<bool>,
}

#[derive(Deserialize)]
struct RemoteFile {
    name: String,
    content: Option<String>,
    format: Option<String>,
    folder: Option<bool>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CloudBuild {
    id: String,
    act_id: String,
    build_number: String,
    status: String,
    act_version: Option<VersionSnapshot>,
}

impl ApifyClient {
    /// Creates a new private Actor with exactly one inline UTF-8 source version.
    /// No existing Actor is modified and no POST is automatically retried.
    pub async fn create_actor(
        &self,
        definition: &ActorDefinition,
    ) -> Result<ActorTicket, ForgeError> {
        let (body, fingerprint) = self.forge_payload(definition)?;
        let deadline = Instant::now() + self.limits.deadline;
        let url = self.forge_url(&["v2", "actors"])?;
        let created: CreatedActor = self
            .forge_request(
                Method::POST,
                url,
                Some(body),
                ForgeStage::CreateActor,
                deadline,
            )
            .await
            .map_err(|e| contextual(e, None, None, true))?;
        let actor_id = self.safe_forge_id(&created.id);
        let validation = (|| {
            if actor_id.is_none()
                || created.name != definition.name
                || created.is_public
                || created
                    .actor_permission_level
                    .as_ref()
                    .is_some_and(|level| level != "LIMITED_PERMISSIONS")
                || created.versions.len() != 1
            {
                return Err(failure(
                    ForgeErrorKind::ProvenanceMismatch,
                    ForgeStage::CreateActor,
                ));
            }
            validate_source(
                definition,
                &created.versions[0],
                false,
                ForgeStage::CreateActor,
            )?;
            Ok(())
        })();
        validation.map_err(|e| contextual(e, actor_id.as_deref(), None, true))?;
        Ok(ActorTicket {
            schema_version: 1,
            actor_id: created.id,
            version_number: definition.version_number.clone(),
            definition_sha256: fingerprint,
        })
    }

    /// Rechecks the stored source version before starting one new build. Persist
    /// the returned ticket; use `resume_build` instead of submitting again.
    pub async fn start_build(
        &self,
        definition: &ActorDefinition,
        actor: &ActorTicket,
    ) -> Result<BuildTicket, ForgeError> {
        self.validate_actor_ticket(definition, actor)?;
        let deadline = Instant::now() + self.limits.deadline;
        let version_url = self.forge_url(&[
            "v2",
            "actors",
            &actor.actor_id,
            "versions",
            &actor.version_number,
        ])?;
        let version: VersionSnapshot = self
            .forge_request(
                Method::GET,
                version_url,
                None,
                ForgeStage::ReadVersion,
                deadline,
            )
            .await
            .map_err(|e| contextual(e, Some(&actor.actor_id), None, false))?;
        validate_source(definition, &version, true, ForgeStage::ReadVersion)
            .map_err(|e| contextual(e, Some(&actor.actor_id), None, false))?;
        let mut url = self.forge_url(&["v2", "actors", &actor.actor_id, "builds"])?;
        url.query_pairs_mut()
            .append_pair("version", &actor.version_number)
            .append_pair("useCache", "false")
            .append_pair("betaPackages", "false")
            .append_pair("waitForFinish", "0");
        let build: CloudBuild = self
            .forge_request(Method::POST, url, None, ForgeStage::StartBuild, deadline)
            .await
            .map_err(|e| contextual(e, Some(&actor.actor_id), None, true))?;
        let build_id = self.safe_forge_id(&build.id);
        self.validate_cloud_build(
            definition,
            &actor.actor_id,
            None,
            None,
            &build,
            ForgeStage::StartBuild,
        )
        .map_err(|e| contextual(e, Some(&actor.actor_id), build_id.as_deref(), true))?;
        Ok(BuildTicket {
            schema_version: 1,
            actor_id: actor.actor_id.clone(),
            build_id: build.id,
            build_number: build.build_number,
            version_number: actor.version_number.clone(),
            definition_sha256: actor.definition_sha256.clone(),
        })
    }

    /// Polls only the previously submitted build. A timeout does not stop the
    /// remote build; the same trusted ticket can be resumed again later.
    pub async fn resume_build(
        &self,
        definition: &ActorDefinition,
        actor: &ActorTicket,
        ticket: &BuildTicket,
    ) -> Result<BuiltActor, ForgeError> {
        self.validate_actor_ticket(definition, actor)?;
        ticket.validate_for(definition, actor)?;
        if self.contains_credential(&ticket.build_id) {
            return Err(failure(
                ForgeErrorKind::TicketMismatch,
                ForgeStage::Configuration,
            ));
        }
        let deadline = Instant::now() + self.limits.deadline;
        let action = async {
            loop {
                let url = self.forge_url(&["v2", "actor-builds", &ticket.build_id])?;
                let build: CloudBuild = self
                    .forge_request(Method::GET, url, None, ForgeStage::PollBuild, deadline)
                    .await?;
                if self.validate_cloud_build(
                    definition,
                    &actor.actor_id,
                    Some(&ticket.build_id),
                    Some(&ticket.build_number),
                    &build,
                    ForgeStage::PollBuild,
                )? {
                    return Ok(BuiltActor {
                        actor_id: actor.actor_id.clone(),
                        build_id: ticket.build_id.clone(),
                        build_number: ticket.build_number.clone(),
                        version_number: actor.version_number.clone(),
                        definition_sha256: actor.definition_sha256.clone(),
                    });
                }
                timeout_at(deadline, sleep(self.limits.poll_interval))
                    .await
                    .map_err(|_| {
                        failure(ForgeErrorKind::DeadlineExceeded, ForgeStage::PollBuild)
                    })?;
            }
        };
        action
            .await
            .map_err(|e| contextual(e, Some(&actor.actor_id), Some(&ticket.build_id), true))
    }

    fn forge_payload(&self, definition: &ActorDefinition) -> Result<(Vec<u8>, String), ForgeError> {
        let payload = definition_payload(definition, self.limits.max_input_bytes)?;
        if self.contains_credential(&definition.name)
            || definition.source_files.iter().any(|file| {
                self.contains_credential(&file.name) || self.contains_credential(&file.content)
            })
        {
            return Err(failure(
                ForgeErrorKind::InvalidDefinition,
                ForgeStage::Configuration,
            ));
        }
        Ok(payload)
    }

    fn validate_actor_ticket(
        &self,
        definition: &ActorDefinition,
        actor: &ActorTicket,
    ) -> Result<(), ForgeError> {
        self.forge_payload(definition)?;
        actor.validate_for(definition)?;
        if self.contains_credential(&actor.actor_id) {
            return Err(failure(
                ForgeErrorKind::TicketMismatch,
                ForgeStage::Configuration,
            ));
        }
        Ok(())
    }

    fn safe_forge_id(&self, id: &str) -> Option<String> {
        (valid_id(id) && !self.contains_credential(id)).then(|| id.to_owned())
    }

    fn validate_cloud_build(
        &self,
        definition: &ActorDefinition,
        actor_id: &str,
        expected_id: Option<&str>,
        expected_number: Option<&str>,
        build: &CloudBuild,
        stage: ForgeStage,
    ) -> Result<bool, ForgeError> {
        if self.safe_forge_id(&build.id).is_none()
            || build.act_id != actor_id
            || expected_id.is_some_and(|id| id != build.id)
            || expected_number.is_some_and(|number| number != build.build_number)
            || !build_number(&build.build_number, &definition.version_number)
        {
            return Err(failure(ForgeErrorKind::ProvenanceMismatch, stage));
        }
        let succeeded = match build.status.as_str() {
            "SUCCEEDED" => true,
            "READY" | "RUNNING" | "TIMING-OUT" | "ABORTING" => false,
            "FAILED" | "TIMED-OUT" | "ABORTED" => {
                return Err(failure(ForgeErrorKind::BuildFailed, stage))
            }
            _ => return Err(failure(ForgeErrorKind::UnknownBuildStatus, stage)),
        };
        if let Some(snapshot) = &build.act_version {
            validate_source(definition, snapshot, succeeded, stage)?;
        } else if succeeded {
            return Err(failure(ForgeErrorKind::ProvenanceMismatch, stage));
        }
        Ok(succeeded)
    }

    fn forge_url(&self, path: &[&str]) -> Result<Url, ForgeError> {
        self.url(path)
            .map_err(|_| failure(ForgeErrorKind::InvalidDefinition, ForgeStage::Configuration))
    }

    async fn forge_request<T: DeserializeOwned>(
        &self,
        method: Method,
        url: Url,
        body: Option<Vec<u8>>,
        stage: ForgeStage,
        deadline: Instant,
    ) -> Result<T, ForgeError> {
        let response = self
            .request(method, url, body, Stage::Build, deadline)
            .await
            .map_err(|error| {
                let kind = match error.kind {
                    ErrorKind::Transport => ForgeErrorKind::Transport,
                    ErrorKind::HttpStatus(status) => ForgeErrorKind::HttpStatus(status),
                    ErrorKind::DeadlineExceeded => ForgeErrorKind::DeadlineExceeded,
                    ErrorKind::ResponseTooLarge => ForgeErrorKind::ResponseTooLarge,
                    _ => ForgeErrorKind::MalformedResponse,
                };
                failure(kind, stage)
            })?;
        decode_json::<Envelope<T>>(&response.bytes)
            .map(|data| data.data)
            .map_err(|_| failure(ForgeErrorKind::MalformedResponse, stage))
    }
}

fn definition_payload(
    definition: &ActorDefinition,
    max_bytes: usize,
) -> Result<(Vec<u8>, String), ForgeError> {
    let invalid = || failure(ForgeErrorKind::InvalidDefinition, ForgeStage::Configuration);
    if !actor_name(&definition.name)
        || !version_number(&definition.version_number)
        || definition.source_files.is_empty()
        || definition.source_files.len() > 256
    {
        return Err(invalid());
    }
    let mut files = BTreeMap::new();
    let mut content_bytes = 0usize;
    for file in &definition.source_files {
        if !source_path(&file.name)
            || file.content.len() > max_bytes.saturating_sub(content_bytes)
            || files
                .insert(file.name.as_str(), file.content.as_str())
                .is_some()
        {
            return Err(invalid());
        }
        content_bytes += file.content.len();
    }
    for name in files.keys() {
        let mut parts = name.split('/');
        let mut prefix = String::new();
        while let Some(part) = parts.next() {
            if !prefix.is_empty() {
                prefix.push('/');
            }
            prefix.push_str(part);
            if parts.clone().next().is_some() && files.contains_key(prefix.as_str()) {
                return Err(invalid());
            }
        }
    }
    let source_files: Vec<_> = files
        .into_iter()
        .map(|(name, content)| json!({"name":name,"content":content,"format":"TEXT"}))
        .collect();
    let payload = json!({
        "name":definition.name, "isPublic":false, "actorPermissionLevel":"LIMITED_PERMISSIONS",
        "versions":[{"versionNumber":definition.version_number,"sourceType":"SOURCE_FILES",
            "sourceFiles":source_files,"envVars":[],"applyEnvVarsToBuild":false,"buildTag":null}]
    });
    let mut writer = BoundedWriter {
        bytes: Vec::new(),
        limit: max_bytes,
    };
    serde_json::to_writer(&mut writer, &payload).map_err(|_| invalid())?;
    Ok((writer.bytes, json_sha256(&payload)))
}

fn validate_source(
    definition: &ActorDefinition,
    snapshot: &VersionSnapshot,
    require_files: bool,
    stage: ForgeStage,
) -> Result<(), ForgeError> {
    let invalid = || failure(ForgeErrorKind::ProvenanceMismatch, stage);
    if snapshot.version_number != definition.version_number
        || snapshot.source_type != "SOURCE_FILES"
        || snapshot
            .env_vars
            .as_ref()
            .is_some_and(|vars| !vars.is_empty())
        || snapshot.apply_env_vars_to_build == Some(true)
    {
        return Err(invalid());
    }
    if let Some(files) = &snapshot.source_files {
        if files.len() != definition.source_files.len() {
            return Err(invalid());
        }
        let expected: BTreeMap<_, _> = definition
            .source_files
            .iter()
            .map(|file| (file.name.as_str(), file.content.as_str()))
            .collect();
        let mut observed = BTreeMap::new();
        for file in files {
            let Some(expected_content) = expected.get(file.name.as_str()) else {
                return Err(invalid());
            };
            if file.folder == Some(true)
                || file
                    .format
                    .as_deref()
                    .is_some_and(|format| format != "TEXT")
                || observed
                    .insert(file.name.as_str(), file.content.as_deref())
                    .is_some()
                || file.content.as_deref() != Some(*expected_content)
            {
                return Err(invalid());
            }
        }
    } else if require_files {
        return Err(invalid());
    }
    Ok(())
}

fn actor_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        && name.as_bytes()[0].is_ascii_alphanumeric()
        && name.as_bytes()[name.len() - 1].is_ascii_alphanumeric()
}
fn version_number(version: &str) -> bool {
    if version.len() > 5 {
        return false;
    }
    let parts: Vec<_> = version.split('.').collect();
    parts.len() == 2
        && parts.iter().all(|part| {
            !part.is_empty()
                && part.len() <= 2
                && part.bytes().all(|b| b.is_ascii_digit())
                && (part.len() == 1 || !part.starts_with('0'))
        })
}
fn build_number(build: &str, version: &str) -> bool {
    if build.len() > 11 {
        return false;
    }
    let Some((prefix, sequence)) = build.rsplit_once('.') else {
        return false;
    };
    prefix == version
        && !sequence.is_empty()
        && sequence.len() <= 5
        && !sequence.starts_with('0')
        && sequence.bytes().all(|b| b.is_ascii_digit())
}
fn source_path(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 256
        && name.split('/').all(|part| {
            !part.is_empty()
                && part != "."
                && part != ".."
                && part
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
        })
}
fn failure(kind: ForgeErrorKind, stage: ForgeStage) -> ForgeError {
    ForgeError {
        kind,
        stage,
        actor_id: None,
        build_id: None,
        submission_may_have_occurred: false,
    }
}
fn contextual(
    mut error: ForgeError,
    actor_id: Option<&str>,
    build_id: Option<&str>,
    submitted: bool,
) -> ForgeError {
    error.actor_id = actor_id.map(str::to_owned);
    error.build_id = build_id.map(str::to_owned);
    error.submission_may_have_occurred = submitted;
    error
}
