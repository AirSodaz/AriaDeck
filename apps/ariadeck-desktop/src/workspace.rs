use std::{env, sync::Arc, time::Duration};

use ariadeck_application::{
    CoordinatorConfig, StoreSnapshot, SyncHandle, TaskListQuery, spawn_sync_coordinator,
};
use ariadeck_domain::{
    ConnectionState, DownloadFilter, DownloadStatus, DownloadTask, ProfileId, TaskProgress,
};
use ariadeck_rpc::{RpcSecret, RpcSyncConnector, WebSocketConfig};
use ariadeck_ui::{
    AppShell, AppShellEvent, ConnectionView, DownloadRowView, TaskCountsView, TaskIdentity,
    TaskStatusView, Theme, WorkspaceFilter, WorkspaceQuery, WorkspaceSnapshot,
};
use gpui::{AppContext as _, Context, Entity, IntoElement, Render, Subscription, Window};
use tokio::{runtime::Runtime, sync::watch};
use url::Url;

pub struct DesktopRoot {
    workspace: Entity<AppShell>,
    sync: Option<SyncHandle>,
    runtime: Arc<Runtime>,
    query_sender: watch::Sender<TaskListQuery>,
    _workspace_subscription: Subscription,
}

impl DesktopRoot {
    #[must_use]
    pub fn new(runtime: Arc<Runtime>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let (sync, initial_snapshot) = match create_sync_handle(&runtime) {
            Ok(handle) => {
                let snapshot = WorkspaceSnapshot {
                    connection: ConnectionView::Connecting,
                    ..WorkspaceSnapshot::default()
                };
                (Some(handle), snapshot)
            }
            Err(error) => {
                tracing::error!(%error, "failed to configure aria2 synchronization");
                let snapshot = WorkspaceSnapshot {
                    connection: ConnectionView::Failed {
                        summary: error,
                        retryable: false,
                    },
                    ..WorkspaceSnapshot::default()
                };
                (None, snapshot)
            }
        };
        let workspace = cx.new(|cx| {
            let mut shell = AppShell::new(Theme::dark(), window, cx);
            shell.set_snapshot(initial_snapshot, cx);
            shell
        });
        let (query_sender, query_receiver) = watch::channel(TaskListQuery::default());
        let workspace_subscription = cx.subscribe(
            &workspace,
            |this: &mut Self, _workspace, event: &AppShellEvent, _cx| match event {
                AppShellEvent::QueryChanged(query) => {
                    this.query_sender.send_replace(map_query(query));
                }
                AppShellEvent::RetryRequested => {
                    if let Some(handle) = this.sync.clone() {
                        this.runtime.spawn(async move {
                            handle.force_refresh().await;
                        });
                    }
                }
            },
        );

        if let Some(handle) = sync.clone() {
            spawn_snapshot_bridge(handle, query_receiver, cx);
        }

        Self {
            workspace,
            sync,
            runtime,
            query_sender,
            _workspace_subscription: workspace_subscription,
        }
    }
}

impl Drop for DesktopRoot {
    fn drop(&mut self) {
        if let Some(handle) = self.sync.take() {
            self.runtime.block_on(handle.stop());
        }
    }
}

impl Render for DesktopRoot {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        self.workspace.clone()
    }
}

fn create_sync_handle(runtime: &Runtime) -> Result<SyncHandle, String> {
    let endpoint =
        env::var("ARIADECK_RPC_URL").unwrap_or_else(|_| "ws://127.0.0.1:6800/jsonrpc".into());
    let endpoint = Url::parse(&endpoint).map_err(|error| format!("Invalid RPC URL: {error}"))?;
    let secret = env::var("ARIADECK_RPC_SECRET")
        .ok()
        .filter(|secret| !secret.is_empty())
        .map(RpcSecret::new);
    let mut websocket = WebSocketConfig::new(endpoint.clone());
    websocket.connect_timeout = Duration::from_millis(750);
    websocket.request_timeout = Duration::from_secs(5);
    let connector = Arc::new(RpcSyncConnector::new(websocket, secret));
    let coordinator = CoordinatorConfig::new(ProfileId::new());
    tracing::info!(
        scheme = endpoint.scheme(),
        host = endpoint.host_str().unwrap_or("unknown"),
        port = endpoint.port_or_known_default(),
        "configured external aria2 RPC profile"
    );
    let _runtime_guard = runtime.enter();
    Ok(spawn_sync_coordinator(connector, coordinator))
}

fn spawn_snapshot_bridge(
    handle: SyncHandle,
    mut query_receiver: watch::Receiver<TaskListQuery>,
    cx: &mut Context<DesktopRoot>,
) {
    let mut events = handle.subscribe();
    cx.spawn(async move |this, cx| {
        loop {
            let query = query_receiver.borrow().clone();
            let Some(snapshot) = handle.snapshot(query.clone()).await else {
                break;
            };
            if *query_receiver.borrow() == query {
                let snapshot = map_snapshot(snapshot);
                if this
                    .update(cx, |this, cx| {
                        this.workspace.update(cx, |workspace, cx| {
                            workspace.set_snapshot(snapshot, cx);
                        });
                    })
                    .is_err()
                {
                    break;
                }
            }

            tokio::select! {
                event = events.recv() => match event {
                    Ok(_) | Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                },
                changed = query_receiver.changed() => {
                    if changed.is_err() {
                        break;
                    }
                }
            }
        }
    })
    .detach();
}

