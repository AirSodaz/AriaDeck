use std::{fmt, sync::Arc};

use async_trait::async_trait;
use secrecy::{ExposeSecret as _, SecretString};
use serde_json::Value;

use crate::{RpcCall, RpcError, RpcTransport};

#[derive(Clone)]
pub struct RpcSecret(Arc<SecretString>);

impl RpcSecret {
    #[must_use]
    pub fn new(value: impl Into<String>) -> Self {
        Self(Arc::new(SecretString::new(value.into())))
    }

    fn token(&self) -> String {
        format!("token:{}", self.0.expose_secret())
    }
}

impl fmt::Debug for RpcSecret {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("RpcSecret([REDACTED])")
    }
}

#[derive(Clone)]
pub struct AuthenticatedTransport<T> {
    inner: T,
    secret: Option<RpcSecret>,
}

impl<T> AuthenticatedTransport<T> {
    #[must_use]
    pub const fn new(inner: T, secret: Option<RpcSecret>) -> Self {
        Self { inner, secret }
    }

    #[must_use]
    pub const fn inner(&self) -> &T {
        &self.inner
    }

    fn inject_secret(&self, mut params: Vec<Value>) -> Vec<Value> {
        if let Some(secret) = &self.secret {
            params.insert(0, Value::String(secret.token()));
        }
        params
    }
}

impl<T> fmt::Debug for AuthenticatedTransport<T> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AuthenticatedTransport")
            .field("secret", &self.secret.as_ref().map(|_| "[REDACTED]"))
            .finish_non_exhaustive()
    }
}

#[async_trait]
impl<T> RpcTransport for AuthenticatedTransport<T>
where
    T: RpcTransport,
{
    async fn call(&self, method: &str, params: Vec<Value>) -> Result<Value, RpcError> {
        self.inner.call(method, self.inject_secret(params)).await
    }

    async fn batch(&self, calls: Vec<RpcCall>) -> Result<Vec<Result<Value, RpcError>>, RpcError> {
        let calls = calls
            .into_iter()
            .map(|call| RpcCall {
                method: call.method,
                params: self.inject_secret(call.params),
            })
            .collect();
        self.inner.batch(calls).await
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;

    #[derive(Default)]
    struct RecordingTransport {
        params: Mutex<Vec<Value>>,
    }

    #[async_trait]
    impl RpcTransport for RecordingTransport {
        async fn call(&self, _method: &str, params: Vec<Value>) -> Result<Value, RpcError> {
            *self
                .params
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner()) = params;
            Ok(Value::Null)
        }
    }

    #[tokio::test]
    async fn secret_is_injected_once_and_debug_is_redacted() {
        let transport = AuthenticatedTransport::new(
            RecordingTransport::default(),
            Some(RpcSecret::new("highly-sensitive")),
        );

        if let Err(error) = transport
            .call("aria2.getVersion", vec![Value::String("argument".into())])
            .await
        {
            panic!("recording call failed: {error}");
        }

        let params = transport
            .inner()
            .params
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        assert_eq!(params[0], Value::String("token:highly-sensitive".into()));
        assert_eq!(params[1], Value::String("argument".into()));
        assert!(!format!("{transport:?}").contains("highly-sensitive"));
    }
}
