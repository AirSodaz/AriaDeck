use ariadeck_application::{
    AddDownloadRequest, DownloadEngineGateway, GatewayError, GatewayErrorKind,
};
use ariadeck_domain::{Gid, GlobalStat, TaskSnapshot};
use async_trait::async_trait;
use serde::de::DeserializeOwned;
use serde_json::{Map, Value, json};

use crate::{
    RpcError, RpcTransport,
    models::{GlobalStatWire, TaskKey, TaskWire, VersionInfo, VersionWire},
};

#[derive(Clone)]
pub struct Aria2Client<T> {
    transport: T,
}

impl<T> Aria2Client<T> {
    #[must_use]
    pub const fn new(transport: T) -> Self {
        Self { transport }
    }

    #[must_use]
    pub const fn transport(&self) -> &T {
        &self.transport
    }
}

impl<T> Aria2Client<T>
where
    T: RpcTransport,
{
    pub async fn get_version(&self) -> Result<VersionInfo, RpcError> {
        const METHOD: &str = "aria2.getVersion";
        let value = self.transport.call(METHOD, Vec::new()).await?;
        decode::<VersionWire>(METHOD, value).map(Into::into)
    }

    pub async fn get_global_stat(&self) -> Result<GlobalStat, RpcError> {
        const METHOD: &str = "aria2.getGlobalStat";
        let value = self.transport.call(METHOD, Vec::new()).await?;
        decode::<GlobalStatWire>(METHOD, value)?.into_domain(METHOD)
    }

    pub async fn tell_active(&self, keys: &[TaskKey]) -> Result<Vec<TaskSnapshot>, RpcError> {
        const METHOD: &str = "aria2.tellActive";
        self.fetch_tasks(METHOD, vec![task_keys(keys)]).await
    }

    pub async fn tell_waiting(
        &self,
        offset: i64,
        count: u32,
        keys: &[TaskKey],
    ) -> Result<Vec<TaskSnapshot>, RpcError> {
        const METHOD: &str = "aria2.tellWaiting";
        self.fetch_tasks(METHOD, vec![json!(offset), json!(count), task_keys(keys)])
            .await
    }

    pub async fn tell_stopped(
        &self,
        offset: i64,
        count: u32,
        keys: &[TaskKey],
    ) -> Result<Vec<TaskSnapshot>, RpcError> {
        const METHOD: &str = "aria2.tellStopped";
        self.fetch_tasks(METHOD, vec![json!(offset), json!(count), task_keys(keys)])
            .await
    }

    pub async fn add_uri(&self, request: &AddDownloadRequest) -> Result<Gid, RpcError> {
        let mut options = request
            .options
            .iter()
            .cloned()
            .map(|(key, value)| (key, Value::String(value)))
            .collect::<Map<_, _>>();
        if let Some(destination) = &request.destination {
            options.insert(
                "dir".into(),
                Value::String(destination.to_string_lossy().into_owned()),
            );
        }
        self.call_gid(
            "aria2.addUri",
            vec![json!(request.uris), Value::Object(options)],
        )
        .await
    }

    pub async fn pause(&self, gid: Gid) -> Result<Gid, RpcError> {
        self.call_gid("aria2.pause", vec![json!(gid.to_string())])
            .await
    }

    pub async fn resume(&self, gid: Gid) -> Result<Gid, RpcError> {
        self.call_gid("aria2.unpause", vec![json!(gid.to_string())])
            .await
    }

    pub async fn remove(&self, gid: Gid) -> Result<Gid, RpcError> {
        self.call_gid("aria2.remove", vec![json!(gid.to_string())])
            .await
    }

    pub async fn shutdown(&self) -> Result<(), RpcError> {
        let value = self.transport.call("aria2.shutdown", Vec::new()).await?;
        match value.as_str() {
            Some("OK") => Ok(()),
            _ => Err(RpcError::InvalidData {
                method: "aria2.shutdown".into(),
                field: "result".into(),
                message: "expected OK".into(),
            }),
        }
    }

    async fn fetch_tasks(
        &self,
        method: &str,
        params: Vec<Value>,
    ) -> Result<Vec<TaskSnapshot>, RpcError> {
        let value = self.transport.call(method, params).await?;
        decode::<Vec<TaskWire>>(method, value)?
            .into_iter()
            .map(|task| task.into_domain(method))
            .collect()
    }

    async fn call_gid(&self, method: &str, params: Vec<Value>) -> Result<Gid, RpcError> {
        let value = self.transport.call(method, params).await?;
        let gid = value.as_str().ok_or_else(|| RpcError::InvalidData {
            method: method.into(),
            field: "result".into(),
            message: "expected a GID string".into(),
        })?;
        gid.parse::<Gid>().map_err(|error| RpcError::InvalidData {
            method: method.into(),
            field: "result".into(),
            message: error.to_string(),
        })
    }
}

fn task_keys(keys: &[TaskKey]) -> Value {
    Value::Array(
        keys.iter()
            .map(|key| Value::String(key.as_str().into()))
            .collect(),
    )
}

fn decode<T>(method: &str, value: Value) -> Result<T, RpcError>
where
    T: DeserializeOwned,
{
    serde_json::from_value(value).map_err(|error| RpcError::InvalidData {
        method: method.into(),
        field: "response".into(),
        message: error.to_string(),
    })
}

