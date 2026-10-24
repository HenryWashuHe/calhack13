//! One durable attempt. Generated source is exported without executing it.
use crate::{emit, storage, Arguments};
use genesis_forge::{
    method_admission::GenerationAdmission,
    method_stage::StageAllocation,
    method_synthesis::{MethodSynthesisPhase, MethodSynthesisPlan, MethodSynthesisSession},
    AnthropicGenerator,
};
use std::io::Write;

#[cfg(test)]
#[path = "method_admission_step_tests.rs"]
mod admission_tests;

pub async fn run(args: &Arguments) -> Result<u8, String> {
    // Every participating step takes the shared journal lock first. Both locks
    // are fail-fast and remain held through the provider action and final save.
    let mut admission = args
        .paths
        .get("--admission")
        .map(|path| {
            let lock = storage::CheckpointLock::acquire(path)?;
            let journal: GenerationAdmission = lock.load()?;
            Ok::<_, String>((lock, journal))
        })
        .transpose()?;
    if let Some(path) = args.paths.get("--allocation") {
        let allocation: StageAllocation = storage::read_json(path)?;
        let (_, journal) = admission
            .as_ref()
            .ok_or("--allocation requires --admission")?;
        allocation
            .validate_admission(journal)
            .map_err(|e| e.to_string())?;
    }
    let lock = storage::CheckpointLock::acquire(args.path("--checkpoint")?)?;
    if args.command == "method-generate-prepare" {
        let plan: MethodSynthesisPlan = storage::read_json(args.path("--plan")?)?;
        let session = MethodSynthesisSession::new(plan).map_err(|e| e.to_string())?;
        lock.create(&session)?;
        status(&session)?;
        return Ok(0);
    }
    let mut session: MethodSynthesisSession = lock.load()?;
    if let Some((_, journal)) = &admission {
        let reserved = journal.is_reserved(&session).map_err(|e| e.to_string())?;
        if session.phase() == MethodSynthesisPhase::Prepared && reserved {
            return Err(
                "admission already reserved; inspect the interrupted attempt, never replay it"
                    .into(),
            );
        }
        if session.phase() != MethodSynthesisPhase::Prepared && !reserved {
            return Err("advanced generation has no reservation in this admission journal".into());
        }
    }
    if args.command == "method-generate-export" {
        let bytes = session
            .package()
            .and_then(|p| {
                p.artifact_json()
                    .map_err(|_| genesis_forge::SynthesisError::Protocol)
            })
            .map_err(|e| e.to_string())?;
        std::io::stdout()
            .lock()
            .write_all(bytes.as_bytes())
            .map_err(|_| "cannot write method package".to_string())?;
        return Ok(0);
    }
    if args.command == "method-generate-status" {
        status(&session)?;
        return Ok(0);
    }
    if session.recover_interrupted().map_err(|e| e.to_string())? {
        lock.replace(&session)?;
        status(&session)?;
        return Ok(1);
    }
    if session.phase() != MethodSynthesisPhase::Prepared {
        status(&session)?;
        return Ok(if session.phase() == MethodSynthesisPhase::Generated {
            0
        } else {
            1
        });
    }
    let key = credential(&session.plan().settings.model, |name| {
        match std::env::var(name) {
            Ok(value) => Ok(Some(value)),
            Err(std::env::VarError::NotPresent) => Ok(None),
            Err(_) => Err("model configuration must be UTF-8".into()),
        }
    })?;
    let client = AnthropicGenerator::new(&key).map_err(|e| e.to_string())?;
    execute_once(
        &lock,
        &mut session,
        &client,
        admission.as_mut().map(|(lock, journal)| (&*lock, journal)),
    )
    .await?;
    status(&session)?;
    Ok(if session.phase() == MethodSynthesisPhase::Generated {
        0
    } else {
        1
    })
}
async fn execute_once(
    lock: &storage::CheckpointLock,
    session: &mut MethodSynthesisSession,
    client: &AnthropicGenerator,
    admission: Option<(&storage::CheckpointLock, &mut GenerationAdmission)>,
) -> Result<(), String> {
    if let Some((journal_lock, journal)) = admission {
        journal.reserve(session).map_err(|e| e.to_string())?;
        // No refund if this save or the later session-intent save is uncertain.
        // The next process must inspect retained state; no provider call occurs
        // until BOTH durable writes have returned success.
        journal_lock.replace(journal)?;
    }
    let request = session.begin().map_err(|e| e.to_string())?;
    let completion = lock
        .persist_before_effect(&*session, || client.generate_text(&request))
        .await?
        .map_err(|e| e.to_string())?;
    session.complete(completion).map_err(|e| e.to_string())?;
    lock.replace(&*session)?;
    Ok(())
}
fn status(session: &MethodSynthesisSession) -> Result<(), String> {
    let completion = session.completion();
    emit(
        &serde_json::json!({"operation_id":session.plan().operation_id,"org_id":session.plan().org_id,
        "phase":session.phase(),"settings":session.plan().settings,"plan_sha256":session.plan().sha256().map_err(|e|e.to_string())?,
        "request_sha256":completion.map(|c|&c.request_sha256),"wire_sha256":completion.map(|c|&c.wire_sha256),
        "transport":completion.map(|c|c.transport),"actual_model":completion.and_then(|c|c.actual_model.as_ref()),"usage":completion.map(|c|&c.usage),
        "package":session.package().ok().and_then(|p|p.reference().ok()),
        "authority":"none: generated source, not execution, acceptance or improvement; provider-default sampling is not fixed G4 evidence"}),
    )
}

