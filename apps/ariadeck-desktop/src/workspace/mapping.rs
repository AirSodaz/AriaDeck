//! Split from workspace.rs — mapping.

use super::*;
use ariadeck_history::SqliteHistoryStore;

pub(crate) fn add_sources_are_uris(sources: &[AddDownloadSourceView]) -> bool {
    !sources.is_empty()
        && sources
            .iter()
            .all(|source| matches!(source, AddDownloadSourceView::Uri { .. }))
}

pub(crate) fn add_source_submission_key(source: &AddDownloadSourceView) -> String {
    match source {
        AddDownloadSourceView::Uri { uri, .. } => magnet_info_hash(uri).map_or_else(
            || format!("uri:{}", normalize_add_uri_key(uri)),
            |info_hash| format!("info-hash:{}", info_hash.to_ascii_lowercase()),
        ),
        AddDownloadSourceView::MetadataFile {
            path, info_hash, ..
        } => {
            if let Some(info_hash) = info_hash {
                return format!("info-hash:{}", info_hash.to_ascii_lowercase());
            }
            let path = path.to_string_lossy().replace('\\', "/");
            let path = if cfg!(windows) {
                path.to_ascii_lowercase()
            } else {
                path
            };
            format!("metadata:{path}")
        }
    }
}

pub(crate) fn command_outcome_is_unknown(outcome: &CommandOutcome) -> bool {
    match outcome {
        CommandOutcome::PartialSuccess { failed, .. } | CommandOutcome::Failure { failed } => {
            failed
                .iter()
                .any(|failure| failure.error.code == ApplicationErrorCode::OutcomeUnknown)
        }
        CommandOutcome::Success { .. } => false,
    }
}

pub(crate) async fn reconcile_unknown_add(
    handle: &SyncHandle,
    sources: &[AddDownloadSourceView],
    known_gids: &mut Option<HashSet<Gid>>,
    unresolved: CommandOutcomeView,
) -> CommandOutcomeView {
    let Some(known) = known_gids.as_mut() else {
        return unresolved;
    };
    handle.force_refresh().await;
    let Some(snapshot) = handle
        .snapshot(ariadeck_application::TaskListQuery::default())
        .await
    else {
        return unresolved;
    };
    if snapshot.stale || !matches!(snapshot.connection_state, ConnectionState::Connected) {
        return unresolved;
    }
    if let Some(task) = find_new_matching_add_task(&snapshot.tasks, sources, known) {
        known.insert(task.gid);
        return CommandOutcomeView::Success {
            tasks: vec![TaskIdentity {
                profile_id: snapshot.session.profile_id.to_string(),
                gid: task.gid.to_string(),
            }],
        };
    }
    CommandOutcomeView::Failure(map_application_error(ApplicationError::new(
        ApplicationErrorCode::NotObserved,
        "aria2 did not report a new matching task after an authoritative refresh. This source can be submitted again safely.",
        true,
    )))
}

pub(crate) fn find_new_matching_add_task<'a>(
    tasks: &'a [DownloadTask],
    sources: &[AddDownloadSourceView],
    known_gids: &HashSet<Gid>,
) -> Option<&'a DownloadTask> {
    tasks
        .iter()
        .find(|task| !known_gids.contains(&task.gid) && task_matches_add_sources(task, sources))
}

pub(crate) fn find_matching_add_task<'a>(
    tasks: &'a [DownloadTask],
    sources: &[AddDownloadSourceView],
) -> Option<&'a DownloadTask> {
    tasks
        .iter()
        .find(|task| task_matches_add_sources(task, sources))
}

pub(crate) fn task_matches_add_sources(
    task: &DownloadTask,
    sources: &[AddDownloadSourceView],
) -> bool {
    if let Some(primary_uri) = task.metadata.primary_uri.as_deref()
        && sources
            .iter()
            .filter_map(|source| match source {
                AddDownloadSourceView::Uri { uri, .. } => Some(uri.as_str()),
                AddDownloadSourceView::MetadataFile { .. } => None,
            })
            .any(|uri| add_uris_equal(primary_uri, uri))
    {
        return true;
    }
    let Some(task_info_hash) = task.metadata.info_hash.as_deref() else {
        return false;
    };
    sources.iter().any(|source| match source {
        AddDownloadSourceView::Uri { uri, .. } => magnet_info_hash(uri)
            .is_some_and(|candidate| candidate.eq_ignore_ascii_case(task_info_hash)),
        AddDownloadSourceView::MetadataFile { info_hash, .. } => info_hash
            .as_deref()
            .is_some_and(|candidate| candidate.eq_ignore_ascii_case(task_info_hash)),
    })
}

pub(crate) fn add_uris_equal(left: &str, right: &str) -> bool {
    match (Url::parse(left.trim()), Url::parse(right.trim())) {
        (Ok(left), Ok(right)) => left == right,
        _ => left.trim() == right.trim(),
    }
}

pub(crate) fn normalize_add_uri_key(uri: &str) -> String {
    Url::parse(uri.trim()).map_or_else(|_| uri.trim().to_owned(), |parsed| parsed.to_string())
}

pub(crate) use ariadeck_domain::magnet_info_hash;

pub(crate) async fn execute_global_task_command(
    sync: Option<SyncHandle>,
    request: GlobalTaskCommandRequestView,
) -> GlobalTaskCommandResultView {
    let GlobalTaskCommandRequestView {
        request_id,
        session,
        command,
    } = request;
    let app_command = match command {
        GlobalTaskCommandView::PauseAll => AppCommand::PauseAll,
        GlobalTaskCommandView::ForcePauseAll => AppCommand::ForcePauseAll,
        GlobalTaskCommandView::ResumeAll => AppCommand::ResumeAll,
    };
    let outcome = match (sync, map_engine_session(&session)) {
        (Some(handle), Ok(engine_session)) => {
            let outcome = handle.execute(engine_session, app_command).await;
            // D-014 global command rule / D-010: a success or unknown outcome
            // forces an authoritative refresh; an unknown mutation is never
            // replayed in the same session.
            if outcome.has_successes() || outcome.has_unknown_outcome() {
                handle.force_refresh().await;
            }
            map_command_outcome(outcome)
        }
        (None, _) => CommandOutcomeView::Failure(unavailable_operation_error()),
        (Some(_), Err(error)) => CommandOutcomeView::Failure(map_application_error(error)),
    };
    GlobalTaskCommandResultView {
        request_id,
        session,
        command,
        outcome,
    }
}

