//! Split from workspace.rs — ops.

use super::*;

pub(crate) const MAX_METADATA_FILE_BYTES: u64 = 16 * 1024 * 1024;

pub(crate) fn preview_metadata_files(
    request: AddDownloadMetadataPreviewRequestView,
) -> AddDownloadMetadataPreviewResultView {
    AddDownloadMetadataPreviewResultView {
        request_id: request.request_id,
        items: request
            .paths
            .into_iter()
            .map(preview_metadata_file)
            .collect(),
    }
}

pub(crate) fn preview_metadata_file(path: PathBuf) -> AddDownloadMetadataPreviewItemView {
    let outcome = metadata_kind_from_path(&path)
        .ok_or_else(|| {
            ApplicationError::new(
                ApplicationErrorCode::Validation,
                format!("Unsupported metadata file extension: {}", path.display()),
                false,
            )
        })
        .and_then(|kind| {
            let content = read_metadata_content(&path, kind)?;
            let preview = parse_metadata(kind, &content).map_err(|error| {
                ApplicationError::new(ApplicationErrorCode::Validation, error, false)
            })?;
            let selected_file_indices = preview.files.iter().map(|file| file.index).collect();
            Ok(AddDownloadMetadataPreviewView {
                path: path.clone(),
                kind,
                content_sha256: preview.content_sha256,
                info_hash: preview.info_hash,
                files: preview
                    .files
                    .into_iter()
                    .map(|file| AddDownloadMetadataFileView {
                        index: file.index,
                        path: file.path,
                        length: file.length,
                    })
                    .collect(),
                selected_file_indices,
            })
        });
    AddDownloadMetadataPreviewItemView {
        path,
        outcome: match outcome {
            Ok(preview) => AddDownloadMetadataPreviewOutcomeView::Ready(preview),
            Err(error) => {
                AddDownloadMetadataPreviewOutcomeView::Failed(map_application_error(error))
            }
        },
    }
}

pub(crate) fn metadata_preview_worker_failure(
    request: AddDownloadMetadataPreviewRequestView,
    error: tokio::task::JoinError,
) -> AddDownloadMetadataPreviewResultView {
    AddDownloadMetadataPreviewResultView {
        request_id: request.request_id,
        items: request
            .paths
            .into_iter()
            .map(|path| AddDownloadMetadataPreviewItemView {
                path,
                outcome: AddDownloadMetadataPreviewOutcomeView::Failed(OperationErrorView {
                    code: ApplicationErrorCode::Internal.as_str().into(),
                    summary: format!("Metadata preview worker stopped unexpectedly: {error}"),
                    retryable: true,
                }),
            })
            .collect(),
    }
}

