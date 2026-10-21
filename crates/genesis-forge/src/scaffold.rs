use crate::{ForgeStateError, SourcePackage};
use genesis_apify::SourceFile;
use std::collections::BTreeSet;

/// Assemble the fixed Actor scaffold with a supplied narrow extraction module.
/// Only the parser and allowed public source URLs vary. No oracle answers are
/// included. The supplied parser may be generated upstream or hand authored;
/// packaging itself is not LLM synthesis or arbitrary-code containment.
pub fn venue_source(
    extractor_typescript: &str,
    source_urls: &[String],
) -> Result<SourcePackage, ForgeStateError> {
    if extractor_typescript.trim().is_empty()
        || extractor_typescript.len() > 128 * 1024
        || source_urls.is_empty()
        || source_urls.len() > 100
    {
        return Err(ForgeStateError::InvalidSource);
    }
    let mut urls = BTreeSet::new();
    for source in source_urls {
        let url = url::Url::parse(source).map_err(|_| ForgeStateError::InvalidSource)?;
        if source.len() > 1024
            || url.scheme() != "https"
            || url.host_str().is_none()
            || !url.username().is_empty()
            || url.password().is_some()
            || url.fragment().is_some()
            || url.as_str() != source
        {
            return Err(ForgeStateError::InvalidSource);
        }
        urls.insert(source);
    }
    macro_rules! template {
        ($path:literal) => {
            SourceFile {
                name: $path.into(),
                content: include_str!(concat!("../../../actors/venue-extractor-template/", $path))
                    .into(),
            }
        };
    }
    let mut files = vec![
        template!(".actor/actor.json"),
        template!(".actor/input_schema.json"),
        template!("Dockerfile"),
        template!("package.json"),
        template!("package-lock.json"),
        template!("tsconfig.json"),
        template!("src/main.ts"),
        template!("src/contract.ts"),
        template!("src/runtime.ts"),
    ];
    files.push(SourceFile {
        name: "src/extractor.ts".into(),
        content: extractor_typescript.into(),
    });
    files.push(SourceFile {
        name: "allowed-sources.json".into(),
        content: serde_json::json!({"urls":urls}).to_string(),
    });
    SourcePackage::new(files)
}