pub(crate) async fn execute_task_command(
    sync: Option<SyncHandle>,
    task_file_gateway: Option<Arc<dyn TaskFileGateway>>,
    request: TaskCommandRequestView,
) -> TaskCommandResultView {
    let TaskCommandRequestView {
        request_id,
        session,
        identity,
        command,
    } = request;
    let mapped = map_engine_session(&session)
        .and_then(|engine_session| map_task_identity(&identity).map(|task| (engine_session, task)));
    let outcome = match (sync, mapped) {
        (Some(handle), Ok((engine_session, task))) => {
            let retry_baseline = if matches!(&command, TaskCommandView::Retry) {
                capture_retry_baseline(&handle, std::slice::from_ref(&task)).await
            } else {
                None
            };
            let remove_baseline = if matches!(
                &command,
                TaskCommandView::RemoveTask
                    | TaskCommandView::ForceRemoveTask
                    | TaskCommandView::RemoveTaskAndFiles
            ) {
                capture_remove_baseline(&handle, std::slice::from_ref(&task)).await
            } else {
                None
            };
            if matches!(&command, TaskCommandView::RemoveTaskAndFiles) {
                let outcome = execute_remove_with_files(
                    &handle,
                    task_file_gateway.as_deref(),
                    engine_session,
                    task,
                    remove_baseline,
                )
                .await;
                if outcome.has_successes() {
                    handle.force_refresh().await;
                }
                return TaskCommandResultView {
                    request_id,
                    session,
                    identity,
                    command,
                    outcome: map_command_outcome(outcome),
                };
            }
            let app_command = match &command {
                TaskCommandView::Pause => AppCommand::PauseTasks(vec![task]),
                TaskCommandView::ForcePause => AppCommand::ForcePauseTasks(vec![task]),
                TaskCommandView::Resume => AppCommand::ResumeTasks(vec![task]),
                TaskCommandView::MoveToQueueTop => {
                    AppCommand::MoveTaskInQueue(MoveTaskInQueueRequest {
                        task,
                        movement: QueueMove::Top,
                    })
                }
                TaskCommandView::MoveUpInQueue => {
                    AppCommand::MoveTaskInQueue(MoveTaskInQueueRequest {
                        task,
                        movement: QueueMove::Up,
                    })
                }
                TaskCommandView::MoveDownInQueue => {
                    AppCommand::MoveTaskInQueue(MoveTaskInQueueRequest {
                        task,
                        movement: QueueMove::Down,
                    })
                }
                TaskCommandView::MoveToQueueBottom => {
                    AppCommand::MoveTaskInQueue(MoveTaskInQueueRequest {
                        task,
                        movement: QueueMove::Bottom,
                    })
                }
                TaskCommandView::Retry => AppCommand::RetryTasks(vec![task]),
                TaskCommandView::SetOutputName { output_name } => {
                    AppCommand::SetTaskOutputName(SetTaskOutputNameRequest {
                        task,
                        output_name: output_name.clone(),
                    })
                }
                TaskCommandView::SetSpeedLimit {
                    download_limit,
                    upload_limit,
                } => AppCommand::SetTaskSpeedLimit(SetTaskSpeedLimitRequest {
                    task,
                    download_limit: ByteRate::new(*download_limit),
                    upload_limit: ByteRate::new(*upload_limit),
                }),
                TaskCommandView::SetConnectionPolicy {
                    max_connection_per_server,
                    split,
                    min_split_size,
                } => AppCommand::SetTaskConnectionPolicy(SetTaskConnectionPolicyRequest {
                    task,
                    policy: TaskConnectionPolicy {
                        max_connection_per_server: *max_connection_per_server,
                        split: *split,
                        min_split_size: *min_split_size,
                    },
                }),
                TaskCommandView::SetOptions {
                    seed_ratio,
                    seed_time_minutes,
                    selected_file_indices,
                } => AppCommand::SetTaskOptions(SetTaskOptionsRequest {
                    task,
                    seed_ratio: seed_ratio.clone(),
                    seed_time_minutes: seed_time_minutes
                        .as_ref()
                        .and_then(|value| value.parse().ok()),
                    selected_file_indices: selected_file_indices.clone(),
                }),
                TaskCommandView::RemoveTask => AppCommand::RemoveTasks(RemoveTasksRequest {
                    tasks: vec![task],
                    scope: TaskRemovalScope::TaskOnly,
                }),
                TaskCommandView::ForceRemoveTask => {
                    AppCommand::ForceRemoveTasks(RemoveTasksRequest {
                        tasks: vec![task],
                        scope: TaskRemovalScope::TaskOnly,
                    })
                }
                TaskCommandView::RemoveTaskAndFiles => unreachable!("handled above"),
            };
            let mut outcome = handle.execute(engine_session, app_command).await;
            if matches!(&command, TaskCommandView::Retry) {
                outcome = reconcile_unknown_retries(&handle, retry_baseline, outcome).await;
            } else if matches!(
                &command,
                TaskCommandView::RemoveTask | TaskCommandView::ForceRemoveTask
            ) {
                outcome = reconcile_unknown_removals(&handle, remove_baseline, outcome).await;
            }
            if outcome.has_successes() {
                handle.force_refresh().await;
            }
            map_command_outcome(outcome)
        }
        (None, _) => CommandOutcomeView::Failure(unavailable_operation_error()),
        (Some(_), Err(error)) => CommandOutcomeView::Failure(map_application_error(error)),
    };
    TaskCommandResultView {
        request_id,
        session,
        identity,
        command,
        outcome,
    }
}

pub(crate) async fn execute_batch_task_command(
    sync: Option<SyncHandle>,
    task_file_gateway: Option<Arc<dyn TaskFileGateway>>,
    request: BatchTaskCommandRequestView,
) -> BatchTaskCommandResultView {
    let BatchTaskCommandRequestView {
        request_id,
        session,
        identities,
        command,
    } = request;
    let mapped = map_engine_session(&session).and_then(|engine_session| {
        identities
            .iter()
            .map(map_task_identity)
            .collect::<Result<Vec<_>, _>>()
            .map(|tasks| (engine_session, tasks))
    });
    let outcome = match (sync, mapped) {
        (Some(handle), Ok((engine_session, tasks))) => {
            let retry_baseline = if command == BatchTaskCommandView::Retry {
                capture_retry_baseline(&handle, &tasks).await
            } else {
                None
            };
            let remove_baseline = if matches!(
                command,
                BatchTaskCommandView::RemoveTask
                    | BatchTaskCommandView::ForceRemoveTask
                    | BatchTaskCommandView::RemoveTaskAndFiles
            ) {
                capture_remove_baseline(&handle, &tasks).await
            } else {
                None
            };
            if command == BatchTaskCommandView::RemoveTaskAndFiles {
                let outcome = execute_batch_remove_with_files(
                    &handle,
                    task_file_gateway.as_deref(),
                    engine_session,
                    &tasks,
                    remove_baseline,
                )
                .await;
                if outcome.has_successes() {
                    handle.force_refresh().await;
                }
                return BatchTaskCommandResultView {
                    request_id,
                    session,
                    identities,
                    command,
                    outcome: map_batch_command_outcome(outcome),
                };
            }
            let app_command = match command {
                BatchTaskCommandView::Pause => AppCommand::PauseTasks(tasks),
                BatchTaskCommandView::ForcePause => AppCommand::ForcePauseTasks(tasks),
                BatchTaskCommandView::Resume => AppCommand::ResumeTasks(tasks),
                BatchTaskCommandView::Retry => AppCommand::RetryTasks(tasks),
                BatchTaskCommandView::RemoveTask => AppCommand::RemoveTasks(RemoveTasksRequest {
                    tasks,
                    scope: TaskRemovalScope::TaskOnly,
                }),
                BatchTaskCommandView::ForceRemoveTask => {
                    AppCommand::ForceRemoveTasks(RemoveTasksRequest {
                        tasks,
                        scope: TaskRemovalScope::TaskOnly,
                    })
                }
                BatchTaskCommandView::RemoveTaskAndFiles => unreachable!("handled above"),
            };
            let mut outcome = handle.execute(engine_session, app_command).await;
            if command == BatchTaskCommandView::Retry {
                outcome = reconcile_unknown_retries(&handle, retry_baseline, outcome).await;
            } else if matches!(
                command,
                BatchTaskCommandView::RemoveTask | BatchTaskCommandView::ForceRemoveTask
            ) {
                outcome = reconcile_unknown_removals(&handle, remove_baseline, outcome).await;
            }
            if outcome.has_successes() {
                handle.force_refresh().await;
            }
            map_batch_command_outcome(outcome)
        }
        (None, _) => BatchCommandOutcomeView::Failure {
            failed: vec![BatchTaskFailureView {
                identity: None,
                error: unavailable_operation_error(),
            }],
        },
        (Some(_), Err(error)) => BatchCommandOutcomeView::Failure {
            failed: vec![BatchTaskFailureView {
                identity: None,
                error: map_application_error(error),
            }],
        },
    };
    BatchTaskCommandResultView {
        request_id,
        session,
        identities,
        command,
        outcome,
    }
}

pub(crate) fn map_engine_session(
    session: &EngineSessionView,
) -> Result<EngineSession, ApplicationError> {
    let profile_id = session.profile_id.parse::<ProfileId>().map_err(|error| {
        ApplicationError::new(
            ApplicationErrorCode::Internal,
            format!("Invalid UI profile identity: {error}"),
            false,
        )
    })?;
    let session_id = session
        .session_id
        .parse::<EngineSessionId>()
        .map_err(|error| {
            ApplicationError::new(
                ApplicationErrorCode::Internal,
                format!("Invalid UI engine-session identity: {error}"),
                false,
            )
        })?;
    if session.generation == 0 {
        return Err(ApplicationError::new(
            ApplicationErrorCode::Internal,
            "The UI supplied an invalid zero session generation.",
            false,
        ));
    }
    Ok(EngineSession::new(
        profile_id,
        session_id,
        SessionGeneration::from_u64(session.generation),
    ))
}

pub(crate) fn map_task_identity(
    identity: &TaskIdentity,
) -> Result<DomainTaskIdentity, ApplicationError> {
    let profile_id = identity.profile_id.parse::<ProfileId>().map_err(|error| {
        ApplicationError::new(
            ApplicationErrorCode::Internal,
            format!("Invalid UI task profile identity: {error}"),
            false,
        )
    })?;
    let gid = identity.gid.parse::<Gid>().map_err(|error| {
        ApplicationError::new(
            ApplicationErrorCode::Internal,
            format!("Invalid UI aria2 GID: {error}"),
            false,
        )
    })?;
    Ok(DomainTaskIdentity::new(profile_id, gid))
}

pub(crate) fn map_command_outcome(outcome: CommandOutcome) -> CommandOutcomeView {
    match outcome {
        CommandOutcome::Success { succeeded } => CommandOutcomeView::Success {
            tasks: succeeded.into_iter().map(map_command_item).collect(),
        },
        CommandOutcome::PartialSuccess { succeeded, failed } => {
            if succeeded.is_empty() {
                CommandOutcomeView::Failure(
                    failed
                        .into_iter()
                        .next()
                        .map(|failure| map_application_error(failure.error))
                        .unwrap_or_else(internal_operation_error),
                )
            } else {
                CommandOutcomeView::Success {
                    tasks: succeeded.into_iter().map(map_command_item).collect(),
                }
            }
        }
        CommandOutcome::Failure { failed } => CommandOutcomeView::Failure(
            failed
                .into_iter()
                .next()
                .map(|failure| map_application_error(failure.error))
                .unwrap_or_else(internal_operation_error),
        ),
    }
}

