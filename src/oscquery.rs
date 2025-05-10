pub mod models;

use axum::{
    extract::{Path as AxumPath, Query, State},
    http::StatusCode,
    response::IntoResponse,
    routing::get,
    Json, Router,
};
use models::{HostInfo, OscRootNode};
use serde_json::json;
use tokio::sync::oneshot::Sender as OneshotSender;
use std::{
    collections::HashMap,
    net::{IpAddr, SocketAddr},
    sync::Arc,
};
use tokio::{
    net::{TcpListener, ToSocketAddrs},
    sync::RwLock,
};

/// Custom error types for the OSCQuery server.
#[derive(thiserror::Error, Debug)]
pub enum Error {
    /// Error from binding the TCP listener.
    #[error("Failed to bind to address: {0}")]
    BindError(#[from] std::io::Error),
    /// Error from parsing a socket address string.
    #[error("Failed to parse address: {0}")]
    ParseError(#[from] std::net::AddrParseError),
}

/// Represents an OSCQuery server that exposes an OSC node tree over HTTP.
pub struct OscQuery {
    /// Sender part of a oneshot channel used to signal graceful shutdown of the server.
    shutdown_tx: Option<OneshotSender<()>>,
    /// Shared state of the OSCQuery server, including host info and the OSC node root.
    state: Arc<OscQueryState>,
}

/// Holds the shared state for the OSCQuery server.
pub struct OscQueryState {
    /// Information about the host OSC server (name, IP, port, extensions).
    host_info: RwLock<HostInfo>,
    /// The root of the OSC address space exposed by this server.
    root: OscRootNode,
}

impl OscQuery {
    /// Creates a new `OscQuery` server instance.
    ///
    /// # Arguments
    /// * `host_info` - Initial `HostInfo` for the server.
    /// * `root` - The `OscRootNode` representing the OSC address space.
    pub fn new(host_info: HostInfo, root: OscRootNode) -> Self {
        let state = Arc::new(OscQueryState {
            host_info: RwLock::new(host_info),
            root,
        });

        Self {
            shutdown_tx: None, // Shutdown sender is None until the server is started.
            state,
        }
    }

    /// Starts the OSCQuery HTTP server, listening on the specified address.
    ///
    /// # Arguments
    /// * `addr` - The address to bind the server to (e.g., "0.0.0.0:0" for any interface, OS-assigned port).
    ///
    /// # Returns
    /// The `SocketAddr` the server is actually listening on, or an `Error`.
    pub async fn serve<T>(&mut self, addr: T) -> Result<SocketAddr, Error>
    where
        T: ToSocketAddrs,
    {
        let shared_state = self.state.clone();
        // Bind a TCP listener to the provided address.
        let listener = TcpListener::bind(addr).await?;
        let local_addr = listener.local_addr()?; // Get the actual local address.

        // Define the Axum application router.
        // It handles requests to the root ("/") and any other path ("/{*path}").
        let app = Router::new()
            .route("/", get(handle_root)) // Route for the root path.
            .route("/{*path}", get(handle_path)) // Route for any sub-path. Note: AxumPath uses `*` for catch-all.
            .with_state(shared_state); // Provide the shared state to handlers.

        // Create a oneshot channel for graceful shutdown signaling.
        let (tx, rx) = tokio::sync::oneshot::channel();
        self.shutdown_tx = Some(tx); // Store the sender part.

        // Spawn a new Tokio task to run the Axum server.
        tokio::spawn(async move {
            // Serve the Axum app using the listener.
            // `with_graceful_shutdown` allows the server to finish processing requests
            // after a shutdown signal is received via the `rx` part of the oneshot channel.
            axum::serve(listener, app)
                .with_graceful_shutdown(async {
                    rx.await.ok(); // Wait for the shutdown signal. `.ok()` handles potential error if sender is dropped.
                })
                .await
                .unwrap_or_else(|e| log::error!("OSCQuery server error: {}", e));
        });
        Ok(local_addr) // Return the address the server is listening on.
    }

    /// Signals the OSCQuery server to shut down gracefully.
    pub fn shutdown(&mut self) {
        if let Some(tx) = self.shutdown_tx.take() {
            // The `_ =` ignores the result of `send`, as the receiver might have already been dropped
            // if the server shut down for other reasons.
            let _ = tx.send(());
        }
    }

    /// Updates the OSC IP address in the server's `HostInfo`.
    pub async fn set_ip(&self, ip: IpAddr) {
        let mut host_info_guard = self.state.host_info.write().await;
        host_info_guard.osc_ip = ip;
    }

    /// Updates the OSC port in the server's `HostInfo`.
    pub async fn set_port(&self, port: u16) {
        let mut host_info_guard = self.state.host_info.write().await;
        host_info_guard.osc_port = port;
    }
}

/// Axum handler for requests to the root path ("/").
/// Delegates to `handle_path` with an empty path string.
async fn handle_root(
    params: Query<HashMap<String, String>>, // Query parameters (e.g., ?HOST_INFO).
    state: State<Arc<OscQueryState>>,       // Shared server state.
) -> impl IntoResponse {
    // Effectively treat a request to "/" as a request to "/<empty_path>".
    handle_path(AxumPath(String::new()), params, state).await
}

/// Axum handler for requests to any path ("/{*path}").
/// Returns information about the OSC node at the requested path or specific attributes.
async fn handle_path(
    AxumPath(path_str): AxumPath<String>,              // The requested path string (e.g., "avatar/parameters/ jakiśParametr").
    Query(params): Query<HashMap<String, String>>, // Query parameters from the URL.
    State(state): State<Arc<OscQueryState>>,       // Shared server state.
) -> impl IntoResponse {
    // Construct the full OSC path by prepending "/".
    let full_path = format!("/{}", path_str);
    // Attempt to retrieve the OSC node corresponding to the full path.
    let Some(node) = state.root.get_node(&full_path) else {
        // If the node is not found, return a 404 Not Found response.
        return (StatusCode::NOT_FOUND, "Node not found").into_response();
    };

    if params.is_empty() {
        // If no query parameters are provided, return the entire OSC node as JSON.
        Json(json!(node)).into_response()
    } else if params.contains_key("HOST_INFO") { // Check specifically for HOST_INFO query.
        // If "HOST_INFO" query parameter is present, return the server's HostInfo as JSON.
        let host_info_guard = state.host_info.read().await;
        Json(json!(*host_info_guard)).into_response()
    } else if params.len() == 1 {
        // If there's exactly one query parameter (and it's not HOST_INFO),
        // assume it's requesting a specific attribute of the node.
        let (attr_key, _attr_val) = params.iter().next().unwrap(); // Get the first (and only) key-value pair.
        let attr_key_uppercase = attr_key.to_uppercase(); // OSCQuery attributes are often uppercase.

        // Serialize the node to a generic JSON value to easily access its fields.
        let node_json = json!(node);
        // Try to get the requested attribute from the JSON representation of the node.
        if let Some(value) = node_json.get(&attr_key_uppercase) {
            // If the attribute exists, return it as a JSON object: { "ATTRIBUTE_NAME": value }.
            Json(json!({ attr_key_uppercase: value })).into_response()
        } else {
            // If the attribute is not found on the node, return 404 Not Found.
            (StatusCode::NOT_FOUND, format!("Attribute {} not found on node {}", attr_key_uppercase, full_path)).into_response()
        }
    } else {
        // If multiple query parameters are provided (and it's not a recognized pattern),
        // return a 400 Bad Request response.
        (StatusCode::BAD_REQUEST, "Invalid or too many query parameters. Use a single attribute query or HOST_INFO.").into_response()
    }
}
