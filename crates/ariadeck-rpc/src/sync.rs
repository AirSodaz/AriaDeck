use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use ariadeck_application::{
    ConnectedSyncSession, DownloadSyncConnector, DownloadSyncSession, EngineCapabilities,
    InitialSyncSnapshot, LiveSyncSnapshot, RefreshHint, StoppedPage, SyncError, SyncErrorKind,
};
use ariadeck_domain::{Gid, GlobalStat, TaskMetadata, TaskSnapshot};
use async_trait::async_trait;
use tokio::sync::mpsc;

use crate::{
    Aria2Client, Aria2Notification, Aria2NotificationKind, AuthenticatedTransport, RpcError,
    RpcSecret, TaskKey, WebSocketConfig, WebSocketTransport,
};

#[derive(Clone)]
pub struct RpcSyncConnector {
    config: WebSocketConfig,
    secret: Option<RpcSecret>,
}

impl RpcSyncConnector {
    #[must_use]
    pub const fn new(config: WebSocketConfig, secret: Option<RpcSecret>) -> Self {
        Self { config, secret }
    }
}

#[async_trait]
impl DownloadSyncConnector for RpcSyncConnector {
    async fn connect(&self) -> Result<ConnectedSyncSession, SyncError> {
        let transport = WebSocketTransport::connect(self.config.clone())
            .await
            .map_err(map_sync_error)?;
        let notifications = notification_hints(&transport);
        let authenticated = AuthenticatedTransport::new(transport.clone(), self.secret.clone());
        let client = Arc::new(Aria2Client::new(authenticated));
        Ok(ConnectedSyncSession::new_with_gateways(
            Box::new(RpcSyncSession {
                client: client.clone(),
                transport,
                metadata_cache: Mutex::new(HashMap::new()),
            }),
            client.clone(),
            client.clone(),
            client,
            notifications,
        ))
    }
}

struct RpcSyncSession {
    client: Arc<Aria2Client<AuthenticatedTransport<WebSocketTransport>>>,
    transport: WebSocketTransport,
    metadata_cache: Mutex<HashMap<Gid, (String, TaskMetadata)>>,
}

impl RpcSyncSession {
    fn remember_tasks(&self, tasks: &[TaskSnapshot]) {
        let mut cache = self
            .metadata_cache
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        for task in tasks {
            cache.insert(task.gid, (task.display_name.clone(), task.metadata.clone()));
        }
    }

    async fn enrich_live_tasks(
        &self,
        mut tasks: Vec<TaskSnapshot>,
    ) -> Result<Vec<TaskSnapshot>, SyncError> {
        let mut missing = Vec::new();
        {
            let cache = self
                .metadata_cache
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            for task in &mut tasks {
                if let Some((display_name, metadata)) = cache.get(&task.gid) {
                    let followed_by = std::mem::take(&mut task.metadata.followed_by);
                    let belongs_to = task.metadata.belongs_to;
                    task.display_name.clone_from(display_name);
                    task.metadata.clone_from(metadata);
                    task.metadata.followed_by = followed_by;
                    task.metadata.belongs_to = belongs_to;
                } else {
                    missing.push(task.gid);
                }
            }
        }
        if missing.is_empty() {
            self.remember_tasks(&tasks);
            return Ok(tasks);
        }

        let discovered = self
            .client
            .refresh_tasks(&missing)
            .await
            .map_err(map_sync_error)?;
        let discovered = discovered
            .into_iter()
            .filter_map(|(gid, task)| task.map(|task| (gid, task)))
            .collect::<HashMap<_, _>>();
        self.remember_tasks(&discovered.values().cloned().collect::<Vec<_>>());
        for task in &mut tasks {
            if let Some(discovered) = discovered.get(&task.gid) {
                task.display_name.clone_from(&discovered.display_name);
                task.metadata.clone_from(&discovered.metadata);
            }
        }
        self.remember_tasks(&tasks);
        Ok(tasks)
    }
}