pub(crate) fn map_batch_command_outcome(outcome: CommandOutcome) -> BatchCommandOutcomeView {
    match outcome {
        CommandOutcome::Success { succeeded } => BatchCommandOutcomeView::Success {
            succeeded: succeeded.into_iter().map(map_command_item).collect(),
        },
        CommandOutcome::PartialSuccess { succeeded, failed } => {
            BatchCommandOutcomeView::PartialSuccess {
                succeeded: succeeded.into_iter().map(map_command_item).collect(),
                failed: failed.into_iter().map(map_batch_failure).collect(),
            }
        }
        CommandOutcome::Failure { failed } => BatchCommandOutcomeView::Failure {
            failed: failed.into_iter().map(map_batch_failure).collect(),
        },
    }
}

pub(crate) fn map_batch_failure(failure: ItemFailure) -> BatchTaskFailureView {
    BatchTaskFailureView {
        identity: failure.item.map(|item| match item {
            CommandItem::Task(identity) => TaskIdentity {
                profile_id: identity.profile_id.to_string(),
                gid: identity.gid.to_string(),
            },
        }),
        error: map_application_error(failure.error),
    }
}

pub(crate) fn map_command_item(item: CommandItem) -> TaskIdentity {
    let CommandItem::Task(identity) = item;
    TaskIdentity {
        profile_id: identity.profile_id.to_string(),
        gid: identity.gid.to_string(),
    }
}

pub(crate) fn map_application_error(error: ApplicationError) -> OperationErrorView {
    OperationErrorView {
        code: error.code.as_str().into(),
        summary: error.summary,
        retryable: error.retryable,
    }
}

pub(crate) fn unavailable_operation_error() -> OperationErrorView {
    OperationErrorView {
        code: "sync.unavailable".into(),
        summary: "The synchronization coordinator is unavailable.".into(),
        retryable: false,
    }
}

pub(crate) fn internal_operation_error() -> OperationErrorView {
    OperationErrorView {
        code: "command.no_result".into(),
        summary: "The command returned no result.".into(),
        retryable: false,
    }
}

pub(crate) fn map_task_details(
    details: TaskDetails,
    connection: TaskConnectionDetails,
    path_validation: TaskPathValidationView,
) -> TaskDetailsView {
    let directory = details.directory.as_ref().map(ToString::to_string);
    let output_path = if details.files.len() == 1 {
        details.files.first().map(|file| file.path.to_string())
    } else {
        directory.clone()
    };
    let primary_source = connection
        .uris
        .first()
        .map(|source| sanitize_source_uri(&source.uri));
    TaskDetailsView {
        directory,
        primary_source,
        output_path,
        path_validation,
        info_hash: details.info_hash,
        piece_length: details.piece_length.map(|length| length.get()),
        piece_count: details.piece_count,
        trackers: details
            .trackers
            .into_iter()
            .map(|tracker| TaskTrackerView {
                tier: tracker.tier,
                uri: ariadeck_domain::redact_tracker_uri(&tracker.uri),
            })
            .collect(),
        uris: connection
            .uris
            .into_iter()
            .map(|uri| TaskUriView {
                uri: sanitize_source_uri(&uri.uri),
                status: match uri.status {
                    TaskUriStatus::Used => TaskUriStatusView::Used,
                    TaskUriStatus::Waiting => TaskUriStatusView::Waiting,
                    TaskUriStatus::Unknown => TaskUriStatusView::Unknown,
                },
            })
            .collect(),
        servers: connection
            .servers
            .into_iter()
            .map(|server| TaskServerView {
                file_index: server.file_index,
                uri: sanitize_source_uri(&server.uri),
                current_uri: sanitize_source_uri(&server.current_uri),
                download_rate: server.download_speed.get(),
            })
            .collect(),
        peers: connection
            .peers
            .into_iter()
            .map(|peer| TaskPeerView {
                address: peer.address,
                port: peer.port,
                download_rate: peer.download_speed.get(),
                upload_rate: peer.upload_speed.get(),
                seeder: peer.seeder,
            })
            .collect(),
        options: connection
            .options
            .into_iter()
            .map(|option| TaskOptionView {
                key: option.key,
                value: option.value,
                redacted: option.redacted,
            })
            .collect(),
        files: details
            .files
            .into_iter()
            .map(|file| TaskFileView {
                index: file.index,
                path: file.path.to_string(),
                length: file.length.get(),
                completed_length: file.completed_length.get(),
                selected: file.selected,
            })
            .collect(),
    }
}

pub(crate) fn create_sync_handle(
    runtime: &Runtime,
    data_dir: &Path,
    settings: &AppSettings,
    catalog: &ProfileCatalog,
) -> Result<(SyncHandle, Option<LocalEngineSupervisor>, Option<String>), String> {
    // One-shot env override for smoke tests still wins over the catalog.
    let external_endpoint = env::var("ARIADECK_RPC_URL")
        .ok()
        .filter(|endpoint| !endpoint.trim().is_empty());
    let active = catalog
        .active()
        .ok_or_else(|| "Profile catalog has no active profile.".to_owned())?;
    let remote_from_catalog = active.kind == ProfileKind::RemoteRpc;
    let is_remote = external_endpoint.is_some() || remote_from_catalog;
    let rpc_runtime = RpcRuntimeConfig::from_values(is_remote, |name| env::var(name).ok())?;
    let mut engine_startup_notice = None;
    let (endpoint, secret, local_engine, profile_id) = if let Some(endpoint) = external_endpoint {
        if endpoint.trim() != endpoint {
            return Err("ARIADECK_RPC_URL must not contain surrounding whitespace.".into());
        }
        let endpoint =
            Url::parse(&endpoint).map_err(|error| format!("Invalid ARIADECK_RPC_URL: {error}"))?;
        let profile_id = env::var("ARIADECK_PROFILE_ID")
            .ok()
            .and_then(|value| value.parse().ok())
            .unwrap_or(active.profile_id);
        let secret = env::var("ARIADECK_RPC_SECRET")
            .ok()
            .filter(|secret| !secret.is_empty())
            .map(RpcSecret::new);
        (endpoint, secret, None, profile_id)
    } else if remote_from_catalog {
        let endpoint_str = active
            .endpoint
            .as_deref()
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| "Active remote profile is missing an endpoint.".to_owned())?;
        let endpoint = Url::parse(endpoint_str)
            .map_err(|error| format!("Invalid remote profile endpoint: {error}"))?;
        let secret = load_profile_rpc_secret(active)?;
        (endpoint, secret, None, active.profile_id)
    } else {
        let config = active
            .as_local_config()
            .ok_or_else(|| "Active local profile is missing a data directory.".to_owned())?;
        // Prefer settings download directory for the live session.
        let mut config = config;
        config.download_dir = settings.download_directory.clone();
        if config.data_dir.as_os_str().is_empty() {
            config.data_dir = data_dir.to_path_buf();
        }
        // Executable resolution: env override → profile pin → managed core → discovery.
        config.executable = resolve_local_executable(data_dir, &config.executable)?;
        let process = LocalEngineSupervisor::spawn(&config)
            .map_err(|error| format!("Failed to start local aria2: {error}"))?;
        // Successful start: remember the active managed core as last working when present.
        let _ = CoreStore::new(data_dir).mark_active_as_last_working();
        let mut notices = Vec::new();
        if process.session_was_recovered() {
            let notice = match process.session_recovery_backup() {
                Some(backup) => format!(
                    "Corrupt aria2 session data was reset so downloads could start; the original was preserved at {}.",
                    backup.display()
                ),
                None => "Corrupt aria2 session data was reset so downloads could start.".to_owned(),
            };
            tracing::warn!(%notice, "local aria2 session file was recovered");
            notices.push(notice);
        }
        engine_startup_notice = (!notices.is_empty()).then(|| notices.join(" "));
        let endpoint = process.endpoint().clone();
        let secret = Some(RpcSecret::new(process.secret().to_owned()));
        (endpoint, secret, Some(process), config.profile_id)
    };

    let mut websocket = WebSocketConfig::new(endpoint.clone());
    websocket.connect_timeout = rpc_runtime.connect_timeout;
    websocket.request_timeout = rpc_runtime.request_timeout;
    websocket.allow_insecure_remote = rpc_runtime.allow_insecure_remote;
    websocket.validate().map_err(|error| error.to_string())?;
    let connector = Arc::new(RpcSyncConnector::new(websocket, secret));
    let mut coordinator = CoordinatorConfig::new(profile_id);
    coordinator.reconnect = rpc_runtime.reconnect;
    match SqliteHistoryStore::open(data_dir.join("history.sqlite")) {
        Ok(store) => {
            coordinator.history = std::sync::Arc::new(store);
        }
        Err(error) => {
            tracing::warn!(%error, "local task history is unavailable");
        }
    }
    tracing::info!(
        scheme = endpoint.scheme(),
        host = endpoint.host_str().unwrap_or("unknown"),
        port = endpoint.port_or_known_default(),
        connect_timeout_ms = rpc_runtime.connect_timeout.as_millis(),
        request_timeout_ms = rpc_runtime.request_timeout.as_millis(),
        reconnect_base_ms = rpc_runtime.reconnect.base_delay.as_millis(),
        reconnect_max_ms = rpc_runtime.reconnect.max_delay.as_millis(),
        reconnect_max_attempts = ?rpc_runtime.reconnect.max_attempts,
        "configured external aria2 RPC profile"
    );
    let _runtime_guard = runtime.enter();
    Ok((
        spawn_sync_coordinator(connector, coordinator),
        local_engine,
        engine_startup_notice,
    ))
}

