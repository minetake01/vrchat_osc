use std::error::Error;
use std::fmt;

#[derive(Debug, Clone, PartialEq)]
pub enum OscQueryError {
    NodeNotFound,
    NodeAlreadyExists,
    AttributeNotFound,
    AttributeTypeMismatch,
    InvalidPath,
    InvalidValue,
    InvalidType,
    InvalidRange,
    InvalidAccess,
    SerializationError,
    DeserializationError,
    HTTPError(u16),
    WebSocketError(String),
    TransportError(String),
    LockError,
    IOError(String),
    ParseError(String),
    InvalidWebSocketCommand,
    UnsupportedFeature(String),
    UnexpectedServerResponse(String),
}

impl fmt::Display for OscQueryError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            OscQueryError::NodeNotFound => write!(f, "Node not found"),
            OscQueryError::NodeAlreadyExists => write!(f, "Node already exists"),
            OscQueryError::AttributeNotFound => write!(f, "Attribute not found"),
            OscQueryError::AttributeTypeMismatch => write!(f, "Attribute type mismatch"),
            OscQueryError::InvalidPath => write!(f, "Invalid path"),
            OscQueryError::InvalidValue => write!(f, "Invalid value"),
            OscQueryError::InvalidType => write!(f, "Invalid type"),
            OscQueryError::InvalidRange => write!(f, "Invalid range"),
            OscQueryError::InvalidAccess => write!(f, "Invalid access"),
            OscQueryError::SerializationError => write!(f, "Serialization error"),
            OscQueryError::DeserializationError => write!(f, "Deserialization error"),
            OscQueryError::HTTPError(status) => write!(f, "HTTP error: {}", status),
            OscQueryError::WebSocketError(msg) => write!(f, "WebSocket error: {}", msg),
            OscQueryError::TransportError(msg) => write!(f, "Transport error: {}", msg),
            OscQueryError::LockError => write!(f, "Lock error"),
            OscQueryError::IOError(msg) => write!(f, "I/O error: {}", msg),
            OscQueryError::ParseError(msg) => write!(f, "Parse error: {}", msg),
            OscQueryError::InvalidWebSocketCommand => write!(f, "Invalid WebSocket command"),
            OscQueryError::UnsupportedFeature(feature) => {
                write!(f, "Unsupported feature: {}", feature)
            }
            OscQueryError::UnexpectedServerResponse(msg) => {
                write!(f, "Unexpected server response: {}", msg)
            }
        }
    }
}

impl Error for OscQueryError {}

impl From<std::io::Error> for OscQueryError {
    fn from(error: std::io::Error) -> Self {
        OscQueryError::IOError(error.to_string())
    }
}

impl From<serde_json::Error> for OscQueryError {
    fn from(error: serde_json::Error) -> Self {
        OscQueryError::ParseError(error.to_string())
    }
}

// You might want to add more From implementations for other error types you're using in your library
