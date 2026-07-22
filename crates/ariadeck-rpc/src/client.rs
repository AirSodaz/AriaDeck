use ariadeck_application::{
    AddDownloadRequest, AddDownloadSource, DownloadEngineGateway, DownloadProxyConfig,
    DownloadProxyMode, FileConflictPolicy, GatewayError, GatewayErrorKind, QueueMove,
    TaskConnectionDetailsGateway, TaskDetailsGateway, TaskRemovalTarget,
};
use ariadeck_domain::{
    Gid, GlobalStat, SpeedLimitConfig, TaskConnectionDetails, TaskDetails, TaskOptionEntry,
    TaskSnapshot, TransferPolicyConfig,
};
use async_trait::async_trait;
use base64::{Engine as _, engine::general_purpose::STANDARD};
use secrecy::ExposeSecret;
use serde::{Deserialize, de::DeserializeOwned};
use serde_json::{Map, Value, json};

use crate::{
    RpcError, RpcTransport,
    models::{
        DetailUriWire, GlobalStatWire, PeerWire, ServerGroupWire, TaskKey, TaskWire, VersionInfo,
        VersionWire,
    },
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
        let AddDownloadSource::Uris(uris) = &request.source else {
            return Err(RpcError::Configuration(
                "aria2.addUri requires URI sources".into(),
            ));
        };
        self.add_uri_with_options(uris, request_options(request))
            .await
    }

    pub async fn add_torrent(&self, request: &AddDownloadRequest) -> Result<Gid, RpcError> {
        let AddDownloadSource::Torrent(content) = &request.source else {
            return Err(RpcError::Configuration(
                "aria2.addTorrent requires Torrent metadata".into(),
            ));
        };
        self.call_gid(
            "aria2.addTorrent",
            vec![
                json!(STANDARD.encode(content)),
                json!([]),
                Value::Object(request_options(request)),
            ],
        )
        .await
    }

    pub async fn add_metalink(&self, request: &AddDownloadRequest) -> Result<Vec<Gid>, RpcError> {
        const METHOD: &str = "aria2.addMetalink";
        let AddDownloadSource::Metalink(content) = &request.source else {
            return Err(RpcError::Configuration(
                "aria2.addMetalink requires Metalink metadata".into(),
            ));
        };
        let value = self
            .transport
            .call(
                METHOD,
                vec![
                    json!(STANDARD.encode(content)),
                    Value::Object(request_options(request)),
                ],
            )
            .await?;
        let encoded = decode::<Vec<String>>(METHOD, value)?;
        if encoded.is_empty() {
            return Err(RpcError::InvalidData {
                method: METHOD.into(),
                field: "result".into(),
                message: "expected at least one GID".into(),
            });
        }
        encoded
            .into_iter()
            .enumerate()
            .map(|(index, gid)| {
                gid.parse::<Gid>().map_err(|error| RpcError::InvalidData {
                    method: METHOD.into(),
                    field: format!("result[{index}]"),
                    message: error.to_string(),
                })
            })
            .collect()
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
            match &fallback.source {
                AddDownloadSource::Uris(uris) => uris.clone(),
                AddDownloadSource::Torrent(_) | AddDownloadSource::Metalink(_) => Vec::new(),
            }
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

    /// Force-pause skips graceful peer/server teardown. Prefer ordinary pause;
    /// use force only when a hung active transfer will not leave Active.
    pub async fn force_pause(&self, gid: Gid) -> Result<Gid, RpcError> {
        self.call_gid("aria2.forcePause", vec![json!(gid.to_string())])
            .await
    }

    pub async fn resume(&self, gid: Gid) -> Result<Gid, RpcError> {
        self.call_gid("aria2.unpause", vec![json!(gid.to_string())])
            .await
    }

    pub async fn pause_all(&self) -> Result<(), RpcError> {
        self.call_ok("aria2.pauseAll", Vec::new()).await
    }

    pub async fn force_pause_all(&self) -> Result<(), RpcError> {
        self.call_ok("aria2.forcePauseAll", Vec::new()).await
    }

    pub async fn resume_all(&self) -> Result<(), RpcError> {
        self.call_ok("aria2.unpauseAll", Vec::new()).await
    }

    pub async fn move_in_queue(&self, gid: Gid, movement: QueueMove) -> Result<u32, RpcError> {
        const METHOD: &str = "aria2.changePosition";
        let (position, how) = match movement {
            QueueMove::Top => (0, "POS_SET"),
            QueueMove::Up => (-1, "POS_CUR"),
            QueueMove::Down => (1, "POS_CUR"),
            QueueMove::Bottom => (0, "POS_END"),
        };
        let value = self
            .transport
            .call(
                METHOD,
                vec![json!(gid.to_string()), json!(position), json!(how)],
            )
            .await?;
        let position = decode::<i64>(METHOD, value)?;
        u32::try_from(position).map_err(|_| RpcError::InvalidData {
            method: METHOD.into(),
            field: "response".into(),
            message: "expected a non-negative queue position".into(),
        })
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

    /// Force-remove skips graceful peer/server teardown for a live download.
    /// The stopped result still remains until `removeDownloadResult`.
    pub async fn force_remove(&self, gid: Gid) -> Result<Gid, RpcError> {
        self.call_gid("aria2.forceRemove", vec![json!(gid.to_string())])
            .await
    }

    pub async fn remove_download_result(&self, gid: Gid) -> Result<(), RpcError> {
        self.call_ok("aria2.removeDownloadResult", vec![json!(gid.to_string())])
            .await
    }

    pub async fn purge_download_result(&self) -> Result<(), RpcError> {
        self.call_ok("aria2.purgeDownloadResult", Vec::new()).await
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

    /// On-demand connection/source projections for the details drawer.
    ///
    /// URIs and read-only options are always fetched. Peers (BitTorrent) and
    /// servers (HTTP(S)/FTP) exist only while a task is active, so those calls
    /// run only for `active` tasks and only for the matching source kind. A
    /// projection that aria2 cannot serve (for example peers on a non-torrent)
    /// is treated as empty rather than an error.
    ///
    /// When more than one projection is needed the adapter uses
    /// `system.multicall` so a single RPC round-trip returns every independent
    /// result, including per-item remote failures that must stay empty rather
    /// than fail the whole drawer.
    pub async fn connection_details(
        &self,
        gid: Gid,
        active: bool,
        is_bittorrent: bool,
    ) -> Result<TaskConnectionDetails, RpcError> {
        let mut details = TaskConnectionDetails::new(gid);
        let gid_param = json!(gid.to_string());
        let mut calls = vec![
            crate::RpcCall::new("aria2.getUris", vec![gid_param.clone()]),
            crate::RpcCall::new("aria2.getOption", vec![gid_param.clone()]),
        ];
        let active_kind = if active {
            if is_bittorrent {
                calls.push(crate::RpcCall::new("aria2.getPeers", vec![gid_param]));
                Some(ActiveProjectionKind::Peers)
            } else {
                calls.push(crate::RpcCall::new("aria2.getServers", vec![gid_param]));
                Some(ActiveProjectionKind::Servers)
            }
        } else {
            None
        };

        let results = self.multicall(calls).await?;
        let mut results = results.into_iter();
        details.uris = decode_detail_uris(next_multicall_result(&mut results, "aria2.getUris")?)?;
        details.options =
            decode_option_entries(next_multicall_result(&mut results, "aria2.getOption")?)?;
        if let Some(kind) = active_kind {
            let method = match kind {
                ActiveProjectionKind::Peers => "aria2.getPeers",
                ActiveProjectionKind::Servers => "aria2.getServers",
            };
            match next_multicall_result(&mut results, method) {
                Ok(value) => match kind {
                    ActiveProjectionKind::Peers => {
                        details.peers = decode_peers(value)?;
                    }
                    ActiveProjectionKind::Servers => {
                        details.servers = decode_servers(value)?;
                    }
                },
                Err(RpcError::Remote { .. }) => {}
                Err(error) => return Err(error),
            }
        }
        Ok(details)
    }

    /// Execute independent methods through aria2's `system.multicall`.
    ///
    /// Unlike a JSON-RPC request array, multicall returns one envelope whose
    /// result array can mix successful values and per-call remote errors. Use
    /// this for read projections that must stay available when one optional
    /// method fails (for example peers while a task leaves Active).
    pub async fn multicall(
        &self,
        calls: Vec<crate::RpcCall>,
    ) -> Result<Vec<Result<Value, RpcError>>, RpcError> {
        if calls.is_empty() {
            return Ok(Vec::new());
        }
        const METHOD: &str = "system.multicall";
        let methods = calls
            .iter()
            .map(|call| {
                json!({
                    "methodName": call.method,
                    "params": call.params,
                })
            })
            .collect::<Vec<_>>();
        let value = self
            .transport
            .call(METHOD, vec![Value::Array(methods)])
            .await?;
        let results = decode::<Vec<Value>>(METHOD, value)?;
        if results.len() != calls.len() {
            return Err(RpcError::InvalidData {
                method: METHOD.into(),
                field: "result".into(),
                message: format!(
                    "expected {} multicall results, got {}",
                    calls.len(),
                    results.len()
                ),
            });
        }
        Ok(results
            .into_iter()
            .zip(calls.into_iter().map(|call| call.method))
            .map(|(entry, method)| decode_multicall_entry(method, entry))
            .collect())
    }

    /// Discover the method names published by the connected aria2 process.
    pub async fn list_methods(&self) -> Result<Vec<String>, RpcError> {
        const METHOD: &str = "system.listMethods";
        let value = self.transport.call(METHOD, Vec::new()).await?;
        let methods = decode::<Vec<String>>(METHOD, value)?;
        if methods.is_empty() {
            return Err(RpcError::InvalidData {
                method: METHOD.into(),
                field: "result".into(),
                message: "expected at least one method name".into(),
            });
        }
        Ok(methods)
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

fn task_option_is_sensitive(key: &str) -> bool {
    ariadeck_domain::task_option_key_is_sensitive(key)
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

fn request_options(request: &AddDownloadRequest) -> Map<String, Value> {
    let mut options = Map::new();
    // aria2 accepts repeated option names for multi-value keys such as
    // `header`. Collapse same-key pairs into a string array so Cookie/Referer
    // and custom headers can travel together (ADD-005).
    for (key, value) in &request.options {
        match options.get_mut(key) {
            Some(Value::String(existing)) => {
                let previous = existing.clone();
                *options.get_mut(key).expect("key exists") =
                    Value::Array(vec![Value::String(previous), Value::String(value.clone())]);
            }
            Some(Value::Array(values)) => values.push(Value::String(value.clone())),
            Some(_) => {
                options.insert(key.clone(), Value::String(value.clone()));
            }
            None => {
                options.insert(key.clone(), Value::String(value.clone()));
            }
        }
    }
    apply_file_conflict_policy(&mut options, request.file_conflict);
    if let Some(destination) = &request.destination {
        options.insert("dir".into(), Value::String(destination.as_str().to_owned()));
    }
    if let Some(indices) = &request.selected_file_indices {
        options.insert(
            "select-file".into(),
            Value::String(format_selected_file_indices(indices)),
        );
    }
    options
}

fn format_selected_file_indices(indices: &[u32]) -> String {
    let mut ranges = Vec::new();
    let Some(&first) = indices.first() else {
        return String::new();
    };
    let mut start = first;
    let mut end = first;
    for &index in &indices[1..] {
        if index == end.saturating_add(1) {
            end = index;
        } else {
            push_file_index_range(&mut ranges, start, end);
            start = index;
            end = index;
        }
    }
    push_file_index_range(&mut ranges, start, end);
    ranges.join(",")
}

fn push_file_index_range(ranges: &mut Vec<String>, start: u32, end: u32) {
    if start == end {
        ranges.push(start.to_string());
    } else {
        ranges.push(format!("{start}-{end}"));
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

fn next_multicall_result(
    results: &mut impl Iterator<Item = Result<Value, RpcError>>,
    method: &str,
) -> Result<Value, RpcError> {
    results.next().ok_or_else(|| {
        RpcError::Protocol(format!("multicall response is missing result for {method}"))
    })?
}

enum ActiveProjectionKind {
    Peers,
    Servers,
}

fn decode_multicall_entry(method: String, entry: Value) -> Result<Value, RpcError> {
    // aria2 wraps a successful multicall item as a one-element array and an
    // error item as an object with code/message, matching the RPC manual.
    if let Value::Array(mut values) = entry {
        if values.len() == 1 {
            return Ok(values.remove(0));
        }
        return Err(RpcError::InvalidData {
            method: "system.multicall".into(),
            field: method,
            message: "expected a one-element success array".into(),
        });
    }
    if let Value::Object(mut object) = entry {
        let code = object
            .remove("code")
            .and_then(|value| value.as_i64())
            .unwrap_or(1);
        let message = object
            .remove("message")
            .and_then(|value| value.as_str().map(str::to_owned))
            .unwrap_or_else(|| format!("{method} failed inside multicall"));
        return Err(RpcError::Remote {
            code,
            message,
            data: object.remove("data"),
        });
    }
    Err(RpcError::InvalidData {
        method: "system.multicall".into(),
        field: method,
        message: "expected a success array or an error object".into(),
    })
}

fn decode_detail_uris(value: Value) -> Result<Vec<ariadeck_domain::TaskUri>, RpcError> {
    const METHOD: &str = "aria2.getUris";
    Ok(decode::<Vec<DetailUriWire>>(METHOD, value)?
        .into_iter()
        .map(DetailUriWire::into_domain)
        .filter(|uri| !uri.uri.is_empty())
        .collect())
}

fn decode_servers(value: Value) -> Result<Vec<ariadeck_domain::TaskServer>, RpcError> {
    const METHOD: &str = "aria2.getServers";
    let groups = decode::<Vec<ServerGroupWire>>(METHOD, value)?;
    let mut servers = Vec::new();
    for group in groups {
        servers.extend(group.into_domain(METHOD)?);
    }
    Ok(servers)
}

fn decode_peers(value: Value) -> Result<Vec<ariadeck_domain::TaskPeer>, RpcError> {
    const METHOD: &str = "aria2.getPeers";
    decode::<Vec<PeerWire>>(METHOD, value)?
        .into_iter()
        .map(|peer| peer.into_domain(METHOD))
        .collect()
}

fn decode_option_entries(value: Value) -> Result<Vec<TaskOptionEntry>, RpcError> {
    const METHOD: &str = "aria2.getOption";
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
    decode_option_map(options)
}

fn decode_option_map(options: Map<String, Value>) -> Result<Vec<TaskOptionEntry>, RpcError> {
    let mut entries = options
        .into_iter()
        .filter_map(|(key, value)| {
            if task_option_is_sensitive(&key) {
                return Some(TaskOptionEntry {
                    key,
                    value: String::new(),
                    redacted: true,
                });
            }
            value.as_str().map(|value| TaskOptionEntry {
                key,
                value: value.to_owned(),
                redacted: false,
            })
        })
        .collect::<Vec<_>>();
    entries.sort_by(|left, right| left.key.cmp(&right.key));
    Ok(entries)
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
    async fn add_download(&self, request: &AddDownloadRequest) -> Result<Vec<Gid>, GatewayError> {
        let result = match &request.source {
            AddDownloadSource::Uris(_) => self.add_uri(request).await.map(|gid| vec![gid]),
            AddDownloadSource::Torrent(_) => self.add_torrent(request).await.map(|gid| vec![gid]),
            AddDownloadSource::Metalink(_) => self.add_metalink(request).await,
        };
        result.map_err(map_mutation_error)
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

    async fn force_pause(&self, gid: Gid) -> Result<(), GatewayError> {
        Aria2Client::force_pause(self, gid)
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

    async fn pause_all(&self) -> Result<(), GatewayError> {
        Aria2Client::pause_all(self)
            .await
            .map_err(map_mutation_error)
    }

    async fn force_pause_all(&self) -> Result<(), GatewayError> {
        Aria2Client::force_pause_all(self)
            .await
            .map_err(map_mutation_error)
    }

    async fn resume_all(&self) -> Result<(), GatewayError> {
        Aria2Client::resume_all(self)
            .await
            .map_err(map_mutation_error)
    }

    async fn move_in_queue(&self, gid: Gid, movement: QueueMove) -> Result<(), GatewayError> {
        Aria2Client::move_in_queue(self, gid, movement)
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

    async fn apply_speed_limit(&self, config: &SpeedLimitConfig) -> Result<(), GatewayError> {
        self.change_global_options(&speed_limit_options(config))
            .await
            .map_err(map_mutation_error)
    }

    async fn apply_transfer_policy(
        &self,
        config: &TransferPolicyConfig,
    ) -> Result<(), GatewayError> {
        self.change_global_options(&transfer_policy_options(config))
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

    async fn force_remove(&self, gid: Gid, target: TaskRemovalTarget) -> Result<(), GatewayError> {
        match target {
            TaskRemovalTarget::LiveTask => match Aria2Client::force_remove(self, gid).await {
                Ok(_) => Ok(()),
                Err(error) if live_task_became_stopped(&error) => {
                    self.remove_download_result(gid).await
                }
                Err(error) => Err(error),
            },
            // A stopped result has no live peers to force-teardown; the ordinary
            // result-removal path is the only supported contract.
            TaskRemovalTarget::DownloadResult => self.remove_download_result(gid).await,
        }
        .map_err(map_mutation_error)
    }
}

fn speed_limit_options(config: &SpeedLimitConfig) -> Vec<(String, String)> {
    vec![
        (
            "max-overall-download-limit".into(),
            config.download_limit.get().to_string(),
        ),
        (
            "max-overall-upload-limit".into(),
            config.upload_limit.get().to_string(),
        ),
    ]
}

fn transfer_policy_options(config: &TransferPolicyConfig) -> Vec<(String, String)> {
    vec![
        (
            "max-concurrent-downloads".into(),
            config.max_concurrent_downloads.to_string(),
        ),
        (
            "max-connection-per-server".into(),
            config.max_connection_per_server.to_string(),
        ),
        ("split".into(), config.split.to_string()),
        ("min-split-size".into(), config.min_split_size.to_string()),
        (
            "file-allocation".into(),
            config.file_allocation.as_aria2().to_owned(),
        ),
        (
            "check-integrity".into(),
            if config.check_integrity {
                "true".into()
            } else {
                "false".into()
            },
        ),
    ]
}

fn download_proxy_options(config: &DownloadProxyConfig) -> Vec<(String, String)> {
    // Manual and System both carry concrete endpoints (System is resolved by
    // the desktop layer before this call). Disabled clears every field.
    let active = matches!(
        config.mode,
        DownloadProxyMode::Manual | DownloadProxyMode::System
    );
    let endpoint = |value: &Option<String>| {
        if active {
            value.clone().unwrap_or_default()
        } else {
            String::new()
        }
    };
    let username = active
        .then(|| config.username.clone())
        .flatten()
        .unwrap_or_default();
    let password = if active {
        config
            .password
            .as_ref()
            .map(ExposeSecret::expose_secret)
            .cloned()
            .unwrap_or_default()
    } else {
        String::new()
    };
    let no_proxy = if active {
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
        // Always push: independent of proxy mode (TLS policy for all HTTPS peers).
        (
            "check-certificate".into(),
            if config.check_certificate {
                "true".into()
            } else {
                "false".into()
            },
        ),
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

#[async_trait]
impl<T> TaskConnectionDetailsGateway for Aria2Client<T>
where
    T: RpcTransport,
{
    async fn connection_details(
        &self,
        gid: Gid,
        active: bool,
        is_bittorrent: bool,
    ) -> Result<TaskConnectionDetails, GatewayError> {
        Aria2Client::connection_details(self, gid, active, is_bittorrent)
            .await
            .map_err(map_query_error)
    }
}

fn map_query_error(error: RpcError) -> GatewayError {
    let (kind, retryable) = match &error {
        RpcError::Closed | RpcError::Transport(_) => (GatewayErrorKind::Disconnected, true),
        RpcError::Tls(_) => (GatewayErrorKind::Disconnected, false),
        RpcError::Authentication(_) => (GatewayErrorKind::Authentication, false),
        RpcError::Timeout { .. } => (GatewayErrorKind::Timeout, true),
        RpcError::Remote { message, .. }
            if message.to_ascii_lowercase().contains("unauthorized") =>
        {
            (GatewayErrorKind::Authentication, false)
        }
        RpcError::Remote { .. } => (GatewayErrorKind::Rejected, false),
        RpcError::Configuration(_)
        | RpcError::Protocol(_)
        | RpcError::Serialization(_)
        | RpcError::InvalidData { .. } => (GatewayErrorKind::Internal, false),
    };
    GatewayError::new(kind, error.to_string(), retryable)
}

fn map_mutation_error(error: RpcError) -> GatewayError {
    let kind = match &error {
        RpcError::Closed
        | RpcError::Transport(_)
        | RpcError::Tls(_)
        | RpcError::Timeout { .. }
        | RpcError::Protocol(_)
        | RpcError::InvalidData { .. } => GatewayErrorKind::OutcomeUnknown,
        RpcError::Authentication(_) => GatewayErrorKind::Authentication,
        RpcError::Remote { message, .. }
            if message.to_ascii_lowercase().contains("unauthorized") =>
        {
            GatewayErrorKind::Authentication
        }
        RpcError::Remote { .. } => GatewayErrorKind::Rejected,
        RpcError::Configuration(_) | RpcError::Serialization(_) => GatewayErrorKind::Internal,
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
    use std::{
        collections::VecDeque,
        sync::{Arc, Mutex},
    };

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
    async fn add_uri_collapses_repeated_headers_into_an_array() {
        let transport = ScriptedTransport::new([Ok(json!("000000000000000a"))]);
        let client = Aria2Client::new(transport);
        let request = AddDownloadRequest {
            source: AddDownloadSource::Uris(vec!["https://example.test/file".into()]),
            destination: None,
            file_conflict: FileConflictPolicy::Reject,
            selected_file_indices: None,
            advanced: Default::default(),
            options: vec![
                ("header".into(), "X-Token: one".into()),
                ("header".into(), "Cookie: session=secret".into()),
                ("referer".into(), "https://example.test/ref".into()),
            ],
        };

        client.add_uri(&request).await.expect("addUri succeeds");
        let calls = client
            .transport()
            .calls
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        assert_eq!(calls[0].0, "aria2.addUri");
        assert_eq!(
            calls[0].1[1]["header"],
            json!(["X-Token: one", "Cookie: session=secret"])
        );
        assert_eq!(calls[0].1[1]["referer"], json!("https://example.test/ref"));
    }

    #[tokio::test]
    async fn add_uri_builds_options_without_losing_destination() {
        let transport = ScriptedTransport::new([Ok(json!("0000000000000009"))]);
        let client = Aria2Client::new(transport);
        let request = AddDownloadRequest {
            source: AddDownloadSource::Uris(vec![
                "https://example.test/file".into(),
                "https://mirror.test/file".into(),
            ]),
            destination: Some("D:/Downloads".into()),
            file_conflict: FileConflictPolicy::AutoRename,
            selected_file_indices: None,
            advanced: Default::default(),
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

    #[tokio::test]
    async fn add_torrent_uploads_base64_content_and_options() {
        let transport = ScriptedTransport::new([Ok(json!("0000000000000010"))]);
        let client = Aria2Client::new(transport);
        let request = AddDownloadRequest {
            source: AddDownloadSource::Torrent(Arc::<[u8]>::from(&b"torrent-bytes"[..])),
            destination: Some("/downloads".into()),
            file_conflict: FileConflictPolicy::Reject,
            selected_file_indices: Some(vec![1, 2, 3, 5]),
            advanced: Default::default(),
            options: vec![("max-connection-per-server".into(), "4".into())],
        };

        let gid = client
            .add_torrent(&request)
            .await
            .expect("torrent upload succeeds");

        assert_eq!(gid, Gid::from_u64(16));
        let calls = client
            .transport()
            .calls
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        assert_eq!(calls[0].0, "aria2.addTorrent");
        assert_eq!(calls[0].1[0], json!(STANDARD.encode(b"torrent-bytes")));
        assert_eq!(calls[0].1[1], json!([]));
        assert_eq!(calls[0].1[2]["dir"], json!("/downloads"));
        assert_eq!(calls[0].1[2]["allow-overwrite"], json!("false"));
        assert_eq!(calls[0].1[2]["auto-file-renaming"], json!("false"));
        assert_eq!(calls[0].1[2]["max-connection-per-server"], json!("4"));
        assert_eq!(calls[0].1[2]["select-file"], json!("1-3,5"));
    }

    #[tokio::test]
    async fn add_metalink_preserves_all_returned_gids() {
        let transport =
            ScriptedTransport::new([Ok(json!(["0000000000000011", "0000000000000012"]))]);
        let client = Aria2Client::new(transport);
        let request = AddDownloadRequest {
            source: AddDownloadSource::Metalink(Arc::<[u8]>::from(&b"<metalink />"[..])),
            destination: None,
            file_conflict: FileConflictPolicy::Reject,
            selected_file_indices: None,
            advanced: Default::default(),
            options: Vec::new(),
        };

        let gids = client
            .add_metalink(&request)
            .await
            .expect("metalink upload succeeds");

        assert_eq!(gids, vec![Gid::from_u64(17), Gid::from_u64(18)]);
        let calls = client
            .transport()
            .calls
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        assert_eq!(calls[0].0, "aria2.addMetalink");
        assert_eq!(calls[0].1[0], json!(STANDARD.encode(b"<metalink />")));
    }

    #[tokio::test]
    async fn add_metalink_rejects_empty_or_malformed_gid_results() {
        for result in [json!([]), json!(["not-a-gid"])] {
            let transport = ScriptedTransport::new([Ok(result)]);
            let client = Aria2Client::new(transport);
            let request = AddDownloadRequest {
                source: AddDownloadSource::Metalink(Arc::<[u8]>::from(&b"metadata"[..])),
                destination: None,
                file_conflict: FileConflictPolicy::Reject,
                selected_file_indices: None,
                advanced: Default::default(),
                options: Vec::new(),
            };

            assert!(client.add_metalink(&request).await.is_err());
        }
    }

    #[tokio::test]
    async fn change_position_maps_each_queue_move_to_the_authoritative_aria2_arguments() {
        // aria2.changePosition takes (gid, pos, how); zero-based POS_SET/POS_END
        // and relative POS_CUR (D-014 evidence).
        for (movement, expected_pos, expected_how) in [
            (QueueMove::Top, json!(0), "POS_SET"),
            (QueueMove::Up, json!(-1), "POS_CUR"),
            (QueueMove::Down, json!(1), "POS_CUR"),
            (QueueMove::Bottom, json!(0), "POS_END"),
        ] {
            let transport = ScriptedTransport::new([Ok(json!(0))]);
            let client = Aria2Client::new(transport);

            let position = client
                .move_in_queue(Gid::from_u64(7), movement)
                .await
                .expect("changePosition must accept a queue move");
            assert_eq!(position, 0);

            let calls = client
                .transport()
                .calls
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            assert_eq!(calls.len(), 1);
            let (method, params) = &calls[0];
            assert_eq!(method, "aria2.changePosition");
            assert_eq!(
                params.as_slice(),
                &[json!("0000000000000007"), expected_pos, json!(expected_how),]
            );
        }
    }

    #[tokio::test]
    async fn change_position_rejects_a_negative_result_position() {
        let transport = ScriptedTransport::new([Ok(json!(-1))]);
        let client = Aria2Client::new(transport);

        assert!(
            client
                .move_in_queue(Gid::from_u64(1), QueueMove::Top)
                .await
                .is_err(),
            "aria2 must report a non-negative queue position"
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
            source: AddDownloadSource::Uris(vec!["https://fallback.test/archive.iso".into()]),
            destination: Some("/fallback".into()),
            file_conflict: FileConflictPolicy::default(),
            selected_file_indices: None,
            advanced: Default::default(),
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
            source: AddDownloadSource::Uris(vec!["https://example.test/archive.iso".into()]),
            destination: None,
            file_conflict: FileConflictPolicy::default(),
            selected_file_indices: None,
            advanced: Default::default(),
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
    async fn transfer_policy_uses_global_options_with_aria2_names() {
        let transport = ScriptedTransport::new([Ok(json!("OK"))]);
        let client = Aria2Client::new(transport);
        let config = TransferPolicyConfig {
            max_concurrent_downloads: 3,
            max_connection_per_server: 8,
            split: 16,
            min_split_size: 1024 * 1024,
            file_allocation: ariadeck_domain::FileAllocationMethod::Falloc,
            check_integrity: true,
        };

        DownloadEngineGateway::apply_transfer_policy(&client, &config)
            .await
            .expect("changeGlobalOption succeeds");

        let calls = client
            .transport()
            .calls
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        assert_eq!(calls[0].0, "aria2.changeGlobalOption");
        let options = calls[0].1[0].as_object().expect("global option object");
        assert_eq!(options["max-concurrent-downloads"], "3");
        assert_eq!(options["max-connection-per-server"], "8");
        assert_eq!(options["split"], "16");
        assert_eq!(options["min-split-size"], (1024 * 1024).to_string());
        assert_eq!(options["file-allocation"], "falloc");
        assert_eq!(options["check-integrity"], "true");
    }

    #[tokio::test]
    async fn system_download_proxy_applies_resolved_endpoints_like_manual() {
        let transport = ScriptedTransport::new([Ok(json!("OK"))]);
        let client = Aria2Client::new(transport);
        let config = DownloadProxyConfig {
            mode: DownloadProxyMode::System,
            all_proxy: Some("http://system-proxy.example:3128".into()),
            no_proxy: vec!["localhost".into()],
            ..DownloadProxyConfig::default()
        };

        DownloadEngineGateway::apply_download_proxy(&client, &config)
            .await
            .expect("system proxy is applied");

        let calls = client
            .transport()
            .calls
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let options = calls[0].1[0].as_object().expect("global option object");
        assert_eq!(options["all-proxy"], "http://system-proxy.example:3128");
        assert_eq!(options["no-proxy"], "localhost");
        assert_eq!(options["all-proxy-user"], "");
        assert_eq!(options["all-proxy-passwd"], "");
        assert_eq!(options["check-certificate"], "true");
    }

    #[tokio::test]
    async fn download_proxy_can_disable_certificate_verification() {
        let transport = ScriptedTransport::new([Ok(json!("OK"))]);
        let client = Aria2Client::new(transport);
        let config = DownloadProxyConfig {
            mode: DownloadProxyMode::Manual,
            all_proxy: Some("http://127.0.0.1:7897".into()),
            check_certificate: false,
            ..DownloadProxyConfig::default()
        };

        DownloadEngineGateway::apply_download_proxy(&client, &config)
            .await
            .expect("proxy apply succeeds");

        let calls = client
            .transport()
            .calls
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let options = calls[0].1[0].as_object().expect("global option object");
        assert_eq!(options["all-proxy"], "http://127.0.0.1:7897");
        assert_eq!(options["check-certificate"], "false");
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
            "bittorrent": {
                "announceList": [
                    ["https://tracker-one.example/announce"],
                    ["https://tracker-two.example/announce", ""]
                ]
            },
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
        assert_eq!(details.trackers.len(), 2);
        assert_eq!(details.trackers[0].tier, 1);
        assert_eq!(details.trackers[1].tier, 2);
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
                "numPieces",
                "bittorrent"
            ])
        );
    }

    #[tokio::test]
    async fn connection_details_loads_active_bittorrent_peers_and_redacts_sensitive_options() {
        // Each successful multicall item is a one-element array around the
        // method's ordinary result value (aria2 system.multicall contract).
        let transport = ScriptedTransport::new([Ok(json!([
            [[
                {
                    "uri": "https://mirror-one.example/file",
                    "status": "used"
                },
                {
                    "uri": "https://mirror-two.example/file",
                    "status": "waiting"
                }
            ]],
            [{
                "max-download-limit": "1M",
                "http-passwd": "must-not-leave-the-adapter",
                "header": ["Cookie: session=secret"],
                "all-proxy": "http://user:password@proxy.example:8080"
            }],
            [[{
                "ip": "2001:db8::1",
                "port": "6881",
                "downloadSpeed": "1024",
                "uploadSpeed": "512",
                "seeder": "true"
            }]]
        ]))]);
        let client = Aria2Client::new(transport);

        let details = client
            .connection_details(Gid::from_u64(9), true, true)
            .await
            .expect("connection projections decode");

        assert_eq!(details.uris.len(), 2);
        assert_eq!(details.uris[0].status, ariadeck_domain::TaskUriStatus::Used);
        assert_eq!(details.peers.len(), 1);
        assert!(details.peers[0].seeder);
        assert!(details.servers.is_empty());
        assert_eq!(
            details
                .options
                .iter()
                .find(|entry| entry.key == "max-download-limit")
                .map(|entry| (entry.value.as_str(), entry.redacted)),
            Some(("1M", false))
        );
        for key in ["all-proxy", "header", "http-passwd"] {
            let option = details
                .options
                .iter()
                .find(|entry| entry.key == key)
                .unwrap_or_else(|| panic!("missing {key}"));
            assert!(option.redacted, "{key} must be redacted in the adapter");
            assert!(
                option.value.is_empty(),
                "{key} value must not leave the adapter"
            );
        }
        let calls = client
            .transport()
            .calls
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        assert_eq!(calls[0].0, "system.multicall");
        let methods = calls[0].1[0]
            .as_array()
            .expect("multicall methods array")
            .iter()
            .map(|entry| entry["methodName"].as_str().unwrap_or_default())
            .collect::<Vec<_>>();
        assert_eq!(
            methods,
            vec!["aria2.getUris", "aria2.getOption", "aria2.getPeers"]
        );
    }

    #[tokio::test]
    async fn connection_details_loads_servers_only_for_active_non_bittorrent_tasks() {
        let transport = ScriptedTransport::new([Ok(json!([
            [[]],
            [{"split": "4"}],
            [[{
                "index": "1",
                "servers": [{
                    "uri": "https://origin.example/file",
                    "currentUri": "https://cdn.example/file",
                    "downloadSpeed": "2048"
                }]
            }]]
        ]))]);
        let client = Aria2Client::new(transport);

        let details = client
            .connection_details(Gid::from_u64(9), true, false)
            .await
            .expect("server projection decodes");

        assert_eq!(details.servers.len(), 1);
        assert!(details.peers.is_empty());
        assert_eq!(details.servers[0].file_index, 1);
        assert_eq!(details.servers[0].download_speed.get(), 2_048);
        let calls = client
            .transport()
            .calls
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        assert_eq!(calls[0].0, "system.multicall");
        let methods = calls[0].1[0]
            .as_array()
            .expect("multicall methods array")
            .iter()
            .map(|entry| entry["methodName"].as_str().unwrap_or_default())
            .collect::<Vec<_>>();
        assert_eq!(
            methods,
            vec!["aria2.getUris", "aria2.getOption", "aria2.getServers"]
        );
    }

    #[tokio::test]
    async fn connection_details_skips_active_only_projections_for_inactive_tasks() {
        let transport = ScriptedTransport::new([Ok(json!([[[]], [{}]]))]);
        let client = Aria2Client::new(transport);

        let details = client
            .connection_details(Gid::from_u64(9), false, true)
            .await
            .expect("inactive projection decodes");

        assert!(details.peers.is_empty());
        assert!(details.servers.is_empty());
        let calls = client
            .transport()
            .calls
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        assert_eq!(calls[0].0, "system.multicall");
        let methods = calls[0].1[0]
            .as_array()
            .expect("multicall methods array")
            .iter()
            .map(|entry| entry["methodName"].as_str().unwrap_or_default())
            .collect::<Vec<_>>();
        assert_eq!(methods, vec!["aria2.getUris", "aria2.getOption"]);
    }

    #[tokio::test]
    async fn connection_details_treats_unavailable_active_only_projection_as_empty() {
        let transport = ScriptedTransport::new([Ok(json!([
            [[]],
            [{"split": "4"}],
            {"code": 1, "message": "GID is not found in active downloads"}
        ]))]);
        let client = Aria2Client::new(transport);

        let details = client
            .connection_details(Gid::from_u64(9), true, true)
            .await
            .expect("an active-state race must keep stable projections available");

        assert!(details.peers.is_empty());
        assert_eq!(details.options[0].key, "split");
    }

    #[tokio::test]
    async fn force_pause_and_force_remove_use_the_force_rpc_methods() {
        let transport = ScriptedTransport::new([
            Ok(json!("0000000000000009")),
            Ok(json!("0000000000000009")),
            Ok(json!("OK")),
            Ok(json!("OK")),
        ]);
        let client = Aria2Client::new(transport);

        DownloadEngineGateway::force_pause(&client, Gid::from_u64(9))
            .await
            .expect("force pause");
        DownloadEngineGateway::force_remove(&client, Gid::from_u64(9), TaskRemovalTarget::LiveTask)
            .await
            .expect("force remove");
        DownloadEngineGateway::force_pause_all(&client)
            .await
            .expect("force pause all");
        Aria2Client::purge_download_result(&client)
            .await
            .expect("purge results");

        let calls = client
            .transport()
            .calls
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        assert_eq!(
            calls.iter().map(|call| call.0.as_str()).collect::<Vec<_>>(),
            vec![
                "aria2.forcePause",
                "aria2.forceRemove",
                "aria2.forcePauseAll",
                "aria2.purgeDownloadResult",
            ]
        );
    }

    #[tokio::test]
    async fn list_methods_returns_engine_published_names() {
        let transport = ScriptedTransport::new([Ok(json!([
            "aria2.addUri",
            "aria2.forcePause",
            "system.multicall",
            "system.listMethods"
        ]))]);
        let client = Aria2Client::new(transport);

        let methods = client.list_methods().await.expect("list methods");
        assert!(methods.iter().any(|method| method == "aria2.forcePause"));
        assert!(methods.iter().any(|method| method == "system.multicall"));
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
                "seeder",
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