pub(crate) fn map_core_registry(store: &CoreStore) -> CoreRegistryView {
    match store.list_installations() {
        Ok(installations) => {
            let registry = store.load_or_default().unwrap_or_default();
            CoreRegistryView {
                active_id: registry.active_id.map(|id| id.to_string()),
                last_working_id: registry.last_working_id.map(|id| id.to_string()),
                installations: installations
                    .into_iter()
                    .map(|core| CoreInstallationView {
                        id: core.id.to_string(),
                        version: core.version,
                        target: core.target,
                        source: match core.source {
                            CoreSource::Imported => CoreSourceView::Imported,
                            CoreSource::Linked => CoreSourceView::Linked,
                            CoreSource::Managed => CoreSourceView::Managed,
                        },
                        executable: core.executable.to_string_lossy().into_owned(),
                        features: core.features,
                        is_active: core.is_active,
                        is_last_working: core.is_last_working,
                        validated_version: core.validated_version,
                        status: match core.status {
                            CoreInstallStatus::Ready => CoreInstallStatusView::Ready,
                            CoreInstallStatus::MissingExecutable => {
                                CoreInstallStatusView::MissingExecutable
                            }
                            CoreInstallStatus::MissingManifest => {
                                CoreInstallStatusView::MissingManifest
                            }
                        },
                    })
                    .collect(),
            }
        }
        Err(error) => {
            tracing::warn!(%error, "failed to list managed aria2 cores");
            CoreRegistryView::default()
        }
    }
}

pub(crate) fn map_profile_catalog(catalog: &ProfileCatalog) -> ProfileCatalogView {
    ProfileCatalogView {
        active_profile_id: catalog.active_profile_id.to_string(),
        profiles: catalog.profiles.iter().map(map_profile_entry).collect(),
    }
}

pub(crate) fn map_profile_entry(entry: &ProfileEntry) -> ProfileEntryView {
    ProfileEntryView {
        profile_id: entry.profile_id.to_string(),
        name: entry.name.clone(),
        kind: match entry.kind {
            ProfileKind::LocalManaged => ProfileKindView::LocalManaged,
            ProfileKind::RemoteRpc => ProfileKindView::RemoteRpc,
        },
        executable: entry
            .executable
            .as_ref()
            .map(|path| path.to_string_lossy().into_owned())
            .unwrap_or_default(),
        download_dir: entry.download_dir.to_string_lossy().into_owned(),
        endpoint: entry.endpoint.clone().unwrap_or_default(),
        has_secret: entry.has_secret,
    }
}

pub(crate) fn map_profile_catalog_request(
    view: &ProfileCatalogView,
    secret_updates: &std::collections::HashMap<String, ProfileRpcSecretUpdateView>,
    existing: &ProfileCatalog,
    data_dir: &Path,
    settings: &AppSettings,
) -> Result<ProfileCatalog, String> {
    if view.profiles.is_empty() {
        return Err("At least one profile is required.".into());
    }
    let mut profiles = Vec::with_capacity(view.profiles.len());
    let mut resolved_active: Option<ProfileId> = None;
    for entry in &view.profiles {
        // Draft rows use non-UUID ids ("draft-local-N"); mint a stable id on save.
        let profile_id = entry
            .profile_id
            .parse::<ProfileId>()
            .unwrap_or_else(|_| ProfileId::new());
        if entry.profile_id == view.active_profile_id {
            resolved_active = Some(profile_id);
        }
        let name = entry.name.trim();
        if name.is_empty() {
            return Err("Profile name cannot be empty.".into());
        }
        // Preserve secret_ref from the existing catalog entry when ids match.
        let previous = entry
            .profile_id
            .parse::<ProfileId>()
            .ok()
            .and_then(|id| existing.get(id).cloned())
            .or_else(|| existing.get(profile_id).cloned());
        let mapped = match entry.kind {
            ProfileKindView::LocalManaged => {
                // Empty executable = use active managed core / discovery at spawn.
                let executable = entry.executable.trim();
                let download_dir = entry.download_dir.trim();
                let download_dir = if download_dir.is_empty() {
                    settings.download_directory.clone()
                } else {
                    PathBuf::from(download_dir)
                };
                let mut profile = ProfileEntry::local_managed(
                    profile_id,
                    name,
                    if executable.is_empty() {
                        PathBuf::new()
                    } else {
                        PathBuf::from(executable)
                    },
                    data_dir.to_path_buf(),
                    download_dir,
                );
                if executable.is_empty() {
                    profile.executable = None;
                }
                profile.has_secret = false;
                profile.secret_ref = None;
                profile
            }
            ProfileKindView::RemoteRpc => {
                let endpoint = entry.endpoint.trim();
                if endpoint.is_empty() {
                    return Err(format!("Remote profile {name} needs a ws/wss endpoint."));
                }
                let download_dir = entry.download_dir.trim();
                let download_dir = if download_dir.is_empty() {
                    settings.download_directory.clone()
                } else {
                    PathBuf::from(download_dir)
                };
                let secret_update = secret_updates
                    .get(&entry.profile_id)
                    .cloned()
                    .unwrap_or(ProfileRpcSecretUpdateView::Unchanged);
                let secret_ref = match secret_update {
                    ProfileRpcSecretUpdateView::Clear => None,
                    ProfileRpcSecretUpdateView::Set(_) => {
                        // Keep previous ref if present so we overwrite the same keyring entry;
                        // otherwise mint a new ref (applied in apply_profile_secret_updates).
                        Some(
                            previous
                                .as_ref()
                                .and_then(|profile| profile.secret_ref)
                                .unwrap_or_else(ariadeck_engine::RpcSecretRef::new),
                        )
                    }
                    ProfileRpcSecretUpdateView::Unchanged => {
                        previous.as_ref().and_then(|profile| profile.secret_ref)
                    }
                };
                let mut profile =
                    ProfileEntry::remote_rpc(profile_id, name, endpoint, download_dir, secret_ref)
                        .map_err(|error| error.to_string())?;
                // has_secret is derived from secret_ref in remote_rpc ctor.
                let _ = &mut profile;
                profile
            }
        };
        profiles.push(mapped);
    }
    let active_profile_id = resolved_active
        .or_else(|| {
            view.active_profile_id
                .parse::<ProfileId>()
                .ok()
                .filter(|id| profiles.iter().any(|profile| profile.profile_id == *id))
        })
        .or_else(|| profiles.first().map(|profile| profile.profile_id))
        .ok_or_else(|| "At least one profile is required.".to_owned())?;
    let catalog = ProfileCatalog {
        schema_version: ariadeck_engine::PROFILE_CATALOG_SCHEMA_VERSION,
        active_profile_id,
        profiles,
    };
    catalog.validate().map_err(|error| error.to_string())?;
    Ok(catalog)
}

