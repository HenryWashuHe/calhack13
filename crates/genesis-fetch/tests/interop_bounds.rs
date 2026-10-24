use chrono::{DateTime, Timelike, Utc};
use genesis_fetch::protocol::{Body, Message, Task};
use serde_json::{json, Value};

fn base(output: Value) -> Message {
    Message {
        schema_version: 1,
        message_id: "2e411f86-2aa3-4bcf-a2f1-178b9d4206ad".into(),
        org_id: "org-a".into(),
        mission_id: "mission-a".into(),
        contract_id: "contract-a".into(),
        correlation_id: "correlation-a".into(),
        expires_at: "2030-01-01T00:00:00Z".parse().unwrap(),
        body: Body::TaskResult {
            task_request_id: "84d8fe17-f15b-4f3f-b40c-aa4968f45c7f".into(),
            job_id: "job-a".into(),
            attempt_id: "attempt-a".into(),
            epoch: 7,
            output,
        },
    }
}

fn with_number_token(token: &str) -> Vec<u8> {
    serde_json::to_string(&base(json!({"number": "TOKEN"})))
        .unwrap()
        .replace("\"TOKEN\"", token)
        .into_bytes()
}

#[test]
fn integer_lexemes_outside_wire_range_are_rejected_before_rounding() {
    for token in [
        "18446744073709551616", // u64::MAX + 1
        "18446744073709551617", // otherwise rounds to the same f64
        "-9223372036854775809", // i64::MIN - 1
        "999999999999999999999999999999999999999999999999999999999999",
        "-99999999999999999999999999999999999999999999999999999999999",
    ] {
        assert!(
            Message::decode(&with_number_token(token)).is_err(),
            "{token}"
        );
    }
}

#[test]
fn valid_integer_boundaries_and_finite_float_literals_keep_their_values() {
    for (token, expected) in [
        ("18446744073709551615", json!(u64::MAX)),
        ("-9223372036854775808", json!(i64::MIN)),
        ("9223372036854775808", json!(i64::MAX as u64 + 1)),
        ("0", json!(0)),
        ("1.8446744073709552e19", json!(1.8446744073709552e19)),
        ("-9.223372036854776E+18", json!(-9.223372036854776e18)),
        ("18446744073709551616.0", json!(18446744073709551616.0)),
        ("0.0000000000000000000001", json!(0.0000000000000000000001)),
    ] {
        let message = Message::decode(&with_number_token(token)).unwrap();
        let Body::TaskResult { output, .. } = message.body else {
            unreachable!()
        };
        assert_eq!(output["number"], expected, "{token}");
    }
    for invalid in ["1e999", "NaN", "--2", "1-2", "01", "+1"] {
        assert!(Message::decode(&with_number_token(invalid)).is_err());
    }
}

#[test]
fn huge_numbers_inside_strings_and_escaped_quotes_are_preserved() {
    let original = base(json!({
        "1844674407370955161600000": "-922337203685477580900000",
        "escaped": "A quote: \" and slash: \\ and digits: 18446744073709551616",
        "unicode_escape": "\\u003118446744073709551616",
        "trailing_slash": "18446744073709551616\\",
    }));
    let bytes = serde_json::to_vec(&original).unwrap();
    assert_eq!(Message::decode(&bytes).unwrap(), original);
    // The following real number must still be inspected after a quoted value.
    let mut invalid = serde_json::to_string(&base(
        json!({"quoted":"\\\"18446744073709551616","number":"TOKEN"}),
    ))
    .unwrap();
    invalid = invalid.replace("\"TOKEN\"", "18446744073709551616");
    assert!(Message::decode(invalid.as_bytes()).is_err());
}

fn nested(levels: usize) -> Value {
    let mut value = Value::Null;
    for level in 0..levels {
        value = if level % 2 == 0 {
            json!([value])
        } else {
            json!({"value":value})
        };
    }
    value
}

#[test]
fn message_depth_counts_object_values_and_array_elements_from_root_zero() {
    // Message.body.output is depth two, so 62 edges put its final null at 64.
    let at_limit = base(nested(62));
    assert!(at_limit.validate().is_ok());
    assert_eq!(
        Message::decode(&serde_json::to_vec(&at_limit).unwrap()).unwrap(),
        at_limit
    );
    let too_deep = base(nested(63));
    assert!(too_deep.validate().is_err());
    assert!(Message::decode(&serde_json::to_vec(&too_deep).unwrap()).is_err());
    assert!(too_deep.canonical_sha256().is_err());
}

#[test]
fn task_input_and_offer_terms_use_their_actual_envelope_depth() {
    let mut task_message = base(Value::Null);
    task_message.body = Body::TaskRequest {
        task: Task {
            job_id: "job-a".into(),
            attempt_id: "attempt-a".into(),
            role_id: "role-a".into(),
            assignment_id: "assignment-a".into(),
            idempotency_key: "job-a:attempt-a".into(),
            epoch: 7,
            deadline: task_message.expires_at,
            input: nested(61),
        },
    };
    assert!(task_message.validate().is_ok());
    assert!(Message::decode(&serde_json::to_vec(&task_message).unwrap()).is_ok());
    if let Body::TaskRequest { task } = &mut task_message.body {
        task.input = nested(62);
    }
    assert!(task_message.validate().is_err());
    assert!(Message::decode(&serde_json::to_vec(&task_message).unwrap()).is_err());

    let mut offer = base(Value::Null);
    offer.body = Body::Offer {
        recruit_message_id: "84d8fe17-f15b-4f3f-b40c-aa4968f45c7f".into(),
        offer_id: "offer-a".into(),
        capabilities: vec!["advice".into()],
        terms: nested(62),
        lease_expires_at: offer.expires_at,
    };
    assert!(offer.validate().is_ok());
    if let Body::Offer { terms, .. } = &mut offer.body {
        *terms = nested(63);
    }
    assert!(offer.validate().is_err());
}

#[test]
fn timestamps_serialize_with_python_compatible_years_and_no_leap_seconds() {
    for valid in ["0001-01-01T00:00:00Z", "9999-12-31T23:59:59.999999999Z"] {
        let mut message = base(Value::Null);
        message.expires_at = valid.parse().unwrap();
        assert!(message.validate().is_ok());
        assert!(Message::decode(&serde_json::to_vec(&message).unwrap()).is_ok());
    }
    let leap = "2030-01-01T23:59:59Z"
        .parse::<DateTime<Utc>>()
        .unwrap()
        .with_nanosecond(1_000_000_000)
        .unwrap();
    for invalid in [
        "0000-01-01T00:00:00Z".parse().unwrap(),
        "+10000-01-01T00:00:00Z".parse().unwrap(),
        leap,
    ] {
        let mut message = base(Value::Null);
        message.expires_at = invalid;
        assert!(message.validate().is_err());
        assert!(Message::decode(&serde_json::to_vec(&message).unwrap()).is_err());
    }
    let mut recruit = base(Value::Null);
    recruit.body = Body::RecruitRequest {
        requirement: "Need advice".into(),
        capabilities: vec!["advice".into()],
        lease_expires_at: "+10000-01-01T00:00:00Z".parse().unwrap(),
    };
    assert!(recruit.validate().is_err());
    let invalid_task = Task {
        job_id: "job".into(),
        attempt_id: "attempt".into(),
        role_id: "role".into(),
        assignment_id: "assignment".into(),
        idempotency_key: "idempotency".into(),
        epoch: 1,
        deadline: leap,
        input: Value::Null,
    };
    assert!(invalid_task.validate().is_err());
}
