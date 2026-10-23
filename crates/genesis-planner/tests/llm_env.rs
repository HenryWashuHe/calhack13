//! Henry's Claude: provider selection from the environment. One test only,
//! so no other test in this binary races the process environment.
use genesis_planner::llm::{from_env, LlmError};

const VARS: [&str; 4] = [
    "GENESIS_LLM_PROVIDER",
    "GENESIS_LLM_MODEL",
    "GENESIS_LLM_API_KEY",
    "GENESIS_LLM_BASE_URL",
];

fn set(provider: &str, model: Option<&str>, key: Option<&str>, base: Option<&str>) {
    for (var, value) in VARS.iter().zip([Some(provider), model, key, base]) {
        match value {
            Some(v) => std::env::set_var(var, v),
            None => std::env::remove_var(var),
        }
    }
}

/// `Ok(Some(model name))`, `Ok(None)`, or the error.
fn selected() -> Result<Option<String>, LlmError> {
    from_env().map(|c| c.map(|c| c.model()))
}

#[test]
fn provider_selection_from_environment() {
    // An unset or pending provider means "no model yet", even when the other
    // variables are present.
    for provider in ["", "pending", "none", " Pending ", "NONE"] {
        set(provider, Some("m"), Some("k"), None);
        assert!(
            matches!(selected(), Ok(None)),
            "{provider:?}: {:?}",
            selected()
        );
    }
    set("pending", None, None, None);
    assert!(matches!(selected(), Ok(None)));
    for var in VARS {
        std::env::remove_var(var);
    }
    assert!(matches!(selected(), Ok(None)));

    set("anthropic", Some("m"), Some("k"), None);
    assert_eq!(selected().unwrap().as_deref(), Some("anthropic/m"));
    set("openai", Some("m"), Some("k"), None);
    assert_eq!(selected().unwrap().as_deref(), Some("openai/m"));
    set("gemini", Some("m"), Some("k"), None);
    assert_eq!(selected().unwrap().as_deref(), Some("gemini/m"));
    set(
        "openai-compatible",
        Some("m"),
        Some("k"),
        Some("http://127.0.0.1:9"),
    );
    assert_eq!(selected().unwrap().as_deref(), Some("openai-compatible/m"));

    set("openai-compatible", Some("m"), Some("k"), None);
    assert!(matches!(selected(), Err(LlmError::NotConfigured(_))));
    set("anthropic", None, Some("k"), None);
    assert!(matches!(selected(), Err(LlmError::NotConfigured(_))));
    set("anthropic", Some("m"), None, None);
    assert!(matches!(selected(), Err(LlmError::NotConfigured(_))));
    set("mystery", Some("m"), Some("k"), None);
    assert!(matches!(selected(), Err(LlmError::NotConfigured(_))));

    for var in VARS {
        std::env::remove_var(var);
    }
}
