use ariadeck_application::{
    ConnectedSyncSession, DownloadSyncConnector, DownloadSyncSession, EngineCapabilities,
    InitialSyncSnapshot, LiveSyncSnapshot, RefreshHint, StoppedPage, SyncError, SyncErrorKind,
};
use ariadeck_domain::{Gid, GlobalStat, TaskSnapshot};
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
        let client = Aria2Client::new(authenticated);
        Ok(ConnectedSyncSession::new(
            Box::new(RpcSyncSession { client, transport }),
            notifications,
        ))
    }
}

struct RpcSyncSession {
    client: Aria2Client<AuthenticatedTransport<WebSocketTransport>>,
    transport: WebSocketTransport,
}

#[async_trait]
impl DownloadSyncSession for RpcSyncSession {
    async fn initial_snapshot(&self, stopped_count: u32) -> Result<InitialSyncSnapshot, SyncError> {
        let snapshot = self
            .client
            .initial_sync_snapshot(stopped_count)
            .await
            .map_err(map_sync_error)?;
        let total = usize::try_from(snapshot.global_stat.stopped_tasks).unwrap_or(usize::MAX);
        Ok(InitialSyncSnapshot {
            capabilities: EngineCapabilities {
                version: snapshot.version.version,
                enabled_features: snapshot.version.enabled_features,
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
        Ok(LiveSyncSnapshot {
            active: snapshot.active,
            waiting: snapshot.waiting,
        })
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
                .tell_stopped(offset_i64, count, TaskKey::LIST_PROJECTION)
        )
        .map_err(map_sync_error)?;
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
        self.client
            .refresh_tasks(gids)
            .await
            .map_err(map_sync_error)
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
        RpcError::Closed | RpcError::Transport(_) => (SyncErrorKind::Disconnected, true),
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