pub(crate) async fn prepare_add_download_request(
    runtime: &tokio::runtime::Handle,
    sources: &[AddDownloadSourceView],
    destination: Option<String>,
    file_conflict: FileConflictPolicyView,
    advanced: AddDownloadAdvancedOptionsView,
) -> Result<PreparedAddDownloadRequest, ApplicationError> {
    let source = match sources {
        [
            AddDownloadSourceView::MetadataFile {
                path,
                kind,
                content_sha256,
                info_hash: _,
                selected_file_indices,
            },
        ] => {
            let path = path.clone();
            let kind = *kind;
            let content_sha256 = content_sha256.clone();
            let selected_file_indices = selected_file_indices.clone();
            runtime
                .spawn_blocking(move || {
                    read_metadata_source_with_selection(
                        &path,
                        kind,
                        &content_sha256,
                        &selected_file_indices,
                    )
                })
                .await
                .map_err(|error| {
                    ApplicationError::new(
                        ApplicationErrorCode::Internal,
                        format!("Metadata file reader stopped unexpectedly: {error}"),
                        true,
                    )
                })??
        }
        [] => {
            return Err(ApplicationError::new(
                ApplicationErrorCode::Validation,
                "At least one download source is required.",
                false,
            ));
        }
        sources
            if sources
                .iter()
                .all(|source| matches!(source, AddDownloadSourceView::Uri { .. })) =>
        {
            PreparedMetadataSource {
                source: AddDownloadSource::Uris(
                    sources
                        .iter()
                        .filter_map(|source| match source {
                            AddDownloadSourceView::Uri { uri, .. } => Some(uri.clone()),
                            AddDownloadSourceView::MetadataFile { .. } => None,
                        })
                        .collect(),
                ),
                selected_file_indices: None,
                destination_files: Vec::new(),
                required_bytes: None,
            }
        }
        _ => {
            return Err(ApplicationError::new(
                ApplicationErrorCode::Validation,
                "Torrent and Metalink files must be submitted one file at a time, separately from links.",
                false,
            ));
        }
    };
    let file_conflict = if matches!(source.source, AddDownloadSource::Uris(_)) {
        match file_conflict {
            FileConflictPolicyView::AutoRename => FileConflictPolicy::AutoRename,
            FileConflictPolicyView::Reject => FileConflictPolicy::Reject,
            FileConflictPolicyView::Overwrite => FileConflictPolicy::Overwrite,
        }
    } else {
        FileConflictPolicy::Reject
    };
    let advanced = map_add_advanced_options(advanced, &source.source)?;
    Ok(PreparedAddDownloadRequest {
        request: AddDownloadRequest {
            source: source.source,
            destination: destination.map(EnginePath::new),
            file_conflict,
            selected_file_indices: source.selected_file_indices,
            advanced,
            options: Vec::new(),
        },
        destination_files: source.destination_files,
        required_bytes: source.required_bytes,
    })
}

pub(crate) fn map_add_advanced_options(
    advanced: AddDownloadAdvancedOptionsView,
    source: &AddDownloadSource,
) -> Result<AddDownloadAdvancedOptions, ApplicationError> {
    if advanced.is_empty() {
        return Ok(AddDownloadAdvancedOptions::default());
    }
    if !matches!(source, AddDownloadSource::Uris(_)) {
        return Err(ApplicationError::new(
            ApplicationErrorCode::Validation,
            "Referer, headers, cookies, authentication, and checksum apply only to direct URL downloads.",
            false,
        ));
    }
    let headers = advanced
        .headers
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_owned)
        .collect::<Vec<_>>();
    let mapped = AddDownloadAdvancedOptions {
        referer: nonempty_optional(advanced.referer),
        user_agent: nonempty_optional(advanced.user_agent),
        headers,
        cookie: advanced
            .cookie
            .map(|value| SecretString::new(value.into_inner())),
        http_user: nonempty_optional(advanced.http_user),
        http_passwd: advanced
            .http_passwd
            .map(|value| SecretString::new(value.into_inner())),
        checksum: nonempty_optional(advanced.checksum),
    };
    mapped.validate()?;
    Ok(mapped)
}

pub(crate) fn nonempty_optional(value: String) -> Option<String> {
    let value = value.trim();
    if value.is_empty() {
        None
    } else {
        Some(value.to_owned())
    }
}

#[derive(Debug)]
pub(crate) struct PreparedAddDownloadRequest {
    pub(crate) request: AddDownloadRequest,
    pub(crate) destination_files: Vec<DownloadDestinationFile>,
    pub(crate) required_bytes: Option<u64>,
}

#[derive(Debug)]
pub(crate) struct PreparedMetadataSource {
    pub(crate) source: AddDownloadSource,
    pub(crate) selected_file_indices: Option<Vec<u32>>,
    pub(crate) destination_files: Vec<DownloadDestinationFile>,
    pub(crate) required_bytes: Option<u64>,
}

