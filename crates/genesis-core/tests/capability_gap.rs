//! `RequestCapability`: a committed, evidence-bound capability need, and the
//! only transitions an acquisition driver may apply to it.

use chrono::Utc;
use genesis_core::capability::StaticCatalog;
use genesis_core::change::*;
use genesis_core::engine::*;
use genesis_core::engine::{MAX_LIVE_CAPABILITY_GAPS, MAX_LIVE_GAPS_PER_REQUIREMENT};
use genesis_core::events::EventKind;
use genesis_core::ids::*;
use genesis_core::lifecycle::CapabilityState;
use genesis_core::model::*;
use std::collections::BTreeMap;

const PORTAL: &str = "https://portal.example/org-a/venues/index.html";

struct Demo {
    state: OrgState,
    lead: RoleId,
    observation: ObservationId,
    requirement: RequirementId,
}

fn demo(evidence_ref: Option<&str>) -> Demo {
    let now = Utc::now();
    let state = OrgState::new(
        OrgId::new("org_a"),
        "Org A",
        Constitution {
            scopes: ["mission.coordinate", "ledger.reserve_venue"]
                .into_iter()
                .map(String::from)
                .collect(),
            budget_cents: 500_000,
            max_active_roles: 6,
            max_active_contracts: 2,
        },
    );
    let spec = MissionBootstrap {
        goal: "Run the campus event".into(),
        constraints: vec![],
        deadline: None,
        success_checks: vec![],
        roles: vec![BootstrapRole {
            key: "lead".into(),
            name: "Lead".into(),
            charter: "Owns the plan".into(),
            authority: AuthorityGrant::new(["mission.coordinate"], 10_000),
            reports_to: None,
        }],
        responsibilities: vec![BootstrapResponsibility {
            purpose: "Coordinate".into(),
            owner: "lead".into(),
            trigger: "events".into(),
            termination: "event ends".into(),
        }],
    };
    let (out, mission) = bootstrap_mission(&state, &spec, now).unwrap();
    let lead = out.state.roles.keys().next().unwrap().clone();
    let (out, observation) = record_observation(
        &out.state,
        &mission,
        "venue-portal",
        "Main hall closed; the portal lists alternatives in an unfamiliar layout",
        evidence_ref.map(String::from),
        now,
    )
    .unwrap();
    let (out, requirement) = record_requirement(
        &out.state,
        &mission,
        "Find a feasible replacement venue",
        std::slice::from_ref(&observation),
        "a venue for 120 is identified from portal data",
    )
    .unwrap();
    Demo {
        state: out.state,
        lead,
        observation,
        requirement,
    }
}

fn assign(d: &Demo) -> OrgOp {
    OrgOp::AssignResponsibility {
        target: ResponsibilityTarget::FromRequirement {
            key: "relocate".into(),
            requirement_id: d.requirement.clone(),
            purpose: "Relocate the event".into(),
            trigger: "venue availability changes".into(),
            termination: "venue reserved".into(),
        },
        owner: RoleRef::existing(d.lead.clone()),
    }
}

fn request(d: &Demo, sources: &[&str]) -> OrgOp {
    OrgOp::RequestCapability {
        key: "venue-reader".into(),
        requirement_id: d.requirement.clone(),
        role: RoleRef::existing(d.lead.clone()),
        responsibility: ResponsibilityRef::New {
            key: "relocate".into(),
        },
        capability_id: CapabilityId::new("venue-availability"),
        purpose: "Extract venue capacity, price, window and availability from the portal".into(),
        observed_sources: sources.iter().map(|s| s.to_string()).collect(),
    }
}

fn proposal(d: &Demo, operations: Vec<OrgOp>) -> OrgChangeProposal {
    OrgChangeProposal {
        expected_org_revision: d.state.revision,
        trigger_evidence_ids: vec![d.observation.clone()],
        rationale: "No installed capability reads this portal".into(),
        budget_limit_cents: Some(0),
        operations,
    }
}

fn run(
    d: &Demo,
    ops: Vec<OrgOp>,
    catalog: &StaticCatalog,
) -> Result<Outcome, Vec<ValidationError>> {
    let p = proposal(d, ops);
    let alloc = KeyAllocation::allocate(&p);
    commit(&d.state, &p, &alloc, &BTreeMap::new(), catalog, Utc::now())
}

