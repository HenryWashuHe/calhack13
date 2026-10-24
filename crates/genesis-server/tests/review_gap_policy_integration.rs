//! Henry's Claude, pressure test of #56 against the #49 kernel contract: a gap
//! committed by the real engine (as the planner would produce it) through
//! `GapAcquisitionPolicy::materialize`.

use chrono::Utc;
use genesis_core::capability::StaticCatalog;
use genesis_core::change::*;
use genesis_core::engine::*;
use genesis_core::ids::*;
use genesis_core::model::*;
use genesis_forge::PublicExample;
use genesis_server::capability_gap_policy::{
    GapAcquisitionPolicy, GapAcquisitionRule, GapPolicyError,
};
use std::collections::BTreeMap;

const DRIFT: &str = "https://portal.example/layout-drift/venues/main-hall.html";
const OTHER: &str = "https://portal.example/layout-drift/venues/lerner.html";

/// Commit AssignResponsibility + RequestCapability with `sources`; return the gap.
fn committed_gap(sources: &[&str]) -> CapabilityGap {
    let org = OrgState::new(
        OrgId::new("org_a"),
        "Org A",
        genesis_server::demo_constitution(),
    );
    let spec: MissionBootstrap = serde_json::from_value(serde_json::json!({
        "goal": "Run the campus hack night for 120 attendees on Nov 14",
        "roles": [{"key": "venue", "name": "Venue operations", "charter": "Owns venue booking",
                   "authority": {"scopes": ["capability.venue_lookup"], "budget_cents": 1000}}],
        "responsibilities": [{"purpose": "Keep a venue booked", "owner": "venue"}]
    }))
    .unwrap();
    let (o, mission) = bootstrap_mission(&org, &spec, Utc::now()).unwrap();
    let (o, obs) = record_observation(
        &o.state,
        &mission,
        "portal",
        "Venue portal pages changed layout",
        Some(DRIFT.into()),
        Utc::now(),
    )
    .unwrap();
    let (o, req) = record_requirement(
        &o.state,
        &mission,
        "Read venue availability from the changed layout",
        std::slice::from_ref(&obs),
        "availability read",
    )
    .unwrap();
    let state = o.state;
    let role = state.roles.values().next().unwrap().role_id.clone();
    let proposal = OrgChangeProposal {
        expected_org_revision: state.revision,
        trigger_evidence_ids: vec![obs],
        rationale: "owner lacks a reader".into(),
        budget_limit_cents: Some(0),
        operations: vec![
            OrgOp::AssignResponsibility {
                target: ResponsibilityTarget::FromRequirement {
                    key: "read".into(),
                    requirement_id: req.clone(),
                    purpose: "Read availability".into(),
                    trigger: "portal changes".into(),
                    termination: "event held".into(),
                },
                owner: RoleRef::existing(role.clone()),
            },
            OrgOp::RequestCapability {
                key: "reader".into(),
                requirement_id: req,
                role: RoleRef::existing(role),
                responsibility: ResponsibilityRef::New { key: "read".into() },
                capability_id: CapabilityId::new("venue-portal-reader"),
                purpose: "Read venue availability".into(),
                observed_sources: sources.iter().map(|s| s.to_string()).collect(),
            },
        ],
    };
    let alloc = KeyAllocation::allocate(&proposal);
    let out = commit(
        &state,
        &proposal,
        &alloc,
        &BTreeMap::new(),
        &StaticCatalog::default(),
        Utc::now(),
    )
    .unwrap_or_else(|e| panic!("kernel rejected {sources:?}: {e:?}"));
    out.state.capability_gaps.into_values().next().unwrap()
}

fn policy() -> GapAcquisitionPolicy {
    GapAcquisitionPolicy {
        rules: vec![GapAcquisitionRule {
            capability_id: "venue-portal-reader".into(),
            suite_id: "venue-portal-suite".into(),
            actor_name_prefix: "genesis-venue".into(),
            allowed_source_urls: vec![DRIFT.into(), OTHER.into()],
            examples: vec![
                PublicExample {
                    url: DRIFT.into(),
                    html: "<html>fixed main hall</html>".into(),
                },
                PublicExample {
                    url: OTHER.into(),
                    html: "<html>fixed lerner</html>".into(),
                },
            ],
            repair: true,
        }],
    }
}

#[test]
fn a_kernel_committed_gap_materializes_with_its_frozen_binding() {
    let gap = committed_gap(&[DRIFT]);
    let org = OrgId::new("org_a");
    let a = policy().materialize(&org, &gap).unwrap();
    assert_eq!(a.client_request_id, gap.gap_id.as_str());
    assert_eq!(a.mission_id, gap.mission_id);
    assert_eq!(a.requirement_id, gap.requirement_id);
    assert_eq!(a.role_id, gap.role_id);
    assert_eq!(a.responsibility_id, gap.responsibility_id);
    assert_eq!(a.allowed_source_urls, vec![DRIFT.to_string()]);
    assert_eq!(a.examples.len(), 1, "only the requested source's example");
    assert!(a.repair.is_some());
    // Restart: identical intent, so the store's intent hash matches.
    let b = policy().materialize(&org, &gap).unwrap();
    assert_eq!(serde_json::to_value(&a).ok(), serde_json::to_value(&b).ok());
}

/// The kernel accepts any source on the evidence's origin. The policy matches
/// exact strings. URLs that are the same page to a human but not the same
/// string pass the kernel and are refused by the policy (fail closed).
#[test]
fn same_origin_near_misses_pass_the_kernel_but_not_the_policy() {
    let org = OrgId::new("org_a");
    for near in [
        "https://portal.example/layout-drift/venues/main-hall.html#top",
        "https://portal.example/layout-drift/venues/main-hall.html?",
        "https://portal.example/layout-drift/venues/./main-hall.html",
        "https://portal.example/layout-drift/venues/main%2Dhall.html",
        "HTTPS://portal.example/layout-drift/venues/main-hall.html",
    ] {
        let gap = committed_gap(&[near]);
        assert!(
            matches!(
                policy().materialize(&org, &gap),
                Err(GapPolicyError::SourceNotAllowed)
            ),
            "{near}"
        );
    }
}

#[test]
fn a_final_gap_still_materializes_so_the_driver_must_gate_on_status() {
    // Documented in #56: lifecycle eligibility is the driver's job.
    let mut gap = committed_gap(&[DRIFT]);
    gap.status = CapabilityGapStatus::Refused {
        reason: "declined".into(),
        acquisition_id: None,
    };
    assert!(policy().materialize(&OrgId::new("org_a"), &gap).is_ok());
}
