//! HTTP routes (TECH_SPEC §10). All routes live under `/api/v1`.

mod acquisition_history;
mod acquisitions;
mod adoptions;
pub(crate) mod capabilities;
mod capability_gaps;
mod changes;
mod events;
mod gateway;
pub(crate) mod jobs;
mod org;
mod search;
mod venue_assessments;

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
        .route("/planner/decisions", post(org::planner_decision))
        .route("/org-changes", post(changes::submit).get(changes::list))
        .route("/org-changes/{id}", get(changes::get_one))
        .route(
            "/org-changes/requests/{request_id}",
            get(changes::by_request),
        )
        .route("/jobs", post(jobs::dispatch).get(jobs::list))
        .route("/venue-assessments", post(venue_assessments::create))
        .route("/venue-assessments/{id}", get(venue_assessments::get))
        .route(
            "/venue-assessments/{id}/verify",
            post(venue_assessments::verify),
        )
        .route("/jobs/{id}", get(jobs::get_one))
        .route(
            "/org-changes/{id}/operations/{role_key}/abandon",
            post(changes::abandon_operation),
        )
        .route(
            "/org-changes/{id}/operations/{role_key}/attach",
            post(changes::attach_operation),
        )
        .route("/jobs/{id}/results", post(jobs::submit_result))
        .route("/ledger", get(jobs::ledger))
        .route("/outbox", get(jobs::outbox))
        .route("/events", get(events::stream))
        .route("/events/log", get(events::log))
        .route("/projection", get(search::status))
        .route("/search/evidence", get(search::evidence))
        .route("/capabilities/search", post(capabilities::search))
        .route("/capabilities/publish", post(capabilities::publish))
        .route(
            "/capabilities/{id}/versions/{version}",
            get(capabilities::manifest),
        )
        .route("/capability-installations", post(capabilities::install))
        .route(
            "/capability-gaps/{id}/acquisition",
            get(capability_gaps::acquisition),
        )
        .route(
            "/capability-adoptions",
            post(adoptions::create).get(adoptions::list),
        )
        .route("/capability-adoptions/{id}", get(adoptions::status))
        .route("/capability-adoptions/{id}/step", post(adoptions::step))
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
        .route(
            "/capability-runs/{id}/verify",
            post(capabilities::verify_run),
        )
        .route(
            "/capability-runs/{id}/verification",
            get(capabilities::run_verification),
        )
        .route(
            "/capability-runs/{id}/finalization",
            get(capabilities::run_finalization),
        )
        .route(
            "/capability-acquisitions",
            post(acquisitions::create).get(acquisition_history::list),
        )
        .route(
            "/capability-acquisitions/{id}/history",
            get(acquisition_history::history),
        )
        .route("/capability-acquisitions/{id}", get(acquisitions::status))
        .route(
            "/capability-acquisitions/{id}/automation",
            get(acquisitions::automation),
        )
        .route(
            "/capability-acquisitions/{id}/step",
            post(acquisitions::step),
        )
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
