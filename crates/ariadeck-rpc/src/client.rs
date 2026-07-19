use ariadeck_application::{
    AddDownloadRequest, DownloadEngineGateway, GatewayError, GatewayErrorKind, TaskDetailsGateway,
    TaskRemovalTarget,
};
use ariadeck_domain::{Gid, GlobalStat, TaskDetails, TaskSnapshot};
use async_trait::async_trait;
use serde::de::DeserializeOwned;
use serde_json::{Map, Value, json};

use crate::{
    RpcError, RpcTransport,
    models::{GlobalStatWire, TaskKey, TaskWire, VersionInfo, VersionWire},
};

const WAITING_PAGE_SIZE: u32 = 1_000;

#[derive(Clone)]
pub struct Aria2Client<T> {
    transport: T,
}

pub(crate) struct InitialRpcSnapshot {
    pub version: VersionInfo,
    pub global_stat: GlobalStat,
    pub active: Vec<TaskSnapshot>,
    pub waiting: Vec<TaskSnapshot>,
    pub stopped: Vec<TaskSnapshot>,
}

pub(crate) struct LiveRpcSnapshot {
    pub active: Vec<TaskSnapshot>,
    pub waiting: Vec<TaskSnapshot>,
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
            options.insert("dir".into(), Value::String(destination.as_str().to_owned()));
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

    pub async fn remove_download_result(&self, gid: Gid) -> Result<(), RpcError> {
        self.call_ok("aria2.removeDownloadResult", vec![json!(gid.to_string())])
            .await
    }

