//! Randomized self-modification through the real server path: saga
//! submission, background provisioning against a flaky provider, commit
//! races between concurrent proposals, observations, requirements and
//! completed duties, all on one in-process organization.
//!
//! Checked continuously and at the end:
//! - every stored revision passes `genesis_core::audit::audit`, consecutive
//!   revisions pass `audit_transition`, and the whole store passes
//!   `genesis_server::audit::audit_history` (the store also refuses any
//!   ill-formed commit, because the test sets GENESIS_AUDIT_COMMITS=1);
//! - every change ends COMMITTED, FAILED or REJECTED, except ones whose
//!   provider outcome is unknown, which stay unresolved for reconciliation;
//! - a committed change's roles are active in exactly the revision it names;
//!   roles of changes that did not commit never appear in any revision;
//! - event revisions never go backwards.
//!
//! Seeded: `GENESIS_SAGA_FUZZ_SEED=<n> cargo test -p genesis-server --test saga_fuzz`.

use chrono::Utc;
use genesis_core::audit::{audit as check, audit_transition as check_step};
use genesis_core::change::ProviderOperationState;
use genesis_core::change::*;
use genesis_core::engine::{self, MissionBootstrap};
use genesis_core::ids::*;
use genesis_core::lifecycle::{ChangeStatus, ResponsibilityStatus, RoleState};
use genesis_core::model::{AgentBinding, AuthorityGrant, OrgState, Owner, Provider};
use genesis_providers::{ProviderError, ProviderSet, ProvisionRequest, RoleProvisioner};
use genesis_server::app::AppState;
use genesis_server::store::EventMeta;
use genesis_server::{build_org, saga, OrgConfig};
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::time::Duration;

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }
    fn chance(&mut self, pct: u64) -> bool {
        self.next() % 100 < pct
    }
}

/// Fails some creations outright, and leaves a few with an unknown outcome.
/// Its own generator keeps provider behavior independent of task timing.
struct Flaky {
    rng: Mutex<Rng>,
    max_delay_ms: u64,
}
type ProviderFuture<'a, T> = Pin<Box<dyn Future<Output = Result<T, ProviderError>> + Send + 'a>>;
impl RoleProvisioner for Flaky {
    fn provider(&self) -> Provider {
        Provider::Elastic
    }
    fn provision<'a, 'b, 'f>(
        &'a self,
        req: &'b ProvisionRequest,
    ) -> ProviderFuture<'f, AgentBinding>
    where
        'a: 'f,
        'b: 'f,
        Self: 'f,
    {
        let (roll, delay) = {
            let mut rng = self.rng.lock().unwrap();
            (rng.below(100), rng.next() % self.max_delay_ms.max(1))
        };
        Box::pin(async move {
            tokio::time::sleep(Duration::from_millis(delay)).await;
            match roll {
                0..=11 => Err(ProviderError::Rejected("fuzz: quota".into())),
                12..=19 => Err(ProviderError::Http {
                    status: 503,
                    body: "fuzz".into(),
                }),
                20..=23 => Err(ProviderError::Unknown(
                    "fuzz: connection dropped after submit".into(),
                )),
                _ => Ok(AgentBinding {
                    provider: Provider::Elastic,
                    provider_agent_id: format!("fuzz-elastic-{}", req.role.role_id),
                    config_digest: req.config_digest(),
                    conversation_ref: None,
                }),
            }
        })
    }
    fn check_ready<'a, 'b, 'f>(&'a self, _: &'b AgentBinding) -> ProviderFuture<'f, bool>
    where
        'a: 'f,
        'b: 'f,
        Self: 'f,
    {
        Box::pin(async { Ok(true) })
    }
}

fn app(seed: u64) -> AppState {
    app_with(seed, None, 7)
}

