//! Watches an organization's event log and plans once per new observation.
//!
//! Idempotent across restarts: an observation that already has a terminal
//! planner decision (`change_proposed`, `no_change`, `escalated`) in the event
//! log is skipped.

use crate::agent::{PlanOutcome, Planner};
use genesis_core::events::{EventKind, PlannerVerdict};
use genesis_core::ids::ObservationId;
use std::collections::BTreeSet;
use std::time::Duration;

pub struct Runner {
    pub planner: Planner,
    pub poll: Duration,
    cursor: u64,
    done: BTreeSet<ObservationId>,
    pending: Vec<ObservationId>,
}

impl Runner {
    pub fn new(planner: Planner, poll: Duration) -> Self {
        Self {
            planner,
            poll,
            cursor: 0,
            done: BTreeSet::new(),
            pending: Vec::new(),
        }
    }

    /// Read new events, then plan for every observation still undecided.
    /// Returns the outcomes produced in this pass.
    pub async fn tick(&mut self) -> anyhow::Result<Vec<(ObservationId, PlanOutcome)>> {
        loop {
            let batch = self.planner.kernel.events_after(self.cursor, 500).await?;
            if batch.is_empty() {
                break;
            }
            for e in &batch {
                self.cursor = e.seq;
                match &e.kind {
                    EventKind::ObservationRecorded { observation_id, .. } => {
                        if !self.done.contains(observation_id)
                            && !self.pending.contains(observation_id)
                        {
                            self.pending.push(observation_id.clone());
                        }
                    }
                    EventKind::PlannerDecision {
                        observation_id,
                        verdict,
                        ..
                    } if *verdict != PlannerVerdict::Rejected => {
                        self.done.insert(observation_id.clone());
                        self.pending.retain(|o| o != observation_id);
                    }
                    _ => {}
                }
            }
        }
        let mut outcomes = Vec::new();
        for obs in std::mem::take(&mut self.pending) {
            if self.done.contains(&obs) {
                continue;
            }
            tracing::info!(observation = %obs, "planning");
            match self.planner.plan(&obs).await {
                Ok(outcome) => {
                    tracing::info!(observation = %obs, ?outcome, "planned");
                    self.done.insert(obs.clone());
                    outcomes.push((obs, outcome));
                }
                Err(e) => {
                    tracing::warn!(observation = %obs, error = %e, "planning failed; will retry");
                    self.pending.push(obs);
                }
            }
        }
        Ok(outcomes)
    }

    pub async fn run_forever(mut self) -> anyhow::Result<()> {
        loop {
            if let Err(e) = self.tick().await {
                tracing::warn!(error = %e, "planner tick failed");
            }
            tokio::time::sleep(self.poll).await;
        }
    }
}