fn codes(r: Result<Outcome, Vec<ValidationError>>) -> Vec<ValidationCode> {
    r.expect_err("rejected")
        .into_iter()
        .map(|e| e.code)
        .collect()
}

#[test]
fn a_committed_request_records_one_open_gap_bound_to_its_evidence() {
    let d = demo(Some(PORTAL));
    let out = run(
        &d,
        vec![
            assign(&d),
            request(&d, &["https://portal.example/org-a/venues/cards.html"]),
        ],
        &StaticCatalog::default(),
    )
    .unwrap();
    assert_eq!(out.state.capability_gaps.len(), 1);
    let gap = out.state.capability_gaps.values().next().unwrap();
    assert_eq!(gap.status, CapabilityGapStatus::Open);
    assert_eq!(gap.requirement_id, d.requirement);
    assert_eq!(gap.role_id, d.lead);
    assert_eq!(gap.capability_id.as_str(), "venue-availability");
    assert_eq!(gap.opened_in_revision, out.state.revision);
    assert_eq!(
        out.state.responsibilities[&gap.responsibility_id].purpose,
        "Relocate the event"
    );
    assert!(out.effects.iter().any(
        |e| matches!(e, EventKind::CapabilityGapOpened { gap_id, .. } if *gap_id == gap.gap_id)
    ));
    // The JSON shape the planner and the console see.
    let json = serde_json::to_value(&out.state.capability_gaps).unwrap();
    assert_eq!(json[gap.gap_id.as_str()]["status"]["state"], "open");
}

#[test]
fn sources_must_share_an_origin_with_the_requirement_evidence() {
    let d = demo(Some(PORTAL));
    let r = run(
        &d,
        vec![
            assign(&d),
            request(&d, &["https://elsewhere.example/venues.html"]),
        ],
        &StaticCatalog::default(),
    );
    assert_eq!(codes(r), vec![ValidationCode::UnknownEvidence]);
    let r = run(
        &d,
        vec![assign(&d), request(&d, &["ftp://portal.example/x"])],
        &StaticCatalog::default(),
    );
    assert_eq!(codes(r), vec![ValidationCode::InvalidInput]);
    let r = run(
        &d,
        vec![assign(&d), request(&d, &["https://user@portal.example/x"])],
        &StaticCatalog::default(),
    );
    assert_eq!(codes(r), vec![ValidationCode::InvalidInput]);
    let r = run(
        &d,
        vec![assign(&d), request(&d, &[])],
        &StaticCatalog::default(),
    );
    assert_eq!(codes(r), vec![ValidationCode::InvalidInput]);
    // Evidence without an http(s) reference cannot justify any source.
    let none = demo(Some("fixture://closure"));
    let r = run(
        &none,
        vec![assign(&none), request(&none, &["https://portal.example/a"])],
        &StaticCatalog::default(),
    );
    assert_eq!(codes(r), vec![ValidationCode::UnknownEvidence]);
    // Origins compare case-insensitively on scheme and host.
    let ok = run(
        &d,
        vec![
            assign(&d),
            request(&d, &["HTTPS://Portal.Example/org-a/other.html"]),
        ],
        &StaticCatalog::default(),
    );
    assert!(ok.is_ok(), "{ok:?}");
}

#[test]
fn the_role_must_own_the_responsibility_first() {
    let d = demo(Some(PORTAL));
    // No assignment earlier in the change: the new responsibility key is unknown.
    let r = run(&d, vec![request(&d, &[PORTAL])], &StaticCatalog::default());
    assert!(codes(r).contains(&ValidationCode::UnknownKey));
    // Responsibility owned by someone else.
    let mut ops = vec![assign(&d)];
    let other = OrgOp::CreateRole {
        key: "other".into(),
        name: "Other".into(),
        charter: "Something else".into(),
        authority: AuthorityGrant::new(["mission.coordinate"], 0),
        reports_to: None,
        provider: Provider::Internal,
        justification: RoleJustification::Expertise,
    };
    ops.insert(0, other);
    let OrgOp::RequestCapability {
        key,
        requirement_id,
        responsibility,
        capability_id,
        purpose,
        observed_sources,
        ..
    } = request(&d, &[PORTAL])
    else {
        unreachable!()
    };
    ops.push(OrgOp::RequestCapability {
        key,
        requirement_id,
        role: RoleRef::new_key("other"),
        responsibility,
        capability_id,
        purpose,
        observed_sources,
    });
    let r = run(&d, ops, &StaticCatalog::default());
    assert!(codes(r).contains(&ValidationCode::AccountabilityConflict));
}

