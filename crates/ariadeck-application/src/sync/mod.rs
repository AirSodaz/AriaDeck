mod backoff;
mod connected;
mod handle;
mod history_sync;
mod offline;
mod types;

#[cfg(test)]
mod tests;

pub use handle::SyncHandle;
pub use types::{
    ActivityMode, ConnectedSyncSession, CoordinatorConfig, DownloadSyncConnector,
    DownloadSyncSession, EngineCapabilities, InitialSyncSnapshot, LiveSyncSnapshot, PollIntervals,
    ReconnectPolicy, RefreshHint, RefreshPolicy, StoppedPage, StoreSnapshot, SyncError,
    SyncErrorKind, SyncEvent,
};

use std::{sync::Arc, time::Instant};

use ariadeck_domain::{ConnectionState, EngineSession, EngineSessionId, SessionGeneration};
use tokio::sync::{broadcast, mpsc, watch};

use crate::DownloadStore;

use self::{
    backoff::{ReconnectBackoff, backoff_seed, reconnect_limit_reached},
    connected::{
        ConnectedContext, ConnectedExit, emit_error, emit_patch, run_connected, set_state,
    },
    handle::{Control, connection_failure},
    history_sync::{apply_initial_snapshot, seed_initial_history_from_disk},
    offline::{
        OfflineContext, UnavailableControlDisposition, cancellation_requested,
        handle_connection_failure, handle_unavailable_control, wait_for_cancellation,
        wait_for_manual_retry, wait_for_retry_delay,
    },
    types::ConnectedSessionParts,
};

pub fn spawn_sync_coordinator(
    connector: Arc<dyn DownloadSyncConnector>,
    config: CoordinatorConfig,
) -> SyncHandle {
    let (commands, command_rx) = mpsc::channel(64);
    let (events, _) = broadcast::channel(256);
    let (cancellation, cancellation_rx) = watch::channel(false);
    let (completion_tx, completion) = watch::channel(false);
    tokio::spawn({
        let events = events.clone();
        async move {
            run_coordinator(connector, config, command_rx, events, cancellation_rx).await;
            let _ = completion_tx.send(true);
        }
    });
    SyncHandle {
        commands,
        events,
        cancellation,
        completion,
    }
}

struct FailureRecovery<'a> {
    events: &'a broadcast::Sender<SyncEvent>,
    state: &'a mut ConnectionState,
    store: &'a mut DownloadStore,
    generation: SessionGeneration,
    backoff: &'a mut ReconnectBackoff,
    commands: &'a mut mpsc::Receiver<Control>,
    activity: &'a mut ActivityMode,
    cancellation: &'a mut watch::Receiver<bool>,
    config: &'a CoordinatorConfig,
}

/// Handle a connection or sync error and wait for either automatic retry or manual retry.
/// Returns `true` to attempt another cycle, or `false` when cancelled/stopped.
async fn recover_after_failure(
    ctx: FailureRecovery<'_>,
    error: &SyncError,
    failure_attempt: Option<u32>,
) -> bool {
    let retryable =
        handle_connection_failure(ctx.events, ctx.state, ctx.store, ctx.generation, error);
    let Some(failure_attempt) = failure_attempt else {
        // Non-automatic retry path (e.g., invalid capabilities during initial snapshot).
        if wait_for_manual_retry(OfflineContext {
            commands: ctx.commands,
            store: ctx.store,
            state: ctx.state,
            activity: ctx.activity,
            cancellation: ctx.cancellation,
            history: &ctx.config.history,
            profile_id: ctx.config.profile_id,
        })
        .await
        {
            ctx.backoff.reset();
            return true;
        }
        return false;
    };

    if !retryable {
        if wait_for_manual_retry(OfflineContext {
            commands: ctx.commands,
            store: ctx.store,
            state: ctx.state,
            activity: ctx.activity,
            cancellation: ctx.cancellation,
            history: &ctx.config.history,
            profile_id: ctx.config.profile_id,
        })
        .await
        {
            ctx.backoff.reset();
            return true;
        }
        return false;
    }

    if reconnect_limit_reached(ctx.config.reconnect, failure_attempt) {
        set_state(
            ctx.events,
            ctx.state,
            ConnectionState::Failed {
                reason: connection_failure(error),
            },
        );
        if wait_for_manual_retry(OfflineContext {
            commands: ctx.commands,
            store: ctx.store,
            state: ctx.state,
            activity: ctx.activity,
            cancellation: ctx.cancellation,
            history: &ctx.config.history,
            profile_id: ctx.config.profile_id,
        })
        .await
        {
            ctx.backoff.reset();
            return true;
        }
        return false;
    }

    let delay = ctx.backoff.next_delay();
    wait_for_retry_delay(
        delay,
        OfflineContext {
            commands: ctx.commands,
            store: ctx.store,
            state: ctx.state,
            activity: ctx.activity,
            cancellation: ctx.cancellation,
            history: &ctx.config.history,
            profile_id: ctx.config.profile_id,
        },
    )
    .await
}

