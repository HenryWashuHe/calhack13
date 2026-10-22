//! Pure interpretation of separately observed acquisition and scheduler state.
//! A completed observation is not an accepted artifact or publication grant.

use serde_json::Value;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ObservationDecision {
    Wait,
    Complete,
    Attention(&'static str),
}

fn positive_version(value: Option<&Value>) -> Result<u64, &'static str> {
    value
        .and_then(Value::as_u64)
        .filter(|version| *version > 0)
        .ok_or("invalid acquisition observation version")
}

/// Interpret one pair of GET responses without importing any evidence. The
/// caller must separately bind the acquisition to the expected organization,
/// mission, requirement, responsibility and role, and validate its final artifact.
pub(super) fn acquisition_observation(
    status: &Value,
    automation: &Value,
    expected_id: &str,
) -> Result<ObservationDecision, &'static str> {
    if expected_id.is_empty()
        || expected_id.len() > 256
        || !expected_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"-_.:".contains(&byte))
        || status.get("id").and_then(Value::as_str) != Some(expected_id)
        || automation.get("id").and_then(Value::as_str) != Some(expected_id)
    {
        return Err("acquisition observation identity mismatch");
    }
    let version = positive_version(status.get("version"))?;
    let current = positive_version(automation.get("current_version"))?;
    let observed = match automation.get("observed_version") {
        Some(Value::Null) => None,
        Some(value) => Some(positive_version(Some(value))?),
        None => return Err("invalid acquisition scheduler version"),
    };
    if observed.is_some_and(|observed| observed > current) {
        return Err("invalid acquisition scheduler version");
    }
    let enabled = automation
        .get("enabled")
        .and_then(Value::as_bool)
        .ok_or("invalid acquisition automation configuration")?;
    let authorized = status
        .get("currently_authorized")
        .and_then(Value::as_bool)
        .ok_or("invalid acquisition observation authority")?;
    let phase = status
        .get("phase")
        .and_then(Value::as_str)
        .filter(|phase| {
            matches!(
                *phase,
                "generating"
                    | "forging"
                    | "verified"
                    | "rejected"
                    | "failed"
                    | "needs_reconciliation"
                    | "fenced"
            )
        })
        .ok_or("invalid acquisition observation phase")?;
    let state = automation
        .get("state")
        .and_then(Value::as_str)
        .filter(|state| {
            matches!(
                *state,
                "disabled" | "ready" | "waiting" | "attention" | "inactive"
            )
        })
        .ok_or("invalid acquisition automation state")?;
    if !authorized || phase == "fenced" {
        return Ok(ObservationDecision::Attention(
            "acquisition authority changed",
        ));
    }
    if !enabled || state == "disabled" {
        return Ok(ObservationDecision::Attention(
            "automatic acquisition is disabled",
        ));
    }
    // The worker can commit between these GETs. Do not combine a scheduler
    // decision for one version with acquisition semantics from another.
    if current != version {
        return Ok(ObservationDecision::Wait);
    }
    match state {
        "ready" | "waiting" => Ok(ObservationDecision::Wait),
        "attention" => {
            if observed != Some(current) {
                return Err("acquisition attention is not bound to the observed version");
            }
            let reason = match automation.get("error_code").and_then(Value::as_str) {
                Some("retry_read") => "acquisition requires an explicit read retry",
                Some("quota") => "acquisition count quota requires attention",
                Some("configuration") => "acquisition configuration requires attention",
                Some("invalid_record") => "acquisition record requires attention",
                Some("conflict") => "acquisition changed concurrently",
                Some("internal") => "acquisition worker requires attention",
                _ => return Err("invalid acquisition attention code"),
            };
            Ok(ObservationDecision::Attention(reason))
        }
        "inactive" if phase == "verified" => Ok(ObservationDecision::Complete),
        "inactive" => Ok(ObservationDecision::Attention(
            "acquisition stopped without verification",
        )),
        // Disabled was handled above; keeping an exhaustive fallback makes
        // malformed responses fail closed if this vocabulary later changes.
        _ => Err("invalid acquisition automation state"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const ID: &str = "acq_observation-test";

    fn status(phase: &str) -> Value {
        json!({"id":ID,"version":8,"currently_authorized":true,"phase":phase,"attempt":0})
    }
    fn automation(state: &str) -> Value {
        json!({"id":ID,"enabled":true,"current_version":8,"observed_version":8,
            "state":state,"error_code":null})
    }

    #[test]
    fn version_skew_waits_before_using_stale_attention_or_completion() {
        for (phase, state) in [("forging", "attention"), ("verified", "inactive")] {
            for current in [7, 9] {
                let mut scheduler = automation(state);
                scheduler["current_version"] = json!(current);
                scheduler["observed_version"] = json!(current);
                scheduler["error_code"] = json!("quota");
                assert_eq!(
                    acquisition_observation(&status(phase), &scheduler, ID),
                    Ok(ObservationDecision::Wait)
                );
            }
        }
    }

    #[test]
    fn only_a_matching_inactive_verified_acquisition_completes() {
        assert_eq!(
            acquisition_observation(&status("verified"), &automation("inactive"), ID),
            Ok(ObservationDecision::Complete)
        );
        for phase in [
            "generating",
            "forging",
            "rejected",
            "failed",
            "needs_reconciliation",
        ] {
            assert!(matches!(
                acquisition_observation(&status(phase), &automation("inactive"), ID),
                Ok(ObservationDecision::Attention(_))
            ));
        }
        // A scheduler does not attest the artifact, and even a candidate's
        // verified label alone cannot complete a still-active observation.
        for state in ["ready", "waiting"] {
            assert_eq!(
                acquisition_observation(&status("verified"), &automation(state), ID),
                Ok(ObservationDecision::Wait)
            );
        }
    }

    #[test]
    fn an_initial_rejection_waits_for_the_server_owned_repair_decision() {
        for state in ["ready", "waiting"] {
            let mut scheduler = automation(state);
            scheduler["observed_version"] = Value::Null;
            assert_eq!(
                acquisition_observation(&status("rejected"), &scheduler, ID),
                Ok(ObservationDecision::Wait)
            );
        }
        assert!(matches!(
            acquisition_observation(&status("rejected"), &automation("inactive"), ID),
            Ok(ObservationDecision::Attention(_))
        ));
    }

    #[test]
    fn attention_uses_only_closed_static_reasons_and_never_claims_a_proof() {
        for code in [
            "retry_read",
            "quota",
            "configuration",
            "invalid_record",
            "conflict",
            "internal",
        ] {
            let mut scheduler = automation("attention");
            scheduler["error_code"] = json!(code);
            scheduler["message"] = json!("PRIVATE_PROVIDER_OR_ORACLE_CANARY");
            let decision = acquisition_observation(&status("verified"), &scheduler, ID).unwrap();
            let ObservationDecision::Attention(reason) = decision else {
                panic!("attention is not completion");
            };
            assert!(!reason.contains("CANARY"));
        }
        for code in [
            Value::Null,
            json!(9),
            json!("PRIVATE_PROVIDER_OR_ORACLE_CANARY"),
        ] {
            let mut scheduler = automation("attention");
            scheduler["error_code"] = code;
            let error = acquisition_observation(&status("forging"), &scheduler, ID).unwrap_err();
            assert!(!error.contains("CANARY"));
        }
        let mut scheduler = automation("attention");
        scheduler["error_code"] = json!("quota");
        for observed in [Value::Null, json!(7)] {
            scheduler["observed_version"] = observed;
            assert!(acquisition_observation(&status("forging"), &scheduler, ID).is_err());
        }
    }

    #[test]
    fn disabled_automation_or_lost_authority_stops_even_during_version_skew() {
        let mut scheduler = automation("ready");
        scheduler["enabled"] = json!(false);
        scheduler["current_version"] = json!(9);
        assert!(matches!(
            acquisition_observation(&status("forging"), &scheduler, ID),
            Ok(ObservationDecision::Attention(_))
        ));
        scheduler["enabled"] = json!(true);
        let mut acquisition = status("verified");
        acquisition["currently_authorized"] = json!(false);
        assert!(matches!(
            acquisition_observation(&acquisition, &scheduler, ID),
            Ok(ObservationDecision::Attention(_))
        ));
        assert!(matches!(
            acquisition_observation(&status("fenced"), &automation("inactive"), ID),
            Ok(ObservationDecision::Attention(_))
        ));
        assert!(matches!(
            acquisition_observation(&status("forging"), &automation("disabled"), ID),
            Ok(ObservationDecision::Attention(_))
        ));
    }

    #[test]
    fn wrong_ids_untyped_versions_and_missing_fields_fail_closed() {
        let mut acquisition = status("verified");
        acquisition["id"] = json!("another-acquisition");
        assert!(acquisition_observation(&acquisition, &automation("inactive"), ID).is_err());
        let mut scheduler = automation("inactive");
        scheduler["id"] = json!("another-acquisition");
        assert!(acquisition_observation(&status("verified"), &scheduler, ID).is_err());
        for version in [Value::Null, json!(0), json!(-1), json!(1.5), json!("8")] {
            let mut acquisition = status("verified");
            acquisition["version"] = version.clone();
            assert!(acquisition_observation(&acquisition, &automation("inactive"), ID).is_err());
            let mut scheduler = automation("inactive");
            scheduler["current_version"] = version;
            assert!(acquisition_observation(&status("verified"), &scheduler, ID).is_err());
        }
        for key in ["enabled", "current_version", "observed_version", "state"] {
            let mut scheduler = automation("inactive");
            scheduler.as_object_mut().unwrap().remove(key);
            assert!(acquisition_observation(&status("verified"), &scheduler, ID).is_err());
        }
        for key in ["currently_authorized", "version", "phase"] {
            let mut acquisition = status("verified");
            acquisition.as_object_mut().unwrap().remove(key);
            assert!(acquisition_observation(&acquisition, &automation("inactive"), ID).is_err());
        }
        let mut scheduler = automation("inactive");
        scheduler["observed_version"] = json!(9);
        assert!(acquisition_observation(&status("verified"), &scheduler, ID).is_err());
    }
}
