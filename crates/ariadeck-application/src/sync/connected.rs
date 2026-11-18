use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

use ariadeck_domain::{ConnectionState, Gid, ProfileId, SessionGeneration, TaskSnapshot};
use tokio::sync::{broadcast, mpsc, watch};

use crate::{
    AppCommand, CommandOutcome, CommandService, DownloadEngineGateway, DownloadStore, StorePatch,
    TaskCommandContext, TaskConnectionDetailsGateway, TaskDetailsGateway, TaskHistoryStore,
    TaskListQuery,
};

use super::{
    backoff::PollTimers,
    handle::{Control, stale_session_error, unsupported_error, wrong_profile_error},
    history_sync::{
        HistoryWriteTracker, delete_history_for_gids, persist_task_category,
        persist_terminal_snapshots, seed_history_into_store, spawn_history_writer,
    },
    offline::wait_for_cancellation,
    types::{
        ActivityMode, CoordinatorConfig, DownloadSyncSession, EngineCapabilities, RefreshHint,
        StoreSnapshot, SyncError, SyncErrorKind, SyncEvent,
    },
};

pub(super) struct ConnectedContext<'a> {
    pub(super) session: &'a dyn DownloadSyncSession,
    pub(super) command_gateway: Option<&'a Arc<dyn DownloadEngineGateway>>,
    pub(super) details_gateway: Option<&'a Arc<dyn TaskDetailsGateway>>,
    pub(super) connection_gateway: Option<&'a Arc<dyn TaskConnectionDetailsGateway>>,
    pub(super) notifications: mpsc::Receiver<RefreshHint>,
    pub(super) commands: &'a mut mpsc::Receiver<Control>,
    pub(super) store: &'a mut DownloadStore,
    pub(super) generation: SessionGeneration,
    pub(super) state: &'a mut ConnectionState,
    pub(super) activity: &'a mut ActivityMode,
    pub(super) config: &'a CoordinatorConfig,
    pub(super) events: &'a broadcast::Sender<SyncEvent>,
    pub(super) cancellation: &'a mut watch::Receiver<bool>,
    pub(super) capabilities: &'a EngineCapabilities,
}

