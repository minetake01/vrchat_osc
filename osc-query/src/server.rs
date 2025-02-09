//! An OSC query server, implemented using the axum web framework.
use crate::node::{HostInfo, OscNode};
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse};
use axum::routing::get;
use axum::{Json, Router};
use serde_json::json;
use std::collections::HashMap;
use std::future::Future;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::sync::{RwLock};
use tokio::sync::oneshot::{Receiver, Sender};
use tokio::task::JoinHandle;

/// An OSC query server
pub struct OscQueryServer {
    shutdown: Option<Sender<()>>,
    state: Arc<OscQueryServerState>,
    socket_addr: Option<SocketAddr>,
}

struct OscQueryServerState {
    host_info: HostInfo,
    root: Arc<RwLock<OscNode>>,
}

impl OscQueryServer {
    /// Create a new server with the given host information
    pub fn new(host_info: HostInfo) -> Self {
        Self {
            shutdown: None,
            state: Arc::new(OscQueryServerState {
                host_info,
                root: Arc::new(RwLock::new(OscNode {
                    full_path: "/".to_string().into(),
                    contents: Some(HashMap::new()),
                    r#type: None,
                    description: None,
                    access: None,
                    value: None,
                    range: None,
                    tags: None,
                    extended_type: None,
                    unit: None,
                    critical: None,
                    clipmode: None,
                    overloads: None,
                })),
            }),
            socket_addr: None,
        }
    }

    /// Shut down the server
    pub fn shutdown(&mut self) {
        if let Some(tx) = self.shutdown.take() {
            let _ = tx.send(());
        }
    }

    /// Set the address to bind to
    pub fn with_address(mut self, addr: SocketAddr) -> Self {
        self.socket_addr = Some(addr);
        self
    }

    /// Apply the given function to a node at the provided path
    pub async fn map_node<F>(&self, path: &str, map: F) -> &Self
    where
        F: FnOnce(&mut OscNode),
    {
        let mut root = self.state.root.write().await;
        let node = find_node_mut(&mut root, path).await;
        if let Some(node) = node {
            map(node);
        }
        self
    }

    /// Add a node to the server
    pub async fn add_node(&self, path: &str, node: OscNode) -> Result<(), String> {
        let mut root = self.state.root.write().await;
        self.add_node_recursive(&mut root, path, node).await
    }

    fn add_node_recursive<'a>(
        &'a self,
        parent: &'a mut OscNode,
        path: &'a str,
        node: OscNode,
    ) -> Pin<Box<dyn Future<Output = Result<(), String>> + 'a>> {
        Box::pin(async move {
            let parts: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
            if parts.is_empty() {
                return Err("Invalid path".to_string());
            }
            if parts.len() == 1 {
                if let Some(contents) = parent.contents.as_mut() {
                    contents.insert(parts[0].to_string(), node);
                    Ok(())
                } else {
                    Err("Parent node is not a container".to_string())
                }
            } else {
                if let Some(contents) = parent.contents.as_mut() {
                    if let Some(child) = contents.get_mut(parts[0]) {
                        self.add_node_recursive(child, &parts[1..].join("/"), node)
                            .await
                    } else {
                        Err("Path not found".to_string())
                    }
                } else {
                    Err("Parent node is not a container".to_string())
                }
            }
        })
    }

    /// Start the server
    pub async fn serve(&mut self) -> JoinHandle<std::io::Result<()>> {
        let addr = self.socket_addr.unwrap_or(([127, 0, 0, 1], 8000).into());
        let state = self.state.clone();
        let app = Router::new()
            .route("/", get(handle_root))
            .route("/*path", get(handle_path))
            .with_state(state);

        let (tx, rx) = tokio::sync::oneshot::channel();
        self.shutdown = Some(tx);

        tokio::spawn(async move {
            let listener = TcpListener::bind(addr).await.unwrap();
            axum::serve(listener, app)
                .with_graceful_shutdown(shutdown_signal(rx))
                .await
        })
    }
}

async fn shutdown_signal(rx: Receiver<()>) {
    tokio::select! {
        _ = rx => (),
    }
}

async fn handle_root(
    Query(params): Query<HashMap<String, String>>,
    State(state): State<Arc<OscQueryServerState>>,
) -> impl IntoResponse {
    handle_node(&state, "/", params).await
}

async fn handle_path(
    Path(path): Path<String>,
    Query(params): Query<HashMap<String, String>>,
    State(state): State<Arc<OscQueryServerState>>,
) -> impl IntoResponse {
    handle_node(&state, &path, params).await
}

async fn handle_node(
    state: &Arc<OscQueryServerState>,
    path: &str,
    params: HashMap<String, String>,
) -> impl IntoResponse {
    let root = state.root.read().await;
    let node = if path == "/" {
        Some(&*root)
    } else {
        find_node(&root, path).await
    };

    match node {
        Some(node) => {
            if params.is_empty() {
                // Return full node information
                Json(json!(node)).into_response()
            } else if params.len() == 1 {
                // Query for a specific attribute
                let (attr, _) = params.iter().next().unwrap();
                match attr.to_uppercase().as_str() {
                    "FULL_PATH" => Json(json!({ "FULL_PATH": node.full_path })).into_response(),
                    "CONTENTS" => Json(json!({ "CONTENTS": node.contents })).into_response(),
                    "TYPE" => Json(json!({ "TYPE": node.r#type })).into_response(),
                    "DESCRIPTION" => {
                        Json(json!({ "DESCRIPTION": node.description })).into_response()
                    }
                    "ACCESS" => Json(json!({ "ACCESS": node.access })).into_response(),
                    "VALUE" => Json(json!({ "VALUE": node.value })).into_response(),
                    "RANGE" => Json(json!({ "RANGE": node.range })).into_response(),
                    "TAGS" => Json(json!({ "TAGS": node.tags })).into_response(),
                    "EXTENDED_TYPE" => {
                        Json(json!({ "EXTENDED_TYPE": node.extended_type })).into_response()
                    }
                    "UNIT" => Json(json!({ "UNIT": node.unit })).into_response(),
                    "CRITICAL" => Json(json!({ "CRITICAL": node.critical })).into_response(),
                    "CLIPMODE" => Json(json!({ "CLIPMODE": node.clipmode })).into_response(),
                    "OVERLOADS" => Json(json!({ "OVERLOADS": node.overloads })).into_response(),
                    "HOST_INFO" => handle_host_info(&state).await.into_response(),
                    _ => (StatusCode::BAD_REQUEST, "Unknown attribute").into_response(),
                }
            } else {
                (StatusCode::BAD_REQUEST, "Invalid query").into_response()
            }
        }
        None => (StatusCode::NOT_FOUND, "Node not found").into_response(),
    }
}

async fn find_node<'a>(root: &'a OscNode, path: &str) -> Option<&'a OscNode> {
    let parts: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    let mut current = root;

    for part in parts {
        if let Some(contents) = current.contents() {
            if let Some(child) = contents.get(part) {
                current = child;
            } else {
                return None;
            }
        } else {
            return None;
        }
    }

    Some(current)
}

async fn find_node_mut<'a>(root: &'a mut OscNode, path: &str) -> Option<&'a mut OscNode> {
    let parts: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    let mut current = root;

    for part in parts {
        if let Some(contents) = current.contents_mut() {
            if let Some(child) = contents.get_mut(part) {
                current = child;
            } else {
                return None;
            }
        } else {
            return None;
        }
    }

    Some(current)
}

async fn handle_host_info(state: &OscQueryServerState) -> Json<serde_json::Value> {
    Json(json!(state.host_info))
}