pub(crate) fn map_settings(settings: &AppSettings) -> SettingsView {
    SettingsView {
        color_scheme: match settings.color_scheme {
            ColorScheme::System => ColorSchemeView::System,
            ColorScheme::Light => ColorSchemeView::Light,
            ColorScheme::Dark => ColorSchemeView::Dark,
        },
        language: match settings.language {
            LanguagePreference::System => LanguagePreferenceView::System,
            LanguagePreference::En => LanguagePreferenceView::English,
            LanguagePreference::ZhCn => LanguagePreferenceView::ChineseSimplified,
        },
        download_directory: settings.download_directory.to_string_lossy().into_owned(),
        download_proxy: DownloadProxySettingsView {
            mode: match settings.download_proxy.mode {
                DownloadProxyMode::Disabled => ProxyModeView::Disabled,
                DownloadProxyMode::System => ProxyModeView::System,
                DownloadProxyMode::Manual => ProxyModeView::Manual,
            },
            all_proxy: settings
                .download_proxy
                .all_proxy
                .clone()
                .unwrap_or_default(),
            http_proxy: settings
                .download_proxy
                .http_proxy
                .clone()
                .unwrap_or_default(),
            https_proxy: settings
                .download_proxy
                .https_proxy
                .clone()
                .unwrap_or_default(),
            ftp_proxy: settings
                .download_proxy
                .ftp_proxy
                .clone()
                .unwrap_or_default(),
            no_proxy: settings.download_proxy.no_proxy.clone(),
            username: settings.download_proxy.username.clone().unwrap_or_default(),
            has_password: settings.download_proxy.credential.is_some(),
            check_certificate: settings.download_proxy.check_certificate,
        },
        speed_limits: SpeedLimitSettingsView {
            download_limit: format_speed_limit_field(settings.speed_limits.download_limit),
            upload_limit: format_speed_limit_field(settings.speed_limits.upload_limit),
        },
        transfer_policy: TransferPolicySettingsView {
            max_concurrent_downloads: settings
                .transfer_policy
                .max_concurrent_downloads
                .to_string(),
            max_connection_per_server: settings
                .transfer_policy
                .max_connection_per_server
                .to_string(),
            split: settings.transfer_policy.split.to_string(),
            min_split_size: format_speed_limit_field(settings.transfer_policy.min_split_size),
            file_allocation: match settings.transfer_policy.file_allocation {
                FileAllocationSetting::None => FileAllocationView::None,
                FileAllocationSetting::Prealloc => FileAllocationView::Prealloc,
                FileAllocationSetting::Trunc => FileAllocationView::Trunc,
                FileAllocationSetting::Falloc => FileAllocationView::Falloc,
            },
            check_integrity: settings.transfer_policy.check_integrity,
        },
        notifications: NotificationSettingsView {
            volume: match settings.notifications.volume {
                NotificationVolume::Normal => NotificationVolumeView::Normal,
                NotificationVolume::Quiet => NotificationVolumeView::Quiet,
                NotificationVolume::Silent => NotificationVolumeView::Silent,
            },
            notify_on_completion: settings.notifications.notify_on_completion,
            notify_on_error: settings.notifications.notify_on_error,
            notify_on_engine_events: settings.notifications.notify_on_engine_events,
            os_notifications: settings.notifications.os_notifications,
            notify_on_low_disk: settings.notifications.notify_on_low_disk,
            low_disk_threshold_bytes: settings.notifications.low_disk_threshold_bytes,
        },
        platform: PlatformSettingsView {
            close_behavior: match settings.platform.close_behavior {
                CloseBehavior::MinimizeToTray => CloseBehaviorView::MinimizeToTray,
                CloseBehavior::Quit => CloseBehaviorView::Quit,
            },
            show_tray_icon: settings.platform.show_tray_icon,
            start_minimized_to_tray: settings.platform.start_minimized_to_tray,
        },
        categories: settings
            .categories
            .iter()
            .map(|category| DownloadCategoryView {
                id: category.id.to_string(),
                name: category.name.clone(),
                directory: category.directory.to_string_lossy().into_owned(),
                extensions: ariadeck_settings::format_category_extensions(&category.extensions),
                is_fallback: category.is_fallback,
            })
            .collect(),
        tracker_list: TrackerListSettingsView {
            enabled: settings.tracker_list.enabled,
            source: match settings.tracker_list.source {
                ariadeck_settings::TrackerListSource::Curated => TrackerListSourceView::Curated,
                ariadeck_settings::TrackerListSource::Custom => TrackerListSourceView::Custom,
            },
            custom_url: settings.tracker_list.custom_url.clone().unwrap_or_default(),
            auto_refresh: settings.tracker_list.auto_refresh,
            last_refreshed_at: settings.tracker_list.last_refreshed_at,
            list_text: settings.tracker_list.list_text.clone(),
        },
    }
}

pub(crate) fn map_settings_request(
    settings: &SettingsView,
    current: &AppSettings,
    password: ProxyPasswordUpdateView,
) -> Result<(AppSettings, ProxyPasswordUpdate), String> {
    let password = match password {
        ProxyPasswordUpdateView::Unchanged => ProxyPasswordUpdate::Unchanged,
        ProxyPasswordUpdateView::Detach => ProxyPasswordUpdate::Detach,
        ProxyPasswordUpdateView::Clear => ProxyPasswordUpdate::Clear,
        ProxyPasswordUpdateView::Set(password) => {
            let password = password.into_inner();
            if password.is_empty() {
                return Err("Proxy password must not be empty.".into());
            }
            ProxyPasswordUpdate::Set(SecretString::new(password))
        }
    };
    let credential = match &password {
        ProxyPasswordUpdate::Unchanged => current.download_proxy.credential,
        ProxyPasswordUpdate::Detach => None,
        ProxyPasswordUpdate::Clear => None,
        ProxyPasswordUpdate::Set(_) => Some(current.download_proxy.credential.unwrap_or_default()),
    };
    let mut mapped = AppSettings {
        color_scheme: match settings.color_scheme {
            ColorSchemeView::System => ColorScheme::System,
            ColorSchemeView::Light => ColorScheme::Light,
            ColorSchemeView::Dark => ColorScheme::Dark,
        },
        language: match settings.language {
            LanguagePreferenceView::System => LanguagePreference::System,
            LanguagePreferenceView::English => LanguagePreference::En,
            LanguagePreferenceView::ChineseSimplified => LanguagePreference::ZhCn,
        },
        download_directory: PathBuf::from(settings.download_directory.trim()),
        download_proxy: DownloadProxySettings {
            mode: match settings.download_proxy.mode {
                ProxyModeView::Disabled => DownloadProxyMode::Disabled,
                ProxyModeView::System => DownloadProxyMode::System,
                ProxyModeView::Manual => DownloadProxyMode::Manual,
            },
            all_proxy: trimmed_value(&settings.download_proxy.all_proxy),
            http_proxy: trimmed_value(&settings.download_proxy.http_proxy),
            https_proxy: trimmed_value(&settings.download_proxy.https_proxy),
            ftp_proxy: trimmed_value(&settings.download_proxy.ftp_proxy),
            no_proxy: settings
                .download_proxy
                .no_proxy
                .iter()
                .map(|entry| entry.trim())
                .filter(|entry| !entry.is_empty())
                .map(ToOwned::to_owned)
                .collect(),
            username: trimmed_value(&settings.download_proxy.username),
            credential,
            check_certificate: settings.download_proxy.check_certificate,
        },
        speed_limits: SpeedLimitSettings {
            download_limit: settings
                .speed_limits
                .parse_download_limit()
                .ok_or_else(|| "Download speed limit must be bytes/second or a K/M/G value (or empty for unlimited).".to_owned())?,
            upload_limit: settings
                .speed_limits
                .parse_upload_limit()
                .ok_or_else(|| "Upload speed limit must be bytes/second or a K/M/G value (or empty for unlimited).".to_owned())?,
        },
        transfer_policy: TransferPolicySettings {
            max_concurrent_downloads: settings
                .transfer_policy
                .parse_max_concurrent_downloads()
                .ok_or_else(|| "Maximum concurrent downloads must be a positive integer.".to_owned())?,
            max_connection_per_server: settings
                .transfer_policy
                .parse_max_connection_per_server()
                .ok_or_else(|| {
                    "Maximum connections per server must be an integer from 1 to 16.".to_owned()
                })?,
            split: settings
                .transfer_policy
                .parse_split()
                .ok_or_else(|| "Split count must be a positive integer.".to_owned())?,
            min_split_size: settings
                .transfer_policy
                .parse_min_split_size()
                .ok_or_else(|| {
                    "Minimum split size must be a positive byte count or K/M/G value.".to_owned()
                })?,
            file_allocation: match settings.transfer_policy.file_allocation {
                FileAllocationView::None => FileAllocationSetting::None,
                FileAllocationView::Prealloc => FileAllocationSetting::Prealloc,
                FileAllocationView::Trunc => FileAllocationSetting::Trunc,
                FileAllocationView::Falloc => FileAllocationSetting::Falloc,
            },
            check_integrity: settings.transfer_policy.check_integrity,
        },
        notifications: NotificationSettings {
            volume: match settings.notifications.volume {
                NotificationVolumeView::Normal => NotificationVolume::Normal,
                NotificationVolumeView::Quiet => NotificationVolume::Quiet,
                NotificationVolumeView::Silent => NotificationVolume::Silent,
            },
            notify_on_completion: settings.notifications.notify_on_completion,
            notify_on_error: settings.notifications.notify_on_error,
            notify_on_engine_events: settings.notifications.notify_on_engine_events,
            os_notifications: settings.notifications.os_notifications,
            notify_on_low_disk: settings.notifications.notify_on_low_disk,
            low_disk_threshold_bytes: settings.notifications.low_disk_threshold_bytes,
        },
        platform: PlatformSettings {
            close_behavior: match settings.platform.close_behavior {
                CloseBehaviorView::MinimizeToTray => CloseBehavior::MinimizeToTray,
                CloseBehaviorView::Quit => CloseBehavior::Quit,
            },
            show_tray_icon: settings.platform.show_tray_icon,
            start_minimized_to_tray: settings.platform.start_minimized_to_tray,
        },
        // List preferences are owned by the shell query path (UI-001), not the
        // settings form. Preserve whatever is currently persisted.
        ui: current.ui,
        categories: Vec::new(),
        tracker_list: ariadeck_settings::TrackerListSettings {
            enabled: settings.tracker_list.enabled,
            source: match settings.tracker_list.source {
                TrackerListSourceView::Curated => ariadeck_settings::TrackerListSource::Curated,
                TrackerListSourceView::Custom => ariadeck_settings::TrackerListSource::Custom,
            },
            custom_url: trimmed_value(&settings.tracker_list.custom_url),
            auto_refresh: settings.tracker_list.auto_refresh,
            last_refreshed_at: settings.tracker_list.last_refreshed_at,
            list_text: settings.tracker_list.list_text.clone(),
        },
    };
    let mut categories = Vec::with_capacity(settings.categories.len());
    for category in &settings.categories {
        let id = category
            .id
            .parse()
            .map_err(|_| "Invalid download category id.".to_owned())?;
        categories.push(ariadeck_settings::DownloadCategory {
            id,
            name: category.name.trim().to_owned(),
            directory: std::path::PathBuf::from(category.directory.trim()),
            extensions: ariadeck_settings::parse_category_extensions_text(&category.extensions),
            is_fallback: category.is_fallback,
        });
    }
    mapped.categories = categories;
    // General is the fixed fallback category (D-042); name match wins.
    for category in &mut mapped.categories {
        category.is_fallback = category.name.eq_ignore_ascii_case("General");
    }
    if !mapped.categories.iter().any(|c| c.is_fallback) {
        if let Some(first) = mapped.categories.first_mut() {
            first.is_fallback = true;
        }
    } else {
        let mut seen = false;
        for category in &mut mapped.categories {
            if category.is_fallback {
                if seen {
                    category.is_fallback = false;
                } else {
                    seen = true;
                }
            }
        }
    }
    ariadeck_settings::sync_download_directory_from_fallback(&mut mapped);
    mapped.validate().map_err(|error| error.to_string())?;
    Ok((mapped, password))
}

