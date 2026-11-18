use std::{sync::Arc, time::Duration};

use ariadeck_domain::{ConnectionState, ProfileId, SessionGeneration};
use tokio::sync::{broadcast, mpsc, watch};

use crate::{DownloadStore, TaskHistoryStore};

use super::{
    connected::{build_snapshot, emit_error, emit_patch, set_state},
    handle::{Control, connection_failure, unavailable_command_outcome, unavailable_error},
    history_sync::persist_task_category,
    types::{ActivityMode, EngineCapabilities, SyncError, SyncEvent},
};

pub(super) enum UnavailableControlDisposition {
    Continue,
    RetryNow,
    Stop,
}

pub(super) struct OfflineContext<'a> {
    pub(super) commands: &'a mut mpsc::Receiver<Control>,
    pub(super) store: &'a mut DownloadStore,
    pub(super) state: &'a ConnectionState,
    pub(super) activity: &'a mut ActivityMode,
    pub(super) cancellation: &'a mut watch::Receiver<bool>,
    pub(super) history: &'a Arc<dyn TaskHistoryStore>,
    pub(super) profile_id: ProfileId,
}

pub(super) async fn handle_unavailable_control(
    control: Option<Control>,
    store: &mut DownloadStore,
    state: &ConnectionState,
    activity: &mut ActivityMode,
    force_refresh_retries: bool,
    history: &Arc<dyn TaskHistoryStore>,
    profile_id: ProfileId,
) -> UnavailableControlDisposition {
    match control {
        Some(Control::SetActivity(mode)) => {
            *activity = mode;
            UnavailableControlDisposition::Continue
        }
        Some(Control::ForceRefresh) if force_refresh_retries => {
            UnavailableControlDisposition::RetryNow
        }
        Some(Control::ForceRefresh) => UnavailableControlDisposition::Continue,
        Some(Control::SetTaskCategory {
            gid,
            category_id,
            sender,
        }) => {
            store.set_task_category_affiliation(gid, category_id.clone());
            // Persist off the async worker thread while disconnected so
            // affiliation survives reconnect seed without stalling Tokio.
            persist_task_category(history, profile_id, gid, category_id).await;
            let _ = sender.send(());
            UnavailableControlDisposition::Continue
        }
        Some(Control::LoadMoreStopped { sender }) => {
            let _ = sender.send(store.stopped_history());
            UnavailableControlDisposition::Continue
        }
        Some(Control::Snapshot { query, sender }) => {
            let _ = sender.send(build_snapshot(
                store,
                state,
                &query,
                &EngineCapabilities::default(),
            ));
            UnavailableControlDisposition::Continue
        }
        Some(Control::Execute { sender, .. }) => {
            let _ = sender.send(unavailable_command_outcome(
                "Task commands are unavailable until aria2 is connected and synchronized.",
            ));
            UnavailableControlDisposition::Continue
        }
        Some(Control::ApplyDownloadProxy { sender, .. }) => {
            let _ = sender.send(Err(unavailable_error(
                "Download proxy settings cannot be applied until aria2 is connected and synchronized.",
            )));
            UnavailableControlDisposition::Continue
        }
        Some(Control::ApplySpeedLimit { sender, .. }) => {
            let _ = sender.send(Err(unavailable_error(
                "Speed limits cannot be applied until aria2 is connected and synchronized.",
            )));
            UnavailableControlDisposition::Continue
        }
        Some(Control::ApplyTransferPolicy { sender, .. }) => {
            let _ = sender.send(Err(unavailable_error(
                "Transfer policy cannot be applied until aria2 is connected and synchronized.",
            )));
            UnavailableControlDisposition::Continue
        }
        Some(Control::ApplyBtTracker { sender, .. }) => {
            let _ = sender.send(Err(unavailable_error(
                "Tracker list cannot be applied until aria2 is connected and synchronized.",
            )));
            UnavailableControlDisposition::Continue
        }
        Some(Control::TaskDetails { sender, .. }) => {
            let _ = sender.send(Err(unavailable_error(
                "Task details are unavailable until aria2 is connected and synchronized.",
            )));
            UnavailableControlDisposition::Continue
        }
        Some(Control::ConnectionDetails { sender, .. }) => {
            let _ = sender.send(Err(unavailable_error(
                "Connection details are unavailable until aria2 is connected and synchronized.",
            )));
            UnavailableControlDisposition::Continue
        }
        None => UnavailableControlDisposition::Stop,
    }
}

pub(super) fn handle_connection_failure(
    events: &broadcast::Sender<SyncEvent>,
    state: &mut ConnectionState,
    store: &mut DownloadStore,
    generation: SessionGeneration,
    error: &SyncError,
) -> bool {
    emit_error(events, error.clone());
    if let Ok(patch) = store.set_stale(generation, true) {
        emit_patch(events, patch);
    }
    if !error.retryable {
        set_state(
            events,
            state,
            ConnectionState::Failed {
                reason: connection_failure(error),
            },
        );
    } else {
        set_state(events, state, ConnectionState::Disconnected);
    }
    error.retryable
}

pub(super) async fn wait_for_retry_delay(delay: Duration, ctx: OfflineContext<'_>) -> bool {
    let OfflineContext {
        commands,
        store,
        state,
        activity,
        cancellation,
        history,
        profile_id,
    } = ctx;
    let sleep = tokio::time::sleep(delay);
    tokio::pin!(sleep);
    loop {
        tokio::select! {
            biased;
            () = wait_for_cancellation(cancellation) => return false,
            () = &mut sleep => return true,
            command = commands.recv() => match handle_unavailable_control(
                command,
                store,
                state,
                activity,
                true,
                history,
                profile_id,
            )
            .await
            {
                UnavailableControlDisposition::Continue => {}
                UnavailableControlDisposition::RetryNow => return true,
                UnavailableControlDisposition::Stop => return false,
            }
        }
    }
}

pub(super) async fn wait_for_manual_retry(ctx: OfflineContext<'_>) -> bool {
    let OfflineContext {
        commands,
        store,
        state,
        activity,
        cancellation,
        history,
        profile_id,
    } = ctx;
    loop {
        tokio::select! {
            biased;
            () = wait_for_cancellation(cancellation) => return false,
            command = commands.recv() => match handle_unavailable_control(
                command,
                store,
                state,
                activity,
                true,
                history,
                profile_id,
            )
            .await
            {
                UnavailableControlDisposition::Continue => {}
                UnavailableControlDisposition::RetryNow => return true,
                UnavailableControlDisposition::Stop => return false,
            }
        }
    }
}

pub(super) fn cancellation_requested(cancellation: &watch::Receiver<bool>) -> bool {
    *cancellation.borrow()
}

pub(super) async fn wait_for_cancellation(cancellation: &mut watch::Receiver<bool>) {
    loop {
        if cancellation_requested(cancellation) {
            return;
        }
        if cancellation.changed().await.is_err() {
            return;
        }
    }
}