fn map_query(query: &WorkspaceQuery) -> TaskListQuery {
    TaskListQuery {
        filter: match query.filter {
            WorkspaceFilter::All => DownloadFilter::All,
            WorkspaceFilter::Active => DownloadFilter::Active,
            WorkspaceFilter::Waiting => DownloadFilter::Waiting,
            WorkspaceFilter::Paused => DownloadFilter::Paused,
            WorkspaceFilter::Completed => DownloadFilter::Completed,
            WorkspaceFilter::Failed => DownloadFilter::Failed,
        },
        search: query.search.clone(),
        ..TaskListQuery::default()
    }
}

fn map_snapshot(snapshot: StoreSnapshot) -> WorkspaceSnapshot {
    let profile_id = snapshot.session.profile_id.to_string();
    WorkspaceSnapshot {
        profile_id: profile_id.clone(),
        generation: snapshot.session.generation.get(),
        source_revision: snapshot.view.source_revision,
        connection: map_connection(snapshot.connection_state),
        stale: snapshot.stale,
        download_rate: snapshot.global_stat.download_speed.get(),
        upload_rate: snapshot.global_stat.upload_speed.get(),
        counts: TaskCountsView {
            all: snapshot.counts.all,
            active: snapshot.counts.active,
            waiting: snapshot.counts.waiting,
            paused: snapshot.counts.paused,
            completed: snapshot.counts.completed,
            failed: snapshot.counts.failed,
        },
        tasks: snapshot
            .tasks
            .into_iter()
            .map(|task| map_task(&profile_id, task))
            .collect(),
    }
}

fn map_connection(state: ConnectionState) -> ConnectionView {
    match state {
        ConnectionState::Disconnected => ConnectionView::Disconnected,
        ConnectionState::Connecting => ConnectionView::Connecting,
        ConnectionState::Authenticating => ConnectionView::Authenticating,
        ConnectionState::Synchronizing => ConnectionView::Synchronizing,
        ConnectionState::Connected => ConnectionView::Connected,
        ConnectionState::Reconnecting { attempt } => ConnectionView::Reconnecting { attempt },
        ConnectionState::Failed { reason } => ConnectionView::Failed {
            summary: reason.summary,
            retryable: reason.retryable,
        },
    }
}

fn map_task(profile_id: &str, task: DownloadTask) -> DownloadRowView {
    let eta_seconds = TaskProgress::new(task.completed_length, task.total_length)
        .eta(task.download_speed)
        .map(|duration| duration.as_secs());
    DownloadRowView {
        identity: TaskIdentity {
            profile_id: profile_id.into(),
            gid: task.gid.to_string(),
        },
        display_name: task.display_name,
        status: match task.status {
            DownloadStatus::Active => TaskStatusView::Active,
            DownloadStatus::Waiting => TaskStatusView::Waiting,
            DownloadStatus::Paused => TaskStatusView::Paused,
            DownloadStatus::Complete => TaskStatusView::Complete,
            DownloadStatus::Error => TaskStatusView::Failed,
            DownloadStatus::Removed => TaskStatusView::Removed,
            DownloadStatus::Verifying => TaskStatusView::Verifying,
            DownloadStatus::Unknown => TaskStatusView::Unknown,
        },
        total_bytes: task.total_length.get(),
        completed_bytes: task.completed_length.get(),
        download_rate: task.download_speed.get(),
        upload_rate: task.upload_speed.get(),
        eta_seconds,
        revision: task.revision,
    }
}

#[cfg(test)]
mod tests {
    use ariadeck_domain::{ByteCount, ByteRate, Gid, TaskSnapshot};

    use super::*;

    #[test]
    fn domain_task_mapping_preserves_identity_progress_and_eta() {
        let mut snapshot = TaskSnapshot::new(Gid::from_u64(9), DownloadStatus::Active, "video.mkv");
        snapshot.total_length = ByteCount::new(1_000);
        snapshot.completed_length = ByteCount::new(400);
        snapshot.download_speed = ByteRate::new(100);
        let task = DownloadTask::from_snapshot(snapshot);

        let mapped = map_task("profile", task);

        assert_eq!(mapped.identity.profile_id, "profile");
        assert_eq!(mapped.identity.gid, "0000000000000009");
        assert_eq!(mapped.status, TaskStatusView::Active);
        assert_eq!(mapped.eta_seconds, Some(6));
    }

    #[test]
    fn presentation_filter_maps_to_application_query() {
        let query = map_query(&WorkspaceQuery {
            filter: WorkspaceFilter::Completed,
            search: "archive".into(),
        });

        assert_eq!(query.filter, DownloadFilter::Completed);
        assert_eq!(query.search, "archive");
    }
}
