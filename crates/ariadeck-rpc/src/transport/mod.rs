mod websocket;

use async_trait::async_trait;
use serde_json::Value;

use crate::{RpcCall, RpcError};

pub use websocket::{WebSocketConfig, WebSocketTransport};

#[async_trait]
pub trait RpcTransport: Send + Sync {
    async fn call(&self, method: &str, params: Vec<Value>) -> Result<Value, RpcError>;

    async fn batch(&self, calls: Vec<RpcCall>) -> Result<Vec<Result<Value, RpcError>>, RpcError> {
        let mut results = Vec::with_capacity(calls.len());
        for call in calls {
            results.push(self.call(&call.method, call.params).await);
        }
        Ok(results)
    }
}
