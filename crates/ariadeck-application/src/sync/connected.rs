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
        HistoryWriteTracker, HistoryWriter, persist_terminal_snapshots, seed_history_into_store,
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
    let mut runner = ConnectedRunner::new(ctx);
    let exit = runner.run().await;
    runner.finish().await;
    exit
}

struct ConnectedRunner<'a> {
    ctx: ConnectedContext<'a>,
    timers: PollTimers,
    pending_tasks: HashSet<Gid>,
    pending_full: bool,
    history_tracker: HistoryWriteTracker,
    history_writer: Option<HistoryWriter>,
    session_cancel_tx: watch::Sender<bool>,
    session_cancel_rx: watch::Receiver<bool>,
}

impl<'a> ConnectedRunner<'a> {
    fn new(ctx: ConnectedContext<'a>) -> Self {
        let timers = PollTimers::new(ctx.config.refresh, *ctx.activity);
        let history_writer = Some(HistoryWriter::spawn(Arc::clone(&ctx.config.history)));
        let (session_cancel_tx, session_cancel_rx) = watch::channel(false);
        Self {
            ctx,
            timers,
            pending_tasks: HashSet::new(),
            pending_full: false,
            history_tracker: HistoryWriteTracker::default(),
            history_writer,
            session_cancel_tx,
            session_cancel_rx,
        }
    }

    async fn finish(mut self) {
        let _ = self.session_cancel_tx.send(true);
        if let Some(writer) = self.history_writer.take() {
            writer.shutdown().await;
        }
    }

    async fn run(&mut self) -> ConnectedExit {
        loop {
            enum Event {
                Cancel,
                Command(Box<Control>),
                CommandsClosed,
                Notification(Option<RefreshHint>),
                NotificationTick,
                GlobalTick,
                LiveTick,
                StoppedTick,
            }

            let event = tokio::select! {
                biased;
                () = wait_for_cancellation(self.ctx.cancellation) => Event::Cancel,
                command = self.ctx.commands.recv() => match command {
                    Some(control) => Event::Command(Box::new(control)),
                    None => Event::CommandsClosed,
                },
                hint = self.ctx.notifications.recv() => Event::Notification(hint),
                _ = self.timers.notification.tick() => Event::NotificationTick,
                _ = self.timers.global.tick() => Event::GlobalTick,
                _ = self.timers.live.tick() => Event::LiveTick,
                _ = self.timers.stopped.tick() => Event::StoppedTick,
            };

            let exit = match event {
                Event::Cancel | Event::CommandsClosed => Some(ConnectedExit::Stop),
                Event::Command(control) => self.handle_control(*control).await,
                Event::Notification(Some(RefreshHint::Task(gid))) => {
                    self.pending_tasks.insert(gid);
                    None
                }
                Event::Notification(Some(RefreshHint::Full)) => {
                    self.pending_full = true;
                    None
                }
                Event::Notification(None) => Some(ConnectedExit::Retry(SyncError::new(
                    SyncErrorKind::Disconnected,
                    "RPC notification stream closed",
                    true,
                ))),
                Event::NotificationTick => self.handle_notification_tick().await,
                Event::GlobalTick => self.handle_global_tick().await,
                Event::LiveTick => self.handle_live_tick().await,
                Event::StoppedTick => self.handle_stopped_tick().await,
            };

            if let Some(exit) = exit {
                return exit;
            }
        }
    }

