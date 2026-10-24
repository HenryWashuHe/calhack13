//! Pure projections from authoritative kernel records to search documents.
//! Document IDs are deterministic, so projecting the same state twice is
//! idempotent and a later revision overwrites the earlier projection.

use crate::{CapabilityDoc, DocKind, EvidenceDoc};
use genesis_core::capability::CapabilityManifest;
use genesis_core::model::{Executor, OrgState, Owner};
use serde::Serialize;
use std::collections::BTreeMap;

/// Serialized name of a unit enum (e.g. `"ACTIVE"`).
fn name<T: Serialize>(v: &T) -> Option<String> {
    serde_json::to_value(v)
        .ok()
        .and_then(|v| v.as_str().map(str::to_owned))
}

fn owner_ref(owner: &Owner) -> String {
    match owner {
        Owner::MissionController => "mission_controller".into(),
        Owner::Role(r) => r.to_string(),
    }
}

/// Every searchable record of one organization at its current revision.
pub fn org_docs(state: &OrgState) -> Vec<EvidenceDoc> {
    let org = state.org_id.to_string();
    let rev = state.revision;
    let doc = |kind: DocKind,
               id: String,
               title: String,
               body: String,
               mission: Option<String>,
               status: Option<String>,
               refs: BTreeMap<String, String>| EvidenceDoc {
        doc_id: format!("{org}:{}:{id}", name(&kind).unwrap_or_default()),
        org_id: org.clone(),
        kind,
        title,
        body,
        mission_id: mission,
        status,
        revision: rev,
        refs,
    };
    let mut out = Vec::new();

    for m in state.missions.values() {
        let mut body = m.constraints.join("\n");
        if !m.success_checks.is_empty() {
            body.push_str("\nSuccess checks: ");
            body.push_str(&m.success_checks.join("; "));
        }
        out.push(doc(
            DocKind::Mission,
            m.mission_id.to_string(),
            m.goal.clone(),
            body,
            Some(m.mission_id.to_string()),
            name(&m.status),
            BTreeMap::new(),
        ));
    }

    for o in state.observations.values() {
        let mut refs = BTreeMap::from([("source".to_owned(), o.source.clone())]);
        if let Some(e) = &o.evidence_ref {
            refs.insert("evidence_ref".into(), e.clone());
        }
        out.push(doc(
            DocKind::Observation,
            o.observation_id.to_string(),
            o.summary.clone(),
            format!("Observed {} from {}", o.observed_at.to_rfc3339(), o.source),
            Some(o.mission_id.to_string()),
            None,
            refs,
        ));
    }

    for r in state.requirements.values() {
        let mut refs = BTreeMap::from([("owner".to_owned(), owner_ref(&r.owner))]);
        for (i, ev) in r.triggering_evidence.iter().enumerate() {
            refs.insert(format!("evidence_{i}"), ev.to_string());
        }
        out.push(doc(
            DocKind::Requirement,
            r.requirement_id.to_string(),
            r.summary.clone(),
            format!("Acceptance: {}", r.acceptance),
            Some(r.mission_id.to_string()),
            name(&r.status),
            refs,
        ));
    }

    for r in state.responsibilities.values() {
        let mut refs = BTreeMap::from([("owner".to_owned(), owner_ref(&r.owner))]);
        if let Some(req) = &r.requirement_id {
            refs.insert("requirement".into(), req.to_string());
        }
        match &r.executor {
            Some(Executor::Role(id)) => {
                refs.insert("executor".into(), id.to_string());
            }
            Some(Executor::Contractor(id)) => {
                refs.insert("contractor".into(), id.to_string());
            }
            _ => {}
        }
        out.push(doc(
            DocKind::Responsibility,
            r.responsibility_id.to_string(),
            r.purpose.clone(),
            format!("Trigger: {}\nTermination: {}", r.trigger, r.termination),
            Some(r.mission_id.to_string()),
            name(&r.status),
            refs,
        ));
    }

    for r in state.roles.values() {
        let mut refs = BTreeMap::new();
        if let Some(b) = &r.binding {
            refs.insert("provider".into(), name(&b.provider).unwrap_or_default());
            refs.insert("provider_agent_id".into(), b.provider_agent_id.clone());
        }
        if let Some(up) = &r.reports_to {
            refs.insert("reports_to".into(), up.to_string());
        }
        out.push(doc(
            DocKind::Role,
            r.role_id.to_string(),
            r.name.clone(),
            r.charter.clone(),
            None,
            name(&r.state),
            refs,
        ));
    }
    out
}

/// Searchable summary of a capability manifest.
pub fn capability_doc(m: &CapabilityManifest) -> CapabilityDoc {
    CapabilityDoc {
        doc_id: format!("{}@{}", m.capability_id, m.version),
        capability_id: m.capability_id.to_string(),
        version: m.version.clone(),
        publisher: m.publisher.to_string(),
        intent: m.intent.clone(),
        applicability_conditions: m.applicability_conditions.clone(),
        known_limitations: m.known_limitations.clone(),
        required_permissions: m.required_permissions.iter().cloned().collect(),
        source_digest: m.source_digest.clone(),
        state: name(&m.state).unwrap_or_default(),
    }
}