pub(crate) fn trimmed_value(value: &str) -> Option<String> {
    let value = value.trim();
    (!value.is_empty()).then(|| value.to_owned())
}

pub(crate) fn spawn_settings_persistence(
    runtime: Arc<Runtime>,
    store: JsonSettingsStore,
    profile_env_store: ProfileEnvironmentStore,
    destination_gateway: Option<Arc<dyn DownloadDestinationGateway>>,
    sync: Option<SyncHandle>,
    credential_store: Arc<dyn ProxyCredentialStore>,
) -> (
    mpsc::UnboundedSender<SettingsPersistenceRequest>,
    JoinHandle<()>,
    mpsc::UnboundedReceiver<SettingsPersistenceResult>,
) {
    let (requests, mut request_receiver) = mpsc::unbounded_channel::<SettingsPersistenceRequest>();
    let (results, result_receiver) = mpsc::unbounded_channel();
    let task = runtime.spawn(async move {
        while let Some(request) = request_receiver.recv().await {
            let result = persist_settings_request(
                store.clone(),
                profile_env_store.clone(),
                destination_gateway.clone(),
                sync.clone(),
                credential_store.clone(),
                request.clone(),
            )
            .await;
            let _ = results.send(SettingsPersistenceResult {
                request_id: request.request_id,
                settings: request.settings,
                result,
            });
        }
    });
    (requests, task, result_receiver)
}

pub(crate) async fn persist_settings_request(
    store: JsonSettingsStore,
    profile_env_store: ProfileEnvironmentStore,
    destination_gateway: Option<Arc<dyn DownloadDestinationGateway>>,
    sync: Option<SyncHandle>,
    credential_store: Arc<dyn ProxyCredentialStore>,
    request: SettingsPersistenceRequest,
) -> Result<(), String> {
    let settings_for_preflight = request.settings.clone();
    tokio::task::spawn_blocking(move || {
        preflight_settings(&settings_for_preflight, destination_gateway.as_deref())
    })
    .await
    .map_err(|error| format!("settings preflight task failed: {error}"))??;

    // Speed limits and transfer policy carry no credentials, so they are
    // applied independently of the proxy credential dance. When only those
    // engine options change we still push them to the running engine before
    // persisting, then persist and roll the engine back on a save failure so
    // disk and engine stay consistent.
    if (request.apply_speed_limit || request.apply_transfer_policy || request.apply_bt_tracker)
        && !request.apply_proxy
    {
        return apply_engine_policy_only(store, profile_env_store, sync, request).await;
    }

    if !request.apply_proxy {
        let settings = request.settings;
        let profile_id = request.active_profile_id;
        return tokio::task::spawn_blocking(move || {
            save_settings_and_profile_env(&store, &profile_env_store, profile_id, &settings)
        })
        .await
        .map_err(|error| format!("settings persistence task failed: {error}"))?;
    }

    let previous_settings = request.previous_settings.clone();
    let next_settings = request.settings.clone();
    let password_update = request.proxy_password.clone();
    let credentials = credential_store.clone();
    let (previous_password, password, mutation) = tokio::task::spawn_blocking(move || {
        let previous_password = load_proxy_password(credentials.as_ref(), &previous_settings)?;
        let (password, mutation) = apply_credential_update(
            credentials.as_ref(),
            &previous_settings,
            &next_settings,
            &password_update,
            previous_password.clone(),
        )?;
        Ok::<_, String>((previous_password, password, mutation))
    })
    .await
    .map_err(|error| format!("credential update task failed: {error}"))??;
    let Some(sync) = sync else {
        rollback_credential_async(credential_store, mutation).await?;
        return Err(
            "Download proxy settings cannot be applied because aria2 is unavailable.".into(),
        );
    };
    let Some(snapshot) = sync.snapshot(TaskListQuery::default()).await else {
        rollback_credential_async(credential_store, mutation).await?;
        return Err("Download proxy settings cannot be applied because the synchronization coordinator is unavailable.".into());
    };
    let next_proxy = match map_download_proxy_config(&request.settings, password) {
        Ok(proxy) => proxy,
        Err(error) => {
            rollback_credential_async(credential_store, mutation).await?;
            return Err(error);
        }
    };
    if let Err(error) = sync
        .apply_download_proxy(snapshot.session, next_proxy)
        .await
    {
        return match rollback_credential_async(credential_store, mutation).await {
            Ok(()) => Err(error.summary),
            Err(rollback) => Err(format!(
                "{} Credential rollback also failed: {rollback}",
                error.summary
            )),
        };
    }

    if request.apply_speed_limit
        && let Err(error) = sync
            .apply_speed_limit(snapshot.session, map_speed_limit_config(&request.settings))
            .await
    {
        // Roll the proxy and credential mutation back so the engine matches the
        // still-unchanged persisted settings.
        let rollback_proxy =
            map_download_proxy_config_or_clear(&request.previous_settings, previous_password);
        let engine_rollback = sync
            .apply_download_proxy(snapshot.session, rollback_proxy)
            .await
            .err()
            .map(|error| error.summary);
        let credential_rollback = rollback_credential_async(credential_store, mutation)
            .await
            .err();
        let mut summary = error.summary;
        if let Some(error) = engine_rollback {
            summary.push_str(&format!(" Proxy rollback also failed: {error}"));
        }
        if let Some(error) = credential_rollback {
            summary.push_str(&format!(" Credential rollback also failed: {error}"));
        }
        return Err(summary);
    }

    if request.apply_transfer_policy
        && let Err(error) = sync
            .apply_transfer_policy(
                snapshot.session,
                map_transfer_policy_config(&request.settings),
            )
            .await
    {
        let rollback_proxy =
            map_download_proxy_config_or_clear(&request.previous_settings, previous_password);
        let proxy_rollback = sync
            .apply_download_proxy(snapshot.session, rollback_proxy)
            .await
            .err()
            .map(|error| error.summary);
        let speed_rollback = if request.apply_speed_limit {
            sync.apply_speed_limit(
                snapshot.session,
                map_speed_limit_config(&request.previous_settings),
            )
            .await
            .err()
            .map(|error| error.summary)
        } else {
            None
        };
        let credential_rollback = rollback_credential_async(credential_store, mutation)
            .await
            .err();
        let mut summary = error.summary;
        if let Some(error) = proxy_rollback {
            summary.push_str(&format!(" Proxy rollback also failed: {error}"));
        }
        if let Some(error) = speed_rollback {
            summary.push_str(&format!(" Speed-limit rollback also failed: {error}"));
        }
        if let Some(error) = credential_rollback {
            summary.push_str(&format!(" Credential rollback also failed: {error}"));
        }
        return Err(summary);
    }

    if request.apply_bt_tracker
        && let Err(error) = sync
            .apply_bt_tracker(snapshot.session, map_bt_tracker_list(&request.settings))
            .await
    {
        let rollback_proxy =
            map_download_proxy_config_or_clear(&request.previous_settings, previous_password);
        let proxy_rollback = sync
            .apply_download_proxy(snapshot.session, rollback_proxy)
            .await
            .err()
            .map(|error| error.summary);
        let speed_rollback = if request.apply_speed_limit {
            sync.apply_speed_limit(
                snapshot.session,
                map_speed_limit_config(&request.previous_settings),
            )
            .await
            .err()
            .map(|error| error.summary)
        } else {
            None
        };
        let policy_rollback = if request.apply_transfer_policy {
            sync.apply_transfer_policy(
                snapshot.session,
                map_transfer_policy_config(&request.previous_settings),
            )
            .await
            .err()
            .map(|error| error.summary)
        } else {
            None
        };
        let credential_rollback = rollback_credential_async(credential_store, mutation)
            .await
            .err();
        let mut summary = error.summary;
        if let Some(error) = proxy_rollback {
            summary.push_str(&format!(" Proxy rollback also failed: {error}"));
        }
        if let Some(error) = speed_rollback {
            summary.push_str(&format!(" Speed-limit rollback also failed: {error}"));
        }
        if let Some(error) = policy_rollback {
            summary.push_str(&format!(" Transfer-policy rollback also failed: {error}"));
        }
        if let Some(error) = credential_rollback {
            summary.push_str(&format!(" Credential rollback also failed: {error}"));
        }
        return Err(summary);
    }

    let settings_to_save = request.settings.clone();
    let save_store = store.clone();
    let env_store = profile_env_store.clone();
    let profile_id = request.active_profile_id;
    if let Err(error) = tokio::task::spawn_blocking(move || {
        save_settings_and_profile_env(&save_store, &env_store, profile_id, &settings_to_save)
    })
    .await
    .map_err(|error| format!("settings persistence task failed: {error}"))?
    {
        let rollback_proxy =
            map_download_proxy_config_or_clear(&request.previous_settings, previous_password);
        let engine_rollback = sync
            .apply_download_proxy(snapshot.session, rollback_proxy)
            .await
            .err()
            .map(|error| error.summary);
        let speed_rollback = if request.apply_speed_limit {
            sync.apply_speed_limit(
                snapshot.session,
                map_speed_limit_config(&request.previous_settings),
            )
            .await
            .err()
            .map(|error| error.summary)
        } else {
            None
        };
        let policy_rollback = if request.apply_transfer_policy {
            sync.apply_transfer_policy(
                snapshot.session,
                map_transfer_policy_config(&request.previous_settings),
            )
            .await
            .err()
            .map(|error| error.summary)
        } else {
            None
        };
        let tracker_rollback = if request.apply_bt_tracker {
            sync.apply_bt_tracker(
                snapshot.session,
                map_bt_tracker_list(&request.previous_settings),
            )
            .await
            .err()
            .map(|error| error.summary)
        } else {
            None
        };
        let credential_rollback = rollback_credential_async(credential_store, mutation)
            .await
            .err();
        let mut summary = format!("Failed to persist proxy settings: {error}");
        if let Some(error) = engine_rollback {
            summary.push_str(&format!(" Engine rollback also failed: {error}"));
        }
        if let Some(error) = speed_rollback {
            summary.push_str(&format!(" Speed-limit rollback also failed: {error}"));
        }
        if let Some(error) = policy_rollback {
            summary.push_str(&format!(" Transfer-policy rollback also failed: {error}"));
        }
        if let Some(error) = tracker_rollback {
            summary.push_str(&format!(" Tracker-list rollback also failed: {error}"));
        }
        if let Some(error) = credential_rollback {
            summary.push_str(&format!(" Credential rollback also failed: {error}"));
        }
        return Err(summary);
    }
    Ok(())
}

