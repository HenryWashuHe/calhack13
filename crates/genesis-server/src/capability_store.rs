//! Local immutable installation pins and acceptance references. Oracle answers
//! remain in backend configuration; only IDs of backend-collected runs persist.

use crate::store::{Store, StoreError, StoreResult};
use genesis_core::capability::CapabilityManifest;
use genesis_core::ids::{InstallationId, JobId};
use rusqlite::{params, OptionalExtension, Transaction};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LocalAcceptance {
    pub suite_id: String,
    pub job_ids: Vec<JobId>,
    pub report_ref: String,
}

impl Store {
    pub fn init_capability_pins(&self) -> StoreResult<()> {
        self.lock().execute_batch(
            "CREATE TABLE IF NOT EXISTS capability_pins (
            installation_id TEXT PRIMARY KEY, manifest_json TEXT NOT NULL,
            acceptance_json TEXT);
            CREATE TABLE IF NOT EXISTS capability_check_batches (
                client_request_id TEXT PRIMARY KEY, request_sha256 TEXT NOT NULL, job_ids_json TEXT NOT NULL);",
        )?;
        Ok(())
    }

    pub fn capability_manifest(&self, id: &InstallationId) -> StoreResult<CapabilityManifest> {
        let text: Option<String> = self
            .lock()
            .query_row(
                "SELECT manifest_json FROM capability_pins WHERE installation_id=?1",
                params![id.as_str()],
                |row| row.get(0),
            )
            .optional()?;
        Ok(serde_json::from_str(&text.ok_or_else(|| {
            StoreError::NotFound(format!("installation pin {id}"))
        })?)?)
    }

    pub fn capability_acceptance(
        &self,
        id: &InstallationId,
    ) -> StoreResult<Option<LocalAcceptance>> {
        let text: Option<Option<String>> = self
            .lock()
            .query_row(
                "SELECT acceptance_json FROM capability_pins WHERE installation_id=?1",
                params![id.as_str()],
                |row| row.get(0),
            )
            .optional()?;
        text.flatten()
            .map(|text| serde_json::from_str(&text).map_err(Into::into))
            .transpose()
    }
}

pub fn insert_pin(
    tx: &Transaction<'_>,
    id: &InstallationId,
    manifest: &CapabilityManifest,
) -> StoreResult<()> {
    tx.execute(
        "INSERT INTO capability_pins (installation_id,manifest_json) VALUES (?1,?2)",
        params![id.as_str(), serde_json::to_string(manifest)?],
    )?;
    Ok(())
}

pub fn save_acceptance(
    tx: &Transaction<'_>,
    id: &InstallationId,
    proof: &LocalAcceptance,
) -> StoreResult<()> {
    let count = tx.execute("UPDATE capability_pins SET acceptance_json=?2 WHERE installation_id=?1 AND acceptance_json IS NULL",
        params![id.as_str(),serde_json::to_string(proof)?])?;
    if count != 1 {
        return Err(StoreError::NotFound(format!(
            "unaccepted installation pin {id}"
        )));
    }
    Ok(())
}

impl Store {
    pub fn find_capability_job(&self, key: &str) -> StoreResult<Option<genesis_core::jobs::Job>> {
        let text: Option<String> = self
            .lock()
            .query_row(
                "SELECT job_json FROM jobs WHERE client_request_id=?1",
                params![key],
                |row| row.get(0),
            )
            .optional()?;
        text.map(|text| serde_json::from_str(&text).map_err(Into::into))
            .transpose()
    }
}

pub fn insert_development_job(
    tx: &Transaction<'_>,
    key: &str,
    job: &genesis_core::jobs::Job,
) -> StoreResult<()> {
    tx.execute("INSERT INTO jobs (job_id,client_request_id,status,job_json,created_at,updated_at) VALUES (?1,?2,?3,?4,?5,?6)",
        params![job.job_id.as_str(),key,format!("{:?}",job.status),serde_json::to_string(job)?,job.created_at.to_rfc3339(),job.updated_at.to_rfc3339()])?;
    Ok(())
}

#[derive(Clone, Debug)]
pub struct CheckBatch {
    pub request_sha256: String,
    pub job_ids: Vec<JobId>,
}

impl Store {
    pub fn capability_check_batch(&self, key: &str) -> StoreResult<Option<CheckBatch>> {
        let row: Option<(String,String)> = self.lock().query_row(
            "SELECT request_sha256,job_ids_json FROM capability_check_batches WHERE client_request_id=?1",
            params![key], |row| Ok((row.get(0)?,row.get(1)?))).optional()?;
        row.map(|(request_sha256, encoded)| {
            Ok(CheckBatch {
                request_sha256,
                job_ids: serde_json::from_str(&encoded)?,
            })
        })
        .transpose()
    }
}

pub fn insert_check_batch(
    tx: &Transaction<'_>,
    key: &str,
    digest: &str,
    jobs: &[JobId],
) -> StoreResult<()> {
    tx.execute("INSERT INTO capability_check_batches (client_request_id,request_sha256,job_ids_json) VALUES (?1,?2,?3)",
        params![key,digest,serde_json::to_string(jobs)?])?;
    Ok(())
}
