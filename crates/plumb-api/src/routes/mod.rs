//! Router composition of the A0.2 runtime handlers (plan A0.2, Hotfix 063): exactly the 30
//! A0.2-owned operations. A0.3-owned operations are not registered.

use axum::Router;

use crate::state::ApiState;

mod gates;
mod meta;
mod model;
mod requirements;
mod sources;

/// The pilot HTTP router over `state`.
pub fn router(state: ApiState) -> Router {
    Router::new()
        .merge(meta::routes())
        .merge(sources::routes())
        .merge(requirements::routes())
        .merge(gates::routes())
        .merge(model::routes())
        .with_state(state)
}
