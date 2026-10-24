//! Proposals come from an untrusted planner. Randomized proposal sequences,
//! chained through successful commits, plus random capability-gap, contract,
//! completion and installation steps, must never panic the engine, and every
//! reachable state must satisfy the organization invariants in [`check`].
//! Deterministic seeds; no extra dependencies.
use chrono::Utc;
use genesis_core::audit::{audit, audit_transition, INVARIANTS, TRANSITION_INVARIANTS};
use genesis_core::capability::StaticCatalog;
use genesis_core::change::*;
use genesis_core::engine::*;
use genesis_core::ids::*;
use genesis_core::lifecycle::*;
use genesis_core::model::*;
use std::collections::BTreeMap;

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn pick<'a, T>(&mut self, v: &'a [T]) -> &'a T {
        &v[(self.next() % v.len() as u64) as usize]
    }
    fn b(&mut self) -> bool {
        self.next().is_multiple_of(2)
    }
}

fn base() -> (OrgState, Vec<String>, Vec<String>, Vec<String>, Vec<String>) {
    let now = Utc::now();
    let st = OrgState::new(
        OrgId::new("org_a"),
        "A",
        Constitution {
            scopes: ["a", "b"].map(String::from).into(),
            budget_cents: 1000,
            max_active_roles: 4,
            max_active_contracts: 1,
        },
    );
    let spec = MissionBootstrap {
        goal: "g".into(),
        constraints: vec![],
        deadline: None,
        success_checks: vec![],
        roles: vec![
            BootstrapRole {
                key: "lead".into(),
                name: "L".into(),
                charter: "c".into(),
                authority: AuthorityGrant::new(["a"], 10),
                reports_to: None,
            },
            BootstrapRole {
                key: "two".into(),
                name: "T".into(),
                charter: "c".into(),
                authority: AuthorityGrant::new(["b"], 10),
                reports_to: Some("lead".into()),
            },
        ],
        responsibilities: vec![BootstrapResponsibility {
            purpose: "p".into(),
            owner: "lead".into(),
            trigger: "t".into(),
            termination: "e".into(),
        }],
    };
    let (o, m) = bootstrap_mission(&st, &spec, now).unwrap();
    let (o, obs) = record_observation(
        &o.state,
        &m,
        "s",
        "x",
        Some("https://p.example/a".into()),
        now,
    )
    .unwrap();
    let (o, req) =
        record_requirement(&o.state, &m, "r", std::slice::from_ref(&obs), "acc").unwrap();
    let st = o.state;
    let roles: Vec<String> = st
        .roles
        .keys()
        .map(|r| r.to_string())
        .chain(["role_missing".into()])
        .collect();
    let resps: Vec<String> = st
        .responsibilities
        .keys()
        .map(|r| r.to_string())
        .chain(["rsp_missing".into()])
        .collect();
    (
        st,
        roles,
        resps,
        vec![req.to_string(), "req_missing".into()],
        vec![obs.to_string(), "obs_missing".into()],
    )
}

/// Every committed state must pass the independent invariant audit, and be a
/// legal successor of the state before it.
fn check(st: &OrgState, prev: &OrgState, step: &'static str) {
    *COVERAGE.lock().unwrap().entry(step).or_default() += 1;
    for (kind, present) in [
        (
            "state with an installed gap",
            st.capability_gaps
                .values()
                .any(|g| matches!(g.status, CapabilityGapStatus::Installed { .. })),
        ),
        (
            "state with a refused gap",
            st.capability_gaps
                .values()
                .any(|g| matches!(g.status, CapabilityGapStatus::Refused { .. })),
        ),
    ] {
        if present {
            *COVERAGE.lock().unwrap().entry(kind).or_default() += 1;
        }
    }
    let mut violations = audit(st);
    violations.extend(audit_transition(prev, st));
    assert!(
        violations.is_empty(),
        "after {step} at r{}: {violations:#?}",
        st.revision
    );
}

static COVERAGE: std::sync::Mutex<BTreeMap<&'static str, usize>> =
    std::sync::Mutex::new(BTreeMap::new());

const SEEDS: [u64; 8] = [
    0x9E3779B97F4A7C15,
    1,
    0xDEADBEEF,
    0x1234_5678_9ABC_DEF0,
    42,
    0xC0FFEE,
    0xFEED_FACE_CAFE_BEEF,
    7,
];

