//! CLI orchestration: commit generation intent before the model POST, then
//! convert its untrusted output into the existing independently verified forge.
use crate::{emit, storage, Arguments, CandidateMetadata};
use genesis_forge::synthesis::{
    GenerationReceipt, ModelConfig, SynthesisPhase, SynthesisRequest, SynthesisSession,
};
use genesis_forge::synthesis_http::AnthropicGenerator;
use genesis_forge::{generated_candidate, SourcePackage};
use genesis_verification::OracleSuite;
use serde_json::{json, Value};
use storage::CheckpointLock;

pub async fn run(args: &Arguments) -> Result<u8, String> {
    let lock = CheckpointLock::acquire(args.path("--checkpoint")?)?;
    if args.command == "generate-prepare" {
        let request: SynthesisRequest = storage::read_json(args.path("--request")?)?;
        let config: ModelConfig = storage::read_json(args.path("--model-config")?)?;
        let session = SynthesisSession::new(request, config).map_err(|e| e.to_string())?;
        lock.create(&session)?;
        emit_status(&session)?;
        return Ok(0);
    }

    let mut session: SynthesisSession = lock.load()?;
    session.validate().map_err(|e| e.to_string())?;
    if args.command == "generate-status" {
        emit_status(&session)?;
        return Ok(0);
    }
    if session.recover_interrupted().map_err(|e| e.to_string())? {
        lock.replace(&session)?;
        emit_status(&session)?;
        eprintln!(
            "genesis-forge: interrupted model request requires reconciliation; no request was sent"
        );
        return Ok(1);
    }
    if session.phase() != SynthesisPhase::Prepared {
        emit_status(&session)?;
        return Ok(if session.phase() == SynthesisPhase::Generated {
            0
        } else {
            1
        });
    }
    // Configuration errors must not consume a generation attempt or leave an
    // ambiguous POST intent. Production has no user-supplied endpoint override.
    let key = std::env::var("ANTHROPIC_API_KEY")
        .map_err(|_| "ANTHROPIC_API_KEY is required for generate-step".to_string())?;
    let generator = AnthropicGenerator::new(&key).map_err(|e| e.to_string())?;
    let config = session.config().clone();
    let prompt = session.begin().map_err(|e| e.to_string())?;
    let completion = lock
        .persist_before_effect(&session, || generator.generate(&prompt, &config))
        .await?;
    session.complete(completion).map_err(|e| e.to_string())?;
    lock.replace(&session)?;
    emit_status(&session)?;
    Ok(if session.phase() == SynthesisPhase::Generated {
        0
    } else {
        1
    })
}

fn receipt_metadata(receipt: &GenerationReceipt) -> Value {
    json!({
        "provider": "anthropic",
        "message_id": receipt.message_id(),
        "model": receipt.model(),
        "input_tokens": receipt.input_tokens(),
        "output_tokens": receipt.output_tokens(),
        "request_sha256": receipt.request_sha256(),
        "extractor_sha256": receipt.extractor_sha256(),
    })
}

fn emit_status(session: &SynthesisSession) -> Result<(), String> {
    emit(&json!({
        "schema_version": 1,
        "operation_id": session.request().operation_id,
        "phase": session.phase(),
        "model": session.config().model,
        "receipt": session.receipt().map(receipt_metadata),
        "failure": session.failure(),
        "authority": "none: generated source is untrusted until independent forge verification"
    }))
}

/// Consume only a completed, internally consistent generation checkpoint. The
/// public request fixes all identity fields and the source allowlist before the
/// model call. Oracle contents are first read by the offline forge preparation.
pub fn generated_source(
    args: &Arguments,
    metadata: &CandidateMetadata,
    suite: &OracleSuite,
) -> Result<SourcePackage, String> {
    let generation_lock = CheckpointLock::acquire(args.path("--generation")?)?;
    let generation: SynthesisSession = generation_lock.load()?;
    generation.validate().map_err(|e| e.to_string())?;
    let request = generation.request();
    if metadata.operation_id != request.operation_id
        || metadata.capability_id != request.candidate.capability_id
        || metadata.capability_version != request.candidate.capability_version
        || metadata.actor_name != request.candidate.actor_name
        || metadata.source_version != request.candidate.source_version
    {
        return Err("candidate metadata differs from the saved generation request".into());
    }
    if suite
        .cases
        .iter()
        .any(|case| !request.allowed_source_urls.contains(&case.source.url))
    {
        return Err("verification suite URL is outside the generation source allowlist".into());
    }
    Ok(generated_candidate(&generation)
        .map_err(|e| e.to_string())?
        .source)
}
