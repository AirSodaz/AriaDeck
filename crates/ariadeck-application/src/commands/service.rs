use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

use ariadeck_domain::{
    DownloadStatus, ProfileId, SpeedLimitConfig, TaskIdentity, TaskSourceKind, TransferPolicyConfig,
};

use crate::{
    DownloadEngineGateway, EngineCapabilities, GatewayError, GatewayErrorKind, TaskRemovalTarget,
};

use super::{
    outcome::{ApplicationError, ApplicationErrorCode, CommandItem, CommandOutcome, ItemFailure},
    requests::{
        AddDownloadRequest, AddDownloadSource, AppCommand, DownloadProxyConfig, EditTaskRequest,
        FileConflictPolicy, QueueMove, SetTaskConnectionPolicyRequest, SetTaskOptionsRequest,
        SetTaskOutputNameRequest, SetTaskSpeedLimitRequest, TaskCommandContext, TaskRemovalScope,
    },
};

pub struct CommandService {
    profile_id: ProfileId,
    gateway: Arc<dyn DownloadEngineGateway>,
    capabilities: EngineCapabilities,
}

impl CommandService {
    #[must_use]
    pub fn new(
        profile_id: ProfileId,
        gateway: Arc<dyn DownloadEngineGateway>,
        capabilities: EngineCapabilities,
    ) -> Self {
        Self {
            profile_id,
            gateway,
            capabilities,
        }
    }

    /// Unit-test helper when capability gates are not under exercise.
    #[cfg(test)]
    #[must_use]
    pub fn new_unprobed(profile_id: ProfileId, gateway: Arc<dyn DownloadEngineGateway>) -> Self {
        Self::new(profile_id, gateway, EngineCapabilities::default())
    }

    fn require_method(&self, method: &str) -> Result<(), ApplicationError> {
        if self.capabilities.allows_method(method) {
            return Ok(());
        }
        Err(ApplicationError::new(
            ApplicationErrorCode::Unsupported,
            EngineCapabilities::unsupported_method_message(method),
            false,
        ))
    }

