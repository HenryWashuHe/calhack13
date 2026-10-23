//! Henry's Claude: the autonomy loop closes. A planner-requested capability
//! moves through the server's gap transitions to `installed`; the kernel
//! records one observation in the same commit; the runner plans on it and the
//! scripted model attaches the capability, which the real kernel commits.

use chrono::Utc;
use genesis_core::engine::{Outcome, CAPABILITY_GAP_SOURCE};
use genesis_core::ids::{CapabilityGapId, CapabilityId, InstallationId};
use genesis_core::lifecycle::InstallationStatus;
use genesis_core::model::{CapabilityGapStatus as S, CapabilityRef, Installation};
use genesis_planner::agent::{PlanOutcome, Planner};
use genesis_planner::kernel::KernelClient;
use genesis_planner::llm::ReplayClient;
use genesis_planner::runner::Runner;
use genesis_server::store::EventMeta;
use genesis_server::{build_org, capability_gaps, spawn_org, OrgConfig};
use serde_json::{json, Value};
use std::sync::Arc;
use std::time::Duration;

const SOURCE: &str = "https://portal.example/listings/index.html";

#[tokio::test]
async fn installed_gap_wakes_the_planner_and_the_capability_is_attached() {
    let cfg = OrgConfig::dev("org_a", "Org A");
    let app = build_org(&cfg, None).unwrap();
    let addr = spawn_org(app.clone(), "127.0.0.1:0").await.unwrap();
    let url = format!("http://{addr}");
    let http = reqwest::Client::new();
    let op = |path: &str| format!("{url}/api/v1{path}");
    let post = |path: &str, body: Value| {
        let req = http
            .post(op(path))
            .bearer_auth(&cfg.tokens.operator)
            .json(&body);
        async move { req.send().await.unwrap().json::<Value>().await.unwrap() }
    };
    let m = post(
        "/missions",
        json!({"goal": "Deliver the program", "roles": [{"key": "lead", "name": "Lead", "charter": "Owns the plan",
               "authority": {"scopes": ["mission.coordinate"], "budget_cents": 10000}}],
               "responsibilities": [{"purpose": "Coordinate the plan", "owner": "lead"}]}),
    )
    .await;
    let mission = m["mission_id"].as_str().unwrap().to_owned();
    post(
        &format!("/missions/{mission}/observations"),
        json!({"source": "portal", "summary": "The listing site changed to a layout nothing here can read",
               "evidence_ref": SOURCE}),
    )
    .await;
    let lead = app
        .current_state()
        .unwrap()
        .roles
        .values()
        .find(|r| r.name == "Lead")
        .unwrap()
        .role_id
        .to_string();

    let request = json!({
        "assessment": "No installed capability reads this source.", "verdict": "change",
        "requirement": {"summary": "Read options from the listing site", "acceptance": "options extracted"},
        "rationale": "The lead owns planning but lacks a reader.", "budget_limit_cents": 0,
        "operations": [
            {"op": "AssignResponsibility", "target": {"kind": "from_requirement", "key": "read", "requirement_id": "$REQUIREMENT",
              "purpose": "Keep listing options current", "trigger": "listing changes", "termination": "plan fixed"},
             "owner": {"kind": "existing", "role_id": lead}},
            {"op": "RequestCapability", "key": "reader", "requirement_id": "$REQUIREMENT",
             "role": {"kind": "existing", "role_id": lead}, "responsibility": {"kind": "new", "key": "read"},
             "capability_id": "listing-reader", "purpose": "Extract options from the listing site",
             "observed_sources": [SOURCE]}
        ]
    })
    .to_string();
    let attach = json!({
        "assessment": "The requested reader is installed and approved.", "verdict": "change",
        "requirement": {"summary": "Use the installed listing reader", "acceptance": "the lead holds listing-reader@1"},
        "rationale": "Attach the capability the lead requested.", "budget_limit_cents": 0,
        "operations": [
            {"op": "AttachCapability", "role": {"kind": "existing", "role_id": lead},
             "capability": {"capability_id": "listing-reader", "version": "1"}}
        ]
    })
    .to_string();
    let model = Arc::new(ReplayClient::new([request, attach]));
    let planner = Planner::new(KernelClient::new(&url, &cfg.tokens.agent), model.clone());
    let mut runner = Runner::new(planner, Duration::from_millis(10));

    // Pass 1: the portal observation becomes an open gap.
    let first = runner.tick().await.unwrap();
    assert_eq!(first.len(), 1);
    assert!(
        matches!(first[0].1, PlanOutcome::Proposed { .. }),
        "{first:?}"
    );
    wait(|| !app.current_state().unwrap().capability_gaps.is_empty()).await;
    let gap: CapabilityGapId = app
        .current_state()
        .unwrap()
        .capability_gaps
        .keys()
        .next()
        .unwrap()
        .clone();
    // Nothing new to plan while the gap is in flight.
    capability_gaps::transition(
        &app,
        &gap,
        S::Acquiring {
            acquisition_id: "acq-1".into(),
        },
    )
    .await
    .unwrap();
    let cap = CapabilityRef {
        capability_id: CapabilityId::new("listing-reader"),
        version: "1".into(),
    };
    capability_gaps::transition(
        &app,
        &gap,
        S::Acquired {
            acquisition_id: "acq-1".into(),
            capability: cap.clone(),
            publication_ref: None,
        },
    )
    .await
    .unwrap();
    assert!(
        runner.tick().await.unwrap().is_empty(),
        "acquired is not installed"
    );

    // Stand-in for local installation and acceptance (covered elsewhere).
    let inst = InstallationId::new("inst_reader");
    let c = cap.clone();
    let i = inst.clone();
    app.mutate(EventMeta::default(), move |s| {
        let mut st = s.clone();
        st.installations.insert(
            i.clone(),
            Installation {
                installation_id: i,
                capability: c,
                status: InstallationStatus::Accepted,
                configuration: Value::Null,
                acceptance_report_ref: None,
                source_digest: "sha256:test".into(),
                installed_at: Utc::now(),
            },
        );
        st.revision += 1;
        Ok((
            Outcome {
                state: st,
                effects: vec![],
            },
            (),
        ))
    })
    .await
    .unwrap();
    let installed = S::Installed {
        acquisition_id: "acq-1".into(),
        capability: cap.clone(),
        installation_id: inst,
    };
    capability_gaps::transition(&app, &gap, installed.clone())
        .await
        .unwrap();
    // A repeated transition (lost response, second driver) adds no second note.
    capability_gaps::transition(&app, &gap, installed)
        .await
        .unwrap();
    let notes = app
        .current_state()
        .unwrap()
        .observations
        .into_values()
        .filter(|o| o.source == CAPABILITY_GAP_SOURCE)
        .count();
    assert_eq!(notes, 1);

    // Pass 2: the kernel's note wakes the planner; the attach commits.
    let second = runner.tick().await.unwrap();
    assert_eq!(second.len(), 1, "{second:?}");
    assert!(
        matches!(second[0].1, PlanOutcome::Proposed { .. }),
        "{second:?}"
    );
    wait(|| {
        app.current_state()
            .unwrap()
            .roles
            .values()
            .any(|r| r.name == "Lead" && r.capabilities.contains(&cap))
    })
    .await;
    // The second prompt carried the installed capability and the note.
    let prompts = model.requests();
    assert_eq!(prompts.len(), 2);
    let user = &prompts[1].messages[0].content;
    assert!(user.contains("listing-reader@1 (approved)"), "{user}");
    assert!(user.contains(CAPABILITY_GAP_SOURCE), "{user}");
    assert!(runner.tick().await.unwrap().is_empty(), "the loop settles");
}

async fn wait(mut ok: impl FnMut() -> bool) {
    for _ in 0..200 {
        if ok() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    panic!("condition not reached");
}
