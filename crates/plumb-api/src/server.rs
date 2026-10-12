//! The blocking server boundary of the pilot API (A0.4, Hotfix 067): `plumb-api` owns the Tokio
//! runtime, the listener and `axum::serve`; the CLI calls `serve_blocking` and depends on none
//! of them.

use std::net::SocketAddr;

use thiserror::Error;

use crate::routes::router;
use crate::state::ApiState;

/// Why the server could not run.
#[derive(Debug, Error)]
pub enum ServeError {
    #[error("the server runtime cannot start: {0}")]
    Runtime(std::io::Error),
    #[error("cannot listen on {addr}: {source}")]
    Bind {
        addr: SocketAddr,
        source: std::io::Error,
    },
    #[error("the server failed: {0}")]
    Serve(std::io::Error),
}

/// Serves the pilot router over `state` on `listen` until Ctrl-C, blocking the caller.
pub fn serve_blocking(state: ApiState, listen: SocketAddr) -> Result<(), ServeError> {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(ServeError::Runtime)?;
    runtime.block_on(async move {
        let listener = tokio::net::TcpListener::bind(listen)
            .await
            .map_err(|source| ServeError::Bind {
                addr: listen,
                source,
            })?;
        axum::serve(listener, router(state))
            .with_graceful_shutdown(async {
                let _ = tokio::signal::ctrl_c().await;
            })
            .await
            .map_err(ServeError::Serve)
    })
}
