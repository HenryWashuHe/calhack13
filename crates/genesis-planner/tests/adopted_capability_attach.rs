//! Henry's Claude: the adopt-first loop closes. A planner-requested capability
//! (`listing-reader`) is satisfied by adopting a differently named foreign
//! capability (`shared-venue-reader@1.0.0`) through the backend-only kernel
//! transition. The kernel's note wakes the runner, the prompt names the
//! adopted version, and the scripted model's attach of that foreign ID is
//! committed by the real kernel.

use chrono::Utc;
use genesis_core::engine::{self, AdoptionInstall, CAPABILITY_GAP_SOURCE};
use genesis_core::ids::{CapabilityGapId, CapabilityId, InstallationId};
use genesis_core::lifecycle::InstallationStatus;
use genesis_core::model::{CapabilityGapStatus as S, CapabilityRef, Installation};
use genesis_planner::agent::{PlanOutcome, Planner};
use genesis_planner::kernel::KernelClient;
use genesis_planner::llm::ReplayClient;
use genesis_planner::runner::Runner;
use genesis_server::store::EventMeta;
use genesis_server::{build_org, spawn_org, OrgConfig};
use serde_json::{json, Value};
use std::sync::Arc;
use std::time::Duration;

const SOURCE: &str = "https://portal.example/listings/index.html";

#[tokio::test]
async fn an_adopted_capability_with_another_name_is_attached_after_the_note() {
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
        "assessment": "The requested reader was adopted as shared-venue-reader@1.0.0.", "verdict": "change",
        "requirement": {"summary": "Use the adopted reader", "acceptance": "the lead holds shared-venue-reader@1.0.0"},
        "rationale": "Attach the capability installed for the lead's request.", "budget_limit_cents": 0,
        "operations": [
            {"op": "AttachCapability", "role": {"kind": "existing", "role_id": lead},
             "capability": {"capability_id": "shared-venue-reader", "version": "1.0.0"}}
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
    // While adoption works, the gap stays open: nothing new to plan.
    assert!(
        runner.tick().await.unwrap().is_empty(),
        "open gap, no new input"
    );

    // Stand-in for B's adoption: an accepted installation of a foreign
    // capability with another name (adoption itself is covered server-side).
    let cap = CapabilityRef {
        capability_id: CapabilityId::new("shared-venue-reader"),
        version: "1.0.0".into(),
    };
    let inst = InstallationId::new("inst_adopted");
    let (c, i, g) = (cap.clone(), inst.clone(), gap.clone());
    app.mutate(EventMeta::default(), move |s| {
        let mut st = s.clone();
        st.installations.insert(
            i.clone(),
            Installation {
                installation_id: i.clone(),
                capability: c.clone(),
                status: InstallationStatus::Accepted,
                configuration: Value::Null,
                acceptance_report_ref: Some("report:adopted".into()),
                source_digest: "sha256:test".into(),
                installed_at: Utc::now(),
            },
        );
        st.revision += 1;
        let done = engine::install_capability_gap_by_adoption_at(
            &st,
            &g,
            &AdoptionInstall {
                adoption_id: "adopt_fixture".into(),
                capability: c,
                installation_id: i,
            },
            Utc::now(),
        )
        .map_err(Into::<genesis_server::error::ApiError>::into)?;
        Ok((done, ()))
    })
    .await
    .unwrap();
    assert!(matches!(
        app.current_state().unwrap().capability_gaps[&gap].status,
        S::Installed { .. }
    ));
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
    assert!(
        user.contains("shared-venue-reader@1.0.0 (approved)"),
        "{user}"
    );
    assert!(
        user.contains("listing-reader for role")
            && user.contains("installed as shared-venue-reader@1.0.0; attach it"),
        "{user}"
    );
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