pub(super) async fn run_connected(ctx: ConnectedContext<'_>) -> ConnectedExit {
    let ConnectedContext {
        session,
        command_gateway,
        details_gateway,
        connection_gateway,
        mut notifications,
        commands,
        store,
        generation,
        state,
        activity,
        config,
        events,
        cancellation,
        capabilities,
    } = ctx;
    let mut timers = PollTimers::new(config.refresh, *activity);
    let mut pending_tasks = HashSet::new();
    let mut pending_full = false;
    let mut history_tracker = HistoryWriteTracker::default();
    let history_writer = spawn_history_writer(Arc::clone(&config.history));

    loop {
        tokio::select! {
            biased;
            () = wait_for_cancellation(cancellation) => return ConnectedExit::Stop,
            command = commands.recv() => {
                match command {
                    Some(Control::SetActivity(mode)) => {
                        *activity = mode;
                        timers = PollTimers::new(config.refresh, mode);
                    }
                    Some(Control::ForceRefresh) => {
                        let result = tokio::select! {
                            biased;
                            () = wait_for_cancellation(cancellation) => return ConnectedExit::Stop,
                            result = refresh_all(
                                session,
                                store,
                                generation,
                                config.stopped_page_size,
                                events,
                                &config.history,
                                config.profile_id,
                            ) => result,
                        };
                        if let Err(error) = result {
                            return ConnectedExit::Retry(error);
                        }
                    }
                    Some(Control::SetTaskCategory {
                        gid,
                        category_id,
                        sender,
                    }) => {
                        persist_task_category(&config.history, config.profile_id, gid, category_id.clone()).await;
                        store.set_task_category_affiliation(gid, category_id);
                        let _ = sender.send(());
                    }
                    Some(Control::LoadMoreStopped { sender }) => {
                        let history = store.stopped_history();
                        if !history.can_load_more {
                            let _ = sender.send(history);
                            continue;
                        }
                        let offset = history.next_offset;
                        let page_size = config.stopped_page_size;
                        let result = tokio::select! {
                            biased;
                            () = wait_for_cancellation(cancellation) => return ConnectedExit::Stop,
                            result = session.refresh_stopped_page(offset, page_size) => result,
                        };
                        match result {
                            Ok(page) => {
                                persist_terminal_snapshots(
                                    &config.history,
                                    config.profile_id,
                                    page.tasks.iter().cloned(),
                                )
                                .await;
                                match store.apply_stopped_page(
                                    generation,
                                    page.offset,
                                    Some(page.total),
                                    page.tasks,
                                ) {
                                    Ok(patch) => {
                                        emit_patch(events, patch);
                                        seed_history_into_store(
                                            store,
                                            generation,
                                            &config.history,
                                            config.profile_id,
                                            events,
                                        )
                                        .await;
                                        let _ = sender.send(store.stopped_history());
                                    }
                                    Err(error) => {
                                        return ConnectedExit::Retry(SyncError::store(error));
                                    }
                                }
                            }
                            Err(error) => return ConnectedExit::Retry(error),
                        }
                    }
                    Some(Control::Snapshot { query, sender }) => {
                        let _ = sender.send(build_snapshot(store, state, &query, capabilities));
                    }
                    Some(Control::Execute {
                        session: expected_session,
                        command,
                        sender,
                    }) => {
                        if expected_session != store.session() {
                            let _ = sender.send(CommandOutcome::failure(stale_session_error()));
                            continue;
                        }
                        let Some(gateway) = command_gateway else {
                            let _ = sender.send(CommandOutcome::failure(unsupported_error(
                                "The connected engine does not expose task commands.",
                            )));
                            continue;
                        };
                        let service = CommandService::new(
                            config.profile_id,
                            gateway.clone(),
                            capabilities.clone(),
                        );
                        let task_contexts = match &command {
                            AppCommand::PauseTasks(tasks)
                            | AppCommand::ForcePauseTasks(tasks)
                            | AppCommand::ResumeTasks(tasks) => Some(tasks.as_slice()),
                            AppCommand::MoveTaskInQueue(request) => {
                                Some(std::slice::from_ref(&request.task))
                            }
                            AppCommand::RemoveTasks(request)
                            | AppCommand::ForceRemoveTasks(request) => {
                                Some(request.tasks.as_slice())
                            }
                            AppCommand::RetryTasks(tasks) => Some(tasks.as_slice()),
                            AppCommand::SetTaskOutputName(request) => {
                                Some(std::slice::from_ref(&request.task))
                            }
                            AppCommand::SetTaskSpeedLimit(request) => {
                                Some(std::slice::from_ref(&request.task))
                            }
                            AppCommand::SetTaskConnectionPolicy(request) => {
                                Some(std::slice::from_ref(&request.task))
                            }
                            AppCommand::SetTaskOptions(request) => {
                                Some(std::slice::from_ref(&request.task))
                            }
                            _ => None,
                        }
                        .map_or_else(HashMap::new, |tasks| {
                            tasks
                                .iter()
                                .filter(|identity| identity.profile_id == config.profile_id)
                                .filter_map(|identity| {
                                    store.task(identity.gid).map(|task| {
                                        (
                                            *identity,
                                            TaskCommandContext {
                                                status: task.status,
                                                metadata: task.metadata.clone(),
                                            },
                                        )
                                    })
                                })
                                .collect::<HashMap<_, _>>()
                        });
                        let refresh_after_success = match &command {
                            AppCommand::SetTaskOutputName(request) => {
                                RefreshHint::Task(request.task.gid)
                            }
                            AppCommand::SetTaskSpeedLimit(request) => {
                                RefreshHint::Task(request.task.gid)
                            }
                            AppCommand::SetTaskConnectionPolicy(request) => {
                                RefreshHint::Task(request.task.gid)
                            }
                            AppCommand::SetTaskOptions(request) => {
                                RefreshHint::Task(request.task.gid)
                            }
                            _ => RefreshHint::Full,
                        };
                        let custom_output_name = match &command {
                            AppCommand::SetTaskOutputName(request) => {
                                Some((request.task.gid, request.output_name.clone()))
                            }
                            _ => None,
                        };
                        let remove_gids = match &command {
                            AppCommand::RemoveTasks(request)
                            | AppCommand::ForceRemoveTasks(request) => request
                                .tasks
                                .iter()
                                .filter(|identity| identity.profile_id == config.profile_id)
                                .map(|identity| identity.gid)
                                .collect::<Vec<_>>(),
                            _ => Vec::new(),
                        };
                        let outcome = tokio::select! {
                            biased;
                            () = wait_for_cancellation(cancellation) => return ConnectedExit::Stop,
                            outcome = service.execute(command, &task_contexts) => outcome,
                        };
                        let refresh_authoritative_state = outcome.has_successes()
                            || outcome.has_unknown_outcome();
                        if outcome.has_successes()
                            && let Some((gid, output_name)) = custom_output_name
                        {
                            match store.set_custom_output_name(generation, gid, output_name) {
                                Ok(patch) => emit_patch(events, patch),
                                Err(error) => {
                                    return ConnectedExit::Retry(SyncError::store(error));
                                }
                            }
                        }
                        if outcome.has_successes() && !remove_gids.is_empty() {
                            let succeeded_gids = outcome
                                .succeeded_tasks()
                                .into_iter()
                                .map(|identity| identity.gid)
                                .collect::<HashSet<_>>();
                            let deleted = remove_gids
                                .into_iter()
                                .filter(|gid| succeeded_gids.contains(gid))
                                .collect::<Vec<_>>();
                            if !deleted.is_empty() {
                                history_tracker.forget(&deleted);
                                delete_history_for_gids(
                                    &config.history,
                                    config.profile_id,
                                    deleted.clone(),
                                )
                                .await;
                                match store.remove_history_only(generation, &deleted) {
                                    Ok(patch) => emit_patch(events, patch),
                                    Err(error) => {
                                        tracing::warn!(
                                            %error,
                                            "failed to drop history-only rows after remove"
                                        );
                                    }
                                }
                            }
                        }
                        if refresh_authoritative_state {
                            match refresh_after_success {
                                RefreshHint::Task(gid) => {
                                    pending_tasks.insert(gid);
                                }
                                RefreshHint::Full => pending_full = true,
                            }
                        }
                        let _ = sender.send(outcome);
                    }
                    Some(Control::ApplyDownloadProxy {
                        session: expected_session,
                        config: proxy,
                        sender,
                    }) => {
                        if expected_session != store.session() {
                            let _ = sender.send(Err(stale_session_error()));
                            continue;
                        }
                        let Some(gateway) = command_gateway else {
                            let _ = sender.send(Err(unsupported_error(
                                "The connected engine does not expose global proxy settings.",
                            )));
                            continue;
                        };
                        let service = CommandService::new(config.profile_id, gateway.clone(), capabilities.clone());
                        let result = tokio::select! {
                            biased;
                            () = wait_for_cancellation(cancellation) => return ConnectedExit::Stop,
                            result = service.apply_download_proxy(&proxy) => result,
                        };
                        let _ = sender.send(result);
                    }
                    Some(Control::ApplySpeedLimit {
                        session: expected_session,
                        config: speed_limit,
                        sender,
                    }) => {
                        if expected_session != store.session() {
                            let _ = sender.send(Err(stale_session_error()));
                            continue;
                        }
                        let Some(gateway) = command_gateway else {
                            let _ = sender.send(Err(unsupported_error(
                                "The connected engine does not expose global speed limits.",
                            )));
                            continue;
                        };
                        let service = CommandService::new(config.profile_id, gateway.clone(), capabilities.clone());
                        let result = tokio::select! {
                            biased;
                            () = wait_for_cancellation(cancellation) => return ConnectedExit::Stop,
                            result = service.apply_speed_limit(&speed_limit) => result,
                        };
                        let _ = sender.send(result);
                    }
                    Some(Control::ApplyTransferPolicy {
                        session: expected_session,
                        config: transfer_policy,
                        sender,
                    }) => {
                        if expected_session != store.session() {
                            let _ = sender.send(Err(stale_session_error()));
                            continue;
                        }
                        let Some(gateway) = command_gateway else {
                            let _ = sender.send(Err(unsupported_error(
                                "The connected engine does not expose transfer-policy settings.",
                            )));
                            continue;
                        };
                        let service = CommandService::new(config.profile_id, gateway.clone(), capabilities.clone());
                        let result = tokio::select! {
                            biased;
                            () = wait_for_cancellation(cancellation) => return ConnectedExit::Stop,
                            result = service.apply_transfer_policy(&transfer_policy) => result,
                        };
                        let _ = sender.send(result);
                    }
                    Some(Control::ApplyBtTracker {
                        session: expected_session,
                        trackers,
                        sender,
                    }) => {
                        if expected_session != store.session() {
                            let _ = sender.send(Err(stale_session_error()));
                            continue;
                        }
                        let Some(gateway) = command_gateway else {
                            let _ = sender.send(Err(unsupported_error(
                                "The connected engine does not expose global tracker settings.",
                            )));
                            continue;
                        };
                        let service = CommandService::new(config.profile_id, gateway.clone(), capabilities.clone());
                        let result = tokio::select! {
                            biased;
                            () = wait_for_cancellation(cancellation) => return ConnectedExit::Stop,
                            result = service.apply_bt_tracker(&trackers) => result,
                        };
                        let _ = sender.send(result);
                    }
                    Some(Control::TaskDetails {
                        session: expected_session,
                        task,
                        sender,
                    }) => {
                        if expected_session != store.session() {
                            let _ = sender.send(Err(stale_session_error()));
                            continue;
                        }
                        if task.profile_id != config.profile_id {
                            let _ = sender.send(Err(wrong_profile_error()));
                            continue;
                        }
                        let Some(gateway) = details_gateway.cloned() else {
                            let _ = sender.send(Err(unsupported_error(
                                "The connected engine does not expose task details.",
                            )));
                            continue;
                        };
                        let mut cancel_rx = cancellation.clone();
                        tokio::spawn(async move {
                            let result = tokio::select! {
                                biased;
                                () = wait_for_cancellation(&mut cancel_rx) => return,
                                result = gateway.task_details(task.gid) => result.map_err(Into::into),
                            };
                            let _ = sender.send(result);
                        });
                    }
                    Some(Control::ConnectionDetails {
                        session: expected_session,
                        task,
                        active,
                        is_bittorrent,
                        sender,
                    }) => {
                        if expected_session != store.session() {
                            let _ = sender.send(Err(stale_session_error()));
                            continue;
                        }
                        if task.profile_id != config.profile_id {
                            let _ = sender.send(Err(wrong_profile_error()));
                            continue;
                        }
                        let Some(gateway) = connection_gateway.cloned() else {
                            let _ = sender.send(Err(unsupported_error(
                                "The connected engine does not expose connection details.",
                            )));
                            continue;
                        };
                        let mut cancel_rx = cancellation.clone();
                        tokio::spawn(async move {
                            let result = tokio::select! {
                                biased;
                                () = wait_for_cancellation(&mut cancel_rx) => return,
                                result = gateway.connection_details(task.gid, active, is_bittorrent)
                                    => result.map_err(Into::into),
                            };
                            let _ = sender.send(result);
                        });
                    }
                    None => return ConnectedExit::Stop,
                }
            }
            hint = notifications.recv() => {
                match hint {
                    Some(RefreshHint::Task(gid)) => { pending_tasks.insert(gid); }
                    Some(RefreshHint::Full) => pending_full = true,
                    None => {
                        return ConnectedExit::Retry(SyncError::new(
                            SyncErrorKind::Disconnected,
                            "RPC notification stream closed",
                            true,
                        ));
                    }
                }
            }
            _ = timers.notification.tick() => {
                if pending_full || !pending_tasks.is_empty() {
                    let refresh = async {
                        if pending_full {
                            pending_full = false;
                            pending_tasks.clear();
                            refresh_all(
                                session,
                                store,
                                generation,
                                config.stopped_page_size,
                                events,
                                &config.history,
                                config.profile_id,
                            ).await
                        } else {
                            let gids = pending_tasks.drain().collect::<Vec<_>>();
                            refresh_targeted(
                                session,
                                store,
                                generation,
                                &gids,
                                events,
                                &config.history,
                                config.profile_id,
                            ).await
                        }
                    };
                    let result = tokio::select! {
                        biased;
                        () = wait_for_cancellation(cancellation) => return ConnectedExit::Stop,
                        result = refresh => result,
                    };
                    if let Err(error) = result {
                        return ConnectedExit::Retry(error);
                    }
                }
            }
            _ = timers.global.tick() => {
                let result = tokio::select! {
                    biased;
                    () = wait_for_cancellation(cancellation) => return ConnectedExit::Stop,
                    result = session.refresh_global_stat() => result,
                };
                match result {
                    Ok(stat) => {
                        if let Err(error) = store.record_speed_sample(generation, stat) {
                            return ConnectedExit::Retry(SyncError::store(error));
                        }
                        match store.update_global_stat(generation, stat) {
                            Ok(patch) => emit_global_update(events, patch),
                            Err(error) => return ConnectedExit::Retry(SyncError::store(error)),
                        }
                    }
                    Err(error) => return ConnectedExit::Retry(error),
                }
            }
            _ = timers.live.tick() => {
                let result = tokio::select! {
                    biased;
                    () = wait_for_cancellation(cancellation) => return ConnectedExit::Stop,
                    result = session.refresh_live() => result,
                };
                match result {
                    Ok(live) => match store.reconcile_live(generation, live.active, live.waiting) {
                        Ok(patch) => emit_patch(events, patch),
                        Err(error) => return ConnectedExit::Retry(SyncError::store(error)),
                    },
                    Err(error) => return ConnectedExit::Retry(error),
                }
            }
            _ = timers.stopped.tick() => {
                // Refresh every already-loaded contiguous page so a prior
                // "Load more" request is not discarded by the periodic poll
                // (HISTORY-001). The first page also updates the authoritative
                // total from aria2's numStoppedTotal.
                let page_size = config.stopped_page_size.max(1);
                let mut offset = 0usize;
                let loaded_through = store.next_stopped_offset().max(page_size as usize);
                loop {
                    let result = tokio::select! {
                        biased;
                        () = wait_for_cancellation(cancellation) => return ConnectedExit::Stop,
                        result = session.refresh_stopped_page(offset, page_size) => result,
                    };
                    match result {
                        Ok(page) => {
                            let total = page.total;
                            let changed_records = history_tracker
                                .collect_changed_records(config.profile_id, &page.tasks);
                            if !changed_records.is_empty()
                                && history_writer.try_send(changed_records).is_err()
                            {
                                tracing::debug!("history write queue full; deferring stopped page batch");
                            }
                            match store.apply_stopped_page(
                                generation,
                                page.offset,
                                Some(total),
                                page.tasks,
                            ) {
                                Ok(patch) => emit_patch(events, patch),
                                Err(error) => return ConnectedExit::Retry(SyncError::store(error)),
                            }
                            offset = offset.saturating_add(page_size as usize);
                            if offset >= loaded_through || offset >= total {
                                break;
                            }
                        }
                        Err(error) => return ConnectedExit::Retry(error),
                    }
                }
            }
        }
    }
}

