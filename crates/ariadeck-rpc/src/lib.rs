//! Typed aria2 JSON-RPC transport and adapter layer.

mod auth;
mod client;
mod error;
mod models;
mod notification;
mod protocol;
mod transport;

pub use auth::{AuthenticatedTransport, RpcSecret};
pub use client::Aria2Client;
pub use error::RpcError;
pub use models::{TaskKey, VersionInfo};
pub use notification::{Aria2Notification, Aria2NotificationKind};
pub use protocol::RpcCall;
pub use transport::{RpcTransport, WebSocketConfig, WebSocketTransport};
