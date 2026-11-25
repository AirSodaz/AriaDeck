//! Shared test doubles for desktop workspace unit tests.

use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use ariadeck_application::{
    AddDownloadRequest, ConnectedSyncSession, CoordinatorConfig, DownloadEngineGateway,
    DownloadProxyConfig, DownloadSyncConnector, DownloadSyncSession, EngineCapabilities,
    GatewayError, GatewayErrorKind, InitialSyncSnapshot, LiveSyncSnapshot, RefreshHint,
    StoppedPage, SyncError, SyncHandle, TaskConnectionDetailsGateway, TaskDetailsGateway,
    TaskListQuery, TaskRemovalTarget, spawn_sync_coordinator,
};
use ariadeck_domain::{
    ConnectionState, Gid, GlobalStat, ProfileId, SpeedLimitConfig, TaskConnectionDetails,
    TaskDetails, TaskSnapshot, TransferPolicyConfig,
};
use ariadeck_settings::{ProxyCredentialError, ProxyCredentialRef, ProxyCredentialStore};
use async_trait::async_trait;
use secrecy::{ExposeSecret as _, SecretString};
use tokio::sync::mpsc;

#[derive(Default)]
pub(crate) struct MemoryProxyCredentialStore {
    entries: Mutex<HashMap<ProxyCredentialRef, String>>,
}

impl ProxyCredentialStore for MemoryProxyCredentialStore {
    fn load(
        &self,
        credential: ProxyCredentialRef,
    ) -> Result<Option<SecretString>, ProxyCredentialError> {
        Ok(self
            .entries
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(&credential)
            .cloned()
            .map(SecretString::from))
    }

    fn save(
        &self,
        credential: ProxyCredentialRef,
        password: &SecretString,
    ) -> Result<(), ProxyCredentialError> {
        self.entries
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .insert(credential, password.expose_secret().to_owned());
        Ok(())
    }

    fn delete(&self, credential: ProxyCredentialRef) -> Result<(), ProxyCredentialError> {
        self.entries
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .remove(&credential);
        Ok(())
    }
}

#[derive(Default)]
pub(crate) struct RecordingGateway {
    pub(crate) fail_on: Mutex<Option<&'static str>>,
    pub(crate) calls: Mutex<Vec<String>>,
}

#[async_trait]
impl DownloadEngineGateway for RecordingGateway {
    async fn add_download(&self, _: &AddDownloadRequest) -> Result<Vec<Gid>, GatewayError> {
        unimplemented!()
    }
    async fn retry_download(&self, _: Gid, _: &AddDownloadRequest) -> Result<Gid, GatewayError> {
        unimplemented!()
    }
    async fn pause(&self, _: Gid) -> Result<(), GatewayError> {
        unimplemented!()
    }
    async fn resume(&self, _: Gid) -> Result<(), GatewayError> {
        unimplemented!()
    }
    async fn change_options(&self, _: Gid, _: &[(String, String)]) -> Result<(), GatewayError> {
        unimplemented!()
    }
    async fn remove(&self, _: Gid, _: TaskRemovalTarget) -> Result<(), GatewayError> {
        unimplemented!()
    }
    async fn apply_download_proxy(&self, cfg: &DownloadProxyConfig) -> Result<(), GatewayError> {
        self.calls
            .lock()
            .expect("lock")
            .push(format!("proxy:{:?}", cfg.mode));
        if *self.fail_on.lock().expect("lock") == Some("proxy") {
            return Err(GatewayError::new(
                GatewayErrorKind::Rejected,
                "proxy boom",
                false,
            ));
        }
        Ok(())
    }
    async fn apply_speed_limit(&self, cfg: &SpeedLimitConfig) -> Result<(), GatewayError> {
        self.calls
            .lock()
            .expect("lock")
            .push(format!("speed:{}", cfg.download_limit.get()));
        if *self.fail_on.lock().expect("lock") == Some("speed") {
            return Err(GatewayError::new(
                GatewayErrorKind::Rejected,
                "speed boom",
                false,
            ));
        }
        Ok(())
    }
    async fn apply_transfer_policy(&self, cfg: &TransferPolicyConfig) -> Result<(), GatewayError> {
        self.calls
            .lock()
            .expect("lock")
            .push(format!("policy:{}", cfg.max_concurrent_downloads));
        if *self.fail_on.lock().expect("lock") == Some("policy") {
            return Err(GatewayError::new(
                GatewayErrorKind::Rejected,
                "policy boom",
                false,
            ));
        }
        Ok(())
    }
    async fn apply_bt_tracker(&self, trackers: &[String]) -> Result<(), GatewayError> {
        self.calls
            .lock()
            .expect("lock")
            .push(format!("tracker:{}", trackers.len()));
        if *self.fail_on.lock().expect("lock") == Some("tracker") {
            return Err(GatewayError::new(
                GatewayErrorKind::Rejected,
                "tracker boom",
                false,
            ));
        }
        Ok(())
    }
}