    fn resolve_live_task_context<'a>(
        &self,
        task: TaskIdentity,
        task_contexts: &'a HashMap<TaskIdentity, TaskCommandContext>,
        terminal_message: &str,
    ) -> Result<&'a TaskCommandContext, CommandOutcome> {
        let item = CommandItem::Task(task);
        if task.profile_id != self.profile_id {
            return Err(CommandOutcome::item_failure(
                item,
                ApplicationError::new(
                    ApplicationErrorCode::WrongProfile,
                    "The task belongs to a different engine profile.",
                    false,
                ),
            ));
        }
        let Some(context) = task_contexts.get(&task) else {
            return Err(CommandOutcome::item_failure(
                item,
                ApplicationError::new(
                    ApplicationErrorCode::Rejected,
                    "The task is no longer present in the current engine session.",
                    false,
                ),
            ));
        };
        if context.status.is_terminal() {
            return Err(CommandOutcome::item_failure(
                item,
                ApplicationError::new(ApplicationErrorCode::Rejected, terminal_message, false),
            ));
        }
        Ok(context)
    }

    pub async fn execute(
        &self,
        command: AppCommand,
        task_contexts: &HashMap<TaskIdentity, TaskCommandContext>,
    ) -> CommandOutcome {
        match command {
            AppCommand::AddDownload(request) => self.add_download(request).await,
            AppCommand::PauseAll => self.execute_global(GlobalTaskOperation::PauseAll).await,
            AppCommand::ForcePauseAll => {
                self.execute_global(GlobalTaskOperation::ForcePauseAll)
                    .await
            }
            AppCommand::ResumeAll => self.execute_global(GlobalTaskOperation::ResumeAll).await,
            AppCommand::PauseTasks(tasks) => {
                self.execute_batch(tasks, TaskOperation::Pause, task_contexts)
                    .await
            }
            AppCommand::ForcePauseTasks(tasks) => {
                self.execute_batch(tasks, TaskOperation::ForcePause, task_contexts)
                    .await
            }
            AppCommand::ResumeTasks(tasks) => {
                self.execute_batch(tasks, TaskOperation::Resume, task_contexts)
                    .await
            }
            AppCommand::MoveTaskInQueue(request) => {
                self.execute_batch(
                    vec![request.task],
                    TaskOperation::MoveInQueue(request.movement),
                    task_contexts,
                )
                .await
            }
            AppCommand::RetryTasks(tasks) => self.retry_tasks(tasks, task_contexts).await,
            AppCommand::SetTaskOutputName(request) => {
                self.set_task_output_name(request, task_contexts).await
            }
            AppCommand::RemoveTasks(request) => {
                self.execute_batch(
                    request.tasks,
                    TaskOperation::Remove(request.scope),
                    task_contexts,
                )
                .await
            }
            AppCommand::ForceRemoveTasks(request) => {
                self.execute_batch(
                    request.tasks,
                    TaskOperation::ForceRemove(request.scope),
                    task_contexts,
                )
                .await
            }
            AppCommand::SetTaskSpeedLimit(request) => {
                self.set_task_speed_limit(request, task_contexts).await
            }
            AppCommand::SetTaskConnectionPolicy(request) => {
                self.set_task_connection_policy(request, task_contexts)
                    .await
            }
            AppCommand::SetTaskOptions(request) => {
                self.set_task_options(request, task_contexts).await
            }
            AppCommand::EditTask(request) => self.edit_task(request, task_contexts).await,
        }
    }

    pub async fn apply_download_proxy(
        &self,
        config: &DownloadProxyConfig,
    ) -> Result<(), ApplicationError> {
        config.validate()?;
        self.require_method("aria2.changeGlobalOption")?;
        self.gateway
            .apply_download_proxy(config)
            .await
            .map_err(Into::into)
    }

    pub async fn apply_speed_limit(
        &self,
        config: &SpeedLimitConfig,
    ) -> Result<(), ApplicationError> {
        self.require_method("aria2.changeGlobalOption")?;
        self.gateway
            .apply_speed_limit(config)
            .await
            .map_err(Into::into)
    }

    pub async fn apply_transfer_policy(
        &self,
        config: &TransferPolicyConfig,
    ) -> Result<(), ApplicationError> {
        if let Err(error) = config.validate() {
            return Err(ApplicationError::new(
                ApplicationErrorCode::Validation,
                error.message(),
                false,
            ));
        }
        self.require_method("aria2.changeGlobalOption")?;
        self.gateway
            .apply_transfer_policy(config)
            .await
            .map_err(Into::into)
    }

    /// Apply or clear aria2 global `bt-tracker` (extra announce URLs).
    pub async fn apply_bt_tracker(&self, trackers: &[String]) -> Result<(), ApplicationError> {
        self.require_method("aria2.changeGlobalOption")?;
        self.gateway
            .apply_bt_tracker(trackers)
            .await
            .map_err(Into::into)
    }

    async fn change_options_gated(
        &self,
        gid: ariadeck_domain::Gid,
        options: &[(String, String)],
    ) -> Result<(), GatewayError> {
        if let Err(error) = self.require_method("aria2.changeOption") {
            return Err(GatewayError::new(
                GatewayErrorKind::Unsupported,
                error.summary,
                false,
            ));
        }
        self.gateway.change_options(gid, options).await
    }

    async fn set_task_speed_limit(
        &self,
        request: SetTaskSpeedLimitRequest,
        task_contexts: &HashMap<TaskIdentity, TaskCommandContext>,
    ) -> CommandOutcome {
        let item = CommandItem::Task(request.task);
        if let Err(outcome) = self.resolve_live_task_context(
            request.task,
            task_contexts,
            "A completed or failed task cannot change its speed limit.",
        ) {
            return outcome;
        }
        let options = [
            (
                "max-download-limit".to_owned(),
                request.download_limit.get().to_string(),
            ),
            (
                "max-upload-limit".to_owned(),
                request.upload_limit.get().to_string(),
            ),
        ];
        finish_single_task(
            item,
            self.change_options_gated(request.task.gid, &options).await,
        )
    }

    async fn set_task_connection_policy(
        &self,
        request: SetTaskConnectionPolicyRequest,
        task_contexts: &HashMap<TaskIdentity, TaskCommandContext>,
    ) -> CommandOutcome {
        let item = CommandItem::Task(request.task);
        if let Err(error) = request.policy.validate() {
            return CommandOutcome::item_failure(
                item,
                ApplicationError::new(ApplicationErrorCode::Validation, error.message(), false),
            );
        }
        if let Err(outcome) = self.resolve_live_task_context(
            request.task,
            task_contexts,
            "A completed or failed task cannot change its connection policy.",
        ) {
            return outcome;
        }
        let options = [
            (
                "max-connection-per-server".to_owned(),
                request.policy.max_connection_per_server.to_string(),
            ),
            ("split".to_owned(), request.policy.split.to_string()),
            (
                "min-split-size".to_owned(),
                request.policy.min_split_size.to_string(),
            ),
        ];
        finish_single_task(
            item,
            self.change_options_gated(request.task.gid, &options).await,
        )
    }

    async fn add_download(&self, mut request: AddDownloadRequest) -> CommandOutcome {
        if let Err(error) = request.validate() {
            return CommandOutcome::failure(error);
        }
        // Flatten typed advanced controls into the option bag before the RPC
        // boundary. Keep validation on the typed form so secrets never need to
        // re-enter UI state.
        let advanced_options = request.advanced.to_option_pairs();
        if !advanced_options.is_empty() {
            request.options.extend(advanced_options);
        }

        match self.gateway.add_download(&request).await {
            Ok(gids) if gids.is_empty() => CommandOutcome::failure(ApplicationError::new(
                ApplicationErrorCode::Internal,
                "Download engine accepted the request without returning a task ID.",
                true,
            )),
            Ok(gids) => CommandOutcome::Success {
                succeeded: gids
                    .into_iter()
                    .map(|gid| CommandItem::Task(TaskIdentity::new(self.profile_id, gid)))
                    .collect(),
            },
            Err(error) => CommandOutcome::failure(error.into()),
        }
    }

    async fn set_task_options(
        &self,
        request: SetTaskOptionsRequest,
        task_contexts: &HashMap<TaskIdentity, TaskCommandContext>,
    ) -> CommandOutcome {
        let item = CommandItem::Task(request.task);
        if let Err(error) = request.validate() {
            return CommandOutcome::item_failure(item, error);
        }
        let context = match self.resolve_live_task_context(
            request.task,
            task_contexts,
            "A completed or failed task cannot change its options.",
        ) {
            Ok(context) => context,
            Err(outcome) => return outcome,
        };
        if (request.seed_ratio.is_some() || request.seed_time_minutes.is_some())
            && !matches!(
                context.metadata.source_kind,
                TaskSourceKind::Magnet | TaskSourceKind::BitTorrent
            )
        {
            return CommandOutcome::item_failure(
                item,
                ApplicationError::new(
                    ApplicationErrorCode::Unsupported,
                    "Seed-ratio and seed-time apply only to BitTorrent tasks.",
                    false,
                ),
            );
        }
        if request.selected_file_indices.is_some()
            && !matches!(
                context.metadata.source_kind,
                TaskSourceKind::Magnet | TaskSourceKind::BitTorrent | TaskSourceKind::Metalink
            )
        {
            return CommandOutcome::item_failure(
                item,
                ApplicationError::new(
                    ApplicationErrorCode::Unsupported,
                    "File selection applies only to Torrent and Metalink tasks.",
                    false,
                ),
            );
        }
        let options = request.to_option_pairs();
        finish_single_task(
            item,
            self.change_options_gated(request.task.gid, &options).await,
        )
    }

    async fn edit_task(
        &self,
        request: EditTaskRequest,
        task_contexts: &HashMap<TaskIdentity, TaskCommandContext>,
    ) -> CommandOutcome {
        let item = CommandItem::Task(request.task);
        if let Err(error) = request.validate() {
            return CommandOutcome::item_failure(item, error);
        }
        if request.task.profile_id != self.profile_id {
            return CommandOutcome::item_failure(
                item,
                ApplicationError::new(
                    ApplicationErrorCode::WrongProfile,
                    "The task belongs to a different engine profile.",
                    false,
                ),
            );
        }
        let Some(context) = task_contexts.get(&request.task) else {
            return CommandOutcome::item_failure(
                item,
                ApplicationError::new(
                    ApplicationErrorCode::Rejected,
                    "The task is no longer present in the current engine session.",
                    false,
                ),
            );
        };

        let option_pairs = request.options.to_option_pairs();
        if context.status == DownloadStatus::Error {
            if !request.allow_error_relink_resume {
                return CommandOutcome::item_failure(
                    item,
                    ApplicationError::new(
                        ApplicationErrorCode::Rejected,
                        "A failed task requires relink resume mode to restart with updated parameters.",
                        false,
                    ),
                );
            }
            if context.metadata.source_kind != TaskSourceKind::DirectUri {
                return CommandOutcome::item_failure(
                    item,
                    ApplicationError::new(
                        ApplicationErrorCode::Unsupported,
                        "Error-task relink resume is supported only for direct URI downloads.",
                        false,
                    ),
                );
            }
            let uris: Vec<String> = match request.new_uris {
                Some(uris) => uris.into_iter().map(|u| u.trim().to_owned()).collect(),
                None => match context.metadata.primary_uri.clone() {
                    Some(uri) if uri != ariadeck_domain::REDACTED_SOURCE_PLACEHOLDER => vec![uri],
                    _ => {
                        return CommandOutcome::item_failure(
                            item,
                            ApplicationError::new(
                                ApplicationErrorCode::Validation,
                                "Provide a new download URL to relink this failed task.",
                                false,
                            ),
                        );
                    }
                },
            };
            return match self
                .gateway
                .relink_error_download(
                    request.task.gid,
                    &uris,
                    &option_pairs,
                    context.metadata.directory.as_ref(),
                )
                .await
            {
                Ok(new_gid) => CommandOutcome::Success {
                    succeeded: vec![CommandItem::Task(TaskIdentity::new(
                        self.profile_id,
                        new_gid,
                    ))],
                },
                Err(error) => CommandOutcome::item_failure(item, error.into()),
            };
        }

        if context.status.is_terminal() {
            return CommandOutcome::item_failure(
                item,
                ApplicationError::new(
                    ApplicationErrorCode::Rejected,
                    "A completed or removed task cannot be edited.",
                    false,
                ),
            );
        }

        if request.new_uris.is_some() {
            if context.metadata.source_kind != TaskSourceKind::DirectUri {
                return CommandOutcome::item_failure(
                    item,
                    ApplicationError::new(
                        ApplicationErrorCode::Unsupported,
                        "URL relinking is supported only for direct URI downloads.",
                        false,
                    ),
                );
            }
            if let Err(error) = self.require_method("aria2.changeUri") {
                return CommandOutcome::item_failure(item, error);
            }
        }
        if !option_pairs.is_empty()
            && let Err(error) = self.require_method("aria2.changeOption")
        {
            return CommandOutcome::item_failure(item, error);
        }

        let needs_pause_for_mutation = context.status == DownloadStatus::Active
            && (request.new_uris.is_some() || request.options.out.is_some());
        if needs_pause_for_mutation {
            if let Err(error) = self.require_method("aria2.pause") {
                return CommandOutcome::item_failure(item, error);
            }
            if let Err(error) = self.require_method("aria2.unpause") {
                return CommandOutcome::item_failure(item, error);
            }
            if let Err(error) = self.gateway.pause(request.task.gid).await {
                return CommandOutcome::item_failure(item, error.into());
            }
        }

        if let Some(new_uris) = request.new_uris {
            let trimmed_new: Vec<String> =
                new_uris.into_iter().map(|u| u.trim().to_owned()).collect();
            let old_uris = match self.gateway.get_uris(request.task.gid).await {
                Ok(uris) => uris,
                Err(error) => {
                    if needs_pause_for_mutation {
                        let _ = self.gateway.resume(request.task.gid).await;
                    }
                    return CommandOutcome::item_failure(item, error.into());
                }
            };
            if let Err(error) = self
                .gateway
                .change_uri(request.task.gid, 1, &old_uris, &trimmed_new, Some(0))
                .await
            {
                if needs_pause_for_mutation {
                    let _ = self.gateway.resume(request.task.gid).await;
                }
                return CommandOutcome::item_failure(item, error.into());
            }
        }

        if !option_pairs.is_empty()
            && let Err(error) = self
                .change_options_gated(request.task.gid, &option_pairs)
                .await
        {
            if needs_pause_for_mutation {
                let _ = self.gateway.resume(request.task.gid).await;
            }
            return CommandOutcome::item_failure(item, error.into());
        }

        if needs_pause_for_mutation && let Err(error) = self.gateway.resume(request.task.gid).await
        {
            return CommandOutcome::item_failure(item, error.into());
        }

        CommandOutcome::Success {
            succeeded: vec![item],
        }
    }

    async fn execute_global(&self, operation: GlobalTaskOperation) -> CommandOutcome {
        let required = match operation {
            GlobalTaskOperation::PauseAll => "aria2.pauseAll",
            GlobalTaskOperation::ForcePauseAll => "aria2.forcePauseAll",
            GlobalTaskOperation::ResumeAll => "aria2.unpauseAll",
        };
        if let Err(error) = self.require_method(required) {
            return CommandOutcome::failure(error);
        }
        let result = match operation {
            GlobalTaskOperation::PauseAll => self.gateway.pause_all().await,
            GlobalTaskOperation::ForcePauseAll => self.gateway.force_pause_all().await,
            GlobalTaskOperation::ResumeAll => self.gateway.resume_all().await,
        };
        match result {
            Ok(()) => CommandOutcome::Success {
                succeeded: Vec::new(),
            },
            Err(error) => CommandOutcome::failure(error.into()),
        }
    }

    async fn set_task_output_name(
        &self,
        request: SetTaskOutputNameRequest,
        task_contexts: &HashMap<TaskIdentity, TaskCommandContext>,
    ) -> CommandOutcome {
        let item = CommandItem::Task(request.task);
        if request.task.profile_id != self.profile_id {
            return CommandOutcome::item_failure(
                item,
                ApplicationError::new(
                    ApplicationErrorCode::WrongProfile,
                    "The task belongs to a different engine profile.",
                    false,
                ),
            );
        }
        let Some(context) = task_contexts.get(&request.task) else {
            return CommandOutcome::item_failure(
                item,
                ApplicationError::new(
                    ApplicationErrorCode::Rejected,
                    "The task is no longer present in the current engine session.",
                    false,
                ),
            );
        };
        let output_name = request.output_name.trim();
        if output_name.is_empty()
            || output_name == "."
            || output_name == ".."
            || output_name.contains(['/', '\\', '\0'])
        {
            return CommandOutcome::item_failure(
                item,
                ApplicationError::new(
                    ApplicationErrorCode::Validation,
                    "Output name must be a non-empty file name without path separators.",
                    false,
                ),
            );
        }
        if context.metadata.source_kind != TaskSourceKind::DirectUri {
            return CommandOutcome::item_failure(
                item,
                ApplicationError::new(
                    ApplicationErrorCode::Unsupported,
                    "A custom output name is currently supported only for direct URI tasks.",
                    false,
                ),
            );
        }
        if context.status.is_terminal() {
            return CommandOutcome::item_failure(
                item,
                ApplicationError::new(
                    ApplicationErrorCode::Rejected,
                    "A completed or failed task cannot change its output name.",
                    false,
                ),
            );
        }

        let options = [("out".to_owned(), output_name.to_owned())];
        finish_single_task(
            item,
            self.change_options_gated(request.task.gid, &options).await,
        )
    }

    async fn execute_batch(
        &self,
        tasks: Vec<TaskIdentity>,
        operation: TaskOperation,
        task_contexts: &HashMap<TaskIdentity, TaskCommandContext>,
    ) -> CommandOutcome {
        if tasks.is_empty() {
            return CommandOutcome::failure(ApplicationError::new(
                ApplicationErrorCode::Validation,
                "At least one task must be selected.",
                false,
            ));
        }

        let mut seen = HashSet::new();
        let mut succeeded = Vec::new();
        let mut failed = Vec::new();
        for identity in tasks.into_iter().filter(|identity| seen.insert(*identity)) {
            let item = CommandItem::Task(identity);
            if identity.profile_id != self.profile_id {
                failed.push(ItemFailure {
                    item: Some(item),
                    error: ApplicationError::new(
                        ApplicationErrorCode::WrongProfile,
                        "The task belongs to a different engine profile.",
                        false,
                    ),
                });
                continue;
            }

            let Some(context) = task_contexts.get(&identity) else {
                failed.push(ItemFailure {
                    item: Some(item),
                    error: ApplicationError::new(
                        ApplicationErrorCode::Rejected,
                        "The task is no longer present in the current engine session.",
                        false,
                    ),
                });
                continue;
            };
            let allowed = match operation {
                TaskOperation::Pause | TaskOperation::ForcePause => matches!(
                    context.status,
                    DownloadStatus::Active
                        | DownloadStatus::Seeding
                        | DownloadStatus::Waiting
                        | DownloadStatus::Verifying
                ),
                TaskOperation::Resume => matches!(context.status, DownloadStatus::Paused),
                TaskOperation::MoveInQueue(_) => !matches!(
                    context.status,
                    DownloadStatus::Complete
                        | DownloadStatus::Error
                        | DownloadStatus::Removed
                        | DownloadStatus::Unknown
                ),
                TaskOperation::Remove(_) | TaskOperation::ForceRemove(_) => {
                    !matches!(context.status, DownloadStatus::Unknown)
                }
            };
            if !allowed {
                failed.push(ItemFailure {
                    item: Some(item),
                    error: ApplicationError::new(
                        ApplicationErrorCode::Rejected,
                        format!(
                            "{} is not available while the task is {:?}.",
                            task_operation_label(operation),
                            context.status
                        ),
                        false,
                    ),
                });
                continue;
            }

            let removal_target = if context.status.is_terminal() {
                TaskRemovalTarget::DownloadResult
            } else {
                TaskRemovalTarget::LiveTask
            };
            if context.is_history_only
                && matches!(
                    operation,
                    TaskOperation::Remove(TaskRemovalScope::TaskOnly)
                        | TaskOperation::ForceRemove(TaskRemovalScope::TaskOnly)
                )
            {
                // Durable history-only rows no longer exist in aria2 memory; mark
                // succeeded so sync removes them from SQLite and the local store.
                succeeded.push(item);
                continue;
            }
            let required = match operation {
                TaskOperation::Pause => "aria2.pause",
                TaskOperation::ForcePause => "aria2.forcePause",
                TaskOperation::Resume => "aria2.unpause",
                TaskOperation::MoveInQueue(_) => "aria2.changePosition",
                TaskOperation::Remove(_) if removal_target == TaskRemovalTarget::DownloadResult => {
                    "aria2.removeDownloadResult"
                }
                TaskOperation::Remove(_) => "aria2.remove",
                TaskOperation::ForceRemove(_)
                    if removal_target == TaskRemovalTarget::DownloadResult =>
                {
                    "aria2.removeDownloadResult"
                }
                TaskOperation::ForceRemove(_) => "aria2.forceRemove",
            };
            if let Err(error) = self.require_method(required) {
                failed.push(ItemFailure {
                    item: Some(item),
                    error,
                });
                continue;
            }
            let result = match operation {
                TaskOperation::Pause => self.gateway.pause(identity.gid).await,
                TaskOperation::ForcePause => self.gateway.force_pause(identity.gid).await,
                TaskOperation::Resume => self.gateway.resume(identity.gid).await,
                TaskOperation::MoveInQueue(movement) => {
                    self.gateway.move_in_queue(identity.gid, movement).await
                }
                TaskOperation::Remove(TaskRemovalScope::TaskOnly) => {
                    self.gateway.remove(identity.gid, removal_target).await
                }
                TaskOperation::ForceRemove(TaskRemovalScope::TaskOnly) => {
                    self.gateway
                        .force_remove(identity.gid, removal_target)
                        .await
                }
            };
            match result {
                Ok(()) => succeeded.push(item),
                Err(error) => failed.push(ItemFailure {
                    item: Some(item),
                    error: error.into(),
                }),
            }
        }

        finish_batch(succeeded, failed)
    }

    async fn retry_tasks(
        &self,
        tasks: Vec<TaskIdentity>,
        task_contexts: &HashMap<TaskIdentity, TaskCommandContext>,
    ) -> CommandOutcome {
        if tasks.is_empty() {
            return CommandOutcome::failure(ApplicationError::new(
                ApplicationErrorCode::Validation,
                "At least one failed task must be selected.",
                false,
            ));
        }

        let mut seen = HashSet::new();
        let mut succeeded = Vec::new();
        let mut failed = Vec::new();
        for identity in tasks.into_iter().filter(|identity| seen.insert(*identity)) {
            let item = CommandItem::Task(identity);
            if identity.profile_id != self.profile_id {
                failed.push(ItemFailure {
                    item: Some(item),
                    error: ApplicationError::new(
                        ApplicationErrorCode::WrongProfile,
                        "The task belongs to a different engine profile.",
                        false,
                    ),
                });
                continue;
            }
            let Some(context) = task_contexts.get(&identity) else {
                failed.push(ItemFailure {
                    item: Some(item),
                    error: ApplicationError::new(
                        ApplicationErrorCode::Rejected,
                        "The failed task is no longer present in the current engine session.",
                        false,
                    ),
                });
                continue;
            };
            if context.status != DownloadStatus::Error {
                failed.push(ItemFailure {
                    item: Some(item),
                    error: ApplicationError::new(
                        ApplicationErrorCode::Rejected,
                        "Only failed tasks can be retried.",
                        false,
                    ),
                });
                continue;
            }
            let source = context.metadata.primary_uri.clone().or_else(|| {
                context
                    .metadata
                    .info_hash
                    .as_ref()
                    .map(|hash| format!("magnet:?xt=urn:btih:{hash}"))
            });
            let Some(source) = source else {
                failed.push(ItemFailure {
                    item: Some(item),
                    error: ApplicationError::new(
                        ApplicationErrorCode::Unsupported,
                        "The task has no replayable URL, magnet link, or info hash.",
                        false,
                    ),
                });
                continue;
            };
            if source == ariadeck_domain::REDACTED_SOURCE_PLACEHOLDER {
                failed.push(ItemFailure {
                    item: Some(item),
                    error: ApplicationError::new(
                        ApplicationErrorCode::Unsupported,
                        "The task source URI was redacted and cannot be retried without re-adding the download.",
                        false,
                    ),
                });
                continue;
            }
            let request = AddDownloadRequest {
                source: AddDownloadSource::Uris(vec![source]),
                destination: context.metadata.directory.clone(),
                file_conflict: FileConflictPolicy::default(),
                selected_file_indices: None,
                advanced: Default::default(),
                options: Vec::new(),
            };
            if let Err(error) = request.validate() {
                failed.push(ItemFailure {
                    item: Some(item),
                    error,
                });
                continue;
            }
            match self.gateway.retry_download(identity.gid, &request).await {
                Ok(gid) => {
                    succeeded.push(CommandItem::Task(TaskIdentity::new(self.profile_id, gid)))
                }
                Err(error) => failed.push(ItemFailure {
                    item: Some(item),
                    error: error.into(),
                }),
            }
        }

        finish_batch(succeeded, failed)
    }
}