    async fn handle_control(&mut self, control: Control) -> Option<ConnectedExit> {
        match control {
            Control::SetActivity(mode) => {
                *self.ctx.activity = mode;
                self.timers = PollTimers::new(self.ctx.config.refresh, mode);
                None
            }
            Control::ForceRefresh => self.handle_force_refresh().await,
            Control::SetTaskCategory {
                gid,
                category_id,
                sender,
            } => {
                if let Some(writer) = self.history_writer.as_ref() {
                    writer
                        .set_task_category(
                            &self.ctx.config.history,
                            self.ctx.config.profile_id,
                            gid,
                            category_id.clone(),
                        )
                        .await;
                }
                match self.ctx.store.set_task_category_affiliation_patch(
                    self.ctx.generation,
                    gid,
                    category_id,
                ) {
                    Ok(patch) => emit_patch(self.ctx.events, patch),
                    Err(error) => {
                        let _ = sender.send(());
                        return Some(ConnectedExit::Retry(SyncError::store(error)));
                    }
                }
                let _ = sender.send(());
                None
            }
            Control::LoadMoreStopped { sender } => self.handle_load_more_stopped(sender).await,
            Control::Snapshot { query, sender } => {
                let _ = sender.send(build_snapshot(
                    self.ctx.store,
                    self.ctx.state,
                    &query,
                    self.ctx.capabilities,
                ));
                None
            }
            Control::Execute {
                session,
                command,
                sender,
            } => self.handle_execute(session, command, sender).await,
            Control::ApplyDownloadProxy {
                session,
                config: proxy,
                sender,
            } => {
                self.handle_global_option(
                    session,
                    "The connected engine does not expose global proxy settings.",
                    sender,
                    |service| async move { service.apply_download_proxy(&proxy).await },
                )
                .await
            }
            Control::ApplySpeedLimit {
                session,
                config: speed_limit,
                sender,
            } => {
                self.handle_global_option(
                    session,
                    "The connected engine does not expose global speed limits.",
                    sender,
                    |service| async move { service.apply_speed_limit(&speed_limit).await },
                )
                .await
            }
            Control::ApplyTransferPolicy {
                session,
                config: transfer_policy,
                sender,
            } => {
                self.handle_global_option(
                    session,
                    "The connected engine does not expose transfer-policy settings.",
                    sender,
                    |service| async move { service.apply_transfer_policy(&transfer_policy).await },
                )
                .await
            }
            Control::ApplyBtTracker {
                session,
                trackers,
                sender,
            } => {
                self.handle_global_option(
                    session,
                    "The connected engine does not expose global tracker settings.",
                    sender,
                    |service| async move { service.apply_bt_tracker(&trackers).await },
                )
                .await
            }
            Control::TaskDetails {
                session,
                task,
                sender,
            } => {
                self.handle_task_details(session, task, sender);
                None
            }
            Control::ConnectionDetails {
                session,
                task,
                active,
                is_bittorrent,
                sender,
            } => {
                self.handle_connection_details(session, task, active, is_bittorrent, sender);
                None
            }
        }
    }

    async fn handle_force_refresh(&mut self) -> Option<ConnectedExit> {
        // Drain pre-queued refresh hints before starting the authoritative
        // refresh so hints emitted prior to this call are subsumed, while hints
        // arriving during or after `refresh_all` are preserved.
        while self.ctx.notifications.try_recv().is_ok() {}
        if let Some(writer) = self.history_writer.as_ref() {
            writer.flush().await;
        }
        let result = tokio::select! {
            biased;
            () = wait_for_cancellation(self.ctx.cancellation) => return Some(ConnectedExit::Stop),
            result = refresh_all(
                self.ctx.session,
                self.ctx.store,
                self.ctx.generation,
                self.ctx.config.stopped_page_size,
                self.ctx.events,
                &self.ctx.config.history,
                self.ctx.config.profile_id,
            ) => result,
        };
        match result {
            Ok(()) => {
                self.pending_full = false;
                self.pending_tasks.clear();
                None
            }
            Err(error) => Some(ConnectedExit::Retry(error)),
        }
    }