#[async_trait]
impl TaskDetailsGateway for RecordingGateway {
    async fn task_details(&self, _: Gid) -> Result<TaskDetails, GatewayError> {
        unimplemented!()
    }
}

#[async_trait]
impl TaskConnectionDetailsGateway for RecordingGateway {
    async fn connection_details(
        &self,
        _: Gid,
        _: bool,
        _: bool,
    ) -> Result<TaskConnectionDetails, GatewayError> {
        unimplemented!()
    }
}

pub(crate) struct StubSession;

#[async_trait]
impl DownloadSyncSession for StubSession {
    async fn initial_snapshot(&self, _: u32) -> Result<InitialSyncSnapshot, SyncError> {
        Ok(InitialSyncSnapshot {
            capabilities: EngineCapabilities {
                version: "1.37.0".into(),
                ..EngineCapabilities::default()
            },
            global_stat: GlobalStat::default(),
            live: LiveSyncSnapshot {
                active: Vec::new(),
                waiting: Vec::new(),
            },
            stopped: StoppedPage {
                offset: 0,
                total: 0,
                tasks: Vec::new(),
            },
        })
    }
    async fn refresh_global_stat(&self) -> Result<GlobalStat, SyncError> {
        Ok(GlobalStat::default())
    }
    async fn refresh_live(&self) -> Result<LiveSyncSnapshot, SyncError> {
        Ok(LiveSyncSnapshot {
            active: Vec::new(),
            waiting: Vec::new(),
        })
    }
    async fn refresh_stopped_page(&self, _: usize, _: u32) -> Result<StoppedPage, SyncError> {
        Ok(StoppedPage {
            offset: 0,
            total: 0,
            tasks: Vec::new(),
        })
    }
    async fn refresh_tasks(
        &self,
        _: &[Gid],
    ) -> Result<Vec<(Gid, Option<TaskSnapshot>)>, SyncError> {
        Ok(Vec::new())
    }
    async fn close(&self) {}
}

pub(crate) struct StubConnector {
    pub(crate) gateway: Arc<RecordingGateway>,
    pub(crate) keepalive_tx: Mutex<Vec<mpsc::Sender<RefreshHint>>>,
}

#[async_trait]
impl DownloadSyncConnector for StubConnector {
    async fn connect(&self) -> Result<ConnectedSyncSession, SyncError> {
        let (tx, rx) = mpsc::channel::<RefreshHint>(4);
        self.keepalive_tx.lock().expect("lock").push(tx);
        Ok(ConnectedSyncSession::new_with_gateways(
            Box::new(StubSession),
            self.gateway.clone(),
            self.gateway.clone(),
            self.gateway.clone(),
            rx,
        ))
    }
}

pub(crate) async fn connected_sync(
    gateway: Arc<RecordingGateway>,
    profile_id: ProfileId,
) -> SyncHandle {
    let handle = spawn_sync_coordinator(
        Arc::new(StubConnector {
            gateway,
            keepalive_tx: Mutex::new(Vec::new()),
        }),
        CoordinatorConfig::new(profile_id),
    );
    for _ in 0..40 {
        if let Some(snap) = handle.snapshot(TaskListQuery::default()).await
            && matches!(snap.connection_state, ConnectionState::Connected)
        {
            return handle;
        }
        tokio::time::sleep(std::time::Duration::from_millis(15)).await;
    }
    panic!("stub sync never reached Connected");
}