fn finish_single_task(item: CommandItem, result: Result<(), GatewayError>) -> CommandOutcome {
    match result {
        Ok(()) => CommandOutcome::Success {
            succeeded: vec![item],
        },
        Err(error) => CommandOutcome::item_failure(item, error.into()),
    }
}

fn task_operation_label(operation: TaskOperation) -> &'static str {
    match operation {
        TaskOperation::Pause => "Pause",
        TaskOperation::ForcePause => "Force pause",
        TaskOperation::Resume => "Resume",
        TaskOperation::MoveInQueue(_) => "Change queue priority",
        TaskOperation::Remove(_) => "Remove",
        TaskOperation::ForceRemove(_) => "Force remove",
    }
}

fn finish_batch(succeeded: Vec<CommandItem>, failed: Vec<ItemFailure>) -> CommandOutcome {
    match (succeeded.is_empty(), failed.is_empty()) {
        (false, true) => CommandOutcome::Success { succeeded },
        (false, false) => CommandOutcome::PartialSuccess { succeeded, failed },
        (true, false) => CommandOutcome::Failure { failed },
        (true, true) => CommandOutcome::failure(ApplicationError::new(
            ApplicationErrorCode::Internal,
            "The command produced no result.",
            false,
        )),
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum TaskOperation {
    Pause,
    ForcePause,
    Resume,
    MoveInQueue(QueueMove),
    Remove(TaskRemovalScope),
    ForceRemove(TaskRemovalScope),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[allow(clippy::enum_variant_names)]
enum GlobalTaskOperation {
    PauseAll,
    ForcePauseAll,
    ResumeAll,
}