pub(crate) fn read_metadata_source_with_selection(
    path: &Path,
    requested_kind: AddDownloadMetadataKindView,
    expected_sha256: &str,
    selected_file_indices: &[u32],
) -> Result<PreparedMetadataSource, ApplicationError> {
    let content = read_metadata_content(path, requested_kind)?;
    let preview = parse_metadata(requested_kind, &content)
        .map_err(|error| ApplicationError::new(ApplicationErrorCode::Validation, error, false))?;
    if !expected_sha256.is_empty() && preview.content_sha256 != expected_sha256 {
        return Err(ApplicationError::new(
            ApplicationErrorCode::Validation,
            format!(
                "Metadata file changed after preview; review it again before adding: {}",
                path.display()
            ),
            false,
        ));
    }
    let requested_indices = selected_file_indices;
    let selected_file_indices = if requested_indices.is_empty() {
        if expected_sha256.is_empty() {
            None
        } else {
            return Err(ApplicationError::new(
                ApplicationErrorCode::Validation,
                format!("Select at least one file from {}.", path.display()),
                false,
            ));
        }
    } else {
        let all_indexes = preview
            .files
            .iter()
            .map(|file| file.index)
            .collect::<HashSet<_>>();
        if requested_indices.first() == Some(&0)
            || requested_indices.windows(2).any(|pair| pair[0] >= pair[1])
            || requested_indices
                .iter()
                .any(|index| !all_indexes.contains(index))
        {
            return Err(ApplicationError::new(
                ApplicationErrorCode::Validation,
                format!(
                    "Metadata file selection is stale or invalid: {}",
                    path.display()
                ),
                false,
            ));
        }
        (requested_indices.len() != preview.files.len()).then(|| requested_indices.to_vec())
    };
    let mut required_bytes = 0_u64;
    let mut destination_files = Vec::with_capacity(requested_indices.len());
    for file in &preview.files {
        if requested_indices.is_empty() || requested_indices.binary_search(&file.index).is_ok() {
            if let Some(length) = file.length {
                required_bytes = required_bytes.checked_add(length).ok_or_else(|| {
                    ApplicationError::new(
                        ApplicationErrorCode::Validation,
                        format!(
                            "Selected metadata file sizes exceed the supported range: {}",
                            path.display()
                        ),
                        false,
                    )
                })?;
            }
            destination_files.push(DownloadDestinationFile {
                relative_path: EnginePath::new(&file.path),
                reject_existing: true,
            });
        }
    }
    let source = match requested_kind {
        AddDownloadMetadataKindView::Torrent => AddDownloadSource::Torrent(content),
        AddDownloadMetadataKindView::Metalink => AddDownloadSource::Metalink(content),
    };
    Ok(PreparedMetadataSource {
        source,
        selected_file_indices,
        destination_files,
        required_bytes: Some(required_bytes),
    })
}

pub(crate) fn read_metadata_content(
    path: &Path,
    requested_kind: AddDownloadMetadataKindView,
) -> Result<Arc<[u8]>, ApplicationError> {
    let detected_kind = metadata_kind_from_path(path).ok_or_else(|| {
        ApplicationError::new(
            ApplicationErrorCode::Validation,
            format!("Unsupported metadata file extension: {}", path.display()),
            false,
        )
    })?;
    if detected_kind != requested_kind {
        return Err(ApplicationError::new(
            ApplicationErrorCode::Validation,
            format!(
                "Metadata file type changed before it could be read: {}",
                path.display()
            ),
            false,
        ));
    }

    let file = fs::File::open(path).map_err(|error| metadata_filesystem_error(path, error))?;
    let metadata = file
        .metadata()
        .map_err(|error| metadata_filesystem_error(path, error))?;
    if !metadata.is_file() {
        return Err(ApplicationError::new(
            ApplicationErrorCode::Validation,
            format!("Metadata source is not a regular file: {}", path.display()),
            false,
        ));
    }
    if metadata.len() == 0 {
        return Err(ApplicationError::new(
            ApplicationErrorCode::Validation,
            format!("Metadata file is empty: {}", path.display()),
            false,
        ));
    }
    if metadata.len() > MAX_METADATA_FILE_BYTES {
        return Err(metadata_file_too_large(path));
    }

    let mut content = Vec::with_capacity(metadata.len() as usize);
    file.take(MAX_METADATA_FILE_BYTES + 1)
        .read_to_end(&mut content)
        .map_err(|error| metadata_filesystem_error(path, error))?;
    if content.is_empty() {
        return Err(ApplicationError::new(
            ApplicationErrorCode::Validation,
            format!("Metadata file is empty: {}", path.display()),
            false,
        ));
    }
    if content.len() as u64 > MAX_METADATA_FILE_BYTES {
        return Err(metadata_file_too_large(path));
    }
    Ok(Arc::<[u8]>::from(content))
}

