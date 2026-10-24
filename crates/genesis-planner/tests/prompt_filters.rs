//! Henry's Claude: the state message shows only what the planner may act on
//! for this observation's mission. Built from real engine transitions; two
//! entities are then moved to terminal states directly.
use chrono::{Duration, Utc};
use genesis_core::engine::{self, MissionBootstrap};
use genesis_core::ids::{MissionId, ObservationId, RequirementId};
use genesis_core::lifecycle::{ResponsibilityStatus, RoleState};
use genesis_core::model::OrgState;
use genesis_planner::prompt::user_message;
use genesis_server::{build_org, OrgConfig};
use serde_json::json;

fn mission(state: &OrgState, goal: &str, roles: &[&str]) -> (OrgState, MissionId) {
    let spec: MissionBootstrap = serde_json::from_value(json!({
        "goal": goal,
        "roles": roles.iter().map(|k| json!({"key": k, "name": format!("Role {k}"),
            "charter": format!("Charter {k}"),
            "authority": {"scopes": ["mission.coordinate"], "budget_cents": 0}})).collect::<Vec<_>>(),
        "responsibilities": roles.iter().map(|k| json!({"purpose": format!("Purpose {k}"), "owner": k}))
            .collect::<Vec<_>>(),
    }))
    .unwrap();
    let (out, id) = engine::bootstrap_mission(state, &spec, Utc::now()).unwrap();
    (out.state, id)
}

fn observe(
    state: &OrgState,
    m: &MissionId,
    summary: &str,
    minutes_ago: i64,
) -> (OrgState, ObservationId) {
    let at = Utc::now() - Duration::minutes(minutes_ago);
    let (out, id) = engine::record_observation(state, m, "test", summary, None, at).unwrap();
    (out.state, id)
}

fn require(
    state: &OrgState,
    m: &MissionId,
    summary: &str,
    evidence: &ObservationId,
) -> (OrgState, RequirementId) {
    let (out, id) =
        engine::record_requirement(state, m, summary, std::slice::from_ref(evidence), "checked")
            .unwrap();
    (out.state, id)
}

/// The text of one `## <title>` section, or `None` if it is absent.
fn section<'a>(message: &'a str, title: &str) -> Option<&'a str> {
    let head = format!("## {title}\n");
    let start = message.find(&head)? + head.len();
    let rest = &message[start..];
    Some(&rest[..rest.find("\n## ").unwrap_or(rest.len())])
}

#[test]
fn prompt_shows_only_live_open_same_mission_entities_and_other_evidence() {
    let initial = build_org(&OrgConfig::dev("org_prompt", "Prompt"), None)
        .unwrap()
        .current_state()
        .unwrap();
    let (s, a) = mission(&initial, "Mission A goal", &["keep", "gone", "done"]);
    let (s, b) = mission(&s, "Mission B goal", &["other"]);
    let (s, a_earlier) = observe(&s, &a, "A earlier report", 10);
    let (s, b_obs) = observe(&s, &b, "B unrelated report", 5);
    let (s, a_now) = observe(&s, &a, "A current report", 0);
    let (s, req_a) = require(&s, &a, "Requirement for A", &a_earlier);
    let (mut s, req_b) = require(&s, &b, "Requirement for B", &b_obs);

    let role_id = |s: &OrgState, name: &str| {
        s.roles
            .values()
            .find(|r| r.name == name)
            .unwrap()
            .role_id
            .clone()
    };
    let resp_id = |s: &OrgState, purpose: &str| {
        s.responsibilities
            .values()
            .find(|r| r.purpose == purpose)
            .unwrap()
            .responsibility_id
            .clone()
    };
    let (keep, gone) = (role_id(&s, "Role keep"), role_id(&s, "Role gone"));
    let (open, closed) = (resp_id(&s, "Purpose keep"), resp_id(&s, "Purpose done"));
    s.roles.get_mut(&gone).unwrap().state = RoleState::Retired;
    s.responsibilities.get_mut(&closed).unwrap().status = ResponsibilityStatus::Completed;

    let current = s.observations[&a_now].clone();
    let message = user_message(&s, &current, &[]);

    let roles = section(&message, "Roles").unwrap();
    assert!(roles.contains(&keep.to_string()), "{roles}");
    assert!(
        !roles.contains(&gone.to_string()),
        "retired role shown: {roles}"
    );
    assert!(!roles.contains("Role gone"), "{roles}");

    let open_section = section(&message, "Open responsibilities").unwrap();
    assert!(open_section.contains(&open.to_string()), "{open_section}");
    assert!(
        !open_section.contains(&closed.to_string()),
        "completed responsibility shown as open: {open_section}"
    );
    assert!(!open_section.contains("(none)"));

    let reqs = section(&message, "Requirements already recorded").unwrap();
    assert!(reqs.contains(&req_a.to_string()), "{reqs}");
    assert!(
        !reqs.contains(&req_b.to_string()),
        "other mission's requirement shown: {reqs}"
    );

    let earlier = section(&message, "Earlier evidence").unwrap();
    assert!(earlier.contains(&a_earlier.to_string()), "{earlier}");
    assert!(
        !earlier.contains(&b_obs.to_string()),
        "other mission's evidence shown: {earlier}"
    );
    assert!(
        !earlier.contains(&a_now.to_string()),
        "current observation listed as earlier evidence: {earlier}"
    );
    assert!(!message.contains("B unrelated report"));

    let new = section(&message, "New evidence").unwrap();
    assert!(new.contains(&format!("- id: {a_now}")), "{new}");
}

#[test]
fn prompt_omits_sections_with_nothing_relevant() {
    let initial = build_org(&OrgConfig::dev("org_prompt", "Prompt"), None)
        .unwrap()
        .current_state()
        .unwrap();
    let (s, a) = mission(&initial, "Mission A goal", &["keep"]);
    let (s, b) = mission(&s, "Mission B goal", &["other"]);
    let (s, b_obs) = observe(&s, &b, "B unrelated report", 5);
    let (s, _) = require(&s, &b, "Requirement for B", &b_obs);
    let (mut s, a_now) = observe(&s, &a, "A current report", 0);
    for r in s.responsibilities.values_mut() {
        r.status = ResponsibilityStatus::Cancelled;
    }
    let message = user_message(&s, &s.observations[&a_now], &[]);
    assert_eq!(
        section(&message, "Open responsibilities").map(str::trim),
        Some("(none)")
    );
    assert!(section(&message, "Requirements already recorded").is_none());
    assert!(section(&message, "Earlier evidence").is_none());
}
