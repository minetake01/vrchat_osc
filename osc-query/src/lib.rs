//! # `oscquery`
//!
//! `osc-query` is a library for building OSC query servers. It provides a simple API for defining
//! an OSC tree and handling OSC query requests.
pub mod models;
mod error;

#[cfg(test)]
mod tests;

pub use crate::error::OscQueryError;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::get;
use axum::{Json, Router};
use models::{HostInfo, OscRootNode};
use serde_json::json;
use tokio::sync::oneshot::Sender;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::{TcpListener, ToSocketAddrs};
use tokio::sync::RwLock;

/// An OSC query server
pub struct OscQuery {
    shutdown: Option<Sender<()>>,
    state: Arc<OscQueryState>,
}

pub struct OscQueryState {
    host_info: RwLock<HostInfo>,
    root: RwLock<OscRootNode>,
}

impl OscQuery {
    /// Create a new server with the given host information
    pub fn new(host_info: HostInfo, root: OscRootNode) -> Self {
        let state = Arc::new(OscQueryState {
            host_info: RwLock::new(host_info),
            root: RwLock::new(root),
        });

        Self {
            shutdown: None,
            state,
        }
    }

    /// Start the server
    pub async fn serve<T>(&mut self, addr: T) -> Result<SocketAddr, OscQueryError>
    where
        T: ToSocketAddrs,
    {
        let state = self.state.clone();
        let listener = TcpListener::bind(addr).await.unwrap();
        let local_addr = listener.local_addr()?;
        let app = Router::new()
            .route("/", get(handle_root))
            .route("/{*path}", get(handle_path))
            .with_state(state);

        let (tx, rx) = tokio::sync::oneshot::channel();
        self.shutdown = Some(tx);

        tokio::spawn(async move {
            axum::serve(listener, app)
                .with_graceful_shutdown(async { rx.await.ok(); })
                .await
        });
        Ok(local_addr)
    }

    /// Stop the server
    pub fn shutdown(&mut self) {
        if let Some(tx) = self.shutdown.take() {
            let _ = tx.send(());
        }
    }

    /// Apply the given function to a host info
    pub async fn modify_host_info<F>(&self, f: F) -> Result<(), OscQueryError>
    where
        F: FnOnce(&mut HostInfo) -> Result<(), OscQueryError>,
    {
        let host_info = &mut self.state.host_info.write().await;
        f(host_info)
    }

    /// Apply the given function to a node
    pub async fn modify_node<F>(&self, f: F) -> Result<(), OscQueryError>
    where
        F: FnOnce(&mut OscRootNode) -> Result<(), OscQueryError>,
    {
        let root = &mut self.state.root.write().await;
        f(root)
    }
}

async fn handle_root(
    params: Query<HashMap<String, String>>,
    state: State<Arc<OscQueryState>>,
) -> impl IntoResponse {
    handle_path(Path("/".to_string()), params, state).await
}

async fn handle_path(
    Path(path): Path<String>,
    Query(params): Query<HashMap<String, String>>,
    State(state): State<Arc<OscQueryState>>,
) -> impl IntoResponse {
    let root = state.root.read().await;
    let Some(node) = root.get_node(&format!("/{}", path)) else {
        return (StatusCode::NOT_FOUND, "Node not found").into_response();
    };

    if params.is_empty() {
        Json(json!(node)).into_response()
    } else if params.len() == 1 {
        let (attr, _) = params.iter().next().unwrap();
        let attr = attr.to_uppercase();
        if attr == "HOST_INFO" {
            let host_info = state.host_info.read().await;
            return Json(json!(*host_info)).into_response();
        }
        let json = json!(node);
        let Some(value) = json.get(&attr) else {
            return (StatusCode::NO_CONTENT, "Attribute not found").into_response();
        };
        format!("{{ {}: {} }}", attr, value).into_response()
    } else {
        (StatusCode::BAD_REQUEST, "Invalid query").into_response()
    }
}