#[test]
fn names_purposes_and_installed_capabilities_are_checked() {
    let d = demo(Some(PORTAL));
    let bad_name = |name: &str| {
        let OrgOp::RequestCapability {
            key,
            requirement_id,
            role,
            responsibility,
            purpose,
            observed_sources,
            ..
        } = request(&d, &[PORTAL])
        else {
            unreachable!()
        };
        vec![
            assign(&d),
            OrgOp::RequestCapability {
                key,
                requirement_id,
                role,
                responsibility,
                capability_id: CapabilityId::new(name),
                purpose,
                observed_sources,
            },
        ]
    };
    for name in ["ab", "Venue", "-x-y", "venue availability", &"a".repeat(65)] {
        assert_eq!(
            codes(run(&d, bad_name(name), &StaticCatalog::default())),
            vec![ValidationCode::InvalidInput],
            "{name}"
        );
    }
    // Already installed and approved: attach it instead.
    let cap = CapabilityRef {
        capability_id: CapabilityId::new("venue-availability"),
        version: "1".into(),
    };
    let mut installed = d.state.clone();
    installed.installations.insert(
        InstallationId::new("inst_1"),
        Installation {
            installation_id: InstallationId::new("inst_1"),
            capability: cap.clone(),
            status: genesis_core::lifecycle::InstallationStatus::Accepted,
            configuration: serde_json::Value::Null,
            acceptance_report_ref: None,
            source_digest: "sha256:x".into(),
            installed_at: Utc::now(),
        },
    );
    let d2 = Demo {
        state: installed,
        ..demo(Some(PORTAL))
    };
    let d2 = Demo {
        lead: d.lead.clone(),
        observation: d.observation.clone(),
        requirement: d.requirement.clone(),
        ..d2
    };
    let catalog = StaticCatalog::default().with(cap, CapabilityState::Approved);
    let r = run(&d2, vec![assign(&d2), request(&d2, &[PORTAL])], &catalog);
    let errs = r.expect_err("rejected");
    assert!(
        errs.iter().any(|e| e.message.contains("AttachCapability")),
        "{errs:?}"
    );
}

#[test]
fn one_live_gap_per_requirement_and_capability() {
    let d = demo(Some(PORTAL));
    let out = run(
        &d,
        vec![assign(&d), request(&d, &[PORTAL])],
        &StaticCatalog::default(),
    )
    .unwrap();
    let rsp = out
        .state
        .capability_gaps
        .values()
        .next()
        .unwrap()
        .responsibility_id
        .clone();
    let d2 = Demo {
        state: out.state,
        lead: d.lead.clone(),
        observation: d.observation.clone(),
        requirement: d.requirement.clone(),
    };
    let again = OrgOp::RequestCapability {
        key: "again".into(),
        requirement_id: d2.requirement.clone(),
        role: RoleRef::existing(d2.lead.clone()),
        responsibility: ResponsibilityRef::Existing {
            responsibility_id: rsp,
        },
        capability_id: CapabilityId::new("venue-availability"),
        purpose: "again".into(),
        observed_sources: vec![PORTAL.into()],
    };
    assert_eq!(
        codes(run(&d2, vec![again], &StaticCatalog::default())),
        vec![ValidationCode::DuplicateKey]
    );
}

