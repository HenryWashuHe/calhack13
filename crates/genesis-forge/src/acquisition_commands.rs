use crate::{emit, storage, Arguments};
use genesis_apify::{ApifyClient, RunLimits};
use genesis_forge::{AcquisitionPhase, AcquisitionPlan, AcquisitionSession, AnthropicGenerator};
use genesis_verification::OracleSuite;
use serde_json::json;
use std::io::Write;
use storage::CheckpointLock;

pub async fn run(args: &Arguments) -> Result<u8, String> {
    let suite: OracleSuite = storage::read_json(args.path("--suite")?)?;
    let lock = CheckpointLock::acquire(args.path("--checkpoint")?)?;
    if args.command == "acquire-prepare" {
        let plan: AcquisitionPlan = storage::read_json(args.path("--plan")?)?;
        let session = AcquisitionSession::new(plan, &suite).map_err(|e| e.to_string())?;
        lock.create_compact(&session)?;
        emit_status(&session, false)?;
        return Ok(0);
    }
    let mut session: AcquisitionSession = lock.load()?;
    session.validate(&suite).map_err(|e| e.to_string())?;
    match args.command.as_str() {
        "acquire-status" => {
            emit_status(&session, false)?;
            return Ok(0);
        }
        "acquire-export" => {
            emit(&session.verified(&suite).map_err(|e| e.to_string())?)?;
            return Ok(0);
        }
        "acquire-export-forge" => {
            // The backend publication gate consumes a ForgeSession checkpoint,
            // not the coordinator envelope. Reverify before exporting that proof.
            session.verified(&suite).map_err(|e| e.to_string())?;
            let output = CheckpointLock::acquire(args.path("--output")?)?;
            output.create_compact(session.forge().ok_or("verified acquisition has no forge")?)?;
            emit(
                &json!({"exported": true, "authority": "none: backend must independently reverify the checkpoint"}),
            )?;
            return Ok(0);
        }
        _ => {}
    }
    let json_lines = args.command == "acquire-run";
    // Recovery runs once at entry. A pending mutating action from a prior process
    // is never mistaken for an action being executed in this process.
    if session
        .recover_interrupted(&suite)
        .map_err(|e| e.to_string())?
    {
        lock.replace_compact(&session)?;
        emit_status(&session, json_lines)?;
        return Ok(1);
    }
    let retryable = retryable_read_failure(&session);
    if args.retry_read && !retryable {
        return Err("--retry-read requires a prior retryable read failure".into());
    }
    if retryable && !args.retry_read {
        return Err("previous read failed; inspect status and explicitly use --retry-read".into());
    }
    for _ in 0..args.max_steps.unwrap_or(1) {
        if session.advance_local(&suite).map_err(|e| e.to_string())? {
            lock.replace_compact(&session)?;
        } else {
            match session.phase() {
                AcquisitionPhase::Verified => {
                    emit_status(&session, json_lines)?;
                    return Ok(0);
                }
                AcquisitionPhase::Rejected
                | AcquisitionPhase::Failed
                | AcquisitionPhase::NeedsReconciliation => {
                    emit_status(&session, json_lines)?;
                    return Ok(1);
                }
                AcquisitionPhase::Generating => {
                    let key = std::env::var("ANTHROPIC_API_KEY")
                        .map_err(|_| "ANTHROPIC_API_KEY is required for generation".to_string())?;
                    let generator = AnthropicGenerator::new(&key).map_err(|e| e.to_string())?;
                    let config = session.generation().config().clone();
                    let prompt = session
                        .begin_generation(&suite)
                        .map_err(|e| e.to_string())?;
                    let completion = lock
                        .persist_compact_before_effect(&session, || {
                            generator.generate(&prompt, &config)
                        })
                        .await?;
                    session
                        .complete_generation(completion, &suite)
                        .map_err(|e| e.to_string())?;
                    lock.replace_compact(&session)?;
                }
                AcquisitionPhase::Forging => {
                    let token = std::env::var("APIFY_TOKEN")
                        .map_err(|_| "APIFY_TOKEN is required for forging".to_string())?;
                    let client = ApifyClient::new(&token, RunLimits::default())
                        .map_err(|e| e.to_string())?;
                    let action = session
                        .prepare_forge(&suite)
                        .map_err(|e| e.to_string())?
                        .ok_or("forge has no next action")?;
                    let completion = lock
                        .persist_compact_before_effect(&session, || action.execute(&client))
                        .await?;
                    session
                        .complete_forge(completion, &suite)
                        .map_err(|e| e.to_string())?;
                    lock.replace_compact(&session)?;
                }
            }
        }
        emit_status(&session, json_lines)?;
        // A run never spends another iteration implicitly retrying a failed GET.
        // A fresh invocation can explicitly authorize that saved read again.
        if retryable_read_failure(&session)
            || matches!(
                session.phase(),
                AcquisitionPhase::Failed | AcquisitionPhase::NeedsReconciliation
            )
        {
            return Ok(1);
        }
        if session.phase() == AcquisitionPhase::Verified {
            return Ok(0);
        }
    }
    // A bounded run may stop on the rejected original before entering its
    // planned repair. Surface that rejection; another invocation can advance it.
    // Otherwise zero means progress, not necessarily completion.
    Ok(u8::from(session.phase() == AcquisitionPhase::Rejected))
}

fn retryable_read_failure(session: &AcquisitionSession) -> bool {
    session.phase() == AcquisitionPhase::Forging
        && session.forge().is_some_and(|forge| {
            forge
                .pending_action()
                .is_some_and(|kind| !kind.is_mutating())
                && forge.failure().is_some_and(|failure| failure.retryable)
        })
}

fn emit_status(session: &AcquisitionSession, json_lines: bool) -> Result<(), String> {
    let value = json!({
        "schema_version": 1,
        "operation_id": session.plan().request.operation_id,
        "current_operation_id": session.generation().request().operation_id,
        "phase": session.phase(),
        "attempt": session.attempt(),
        "generation_phase": session.generation().phase(),
        "generation_failure": session.generation().failure(),
        "forge_phase": session.forge().map(|f| f.phase()),
        "pending_action": session.forge().and_then(|f| f.pending_action()),
        "artifact": session.forge().and_then(|f| f.artifact()),
        "completed_cases": session.forge().map(|f| f.completed_cases()),
        "forge_failure": session.forge().and_then(|f| f.failure()),
        "unchanged_repair": session.unchanged_repair(),
        "authority": "none: verified drafts still require organization publication and local acceptance"
    });
    if json_lines {
        std::io::stdout()
            .lock()
            .write_all(&storage::encode_compact_json(&value)?)
            .map_err(|_| "cannot write JSON status".into())
    } else {
        emit(&value)
    }
}