pub(super) async fn refresh_all(
    session: &dyn DownloadSyncSession,
    store: &mut DownloadStore,
    generation: SessionGeneration,
    stopped_page_size: u32,
    events: &broadcast::Sender<SyncEvent>,
    history: &Arc<dyn TaskHistoryStore>,
    profile_id: ProfileId,
) -> Result<(), SyncError> {
    let global = session.refresh_global_stat().await?;
    let live = session.refresh_live().await?;
    emit_patch(
        events,
        store
            .update_global_stat(generation, global)
            .map_err(SyncError::store)?,
    );
    refresh_loaded_stopped_pages(
        session,
        store,
        generation,
        stopped_page_size,
        events,
        history,
        profile_id,
    )
    .await?;
    emit_patch(
        events,
        store
            .reconcile_live(generation, live.active, live.waiting)
            .map_err(SyncError::store)?,
    );
    seed_history_into_store(store, generation, history, profile_id, events).await;
    Ok(())
}

pub(super) async fn refresh_loaded_stopped_pages(
    session: &dyn DownloadSyncSession,
    store: &mut DownloadStore,
    generation: SessionGeneration,
    stopped_page_size: u32,
    events: &broadcast::Sender<SyncEvent>,
    history: &Arc<dyn TaskHistoryStore>,
    profile_id: ProfileId,
) -> Result<(), SyncError> {
    let page_size = stopped_page_size.max(1);
    let mut offset = 0usize;
    let loaded_through = store.next_stopped_offset().max(page_size as usize);
    loop {
        let page = session.refresh_stopped_page(offset, page_size).await?;
        let total = page.total;
        persist_terminal_snapshots(history, profile_id, page.tasks.iter().cloned()).await;
        emit_patch(
            events,
            store
                .apply_stopped_page(generation, page.offset, Some(total), page.tasks)
                .map_err(SyncError::store)?,
        );
        offset = offset.saturating_add(page_size as usize);
        if offset >= loaded_through || offset >= total {
            break;
        }
    }
    Ok(())
}

