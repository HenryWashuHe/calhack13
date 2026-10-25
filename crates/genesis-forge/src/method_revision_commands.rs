//! Offline parent-bound preparation and read-only observation. The existing
//! generation commands retain sole responsibility for provider calls/recovery.
use crate::{emit, storage, Arguments};
use genesis_forge::{
    method_revision::{MethodRevisionInput, MethodRevisionSpec, MAX_METHOD_REVISION_INPUT_BYTES},
    method_synthesis::MethodSynthesisSession,
};

pub fn run(args: &Arguments) -> Result<u8, String> {
    let (spec, parent) = if let Some(path) = args.paths.get("--input") {
        let input: MethodRevisionInput = genesis_verification::decode_json(&storage::read_bytes(
            path,
            MAX_METHOD_REVISION_INPUT_BYTES,
        )?)
        .map_err(|_| "invalid revision input JSON/schema".to_string())?;
        input.validate().map_err(|e| e.to_string())?;
        (input.spec, input.parent_package_json.into_bytes())
    } else {
        let spec: MethodRevisionSpec = storage::read_json(args.path("--plan")?)?;
        // Revision v1's canonical parent is one public document, capped at 128 KiB.
        let parent = storage::read_bytes(args.path("--parent")?, 128 * 1024)?;
        (spec, parent)
    };
    if args.command == "method-revise-prepare" {
        let session = spec.prepare(&parent).map_err(|e| e.to_string())?;
        let lock = storage::CheckpointLock::acquire(args.path("--checkpoint")?)?;
        lock.create(&session)?;
        emit(&serde_json::json!({
            "phase": session.phase(),
            "revision_spec_sha256": spec.sha256().map_err(|e|e.to_string())?,
            "parent": spec.parent,
            "org_id": session.plan().org_id,
            "operation_id": session.plan().operation_id,
            "request_sha256": session.request_sha256(),
            "checkpoint_sha256": session.checkpoint_sha256(),
            "authority": "none: source preparation only; no generation, execution or adoption"
        }))?;
    } else {
        // Deliberately no lock, recovery, provider client or writes. A pending
        // attempt remains pending even when its original lock still exists.
        let session: MethodSynthesisSession = storage::read_json(args.path("--checkpoint")?)?;
        let observation = spec.observe(&parent, &session).map_err(|e| e.to_string())?;
        emit(&observation)?;
    }
    Ok(0)
}