#[async_trait]
impl DownloadSyncSession for RpcSyncSession {
    async fn initial_snapshot(&self, stopped_count: u32) -> Result<InitialSyncSnapshot, SyncError> {
        let snapshot = self
            .client
            .initial_sync_snapshot(stopped_count)
            .await
            .map_err(map_sync_error)?;
        self.remember_tasks(&snapshot.active);
        self.remember_tasks(&snapshot.waiting);
        self.remember_tasks(&snapshot.stopped);
        let total = usize::try_from(snapshot.global_stat.stopped_tasks).unwrap_or(usize::MAX);
        let methods = match self.client.list_methods().await {
            Ok(methods) => methods,
            Err(error) => {
                // listMethods is optional; a remote that omits it still works
                // for every method AriaDeck already uses. Keep methods empty
                // so capability checks treat support as unknown.
                tracing::debug!(error = %error, "system.listMethods unavailable");
                Vec::new()
            }
        };
        Ok(InitialSyncSnapshot {
            capabilities: EngineCapabilities {
                version: snapshot.version.version,
                enabled_features: snapshot.version.enabled_features,
                methods,
            },
            global_stat: snapshot.global_stat,
            live: LiveSyncSnapshot {
                active: snapshot.active,
                waiting: snapshot.waiting,
            },
            stopped: StoppedPage {
                offset: 0,
                total,
                tasks: snapshot.stopped,
            },
        })
    }

    async fn refresh_global_stat(&self) -> Result<GlobalStat, SyncError> {
        self.client.get_global_stat().await.map_err(map_sync_error)
    }

    async fn refresh_live(&self) -> Result<LiveSyncSnapshot, SyncError> {
        let snapshot = self
            .client
            .refresh_live_snapshot()
            .await
            .map_err(map_sync_error)?;
        let active = self.enrich_live_tasks(snapshot.active).await?;
        let waiting = self.enrich_live_tasks(snapshot.waiting).await?;
        Ok(LiveSyncSnapshot { active, waiting })
    }

    async fn refresh_stopped_page(
        &self,
        offset: usize,
        count: u32,
    ) -> Result<StoppedPage, SyncError> {
        let offset_i64 = i64::try_from(offset)
            .map_err(|error| SyncError::new(SyncErrorKind::Internal, error.to_string(), false))?;
        let (global, tasks) = tokio::try_join!(
            self.client.get_global_stat(),
            self.client
                .tell_stopped(offset_i64, count, TaskKey::DISCOVERY_PROJECTION)
        )
        .map_err(map_sync_error)?;
        self.remember_tasks(&tasks);
        Ok(StoppedPage {
            offset,
            total: usize::try_from(global.stopped_tasks).unwrap_or(usize::MAX),
            tasks,
        })
    }

    async fn refresh_tasks(
        &self,
        gids: &[Gid],
    ) -> Result<Vec<(Gid, Option<TaskSnapshot>)>, SyncError> {
        let tasks = self
            .client
            .refresh_tasks(gids)
            .await
            .map_err(map_sync_error)?;
        let discovered = tasks
            .iter()
            .filter_map(|(_, task)| task.as_ref().cloned())
            .collect::<Vec<_>>();
        self.remember_tasks(&discovered);
        Ok(tasks)
    }

    async fn close(&self) {
        self.transport.close().await;
    }
}

fn notification_hints(transport: &WebSocketTransport) -> mpsc::Receiver<RefreshHint> {
    let notifications = transport.subscribe_notifications();
    let closed = transport.subscribe_closed();
    let (sender, receiver) = mpsc::channel(128);
    tokio::spawn(forward_notification_hints(notifications, closed, sender));
    receiver
}

async fn forward_notification_hints(
    mut notifications: tokio::sync::broadcast::Receiver<Aria2Notification>,
    mut closed: tokio::sync::watch::Receiver<Option<RpcError>>,
    sender: mpsc::Sender<RefreshHint>,
) {
    loop {
        if closed.borrow().is_some() {
            break;
        }
        tokio::select! {
            notification = notifications.recv() => {
                let hint = match notification {
                    Ok(notification) => notification_hint(notification),
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => RefreshHint::Full,
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                };
                if sender.send(hint).await.is_err() {
                    break;
                }
            }
            changed = closed.changed() => {
                if changed.is_err() || closed.borrow().is_some() {
                    break;
                }
            }
        }
    }
}