    async fn handle_load_more_stopped(
        &mut self,
        sender: tokio::sync::oneshot::Sender<crate::StoppedHistoryState>,
    ) -> Option<ConnectedExit> {
        let history = self.ctx.store.stopped_history();
        if !history.can_load_more {
            let _ = sender.send(history);
            return None;
        }
        let offset = history.next_offset;
        let page_size = self.ctx.config.stopped_page_size;
        let result = tokio::select! {
            biased;
            () = wait_for_cancellation(self.ctx.cancellation) => return Some(ConnectedExit::Stop),
            result = self.ctx.session.refresh_stopped_page(offset, page_size) => result,
        };
        match result {
            Ok(page) => {
                persist_terminal_snapshots(
                    &self.ctx.config.history,
                    self.ctx.config.profile_id,
                    page.tasks.iter().cloned(),
                )
                .await;
                match self.ctx.store.apply_stopped_page(
                    self.ctx.generation,
                    page.offset,
                    Some(page.total),
                    page.tasks,
                ) {
                    Ok(patch) => {
                        emit_patch(self.ctx.events, patch);
                        if let Some(writer) = self.history_writer.as_ref() {
                            writer.flush().await;
                        }
                        seed_history_into_store(
                            self.ctx.store,
                            self.ctx.generation,
                            &self.ctx.config.history,
                            self.ctx.config.profile_id,
                            self.ctx.events,
                        )
                        .await;
                        let _ = sender.send(self.ctx.store.stopped_history());
                        None
                    }
                    Err(error) => Some(ConnectedExit::Retry(SyncError::store(error))),
                }
            }
            Err(error) => Some(ConnectedExit::Retry(error)),
        }
    }

    async fn handle_execute(
        &mut self,
        expected_session: ariadeck_domain::EngineSession,
        command: AppCommand,
        sender: tokio::sync::oneshot::Sender<CommandOutcome>,
    ) -> Option<ConnectedExit> {
        if expected_session != self.ctx.store.session() {
            let _ = sender.send(CommandOutcome::failure(stale_session_error()));
            return None;
        }
        let Some(gateway) = self.ctx.command_gateway else {
            let _ = sender.send(CommandOutcome::failure(unsupported_error(
                "The connected engine does not expose task commands.",
            )));
            return None;
        };
        let service = CommandService::new(
            self.ctx.config.profile_id,
            gateway.clone(),
            self.ctx.capabilities.clone(),
        );
        let task_contexts =
            build_task_contexts(self.ctx.store, self.ctx.config.profile_id, &command);
        let refresh_after_success = refresh_hint_for_command(&command);
        let custom_output_name = match &command {
            AppCommand::SetTaskOutputName(request) => {
                Some((request.task.gid, request.output_name.trim().to_owned()))
            }
            _ => None,
        };
        let remove_gids = match &command {
            AppCommand::RemoveTasks(request) | AppCommand::ForceRemoveTasks(request) => request
                .tasks
                .iter()
                .filter(|identity| identity.profile_id == self.ctx.config.profile_id)
                .map(|identity| identity.gid)
                .collect::<Vec<_>>(),
            _ => Vec::new(),
        };
        let outcome = tokio::select! {
            biased;
            () = wait_for_cancellation(self.ctx.cancellation) => return Some(ConnectedExit::Stop),
            outcome = service.execute(command, &task_contexts) => outcome,
        };
        let refresh_authoritative_state = outcome.has_successes() || outcome.has_unknown_outcome();
        if outcome.has_successes()
            && let Some((gid, output_name)) = custom_output_name
        {
            match self
                .ctx
                .store
                .set_custom_output_name(self.ctx.generation, gid, output_name)
            {
                Ok(patch) => emit_patch(self.ctx.events, patch),
                Err(error) => return Some(ConnectedExit::Retry(SyncError::store(error))),
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
                self.history_tracker.forget(&deleted);
                if let Some(writer) = self.history_writer.as_ref() {
                    writer
                        .remove_batch(
                            &self.ctx.config.history,
                            self.ctx.config.profile_id,
                            deleted.clone(),
                        )
                        .await;
                }
                match self
                    .ctx
                    .store
                    .remove_history_only(self.ctx.generation, &deleted)
                {
                    Ok(patch) => emit_patch(self.ctx.events, patch),
                    Err(error) => {
                        tracing::warn!(%error, "failed to drop history-only rows after remove");
                    }
                }
            }
        }
        if refresh_authoritative_state {
            match refresh_after_success {
                RefreshHint::Task(gid) => {
                    self.pending_tasks.insert(gid);
                }
                RefreshHint::Full => self.pending_full = true,
            }
        }
        let _ = sender.send(outcome);
        None
    }