fn app_with(seed: u64, db: Option<std::path::PathBuf>, max_delay_ms: u64) -> AppState {
    let mut config = OrgConfig::dev("org_saga_fuzz", "Saga fuzz");
    config.provision_timeout = Duration::from_secs(5);
    config.db = db;
    let flaky = Arc::new(Flaky {
        rng: Mutex::new(Rng(seed ^ 0xA5A5_5A5A)),
        max_delay_ms,
    });
    // After a simulated crash the previous lifetime's threads may still be
    // finishing a statement; a real process exit would release the file at
    // once, so wait for it instead of failing.
    let providers = || Some(ProviderSet::new().with_provisioner(Provider::Elastic, flaky.clone()));
    for _ in 0..200 {
        match build_org(&config, providers()) {
            Ok(app) => return app,
            Err(e) if format!("{e:?}").contains("locked") => {
                std::thread::sleep(Duration::from_millis(10))
            }
            Err(e) => panic!("{e:?}"),
        }
    }
    panic!("database stayed locked")
}

async fn bootstrap(app: &AppState) -> MissionId {
    let spec: MissionBootstrap = serde_json::from_value(json!({
        "goal": "Run the event",
        "roles": [
            {"key":"a","name":"Role a","charter":"Coordinate","authority":{"scopes":["mission.coordinate"],"budget_cents":100000}},
            {"key":"b","name":"Role b","charter":"Schedule","authority":{"scopes":["program.schedule"],"budget_cents":50000},"reports_to":"a"},
            {"key":"c","name":"Role c","charter":"Inform","authority":{"scopes":["attendees.inform"],"budget_cents":10000},"reports_to":"a"}
        ],
        "responsibilities": [
            {"purpose":"Duty a","owner":"a"}, {"purpose":"Duty b","owner":"b"}, {"purpose":"Duty c","owner":"c"}
        ]
    }))
    .unwrap();
    app.mutate(EventMeta::default(), |state| {
        engine::bootstrap_mission(state, &spec, Utc::now())
            .map_err(genesis_server::error::validation_error)
    })
    .await
    .unwrap()
    .value
}

const SCOPES: &[&str] = &[
    "mission.coordinate",
    "program.schedule",
    "attendees.inform",
    "ledger.reserve_venue",
    "contract.logistics",
    "monitor.venue",
];

fn sorted_live(st: &OrgState) -> Vec<RoleId> {
    let mut v: Vec<_> = st
        .roles
        .values()
        .filter(|r| r.state == RoleState::Active)
        .map(|r| (r.name.clone(), r.role_id.clone()))
        .collect();
    v.sort();
    v.into_iter().map(|(_, id)| id).collect()
}

