use serde::{Deserialize, Serialize};


#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OSCQueryRequest {
    pub path: String,
    pub query: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OSCQueryResponse {
    pub status: u16,
    pub body: String,
}
