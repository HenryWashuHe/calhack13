//! Offline G3 plumbing rehearsal: B revises the exact method it holds (A's),
//! then the revised child is compared against that exact parent under a
//! frozen criterion, and the combined cost report charges the revision.
//!
//! Real code on every seam: the forge's `MethodRevisionSpec` puts the parent's
//! canonical bytes into the frozen generation request, one completion comes
//! back over loopback HTTP from a local fixture server (not a model), the
//! observation binds child to parent, both packages run as real child
//! processes (sandbox mode `none`, so not namespace evidence) and the trusted
//! adapter, verdict and cost report read the same retained evidence.
//!
//! The parent is the hand-written fixed-cart control and the "revision" is the
//! hand-written random-search control. This checks that a revision can be
//! judged against its own parent, not that any revision improved anything.
use genesis_core::development_evaluation::EvaluationOutcome;
use genesis_core::development_method::{ExecutionOutcome, ResourceGrant};
use genesis_core::development_package::{MethodPackage, MethodSourceFile};
use genesis_core::development_trial::TrialEvidence;
use genesis_forge::method_accounting::{
    assess_costs, CostScope, GeneratedMethodLink, GenerationExpectation, TrialExpectation,
};
use genesis_forge::method_revision::MethodRevisionSpec;
use genesis_forge::method_synthesis::{MethodSynthesisPhase, MethodSynthesisPlan};
use genesis_forge::text_generation::GenerationTransport;
use genesis_forge::AnthropicGenerator;
use genesis_lab_eval::family::Family;
use genesis_lab_exec::store::Store;
use genesis_lab_exec::{example, refs, run, sandbox::Mode};
use genesis_lab_score::score_slot;
use genesis_lab_score::verdict::{decide, SlotEvidence, Verdict};
use std::path::{Path, PathBuf};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

const MODEL: &str = "rehearsal-fixture-model";
const PARENT: &str = include_str!("../../executor/methods/fixed-cart/fixed_cart.py");
const REVISED: &str = include_str!("../../executor/methods/random-search/random_search.py");

/// Serves exactly one HTTP response and returns the request body it received.
async fn serve_once(body: Vec<u8>) -> (String, tokio::task::JoinHandle<Vec<u8>>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}/v1/messages", listener.local_addr().unwrap());
    let task = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut bytes = vec![];
        let body_start = loop {
            let mut buf = [0; 8192];
            let n = socket.read(&mut buf).await.unwrap();
            assert!(n > 0, "request cut short");
            bytes.extend_from_slice(&buf[..n]);
            if let Some(b) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
                let len: usize = String::from_utf8_lossy(&bytes[..b])
                    .lines()
                    .find_map(|l| {
                        l.to_ascii_lowercase()
                            .strip_prefix("content-length: ")
                            .and_then(|s| s.parse().ok())
                    })
                    .unwrap();
                if bytes.len() >= b + 4 + len {
                    break b + 4;
                }
            }
        };
        let head = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        );
        socket.write_all(head.as_bytes()).await.unwrap();
        socket.write_all(&body).await.unwrap();
        bytes.split_off(body_start)
    });
    (endpoint, task)
}

fn tmp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("lab-g3-rehearsal-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    d
}

fn output(dir: &Path, receipt: &genesis_core::development_method::ExecutionReceipt) -> Vec<u8> {
    std::fs::read(dir.join("artifacts").join(&receipt.outputs[0].sha256)).unwrap()
}