    pub async fn task_details(&self, gid: Gid) -> Result<TaskDetails, RpcError> {
        const METHOD: &str = "aria2.tellStatus";
        let value = self
            .transport
            .call(
                METHOD,
                vec![
                    json!(gid.to_string()),
                    task_keys(TaskKey::DETAILS_PROJECTION),
                ],
            )
            .await?;
        decode::<TaskWire>(METHOD, value)?.into_details(METHOD)
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

    pub(crate) async fn initial_sync_snapshot(
        &self,
        stopped_count: u32,
    ) -> Result<InitialRpcSnapshot, RpcError> {
        let keys = task_keys(TaskKey::DISCOVERY_PROJECTION);
        let results = self
            .transport
            .batch(vec![
                crate::RpcCall::new("aria2.getVersion", Vec::new()),
                crate::RpcCall::new("aria2.getGlobalStat", Vec::new()),
                crate::RpcCall::new("aria2.tellActive", vec![keys.clone()]),
                crate::RpcCall::new(
                    "aria2.tellWaiting",
                    vec![json!(0), json!(WAITING_PAGE_SIZE), keys.clone()],
                ),
                crate::RpcCall::new(
                    "aria2.tellStopped",
                    vec![json!(0), json!(stopped_count), keys],
                ),
            ])
            .await?;
        let mut results = results.into_iter();
        let version = decode::<VersionWire>(
            "aria2.getVersion",
            next_batch_result(&mut results, "aria2.getVersion")?,
        )?
        .into();
        let global_stat = decode::<GlobalStatWire>(
            "aria2.getGlobalStat",
            next_batch_result(&mut results, "aria2.getGlobalStat")?,
        )?
        .into_domain("aria2.getGlobalStat")?;
        let active = decode_tasks(
            "aria2.tellActive",
            next_batch_result(&mut results, "aria2.tellActive")?,
        )?;
        let waiting = decode_tasks(
            "aria2.tellWaiting",
            next_batch_result(&mut results, "aria2.tellWaiting")?,
        )?;
        let waiting = self
            .complete_waiting_snapshot(
                waiting,
                global_stat.waiting_tasks,
                TaskKey::DISCOVERY_PROJECTION,
            )
            .await?;
        let stopped = decode_tasks(
            "aria2.tellStopped",
            next_batch_result(&mut results, "aria2.tellStopped")?,
        )?;
        Ok(InitialRpcSnapshot {
            version,
            global_stat,
            active,
            waiting,
            stopped,
        })
    }

    pub(crate) async fn refresh_live_snapshot(&self) -> Result<LiveRpcSnapshot, RpcError> {
        let keys = task_keys(TaskKey::LIST_PROJECTION);
        let results = self
            .transport
            .batch(vec![
                crate::RpcCall::new("aria2.getGlobalStat", Vec::new()),
                crate::RpcCall::new("aria2.tellActive", vec![keys.clone()]),
                crate::RpcCall::new(
                    "aria2.tellWaiting",
                    vec![json!(0), json!(WAITING_PAGE_SIZE), keys],
                ),
            ])
            .await?;
        let mut results = results.into_iter();
        let global_stat = decode::<GlobalStatWire>(
            "aria2.getGlobalStat",
            next_batch_result(&mut results, "aria2.getGlobalStat")?,
        )?
        .into_domain("aria2.getGlobalStat")?;
        let active = decode_tasks(
            "aria2.tellActive",
            next_batch_result(&mut results, "aria2.tellActive")?,
        )?;
        let waiting = decode_tasks(
            "aria2.tellWaiting",
            next_batch_result(&mut results, "aria2.tellWaiting")?,
        )?;
        let waiting = self
            .complete_waiting_snapshot(waiting, global_stat.waiting_tasks, TaskKey::LIST_PROJECTION)
            .await?;
        Ok(LiveRpcSnapshot { active, waiting })
    }

    async fn complete_waiting_snapshot(
        &self,
        mut waiting: Vec<TaskSnapshot>,
        expected_total: u32,
        projection: &[TaskKey],
    ) -> Result<Vec<TaskSnapshot>, RpcError> {
        let loaded = u32::try_from(waiting.len()).map_err(|error| RpcError::InvalidData {
            method: "aria2.tellWaiting".into(),
            field: "result".into(),
            message: error.to_string(),
        })?;
        if loaded >= expected_total {
            return Ok(waiting);
        }

        let keys = task_keys(projection);
        let mut offset = loaded;
        let mut calls = Vec::new();
        while offset < expected_total {
            let count = expected_total.saturating_sub(offset).min(WAITING_PAGE_SIZE);
            calls.push(crate::RpcCall::new(
                "aria2.tellWaiting",
                vec![json!(i64::from(offset)), json!(count), keys.clone()],
            ));
            offset = offset.saturating_add(count);
        }

        for result in self.transport.batch(calls).await? {
            waiting.extend(decode_tasks("aria2.tellWaiting", result?)?);
        }
        Ok(waiting)
    }

    pub(crate) async fn refresh_tasks(
        &self,
        gids: &[Gid],
    ) -> Result<Vec<(Gid, Option<TaskSnapshot>)>, RpcError> {
        if gids.is_empty() {
            return Ok(Vec::new());
        }
        let keys = task_keys(TaskKey::DISCOVERY_PROJECTION);
        let results = self
            .transport
            .batch(
                gids.iter()
                    .map(|gid| {
                        crate::RpcCall::new(
                            "aria2.tellStatus",
                            vec![json!(gid.to_string()), keys.clone()],
                        )
                    })
                    .collect(),
            )
            .await?;

        gids.iter()
            .copied()
            .zip(results)
            .map(|(gid, result)| match result {
                Ok(value) => decode::<TaskWire>("aria2.tellStatus", value)
                    .and_then(|task| task.into_domain("aria2.tellStatus"))
                    .map(|task| (gid, Some(task))),
                Err(RpcError::Remote { message, .. }) if is_gid_not_found(&message) => {
                    Ok((gid, None))
                }
                Err(error) => Err(error),
            })
            .collect()
    }

    async fn fetch_tasks(
        &self,
        method: &str,
        params: Vec<Value>,
    ) -> Result<Vec<TaskSnapshot>, RpcError> {
        let value = self.transport.call(method, params).await?;
        decode_tasks(method, value)
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

    async fn call_ok(&self, method: &str, params: Vec<Value>) -> Result<(), RpcError> {
        let value = self.transport.call(method, params).await?;
        if value.as_str() == Some("OK") {
            return Ok(());
        }
        Err(RpcError::InvalidData {
            method: method.into(),
            field: "result".into(),
            message: "expected the string OK".into(),
        })
    }
}

fn next_batch_result(
    results: &mut impl Iterator<Item = Result<Value, RpcError>>,
    method: &str,
) -> Result<Value, RpcError> {
    results.next().ok_or_else(|| {
        RpcError::Protocol(format!("batch response is missing result for {method}"))
    })?
}

fn decode_tasks(method: &str, value: Value) -> Result<Vec<TaskSnapshot>, RpcError> {
    decode::<Vec<TaskWire>>(method, value)?
        .into_iter()
        .map(|task| task.into_domain(method))
        .collect()
}

fn is_gid_not_found(message: &str) -> bool {
    let message = message.to_ascii_lowercase();
    message.contains("gid") && message.contains("not found")
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
        self.add_uri(request).await.map_err(map_mutation_error)
    }

    async fn pause(&self, gid: Gid) -> Result<(), GatewayError> {
        Aria2Client::pause(self, gid)
            .await
            .map(|_| ())
            .map_err(map_mutation_error)
    }

    async fn resume(&self, gid: Gid) -> Result<(), GatewayError> {
        Aria2Client::resume(self, gid)
            .await
            .map(|_| ())
            .map_err(map_mutation_error)
    }

    async fn remove(&self, gid: Gid, target: TaskRemovalTarget) -> Result<(), GatewayError> {
        match target {
            TaskRemovalTarget::LiveTask => match Aria2Client::remove(self, gid).await {
                Ok(_) => Ok(()),
                Err(error) if live_task_became_stopped(&error) => {
                    self.remove_download_result(gid).await
                }
                Err(error) => Err(error),
            },
            TaskRemovalTarget::DownloadResult => self.remove_download_result(gid).await,
        }
        .map_err(map_mutation_error)
    }
}

#[async_trait]
impl<T> TaskDetailsGateway for Aria2Client<T>
where
    T: RpcTransport,
{
    async fn task_details(&self, gid: Gid) -> Result<TaskDetails, GatewayError> {
        Aria2Client::task_details(self, gid)
            .await
            .map_err(map_query_error)
    }
}

fn map_query_error(error: RpcError) -> GatewayError {
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

fn map_mutation_error(error: RpcError) -> GatewayError {
    let kind = match &error {
        RpcError::Closed
        | RpcError::Transport(_)
        | RpcError::Timeout { .. }
        | RpcError::Protocol(_)
        | RpcError::InvalidData { .. } => GatewayErrorKind::OutcomeUnknown,
        RpcError::Remote { message, .. }
            if message.to_ascii_lowercase().contains("unauthorized") =>
        {
            GatewayErrorKind::Authentication
        }
        RpcError::Remote { .. } => GatewayErrorKind::Rejected,
        RpcError::Serialization(_) => GatewayErrorKind::Internal,
    };
    GatewayError::new(kind, error.to_string(), false)
}

fn live_task_became_stopped(error: &RpcError) -> bool {
    let RpcError::Remote { message, .. } = error else {
        return false;
    };
    let message = message.to_ascii_lowercase();
    !message.contains("unauthorized")
        && (message.contains("not found")
            || message.contains("not active")
            || message.contains("stopped"))
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
        let tasks = match client.tell_active(TaskKey::DISCOVERY_PROJECTION).await {
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

    #[tokio::test]
    async fn details_projection_decodes_engine_paths_and_files() {
        let transport = ScriptedTransport::new([Ok(json!({
            "gid": "0000000000000009",
            "dir": "/srv/downloads",
            "infoHash": "abc123",
            "pieceLength": "1048576",
            "numPieces": "4",
            "files": [{
                "index": "1",
                "path": "/srv/downloads/archive.iso",
                "length": "4194304",
                "completedLength": "1048576",
                "selected": "true",
                "uris": [{"uri": "https://secret.example/item"}]
            }]
        }))]);
        let client = Aria2Client::new(transport);

        let details = match client.task_details(Gid::from_u64(9)).await {
            Ok(details) => details,
            Err(error) => panic!("tellStatus details failed: {error}"),
        };

        assert_eq!(details.gid, Gid::from_u64(9));
        assert_eq!(
            details
                .directory
                .as_ref()
                .map(ariadeck_domain::EnginePath::as_str),
            Some("/srv/downloads")
        );
        assert_eq!(details.files.len(), 1);
        assert_eq!(details.files[0].path.as_str(), "/srv/downloads/archive.iso");
        assert!(details.files[0].selected);
        let calls = client
            .transport()
            .calls
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        assert_eq!(calls[0].0, "aria2.tellStatus");
        assert_eq!(
            calls[0].1[1],
            json!([
                "gid",
                "files",
                "dir",
                "infoHash",
                "pieceLength",
                "numPieces"
            ])
        );
    }

    #[tokio::test]
    async fn terminal_removal_uses_remove_download_result() {
        let transport = ScriptedTransport::new([Ok(json!("OK"))]);
        let client = Aria2Client::new(transport);

        let result = DownloadEngineGateway::remove(
            &client,
            Gid::from_u64(9),
            TaskRemovalTarget::DownloadResult,
        )
        .await;

        assert!(result.is_ok());
        let calls = client
            .transport()
            .calls
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        assert_eq!(calls[0].0, "aria2.removeDownloadResult");
    }

    #[tokio::test]
    async fn live_removal_falls_back_when_the_task_completed_before_rpc_execution() {
        let transport = ScriptedTransport::new([
            Err(RpcError::Remote {
                code: 1,
                message: "GID is not found in active downloads".into(),
                data: None,
            }),
            Ok(json!("OK")),
        ]);
        let client = Aria2Client::new(transport);

        let result =
            DownloadEngineGateway::remove(&client, Gid::from_u64(9), TaskRemovalTarget::LiveTask)
                .await;

        assert!(result.is_ok());
        let calls = client
            .transport()
            .calls
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        assert_eq!(calls[0].0, "aria2.remove");
        assert_eq!(calls[1].0, "aria2.removeDownloadResult");
    }

    #[tokio::test]
    async fn malformed_remove_download_result_is_an_unknown_mutation_outcome() {
        let transport = ScriptedTransport::new([Ok(json!("0000000000000009"))]);
        let client = Aria2Client::new(transport);

        let error = DownloadEngineGateway::remove(
            &client,
            Gid::from_u64(9),
            TaskRemovalTarget::DownloadResult,
        )
        .await
        .expect_err("unexpected response must not be reported as success");

        assert_eq!(error.kind, GatewayErrorKind::OutcomeUnknown);
        assert!(!error.retryable);
    }

    #[tokio::test]
    async fn live_snapshot_fetches_every_waiting_page_before_reconcile() {
        let first_page = (1_u64..=1_000)
            .map(|value| {
                json!({
                    "gid": format!("{value:016x}"),
                    "status": "waiting",
                    "files": [{"path": format!("/tmp/{value}.bin")}]
                })
            })
            .collect::<Vec<_>>();
        let transport = ScriptedTransport::new([
            Ok(json!({
                "numActive": "0",
                "numWaiting": "1001",
                "numStoppedTotal": "0"
            })),
            Ok(json!([])),
            Ok(Value::Array(first_page)),
            Ok(json!([{
                "gid": "00000000000003e9",
                "status": "waiting",
                "files": [{"path": "/tmp/1001.bin"}]
            }])),
        ]);
        let client = Aria2Client::new(transport);

        let snapshot = match client.refresh_live_snapshot().await {
            Ok(snapshot) => snapshot,
            Err(error) => panic!("live snapshot failed: {error}"),
        };

        assert_eq!(snapshot.waiting.len(), 1_001);
        let calls = client
            .transport()
            .calls
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        assert_eq!(calls.len(), 4);
        assert_eq!(
            calls[1].1[0],
            json!([
                "gid",
                "status",
                "totalLength",
                "completedLength",
                "uploadLength",
                "downloadSpeed",
                "uploadSpeed",
                "connections",
                "errorCode",
                "errorMessage",
                "verifyIntegrityPending"
            ])
        );
        assert_eq!(calls[3].0, "aria2.tellWaiting");
        assert_eq!(calls[3].1[0], json!(1_000));
        assert_eq!(calls[3].1[1], json!(1));
    }

    #[test]
    fn authentication_remote_error_maps_to_gateway_category() {
        let error = map_query_error(RpcError::Remote {
            code: 1,
            message: "Unauthorized".into(),
            data: None,
        });

        assert_eq!(error.kind, GatewayErrorKind::Authentication);
        assert!(!error.retryable);
    }

    #[test]
    fn mutating_timeout_is_reported_as_an_unknown_outcome() {
        let error = map_mutation_error(RpcError::Timeout {
            method: "aria2.addUri".into(),
        });

        assert_eq!(error.kind, GatewayErrorKind::OutcomeUnknown);
        assert!(!error.retryable);
    }

    #[test]
    fn malformed_mutation_responses_are_reported_as_unknown_outcomes() {
        for rpc_error in [
            RpcError::Protocol("malformed response".into()),
            RpcError::InvalidData {
                method: "aria2.addUri".into(),
                field: "result".into(),
                message: "expected GID".into(),
            },
        ] {
            let error = map_mutation_error(rpc_error);
            assert_eq!(error.kind, GatewayErrorKind::OutcomeUnknown);
            assert!(!error.retryable);
        }
    }
}
