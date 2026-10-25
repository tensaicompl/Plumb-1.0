//! The blocking server boundary of the pilot API (A0.4, Hotfix 067): `plumb-api` owns the Tokio
//! runtime, the listener and `axum::serve`; the CLI calls `serve_blocking` and depends on none
//! of them.
//!
//! Loopback-only serving (H0.8, architecture §33.8): the product has no user authentication, so
//! the server refuses any non-loopback bind address, and every request must carry a loopback
//! `Host` and, when present, an `http` loopback `Origin` (enforced by the router layer).

use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr};

use thiserror::Error;

use crate::state::ApiState;

/// Why the server could not run.
#[derive(Debug, Error)]
pub enum ServeError {
    /// The bind address is not a loopback address (the pilot has no user authentication).
    #[error("refusing to listen on non-loopback address {addr}: the pilot server binds only to 127.0.0.1 or [::1]")]
    NonLoopbackBind { addr: SocketAddr },
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

/// Refuses a non-loopback bind address with the typed [`ServeError::NonLoopbackBind`]. Only
/// `127.0.0.1` and `::1` are accepted (not the unspecified `0.0.0.0` / `::`, nor other
/// `127.0.0.0/8` addresses, so the bound host is always one the Host check admits).
pub fn require_loopback_bind(addr: SocketAddr) -> Result<(), ServeError> {
    let loopback = match addr {
        SocketAddr::V4(v4) => *v4.ip() == Ipv4Addr::LOCALHOST,
        SocketAddr::V6(v6) => *v6.ip() == Ipv6Addr::LOCALHOST,
    };
    if loopback {
        Ok(())
    } else {
        Err(ServeError::NonLoopbackBind { addr })
    }
}

/// Whether `authority` (a `Host` header value or an origin's authority) is `127.0.0.1`,
/// `[::1]` or `localhost` (ASCII case-insensitive), optionally followed by `:<port>`.
pub fn is_loopback_authority(authority: &str) -> bool {
    let (host, port) = if let Some(rest) = authority.strip_prefix('[') {
        let Some((inside, after)) = rest.split_once(']') else {
            return false;
        };
        if inside.parse::<Ipv6Addr>() != Ok(Ipv6Addr::LOCALHOST) {
            return false;
        }
        match after {
            "" => return true,
            _ => match after.strip_prefix(':') {
                Some(port) => ("[::1]", Some(port)),
                None => return false,
            },
        }
    } else {
        match authority.split_once(':') {
            Some((host, port)) => (host, Some(port)),
            None => (authority, None),
        }
    };
    let host_ok = host == "[::1]" || host == "127.0.0.1" || host.eq_ignore_ascii_case("localhost");
    let port_ok = port.is_none_or(|p| {
        !p.is_empty()
            && p.len() <= 5
            && p.bytes().all(|b| b.is_ascii_digit())
            && p.parse::<u16>().is_ok()
    });
    host_ok && port_ok
}

/// Whether `origin` (an `Origin` header value) is an `http` origin of a loopback host, any port.
pub fn is_loopback_origin(origin: &str) -> bool {
    let Some((scheme, authority)) = origin.split_once("://") else {
        return false;
    };
    scheme.eq_ignore_ascii_case("http") && is_loopback_authority(authority)
}

/// Resolves on SIGINT (Ctrl-C) or, on Unix, SIGTERM.
async fn shutdown_signal() {
    let interrupt = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    #[cfg(unix)]
    {
        let terminate = async {
            match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
                Ok(mut signal) => {
                    signal.recv().await;
                }
                Err(_) => std::future::pending::<()>().await,
            }
        };
        tokio::select! {
            () = interrupt => {}
            () = terminate => {}
        }
    }
    #[cfg(not(unix))]
    interrupt.await;
}

/// Serves the pilot router over `state` on `listen` until SIGINT or SIGTERM, blocking the
/// caller. A non-loopback `listen` is refused before any runtime or listener is created. On
/// shutdown the stage job queue closes and the in-flight job is drained or persisted
/// interrupted within `STAGE_SHUTDOWN_GRACE_SECONDS` (H0.3, architecture §33.3). With `ui_dir`
/// the built UI is served on the same origin ([`crate::router_with_ui`]).
pub fn serve_blocking(
    state: ApiState,
    listen: SocketAddr,
    ui_dir: Option<&std::path::Path>,
) -> Result<(), ServeError> {
    require_loopback_bind(listen)?;
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(ServeError::Runtime)?;
    let stages = state.clone();
    let closing = state.clone();
    let served = runtime.block_on(async move {
        let listener = tokio::net::TcpListener::bind(listen)
            .await
            .map_err(|source| ServeError::Bind {
                addr: listen,
                source,
            })?;
        axum::serve(listener, crate::routes::router_with_ui(state, ui_dir))
            .with_graceful_shutdown(async move {
                shutdown_signal().await;
                // Close the queue to new claims as soon as the signal arrives.
                closing.begin_stage_shutdown();
            })
            .await
            .map_err(ServeError::Serve)
    });
    if !stages.shutdown_stages() {
        tracing::error!(
            target: "plumb_api::stage",
            "the in-flight stage job was persisted interrupted at shutdown"
        );
    }
    served
}
