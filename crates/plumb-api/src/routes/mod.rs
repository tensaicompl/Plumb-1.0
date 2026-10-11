//! Router composition of the pilot runtime handlers: the 30 A0.2-owned operations (plan A0.2,
//! Hotfix 063) and the 40 A0.3-owned operations (plan A0.3, Hotfix 065). The two A0.4-owned
//! stage and job operations are not registered.

use axum::Router;

use crate::state::ApiState;

mod gates;
mod meta;
mod model;
mod proposals;
mod questions;
mod requirements;
mod scenarios;
mod sentences;
mod sessions;
mod sources;

/// The pilot HTTP router over `state`.
pub fn router(state: ApiState) -> Router {
    Router::new()
        .merge(meta::routes())
        .merge(sources::routes())
        .merge(requirements::routes())
        .merge(gates::routes())
        .merge(model::routes())
        .merge(sentences::routes())
        .merge(questions::routes())
        .merge(proposals::routes())
        .merge(sessions::routes())
        .merge(scenarios::routes())
        .with_state(state)
}