pub(crate) fn metadata_kind_from_path(path: &Path) -> Option<AddDownloadMetadataKindView> {
    let extension = path.extension()?.to_string_lossy();
    if extension.eq_ignore_ascii_case("torrent") {
        Some(AddDownloadMetadataKindView::Torrent)
    } else if extension.eq_ignore_ascii_case("metalink") || extension.eq_ignore_ascii_case("meta4")
    {
        Some(AddDownloadMetadataKindView::Metalink)
    } else {
        None
    }
}

pub(crate) fn metadata_filesystem_error(path: &Path, error: std::io::Error) -> ApplicationError {
    ApplicationError::new(
        ApplicationErrorCode::Filesystem,
        format!("Failed to read metadata file {}: {error}", path.display()),
        true,
    )
}

pub(crate) fn metadata_file_too_large(path: &Path) -> ApplicationError {
    ApplicationError::new(
        ApplicationErrorCode::Validation,
        format!(
            "Metadata file exceeds the 16 MiB upload limit: {}",
            path.display()
        ),
        false,
    )
}

#[derive(Clone)]
pub(crate) struct RemoveReconciliationBaseline {
    pub(crate) originals: HashMap<DomainTaskIdentity, DownloadTask>,
}

pub(crate) async fn capture_remove_baseline(
    handle: &SyncHandle,
    tasks: &[DomainTaskIdentity],
) -> Option<RemoveReconciliationBaseline> {
    let snapshot = handle
        .snapshot(ariadeck_application::TaskListQuery::default())
        .await?;
    if snapshot.stale || !matches!(snapshot.connection_state, ConnectionState::Connected) {
        return None;
    }
    let requested = tasks.iter().map(|task| task.gid).collect::<HashSet<_>>();
    let profile_id = snapshot.session.profile_id;
    Some(RemoveReconciliationBaseline {
        originals: snapshot
            .tasks
            .iter()
            .filter(|task| requested.contains(&task.gid))
            .map(|task| (DomainTaskIdentity::new(profile_id, task.gid), task.clone()))
            .collect(),
    })
}

pub(crate) async fn execute_remove_with_files(
    handle: &SyncHandle,
    file_gateway: Option<&dyn TaskFileGateway>,
    session: EngineSession,
    task: DomainTaskIdentity,
    baseline: Option<RemoveReconciliationBaseline>,
) -> CommandOutcome {
    let item = CommandItem::Task(task);
    let Some(file_gateway) = file_gateway else {
        return command_item_failure(
            item,
            ApplicationError::new(
                ApplicationErrorCode::Unsupported,
                "Local file removal is unavailable for this external engine profile.",
                false,
            ),
        );
    };
    let Some(original) = baseline
        .as_ref()
        .and_then(|baseline| baseline.originals.get(&task))
    else {
        return command_item_failure(
            item,
            ApplicationError::new(
                ApplicationErrorCode::Rejected,
                "The task is no longer available for a safe local-file preflight.",
                false,
            ),
        );
    };
    let details = match handle.task_details(session, task).await {
        Ok(details) => details,
        Err(error) => return command_item_failure(item, error),
    };
    let Some(directory) = details
        .directory
        .or_else(|| original.metadata.directory.clone())
    else {
        return command_item_failure(
            item,
            ApplicationError::new(
                ApplicationErrorCode::UnsafePath,
                "aria2 did not report a task directory; no local files were touched.",
                false,
            ),
        );
    };
    let file_request = TaskFileRemovalRequest {
        directory,
        files: details.files.into_iter().map(|file| file.path).collect(),
        include_control_files: original.status != DownloadStatus::Complete,
    };
    let original_status = original.status;
    let preview = match file_gateway.preflight(&file_request) {
        Ok(preview) => preview,
        Err(error) => return command_item_failure(item, error.into()),
    };
    tracing::info!(
        content_files = preview.content_files,
        control_files = preview.control_files,
        missing_paths = preview.missing_paths,
        "validated local task file removal"
    );

    if original_status.is_terminal()
        && let Err(error) = move_task_files_to_trash(file_gateway, &file_request).await
    {
        return command_item_failure(item, error);
    }

    let command = AppCommand::RemoveTasks(RemoveTasksRequest {
        tasks: vec![task],
        scope: TaskRemovalScope::TaskOnly,
    });
    let outcome = handle.execute(session, command).await;
    let outcome = reconcile_unknown_removals(handle, baseline, outcome).await;
    if !outcome.has_successes() || original_status.is_terminal() {
        return outcome;
    }
    if let Err(error) = move_task_files_to_trash(file_gateway, &file_request).await {
        return command_item_failure(item, error);
    }
    outcome
}

