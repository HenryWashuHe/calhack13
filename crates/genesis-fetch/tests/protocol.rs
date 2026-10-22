use chrono::{Duration, Utc};
use genesis_fetch::protocol::{address, Body, Message, Task, MAX_MESSAGE_BYTES};
use serde_json::{json, Value};

const SDK_ADDRESS: &str = "agent1qgllj55vfr6ueyktacwgsv9fysg0qtexqrv4l738eq006km8e2hxu8pg5ev";

fn base() -> Message {
    let now: chrono::DateTime<Utc> = "2030-01-01T00:00:00Z".parse().unwrap();
    Message {
        schema_version: 1,
        message_id: "00000000-0000-0000-0000-000000000001".into(),
        org_id: "org-a".into(),
        mission_id: "mission-a".into(),
        contract_id: "contract-a".into(),
        correlation_id: "correlation-a".into(),
        expires_at: now + Duration::minutes(10),
        body: Body::TaskRequest {
            task: Task {
                job_id: "job-a".into(),
                attempt_id: "attempt-a".into(),
                role_id: "role-a".into(),
                assignment_id: "assignment-a".into(),
                idempotency_key: "job-a:attempt-a".into(),
                epoch: 7,
                deadline: now + Duration::minutes(5),
                input: json!({"name":"Berkeley"}),
            },
        },
    }
}
fn decode(value: &Value) -> Result<Message, genesis_fetch::protocol::ProtocolError> {
    Message::decode(&serde_json::to_vec(value).unwrap())
}

#[test]
fn native_sdk_address_passes_but_changed_checksum_wrong_hrp_and_case_do_not() {
    assert!(address(SDK_ADDRESS));
    let mut corrupted = SDK_ADDRESS.to_owned();
    corrupted.pop();
    corrupted.push('q');
    for invalid in [
        corrupted,
        SDK_ADDRESS.to_uppercase(),
        SDK_ADDRESS.replacen("agent1", "other1", 1),
        "agent1invalid".into(),
        format!(" {SDK_ADDRESS}"),
        format!("{SDK_ADDRESS}q"),
    ] {
        assert!(!address(&invalid), "{invalid}");
    }
}

#[test]
fn strict_wire_schema_allows_arbitrary_task_data_but_rejects_unknown_protocol_fields() {
    let mut value = serde_json::to_value(base()).unwrap();
    value["body"]["task"]["input"] = json!({"unrecognized_domain_field":{"nested":[1,true,null]}});
    assert!(decode(&value).is_ok());
    for location in ["envelope", "body", "task"] {
        let mut bad = value.clone();
        match location {
            "envelope" => bad["sender"] = json!(SDK_ADDRESS),
            "body" => bad["body"]["authority"] = json!("admin"),
            "task" => bad["body"]["task"]["verified"] = json!(true),
            _ => unreachable!(),
        }
        assert!(decode(&bad).is_err());
    }
    let mut missing = value.clone();
    missing.as_object_mut().unwrap().remove("org_id");
    assert!(decode(&missing).is_err());
    let mut unsupported = value.clone();
    unsupported["schema_version"] = json!(2);
    assert!(decode(&unsupported).is_err());
}

#[test]
fn duplicate_keys_at_every_depth_and_nonfinite_numbers_are_rejected() {
    let clean = serde_json::to_string(&base()).unwrap();
    for bad in [
        clean.replacen(
            "\"org_id\":\"org-a\"",
            "\"org_id\":\"org-a\",\"org_id\":\"org-b\"",
            1,
        ),
        clean.replacen(
            "\"kind\":\"task_request\"",
            "\"kind\":\"task_request\",\"kind\":\"task_request\"",
            1,
        ),
        clean.replacen("\"epoch\":7", "\"epoch\":7,\"epoch\":8", 1),
        clean.replacen(
            "\"name\":\"Berkeley\"",
            "\"name\":\"Berkeley\",\"name\":\"Oakland\"",
            1,
        ),
        clean.replacen("\"name\":\"Berkeley\"", "\"number\":NaN", 1),
        clean.replacen("\"name\":\"Berkeley\"", "\"number\":1e999", 1),
    ] {
        assert_ne!(bad, clean, "test mutation must change its input");
        assert!(Message::decode(bad.as_bytes()).is_err());
    }
}

