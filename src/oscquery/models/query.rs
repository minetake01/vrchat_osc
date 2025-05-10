use serde::{Deserialize, Serialize};

/// Represents a request to an OSCQuery server.
/// This might be used if constructing queries programmatically,
/// though typical OSCQuery interaction is via HTTP GET requests on paths.
/// This struct seems more aligned with a potential future RPC-style interaction
/// or a more structured query mechanism than standard OSCQuery path traversal.
///
/// Note: Standard OSCQuery typically uses the HTTP path to specify the OSC node
/// and query parameters (like "?TYPE" or "?VALUE") to request specific attributes.
/// This struct might be for a custom extension or an alternative query method.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OSCQueryRequest {
    /// The OSC path to query (e.g., "/avatar/parameters/SomeParameter").
    pub path: String,
    /// Optional query string or specific query command.
    /// This could represent attributes like "TYPE", "VALUE", "HOST_INFO", etc.
    pub query: Option<String>,
}

/// Represents a response from an OSCQuery server.
/// Similar to `OSCQueryRequest`, this seems more for a structured RPC-style
/// interaction rather than the typical direct HTTP responses of OSCQuery.
/// Standard OSCQuery responses are typically JSON documents representing
/// the node or its attributes, with HTTP status codes indicating success/failure.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OSCQueryResponse {
    /// HTTP-like status code for the response.
    pub status: u16,
    /// The body of the response, likely a JSON string or other textual representation.
    pub body: String,
}