pub(crate) async fn execute_batch_remove_with_files(
    handle: &SyncHandle,
    file_gateway: Option<&dyn TaskFileGateway>,
    session: EngineSession,
    tasks: &[DomainTaskIdentity],
    baseline: Option<RemoveReconciliationBaseline>,
) -> CommandOutcome {
    if tasks.is_empty() {
        return CommandOutcome::failure(ApplicationError::new(
            ApplicationErrorCode::Validation,
            "At least one task must be selected.",
            false,
        ));
    }
    let mut succeeded = Vec::new();
    let mut failed = Vec::new();
    let mut seen = HashSet::new();
    for task in tasks.iter().copied().filter(|task| seen.insert(*task)) {
        let outcome =
            execute_remove_with_files(handle, file_gateway, session, task, baseline.clone()).await;
        let (item_successes, item_failures) = split_command_outcome(outcome);
        succeeded.extend(item_successes);
        failed.extend(item_failures);
    }
    finish_reconciled_outcome(succeeded, failed)
}

pub(crate) async fn move_task_files_to_trash(
    gateway: &dyn TaskFileGateway,
    request: &TaskFileRemovalRequest,
) -> Result<(), ApplicationError> {
    let report = gateway
        .move_to_trash(request)
        .await
        .map_err(ApplicationError::from)?;
    tracing::info!(
        moved_to_trash = report.moved_to_trash,
        missing_paths = report.missing_paths,
        "moved local task files to Trash"
    );
    Ok(())
}

pub(crate) async fn reconcile_unknown_removals(
    handle: &SyncHandle,
    baseline: Option<RemoveReconciliationBaseline>,
    outcome: CommandOutcome,
) -> CommandOutcome {
    if !command_outcome_is_unknown(&outcome) {
        return outcome;
    }
    let Some(baseline) = baseline else {
        return outcome;
    };
    handle.force_refresh().await;
    let Some(snapshot) = handle
        .snapshot(ariadeck_application::TaskListQuery::default())
        .await
    else {
        return outcome;
    };
    if snapshot.stale || !matches!(snapshot.connection_state, ConnectionState::Connected) {
        return outcome;
    }
    reconcile_remove_outcome(&baseline, &snapshot.tasks, outcome)
}

pub(crate) fn reconcile_remove_outcome(
    baseline: &RemoveReconciliationBaseline,
    tasks: &[DownloadTask],
    outcome: CommandOutcome,
) -> CommandOutcome {
    let (mut succeeded, failed) = split_command_outcome(outcome);
    let mut remaining_failures = Vec::new();
    for mut failure in failed {
        if failure.error.code != ApplicationErrorCode::OutcomeUnknown {
            remaining_failures.push(failure);
            continue;
        }
        let Some(CommandItem::Task(identity)) = failure.item else {
            remaining_failures.push(failure);
            continue;
        };
        let Some(original) = baseline.originals.get(&identity) else {
            remaining_failures.push(failure);
            continue;
        };
        let observed = tasks.iter().find(|task| task.gid == identity.gid);
        let removal_observed = if original.status.is_terminal() {
            observed.is_none()
        } else {
            observed.is_none_or(|task| task.status == DownloadStatus::Removed)
        };
        if removal_observed {
            succeeded.push(CommandItem::Task(identity));
        } else {
            failure.error = ApplicationError::new(
                ApplicationErrorCode::RemovalNotObserved,
                "aria2 did not report the task as removed after an authoritative refresh. The removal can be requested again safely.",
                true,
            );
            remaining_failures.push(failure);
        }
    }
    finish_reconciled_outcome(succeeded, remaining_failures)
}