fn proposal(rng: &mut Rng, st: &OrgState, n: u64) -> OrgChangeProposal {
    let live = sorted_live(st);
    let pick = |rng: &mut Rng, xs: &[RoleId]| {
        xs.get(rng.below(xs.len()))
            .cloned()
            .unwrap_or_else(|| RoleId::new("role_missing"))
    };
    let mut obs: Vec<_> = st
        .observations
        .values()
        .map(|o| (o.summary.clone(), o.observation_id.clone()))
        .collect();
    obs.sort();
    let mut unmet: Vec<_> = st
        .requirements
        .values()
        .filter(|q| q.status == genesis_core::lifecycle::RequirementStatus::Unmet)
        .map(|q| (q.summary.clone(), q.requirement_id.clone()))
        .collect();
    unmet.sort();
    let mut open: Vec<_> = st
        .responsibilities
        .values()
        .filter(|r| r.status == ResponsibilityStatus::Open)
        .map(|r| {
            (
                r.purpose.clone(),
                r.responsibility_id.clone(),
                r.owner.clone(),
            )
        })
        .collect();
    open.sort_by(|a, b| a.0.cmp(&b.0));

    let key = format!("k{n}");
    let mut scopes: Vec<String> = vec![SCOPES[rng.below(SCOPES.len())].into()];
    if rng.chance(8) {
        scopes.push("admin.everything".into());
    }
    let create = OrgOp::CreateRole {
        key: key.clone(),
        name: format!("Role n{n:05}"),
        charter: "Generated".into(),
        authority: AuthorityGrant::new(scopes, [0, 20_000, 80_000, 400_000][rng.below(4)]),
        reports_to: Some(RoleRef::existing(pick(rng, &live))),
        provider: if rng.chance(70) {
            Provider::Elastic
        } else {
            Provider::Creao
        },
        justification: RoleJustification::Lifecycle,
    };
    let ops = match rng.below(if live.len() <= 2 { 2 } else { 6 }) {
        // A role asks for a capability for its own requirement-backed duty.
        5 => {
            let owned: Vec<_> = st
                .responsibilities
                .values()
                .filter(|r| r.status == ResponsibilityStatus::Open)
                .filter_map(|r| match (&r.owner, &r.requirement_id) {
                    (Owner::Role(o), Some(q)) => Some((
                        r.purpose.clone(),
                        r.responsibility_id.clone(),
                        o.clone(),
                        q.clone(),
                    )),
                    _ => None,
                })
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect();
            match owned.get(rng.below(owned.len())) {
                Some((_, rid, owner, q)) => vec![OrgOp::RequestCapability {
                    key: format!("g{n}"),
                    requirement_id: q.clone(),
                    role: RoleRef::existing(owner.clone()),
                    responsibility: ResponsibilityRef::Existing {
                        responsibility_id: rid.clone(),
                    },
                    capability_id: CapabilityId::new(format!("venue-reader-{}", n % 4)),
                    purpose: "Read the venue portal".into(),
                    observed_sources: vec![if rng.chance(85) {
                        "https://portal.example.com/listing".into()
                    } else {
                        "https://elsewhere.example.org/".into()
                    }],
                }],
                None => vec![create],
            }
        }
        0 => match unmet.get(rng.below(unmet.len())) {
            Some((_, q)) => vec![
                create,
                OrgOp::AssignResponsibility {
                    target: ResponsibilityTarget::FromRequirement {
                        key: format!("d{n}"),
                        requirement_id: q.clone(),
                        purpose: format!("Duty n{n:05}"),
                        trigger: "t".into(),
                        termination: "e".into(),
                    },
                    owner: RoleRef::new_key(key),
                },
            ],
            None => vec![create],
        },
        1 => match open.get(rng.below(open.len())) {
            Some((_, rid, Owner::Role(from))) => vec![
                create,
                OrgOp::ReassignResponsibility {
                    responsibility_id: rid.clone(),
                    from: from.clone(),
                    to: RoleRef::new_key(key),
                },
            ],
            _ => vec![create],
        },
        2 => {
            let old = pick(rng, &live);
            let handoffs = open
                .iter()
                .filter(|(_, _, o)| o == &Owner::Role(old.clone()))
                .map(|(_, rid, _)| Handoff {
                    responsibility_id: rid.clone(),
                    to: RoleRef::new_key(key.clone()),
                })
                .collect();
            vec![
                create,
                OrgOp::RetireRole {
                    role_id: old,
                    handoffs,
                },
            ]
        }
        3 => {
            let old = pick(rng, &live);
            let heir = pick(rng, &live);
            let handoffs = open
                .iter()
                .filter(|(_, _, o)| o == &Owner::Role(old.clone()))
                .map(|(_, rid, _)| Handoff {
                    responsibility_id: rid.clone(),
                    to: RoleRef::existing(heir.clone()),
                })
                .collect();
            vec![OrgOp::RetireRole {
                role_id: old,
                handoffs,
            }]
        }
        _ => vec![OrgOp::ChangeReportingEdge {
            role: RoleRef::existing(pick(rng, &live)),
            reports_to: Some(RoleRef::existing(pick(rng, &live))),
        }],
    };
    OrgChangeProposal {
        expected_org_revision: st.revision,
        trigger_evidence_ids: obs
            .get(rng.below(obs.len()))
            .map(|(_, id)| id.clone())
            .into_iter()
            .collect(),
        rationale: format!("fuzz {n}"),
        operations: ops,
        budget_limit_cents: None,
    }
}