    async fn handle_global_option<F, Fut>(
        &mut self,
        expected_session: ariadeck_domain::EngineSession,
        unsupported_message: &'static str,
        sender: tokio::sync::oneshot::Sender<Result<(), crate::ApplicationError>>,
        apply: F,
    ) -> Option<ConnectedExit>
    where
        F: FnOnce(CommandService) -> Fut,
        Fut: std::future::Future<Output = Result<(), crate::ApplicationError>>,
    {
        if expected_session != self.ctx.store.session() {
            let _ = sender.send(Err(stale_session_error()));
            return None;
        }
        let Some(gateway) = self.ctx.command_gateway else {
            let _ = sender.send(Err(unsupported_error(unsupported_message)));
            return None;
        };
        let service = CommandService::new(
            self.ctx.config.profile_id,
            gateway.clone(),
            self.ctx.capabilities.clone(),
        );
        let result = tokio::select! {
            biased;
            () = wait_for_cancellation(self.ctx.cancellation) => return Some(ConnectedExit::Stop),
            result = apply(service) => result,
        };
        let _ = sender.send(result);
        None
    }

    fn handle_task_details(
        &self,
        expected_session: ariadeck_domain::EngineSession,
        task: ariadeck_domain::TaskIdentity,
        sender: tokio::sync::oneshot::Sender<
            Result<ariadeck_domain::TaskDetails, crate::ApplicationError>,
        >,
    ) {
        if expected_session != self.ctx.store.session() {
            let _ = sender.send(Err(stale_session_error()));
            return;
        }
        if task.profile_id != self.ctx.config.profile_id {
            let _ = sender.send(Err(wrong_profile_error()));
            return;
        }
        let Some(gateway) = self.ctx.details_gateway.cloned() else {
            let _ = sender.send(Err(unsupported_error(
                "The connected engine does not expose task details.",
            )));
            return;
        };
        let mut cancel_rx = self.ctx.cancellation.clone();
        let mut session_cancel_rx = self.session_cancel_rx.clone();
        tokio::spawn(async move {
            let result = tokio::select! {
                biased;
                () = wait_for_cancellation(&mut cancel_rx) => return,
                () = wait_for_cancellation(&mut session_cancel_rx) => return,
                result = gateway.task_details(task.gid) => result.map_err(Into::into),
            };
            let _ = sender.send(result);
        });
    }

    fn handle_connection_details(
        &self,
        expected_session: ariadeck_domain::EngineSession,
        task: ariadeck_domain::TaskIdentity,
        active: bool,
        is_bittorrent: bool,
        sender: tokio::sync::oneshot::Sender<
            Result<ariadeck_domain::TaskConnectionDetails, crate::ApplicationError>,
        >,
    ) {
        if expected_session != self.ctx.store.session() {
            let _ = sender.send(Err(stale_session_error()));
            return;
        }
        if task.profile_id != self.ctx.config.profile_id {
            let _ = sender.send(Err(wrong_profile_error()));
            return;
        }
        let Some(gateway) = self.ctx.connection_gateway.cloned() else {
            let _ = sender.send(Err(unsupported_error(
                "The connected engine does not expose connection details.",
            )));
            return;
        };
        let mut cancel_rx = self.ctx.cancellation.clone();
        let mut session_cancel_rx = self.session_cancel_rx.clone();
        tokio::spawn(async move {
            let result = tokio::select! {
                biased;
                () = wait_for_cancellation(&mut cancel_rx) => return,
                () = wait_for_cancellation(&mut session_cancel_rx) => return,
                result = gateway.connection_details(task.gid, active, is_bittorrent)
                    => result.map_err(Into::into),
            };
            let _ = sender.send(result);
        });
    }