pub(crate) fn command_item_failure(item: CommandItem, error: ApplicationError) -> CommandOutcome {
    CommandOutcome::Failure {
        failed: vec![ItemFailure {
            item: Some(item),
            error,
        }],
    }
}

#[derive(Clone)]
pub(crate) struct RetryReconciliationBaseline {
    pub(crate) known_gids: HashSet<Gid>,
    pub(crate) originals: HashMap<DomainTaskIdentity, DownloadTask>,
}

pub(crate) async fn capture_retry_baseline(
    handle: &SyncHandle,
    tasks: &[DomainTaskIdentity],
) -> Option<RetryReconciliationBaseline> {
    let snapshot = handle
        .snapshot(ariadeck_application::TaskListQuery::default())
        .await?;
    if snapshot.stale || !matches!(snapshot.connection_state, ConnectionState::Connected) {
        return None;
    }

    let requested = tasks.iter().map(|task| task.gid).collect::<HashSet<_>>();
    let profile_id = snapshot.session.profile_id;
    let originals = snapshot
        .tasks
        .iter()
        .filter(|task| requested.contains(&task.gid))
        .map(|task| (DomainTaskIdentity::new(profile_id, task.gid), task.clone()))
        .collect();
    Some(RetryReconciliationBaseline {
        known_gids: snapshot.tasks.iter().map(|task| task.gid).collect(),
        originals,
    })
}

pub(crate) async fn reconcile_unknown_retries(
    handle: &SyncHandle,
    baseline: Option<RetryReconciliationBaseline>,
    outcome: CommandOutcome,
) -> CommandOutcome {
    if !command_outcome_is_unknown(&outcome) {
        return outcome;
    }
    let Some(baseline) = baseline else {
        return outcome;
    };

    handle.force_refresh().await;
    let Some(snapshot) = handle
        .snapshot(ariadeck_application::TaskListQuery::default())
        .await
    else {
        return outcome;
    };
    if snapshot.stale || !matches!(snapshot.connection_state, ConnectionState::Connected) {
        return outcome;
    }

    reconcile_retry_outcome(
        baseline,
        snapshot.session.profile_id,
        &snapshot.tasks,
        outcome,
    )
}

pub(crate) fn reconcile_retry_outcome(
    baseline: RetryReconciliationBaseline,
    profile_id: ProfileId,
    candidates: &[DownloadTask],
    outcome: CommandOutcome,
) -> CommandOutcome {
    let (mut succeeded, failed) = split_command_outcome(outcome);
    let mut reserved_gids = baseline.known_gids;
    reserved_gids.extend(succeeded.iter().map(|item| match item {
        CommandItem::Task(identity) => identity.gid,
    }));
    let mut remaining_failures = Vec::new();
    for mut failure in failed {
        if failure.error.code != ApplicationErrorCode::OutcomeUnknown {
            remaining_failures.push(failure);
            continue;
        }
        let Some(CommandItem::Task(original_identity)) = failure.item else {
            remaining_failures.push(failure);
            continue;
        };
        let Some(original) = baseline.originals.get(&original_identity) else {
            remaining_failures.push(failure);
            continue;
        };
        if let Some(replacement) = candidates.iter().find(|candidate| {
            !reserved_gids.contains(&candidate.gid)
                && task_matches_retry_source(candidate, original)
        }) {
            reserved_gids.insert(replacement.gid);
            succeeded.push(CommandItem::Task(DomainTaskIdentity::new(
                profile_id,
                replacement.gid,
            )));
        } else {
            failure.error = ApplicationError::new(
                ApplicationErrorCode::RetryNotObserved,
                "aria2 did not report a new matching retry task after an authoritative refresh. The failed task can be retried again safely.",
                true,
            );
            remaining_failures.push(failure);
        }
    }
    finish_reconciled_outcome(succeeded, remaining_failures)
}