fn terminal(s: ChangeStatus) -> bool {
    matches!(
        s,
        ChangeStatus::Committed | ChangeStatus::Failed | ChangeStatus::Rejected
    )
}

async fn drain(app: &AppState) {
    for _ in 0..400 {
        let changes = app.store.list_changes().unwrap();
        let busy = changes.iter().any(|c| {
            !terminal(c.status)
                && !c.operations.values().any(|o| {
                    o.attention.is_some()
                        || matches!(o.state, ProviderOperationState::NeedsReconciliation)
                })
        });
        if !busy {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

async fn run(seed: u64, steps: u64) -> (u64, u64, u64) {
    let app = app(seed);
    let mission = bootstrap(&app).await;
    let mut rng = Rng(seed.max(1));
    act(&app, &mission, &mut rng, seed, 0..steps).await;
    drain(&app).await;
    tokio::time::sleep(Duration::from_millis(50)).await;
    verify_history(&app, seed)
}

async fn act(
    app: &AppState,
    mission: &MissionId,
    rng: &mut Rng,
    seed: u64,
    range: std::ops::Range<u64>,
) {
    let (app, mission) = (app.clone(), mission.clone());
    for n in range {
        let st = app.current_state().unwrap();
        match rng.below(100) {
            0..=14 => {
                app.mutate(EventMeta::default(), |s| {
                    engine::record_observation(
                        s,
                        &mission,
                        "fuzz",
                        &format!("Obs n{n:05}"),
                        Some(format!("https://portal.example.com/notice/{n}")),
                        Utc::now(),
                    )
                    .map_err(Into::into)
                })
                .await
                .unwrap();
            }
            15..=24 => {
                let mut obs: Vec<_> = st
                    .observations
                    .values()
                    .map(|o| (o.summary.clone(), o.observation_id.clone()))
                    .collect();
                obs.sort();
                if let Some((_, o)) = obs.get(rng.below(obs.len())).cloned() {
                    app.mutate(EventMeta::default(), |s| {
                        engine::record_requirement(
                            s,
                            &mission,
                            &format!("Req n{n:05}"),
                            std::slice::from_ref(&o),
                            "done",
                        )
                        .map_err(Into::into)
                    })
                    .await
                    .unwrap();
                }
            }
            25..=29 => {
                let mut open: Vec<_> = st
                    .responsibilities
                    .values()
                    .filter(|r| r.status == ResponsibilityStatus::Open)
                    .map(|r| (r.purpose.clone(), r.responsibility_id.clone()))
                    .collect();
                open.sort();
                if let Some((_, rid)) = open.get(rng.below(open.len())).cloned() {
                    let _ = app
                        .mutate(EventMeta::default(), |s| {
                            engine::complete_responsibility(s, &rid)
                                .map(|o| (o, ()))
                                .map_err(Into::into)
                        })
                        .await;
                }
            }
            30..=39 => drain(&app).await,
            _ => {
                // One proposal, or two racing against the same revision.
                let racers = if rng.chance(25) { 2 } else { 1 };
                for r in 0..racers {
                    let p = proposal(rng, &st, n * 10 + r);
                    let _ = saga::submit(&app, "fuzz", &format!("fuzz-{seed}-{n}-{r}"), p).await;
                }
            }
        }
        let now = app.current_state().unwrap();
        let v = check(&now);
        assert!(v.is_empty(), "seed {seed} step {n}: {v:#?}");
    }
}

/// Committed operations across every run in this process, by name.
static COMMITTED_OPS: Mutex<BTreeMap<&'static str, u64>> = Mutex::new(BTreeMap::new());

fn verify_history(app: &AppState, seed: u64) -> (u64, u64, u64) {
    // The server's own history audit: snapshots, transitions and events.
    let h = genesis_server::audit::audit_history(&app.store).unwrap();
    assert!(h.ok(), "seed {seed}: history audit: {:#?}", h.violations);
    // Whole-history checks.
    let head = app.current_state().unwrap();
    let mut prev: Option<OrgState> = None;
    let mut revisions = Vec::new();
    for r in 0..=head.revision {
        let s = app.store.state_at(r).unwrap();
        let v = check(&s);
        assert!(v.is_empty(), "seed {seed} r{r}: {v:#?}");
        if let Some(p) = &prev {
            let v = check_step(p, &s);
            assert!(v.is_empty(), "seed {seed} r{} -> r{r}: {v:#?}", p.revision);
        }
        prev = Some(s.clone());
        revisions.push(s);
    }
    let changes = app.store.list_changes().unwrap();
    let (mut committed, mut failed, mut unresolved) = (0, 0, 0);
    for c in &changes {
        let roles: BTreeSet<&RoleId> = c.allocation.roles.values().collect();
        match c.status {
            ChangeStatus::Committed => {
                committed += 1;
                for op in &c.proposal.operations {
                    *COMMITTED_OPS.lock().unwrap().entry(op.name()).or_insert(0) += 1;
                }
                let at = c.committed_revision.expect("committed revision");
                let snap = &revisions[at as usize];
                let before = &revisions[at as usize - 1];
                for id in &roles {
                    assert_eq!(
                        snap.roles[*id].state,
                        RoleState::Active,
                        "seed {seed}: {} role not active at r{at}",
                        c.change_id
                    );
                    assert!(
                        !before.roles.contains_key(*id),
                        "seed {seed}: role existed before its change committed"
                    );
                }
                assert_eq!(
                    c.proposal.expected_org_revision + 1,
                    at,
                    "seed {seed}: committed off its expected revision"
                );
            }
            ChangeStatus::Failed | ChangeStatus::Rejected => {
                failed += 1;
                for s in &revisions {
                    for id in &roles {
                        assert!(
                            !s.roles.contains_key(*id),
                            "seed {seed}: role of uncommitted {} appears at r{}",
                            c.change_id,
                            s.revision
                        );
                    }
                }
            }
            other => {
                unresolved += 1;
                let needs_operator = c.operations.values().any(|o| {
                    o.attention.is_some()
                        || matches!(o.state, ProviderOperationState::NeedsReconciliation)
                });
                assert!(needs_operator, "seed {seed}: change {} stuck in {other:?} without asking for an operator: {c:#?}", c.change_id);
                for s in &revisions {
                    for id in &roles {
                        assert!(
                            s.roles
                                .get(*id)
                                .is_none_or(|r| r.state != RoleState::Active),
                            "seed {seed}: unresolved change has an active role"
                        );
                    }
                }
            }
        }
    }
    let events = app.store.events_after(0, 100_000).unwrap();
    for w in events.windows(2) {
        assert!(
            w[0].seq < w[1].seq && w[0].revision <= w[1].revision,
            "seed {seed}: event order"
        );
    }
    (committed, failed, unresolved)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn saga_self_modification_keeps_every_revision_valid() {
    std::env::set_var("GENESIS_AUDIT_COMMITS", "1");
    let seeds: Vec<u64> = match std::env::var("GENESIS_SAGA_FUZZ_SEED") {
        Ok(s) => vec![s.parse().unwrap()],
        Err(_) => (1..=6).map(|i| i * 0x2545_F491).collect(),
    };
    let steps = std::env::var("GENESIS_SAGA_FUZZ_STEPS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(120);
    let mut totals = BTreeMap::new();
    for seed in &seeds {
        let (c, f, u) = run(*seed, steps).await;
        *totals.entry("committed").or_insert(0) += c;
        *totals.entry("failed_or_rejected").or_insert(0) += f;
        *totals.entry("unresolved_for_operator").or_insert(0) += u;
    }
    let ops = COMMITTED_OPS.lock().unwrap().clone();
    eprintln!(
        "saga fuzz: {} seeds x {steps} steps: {totals:?}; committed ops {ops:?}",
        seeds.len()
    );
    for op in [
        "CreateRole",
        "AssignResponsibility",
        "ReassignResponsibility",
        "RetireRole",
        "RequestCapability",
    ] {
        assert!(
            ops.get(op).copied().unwrap_or(0) > 0,
            "no committed {op}: the generator is not exercising it"
        );
    }
    assert!(totals["committed"] > 0 && totals["failed_or_rejected"] > 0);
}

/// Each "process lifetime" runs on its own Tokio runtime against one SQLite
/// file. The runtime is torn down at a random moment, cancelling sagas
/// mid-provisioning, mid-save or mid-commit. The next lifetime reopens the
/// file and runs the same startup recovery as `serve_org`
/// (`saga::resume_pending`). After the last restart every change must settle
/// and the whole history must pass the same checks as above, and no change
/// may record a provisioned binding twice for one role.
#[test]
fn crash_and_restart_mid_saga_never_corrupts_history() {
    std::env::set_var("GENESIS_AUDIT_COMMITS", "1");
    let seeds: Vec<u64> = match std::env::var("GENESIS_SAGA_FUZZ_SEED") {
        Ok(s) => vec![s.parse().unwrap()],
        Err(_) => (1..=3).map(|i| i * 0x9E37_79B9).collect(),
    };
    let lifetimes: u64 = std::env::var("GENESIS_SAGA_FUZZ_LIFETIMES")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(10);
    let mut totals = (0, 0, 0, 0usize);
    let mut resumed_total = 0usize;
    for seed in seeds {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("org.db");
        let mut rng = Rng(seed);
        let mut mission: Option<MissionId> = None;
        for life in 0..lifetimes {
            let rt = tokio::runtime::Builder::new_multi_thread()
                .worker_threads(2)
                .enable_all()
                .build()
                .unwrap();
            let work = rng.below(40) as u64 + 5;
            let crash_after = Duration::from_millis(rng.below(12) as u64);
            let db2 = db.clone();
            let m = mission.clone();
            let mut life_rng = Rng(rng.next() | 1);
            let (m2, resumed) = rt.block_on(async move {
                let app = app_with(seed ^ life, Some(db2), 80);
                let resumed = saga::resume_pending(&app).await.unwrap();
                let mission = match m {
                    Some(m) => m,
                    None => bootstrap(&app).await,
                };
                // Work, then keep going in the background until the crash.
                act(
                    &app,
                    &mission,
                    &mut life_rng,
                    seed,
                    life * 1000..life * 1000 + work,
                )
                .await;
                tokio::time::sleep(crash_after).await;
                (mission, resumed)
            });
            mission = Some(m2);
            resumed_total += resumed;
            // Crash: drop every task wherever it is.
            rt.shutdown_timeout(Duration::from_millis(0));
            totals.3 += 1;
        }
        // Final lifetime: recover, settle, verify.
        let rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .unwrap();
        let ((c, f, u), last_resumed) = rt.block_on(async {
            let app = app_with(seed ^ 0xFFFF, Some(db.clone()), 1);
            let resumed = saga::resume_pending(&app).await.unwrap();
            drain(&app).await;
            tokio::time::sleep(Duration::from_millis(100)).await;
            let events = app.store.events_after(0, 1_000_000).unwrap();
            let mut seen = BTreeSet::new();
            for e in &events {
                if let genesis_core::events::EventKind::RoleProvisioned {
                    change_id,
                    role_key,
                    ..
                } = &e.kind
                {
                    assert!(
                        seen.insert((change_id.clone(), role_key.clone())),
                        "seed {seed}: {change_id} recorded a binding for {role_key} twice"
                    );
                }
            }
            (verify_history(&app, seed), resumed)
        });
        rt.shutdown_timeout(Duration::from_millis(0));
        resumed_total += last_resumed;
        totals.0 += c;
        totals.1 += f;
        totals.2 += u;
    }
    eprintln!(
        "crash fuzz: {} crashes, {resumed_total} interrupted changes resumed; committed {}, failed/rejected {}, unresolved {}",
        totals.3, totals.0, totals.1, totals.2
    );
    assert!(totals.0 > 0);
    assert!(
        resumed_total > 0,
        "no crash interrupted a saga; the test would prove nothing"
    );
}