// A partial project configuration never falls back to another provider/model.
// When present, the project key wins; the legacy key is used only if all three
// GENESIS_LLM_* variables are absent. Secrets never enter errors or checkpoints.
fn credential(
    model: &str,
    read: impl Fn(&str) -> Result<Option<String>, String>,
) -> Result<String, String> {
    let provider = read("GENESIS_LLM_PROVIDER")?;
    let selected_model = read("GENESIS_LLM_MODEL")?;
    let key = read("GENESIS_LLM_API_KEY")?;
    if provider.is_some() || selected_model.is_some() || key.is_some() {
        if provider.as_deref() != Some("anthropic") {
            return Err("method generation requires GENESIS_LLM_PROVIDER=anthropic".into());
        }
        if selected_model.as_deref() != Some(model) {
            return Err("GENESIS_LLM_MODEL must exactly match the frozen plan model".into());
        }
        return key
            .filter(|s| !s.is_empty())
            .ok_or_else(|| "GENESIS_LLM_API_KEY is required for the project configuration".into());
    }
    read("ANTHROPIC_API_KEY")?.filter(|s|!s.is_empty()).ok_or_else(||"configure GENESIS_LLM_PROVIDER/MODEL/API_KEY or ANTHROPIC_API_KEY for method-generate-step".into())
}

#[cfg(test)]
mod tests {
    use super::credential;
    #[test]
    fn project_configuration_is_exact_and_never_silently_falls_back() {
        let lookup = |entries: Vec<(&str, &str)>| {
            credential("fixed-model", |name| {
                Ok(entries
                    .iter()
                    .find(|(k, _)| *k == name)
                    .map(|(_, v)| v.to_string()))
            })
        };
        assert_eq!(
            lookup(vec![("ANTHROPIC_API_KEY", "legacy")]).unwrap(),
            "legacy"
        );
        assert_eq!(
            lookup(vec![
                ("GENESIS_LLM_PROVIDER", "anthropic"),
                ("GENESIS_LLM_MODEL", "fixed-model"),
                ("GENESIS_LLM_API_KEY", "project"),
                ("ANTHROPIC_API_KEY", "legacy")
            ])
            .unwrap(),
            "project"
        );
        for entries in [
            vec![("GENESIS_LLM_PROVIDER", "openai")],
            vec![
                ("GENESIS_LLM_PROVIDER", "anthropic"),
                ("GENESIS_LLM_MODEL", "wrong"),
                ("GENESIS_LLM_API_KEY", "project"),
            ],
            vec![
                ("GENESIS_LLM_PROVIDER", "anthropic"),
                ("GENESIS_LLM_MODEL", "fixed-model"),
            ],
            vec![("GENESIS_LLM_API_KEY", "project")],
        ] {
            let mut entries = entries;
            entries.push(("ANTHROPIC_API_KEY", "legacy"));
            assert!(lookup(entries).is_err());
        }
    }
    mod plan_fixture {
        include!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/common/method_plan.rs"
        ));
    }
    mod http_fixture {
        include!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/common/text_generation_fixture.rs"
        ));
    }
    #[tokio::test]
    async fn checkpoint_is_requesting_before_model_post_and_final_output_is_not_executed() {
        use genesis_forge::{
            method_synthesis::{MethodSynthesisPhase, MethodSynthesisSession},
            AnthropicGenerator,
        };
        let directory = std::env::temp_dir().join(format!(
            "genesis-method-persist-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&directory).unwrap();
        let checkpoint = directory.join("generation.json");
        let lock = crate::storage::CheckpointLock::acquire(&checkpoint).unwrap();
        let mut session = MethodSynthesisSession::new(plan_fixture::plan()).unwrap();
        lock.create(&session).unwrap();
        let observer_path = checkpoint.clone();
        let (endpoint, server) = http_fixture::serve_observed(
            serde_json::to_vec(&http_fixture::response(
                &serde_json::json!({"source":"raise Exception('MUST_NOT_EXECUTE')"}).to_string(),
            ))
            .unwrap(),
            200,
            std::time::Duration::ZERO,
            move || {
                let saved: MethodSynthesisSession =
                    serde_json::from_slice(&std::fs::read(observer_path).unwrap()).unwrap();
                assert_eq!(saved.phase(), MethodSynthesisPhase::Requesting);
                assert!(saved.package().is_err());
            },
        )
        .await;
        let client = AnthropicGenerator::with_test_endpoint(&endpoint, "synthetic-key").unwrap();
        super::execute_once(&lock, &mut session, &client, None)
            .await
            .unwrap();
        server.await.unwrap();
        let saved: MethodSynthesisSession = lock.load().unwrap();
        assert_eq!(saved.phase(), MethodSynthesisPhase::Generated);
        assert!(saved.package().unwrap().files()[0]
            .content
            .contains("MUST_NOT_EXECUTE"));
        drop(lock);
        std::fs::remove_dir_all(directory).unwrap();
    }
}
