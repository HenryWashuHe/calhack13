use genesis_core::development_method::ExactContentRef;
use genesis_forge::method_synthesis::{MethodSynthesisPlan, PublicDocument};
use genesis_forge::text_generation::{Sampling, TextModelSettings};
use sha2::{Digest, Sha256};
pub fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
pub fn reference(id: &str, bytes: &[u8]) -> ExactContentRef {
    ExactContentRef {
        domain: "fixture".into(),
        schema: "public-text/1".into(),
        id: id.into(),
        version: "1".into(),
        sha256: hash(bytes),
    }
}
fn document(id: &str, text: &str) -> PublicDocument {
    PublicDocument {
        reference: reference(id, text.as_bytes()),
        text: text.into(),
    }
}
pub fn plan() -> MethodSynthesisPlan {
    MethodSynthesisPlan {
    operation_id:"generate_1".into(),org_id:"org_a".into(),target_id:"search_method".into(),target_version:"1".into(),
    runtime:reference("fixture_runtime",b"runtime"),protocol:document("fixture_protocol","Read one public input; emit one output. This is a test-only protocol, not the lab ABI."),
    entrypoint:"method.py".into(),dependencies:vec![],public_task:document("public_task","Develop a candidate from the public input."),
    public_inputs:vec![document("input","public fixture")],instruction:"Generate a method, not a precomputed answer.".into(),
    settings:TextModelSettings {model:"fixture-model-1".into(),max_tokens:512,timeout_ms:1000,sampling:Sampling::ProviderDefault},
}
}