fn notification_hint(notification: Aria2Notification) -> RefreshHint {
    match notification.kind {
        Aria2NotificationKind::DownloadStopped
        | Aria2NotificationKind::DownloadCompleted
        | Aria2NotificationKind::DownloadErrored
        | Aria2NotificationKind::BitTorrentDownloadCompleted => RefreshHint::Full,
        Aria2NotificationKind::DownloadStarted
        | Aria2NotificationKind::DownloadPaused
        | Aria2NotificationKind::Unknown(_) => notification
            .gid
            .map_or(RefreshHint::Full, RefreshHint::Task),
    }
}

fn map_sync_error(error: RpcError) -> SyncError {
    let (kind, retryable) = match &error {
        RpcError::Configuration(_) => (SyncErrorKind::Configuration, false),
        RpcError::Closed | RpcError::Transport(_) => (SyncErrorKind::Disconnected, true),
        RpcError::Tls(_) => (SyncErrorKind::Tls, false),
        RpcError::Authentication(_) => (SyncErrorKind::Authentication, false),
        RpcError::Timeout { .. } => (SyncErrorKind::Timeout, true),
        RpcError::Remote { message, .. }
            if message.to_ascii_lowercase().contains("unauthorized") =>
        {
            (SyncErrorKind::Authentication, false)
        }
        RpcError::Remote { .. } | RpcError::Protocol(_) | RpcError::InvalidData { .. } => {
            (SyncErrorKind::Protocol, false)
        }
        RpcError::Serialization(_) => (SyncErrorKind::Internal, false),
    };
    SyncError::new(kind, error.to_string(), retryable)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn authentication_and_disconnect_errors_keep_recovery_semantics() {
        let authentication = map_sync_error(RpcError::Remote {
            code: 1,
            message: "Unauthorized".into(),
            data: None,
        });
        let disconnected = map_sync_error(RpcError::Closed);

        assert_eq!(authentication.kind, SyncErrorKind::Authentication);
        assert!(!authentication.retryable);
        assert_eq!(disconnected.kind, SyncErrorKind::Disconnected);
        assert!(disconnected.retryable);
    }

    #[test]
    fn configuration_tls_and_handshake_authentication_errors_are_terminal() {
        for (error, expected) in [
            (
                RpcError::Configuration("HTTP fallback is disabled".into()),
                SyncErrorKind::Configuration,
            ),
            (RpcError::Tls("unknown issuer".into()), SyncErrorKind::Tls),
            (
                RpcError::Authentication("HTTP status 401".into()),
                SyncErrorKind::Authentication,
            ),
        ] {
            let mapped = map_sync_error(error);
            assert_eq!(mapped.kind, expected);
            assert!(!mapped.retryable);
        }
    }

    #[test]
    fn terminal_notifications_refresh_live_and_stopped_collections_together() {
        let gid = Gid::from_u64(7);

        assert_eq!(
            notification_hint(Aria2Notification {
                kind: Aria2NotificationKind::DownloadCompleted,
                gid: Some(gid),
            }),
            RefreshHint::Full
        );
        assert_eq!(
            notification_hint(Aria2Notification {
                kind: Aria2NotificationKind::DownloadPaused,
                gid: Some(gid),
            }),
            RefreshHint::Task(gid)
        );
    }

    #[tokio::test]
    async fn already_closed_transport_closes_hint_stream_immediately() {
        let (notifications, _) = tokio::sync::broadcast::channel(1);
        let notification_receiver = notifications.subscribe();
        let (_closed_sender, closed) = tokio::sync::watch::channel(Some(RpcError::Closed));
        let (sender, mut receiver) = mpsc::channel(1);

        forward_notification_hints(notification_receiver, closed, sender).await;

        assert_eq!(receiver.recv().await, None);
    }
}