pub(crate) fn save_settings_and_profile_env(
    store: &JsonSettingsStore,
    profile_env_store: &ProfileEnvironmentStore,
    profile_id: Option<ProfileId>,
    settings: &AppSettings,
) -> Result<(), String> {
    store.save(settings).map_err(|error| error.to_string())?;
    if let Some(profile_id) = profile_id {
        let env = ProfileEnvironment::from_settings(settings);
        profile_env_store
            .save(profile_id.as_uuid(), &env)
            .map_err(|error| error.to_string())?;
    }
    Ok(())
}

/// Apply and persist a speed-limit and/or transfer-policy settings change.
///
/// Pushes the new options to the running engine first, then persists to disk
/// and rolls the engine back if persistence fails, so the engine never
/// diverges from the source-of-truth settings file.
pub(crate) async fn apply_engine_policy_only(
    store: JsonSettingsStore,
    profile_env_store: ProfileEnvironmentStore,
    sync: Option<SyncHandle>,
    request: SettingsPersistenceRequest,
) -> Result<(), String> {
    let Some(sync) = sync else {
        return Err(
            "Engine transfer settings cannot be applied because aria2 is unavailable.".into(),
        );
    };
    let Some(snapshot) = sync.snapshot(TaskListQuery::default()).await else {
        return Err(
            "Engine transfer settings cannot be applied because the synchronization coordinator is unavailable."
                .into(),
        );
    };
    if request.apply_speed_limit
        && let Err(error) = sync
            .apply_speed_limit(snapshot.session, map_speed_limit_config(&request.settings))
            .await
    {
        return Err(error.summary);
    }
    if request.apply_transfer_policy
        && let Err(error) = sync
            .apply_transfer_policy(
                snapshot.session,
                map_transfer_policy_config(&request.settings),
            )
            .await
    {
        if request.apply_speed_limit {
            let _ = sync
                .apply_speed_limit(
                    snapshot.session,
                    map_speed_limit_config(&request.previous_settings),
                )
                .await;
        }
        return Err(error.summary);
    }
    if request.apply_bt_tracker
        && let Err(error) = sync
            .apply_bt_tracker(snapshot.session, map_bt_tracker_list(&request.settings))
            .await
    {
        if request.apply_speed_limit {
            let _ = sync
                .apply_speed_limit(
                    snapshot.session,
                    map_speed_limit_config(&request.previous_settings),
                )
                .await;
        }
        if request.apply_transfer_policy {
            let _ = sync
                .apply_transfer_policy(
                    snapshot.session,
                    map_transfer_policy_config(&request.previous_settings),
                )
                .await;
        }
        return Err(error.summary);
    }

    let settings_to_save = request.settings.clone();
    let save_store = store.clone();
    let env_store = profile_env_store.clone();
    let profile_id = request.active_profile_id;
    if let Err(error) = tokio::task::spawn_blocking(move || {
        save_settings_and_profile_env(&save_store, &env_store, profile_id, &settings_to_save)
    })
    .await
    .map_err(|error| format!("settings persistence task failed: {error}"))?
    {
        let mut summary = format!("Failed to persist transfer settings: {error}");
        if request.apply_speed_limit
            && let Some(error) = sync
                .apply_speed_limit(
                    snapshot.session,
                    map_speed_limit_config(&request.previous_settings),
                )
                .await
                .err()
                .map(|error| error.summary)
        {
            summary.push_str(&format!(" Speed-limit rollback also failed: {error}"));
        }
        if request.apply_transfer_policy
            && let Some(error) = sync
                .apply_transfer_policy(
                    snapshot.session,
                    map_transfer_policy_config(&request.previous_settings),
                )
                .await
                .err()
                .map(|error| error.summary)
        {
            summary.push_str(&format!(" Transfer-policy rollback also failed: {error}"));
        }
        if request.apply_bt_tracker
            && let Some(error) = sync
                .apply_bt_tracker(
                    snapshot.session,
                    map_bt_tracker_list(&request.previous_settings),
                )
                .await
                .err()
                .map(|error| error.summary)
        {
            summary.push_str(&format!(" Tracker-list rollback also failed: {error}"));
        }
        return Err(summary);
    }
    Ok(())
}

pub(crate) fn map_ui_preferences_to_query(ui: &UiPreferences) -> WorkspaceQuery {
    WorkspaceQuery {
        filter: match ui.list_filter {
            ListFilterPreference::All => WorkspaceFilter::All,
            ListFilterPreference::Active => WorkspaceFilter::Active,
            ListFilterPreference::Waiting => WorkspaceFilter::Waiting,
            ListFilterPreference::Paused => WorkspaceFilter::Paused,
            ListFilterPreference::Completed => WorkspaceFilter::Completed,
            ListFilterPreference::Failed => WorkspaceFilter::Failed,
        },
        search: String::new(),
        sort_key: match ui.list_sort_key {
            ListSortKeyPreference::Queue => WorkspaceSortKey::Queue,
            ListSortKeyPreference::Name => WorkspaceSortKey::Name,
            ListSortKeyPreference::Status => WorkspaceSortKey::Status,
            ListSortKeyPreference::Progress => WorkspaceSortKey::Progress,
            ListSortKeyPreference::DownloadSpeed => WorkspaceSortKey::DownloadSpeed,
            ListSortKeyPreference::Size => WorkspaceSortKey::Size,
        },
        sort_direction: match ui.list_sort_direction {
            ListSortDirectionPreference::Ascending => WorkspaceSortDirection::Ascending,
            ListSortDirectionPreference::Descending => WorkspaceSortDirection::Descending,
        },
        category_id: None,
    }
}

