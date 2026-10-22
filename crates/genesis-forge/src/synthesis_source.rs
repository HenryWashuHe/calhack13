use crate::synthesis::SynthesisSession;
use crate::{venue_source, CandidateSpec, ForgeStateError, SourcePackage};
use genesis_apify::SourceFile;
use serde_json::json;

/// Package a completed generator output with the fixed production scaffold.
/// This does not compile or execute generated code, verify its semantics, or
/// authorize a provider effect. A caller must create and drive a ForgeSession
/// with an independently trusted oracle before publication or installation.
///
/// All candidate metadata and allowed source URLs come from the saved public
/// generation request. The package contains only compact provenance, never the
/// request's example HTML or the verification oracle. Local receipt hashes detect
/// accidental drift; they are not cryptographic provider attestations.
pub fn generated_candidate(
    generation: &SynthesisSession,
) -> Result<CandidateSpec, ForgeStateError> {
    generation
        .validate()
        .map_err(|_| ForgeStateError::InvalidCandidate)?;
    let request = generation.request();
    let parser = generation
        .extractor()
        .map_err(|_| ForgeStateError::InvalidCandidate)?;
    let receipt = generation
        .receipt()
        .ok_or(ForgeStateError::InvalidCandidate)?;
    let source = venue_source(parser, &request.allowed_source_urls)?;
    let mut files = source.files().to_vec();
    files.push(SourceFile {
        name: "genesis-synthesis.json".into(),
        content: serde_json::to_string_pretty(&json!({
            "format": "genesis-synthesis-v1",
            "operation_id": request.operation_id,
            "candidate": request.candidate,
            "receipt": {
                "provider": "anthropic",
                "message_id": receipt.message_id(),
                "model": receipt.model(),
                "input_tokens": receipt.input_tokens(),
                "output_tokens": receipt.output_tokens(),
                "request_sha256": receipt.request_sha256(),
                "extractor_sha256": receipt.extractor_sha256(),
            },
            "scaffold_source_sha256": source.sha256()?,
            "authority": "none: local provenance is not provider attestation or semantic verification"
        })).map_err(|_| ForgeStateError::InvalidSource)?,
    });
    let candidate = CandidateSpec {
        operation_id: request.operation_id.clone(),
        capability_id: request.candidate.capability_id.clone(),
        capability_version: request.candidate.capability_version.clone(),
        actor_name: request.candidate.actor_name.clone(),
        source_version: request.candidate.source_version.clone(),
        source: SourcePackage::new(files)?,
    };
    candidate.validate()?;
    Ok(candidate)
}
