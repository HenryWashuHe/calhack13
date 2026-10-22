//! HTTP routes (TECH_SPEC §10). All routes live under `/api/v1`.

mod capabilities;
mod changes;
mod events;
mod gateway;
mod jobs;
mod org;

use crate::app::AppState;
use axum::extract::DefaultBodyLimit;
use axum::routing::{get, post};
use axum::Router;
use tower_http::cors::{Any, CorsLayer};
use tower_http::trace::TraceLayer;

pub fn router(state: AppState) -> Router {
    let api = Router::new()
        .route("/health", get(org::health))
        .route("/organization", get(org::organization))
        .route("/organization/revisions/{revision}", get(org::revision))
        .route("/missions", post(org::create_mission))
        .route("/missions/{id}/observations", post(org::add_observation))
        .route("/missions/{id}/requirements", post(org::add_requirement))
        .route("/contracts/{id}/state", post(org::update_contract))
        .route("/org-changes", post(changes::submit).get(changes::list))
        .route("/org-changes/{id}", get(changes::get_one))
        .route("/jobs", post(jobs::dispatch).get(jobs::list))
        .route("/jobs/{id}", get(jobs::get_one))
        .route(
            "/org-changes/{id}/operations/{role_key}/abandon",
            post(changes::abandon_operation),
        )
        .route("/jobs/{id}/results", post(jobs::submit_result))
        .route("/ledger", get(jobs::ledger))
        .route("/outbox", get(jobs::outbox))
        .route("/events", get(events::stream))
        .route("/events/log", get(events::log))
        .route("/capabilities/search", post(capabilities::search))
        .route("/capabilities/publish", post(capabilities::publish))
        .route(
            "/capabilities/{id}/versions/{version}",
            get(capabilities::manifest),
        )
        .route("/capability-installations", post(capabilities::install))
        .route(
            "/capability-installations/{id}/acceptance",
            post(capabilities::acceptance),
        )
        .route(
            "/capability-installations/{id}/checks",
            post(capabilities::checks),
        )
        .route("/capability-runs/{id}", get(capabilities::get_run))
        .route("/capability-runs/{id}/step", post(capabilities::step_run))
        .route("/capability-runs", post(capabilities::run))
        .route(
            "/gateway/readiness-receipts",
            post(gateway::readiness_receipt).layer(DefaultBodyLimit::max(4096)),
        )
        .route(
            "/gateway/readiness-challenges/{id}",
            get(gateway::readiness_challenge),
        );

    Router::new()
        .nest("/api/v1", api)
        .layer(
            CorsLayer::new()
                .allow_origin(Any)
                .allow_methods(Any)
                .allow_headers(Any),
        )
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}