pub(crate) fn task_matches_retry_source(candidate: &DownloadTask, original: &DownloadTask) -> bool {
    if let (Some(candidate_uri), Some(original_uri)) = (
        candidate.metadata.primary_uri.as_deref(),
        original.metadata.primary_uri.as_deref(),
    ) && add_uris_equal(candidate_uri, original_uri)
    {
        return true;
    }

    let original_hash = original.metadata.info_hash.clone().or_else(|| {
        original
            .metadata
            .primary_uri
            .as_deref()
            .and_then(magnet_info_hash)
    });
    match (
        candidate.metadata.info_hash.as_deref(),
        original_hash.as_deref(),
    ) {
        (Some(candidate), Some(original)) => candidate.eq_ignore_ascii_case(original),
        _ => false,
    }
}

pub(crate) fn split_command_outcome(
    outcome: CommandOutcome,
) -> (Vec<CommandItem>, Vec<ItemFailure>) {
    match outcome {
        CommandOutcome::Success { succeeded } => (succeeded, Vec::new()),
        CommandOutcome::PartialSuccess { succeeded, failed } => (succeeded, failed),
        CommandOutcome::Failure { failed } => (Vec::new(), failed),
    }
}

pub(crate) fn finish_reconciled_outcome(
    succeeded: Vec<CommandItem>,
    failed: Vec<ItemFailure>,
) -> CommandOutcome {
    match (succeeded.is_empty(), failed.is_empty()) {
        (false, true) => CommandOutcome::Success { succeeded },
        (false, false) => CommandOutcome::PartialSuccess { succeeded, failed },
        (true, false) => CommandOutcome::Failure { failed },
        (true, true) => CommandOutcome::Failure {
            failed: vec![ItemFailure {
                item: None,
                error: ApplicationError::new(
                    ApplicationErrorCode::Internal,
                    "Retry reconciliation produced no result.",
                    false,
                ),
            }],
        },
    }
}

pub(crate) async fn execute_task_details(
    runtime: tokio::runtime::Handle,
    sync: Option<SyncHandle>,
    task_file_gateway: Option<Arc<dyn TaskFileGateway>>,
    request: TaskDetailsRequestView,
) -> TaskDetailsResultView {
    let TaskDetailsRequestView {
        request_id,
        session,
        identity,
        active,
        is_bittorrent,
    } = request;
    let mapped = map_engine_session(&session)
        .and_then(|engine_session| map_task_identity(&identity).map(|task| (engine_session, task)));
    let outcome = match (sync, mapped) {
        (Some(handle), Ok((engine_session, task))) => {
            let task_details = handle.task_details(engine_session, task);
            let connection_details =
                handle.connection_details(engine_session, task, active, is_bittorrent);
            match tokio::join!(task_details, connection_details) {
                (Ok(details), Ok(connection)) if details.gid == connection.gid => {
                    let path_validation =
                        validate_task_paths(&runtime, task_file_gateway, &details).await;
                    TaskDetailsOutcomeView::Ready(Box::new(map_task_details(
                        details,
                        connection,
                        path_validation,
                    )))
                }
                (Ok(_), Ok(_)) => TaskDetailsOutcomeView::Failed(OperationErrorView {
                    code: ApplicationErrorCode::Internal.as_str().into(),
                    summary: "aria2 returned mismatched task detail identities.".into(),
                    retryable: false,
                }),
                (Err(error), _) | (_, Err(error)) => {
                    TaskDetailsOutcomeView::Failed(map_application_error(error))
                }
            }
        }
        (None, _) => TaskDetailsOutcomeView::Failed(unavailable_operation_error()),
        (Some(_), Err(error)) => TaskDetailsOutcomeView::Failed(map_application_error(error)),
    };
    TaskDetailsResultView {
        request_id,
        session,
        identity,
        outcome,
    }
}

