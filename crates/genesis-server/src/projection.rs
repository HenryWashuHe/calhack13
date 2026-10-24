//! Elastic projection of this organization's state.
//!
//! The kernel store is authoritative. After commits, a background task writes
//! the current organization records to this organization's Elastic evidence
//! index. Projection failures are logged and retried; they never block or
//! roll back a kernel change. Search results are candidates; callers re-read
//! authoritative records before acting on them.

use crate::app::AppState;
use genesis_elastic::{org_docs, ElasticClient};
use serde::Serialize;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::broadcast::error::RecvError;

/// Observable projection status, served at `GET /api/v1/projection`.
#[derive(Debug, Clone, Default, Serialize, PartialEq, Eq)]
pub struct ProjectionStatus {
    pub configured: bool,
    /// Last organization revision fully written to Elastic.
    pub projected_revision: Option<u64>,
    pub documents: usize,
    /// Sanitized error of the last failed attempt, cleared on success.
    pub last_error: Option<String>,
}

#[derive(Clone, Default)]
pub struct Projection {
    pub client: Option<ElasticClient>,
    status: Arc<Mutex<ProjectionStatus>>,
}

impl Projection {
    pub fn disabled() -> Self {
        Self::default()
    }

    pub fn new(client: ElasticClient) -> Self {
        Self {
            client: Some(client),
            status: Arc::new(Mutex::new(ProjectionStatus {
                configured: true,
                ..Default::default()
            })),
        }
    }

    pub fn status(&self) -> ProjectionStatus {
        self.status
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone()
    }

    fn update(&self, f: impl FnOnce(&mut ProjectionStatus)) {
        f(&mut self.status.lock().unwrap_or_else(|p| p.into_inner()));
    }
}

/// Project the current state once if it is newer than the last projection.
/// Returns the revision projected, if any work was done.
pub async fn project_once(app: &AppState) -> Result<Option<u64>, String> {
    let Some(client) = &app.projection.client else {
        return Ok(None);
    };
    let state = app.current_state().map_err(|e| e.message)?;
    let done = app.projection.status().projected_revision;
    if done.is_some_and(|r| r >= state.revision) {
        return Ok(None);
    }
    let org = app.org_id.to_string();
    let docs = org_docs(&state);
    let result = async {
        if done.is_none() {
            client.ensure_indices(&org).await?;
        }
        client.index_evidence(&org, &docs).await
    }
    .await;
    match result {
        Ok(n) => {
            app.projection.update(|s| {
                s.projected_revision = Some(state.revision);
                s.documents = n;
                s.last_error = None;
            });
            Ok(Some(state.revision))
        }
        Err(e) => {
            let msg = e.to_string();
            app.projection.update(|s| s.last_error = Some(msg.clone()));
            Err(msg)
        }
    }
}

/// Keep the projection current: project at startup, after each event burst,
/// and retry failures with backoff. Does nothing when Elastic is not configured.
pub fn spawn(app: AppState) -> Option<tokio::task::JoinHandle<()>> {
    app.projection.client.as_ref()?;
    let mut rx = app.events.subscribe();
    Some(tokio::spawn(async move {
        let mut backoff = Duration::from_millis(500);
        loop {
            match project_once(&app).await {
                Ok(_) => backoff = Duration::from_millis(500),
                Err(error) => {
                    tracing::warn!(%error, "elastic projection failed; will retry");
                    tokio::time::sleep(backoff).await;
                    backoff = (backoff * 2).min(Duration::from_secs(30));
                    continue;
                }
            }
            // Wait for the next commit, then let a burst settle.
            match rx.recv().await {
                Ok(_) | Err(RecvError::Lagged(_)) => {
                    tokio::time::sleep(Duration::from_millis(200)).await;
                    while !rx.is_empty() {
                        let _ = rx.try_recv();
                    }
                }
                Err(RecvError::Closed) => break,
            }
        }
    }))
}