#[test]
fn only_forward_gap_transitions_are_allowed() {
    use CapabilityGapStatus as S;
    let d = demo(Some(PORTAL));
    let out = run(
        &d,
        vec![assign(&d), request(&d, &[PORTAL])],
        &StaticCatalog::default(),
    )
    .unwrap();
    let id = out.state.capability_gaps.keys().next().unwrap().clone();
    let gap = out.state.capability_gaps[&id].clone();
    assert_eq!(gap.triggering_evidence, vec![d.observation.clone()]);
    assert_eq!(gap.requirement_digest.len(), 64);
    let cref = |name: &str, v: &str| CapabilityRef {
        capability_id: CapabilityId::new(name),
        version: v.into(),
    };
    let acquired = |acq: &str, name: &str, v: &str| S::Acquired {
        acquisition_id: acq.into(),
        capability: cref(name, v),
        publication_ref: None,
    };
    let refused = |acq: Option<&str>| S::Refused {
        reason: "no policy for this capability".into(),
        acquisition_id: acq.map(String::from),
    };
    let err = |s: &OrgState, to: S| update_capability_gap(s, &id, to).unwrap_err().code;
    let s0 = out.state;

    // Open cannot skip ahead or reopen.
    assert_eq!(
        err(&s0, acquired("acq-1", "venue-availability", "1")),
        ValidationCode::InvalidTransition
    );
    assert_eq!(err(&s0, S::Open), ValidationCode::InvalidTransition);
    assert_eq!(
        err(
            &s0,
            S::Acquiring {
                acquisition_id: " ".into()
            }
        ),
        ValidationCode::InvalidInput
    );
    let s1 = update_capability_gap(
        &s0,
        &id,
        S::Acquiring {
            acquisition_id: "acq-1".into(),
        },
    )
    .unwrap();
    assert_eq!(s1.state.revision, s0.revision + 1);
    assert!(matches!(
        &s1.effects[0],
        EventKind::CapabilityGapStatusChanged { from: S::Open, .. }
    ));
    let s1 = s1.state;

    // Acquired: same acquisition, this gap's capability, exact version.
    assert_eq!(
        err(&s1, acquired("acq-OTHER", "venue-availability", "1")),
        ValidationCode::InvalidInput
    );
    assert_eq!(
        err(&s1, acquired("acq-1", "other-cap", "1")),
        ValidationCode::InvalidInput
    );
    assert_eq!(
        err(&s1, acquired("acq-1", "venue-availability", "latest")),
        ValidationCode::InvalidInput
    );
    // Refusal after claiming must keep the bound acquisition.
    assert_eq!(err(&s1, refused(None)), ValidationCode::InvalidInput);
    assert_eq!(
        err(&s1, refused(Some("acq-OTHER"))),
        ValidationCode::InvalidInput
    );
    assert!(update_capability_gap(&s1, &id, refused(Some("acq-1"))).is_ok());
    let s2 = update_capability_gap(&s1, &id, acquired("acq-1", "venue-availability", "1"))
        .unwrap()
        .state;

    // Verified is not installed: Installed needs an accepted local installation
    // of exactly that version.
    let inst = InstallationId::new("inst_1");
    let installed = |cap: CapabilityRef| S::Installed {
        acquisition_id: "acq-1".into(),
        capability: cap,
        installation_id: inst.clone(),
    };
    assert_eq!(
        err(&s2, installed(cref("venue-availability", "1"))),
        ValidationCode::InvalidInput,
        "no installation yet"
    );
    let mut with_install = s2.clone();
    let mut install = Installation {
        installation_id: inst.clone(),
        capability: cref("venue-availability", "1"),
        status: genesis_core::lifecycle::InstallationStatus::Checking,
        configuration: serde_json::Value::Null,
        acceptance_report_ref: None,
        source_digest: "sha256:x".into(),
        installed_at: Utc::now(),
    };
    with_install
        .installations
        .insert(inst.clone(), install.clone());
    assert_eq!(
        err(&with_install, installed(cref("venue-availability", "1"))),
        ValidationCode::InvalidInput,
        "checking is not accepted"
    );
    install.status = genesis_core::lifecycle::InstallationStatus::Accepted;
    with_install.installations.insert(inst.clone(), install);
    assert_eq!(
        err(&with_install, installed(cref("venue-availability", "2"))),
        ValidationCode::InvalidInput,
        "different version"
    );
    let s3 = update_capability_gap(
        &with_install,
        &id,
        installed(cref("venue-availability", "1")),
    )
    .unwrap()
    .state;

    // Installed and Refused are final.
    for to in [
        S::Open,
        refused(Some("acq-1")),
        S::Acquiring {
            acquisition_id: "acq-2".into(),
        },
    ] {
        assert_eq!(err(&s3, to), ValidationCode::InvalidTransition);
    }
    assert_eq!(
        err(
            &s0,
            S::Refused {
                reason: " ".into(),
                acquisition_id: None
            }
        ),
        ValidationCode::InvalidInput
    );
    let r = update_capability_gap(&s0, &id, refused(None))
        .unwrap()
        .state;
    assert_eq!(
        err(
            &r,
            S::Acquiring {
                acquisition_id: "acq-3".into()
            }
        ),
        ValidationCode::InvalidTransition
    );
    assert!(
        update_capability_gap(&s0, &CapabilityGapId::new("gap_missing"), refused(None)).is_err()
    );
}