    async fn handle_notification_tick(&mut self) -> Option<ConnectedExit> {
        if !self.pending_full && self.pending_tasks.is_empty() {
            return None;
        }
        let refresh = async {
            if self.pending_full {
                self.pending_full = false;
                self.pending_tasks.clear();
                refresh_all(
                    self.ctx.session,
                    self.ctx.store,
                    self.ctx.generation,
                    self.ctx.config.stopped_page_size,
                    self.ctx.events,
                    &self.ctx.config.history,
                    self.ctx.config.profile_id,
                )
                .await
            } else {
                let gids = self.pending_tasks.drain().collect::<Vec<_>>();
                refresh_targeted(
                    self.ctx.session,
                    self.ctx.store,
                    self.ctx.generation,
                    &gids,
                    self.ctx.events,
                    &self.ctx.config.history,
                    self.ctx.config.profile_id,
                )
                .await
            }
        };
        let result = tokio::select! {
            biased;
            () = wait_for_cancellation(self.ctx.cancellation) => return Some(ConnectedExit::Stop),
            result = refresh => result,
        };
        result.err().map(ConnectedExit::Retry)
    }

    async fn handle_global_tick(&mut self) -> Option<ConnectedExit> {
        let result = tokio::select! {
            biased;
            () = wait_for_cancellation(self.ctx.cancellation) => return Some(ConnectedExit::Stop),
            result = self.ctx.session.refresh_global_stat() => result,
        };
        match result {
            Ok(stat) => {
                if let Err(error) = self
                    .ctx
                    .store
                    .record_speed_sample(self.ctx.generation, stat)
                {
                    return Some(ConnectedExit::Retry(SyncError::store(error)));
                }
                match self.ctx.store.update_global_stat(self.ctx.generation, stat) {
                    Ok(patch) => {
                        emit_global_update(self.ctx.events, patch);
                        None
                    }
                    Err(error) => Some(ConnectedExit::Retry(SyncError::store(error))),
                }
            }
            Err(error) => Some(ConnectedExit::Retry(error)),
        }
    }

    async fn handle_live_tick(&mut self) -> Option<ConnectedExit> {
        let result = tokio::select! {
            biased;
            () = wait_for_cancellation(self.ctx.cancellation) => return Some(ConnectedExit::Stop),
            result = self.ctx.session.refresh_live() => result,
        };
        match result {
            Ok(live) => {
                match self
                    .ctx
                    .store
                    .reconcile_live(self.ctx.generation, live.active, live.waiting)
                {
                    Ok(patch) => {
                        emit_patch(self.ctx.events, patch);
                        None
                    }
                    Err(error) => Some(ConnectedExit::Retry(SyncError::store(error))),
                }
            }
            Err(error) => Some(ConnectedExit::Retry(error)),
        }
    }