pub(crate) async fn validate_task_paths(
    runtime: &tokio::runtime::Handle,
    gateway: Option<Arc<dyn TaskFileGateway>>,
    details: &TaskDetails,
) -> TaskPathValidationView {
    let Some(gateway) = gateway else {
        return TaskPathValidationView::Unavailable;
    };
    let Some(directory) = details.directory.clone() else {
        return TaskPathValidationView::Warning(OperationErrorView {
            code: ApplicationErrorCode::Validation.as_str().into(),
            summary:
                "aria2 did not report a task directory, so the local path could not be validated."
                    .into(),
            retryable: true,
        });
    };
    if details.files.is_empty() {
        return TaskPathValidationView::Warning(OperationErrorView {
            code: ApplicationErrorCode::Validation.as_str().into(),
            summary:
                "aria2 did not report any task files, so the local path could not be validated."
                    .into(),
            retryable: true,
        });
    }
    let request = TaskFileRemovalRequest {
        directory,
        files: details.files.iter().map(|file| file.path.clone()).collect(),
        include_control_files: false,
    };
    match runtime
        .spawn_blocking(move || gateway.preflight(&request))
        .await
    {
        Ok(Ok(preview)) => TaskPathValidationView::Valid {
            existing_files: preview.content_files,
            missing_paths: preview.missing_paths,
        },
        Ok(Err(error)) => TaskPathValidationView::Warning(map_application_error(error.into())),
        Err(error) => TaskPathValidationView::Warning(OperationErrorView {
            code: ApplicationErrorCode::Internal.as_str().into(),
            summary: format!("Local path validation worker stopped unexpectedly: {error}"),
            retryable: true,
        }),
    }
}

pub(crate) async fn execute_task_open(
    sync: Option<SyncHandle>,
    task_file_gateway: Option<Arc<dyn TaskFileGateway>>,
    request: TaskOpenRequestView,
) -> TaskOpenResultView {
    let mapped = map_engine_session(&request.session)
        .and_then(|session| map_task_identity(&request.identity).map(|task| (session, task)));
    let outcome = match (sync, task_file_gateway, mapped) {
        (Some(handle), Some(gateway), Ok((session, task))) => {
            match handle.task_details(session, task).await {
                Ok(details) if details.gid == task.gid => {
                    let Some(directory) = details.directory else {
                        return TaskOpenResultView {
                            request_id: request.request_id,
                            session: request.session,
                            identity: request.identity,
                            target: request.target,
                            outcome: TaskOpenOutcomeView::Failure(OperationErrorView {
                                code: ApplicationErrorCode::Validation.as_str().into(),
                                summary: "aria2 did not report a task directory.".into(),
                                retryable: true,
                            }),
                        };
                    };
                    let open_request = TaskOpenRequest {
                        directory,
                        files: details.files.into_iter().map(|file| file.path).collect(),
                        target: match request.target {
                            TaskOpenTargetView::Download => TaskOpenTarget::Download,
                            TaskOpenTargetView::Folder => TaskOpenTarget::Folder,
                        },
                    };
                    match gateway.open(&open_request).await {
                        Ok(()) => TaskOpenOutcomeView::Success,
                        Err(error) => {
                            TaskOpenOutcomeView::Failure(map_application_error(error.into()))
                        }
                    }
                }
                Ok(_) => TaskOpenOutcomeView::Failure(OperationErrorView {
                    code: ApplicationErrorCode::Internal.as_str().into(),
                    summary: "aria2 returned details for a different task.".into(),
                    retryable: false,
                }),
                Err(error) => TaskOpenOutcomeView::Failure(map_application_error(error)),
            }
        }
        (_, None, _) => TaskOpenOutcomeView::Failure(OperationErrorView {
            code: ApplicationErrorCode::Unsupported.as_str().into(),
            summary: "Opening downloads is available only for the managed local engine.".into(),
            retryable: false,
        }),
        (None, _, _) => TaskOpenOutcomeView::Failure(unavailable_operation_error()),
        (Some(_), Some(_), Err(error)) => {
            TaskOpenOutcomeView::Failure(map_application_error(error))
        }
    };
    TaskOpenResultView {
        request_id: request.request_id,
        session: request.session,
        identity: request.identity,
        target: request.target,
        outcome,
    }
}
