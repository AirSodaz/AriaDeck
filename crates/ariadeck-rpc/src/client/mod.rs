mod decode;
mod gateway;
mod options;

#[cfg(test)]
mod tests;

use ariadeck_application::{AddDownloadRequest, AddDownloadSource, QueueMove};
use ariadeck_domain::{Gid, GlobalStat, TaskConnectionDetails, TaskDetails, TaskSnapshot};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde_json::{Map, Value, json};

use crate::{
    RpcError, RpcTransport,
    models::{GlobalStatWire, TaskKey, TaskWire, UriWire, VersionInfo, VersionWire},
};

use self::{
    decode::{
        decode, decode_detail_uris, decode_multicall_entry, decode_option_entries, decode_peers,
        decode_servers, decode_tasks, decode_validated_option_map, is_gid_not_found,
        is_no_uri_data, next_batch_result, next_multicall_result, task_keys,
    },
    options::request_options,
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

pub(super) enum RetryRpcError {
    Query(RpcError),
    Mutation(RpcError),
}

enum ActiveProjectionKind {
    Peers,
    Servers,
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

    pub(super) async fn retry_uri(
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
        decode_validated_option_map(METHOD, value)
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
        self.call_ok("aria2.shutdown", Vec::new()).await
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