#[tokio::test(flavor = "current_thread")]
async fn revision_is_judged_against_its_exact_parent_offline() {
    let interpreter = run::interpreter_version("python3");
    let family = Family::RavineCrossing;

    // 1. The parent B holds: A's method, as exact canonical package bytes.
    let parent = MethodPackage::new(
        "fixed-cart".into(),
        "0.1.0".into(),
        refs::runtime(&interpreter),
        refs::protocol(),
        "fixed_cart.py".into(),
        vec![],
        vec![MethodSourceFile {
            path: "fixed_cart.py".into(),
            content: PARENT.into(),
        }],
    )
    .unwrap();
    let parent_bytes = parent.artifact_json().unwrap();

    // 2. B's revision request, through the forge's own spec, session and transport.
    let mut generation = example::synthesis_plan(family, MODEL, "revision-1", &interpreter);
    generation["org_id"] = "org_example_b".into();
    generation["target_id"] = "revised-method".into();
    let generation: MethodSynthesisPlan = serde_json::from_value(generation).unwrap();
    let spec = MethodRevisionSpec {
        generation,
        parent: parent.reference().unwrap(),
    };
    let mut session = spec.prepare(parent_bytes.as_bytes()).unwrap();
    // The exact parent bytes are the frozen request's final public input.
    let last = session.plan().public_inputs.last().unwrap();
    assert_eq!(last.reference, spec.parent);
    assert_eq!(last.text, parent_bytes);
    let request = session.begin().unwrap();
    let reply = serde_json::json!({
        "id": "msg_revision_1", "type": "message", "role": "assistant", "model": MODEL,
        "content": [{"type": "text", "text": serde_json::json!({"source": REVISED}).to_string()}],
        "stop_reason": "end_turn", "stop_sequence": null,
        "usage": {"input_tokens": 3000, "output_tokens": 900},
    });
    let (endpoint, server) = serve_once(serde_json::to_vec(&reply).unwrap()).await;
    let completion = AnthropicGenerator::with_test_endpoint(&endpoint, "fixture-key")
        .unwrap()
        .generate_text(&request)
        .await
        .unwrap();
    assert_eq!(server.await.unwrap(), request.wire_bytes().unwrap());
    assert_eq!(completion.transport, GenerationTransport::LoopbackFixture);
    session.complete(completion).unwrap();
    assert_eq!(session.phase(), MethodSynthesisPhase::Generated);

    // 3. The observation binds the child to its parent; a changed source is
    // a byte-level fact only.
    let seen = spec.observe(parent_bytes.as_bytes(), &session).unwrap();
    assert!(seen.source_changed);
    assert_eq!(seen.org_id, "org_example_b");
    let child = session.package().unwrap();
    assert_eq!(seen.child, child.reference().unwrap());
    let child_bytes = child.artifact_json().unwrap();

    // 4. The comparison is the revision against its exact parent, nothing else.
    let methods = example::ExampleMethods {
        candidate: seen.child.clone(),
        control: seen.parent.clone(),
    };
    let grant = ResourceGrant {
        model_calls: 0,
        input_tokens: 0,
        output_tokens: 0,
        wall_time_ms: 60_000,
        simulation_steps: 6 * 2400,
    };
    let mut trial =
        example::acceptance_plan_for(family, &methods, Mode::None, &interpreter, 3, grant);
    let ids: Vec<String> = trial
        .development_instances
        .iter()
        .map(|r| r.id.clone())
        .collect();
    // Mode `none` can't establish zero model use; the criterion says so.
    let criterion = genesis_core::digest::canonical_json(&serde_json::json!({
        "rule": "success-count-margin",
        "min_margin": 1,
        "instances": ids,
        "unknown_usage_allowed": ["model_calls", "input_tokens", "output_tokens"],
    }))
    .into_bytes();
    trial.criterion = refs::content("lab-criterion/1", "success-count-margin", "1", &criterion);
    trial.validate().unwrap();
    assert_eq!(
        trial.expected_binding("candidate").unwrap().method,
        seen.child
    );
    assert_eq!(
        trial.expected_binding("control").unwrap().method,
        seen.parent
    );

    // 5. Both slots as real child processes, then trusted scoring.
    let cfg = run::Config {
        sandbox: Mode::None,
        interpreter_version: interpreter.clone(),
        stage_root: tmp("stage"),
    };
    // The plan refuses the slots' packages swapped: identity is checked first.
    let swapped = tmp("swapped");
    assert!(run::execute(&trial, "control", child_bytes.as_bytes(), &swapped, &cfg).is_err());
    assert!(!swapped.join("ledger.json").exists());
    let mut evidence = vec![];
    for (slot, bytes) in [
        ("candidate", child_bytes.as_bytes()),
        ("control", parent_bytes.as_bytes()),
    ] {
        let state = tmp(slot);
        let done = run::execute(&trial, slot, bytes, &state, &cfg).unwrap();
        assert_eq!(
            done.receipt.outcome,
            ExecutionOutcome::OutputGenerated,
            "{slot}"
        );
        let score = tmp(&format!("{slot}-score"));
        let store = Store::open(&score).unwrap();
        let scored = score_slot(
            &trial,
            slot,
            &done.receipt,
            &output(&state, &done.receipt),
            &store,
        )
        .unwrap();
        let EvaluationOutcome::Scored { metrics } = &scored.outcome else {
            panic!("{slot}: {:?}", scored.outcome)
        };
        let metrics = std::fs::read(score.join("artifacts").join(&metrics.sha256)).unwrap();
        evidence.push(SlotEvidence {
            execution: done.receipt,
            scoring: Some(scored),
            metrics: Some(metrics),
        });
    }
    let verdict = decide(&trial, &criterion, &evidence[0], &evidence[1]).unwrap();
    eprintln!("revision rehearsal verdict (controls only): {verdict:?}");
    assert!(
        matches!(verdict, Verdict::Accepted { .. } | Verdict::Rejected { .. }),
        "{verdict:?}"
    );

    // 6. The revision's generation is charged once and linked to the
    // candidate slot by exact package; the parent's own development is not
    // part of this scope.
    let scope = CostScope {
        id: "revision-rehearsal-cost".into(),
        generations: vec![GenerationExpectation {
            org_id: seen.org_id.clone(),
            operation_id: seen.operation_id.clone(),
            plan_sha256: session.plan().sha256().unwrap(),
            request_sha256: seen.request_sha256.clone(),
            checkpoint_sha256: seen.checkpoint_sha256.clone(),
        }],
        trial: Some(TrialExpectation {
            plan: trial.reference().unwrap(),
            slots: vec!["candidate".into(), "control".into()],
        }),
        generated_links: vec![GeneratedMethodLink {
            org_id: seen.org_id.clone(),
            operation_id: seen.operation_id.clone(),
            slot_id: "candidate".into(),
        }],
        total_grant: ResourceGrant {
            model_calls: 1,
            input_tokens: 100_000,
            output_tokens: 8192,
            wall_time_ms: 10 * 60_000,
            simulation_steps: 2 * 6 * 2400 + 2 * 3 * 2400,
        },
    };
    let rows: Vec<TrialEvidence> = ["candidate", "control"]
        .into_iter()
        .zip(&evidence)
        .map(|(slot, e)| TrialEvidence {
            slot_id: slot.into(),
            execution: e.execution.clone(),
            scoring: e.scoring.clone(),
        })
        .collect();
    let report = assess_costs(
        &scope,
        std::slice::from_ref(&session),
        Some((&trial, &rows)),
    )
    .unwrap();
    assert_eq!(report.matched_package_links, [true]);
    let total = |stage: &[genesis_forge::method_accounting::ResourceTotal], name: &str| {
        let t = stage.iter().find(|t| t.resource == name).unwrap();
        (t.known_lower_bound, t.unknown, t.limit_exceeded)
    };
    assert_eq!(total(&report.generation, "model_calls"), (1, false, false));
    assert_eq!(
        total(&report.generation, "input_tokens"),
        (3000, false, false)
    );

    // Linking the generation to the parent's slot is refused outright: the
    // parent was not produced by this revision.
    let mut wrong = scope.clone();
    wrong.generated_links[0].slot_id = "control".into();
    let refused = assess_costs(
        &wrong,
        std::slice::from_ref(&session),
        Some((&trial, &rows)),
    )
    .unwrap_err();
    assert!(
        refused.to_string().contains("differs from slot"),
        "{refused}"
    );
}
