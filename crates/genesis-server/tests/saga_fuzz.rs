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

/// What the fake provider was asked to do, kept across process lifetimes so
/// the oracles can see saga mistakes the kernel audit cannot.
#[derive(Default)]
struct Ledger {
    /// One entry per `provision` call: the role, and whether a binding for it
    /// was already recorded (a `role_provisioned` event) when the call came.
    calls: Mutex<Vec<(String, bool)>>,
    /// Roles whose readiness check answered not-ready or failed.
    readiness_failed: Mutex<BTreeSet<String>>,
}

/// Fails some creations outright, leaves a few with an unknown outcome, and
/// fails some readiness checks. Its own generator keeps provider behavior
/// independent of task timing.
struct Flaky {
    rng: Mutex<Rng>,
    max_delay_ms: u64,
    ledger: Arc<Ledger>,
    /// When set, `provision` checks the event log for an existing binding.
    db: Option<std::path::PathBuf>,
}

impl Flaky {
    fn already_bound(&self, role_id: &str) -> bool {
        let Some(db) = &self.db else { return false };
        let Ok(conn) =
            rusqlite::Connection::open_with_flags(db, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
        else {
            return false;
        };
        let _ = conn.busy_timeout(Duration::from_secs(5));
        conn.query_row(
            "SELECT COUNT(*) FROM events WHERE event_type = 'role_provisioned' AND payload_json LIKE ?1",
            [format!("%{role_id}%")],
            |r| r.get::<_, i64>(0),
        )
        .map(|n| n > 0)
        .unwrap_or(false)
    }
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
        let role_id = req.role.role_id.to_string();
        let bound = self.already_bound(&role_id);
        self.ledger.calls.lock().unwrap().push((role_id, bound));
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
    fn check_ready<'a, 'b, 'f>(&'a self, binding: &'b AgentBinding) -> ProviderFuture<'f, bool>
    where
        'a: 'f,
        'b: 'f,
        Self: 'f,
    {
        let (roll, delay) = {
            let mut rng = self.rng.lock().unwrap();
            (rng.below(100), rng.next() % self.max_delay_ms.max(1))
        };
        let role_id = binding
            .provider_agent_id
            .strip_prefix("fuzz-elastic-")
            .unwrap_or(&binding.provider_agent_id)
            .to_owned();
        let ledger = self.ledger.clone();
        Box::pin(async move {
            // Readiness takes time too, so a crash can land after the
            // binding is saved and before the role is ready.
            tokio::time::sleep(Duration::from_millis(delay)).await;
            let answer = match roll {
                0..=7 => Ok(false),
                8..=11 => Err(ProviderError::Http {
                    status: 503,
                    body: "fuzz readiness".into(),
                }),
                _ => Ok(true),
            };
            if !matches!(answer, Ok(true)) {
                ledger.readiness_failed.lock().unwrap().insert(role_id);
            }
            answer
        })
    }
}

fn app(seed: u64, ledger: Arc<Ledger>) -> AppState {
    app_with(seed, None, 7, ledger)
}

fn app_with(
    seed: u64,
    db: Option<std::path::PathBuf>,
    max_delay_ms: u64,
    ledger: Arc<Ledger>,
) -> AppState {
    let mut config = OrgConfig::dev("org_saga_fuzz", "Saga fuzz");
    config.provision_timeout = Duration::from_secs(5);
    config.db = db.clone();
    let flaky = Arc::new(Flaky {
        rng: Mutex::new(Rng(seed ^ 0xA5A5_5A5A)),
        max_delay_ms,
        ledger,
        db,
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
    let ledger = Arc::new(Ledger::default());
    let app = app(seed, ledger.clone());
    let mission = bootstrap(&app).await;
    let mut rng = Rng(seed.max(1));
    act(&app, &mission, &mut rng, seed, 0..steps).await;
    drain(&app).await;
    tokio::time::sleep(Duration::from_millis(50)).await;
    let totals = verify_history(&app, seed);
    READINESS_FAILURES.fetch_add(
        check_ledger(&app, seed, &ledger, false),
        std::sync::atomic::Ordering::SeqCst,
    );
    totals
}

/// Readiness failures the fake produced across all runs in this process.
static READINESS_FAILURES: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// Provider-side oracles. A role whose readiness failed never becomes active.
/// Without crashes each role is created at most once; across crashes a role
/// may be created again only if no binding for it was recorded yet (the
/// create can have been lost before its save).
fn check_ledger(app: &AppState, seed: u64, ledger: &Ledger, crashes: bool) -> usize {
    let head = app.current_state().unwrap();
    let failed = ledger.readiness_failed.lock().unwrap().clone();
    for r in 0..=head.revision {
        let s = app.store.state_at(r).unwrap();
        for id in &failed {
            let active = s
                .roles
                .values()
                .any(|role| role.role_id.as_str() == id && role.state == RoleState::Active);
            assert!(
                !active,
                "seed {seed}: role {id} failed readiness but is active at r{r}"
            );
        }
    }
    let calls = ledger.calls.lock().unwrap().clone();
    if crashes {
        for (role, bound) in &calls {
            assert!(
                !bound,
                "seed {seed}: role {role} was created again after its binding was recorded"
            );
        }
    } else {
        let mut per_role: BTreeMap<&str, usize> = BTreeMap::new();
        for (role, _) in &calls {
            *per_role.entry(role.as_str()).or_insert(0) += 1;
        }
        for (role, n) in per_role {
            assert!(
                n <= 1,
                "seed {seed}: role {role} was created {n} times without a crash"
            );
        }
    }
    failed.len()
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
    // The readiness oracle proves nothing unless the fake failed some checks.
    assert!(
        READINESS_FAILURES.load(std::sync::atomic::Ordering::SeqCst) > 0,
        "no readiness failure was exercised"
    );
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
    // How often a crash caught a change at each interesting point.
    let mut crashed_ready = 0usize;
    let mut crashed_bound_not_ready = 0usize;
    for seed in seeds {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("org.db");
        let ledger = Arc::new(Ledger::default());
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
            let hold_commits = rng.chance(50);
            let ledger2 = ledger.clone();
            let (m2, resumed, at_crash) = rt.block_on(async move {
                let app = app_with(seed ^ life, Some(db2), 80, ledger2);
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
                if hold_commits {
                    // Hold the kernel lock so sagas that reach commit wait
                    // in READY; the crash then lands between READY and commit.
                    let kernel = app.kernel.clone();
                    tokio::spawn(async move {
                        let _held = kernel.lock_owned().await;
                        std::future::pending::<()>().await;
                    });
                    tokio::time::sleep(Duration::from_millis(150)).await;
                }
                tokio::time::sleep(crash_after).await;
                let changes = app.store.list_changes().unwrap_or_default();
                let ready = changes
                    .iter()
                    .filter(|c| c.status == ChangeStatus::Ready)
                    .count();
                let bound = changes
                    .iter()
                    .filter(|c| {
                        c.status == ChangeStatus::Provisioning
                            && c.provisioned.keys().any(|k| !c.ready.contains(k))
                    })
                    .count();
                (mission, resumed, (ready, bound))
            });
            mission = Some(m2);
            resumed_total += resumed;
            crashed_ready += at_crash.0;
            crashed_bound_not_ready += at_crash.1;
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
            let app = app_with(seed ^ 0xFFFF, Some(db.clone()), 1, ledger.clone());
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
            let totals = verify_history(&app, seed);
            READINESS_FAILURES.fetch_add(
                check_ledger(&app, seed, &ledger, true),
                std::sync::atomic::Ordering::SeqCst,
            );
            (totals, resumed)
        });
        rt.shutdown_timeout(Duration::from_millis(0));
        resumed_total += last_resumed;
        totals.0 += c;
        totals.1 += f;
        totals.2 += u;
    }
    eprintln!(
        "crash fuzz: {} crashes, {resumed_total} interrupted changes resumed ({crashed_ready} caught in READY, {crashed_bound_not_ready} with a saved binding not yet ready); committed {}, failed/rejected {}, unresolved {}",
        totals.3, totals.0, totals.1, totals.2
    );
    assert!(
        crashed_ready > 0,
        "no crash landed between READY and commit"
    );
    assert!(
        crashed_bound_not_ready > 0,
        "no crash landed between a saved binding and readiness"
    );
    assert!(totals.0 > 0);
    assert!(
        resumed_total > 0,
        "no crash interrupted a saga; the test would prove nothing"
    );
}

/// A refusal at commit that is not a revision conflict must end FAILED, with
/// no committed revision and none of the change's roles in any snapshot. The
/// fuzzers rarely reach that branch, so it is forced here: the process stops
/// while the change waits in READY, its saved binding is removed, and on
/// restart the kernel refuses to activate an unprovisioned role.
#[test]
fn a_non_conflict_commit_refusal_is_recorded_as_failed() {
    std::env::set_var("GENESIS_AUDIT_COMMITS", "1");
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("org.db");
    let ledger = Arc::new(Ledger::default());
    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .unwrap();
    let change_id = rt.block_on(async {
        // Seed chosen so the first creation and readiness both succeed.
        let app = app_with(0, Some(db.clone()), 1, ledger.clone());
        let mission = bootstrap(&app).await;
        app.mutate(EventMeta::default(), |s| {
            engine::record_observation(s, &mission, "t", "Obs", None, Utc::now()).map_err(Into::into)
        })
        .await
        .unwrap();
        let st = app.current_state().unwrap();
        let obs = st.observations.keys().next().unwrap().clone();
        let proposal: OrgChangeProposal = serde_json::from_value(json!({
            "expected_org_revision": st.revision, "trigger_evidence_ids": [obs],
            "rationale": "A scout takes over duty a",
            "operations": [
              {"op":"CreateRole","key":"scout","name":"Scout","charter":"Scout",
               "authority":{"scopes":["mission.coordinate"],"budget_cents":0},
               "reports_to": null, "provider":"elastic","justification":"lifecycle"},
              {"op":"ReassignResponsibility",
               "responsibility_id": st.responsibilities.values().find(|r| r.purpose == "Duty a").unwrap().responsibility_id,
               "from": st.roles.values().find(|r| r.name == "Role a").unwrap().role_id,
               "to": {"kind":"new","key":"scout"}}]
        }))
        .unwrap();
        // Submit first (submission validates under the kernel lock), then
        // hold the kernel lock so the change parks in READY before commit.
        let (_, record) = saga::submit(&app, "fuzz", "forced", proposal).await.unwrap();
        let kernel = app.kernel.clone();
        tokio::spawn(async move {
            let _g = kernel.lock_owned().await;
            std::future::pending::<()>().await;
        });
        for _ in 0..200 {
            if app.store.get_change(&record.change_id).unwrap().status == ChangeStatus::Ready {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        record.change_id
    });
    rt.shutdown_timeout(Duration::from_millis(0));
    // Remove the saved binding from the parked change.
    {
        let conn = rusqlite::Connection::open(&db).unwrap();
        conn.busy_timeout(Duration::from_secs(5)).unwrap();
        let json: String = conn
            .query_row(
                "SELECT record_json FROM org_changes WHERE change_id = ?1",
                [change_id.as_str()],
                |r| r.get(0),
            )
            .unwrap();
        let mut rec: OrgChangeRecord = serde_json::from_str(&json).unwrap();
        assert_eq!(
            rec.status,
            ChangeStatus::Ready,
            "the change must be parked in READY"
        );
        rec.provisioned.clear();
        conn.execute(
            "UPDATE org_changes SET record_json = ?2 WHERE change_id = ?1",
            rusqlite::params![change_id.as_str(), serde_json::to_string(&rec).unwrap()],
        )
        .unwrap();
    }
    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .unwrap();
    rt.block_on(async {
        let app = app_with(1, Some(db.clone()), 1, ledger.clone());
        saga::resume_pending(&app).await.unwrap();
        drain(&app).await;
        let rec = app.store.get_change(&change_id).unwrap();
        assert_eq!(rec.status, ChangeStatus::Failed, "{rec:#?}");
        assert_eq!(rec.committed_revision, None);
        assert!(rec
            .errors
            .iter()
            .any(|e| e.code == genesis_core::engine::ValidationCode::NotProvisioned));
        verify_history(&app, 0);
    });
    rt.shutdown_timeout(Duration::from_millis(0));
}
