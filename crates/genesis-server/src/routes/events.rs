//! Event log access: SSE stream with reconnect cursor, and a JSON log.

use crate::app::AppState;
use crate::auth::Principal;
use crate::error::ApiResult;
use axum::extract::{Query, State};
use axum::http::HeaderMap;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::Json;
use futures::Stream;
use genesis_core::events::DomainEvent;
use serde::Deserialize;
use std::convert::Infallible;
use std::time::Duration;
use tokio::sync::broadcast::error::RecvError;

#[derive(Deserialize)]
pub struct Cursor {
    /// Return events with `seq` greater than this.
    pub after: Option<u64>,
    pub limit: Option<u32>,
}

pub async fn log(
    State(app): State<AppState>,
    p: Principal,
    Query(c): Query<Cursor>,
) -> ApiResult<Json<Vec<DomainEvent>>> {
    p.require(&[Principal::Operator, Principal::Agent, Principal::Adapter])?;
    Ok(Json(app.store.events_after(
        c.after.unwrap_or(0),
        c.limit.unwrap_or(500).min(5000),
    )?))
}

fn sse_event(e: &DomainEvent) -> Event {
    Event::default()
        .id(e.seq.to_string())
        .event("domain_event")
        .json_data(e)
        .unwrap_or_else(|_| Event::default().comment("serialization error"))
}

/// `GET /api/v1/events?after=<seq>`. Honors `Last-Event-ID` on reconnect.
/// Replays persisted events after the cursor, then streams live ones.
pub async fn stream(
    State(app): State<AppState>,
    p: Principal,
    headers: HeaderMap,
    Query(c): Query<Cursor>,
) -> ApiResult<Sse<impl Stream<Item = Result<Event, Infallible>>>> {
    p.require(&[Principal::Operator, Principal::Agent, Principal::Adapter])?;
    let from_header = headers
        .get("last-event-id")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<u64>().ok());
    let cursor = from_header.or(c.after).unwrap_or(0);
    // Subscribe before reading the backlog so nothing is missed in between.
    let mut rx = app.events.subscribe();
    let store = app.store.clone();
    let s = async_stream::stream! {
        let mut last = cursor;
        loop {
            match store.events_after(last, 1000) {
                Ok(batch) if !batch.is_empty() => {
                    for e in &batch {
                        last = e.seq;
                        yield Ok(sse_event(e));
                    }
                }
                Ok(_) => break,
                Err(err) => {
                    tracing::warn!(error = %err, "event backlog read failed");
                    break;
                }
            }
        }
        loop {
            match rx.recv().await {
                Ok(e) if e.seq > last => {
                    last = e.seq;
                    yield Ok(sse_event(&e));
                }
                Ok(_) => {}
                Err(RecvError::Lagged(_)) => {
                    // Fell behind the broadcast buffer: catch up from the store.
                    if let Ok(batch) = store.events_after(last, 5000) {
                        for e in &batch {
                            last = e.seq;
                            yield Ok(sse_event(e));
                        }
                    }
                }
                Err(RecvError::Closed) => break,
            }
        }
    };
    Ok(Sse::new(s).keep_alive(KeepAlive::new().interval(Duration::from_secs(15))))
}
