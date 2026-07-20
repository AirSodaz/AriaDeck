use serde_json::Value;
use thiserror::Error;

#[derive(Clone, Debug, Error, PartialEq)]
pub enum RpcError {
    #[error("invalid RPC connection configuration: {0}")]
    Configuration(String),
    #[error("RPC transport is closed")]
    Closed,
    #[error("RPC transport failed: {0}")]
    Transport(String),
    #[error("RPC TLS validation or handshake failed: {0}")]
    Tls(String),
    #[error("RPC authentication failed: {0}")]
    Authentication(String),
    #[error("RPC request timed out: {method}")]
    Timeout { method: String },
    #[error("invalid JSON-RPC message: {0}")]
    Protocol(String),
    #[error("failed to serialize JSON-RPC message: {0}")]
    Serialization(String),
    #[error("invalid aria2 payload for {method}, field {field}: {message}")]
    InvalidData {
        method: String,
        field: String,
        message: String,
    },
    #[error("aria2 returned RPC error {code}: {message}")]
    Remote {
        code: i64,
        message: String,
        data: Option<Value>,
    },
}
