pub mod models;

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::get;
use axum::{Json, Router};
use models::{HostInfo, OscRootNode};
use serde_json::json;
use tokio::sync::oneshot::Sender;
use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use tokio::net::{TcpListener, ToSocketAddrs};
use tokio::sync::RwLock;

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("Failed to bind to address: {0}")]
    BindError(#[from] std::io::Error),
    #[error("Failed to parse address: {0}")]
    ParseError(#[from] std::net::AddrParseError),
}

/// An OSC query server
pub struct OscQuery {
    shutdown: Option<Sender<()>>,
    state: Arc<OscQueryState>,
}

pub struct OscQueryState {
    host_info: RwLock<HostInfo>,
    root: OscRootNode,
}

impl OscQuery {
    /// Create a new server
    pub fn new(host_info: HostInfo, root: OscRootNode) -> Self {
        let state = Arc::new(OscQueryState {
            host_info: RwLock::new(host_info),
            root,
        });

        Self {
            shutdown: None,
            state,
        }
    }

    /// Start the server
    pub async fn serve<T>(&mut self, addr: T) -> Result<SocketAddr, Error>
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

    /// Change the osc ip address
    pub async fn set_ip(&self, ip: IpAddr) {
        let mut host_info = self.state.host_info.write().await;
        host_info.osc_ip = ip;
    }

    /// Change the osc port
    pub async fn set_port(&self, port: u16) {
        let mut host_info = self.state.host_info.write().await;
        host_info.osc_port = port;
    }
}

async fn handle_root(
    params: Query<HashMap<String, String>>,
    state: State<Arc<OscQueryState>>,
) -> impl IntoResponse {
    handle_path(Path(String::new()), params, state).await
}

async fn handle_path(
    Path(path): Path<String>,
    Query(params): Query<HashMap<String, String>>,
    State(state): State<Arc<OscQueryState>>,
) -> impl IntoResponse {
    let Some(node) = state.root.get_node(&format!("/{}", path)) else {
        return (StatusCode::NOT_FOUND, "Node not found").into_response();
    };

    if params.is_empty() {
        // If no parameters are provided, return the entire node
        Json(json!(node)).into_response()
    } else if params.len() == 1 {
        let (attr, _) = params.iter().next().unwrap();
        let attr = attr.to_uppercase();

        // If the attribute is "HOST_INFO", return the host_info
        if attr == "HOST_INFO" {
            let host_info = state.host_info.read().await;
            return Json(json!(*host_info)).into_response();
        }

        // Return the value of the specified attribute
        let json = json!(node);
        let Some(value) = json.get(&attr) else {
            return (StatusCode::NO_CONTENT, "Attribute not found").into_response();
        };
        format!("{{ {}: {} }}", attr, value).into_response()
    } else {
        (StatusCode::BAD_REQUEST, "Invalid query").into_response()
    }
}
