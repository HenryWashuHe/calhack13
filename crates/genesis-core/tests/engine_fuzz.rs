//! Proposals come from an untrusted planner. Randomized proposal sequences,
//! chained through successful commits, plus random capability-gap
//! transitions, must never panic the engine or exceed the live-gap limit.
//! Deterministic seed; no extra dependencies.
use chrono::Utc;
use genesis_core::capability::StaticCatalog;
use genesis_core::change::*;
use genesis_core::engine::*;
use genesis_core::ids::*;
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

#[test]
fn random_proposals_never_panic() {
    const ITERATIONS: usize = 3000;
    let (st0, roles0, resps0, reqs, obs) = base();
    let mut st = st0;
    let (mut roles, mut resps) = (roles0, resps0);
    let mut commits = 0;
    let keys = ["k1", "k2", "", "k1"];
    let mut rng = Rng(0x9E3779B97F4A7C15);
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
            let to = match rng.next() % 5 {
                0 => CapabilityGapStatus::Open,
                1 => CapabilityGapStatus::Acquiring {
                    acquisition_id: rng.pick(&["a1", "a2", ""]).to_string(),
                },
                2 => CapabilityGapStatus::Acquired {
                    acquisition_id: rng.pick(&["a1", "a2"]).to_string(),
                    capability: cap,
                    publication_ref: None,
                },
                3 => CapabilityGapStatus::Installed {
                    acquisition_id: "a1".into(),
                    capability: cap,
                    installation_id: InstallationId::new("inst_x"),
                },
                _ => CapabilityGapStatus::Refused {
                    reason: "r".into(),
                    acquisition_id: if rng.b() { Some("a1".into()) } else { None },
                },
            };
            if let Ok(out) = update_capability_gap(&st, &g, to) {
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
        commits > 40,
        "the generator must reach committed states ({commits})"
    );
    assert!(
        !st.capability_gaps.is_empty(),
        "the generator must create gaps"
    );
    let live = st
        .capability_gaps
        .values()
        .filter(|g| !g.status.is_terminal())
        .count();
    assert!(
        live <= MAX_LIVE_CAPABILITY_GAPS && max_live <= MAX_LIVE_CAPABILITY_GAPS,
        "{live}/{max_live} live gaps"
    );
}
