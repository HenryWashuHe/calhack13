//! Neither Elastic client follows redirects. A 307/308 would resend the JSON
//! body (mission context, job input, evidence) to the Location's origin;
//! dropping Authorization on a cross-origin hop does not protect the body.

use axum::extract::State;
use axum::http::{header, StatusCode, Uri};
use axum::response::IntoResponse;
use axum::Router;
use genesis_elastic::{AgentBuilder, AgentBuilderConfig, ElasticClient, ElasticConfig};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

async fn serve(router: Router) -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    url
}

/// A sink that counts every request it receives, and an origin that answers
/// every request with `status` pointing at the sink.
async fn redirect_pair(status: StatusCode) -> (String, Arc<AtomicUsize>) {
    let hits = Arc::new(AtomicUsize::new(0));
    let sink = serve(
        Router::new()
            .fallback(|State(h): State<Arc<AtomicUsize>>| async move {
                h.fetch_add(1, Ordering::SeqCst);
                "{}"
            })
            .with_state(hits.clone()),
    )
    .await;
    let origin = serve(Router::new().fallback(move |uri: Uri| {
        let to = format!("{sink}{uri}");
        async move { (status, [(header::LOCATION, to)]).into_response() }
    }))
    .await;
    (origin, hits)
}

#[tokio::test]
async fn agent_builder_converse_does_not_follow_redirects() {
    for status in [
        StatusCode::TEMPORARY_REDIRECT,
        StatusCode::PERMANENT_REDIRECT,
    ] {
        let (origin, hits) = redirect_pair(status).await;
        let ab = AgentBuilder::new(AgentBuilderConfig {
            kibana_url: origin,
            api_key: "a2V5".into(),
            space: None,
            index_prefix: "genesis".into(),
            timeout: Duration::from_secs(5),
        })
        .unwrap();
        let result = ab
            .converse("agent-1", "PRIVATE_MISSION_CONTEXT", None)
            .await;
        assert!(
            result.is_err(),
            "{status}: a redirect must not look like success"
        );
        assert_eq!(
            hits.load(Ordering::SeqCst),
            0,
            "{status}: the body reached the redirect target"
        );
    }
}

#[tokio::test]
async fn elasticsearch_client_does_not_follow_redirects() {
    let (origin, hits) = redirect_pair(StatusCode::TEMPORARY_REDIRECT).await;
    let es = ElasticClient::new(ElasticConfig {
        elasticsearch_url: origin,
        api_key: "a2V5".into(),
        index_prefix: "genesis".into(),
        timeout: Duration::from_secs(5),
    })
    .unwrap();
    assert!(es
        .search_capabilities("PRIVATE_QUERY", None, 5)
        .await
        .is_err());
    assert!(es.ensure_indices("org_a").await.is_err());
    assert_eq!(hits.load(Ordering::SeqCst), 0);
}
