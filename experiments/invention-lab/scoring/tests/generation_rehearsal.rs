//! Offline G1 rehearsal: every seam from method generation to the acceptance
//! verdict, with the real code on each side, and nothing live.
//!
//! The executor builds the genesis-forge `MethodSynthesisPlan`; the forge
//! session records its requesting intent (round-tripped through JSON in memory;
//! the forge's CLI tests cover the durable file) and gets one completion over loopback HTTP
//! from a local fixture server (not a model); the exported package bytes are
//! frozen as the candidate slot; both slots run as real child processes
//! (sandbox mode `none`, so this is not namespace evidence); both outputs are
//! scored by the trusted adapter, the verdict decides and the combined cost
//! report accounts for generation, execution and scoring together, both in
//! process and from the retained files through the forge's own CLI.
//!
//! The "generated" source is the hand-written random-search control. This
//! checks plumbing only: no generated method, no live provider, no benefit.
use genesis_core::development_evaluation::EvaluationOutcome;
use genesis_core::development_method::{ExecutionOutcome, ResourceGrant};
use genesis_core::development_package::{MethodPackage, MethodSourceFile};
use genesis_core::development_trial::TrialEvidence;
use genesis_forge::method_accounting::{
    assess_costs, CostScope, GeneratedMethodLink, GenerationExpectation, TrialExpectation,
};
use genesis_forge::method_synthesis::{
    MethodSynthesisPhase, MethodSynthesisPlan, MethodSynthesisSession,
};
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
const STAND_IN: &str = include_str!("../../executor/methods/random-search/random_search.py");
const FIXED_CART: &str = include_str!("../../executor/methods/fixed-cart/fixed_cart.py");

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
    let d = std::env::temp_dir().join(format!("lab-g1-rehearsal-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    d
}

fn output(dir: &Path, receipt: &genesis_core::development_method::ExecutionReceipt) -> Vec<u8> {
    std::fs::read(dir.join("artifacts").join(&receipt.outputs[0].sha256)).unwrap()
}

#[tokio::test(flavor = "current_thread")]
async fn generation_to_verdict_runs_end_to_end_offline() {
    let interpreter = run::interpreter_version("python3");
    let family = Family::RavineCrossing;

    // 1. Generation, through the forge's own session and transport.
    let plan: MethodSynthesisPlan = serde_json::from_value(example::synthesis_plan(
        family,
        MODEL,
        "rehearsal-1",
        &interpreter,
    ))
    .unwrap();
    let mut session = MethodSynthesisSession::new(plan).unwrap();
    let request = session.begin().unwrap();
    // Serialized requesting-intent roundtrip, in memory, before the one POST.
    // The original session continues; durable intent is the forge CLI's job.
    let pending: MethodSynthesisSession =
        serde_json::from_slice(&serde_json::to_vec(&session).unwrap()).unwrap();
    assert_eq!(pending.phase(), MethodSynthesisPhase::Requesting);
    let reply = serde_json::json!({
        "id": "msg_rehearsal_1", "type": "message", "role": "assistant", "model": MODEL,
        "content": [{"type": "text", "text": serde_json::json!({"source": STAND_IN}).to_string()}],
        "stop_reason": "end_turn", "stop_sequence": null,
        "usage": {"input_tokens": 1000, "output_tokens": 900},
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
    let generated = session.package().unwrap().artifact_json().unwrap();

    // 2. Freeze the exported bytes as the candidate, beside the fixed-cart control.
    let control = MethodPackage::new(
        "fixed-cart".into(),
        "0.1.0".into(),
        refs::runtime(&interpreter),
        refs::protocol(),
        "fixed_cart.py".into(),
        vec![],
        vec![MethodSourceFile {
            path: "fixed_cart.py".into(),
            content: FIXED_CART.into(),
        }],
    )
    .unwrap();
    let control_bytes = control.artifact_json().unwrap();
    let methods = example::ExampleMethods {
        candidate: session.package().unwrap().reference().unwrap(),
        control: control.reference().unwrap(),
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
    // This sandbox can't establish zero model use, so model usage is unknown;
    // the frozen criterion says so explicitly instead of the verdict guessing.
    let ids: Vec<String> = trial
        .development_instances
        .iter()
        .map(|r| r.id.clone())
        .collect();
    let criterion = genesis_core::digest::canonical_json(&serde_json::json!({
        "rule": "success-count-margin",
        "min_margin": 1,
        "instances": ids,
        "unknown_usage_allowed": ["model_calls", "input_tokens", "output_tokens"],
    }))
    .into_bytes();
    trial.criterion = refs::content("lab-criterion/1", "success-count-margin", "1", &criterion);
    trial.validate().unwrap();

    // 3. Both slots as real child processes, then trusted scoring.
    let cfg = run::Config {
        sandbox: Mode::None,
        interpreter_version: interpreter.clone(),
        stage_root: tmp("stage"),
    };
    let mut evidence = vec![];
    for (slot, bytes) in [
        ("candidate", generated.as_bytes()),
        ("control", control_bytes.as_bytes()),
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

    // 4. A real decision, not Insufficient: every input was known or allowed.
    let verdict = decide(&trial, &criterion, &evidence[0], &evidence[1]).unwrap();
    eprintln!("rehearsal verdict (controls only): {verdict:?}");
    assert!(
        matches!(verdict, Verdict::Accepted { .. } | Verdict::Rejected { .. }),
        "{verdict:?}"
    );

    // 5. The combined cost report over the same retained evidence: one
    // generation counted once, linked to the candidate slot by exact package.
    let scope = CostScope {
        id: "rehearsal-cost".into(),
        generations: vec![GenerationExpectation {
            org_id: session.plan().org_id.clone(),
            operation_id: session.plan().operation_id.clone(),
            plan_sha256: session.plan().sha256().unwrap(),
            request_sha256: session.request_sha256().into(),
            checkpoint_sha256: session.checkpoint_sha256().into(),
        }],
        trial: Some(TrialExpectation {
            plan: trial.reference().unwrap(),
            slots: vec!["candidate".into(), "control".into()],
        }),
        generated_links: vec![GeneratedMethodLink {
            org_id: session.plan().org_id.clone(),
            operation_id: session.plan().operation_id.clone(),
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
    // Generation: one measured call and its reported tokens.
    assert_eq!(total(&report.generation, "model_calls"), (1, false, false));
    assert_eq!(
        total(&report.generation, "output_tokens"),
        (900, false, false)
    );
    // Execution can't establish zero model use here, so the combined model
    // count is a known lower bound of 1 with unknown on top, never "1".
    assert_eq!(total(&report.combined, "model_calls"), (1, true, false));
    let (steps, unknown, over) = total(&report.combined, "simulation_steps");
    assert!(steps > 0 && !unknown && !over, "{steps} {unknown} {over}");

    // 6. The same assessment from retained files through the forge's own
    // `method-cost-assess` CLI: the saved checkpoint, plan, evidence and
    // declared scope, read back by a separate process, give the same report.
    let retained = tmp("retained");
    std::fs::create_dir_all(&retained).unwrap();
    let write = |name: &str, value: serde_json::Value| {
        std::fs::write(retained.join(name), serde_json::to_vec(&value).unwrap()).unwrap();
    };
    write("scope.json", serde_json::to_value(&scope).unwrap());
    write("generation.json", serde_json::to_value(&session).unwrap());
    write("plan.json", serde_json::to_value(&trial).unwrap());
    write(
        "evidence.json",
        serde_json::Value::Array(
            rows.iter()
                .map(|e| serde_json::json!({"slot_id": e.slot_id, "execution": e.execution, "scoring": e.scoring}))
                .collect(),
        ),
    );
    write(
        "manifest.json",
        serde_json::json!({
            "schema": "genesis-method-cost-input/1",
            "scope": "scope.json",
            "generation_checkpoints": ["generation.json"],
            "trial": {"plan": "plan.json", "evidence": "evidence.json"},
        }),
    );
    let out = std::process::Command::new(forge_cli())
        .args(["method-cost-assess", "--manifest"])
        .arg(retained.join("manifest.json"))
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "method-cost-assess: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let from_files: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(from_files, serde_json::to_value(&report).unwrap());

    // Control: the CLI really reads the files. Drop the control's scoring
    // receipt and the retained-file report shows scoring as unknown.
    let mut partial: serde_json::Value =
        serde_json::from_slice(&std::fs::read(retained.join("evidence.json")).unwrap()).unwrap();
    partial[1]["scoring"] = serde_json::Value::Null;
    write("evidence.json", partial);
    let out = std::process::Command::new(forge_cli())
        .args(["method-cost-assess", "--manifest"])
        .arg(retained.join("manifest.json"))
        .output()
        .unwrap();
    assert!(out.status.success());
    let partial: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let scoring_steps = partial["scoring"]
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["resource"] == "simulation_steps")
        .unwrap();
    assert_eq!(scoring_steps["unknown"], true, "{scoring_steps}");
}

/// The forge CLI, built from this checkout into this test target's scratch
/// directory (a separate target directory, so no build lock is shared).
fn forge_cli() -> PathBuf {
    let target = Path::new(env!("CARGO_TARGET_TMPDIR")).join("forge-cli");
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../Cargo.toml");
    let status = std::process::Command::new(env!("CARGO"))
        .args([
            "build",
            "-q",
            "-p",
            "genesis-forge",
            "--bin",
            "genesis-forge",
        ])
        .arg("--manifest-path")
        .arg(&manifest)
        .arg("--target-dir")
        .arg(&target)
        .status()
        .unwrap();
    assert!(status.success(), "building the forge CLI failed");
    target.join("debug").join("genesis-forge")
}