pub(crate) fn map_query(query: &WorkspaceQuery) -> TaskListQuery {
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
        sort: DownloadSort {
            key: match query.sort_key {
                WorkspaceSortKey::Queue => SortKey::Queue,
                WorkspaceSortKey::Name => SortKey::Name,
                WorkspaceSortKey::Status => SortKey::Status,
                WorkspaceSortKey::Progress => SortKey::Progress,
                WorkspaceSortKey::DownloadSpeed => SortKey::DownloadSpeed,
                WorkspaceSortKey::Size => SortKey::Size,
            },
            direction: match query.sort_direction {
                WorkspaceSortDirection::Ascending => SortDirection::Ascending,
                WorkspaceSortDirection::Descending => SortDirection::Descending,
            },
        },
        category_id: query.category_id.clone(),
    }
}

pub(crate) fn map_snapshot(
    snapshot: StoreSnapshot,
    local_path_actions_available: bool,
) -> WorkspaceSnapshot {
    let profile_id = snapshot.session.profile_id.to_string();
    let observed_seeding_seconds = snapshot.observed_seeding_seconds;
    let category_by_gid = snapshot.category_by_gid;
    WorkspaceSnapshot {
        profile_id: profile_id.clone(),
        session_id: snapshot.session.session_id.to_string(),
        generation: snapshot.session.generation.get(),
        source_revision: snapshot.view.source_revision,
        connection: map_connection(snapshot.connection_state),
        stale: snapshot.stale,
        local_path_actions_available,
        download_rate: snapshot.global_stat.download_speed.get(),
        upload_rate: snapshot.global_stat.upload_speed.get(),
        speed_history: snapshot
            .speed_history
            .samples()
            .iter()
            .map(|sample| SpeedSampleView {
                download_rate: sample.download.get(),
                upload_rate: sample.upload.get(),
            })
            .collect(),
        counts: TaskCountsView {
            all: snapshot.counts.all,
            active: snapshot.counts.active,
            waiting: snapshot.counts.waiting,
            paused: snapshot.counts.paused,
            completed: snapshot.counts.completed,
            failed: snapshot.counts.failed,
        },
        stopped_history: StoppedHistoryView {
            loaded: snapshot.stopped_history.loaded,
            total: snapshot.stopped_history.total,
            can_load_more: snapshot.stopped_history.can_load_more,
            local_saved: snapshot.stopped_history.local_saved,
        },
        tasks: snapshot
            .tasks
            .into_iter()
            .map(|task| {
                let observed = observed_seeding_seconds.get(&task.gid).copied();
                {
                    let category_id = category_by_gid.get(&task.gid).cloned();
                    map_task(&profile_id, task, observed, category_id)
                }
            })
            .collect(),
        capabilities: map_capabilities(&snapshot.capabilities),
    }
}

pub(crate) fn map_capabilities(capabilities: &EngineCapabilities) -> EngineCapabilitiesView {
    EngineCapabilitiesView {
        version: capabilities.version.clone(),
        methods_probed: capabilities.methods_probed(),
        force_pause: capabilities.supports_force_pause(),
        force_pause_all: capabilities.supports_force_pause_all(),
        force_remove: capabilities.supports_force_remove(),
        queue_positioning: capabilities.supports_queue_positioning(),
        change_option: capabilities.supports_change_option(),
        change_global_option: capabilities.supports_change_global_option(),
        get_peers: capabilities.supports_get_peers(),
        get_servers: capabilities.supports_get_servers(),
        multicall: capabilities.supports_multicall(),
    }
}

pub(crate) fn map_connection(state: ConnectionState) -> ConnectionView {
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

pub(crate) fn map_local_engine_health(health: LocalEngineHealth) -> EngineHealthView {
    match health {
        LocalEngineHealth::Running { restarts } => EngineHealthView::Running { restarts },
        LocalEngineHealth::Restarting { attempt } => EngineHealthView::Restarting { attempt },
        LocalEngineHealth::Failed { reason, .. } => EngineHealthView::Failed { summary: reason },
    }
}

pub(crate) fn map_task(
    profile_id: &str,
    task: DownloadTask,
    observed_seeding_seconds: Option<u64>,
    category_id: Option<String>,
) -> DownloadRowView {
    let eta_seconds = (task.status != DownloadStatus::Seeding)
        .then(|| {
            TaskProgress::new(task.completed_length, task.total_length)
                .eta(task.download_speed)
                .map(|duration| duration.as_secs())
        })
        .flatten();
    let primary_source = task
        .metadata
        .primary_uri
        .as_deref()
        .map(ariadeck_domain::redact_source_uri);
    let directory = task.metadata.directory.as_ref().map(ToString::to_string);
    DownloadRowView {
        identity: TaskIdentity {
            profile_id: profile_id.into(),
            gid: task.gid.to_string(),
        },
        display_name: task.display_name,
        name_state: match task.name_state {
            ariadeck_domain::TaskNameState::Resolving => TaskNameStateView::Resolving,
            ariadeck_domain::TaskNameState::Resolved => TaskNameStateView::Resolved,
            ariadeck_domain::TaskNameState::Custom => TaskNameStateView::Custom,
        },
        source_kind: match task.metadata.source_kind {
            ariadeck_domain::TaskSourceKind::Unknown => TaskSourceKindView::Unknown,
            ariadeck_domain::TaskSourceKind::DirectUri => TaskSourceKindView::DirectUri,
            ariadeck_domain::TaskSourceKind::Magnet => TaskSourceKindView::Magnet,
            ariadeck_domain::TaskSourceKind::BitTorrent => TaskSourceKindView::BitTorrent,
            ariadeck_domain::TaskSourceKind::Metalink => TaskSourceKindView::Metalink,
        },
        primary_source,
        directory,
        category_id,
        followed_by: task
            .metadata
            .followed_by
            .into_iter()
            .map(|gid| gid.to_string())
            .collect(),
        belongs_to: task.metadata.belongs_to.map(|gid| gid.to_string()),
        status: match task.status {
            DownloadStatus::Active => TaskStatusView::Active,
            DownloadStatus::Seeding => TaskStatusView::Seeding,
            DownloadStatus::Waiting => TaskStatusView::Waiting,
            DownloadStatus::Paused => TaskStatusView::Paused,
            DownloadStatus::Complete => TaskStatusView::Complete,
            DownloadStatus::Error => TaskStatusView::Failed,
            DownloadStatus::Removed => TaskStatusView::Removed,
            DownloadStatus::Verifying => TaskStatusView::Verifying,
            DownloadStatus::Unknown => TaskStatusView::Unknown,
        },
        error: task.error.map(classify_task_error),
        total_bytes: task.total_length.get(),
        completed_bytes: task.completed_length.get(),
        uploaded_bytes: task.upload_length.get(),
        download_rate: task.download_speed.get(),
        upload_rate: task.upload_speed.get(),
        eta_seconds,
        observed_seeding_seconds,
        revision: task.revision,
    }
}

pub(crate) fn classify_task_error(error: ariadeck_domain::TaskError) -> TaskErrorView {
    let message = error.message.trim();
    let normalized = message.to_ascii_lowercase();
    let summary = if normalized.contains("permission denied")
        || normalized.contains("access is denied")
        || normalized.contains("access denied")
    {
        "Permission denied. Check the download directory and file permissions.".into()
    } else if normalized.contains("file name too long")
        || normalized.contains("filename too long")
        || normalized.contains("path too long")
        || normalized.contains("error 206")
    {
        "The output path is too long. Choose a shorter directory or filename.".into()
    } else {
        match error.code {
            Some(9) => "Not enough disk space in the download directory.".into(),
            Some(10) => "The downloaded piece length does not match the expected metadata.".into(),
            Some(11) => "Output conflict: another task is downloading the same file.".into(),
            Some(12) => "Output conflict: the same Torrent is already downloading.".into(),
            Some(13) => "Output conflict: the destination file already exists.".into(),
            Some(14) => {
                "aria2 could not rename the output file. Check the path and permissions.".into()
            }
            Some(15) => {
                "aria2 could not open the output file. Check that it exists and is accessible."
                    .into()
            }
            Some(16) => {
                "aria2 could not create the output file. Check the directory and permissions."
                    .into()
            }
            Some(17) => "A filesystem input/output error interrupted the download.".into(),
            Some(18) => "aria2 could not create the download directory.".into(),
            _ if !message.is_empty() => message.into(),
            Some(code) => format!("aria2 reported error code {code}."),
            None => "aria2 reported an unspecified download error.".into(),
        }
    };
    TaskErrorView {
        code: error.code,
        summary,
        details: (!message.is_empty()).then(|| message.to_owned()),
    }
}

pub(crate) fn sanitize_source_uri(uri: &str) -> String {
    ariadeck_domain::redact_source_uri(uri)
}