pub(super) async fn refresh_targeted(
    session: &dyn DownloadSyncSession,
    store: &mut DownloadStore,
    generation: SessionGeneration,
    gids: &[Gid],
    events: &broadcast::Sender<SyncEvent>,
    history: &Arc<dyn TaskHistoryStore>,
    profile_id: ProfileId,
) -> Result<(), SyncError> {
    let refreshed = session.refresh_tasks(gids).await?;
    let terminal_snapshots: Vec<TaskSnapshot> = refreshed
        .iter()
        .filter_map(|(_, snapshot)| snapshot.clone())
        .collect();
    persist_terminal_snapshots(history, profile_id, terminal_snapshots).await;
    for (gid, snapshot) in refreshed {
        emit_patch(
            events,
            store
                .apply_task_snapshot(generation, gid, snapshot)
                .map_err(SyncError::store)?,
        );
    }
    Ok(())
}

pub(super) fn build_snapshot(
    store: &DownloadStore,
    state: &ConnectionState,
    query: &TaskListQuery,
    capabilities: &EngineCapabilities,
) -> StoreSnapshot {
    let view = store.view(query);
    let tasks = view
        .visible_gids
        .iter()
        .filter_map(|gid| store.task(*gid).cloned())
        .collect::<Vec<_>>();
    let observed_seeding_seconds = tasks
        .iter()
        .filter_map(|task| {
            store
                .observed_seeding_seconds(task.gid)
                .map(|seconds| (task.gid, seconds))
        })
        .collect();
    StoreSnapshot {
        session: store.session(),
        connection_state: state.clone(),
        stale: store.is_stale(),
        global_stat: store.global_stat(),
        speed_history: store.speed_history().clone(),
        counts: store.counts(),
        stopped_history: store.stopped_history(),
        view,
        tasks,
        observed_seeding_seconds,
        capabilities: capabilities.clone(),
        category_by_gid: store.category_by_gid.clone(),
    }
}

pub(super) fn set_state(
    events: &broadcast::Sender<SyncEvent>,
    state: &mut ConnectionState,
    next: ConnectionState,
) {
    if *state != next {
        *state = next.clone();
        let _ = events.send(SyncEvent::ConnectionStateChanged(next));
    }
}

pub(super) fn emit_patch(events: &broadcast::Sender<SyncEvent>, patch: StorePatch) {
    if !patch.is_empty() {
        let _ = events.send(SyncEvent::StorePatched(patch));
    }
}

pub(super) fn emit_global_update(events: &broadcast::Sender<SyncEvent>, patch: StorePatch) {
    if patch.is_empty() {
        let _ = events.send(SyncEvent::SpeedHistoryChanged);
    } else {
        emit_patch(events, patch);
    }
}

pub(super) fn emit_error(events: &broadcast::Sender<SyncEvent>, error: SyncError) {
    let _ = events.send(SyncEvent::Error(error));
}

pub(super) enum ConnectedExit {
    Retry(SyncError),
    Stop,
}
