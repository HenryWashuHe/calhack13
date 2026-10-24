use super::*;
use genesis_core::development_method::ResourceGrant;
use genesis_forge::method_roster::GenerationRoster;
use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
    time::Duration,
};
mod fixture {
    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/common/method_plan.rs"
    ));
}
mod http {
    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/common/text_generation_fixture.rs"
    ));
}
static NEXT: AtomicU64 = AtomicU64::new(0);
struct Directory(PathBuf);
impl Directory {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "genesis-admission-effect-{}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn journal(session: &MethodSynthesisSession) -> GenerationAdmission {
    GenerationAdmission::new(
        GenerationRoster::freeze(
            "effect".into(),
            session.plan().settings.clone(),
            ResourceGrant {
                model_calls: 1,
                input_tokens: 100,
                output_tokens: 16384,
                wall_time_ms: 10000,
                simulation_steps: 0,
            },
            std::slice::from_ref(session),
        )
        .unwrap(),
    )
    .unwrap()
}

#[tokio::test]
async fn reservation_and_intent_are_saved_and_locked_before_the_only_post() {
    let dir = Directory::new();
    let checkpoint = dir.0.join("generation.json");
    let admission = dir.0.join("admission.json");
    let admission_lock = storage::CheckpointLock::acquire(&admission).unwrap();
    let lock = storage::CheckpointLock::acquire(&checkpoint).unwrap();
    let mut session = MethodSynthesisSession::new(fixture::plan()).unwrap();
    let mut initial_copy = session.clone();
    let mut journal = journal(&session);
    admission_lock.create(&journal).unwrap();
    lock.create(&session).unwrap();
    let observe_dir = dir.0.clone();
    let (endpoint, server) = http::serve_observed(
        serde_json::to_vec(&http::response(
            &serde_json::json!({"source":"raise Exception('MUST_NOT_EXECUTE')"}).to_string(),
        ))
        .unwrap(),
        200,
        Duration::ZERO,
        move || {
            let saved: MethodSynthesisSession =
                storage::read_json(&observe_dir.join("generation.json")).unwrap();
            let journal: GenerationAdmission =
                storage::read_json(&observe_dir.join("admission.json")).unwrap();
            assert_eq!(saved.phase(), MethodSynthesisPhase::Requesting);
            assert!(journal.is_reserved(&saved).unwrap());
            assert_eq!(journal.summary().unwrap().reserved_attempts, 1);
            assert!(observe_dir.join("admission.json.lock").is_file());
            assert!(observe_dir.join("generation.json.lock").is_file());
        },
    )
    .await;
    let client = AnthropicGenerator::with_test_endpoint(&endpoint, "synthetic-key").unwrap();
    execute_once(
        &lock,
        &mut session,
        &client,
        Some((&admission_lock, &mut journal)),
    )
    .await
    .unwrap();
    server.await.unwrap();
    assert_eq!(session.phase(), MethodSynthesisPhase::Generated);
    let bytes = fs::read(&admission).unwrap();
    assert!(execute_once(
        &lock,
        &mut initial_copy,
        &client,
        Some((&admission_lock, &mut journal))
    )
    .await
    .is_err());
    assert_eq!(fs::read(&admission).unwrap(), bytes);
    assert_eq!(initial_copy.phase(), MethodSynthesisPhase::Prepared);
}

#[tokio::test]
async fn either_failed_durable_write_prevents_a_post_and_reservation_never_refunds() {
    for broken in ["admission.json", "generation.json"] {
        let dir = Directory::new();
        let admission_lock =
            storage::CheckpointLock::acquire(&dir.0.join("admission.json")).unwrap();
        let lock = storage::CheckpointLock::acquire(&dir.0.join("generation.json")).unwrap();
        let mut plan = fixture::plan();
        plan.settings.timeout_ms = 50;
        let mut session = MethodSynthesisSession::new(plan).unwrap();
        let prepared = session.clone();
        let mut journal = journal(&session);
        admission_lock.create(&journal).unwrap();
        lock.create(&session).unwrap();
        fs::remove_file(dir.0.join(broken)).unwrap();
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let endpoint = format!("http://{}/v1/messages", listener.local_addr().unwrap());
        let client = AnthropicGenerator::with_test_endpoint(&endpoint, "synthetic-key").unwrap();
        assert!(execute_once(
            &lock,
            &mut session,
            &client,
            Some((&admission_lock, &mut journal))
        )
        .await
        .is_err());
        assert_eq!(
            listener.accept().unwrap_err().kind(),
            std::io::ErrorKind::WouldBlock
        );
        if broken == "generation.json" {
            let mut saved: GenerationAdmission = admission_lock.load().unwrap();
            assert!(saved.is_reserved(&prepared).unwrap());
            assert!(saved.reserve(&prepared).is_err());
        } else {
            let saved: MethodSynthesisSession = lock.load().unwrap();
            assert_eq!(saved.phase(), MethodSynthesisPhase::Prepared);
        }
    }
}