    async fn handle_stopped_tick(&mut self) -> Option<ConnectedExit> {
        // Refresh every already-loaded contiguous page so a prior
        // "Load more" request is not discarded by the periodic poll
        // (HISTORY-001). The first page also updates the authoritative
        // total from aria2's numStoppedTotal.
        let page_size = self.ctx.config.stopped_page_size.max(1);
        let mut offset = 0usize;
        let loaded_through = self.ctx.store.next_stopped_offset().max(page_size as usize);
        loop {
            let result = tokio::select! {
                biased;
                () = wait_for_cancellation(self.ctx.cancellation) => return Some(ConnectedExit::Stop),
                result = self.ctx.session.refresh_stopped_page(offset, page_size) => result,
            };
            match result {
                Ok(page) => {
                    let total = page.total;
                    let batch = self
                        .history_tracker
                        .diff_changed_records(self.ctx.config.profile_id, &page.tasks);
                    if !batch.is_empty() {
                        let (records, fingerprints) = batch.into_parts();
                        if self
                            .history_writer
                            .as_ref()
                            .is_some_and(|writer| writer.try_upsert_batch(records))
                        {
                            self.history_tracker.commit_enqueued(fingerprints);
                        } else {
                            tracing::debug!(
                                "history write queue full; deferring stopped page batch"
                            );
                        }
                    }
                    match self.ctx.store.apply_stopped_page(
                        self.ctx.generation,
                        page.offset,
                        Some(total),
                        page.tasks,
                    ) {
                        Ok(patch) => emit_patch(self.ctx.events, patch),
                        Err(error) => return Some(ConnectedExit::Retry(SyncError::store(error))),
                    }
                    offset = offset.saturating_add(page_size as usize);
                    if offset >= loaded_through || offset >= total {
                        break;
                    }
                }
                Err(error) => return Some(ConnectedExit::Retry(error)),
            }
        }
        match self.ctx.store.prune_unreferenced_tasks(self.ctx.generation) {
            Ok(patch) => emit_patch(self.ctx.events, patch),
            Err(error) => return Some(ConnectedExit::Retry(SyncError::store(error))),
        }
        None
    }
}

fn build_task_contexts(
    store: &DownloadStore,
    profile_id: ProfileId,
    command: &AppCommand,
) -> HashMap<ariadeck_domain::TaskIdentity, TaskCommandContext> {
    let tasks = match command {
        AppCommand::PauseTasks(tasks)
        | AppCommand::ForcePauseTasks(tasks)
        | AppCommand::ResumeTasks(tasks) => Some(tasks.as_slice()),
        AppCommand::MoveTaskInQueue(request) => Some(std::slice::from_ref(&request.task)),
        AppCommand::RemoveTasks(request) | AppCommand::ForceRemoveTasks(request) => {
            Some(request.tasks.as_slice())
        }
        AppCommand::RetryTasks(tasks) => Some(tasks.as_slice()),
        AppCommand::SetTaskOutputName(request) => Some(std::slice::from_ref(&request.task)),
        AppCommand::SetTaskSpeedLimit(request) => Some(std::slice::from_ref(&request.task)),
        AppCommand::SetTaskConnectionPolicy(request) => Some(std::slice::from_ref(&request.task)),
        AppCommand::SetTaskOptions(request) => Some(std::slice::from_ref(&request.task)),
        _ => None,
    };
    tasks.map_or_else(HashMap::new, |tasks| {
        tasks
            .iter()
            .filter(|identity| identity.profile_id == profile_id)
            .filter_map(|identity| {
                store.task(identity.gid).map(|task| {
                    (
                        *identity,
                        TaskCommandContext {
                            status: task.status,
                            metadata: task.metadata.clone(),
                            is_history_only: store.is_history_only(identity.gid),
                        },
                    )
                })
            })
            .collect()
    })
}

fn refresh_hint_for_command(command: &AppCommand) -> RefreshHint {
    match command {
        AppCommand::SetTaskOutputName(request) => RefreshHint::Task(request.task.gid),
        AppCommand::SetTaskSpeedLimit(request) => RefreshHint::Task(request.task.gid),
        AppCommand::SetTaskConnectionPolicy(request) => RefreshHint::Task(request.task.gid),
        AppCommand::SetTaskOptions(request) => RefreshHint::Task(request.task.gid),
        _ => RefreshHint::Full,
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
    emit_patch(
        events,
        store
            .prune_unreferenced_tasks(generation)
            .map_err(SyncError::store)?,
    );
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
    emit_patch(
        events,
        store
            .prune_unreferenced_tasks(generation)
            .map_err(SyncError::store)?,
    );
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