#[test]
fn proposals_without_the_new_field_still_deserialize() {
    // Change records saved before this op existed have no `gaps` allocation.
    let alloc: KeyAllocation = serde_json::from_value(
        serde_json::json!({"roles": {}, "responsibilities": {}, "contracts": {}}),
    )
    .unwrap();
    assert!(alloc.gaps.is_empty());
    let d = demo(Some(PORTAL));
    let mut v = serde_json::to_value(&d.state).unwrap();
    v.as_object_mut().unwrap().remove("capability_gaps");
    let back: OrgState = serde_json::from_value(v).unwrap();
    assert!(back.capability_gaps.is_empty());
}

#[test]
fn live_gaps_are_bounded_per_requirement_and_per_organization() {
    let mut d = demo(Some(PORTAL));
    // One responsibility, then requests under distinct names for the same requirement.
    let out = run(&d, vec![assign(&d)], &StaticCatalog::default()).unwrap();
    let rsp = out
        .state
        .responsibilities
        .values()
        .find(|r| r.purpose == "Relocate the event")
        .unwrap()
        .responsibility_id
        .clone();
    d.state = out.state;
    let ask = |d: &Demo, name: &str, requirement: &RequirementId| OrgOp::RequestCapability {
        key: format!("k-{name}"),
        requirement_id: requirement.clone(),
        role: RoleRef::existing(d.lead.clone()),
        responsibility: ResponsibilityRef::Existing {
            responsibility_id: rsp.clone(),
        },
        capability_id: CapabilityId::new(name),
        purpose: "p".into(),
        observed_sources: vec![PORTAL.into()],
    };
    for n in 0..MAX_LIVE_GAPS_PER_REQUIREMENT {
        let req = d.requirement.clone();
        d.state = run(
            &d,
            vec![ask(&d, &format!("cap-{n}"), &req)],
            &StaticCatalog::default(),
        )
        .unwrap()
        .state;
    }
    let req = d.requirement.clone();
    assert_eq!(
        codes(run(
            &d,
            vec![ask(&d, "cap-extra", &req)],
            &StaticCatalog::default()
        )),
        vec![ValidationCode::CapabilityGapLimitExceeded]
    );
    // A refused gap no longer counts.
    let first = d.state.capability_gaps.keys().next().unwrap().clone();
    d.state = update_capability_gap(
        &d.state,
        &first,
        CapabilityGapStatus::Refused {
            reason: "no policy".into(),
            acquisition_id: None,
        },
    )
    .unwrap()
    .state;
    d.state = run(
        &d,
        vec![ask(&d, "cap-extra", &req)],
        &StaticCatalog::default(),
    )
    .unwrap()
    .state;

    // Organization-wide limit across many requirements.
    let mission = d.state.missions.keys().next().unwrap().clone();
    let mut created = d
        .state
        .capability_gaps
        .values()
        .filter(|g| !g.status.is_terminal())
        .count();
    while created < MAX_LIVE_CAPABILITY_GAPS {
        let (o, r) = record_requirement(
            &d.state,
            &mission,
            &format!("need {created}"),
            std::slice::from_ref(&d.observation),
            "ok",
        )
        .unwrap();
        d.state = o.state;
        d.state = run(
            &d,
            vec![ask(&d, &format!("org-cap-{created}"), &r)],
            &StaticCatalog::default(),
        )
        .unwrap()
        .state;
        created += 1;
    }
    let (o, r) = record_requirement(
        &d.state,
        &mission,
        "one more",
        std::slice::from_ref(&d.observation),
        "ok",
    )
    .unwrap();
    d.state = o.state;
    assert_eq!(
        codes(run(
            &d,
            vec![ask(&d, "org-cap-over", &r)],
            &StaticCatalog::default()
        )),
        vec![ValidationCode::CapabilityGapLimitExceeded]
    );
}
