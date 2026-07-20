use ariadeck_application::{
    AddDownloadRequest, DownloadEngineGateway, DownloadProxyConfig, DownloadProxyMode,
    FileConflictPolicy, GatewayError, GatewayErrorKind, TaskDetailsGateway, TaskRemovalTarget,
};
use ariadeck_domain::{Gid, GlobalStat, TaskDetails, TaskSnapshot};
use async_trait::async_trait;
use secrecy::ExposeSecret;
use serde::{Deserialize, de::DeserializeOwned};
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
        apply_file_conflict_policy(&mut options, request.file_conflict);
        if let Some(destination) = &request.destination {
            options.insert("dir".into(), Value::String(destination.as_str().to_owned()));
        }
        self.add_uri_with_options(&request.uris, options).await
    }

    async fn retry_uri(
        &self,
        gid: Gid,
        fallback: &AddDownloadRequest,
    ) -> Result<Gid, RetryRpcError> {
        let mut options = self.get_options(gid).await.map_err(RetryRpcError::Query)?;
        let discovered_uris = match self.get_uris(gid).await {
            Ok(uris) => uris,
            Err(error) if is_no_uri_data(&error) => Vec::new(),
            Err(error) => return Err(RetryRpcError::Query(error)),
        };
        let uris = if discovered_uris.is_empty() {
            fallback.uris.clone()
        } else {
            discovered_uris
        };

        options.remove("gid");
        options.remove("pause");
        for (key, value) in &fallback.options {
            options
                .entry(key.clone())
                .or_insert_with(|| Value::String(value.clone()));
        }
        if let Some(destination) = &fallback.destination {
            options
                .entry("dir")
                .or_insert_with(|| Value::String(destination.as_str().to_owned()));
        }

        self.add_uri_with_options(&uris, options)
            .await
            .map_err(RetryRpcError::Mutation)
    }

    pub async fn get_options(&self, gid: Gid) -> Result<Map<String, Value>, RpcError> {
        const METHOD: &str = "aria2.getOption";
        let value = self
            .transport
            .call(METHOD, vec![json!(gid.to_string())])
            .await?;
        let options = decode::<Map<String, Value>>(METHOD, value)?;
        for (key, value) in &options {
            let valid = matches!(value, Value::String(_))
                || matches!(value, Value::Array(values) if values.iter().all(Value::is_string));
            if !valid {
                return Err(RpcError::InvalidData {
                    method: METHOD.into(),
                    field: key.clone(),
                    message: "expected a string or an array of strings".into(),
                });
            }
        }
        Ok(options)
    }

    async fn get_uris(&self, gid: Gid) -> Result<Vec<String>, RpcError> {
        const METHOD: &str = "aria2.getUris";
        let value = self
            .transport
            .call(METHOD, vec![json!(gid.to_string())])
            .await?;
        let uris = decode::<Vec<UriWire>>(METHOD, value)?;
        let mut seen = std::collections::HashSet::new();
        Ok(uris
            .into_iter()
            .map(|uri| uri.uri.trim().to_owned())
            .filter(|uri| !uri.is_empty() && seen.insert(uri.clone()))
            .collect())
    }

    async fn add_uri_with_options(
        &self,
        uris: &[String],
        options: Map<String, Value>,
    ) -> Result<Gid, RpcError> {
        self.call_gid("aria2.addUri", vec![json!(uris), Value::Object(options)])
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

    pub async fn change_options(
        &self,
        gid: Gid,
        options: &[(String, String)],
    ) -> Result<(), RpcError> {
        let options = options
            .iter()
            .cloned()
            .map(|(key, value)| (key, Value::String(value)))
            .collect::<Map<_, _>>();
        self.call_ok(
            "aria2.changeOption",
            vec![json!(gid.to_string()), Value::Object(options)],
        )
        .await
    }

    pub async fn change_global_options(
        &self,
        options: &[(String, String)],
    ) -> Result<(), RpcError> {
        let options = options
            .iter()
            .cloned()
            .map(|(key, value)| (key, Value::String(value)))
            .collect::<Map<_, _>>();
        self.call_ok("aria2.changeGlobalOption", vec![Value::Object(options)])
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

fn apply_file_conflict_policy(options: &mut Map<String, Value>, policy: FileConflictPolicy) {
    let (allow_overwrite, auto_file_renaming) = match policy {
        FileConflictPolicy::AutoRename => (false, true),
        FileConflictPolicy::Reject => (false, false),
        FileConflictPolicy::Overwrite => (true, false),
    };
    options.insert(
        "allow-overwrite".into(),
        Value::String(allow_overwrite.to_string()),
    );
    options.insert(
        "auto-file-renaming".into(),
        Value::String(auto_file_renaming.to_string()),
    );
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

fn is_no_uri_data(error: &RpcError) -> bool {
    matches!(
        error,
        RpcError::Remote { message, .. }
            if message.to_ascii_lowercase().contains("no uri data is available")
    )
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

    async fn retry_download(
        &self,
        gid: Gid,
        fallback: &AddDownloadRequest,
    ) -> Result<Gid, GatewayError> {
        match self.retry_uri(gid, fallback).await {
            Ok(gid) => Ok(gid),
            Err(RetryRpcError::Query(error)) => Err(map_query_error(error)),
            Err(RetryRpcError::Mutation(error)) => Err(map_mutation_error(error)),
        }
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

    async fn change_options(
        &self,
        gid: Gid,
        options: &[(String, String)],
    ) -> Result<(), GatewayError> {
        Aria2Client::change_options(self, gid, options)
            .await
            .map_err(map_mutation_error)
    }

    async fn apply_download_proxy(&self, config: &DownloadProxyConfig) -> Result<(), GatewayError> {
        self.change_global_options(&download_proxy_options(config))
            .await
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

fn download_proxy_options(config: &DownloadProxyConfig) -> Vec<(String, String)> {
    let manual = config.mode == DownloadProxyMode::Manual;
    let endpoint = |value: &Option<String>| {
        if manual {
            value.clone().unwrap_or_default()
        } else {
            String::new()
        }
    };
    let username = manual
        .then(|| config.username.clone())
        .flatten()
        .unwrap_or_default();
    let password = if manual {
        config
            .password
            .as_ref()
            .map(ExposeSecret::expose_secret)
            .cloned()
            .unwrap_or_default()
    } else {
        String::new()
    };
    let no_proxy = if manual {
        config.no_proxy.join(",")
    } else {
        String::new()
    };
    let mut options = vec![
        ("all-proxy".into(), endpoint(&config.all_proxy)),
        ("http-proxy".into(), endpoint(&config.http_proxy)),
        ("https-proxy".into(), endpoint(&config.https_proxy)),
        ("ftp-proxy".into(), endpoint(&config.ftp_proxy)),
        ("no-proxy".into(), no_proxy),
    ];
    for prefix in ["all", "http", "https", "ftp"] {
        options.push((format!("{prefix}-proxy-user"), username.clone()));
        options.push((format!("{prefix}-proxy-passwd"), password.clone()));
    }
    options
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

#[derive(Deserialize)]
struct UriWire {
    uri: String,
}

enum RetryRpcError {
    Query(RpcError),
    Mutation(RpcError),
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
            uris: vec![
                "https://example.test/file".into(),
                "https://mirror.test/file".into(),
            ],
            destination: Some("D:/Downloads".into()),
            file_conflict: FileConflictPolicy::AutoRename,
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
        assert_eq!(
            calls[0].1[0],
            json!(["https://example.test/file", "https://mirror.test/file"])
        );
        assert_eq!(calls[0].1[1]["dir"], Value::String("D:/Downloads".into()));
        assert_eq!(
            calls[0].1[1]["max-download-limit"],
            Value::String("1M".into())
        );
        assert_eq!(
            calls[0].1[1]["allow-overwrite"],
            Value::String("false".into())
        );
        assert_eq!(
            calls[0].1[1]["auto-file-renaming"],
            Value::String("true".into())
        );
    }

    #[test]
    fn file_conflict_policy_maps_to_authoritative_aria2_options() {
        for (policy, allow_overwrite, auto_file_renaming) in [
            (FileConflictPolicy::AutoRename, "false", "true"),
            (FileConflictPolicy::Reject, "false", "false"),
            (FileConflictPolicy::Overwrite, "true", "false"),
        ] {
            let mut options = Map::from_iter([
                ("allow-overwrite".into(), Value::String("unexpected".into())),
                (
                    "auto-file-renaming".into(),
                    Value::String("unexpected".into()),
                ),
            ]);

            apply_file_conflict_policy(&mut options, policy);

            assert_eq!(
                options["allow-overwrite"],
                Value::String(allow_overwrite.into())
            );
            assert_eq!(
                options["auto-file-renaming"],
                Value::String(auto_file_renaming.into())
            );
        }
    }

    #[tokio::test]
    async fn retry_replays_sources_and_safe_task_options_with_a_new_gid() {
        let transport = ScriptedTransport::new([
            Ok(json!({
                "gid": "0000000000000007",
                "pause": "true",
                "dir": "/downloads",
                "out": "renamed.iso",
                "header": ["Cookie: session=abc", "Referer: https://example.test/"],
                "load-cookies": "/cookies.txt",
                "all-proxy": "http://proxy.test:8080",
                "max-download-limit": "1M",
                "max-connection-per-server": "4",
                "checksum": "sha-256=deadbeef",
                "select-file": "1,3-4"
            })),
            Ok(json!([
                {"uri": "https://example.test/archive.iso", "status": "used"},
                {"uri": "https://mirror.test/archive.iso", "status": "waiting"}
            ])),
            Ok(json!("0000000000000009")),
        ]);
        let client = Aria2Client::new(transport);
        let fallback = AddDownloadRequest {
            uris: vec!["https://fallback.test/archive.iso".into()],
            destination: Some("/fallback".into()),
            file_conflict: FileConflictPolicy::default(),
            options: Vec::new(),
        };

        let new_gid = DownloadEngineGateway::retry_download(&client, Gid::from_u64(7), &fallback)
            .await
            .expect("retry succeeds");

        assert_eq!(new_gid, Gid::from_u64(9));
        let calls = client
            .transport()
            .calls
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        assert_eq!(
            calls.iter().map(|call| call.0.as_str()).collect::<Vec<_>>(),
            vec!["aria2.getOption", "aria2.getUris", "aria2.addUri"]
        );
        assert_eq!(
            calls[2].1[0],
            json!([
                "https://example.test/archive.iso",
                "https://mirror.test/archive.iso"
            ])
        );
        let replayed = calls[2].1[1].as_object().expect("option object");
        for key in [
            "dir",
            "out",
            "header",
            "load-cookies",
            "all-proxy",
            "max-download-limit",
            "max-connection-per-server",
            "checksum",
            "select-file",
        ] {
            assert!(replayed.contains_key(key), "missing replayed option {key}");
        }
        assert!(!replayed.contains_key("gid"));
        assert!(!replayed.contains_key("pause"));
    }

    #[tokio::test]
    async fn retry_maps_only_the_add_uri_phase_to_unknown_outcome() {
        let query_failure = ScriptedTransport::new([Err(RpcError::Timeout {
            method: "aria2.getOption".into(),
        })]);
        let query_client = Aria2Client::new(query_failure);
        let fallback = AddDownloadRequest {
            uris: vec!["https://example.test/archive.iso".into()],
            destination: None,
            file_conflict: FileConflictPolicy::default(),
            options: Vec::new(),
        };
        let query_error =
            DownloadEngineGateway::retry_download(&query_client, Gid::from_u64(7), &fallback)
                .await
                .expect_err("query timeout must fail");
        assert_eq!(query_error.kind, GatewayErrorKind::Timeout);
        assert!(query_error.retryable);

        let mutation_failure = ScriptedTransport::new([
            Ok(json!({"dir": "/downloads"})),
            Err(RpcError::Remote {
                code: 1,
                message: "No URI data is available for GID#0000000000000007".into(),
                data: None,
            }),
            Err(RpcError::Timeout {
                method: "aria2.addUri".into(),
            }),
        ]);
        let mutation_client = Aria2Client::new(mutation_failure);
        let mutation_error =
            DownloadEngineGateway::retry_download(&mutation_client, Gid::from_u64(7), &fallback)
                .await
                .expect_err("addUri timeout must be unknown");
        assert_eq!(mutation_error.kind, GatewayErrorKind::OutcomeUnknown);
        assert!(!mutation_error.retryable);
        assert_eq!(
            mutation_client
                .transport()
                .calls
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .len(),
            3
        );
    }

    #[tokio::test]
    async fn change_options_sends_output_name_to_the_exact_gid() {
        let transport = ScriptedTransport::new([Ok(json!("OK"))]);
        let client = Aria2Client::new(transport);
        let gid = Gid::from_u64(10);

        client
            .change_options(gid, &[("out".into(), "renamed.iso".into())])
            .await
            .expect("changeOption succeeds");

        let calls = client
            .transport()
            .calls
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        assert_eq!(calls[0].0, "aria2.changeOption");
        assert_eq!(calls[0].1[0], Value::String(gid.to_string()));
        assert_eq!(calls[0].1[1]["out"], Value::String("renamed.iso".into()));
    }

    #[tokio::test]
    async fn download_proxy_uses_global_options_and_clears_unspecified_values() {
        let transport = ScriptedTransport::new([Ok(json!("OK"))]);
        let client = Aria2Client::new(transport);
        let config = DownloadProxyConfig {
            mode: DownloadProxyMode::Manual,
            all_proxy: Some("http://proxy.example:8080".into()),
            https_proxy: Some("secure-proxy.example:8443".into()),
            no_proxy: vec!["localhost".into(), "10.0.0.0/8".into()],
            username: Some("proxy-user".into()),
            password: Some(secrecy::SecretString::new("secret-value".into())),
            ..DownloadProxyConfig::default()
        };

        DownloadEngineGateway::apply_download_proxy(&client, &config)
            .await
            .expect("changeGlobalOption succeeds");

        let calls = client
            .transport()
            .calls
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        assert_eq!(calls[0].0, "aria2.changeGlobalOption");
        let options = calls[0].1[0].as_object().expect("global option object");
        assert_eq!(options["all-proxy"], "http://proxy.example:8080");
        assert_eq!(options["http-proxy"], "");
        assert_eq!(options["https-proxy"], "secure-proxy.example:8443");
        assert_eq!(options["no-proxy"], "localhost,10.0.0.0/8");
        assert_eq!(options["all-proxy-user"], "proxy-user");
        assert_eq!(options["all-proxy-passwd"], "secret-value");
        assert_eq!(options["ftp-proxy-passwd"], "secret-value");
    }

    #[tokio::test]
    async fn disabled_download_proxy_explicitly_clears_endpoints_and_credentials() {
        let transport = ScriptedTransport::new([Ok(json!("OK"))]);
        let client = Aria2Client::new(transport);
        let config = DownloadProxyConfig {
            mode: DownloadProxyMode::Disabled,
            all_proxy: Some("http://stale.example:8080".into()),
            username: Some("stale-user".into()),
            password: Some(secrecy::SecretString::new("stale-secret".into())),
            ..DownloadProxyConfig::default()
        };

        DownloadEngineGateway::apply_download_proxy(&client, &config)
            .await
            .expect("disabled proxy is applied");

        let calls = client
            .transport()
            .calls
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let options = calls[0].1[0].as_object().expect("global option object");
        for key in [
            "all-proxy",
            "http-proxy",
            "https-proxy",
            "ftp-proxy",
            "no-proxy",
            "all-proxy-user",
            "all-proxy-passwd",
            "http-proxy-user",
            "http-proxy-passwd",
            "https-proxy-user",
            "https-proxy-passwd",
            "ftp-proxy-user",
            "ftp-proxy-passwd",
        ] {
            assert_eq!(options[key], "", "{key} was not cleared");
        }
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