#[test]
fn random_proposals_never_panic() {
    let mut total = 0;
    for seed in SEEDS {
        total += run(seed);
    }
    assert!(
        total > 8 * 40,
        "the generator must reach committed states ({total})"
    );
    // Every kind of step must actually be exercised, or the invariants prove little.
    let coverage = COVERAGE.lock().unwrap().clone();
    eprintln!("checked states: {coverage:?}");
    for kind in [
        "commit",
        "gap transition",
        "contract transition",
        "completion",
        "installation result",
        "state with an installed gap",
        "state with a refused gap",
    ] {
        assert!(
            coverage.get(kind).copied().unwrap_or(0) > 0,
            "never reached: {kind}"
        );
    }
}

/// One seeded run; returns the number of successful commits.
fn run(seed: u64) -> usize {
    const ITERATIONS: usize = 1000;
    let (st0, roles0, resps0, reqs, obs) = base();
    let mut st = st0;
    let (mut roles, mut resps) = (roles0, resps0);
    let mut commits = 0;
    let keys = ["k1", "k2", "", "k1"];
    let mut rng = Rng(seed);
    let mut checked = 0;
    let mut max_live = 0;
    for _ in 0..ITERATIONS {
        let n = (rng.next() % 3) as usize;
        let mut ops = vec![];
        let mut cited = ObservationId::new(rng.pick(&obs).clone());
        if rng.next().is_multiple_of(3) {
            // Valid template: a fresh requirement owned by an active role, then a request.
            let now = Utc::now();
            let m = st.missions.keys().next().unwrap().clone();
            let (o1, ob) =
                record_observation(&st, &m, "s", "x", Some("https://p.example/a".into()), now)
                    .unwrap();
            let (o2, rq) =
                record_requirement(&o1.state, &m, "r", std::slice::from_ref(&ob), "a").unwrap();
            st = o2.state;
            cited = ob;
            if let Some(active) = st
                .roles
                .values()
                .find(|r| r.state.accepts_work())
                .map(|r| r.role_id.clone())
            {
                ops.push(OrgOp::AssignResponsibility {
                    target: ResponsibilityTarget::FromRequirement {
                        key: "t".into(),
                        requirement_id: rq.clone(),
                        purpose: "p".into(),
                        trigger: "t".into(),
                        termination: "e".into(),
                    },
                    owner: RoleRef::existing(active.clone()),
                });
                ops.push(OrgOp::RequestCapability {
                    key: "g".into(),
                    requirement_id: rq,
                    role: RoleRef::existing(active),
                    responsibility: ResponsibilityRef::New { key: "t".into() },
                    capability_id: CapabilityId::new("listing-reader"),
                    purpose: "p".into(),
                    observed_sources: vec!["https://p.example/b".into()],
                });
            }
        }
        for _ in 0..n {
            let role = if rng.b() {
                RoleRef::existing(RoleId::new(rng.pick(&roles).clone()))
            } else {
                RoleRef::new_key(*rng.pick(&keys))
            };
            let resp = if rng.b() {
                ResponsibilityRef::Existing {
                    responsibility_id: ResponsibilityId::new(rng.pick(&resps).clone()),
                }
            } else {
                ResponsibilityRef::New {
                    key: rng.pick(&keys).to_string(),
                }
            };
            let op = match rng.next() % 9 {
                0 => OrgOp::CreateRole {
                    key: rng.pick(&keys).to_string(),
                    name: if rng.b() { "n".into() } else { "".into() },
                    charter: "c".into(),
                    authority: AuthorityGrant::new(
                        [*rng.pick(&["a", "b", "zz"])],
                        rng.next() % 2000,
                    ),
                    reports_to: if rng.b() { Some(role.clone()) } else { None },
                    provider: *rng.pick(&[
                        Provider::Elastic,
                        Provider::Creao,
                        Provider::Apify,
                        Provider::Internal,
                    ]),
                    justification: RoleJustification::Expertise,
                },
                1 => OrgOp::AssignResponsibility {
                    target: if rng.b() {
                        ResponsibilityTarget::FromRequirement {
                            key: rng.pick(&keys).to_string(),
                            requirement_id: RequirementId::new(rng.pick(&reqs).clone()),
                            purpose: "p".into(),
                            trigger: "t".into(),
                            termination: "e".into(),
                        }
                    } else {
                        ResponsibilityTarget::Existing {
                            responsibility_id: ResponsibilityId::new(rng.pick(&resps).clone()),
                        }
                    },
                    owner: role,
                },
                2 => OrgOp::DelegateTask {
                    responsibility: resp,
                    executor: if rng.b() {
                        ExecutorRef::Role { role }
                    } else {
                        ExecutorRef::Contractor {
                            contract: ContractRef::New {
                                key: rng.pick(&keys).to_string(),
                            },
                        }
                    },
                    task: "t".into(),
                },
                3 => OrgOp::ReassignResponsibility {
                    responsibility_id: ResponsibilityId::new(rng.pick(&resps).clone()),
                    from: RoleId::new(rng.pick(&roles).clone()),
                    to: role,
                },
                4 => OrgOp::AttachCapability {
                    role,
                    capability: CapabilityRef {
                        capability_id: CapabilityId::new("c"),
                        version: rng.pick(&["1", "latest", ""]).to_string(),
                    },
                },
                5 => OrgOp::ChangeReportingEdge {
                    role: role.clone(),
                    reports_to: if rng.b() { Some(role) } else { None },
                },
                6 => OrgOp::RetireRole {
                    role_id: RoleId::new(rng.pick(&roles).clone()),
                    handoffs: if rng.b() {
                        vec![Handoff {
                            responsibility_id: ResponsibilityId::new(rng.pick(&resps).clone()),
                            to: role,
                        }]
                    } else {
                        vec![]
                    },
                },
                7 => OrgOp::RecruitSpecialist {
                    key: rng.pick(&keys).to_string(),
                    specialist_address: "agent1x".into(),
                    protocol: "p".into(),
                    scope: "s".into(),
                    deadline: None,
                    supervising_role: if rng.b() { Some(role) } else { None },
                },
                _ => OrgOp::RequestCapability {
                    key: rng.pick(&keys).to_string(),
                    requirement_id: RequirementId::new(rng.pick(&reqs).clone()),
                    role,
                    responsibility: resp,
                    capability_id: CapabilityId::new(*rng.pick(&[
                        "listing-reader",
                        "x",
                        "Bad Name",
                    ])),
                    purpose: if rng.b() { "p".into() } else { "".into() },
                    observed_sources: rng
                        .pick(&[
                            vec!["https://p.example/b".to_string()],
                            vec![],
                            vec!["https://q.example/".to_string()],
                            vec!["https://".to_string()],
                            vec!["://x".to_string()],
                        ])
                        .clone(),
                },
            };
            ops.push(op);
        }
        let p = OrgChangeProposal {
            expected_org_revision: if rng.next().is_multiple_of(10) {
                st.revision + 1
            } else {
                st.revision
            },
            trigger_evidence_ids: vec![cited],
            rationale: "r".into(),
            budget_limit_cents: if rng.b() {
                Some(rng.next() % 3000)
            } else {
                None
            },
            operations: ops,
        };
        let alloc = KeyAllocation::allocate(&p);
        let _ = validate(&st, &p, &alloc, &StaticCatalog::default(), Utc::now());
        let mut bindings = BTreeMap::new();
        for k in keys {
            bindings.insert(
                k.to_string(),
                AgentBinding {
                    provider: Provider::Elastic,
                    provider_agent_id: "a".into(),
                    config_digest: "d".into(),
                    conversation_ref: None,
                },
            );
        }
        if let Ok(out) = commit(
            &st,
            &p,
            &alloc,
            &bindings,
            &StaticCatalog::default(),
            Utc::now(),
        ) {
            check(&out.state, &st, "commit");
            st = out.state;
            commits += 1;
            roles = st
                .roles
                .keys()
                .map(|r| r.to_string())
                .chain(["role_missing".into()])
                .collect();
            resps = st
                .responsibilities
                .keys()
                .map(|r| r.to_string())
                .chain(["rsp_missing".into()])
                .collect();
        }
        // Random gap transitions against whatever gaps exist.
        let gaps: Vec<_> = st.capability_gaps.keys().cloned().collect();
        if !gaps.is_empty() {
            let g = rng.pick(&gaps).clone();
            let cap = CapabilityRef {
                capability_id: CapabilityId::new(*rng.pick(&["listing-reader", "x"])),
                version: rng.pick(&["1", "latest"]).to_string(),
            };
            let gap = &st.capability_gaps[&g];
            let plausible = |cap: CapabilityRef| -> Option<CapabilityGapStatus> {
                let acquisition_id = gap.status.acquisition_id().unwrap_or("a1").to_string();
                match &gap.status {
                    CapabilityGapStatus::Open => {
                        Some(CapabilityGapStatus::Acquiring { acquisition_id })
                    }
                    CapabilityGapStatus::Acquiring { .. } => Some(CapabilityGapStatus::Acquired {
                        acquisition_id,
                        capability: CapabilityRef {
                            capability_id: gap.capability_id.clone(),
                            version: cap.version,
                        },
                        publication_ref: None,
                    }),
                    CapabilityGapStatus::Acquired { capability, .. } => {
                        let installation_id = st
                            .installations
                            .values()
                            .find(|i| &i.capability == capability)
                            .map(|i| i.installation_id.clone())?;
                        Some(CapabilityGapStatus::Installed {
                            acquisition_id,
                            capability: capability.clone(),
                            installation_id,
                        })
                    }
                    _ => None,
                }
            };
            // Mostly the next step a driver would take, sometimes anything.
            let next = if !rng.next().is_multiple_of(4) {
                plausible(cap.clone())
            } else {
                None
            };
            let to = match next {
                Some(to) => to,
                None => match rng.next() % 5 {
                    0 => CapabilityGapStatus::Open,
                    1 => CapabilityGapStatus::Acquiring {
                        acquisition_id: rng.pick(&["a1", "a2", ""]).to_string(),
                    },
                    2 => CapabilityGapStatus::Acquired {
                        acquisition_id: rng.pick(&["a1", "a2"]).to_string(),
                        capability: cap,
                        publication_ref: None,
                    },
                    3 => {
                        let mut ids: Vec<_> = st.installations.keys().cloned().collect();
                        ids.push(InstallationId::new("inst_missing"));
                        CapabilityGapStatus::Installed {
                            acquisition_id: rng.pick(&["a1", "a2"]).to_string(),
                            capability: cap,
                            installation_id: rng.pick(&ids).clone(),
                        }
                    }
                    _ => CapabilityGapStatus::Refused {
                        reason: "r".into(),
                        acquisition_id: if rng.b() { Some("a1".into()) } else { None },
                    },
                },
            };
            if let Ok(out) = update_capability_gap(&st, &g, to) {
                check(&out.state, &st, "gap transition");
                st = out.state;
            }
        }
        // Trusted backend steps: contract lifecycle, completion, installations.
        let contracts: Vec<_> = st.contracts.keys().cloned().collect();
        if !contracts.is_empty() && rng.next().is_multiple_of(4) {
            let k = rng.pick(&contracts).clone();
            let to = *rng.pick(&[
                ContractState::Offered,
                ContractState::Accepted,
                ContractState::Released,
                ContractState::Declined,
            ]);
            if let Ok(out) = update_contract(&st, &k, to) {
                check(&out.state, &st, "contract transition");
                st = out.state;
            }
        }
        if rng.next().is_multiple_of(7) {
            let r = ResponsibilityId::new(rng.pick(&resps).clone());
            if let Ok(out) = complete_responsibility(&st, &r) {
                check(&out.state, &st, "completion");
                st = out.state;
            }
        }
        if rng.next().is_multiple_of(3) {
            // A trusted installer begins one installation per capability at a
            // time (the same rule as `begin_installation`).
            let capability = CapabilityRef {
                capability_id: CapabilityId::new(*rng.pick(&["listing-reader", "x"])),
                version: rng.pick(&["1", "latest"]).to_string(),
            };
            let busy = st.installations.values().any(|i| {
                i.capability == capability
                    && matches!(
                        i.status,
                        InstallationStatus::Checking | InstallationStatus::Accepted
                    )
            });
            if !busy {
                let id = InstallationId::new(format!("inst_{}", st.installations.len()));
                st.installations.insert(
                    id.clone(),
                    Installation {
                        installation_id: id,
                        capability,
                        status: InstallationStatus::Checking,
                        configuration: serde_json::Value::Null,
                        acceptance_report_ref: None,
                        source_digest: "sha256:x".into(),
                        installed_at: Utc::now(),
                    },
                );
            }
        }
        let checking: Vec<_> = st
            .installations
            .values()
            .filter(|i| i.status == InstallationStatus::Checking)
            .map(|i| i.installation_id.clone())
            .collect();
        if !checking.is_empty() && rng.next().is_multiple_of(3) {
            let id = rng.pick(&checking).clone();
            if let Ok(out) =
                finish_installation(&st, &id, !rng.next().is_multiple_of(4), "r", vec![])
            {
                check(&out.state, &st, "installation result");
                st = out.state;
            }
        }
        max_live = max_live.max(
            st.capability_gaps
                .values()
                .filter(|g| !g.status.is_terminal())
                .count(),
        );
        checked += 1;
    }
    assert_eq!(checked, ITERATIONS);
    assert!(
        !st.capability_gaps.is_empty(),
        "seed {seed}: the generator must create gaps"
    );
    assert!(
        max_live <= MAX_LIVE_CAPABILITY_GAPS,
        "seed {seed}: {max_live} live gaps"
    );
    commits
}