#[async_trait]
impl<T> DownloadEngineGateway for Aria2Client<T>
where
    T: RpcTransport,
{
    async fn add_download(&self, request: &AddDownloadRequest) -> Result<Gid, GatewayError> {
        self.add_uri(request).await.map_err(map_gateway_error)
    }

    async fn pause(&self, gid: Gid) -> Result<(), GatewayError> {
        Aria2Client::pause(self, gid)
            .await
            .map(|_| ())
            .map_err(map_gateway_error)
    }

    async fn resume(&self, gid: Gid) -> Result<(), GatewayError> {
        Aria2Client::resume(self, gid)
            .await
            .map(|_| ())
            .map_err(map_gateway_error)
    }

    async fn remove(&self, gid: Gid) -> Result<(), GatewayError> {
        Aria2Client::remove(self, gid)
            .await
            .map(|_| ())
            .map_err(map_gateway_error)
    }
}

fn map_gateway_error(error: RpcError) -> GatewayError {
    let (kind, retryable) = match &error {
        RpcError::Closed | RpcError::Transport(_) => (GatewayErrorKind::Disconnected, true),
        RpcError::Timeout { .. } => (GatewayErrorKind::Timeout, true),
        RpcError::Remote { message, .. }
            if message.to_ascii_lowercase().contains("unauthorized") =>
        {
            (GatewayErrorKind::Authentication, false)
        }
        RpcError::Remote { .. } => (GatewayErrorKind::Rejected, false),
        RpcError::Protocol(_) | RpcError::Serialization(_) | RpcError::InvalidData { .. } => {
            (GatewayErrorKind::Internal, false)
        }
    };
    GatewayError::new(kind, error.to_string(), retryable)
}

#[cfg(test)]
mod tests {
    use std::{collections::VecDeque, sync::Mutex};

    use async_trait::async_trait;
    use serde_json::json;

    use super::*;

    struct ScriptedTransport {
        responses: Mutex<VecDeque<Result<Value, RpcError>>>,
        calls: Mutex<Vec<(String, Vec<Value>)>>,
    }

    impl ScriptedTransport {
        fn new(responses: impl IntoIterator<Item = Result<Value, RpcError>>) -> Self {
            Self {
                responses: Mutex::new(responses.into_iter().collect()),
                calls: Mutex::default(),
            }
        }
    }

    #[async_trait]
    impl RpcTransport for ScriptedTransport {
        async fn call(&self, method: &str, params: Vec<Value>) -> Result<Value, RpcError> {
            self.calls
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .push((method.into(), params));
            self.responses
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .pop_front()
                .unwrap_or(Err(RpcError::Closed))
        }
    }

    #[tokio::test]
    async fn typed_methods_convert_version_global_stat_and_tasks() {
        let transport = ScriptedTransport::new([
            Ok(json!({"version": "1.37.0", "enabledFeatures": ["BitTorrent", "HTTPS"]})),
            Ok(json!({
                "downloadSpeed": "123",
                "uploadSpeed": "45",
                "numActive": "2",
                "numWaiting": "3",
                "numStoppedTotal": "4"
            })),
            Ok(json!([{
                "gid": "0000000000000001",
                "status": "active",
                "files": [{"path": "/tmp/item.bin"}]
            }])),
        ]);
        let client = Aria2Client::new(transport);

        let version = match client.get_version().await {
            Ok(version) => version,
            Err(error) => panic!("getVersion failed: {error}"),
        };
        let stat = match client.get_global_stat().await {
            Ok(stat) => stat,
            Err(error) => panic!("getGlobalStat failed: {error}"),
        };
        let tasks = match client.tell_active(TaskKey::LIST_PROJECTION).await {
            Ok(tasks) => tasks,
            Err(error) => panic!("tellActive failed: {error}"),
        };

        assert_eq!(version.version, "1.37.0");
        assert_eq!(stat.download_speed, ariadeck_domain::ByteRate::new(123));
        assert_eq!(tasks[0].display_name, "item.bin");
    }

    #[tokio::test]
    async fn add_uri_builds_options_without_losing_destination() {
        let transport = ScriptedTransport::new([Ok(json!("0000000000000009"))]);
        let client = Aria2Client::new(transport);
        let request = AddDownloadRequest {
            uris: vec!["https://example.test/file".into()],
            destination: Some("D:/Downloads".into()),
            options: vec![("max-download-limit".into(), "1M".into())],
        };

        let gid = match client.add_uri(&request).await {
            Ok(gid) => gid,
            Err(error) => panic!("addUri failed: {error}"),
        };
        assert_eq!(gid, Gid::from_u64(9));
        let calls = client
            .transport()
            .calls
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        assert_eq!(calls[0].0, "aria2.addUri");
        assert_eq!(calls[0].1[1]["dir"], Value::String("D:/Downloads".into()));
        assert_eq!(
            calls[0].1[1]["max-download-limit"],
            Value::String("1M".into())
        );
    }

    #[test]
    fn authentication_remote_error_maps_to_gateway_category() {
        let error = map_gateway_error(RpcError::Remote {
            code: 1,
            message: "Unauthorized".into(),
            data: None,
        });

        assert_eq!(error.kind, GatewayErrorKind::Authentication);
        assert!(!error.retryable);
    }
}