async fn run_coordinator(
    connector: Arc<dyn DownloadSyncConnector>,
    config: CoordinatorConfig,
    mut commands: mpsc::Receiver<Control>,
    events: broadcast::Sender<SyncEvent>,
    mut cancellation: watch::Receiver<bool>,
) {
    let mut generation = SessionGeneration::initial();
    let mut store = DownloadStore::new(EngineSession::new(
        config.profile_id,
        EngineSessionId::new(),
        generation,
    ));
    // Seed durable history before connect so Completed/Failed remain visible offline.
    seed_initial_history_from_disk(&mut store, generation, &config.history, config.profile_id)
        .await;
    let mut state = ConnectionState::Disconnected;
    let mut activity = ActivityMode::Foreground;
    let mut backoff = ReconnectBackoff::new(config.reconnect, backoff_seed());
    let mut first_attempt = true;

    loop {
        if cancellation_requested(&cancellation) {
            return;
        }
        if !first_attempt {
            generation = generation.next();
            let session = EngineSession::new(config.profile_id, EngineSessionId::new(), generation);
            match store.begin_session(session) {
                Ok(patch) => emit_patch(&events, patch),
                Err(error) => {
                    emit_error(&events, SyncError::store(error));
                    return;
                }
            }
        }
        let initial_attempt = first_attempt;
        first_attempt = false;

        let failure_attempt = backoff.attempt().saturating_add(1);
        set_state(
            &events,
            &mut state,
            if initial_attempt {
                ConnectionState::Connecting
            } else {
                ConnectionState::Reconnecting {
                    attempt: backoff.attempt().max(1),
                }
            },
        );

        let connect = connector.connect();
        tokio::pin!(connect);
        let connect_result = loop {
            tokio::select! {
                biased;
                () = wait_for_cancellation(&mut cancellation) => return,
                command = commands.recv() => match handle_unavailable_control(
                    command,
                    &mut store,
                    &state,
                    &mut activity,
                    false,
                    &config.history,
                    config.profile_id,
                )
                .await
                {
                    UnavailableControlDisposition::Continue
                    | UnavailableControlDisposition::RetryNow => {}
                    UnavailableControlDisposition::Stop => return,
                },
                result = &mut connect => break result,
            }
        };
        let connected = match connect_result {
            Ok(connected) => connected,
            Err(error) => {
                if recover_after_failure(
                    FailureRecovery {
                        events: &events,
                        state: &mut state,
                        store: &mut store,
                        generation,
                        backoff: &mut backoff,
                        commands: &mut commands,
                        activity: &mut activity,
                        cancellation: &mut cancellation,
                        config: &config,
                    },
                    &error,
                    Some(failure_attempt),
                )
                .await
                {
                    continue;
                }
                return;
            }
        };

        let ConnectedSessionParts {
            session,
            command_gateway,
            details_gateway,
            connection_gateway,
            notifications,
        } = connected.into_parts();
        set_state(&events, &mut state, ConnectionState::Authenticating);
        let initial_snapshot = session.initial_snapshot(config.stopped_page_size);
        tokio::pin!(initial_snapshot);
        let initial_result = loop {
            tokio::select! {
                biased;
                () = wait_for_cancellation(&mut cancellation) => {
                    session.close().await;
                    return;
                }
                command = commands.recv() => match handle_unavailable_control(
                    command,
                    &mut store,
                    &state,
                    &mut activity,
                    false,
                    &config.history,
                    config.profile_id,
                )
                .await
                {
                    UnavailableControlDisposition::Continue
                    | UnavailableControlDisposition::RetryNow => {}
                    UnavailableControlDisposition::Stop => {
                        session.close().await;
                        return;
                    }
                },
                result = &mut initial_snapshot => break result,
            }
        };
        let initial = match initial_result {
            Ok(initial) => initial,
            Err(error) => {
                session.close().await;
                if recover_after_failure(
                    FailureRecovery {
                        events: &events,
                        state: &mut state,
                        store: &mut store,
                        generation,
                        backoff: &mut backoff,
                        commands: &mut commands,
                        activity: &mut activity,
                        cancellation: &mut cancellation,
                        config: &config,
                    },
                    &error,
                    Some(failure_attempt),
                )
                .await
                {
                    continue;
                }
                return;
            }
        };

        set_state(&events, &mut state, ConnectionState::Synchronizing);
        let capabilities = match apply_initial_snapshot(
            &mut store,
            generation,
            initial,
            &events,
            &config.history,
            config.profile_id,
        )
        .await
        {
            Ok(capabilities) => capabilities,
            Err(error) => {
                session.close().await;
                if recover_after_failure(
                    FailureRecovery {
                        events: &events,
                        state: &mut state,
                        store: &mut store,
                        generation,
                        backoff: &mut backoff,
                        commands: &mut commands,
                        activity: &mut activity,
                        cancellation: &mut cancellation,
                        config: &config,
                    },
                    &error,
                    None,
                )
                .await
                {
                    continue;
                }
                return;
            }
        };
        set_state(&events, &mut state, ConnectionState::Connected);
        let connected_at = Instant::now();

        match run_connected(ConnectedContext {
            session: session.as_ref(),
            command_gateway: command_gateway.as_ref(),
            details_gateway: details_gateway.as_ref(),
            connection_gateway: connection_gateway.as_ref(),
            notifications,
            commands: &mut commands,
            store: &mut store,
            generation,
            state: &mut state,
            activity: &mut activity,
            config: &config,
            events: &events,
            cancellation: &mut cancellation,
            capabilities: &capabilities,
        })
        .await
        {
            ConnectedExit::Stop => {
                session.close().await;
                set_state(&events, &mut state, ConnectionState::Disconnected);
                return;
            }
            ConnectedExit::Retry(error) => {
                session.close().await;
                if connected_at.elapsed() >= config.reconnect.reset_after {
                    backoff.reset();
                }
                let failure_attempt = backoff.attempt().saturating_add(1);
                if !recover_after_failure(
                    FailureRecovery {
                        events: &events,
                        state: &mut state,
                        store: &mut store,
                        generation,
                        backoff: &mut backoff,
                        commands: &mut commands,
                        activity: &mut activity,
                        cancellation: &mut cancellation,
                        config: &config,
                    },
                    &error,
                    Some(failure_attempt),
                )
                .await
                {
                    return;
                }
            }
        }
    }
}
