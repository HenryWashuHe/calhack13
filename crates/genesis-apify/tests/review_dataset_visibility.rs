// Independent PR26 review cases; synthetic loopback responses only.
include!("http_contract.rs");

#[tokio::test]
async fn review_visibility_accepts_the_fifth_read_and_preserves_the_frozen_receipt() {
    let mut replies = vec![Reply::json(build()), Reply::json(run("SUCCEEDED"))];
    for _ in 0..4 {
        let mut page = Reply::json(json!([]));
        page.headers = vec![
            ("X-Apify-Pagination-Total", "0".into()),
            ("X-Apify-Pagination-Count", "0".into()),
            ("X-Apify-Pagination-Offset", "0".into()),
        ];
        replies.push(page);
    }
    replies.push(Reply::json(json!([output()])));
    let mock = Mock::serve(replies).await;
    let expected = request();
    let mut config = limits();
    config.deadline = Duration::from_secs(10);
    let receipt = mock.client(config).execute(&expected).await.unwrap();
    assert_eq!(receipt.artifact, expected.artifact);
    assert_eq!(receipt.source, expected.source);
    assert_eq!(receipt.input_sha256, json_sha256(&expected.input));
    assert_eq!(receipt.run_id, "run1");
    assert_eq!(receipt.dataset_id, "dataset1");
    assert_eq!(receipt.output, output());
    let requests = mock.finish().await;
    assert_eq!(requests.len(), 7);
    assert_eq!(
        requests.iter().filter(|r| r.starts_with("POST ")).count(),
        1
    );
    let reads: Vec<_> = requests
        .iter()
        .filter(|r| r.starts_with("GET /v2/datasets/"))
        .collect();
    assert_eq!(reads.len(), 5);
    assert!(reads
        .iter()
        .all(|r| r.lines().next() == reads[0].lines().next()));
}

#[tokio::test]
async fn review_visibility_stops_on_invalid_page_after_a_valid_empty_page() {
    let mut malformed = Reply::json(json!([]));
    malformed.body = "[".into();
    let mut duplicate = Reply::json(json!([]));
    duplicate.body = r#"[{"same":1,"same":2}]"#.into();
    let mut inconsistent = Reply::json(json!([]));
    inconsistent
        .headers
        .push(("X-Apify-Pagination-Offset", "1".into()));
    let mut overflow = Reply::json(json!([output()]));
    overflow
        .headers
        .push(("X-Apify-Pagination-Total", "2".into()));
    let mut unavailable = Reply::json(json!([]));
    unavailable.status = 503;
    for (invalid, expected_error) in [
        (malformed, ErrorKind::MalformedResponse),
        (duplicate, ErrorKind::MalformedResponse),
        (
            Reply::json(json!([output(), output()])),
            ErrorKind::InvalidDataset,
        ),
        (Reply::json(json!([null])), ErrorKind::InvalidDataset),
        (inconsistent, ErrorKind::InvalidDataset),
        (overflow, ErrorKind::InvalidDataset),
        (unavailable, ErrorKind::HttpStatus(503)),
    ] {
        let mock = Mock::serve(vec![
            Reply::json(build()),
            Reply::json(run("SUCCEEDED")),
            Reply::json(json!([])),
            invalid,
        ])
        .await;
        let error = mock.client(limits()).execute(&request()).await.unwrap_err();
        assert_eq!(error.kind, expected_error);
        assert_eq!(error.stage, Stage::Dataset);
        assert_eq!(error.run_id.as_deref(), Some("run1"));
        assert!(error.submission_may_have_occurred);
        let requests = mock.finish().await;
        assert_eq!(requests.len(), 4);
        assert_eq!(
            requests.iter().filter(|r| r.starts_with("POST ")).count(),
            1
        );
        assert_eq!(
            requests
                .iter()
                .filter(|r| r.starts_with("GET /v2/datasets/"))
                .count(),
            2
        );
    }
}

#[tokio::test]
async fn review_visibility_never_starts_for_an_untrusted_successful_run() {
    for field in ["id", "actId", "buildId", "defaultDatasetId"] {
        let mut changed = run("SUCCEEDED");
        changed["data"][field] = json!("other-provider-id");
        let mock = Mock::serve(vec![
            Reply::json(build()),
            Reply::json(run("RUNNING")),
            Reply::json(changed),
        ])
        .await;
        let error = mock.client(limits()).execute(&request()).await.unwrap_err();
        assert_eq!(error.kind, ErrorKind::ProvenanceMismatch, "{field}");
        let requests = mock.finish().await;
        assert_eq!(requests.len(), 3);
        assert!(requests.iter().all(|r| !r.starts_with("GET /v2/datasets/")));
        assert_eq!(
            requests.iter().filter(|r| r.starts_with("POST ")).count(),
            1
        );
    }
}