#[test]
fn canonical_fingerprint_is_stable_across_json_key_order_and_binds_all_context() {
    let original = base();
    let left = serde_json::to_value(&original).unwrap();
    let mut text = String::from("{");
    for (index, (key, value)) in left.as_object().unwrap().iter().rev().enumerate() {
        if index > 0 {
            text.push(',');
        }
        text.push_str(&serde_json::to_string(key).unwrap());
        text.push(':');
        text.push_str(&serde_json::to_string(value).unwrap());
    }
    text.push('}');
    let reordered = Message::decode(text.as_bytes()).unwrap();
    let digest = original.canonical_sha256().unwrap();
    assert_eq!(reordered.canonical_sha256().unwrap(), digest);
    for field in [
        "org",
        "mission",
        "contract",
        "correlation",
        "epoch",
        "input",
        "deadline",
    ] {
        let mut changed = base();
        match field {
            "org" => changed.org_id = "org-b".into(),
            "mission" => changed.mission_id = "mission-b".into(),
            "contract" => changed.contract_id = "contract-b".into(),
            "correlation" => changed.correlation_id = "correlation-b".into(),
            _ => {
                if let Body::TaskRequest { task } = &mut changed.body {
                    match field {
                        "epoch" => task.epoch += 1,
                        "input" => task.input = json!({"name":"Oakland"}),
                        "deadline" => task.deadline += Duration::seconds(1),
                        _ => unreachable!(),
                    }
                }
            }
        }
        assert_ne!(changed.canonical_sha256().unwrap(), digest, "{field}");
    }
}

#[test]
fn task_deadline_must_fit_message_and_domain_ids_have_bounded_unambiguous_syntax() {
    let mut too_late = base();
    if let Body::TaskRequest { task } = &mut too_late.body {
        task.deadline = too_late.expires_at + Duration::seconds(1);
    }
    assert!(too_late.validate().is_err());
    for invalid in [
        "".to_owned(),
        "../role".into(),
        "org a".into(),
        "org\na".into(),
        "org-é".into(),
        "a".repeat(129),
    ] {
        let mut changed = base();
        changed.org_id = invalid;
        assert!(changed.validate().is_err());
    }
    let mut noncanonical = base();
    noncanonical.message_id = "00000000000000000000000000000001".into();
    assert!(noncanonical.validate().is_err());
    let mut signed = base();
    signed.message_id = "00000000-0000-0000-0000-00000000000A".into();
    assert!(signed.validate().is_err());
}

#[test]
fn oversized_nested_inputs_and_wire_buffers_fail_before_becoming_evidence() {
    let mut too_large = base();
    if let Body::TaskRequest { task } = &mut too_large.body {
        task.input = json!("x".repeat(32 * 1024));
    }
    assert!(too_large.validate().is_err());
    let mut maximal = base();
    if let Body::TaskRequest { task } = &mut maximal.body {
        task.input = json!("x".repeat(32 * 1024 - 2));
    }
    assert!(maximal.validate().is_ok());
    let mut padded = serde_json::to_vec(&base()).unwrap();
    padded.resize(MAX_MESSAGE_BYTES + 1, b' ');
    assert!(Message::decode(&padded).is_err());
}

#[test]
fn recruitment_rejects_duplicate_capabilities_controls_and_envelope_outliving_lease() {
    let mut request = base();
    let lease = request.expires_at + Duration::minutes(10);
    request.body = Body::RecruitRequest {
        requirement: "Find public notices".into(),
        capabilities: vec!["public-notices".into()],
        lease_expires_at: lease,
    };
    assert!(request.validate().is_ok());
    for failure in ["duplicate", "empty", "control", "lease", "count"] {
        let mut bad = request.clone();
        if let Body::RecruitRequest {
            requirement,
            capabilities,
            lease_expires_at,
        } = &mut bad.body
        {
            match failure {
                "duplicate" => capabilities.push("public-notices".into()),
                "empty" => capabilities.clear(),
                "control" => *requirement = "find\0notices".into(),
                "lease" => *lease_expires_at = bad.expires_at - Duration::seconds(1),
                "count" => *capabilities = (0..17).map(|i| format!("capability-{i}")).collect(),
                _ => unreachable!(),
            }
        }
        assert!(bad.validate().is_err(), "{failure}");
    }
}