/// The audit itself must reject broken states, naming the right invariant,
/// or a green fuzz run proves nothing.
#[test]
fn the_audit_names_each_tampered_invariant() {
    let (st, ..) = base();
    assert_eq!(audit(&st), vec![]);
    let lead = st
        .roles
        .values()
        .find(|r| r.reports_to.is_none())
        .unwrap()
        .role_id
        .clone();
    type Tamper = Box<dyn Fn(&mut OrgState)>;
    let tamper: Vec<(&str, Tamper)> = vec![
        (
            "reporting_tree",
            Box::new(|s: &mut OrgState| {
                let id = s.roles.keys().next().unwrap().clone();
                s.roles.get_mut(&id).unwrap().reports_to = Some(id.clone());
            }),
        ),
        (
            "budget_limit",
            Box::new(|s: &mut OrgState| {
                s.roles.values_mut().next().unwrap().authority.budget_cents = 1_000_000;
            }),
        ),
        (
            "constitution_scopes",
            Box::new(|s: &mut OrgState| {
                let r = s.roles.values_mut().next().unwrap();
                r.authority.scopes.insert("root".into());
            }),
        ),
        (
            "single_active_assignment",
            Box::new(|s: &mut OrgState| {
                let mut a = s.assignments.values().next().unwrap().clone();
                a.assignment_id = AssignmentId::new("asg_dup");
                s.assignments.insert(a.assignment_id.clone(), a);
            }),
        ),
        (
            "live_owner",
            Box::new(move |s: &mut OrgState| {
                let r = s.roles.get_mut(&lead).unwrap();
                r.state = RoleState::Retired;
                r.retired_in_revision = Some(r.created_in_revision);
            }),
        ),
        (
            "role_limit",
            Box::new(|s: &mut OrgState| s.constitution.max_active_roles = 0),
        ),
        (
            "requirement_evidence",
            Box::new(|s: &mut OrgState| s.observations.clear()),
        ),
        (
            "map_key_identity",
            Box::new(|s: &mut OrgState| {
                let role = s.roles.values_mut().next().unwrap();
                role.role_id = RoleId::new("role_elsewhere");
            }),
        ),
    ];
    for (expected, f) in tamper {
        assert!(INVARIANTS.contains(&expected));
        let mut bad = st.clone();
        f(&mut bad);
        let found: Vec<_> = audit(&bad).into_iter().map(|v| v.invariant).collect();
        assert!(
            found.contains(&expected),
            "tampering {expected} was reported as {found:?}"
        );
    }
}

