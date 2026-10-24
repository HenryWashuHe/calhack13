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
    // Origins compare case-insensitively on scheme and host. Evidence may be
    // written loosely; sources must be canonical (see
    // sources_must_be_usable_by_the_forge).
    let loose = demo(Some("HTTPS://Portal.Example/org-a/venues/index.html"));
    let ok = run(
        &loose,
        vec![
            assign(&loose),
            request(&loose, &["https://portal.example/org-a/other.html"]),
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

    // Organization-wide limit across many requirements, each with its own
    // responsibility created in the same change.
    let mission = d.state.missions.keys().next().unwrap().clone();
    let pair = |d: &Demo, n: usize, r: &RequirementId| {
        vec![
            OrgOp::AssignResponsibility {
                target: ResponsibilityTarget::FromRequirement {
                    key: format!("rsp-{n}"),
                    requirement_id: r.clone(),
                    purpose: format!("Need {n}"),
                    trigger: "t".into(),
                    termination: "e".into(),
                },
                owner: RoleRef::existing(d.lead.clone()),
            },
            OrgOp::RequestCapability {
                key: format!("gap-{n}"),
                requirement_id: r.clone(),
                role: RoleRef::existing(d.lead.clone()),
                responsibility: ResponsibilityRef::New {
                    key: format!("rsp-{n}"),
                },
                capability_id: CapabilityId::new(format!("org-cap-{n}")),
                purpose: "p".into(),
                observed_sources: vec![PORTAL.into()],
            },
        ]
    };
    let mut created = d
        .state
        .capability_gaps
        .values()
        .filter(|g| !g.status.is_terminal())
        .count();
    let new_requirement = |d: &mut Demo, label: &str| {
        let (o, r) = record_requirement(
            &d.state,
            &mission,
            label,
            std::slice::from_ref(&d.observation),
            "ok",
        )
        .unwrap();
        d.state = o.state;
        r
    };
    while created < MAX_LIVE_CAPABILITY_GAPS {
        let r = new_requirement(&mut d, &format!("need {created}"));
        d.state = run(&d, pair(&d, created, &r), &StaticCatalog::default())
            .unwrap()
            .state;
        created += 1;
    }
    let r = new_requirement(&mut d, "one more");
    let errs = codes(run(&d, pair(&d, 999, &r), &StaticCatalog::default()));
    assert_eq!(errs, vec![ValidationCode::CapabilityGapLimitExceeded]);
}

// Codex review regressions (b6aeb07 on codex/review-capability-gap49).

#[test]
fn review_gap_must_reference_the_responsibility_for_its_requirement() {
    let d = demo(Some(PORTAL));
    let unrelated = d.state.responsibilities.keys().next().unwrap().clone();
    assert!(d.state.responsibilities[&unrelated]
        .requirement_id
        .is_none());
    let mut gap = request(&d, &[PORTAL]);
    let OrgOp::RequestCapability { responsibility, .. } = &mut gap else {
        unreachable!()
    };
    *responsibility = ResponsibilityRef::Existing {
        responsibility_id: unrelated,
    };
    // The same change legitimately assigns the requirement, but the gap names
    // the pre-existing bootstrap responsibility rather than that assignment.
    let result = run(&d, vec![assign(&d), gap], &StaticCatalog::default());
    assert!(
        result.is_err(),
        "a gap for an unrelated responsibility can never pass acquisition authority"
    );
}

#[test]
fn review_verified_gap_can_record_later_publication_without_rebinding() {
    let d = demo(Some(PORTAL));
    let state = run(
        &d,
        vec![assign(&d), request(&d, &[PORTAL])],
        &StaticCatalog::default(),
    )
    .unwrap()
    .state;
    let id = state.capability_gaps.keys().next().unwrap().clone();
    let state = update_capability_gap(
        &state,
        &id,
        CapabilityGapStatus::Acquiring {
            acquisition_id: "acq-1".into(),
        },
    )
    .unwrap()
    .state;
    let acquired = |publication_ref| CapabilityGapStatus::Acquired {
        acquisition_id: "acq-1".into(),
        capability: CapabilityRef {
            capability_id: CapabilityId::new("venue-availability"),
            version: "1".into(),
        },
        publication_ref,
    };
    let state = update_capability_gap(&state, &id, acquired(None))
        .unwrap()
        .state;
    assert!(
        update_capability_gap(
            &state,
            &id,
            acquired(Some("registry:venue-availability@1".into())),
        )
        .is_ok(),
        "verification precedes publication; a later confirmed reference must be recordable"
    );
}

#[test]
fn publication_enrichment_is_once_and_never_rebinds() {
    let d = demo(Some(PORTAL));
    let s = run(
        &d,
        vec![assign(&d), request(&d, &[PORTAL])],
        &StaticCatalog::default(),
    )
    .unwrap()
    .state;
    let id = s.capability_gaps.keys().next().unwrap().clone();
    let s = update_capability_gap(
        &s,
        &id,
        CapabilityGapStatus::Acquiring {
            acquisition_id: "acq-1".into(),
        },
    )
    .unwrap()
    .state;
    let acq = |name: &str, v: &str, p: Option<&str>| CapabilityGapStatus::Acquired {
        acquisition_id: "acq-1".into(),
        capability: CapabilityRef {
            capability_id: CapabilityId::new(name),
            version: v.into(),
        },
        publication_ref: p.map(String::from),
    };
    let s = update_capability_gap(&s, &id, acq("venue-availability", "1", None))
        .unwrap()
        .state;
    // Cannot change the verified version while recording publication.
    assert!(update_capability_gap(&s, &id, acq("venue-availability", "2", Some("reg:x"))).is_err());
    let s = update_capability_gap(&s, &id, acq("venue-availability", "1", Some("reg:x")))
        .unwrap()
        .state;
    // Recorded references are immutable: no replacement, no removal.
    assert_eq!(
        update_capability_gap(&s, &id, acq("venue-availability", "1", Some("reg:y")))
            .unwrap_err()
            .code,
        ValidationCode::InvalidTransition
    );
    assert_eq!(
        update_capability_gap(&s, &id, acq("venue-availability", "1", None))
            .unwrap_err()
            .code,
        ValidationCode::InvalidTransition
    );
    // A different acquisition cannot be recorded either.
    let other = CapabilityGapStatus::Acquired {
        acquisition_id: "acq-2".into(),
        capability: CapabilityRef {
            capability_id: CapabilityId::new("venue-availability"),
            version: "1".into(),
        },
        publication_ref: Some("reg:x".into()),
    };
    assert!(update_capability_gap(&s, &id, other).is_err());
}

#[test]
fn final_transitions_wake_the_planner_with_one_observation() {
    use CapabilityGapStatus as S;
    let d = demo(Some(PORTAL));
    let out = run(
        &d,
        vec![assign(&d), request(&d, &[PORTAL])],
        &StaticCatalog::default(),
    )
    .unwrap();
    let id = out.state.capability_gaps.keys().next().unwrap().clone();
    let mission = out.state.capability_gaps[&id].mission_id.clone();
    let cap = CapabilityRef {
        capability_id: CapabilityId::new("venue-availability"),
        version: "1".into(),
    };
    let observations = |s: &OrgState| s.observations.len();
    let base = observations(&out.state);

    // Intermediate states do not wake the planner.
    let s1 = update_capability_gap(
        &out.state,
        &id,
        S::Acquiring {
            acquisition_id: "acq-1".into(),
        },
    )
    .unwrap();
    assert_eq!(s1.effects.len(), 1);
    let s2 = update_capability_gap(
        &s1.state,
        &id,
        S::Acquired {
            acquisition_id: "acq-1".into(),
            capability: cap.clone(),
            publication_ref: None,
        },
    )
    .unwrap();
    assert_eq!(s2.effects.len(), 1);
    assert_eq!(observations(&s2.state), base);

    // Installed: one observation on the gap's mission, in the same outcome.
    let inst = InstallationId::new("inst_1");
    let mut st = s2.state.clone();
    st.installations.insert(
        inst.clone(),
        Installation {
            installation_id: inst.clone(),
            capability: cap.clone(),
            status: genesis_core::lifecycle::InstallationStatus::Accepted,
            configuration: serde_json::Value::Null,
            acceptance_report_ref: None,
            source_digest: "sha256:x".into(),
            installed_at: Utc::now(),
        },
    );
    let at = Utc::now();
    let done = update_capability_gap_at(
        &st,
        &id,
        S::Installed {
            acquisition_id: "acq-1".into(),
            capability: cap.clone(),
            installation_id: inst.clone(),
        },
        at,
    )
    .unwrap();
    assert_eq!(done.state.revision, st.revision + 1);
    assert_eq!(observations(&done.state), base + 1);
    let EventKind::ObservationRecorded {
        observation_id,
        source,
        summary,
    } = &done.effects[1]
    else {
        panic!("expected an observation, got {:?}", done.effects);
    };
    assert_eq!(done.effects.len(), 2);
    assert_eq!(source, CAPABILITY_GAP_SOURCE);
    assert!(summary.contains("venue-availability@1") && summary.contains("inst_1"));
    let o = &done.state.observations[observation_id];
    assert_eq!(o.mission_id, mission);
    assert_eq!(o.observed_at, at);
    assert_eq!(
        o.evidence_ref.as_deref(),
        Some(format!("genesis://capability-gaps/{id}").as_str())
    );

    // Refused also wakes it, with the reason.
    let refused = update_capability_gap(
        &s1.state,
        &id,
        S::Refused {
            reason: "operator policy declined".into(),
            acquisition_id: Some("acq-1".into()),
        },
    )
    .unwrap();
    assert_eq!(observations(&refused.state), base + 1);
    assert!(matches!(
        &refused.effects[1],
        EventKind::ObservationRecorded { summary, .. } if summary.contains("operator policy declined")
    ));
}

#[test]
fn a_kernel_gap_observation_cannot_justify_another_request() {
    // The refusal note carries no public URL, so a fresh requirement
    // triggered only by it cannot back a RequestCapability: no re-request
    // loop driven by the kernel's own notes.
    use CapabilityGapStatus as S;
    let d = demo(Some(PORTAL));
    let out = run(
        &d,
        vec![assign(&d), request(&d, &[PORTAL])],
        &StaticCatalog::default(),
    )
    .unwrap();
    let id = out.state.capability_gaps.keys().next().unwrap().clone();
    let refused = update_capability_gap(
        &out.state,
        &id,
        S::Refused {
            reason: "declined".into(),
            acquisition_id: None,
        },
    )
    .unwrap()
    .state;
    let (note, mission) = refused
        .observations
        .values()
        .find(|o| o.source == CAPABILITY_GAP_SOURCE)
        .map(|o| (o.observation_id.clone(), o.mission_id.clone()))
        .unwrap();
    let (rec, requirement) = record_requirement(
        &refused,
        &mission,
        "Read venue availability another way",
        std::slice::from_ref(&note),
        "Availability is read",
    )
    .unwrap();
    let d2 = Demo {
        state: rec.state,
        lead: d.lead.clone(),
        observation: note,
        requirement,
    };
    let mut retry = request(&d2, &[PORTAL]);
    if let OrgOp::RequestCapability {
        key, capability_id, ..
    } = &mut retry
    {
        *key = "venue-reader-2".into();
        *capability_id = CapabilityId::new("venue-availability-alt");
    }
    let mut again = assign(&d2);
    if let OrgOp::AssignResponsibility {
        target: ResponsibilityTarget::FromRequirement { key, .. },
        ..
    } = &mut again
    {
        *key = "relocate".into();
    }
    let rejected = run(&d2, vec![again, retry], &StaticCatalog::default());
    let errs = rejected.expect_err("kernel note justified a new request");
    assert!(
        errs.iter()
            .any(|e| e.message.contains("origin") || e.message.contains("http")),
        "{errs:?}"
    );
}

#[test]
fn sources_must_be_usable_by_the_forge() {
    // Same rules as genesis_forge::SynthesisRequest::validate, so a committed
    // gap is always one a trusted policy could materialize.
    let rejected = [
        "http://portal.example/org-a/venues/index.html",
        "https://portal.example/org-a/venues/index.html#top",
        "https://user@portal.example/org-a/venues/index.html",
        "https://PORTAL.example/org-a/venues/index.html",
        "https://portal.example:443/org-a/venues/index.html",
        "https://portal.example/org-a/venues/../venues/index.html",
        "https://portal.example",
    ];
    for bad in rejected {
        let d = demo(Some(PORTAL));
        let r = run(
            &d,
            vec![assign(&d), request(&d, &[bad])],
            &StaticCatalog::default(),
        );
        let errs = r.expect_err(bad);
        assert!(
            errs.iter()
                .any(|e| e.code == ValidationCode::InvalidInput
                    && e.message.contains("canonical https")),
            "{bad}: {errs:?}"
        );
    }
    let d = demo(Some(PORTAL));
    assert!(run(
        &d,
        vec![
            assign(&d),
            request(&d, &["https://portal.example/org-a/venues/b.html?x=1"])
        ],
        &StaticCatalog::default()
    )
    .is_ok());
}

#[test]
fn http_evidence_cannot_back_an_https_request() {
    // Evidence served over plain http (a local portal) has an http origin; an
    // https source on the same host is a different origin. Serve the portal
    // over https (the public tunnel) for the capability path.
    let d = demo(Some("http://portal.example/org-a/venues/index.html"));
    let errs = run(
        &d,
        vec![assign(&d), request(&d, &[PORTAL])],
        &StaticCatalog::default(),
    )
    .expect_err("origin mismatch");
    assert!(
        errs.iter()
            .any(|e| e.code == ValidationCode::UnknownEvidence),
        "{errs:?}"
    );
}

#[test]
fn a_closed_or_expired_mission_gets_the_audit_but_no_wakeup() {
    use genesis_core::lifecycle::MissionStatus;
    use CapabilityGapStatus as S;
    for close in ["succeeded", "failed", "expired"] {
        let d = demo(Some(PORTAL));
        let out = run(
            &d,
            vec![assign(&d), request(&d, &[PORTAL])],
            &StaticCatalog::default(),
        )
        .unwrap();
        let id = out.state.capability_gaps.keys().next().unwrap().clone();
        let mut st = out.state.clone();
        let mission = st.capability_gaps[&id].mission_id.clone();
        let m = st.missions.get_mut(&mission).unwrap();
        match close {
            "succeeded" => m.status = MissionStatus::Succeeded,
            "failed" => m.status = MissionStatus::Failed,
            _ => m.deadline = Some(Utc::now() - chrono::Duration::seconds(1)),
        }
        let before = st.observations.len();
        let done = update_capability_gap(
            &st,
            &id,
            S::Refused {
                reason: "declined".into(),
                acquisition_id: None,
            },
        )
        .unwrap();
        assert!(
            matches!(done.state.capability_gaps[&id].status, S::Refused { .. }),
            "{close}"
        );
        assert_eq!(done.effects.len(), 1, "{close}: audit only");
        assert!(matches!(
            done.effects[0],
            EventKind::CapabilityGapStatusChanged { .. }
        ));
        assert_eq!(done.state.observations.len(), before, "{close}: no wake-up");
    }
}