/// Same for the transition audit: rewinding history is caught by name.
#[test]
fn the_transition_audit_names_each_rewind() {
    let (st, ..) = base();
    let mut later = st.clone();
    later.revision += 1;
    assert_eq!(audit_transition(&st, &later), vec![]);
    type Tamper = Box<dyn Fn(&mut OrgState)>;
    let tamper: Vec<(&str, Tamper)> = vec![
        (
            "revision_advances",
            Box::new(|s: &mut OrgState| s.revision -= 1),
        ),
        (
            "history_is_append_only",
            Box::new(|s: &mut OrgState| s.requirements.clear()),
        ),
        (
            "observations_immutable",
            Box::new(|s: &mut OrgState| {
                s.observations.values_mut().next().unwrap().summary = "edited".into();
            }),
        ),
        (
            "lifecycle_reachable",
            Box::new(|s: &mut OrgState| {
                s.roles.values_mut().next().unwrap().state = RoleState::Proposed;
            }),
        ),
        (
            "epoch_monotonic",
            Box::new(|s: &mut OrgState| {
                s.assignments.values_mut().next().unwrap().epoch += 1;
            }),
        ),
    ];
    for (expected, f) in tamper {
        assert!(TRANSITION_INVARIANTS.contains(&expected));
        let mut bad = later.clone();
        f(&mut bad);
        let found: Vec<_> = audit_transition(&st, &bad)
            .into_iter()
            .map(|v| v.invariant)
            .collect();
        assert!(
            found.contains(&expected),
            "rewinding {expected} was reported as {found:?}"
        );
    }
}
