//! Task commands and related dialogs for AppShell.

use super::*;

impl AppShell {
    pub(crate) fn pause_selected(
        &mut self,
        _: &PauseSelectedTask,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.visible_selected_task_count() > 1 {
            self.begin_batch_task_command(BatchTaskCommandView::Pause, cx);
        } else {
            self.begin_task_command(TaskCommandView::Pause, cx);
        }
    }

    pub(crate) fn resume_selected(
        &mut self,
        _: &ResumeSelectedTask,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.visible_selected_task_count() > 1 {
            self.begin_batch_task_command(BatchTaskCommandView::Resume, cx);
        } else {
            self.begin_task_command(TaskCommandView::Resume, cx);
        }
    }

    pub(crate) fn retry_selected(
        &mut self,
        _: &RetrySelectedTask,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.visible_selected_task_count() > 1 {
            self.begin_batch_task_command(BatchTaskCommandView::Retry, cx);
        } else {
            self.begin_task_command(TaskCommandView::Retry, cx);
        }
    }

    pub(crate) fn open_task_output_name_action(
        &mut self,
        _: &OpenTaskOutputName,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_task_output_name(window, cx);
    }

    pub(crate) fn open_task_output_name(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.output_name_dialog.is_some() {
            window.focus(&self.output_name_input.focus_handle(cx), cx);
            return;
        }
        if self.add_dialog.open
            || self.remove_confirmation.is_some()
            || self.batch_failure_details.is_some()
            || self.pending_task_command.is_some()
            || self.pending_batch_command.is_some()
        {
            return;
        }
        let Some(task) = self.selected_task_view() else {
            self.show_notice(self.t("notice-select-task-first"), true, cx);
            return;
        };
        if !task.can_set_output_name() || !self.snapshot.commands_available() {
            self.show_notice(
                "Output names can be changed only for non-terminal direct URI tasks.",
                true,
                cx,
            );
            return;
        }

        let initial_name = if task.name_state.is_resolving() {
            String::new()
        } else {
            task.display_name.clone()
        };
        self.output_name_input
            .update(cx, |input, cx| input.set_text(initial_name, cx));
        self.output_name_dialog = Some(TaskOutputNameDialog {
            identity: task.identity.clone(),
            display_name: task_display_name(&task),
            active: task.status.uses_active_connections(),
            previous_focus: window.focused(cx).map(|focus| focus.downgrade()),
            error: None,
        });
        cx.notify();
        cx.defer_in(window, |this, window, cx| {
            if this.output_name_dialog.is_some() {
                window.focus(&this.output_name_input.focus_handle(cx), cx);
            }
        });
    }

    pub(crate) fn close_task_output_name_action(
        &mut self,
        _: &CloseTaskOutputName,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.close_task_output_name(window, cx);
    }

    pub(crate) fn close_task_output_name(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.output_name_dialog.is_none()
            || self.pending_task_command.as_ref().is_some_and(|pending| {
                matches!(&pending.command, TaskCommandView::SetOutputName { .. })
            })
        {
            return;
        }
        let previous_focus = self
            .output_name_dialog
            .take()
            .and_then(|dialog| dialog.previous_focus)
            .and_then(|focus| focus.upgrade());
        if let Some(focus) = previous_focus {
            window.focus(&focus, cx);
        } else {
            window.focus(&self.focus_handle, cx);
        }
        cx.notify();
    }

    pub(crate) fn submit_task_output_name_action(
        &mut self,
        _: &SubmitTaskOutputName,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.submit_task_output_name(cx);
    }

    pub(crate) fn submit_task_output_name(&mut self, cx: &mut Context<Self>) {
        if self.pending_task_command.is_some() {
            return;
        }
        let Some(identity) = self
            .output_name_dialog
            .as_ref()
            .map(|dialog| dialog.identity.clone())
        else {
            return;
        };
        let output_name = self.output_name_input.read(cx).text().trim().to_owned();
        if let Some(key) = output_name_validation_error(&output_name) {
            let summary = self.t(key);
            if let Some(dialog) = &mut self.output_name_dialog {
                dialog.error = Some(OperationErrorView {
                    code: "validation.invalid_output_name".into(),
                    summary,
                    retryable: false,
                });
            }
            cx.notify();
            return;
        }
        let current_task = self
            .snapshot
            .tasks
            .iter()
            .find(|task| task.identity == identity);
        if self.selected.as_ref() != Some(&identity)
            || current_task.is_none_or(|task| !task.can_set_output_name())
        {
            if let Some(dialog) = &mut self.output_name_dialog {
                dialog.error = Some(OperationErrorView {
                    code: "command.task_changed".into(),
                    summary: "The task changed. Close this dialog and review its current state."
                        .into(),
                    retryable: false,
                });
            }
            cx.notify();
            return;
        }
        self.begin_task_command(TaskCommandView::SetOutputName { output_name }, cx);
    }

    pub(crate) fn open_task_speed_limit_action(
        &mut self,
        _: &OpenTaskSpeedLimit,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_task_speed_limit(window, cx);
    }

    pub(crate) fn open_task_speed_limit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.task_speed_limit_dialog.is_some() {
            window.focus(&self.task_inputs.download_limit.focus_handle(cx), cx);
            return;
        }
        if self.add_dialog.open
            || self.output_name_dialog.is_some()
            || self.remove_confirmation.is_some()
            || self.batch_failure_details.is_some()
            || self.pending_task_command.is_some()
            || self.pending_batch_command.is_some()
        {
            return;
        }
        let Some(task) = self.selected_task_view() else {
            self.show_notice(self.t("notice-select-task-first"), true, cx);
            return;
        };
        if !task.status.can_set_speed_limit() || !self.snapshot.commands_available() {
            self.show_notice(
                "Speed limits can be set only for a task that is still downloading.",
                true,
                cx,
            );
            return;
        }
        // The list projection does not carry per-task limits (that is DETAIL-001's
        // getOption surface), so the fields start blank and set a fresh value.
        self.task_inputs
            .download_limit
            .update(cx, |input, cx| input.set_text("", cx));
        self.task_inputs
            .upload_limit
            .update(cx, |input, cx| input.set_text("", cx));
        self.task_speed_limit_dialog = Some(TaskSpeedLimitDialog {
            identity: task.identity.clone(),
            display_name: task_display_name(&task),
            previous_focus: window.focused(cx).map(|focus| focus.downgrade()),
            error: None,
        });
        cx.notify();
        cx.defer_in(window, |this, window, cx| {
            if this.task_speed_limit_dialog.is_some() {
                window.focus(&this.task_inputs.download_limit.focus_handle(cx), cx);
            }
        });
    }

    pub(crate) fn close_task_speed_limit_action(
        &mut self,
        _: &CloseTaskSpeedLimit,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.close_task_speed_limit(window, cx);
    }

    pub(crate) fn close_task_speed_limit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.task_speed_limit_dialog.is_none()
            || self.pending_task_command.as_ref().is_some_and(|pending| {
                matches!(&pending.command, TaskCommandView::SetSpeedLimit { .. })
            })
        {
            return;
        }
        let previous_focus = self
            .task_speed_limit_dialog
            .take()
            .and_then(|dialog| dialog.previous_focus)
            .and_then(|focus| focus.upgrade());
        if let Some(focus) = previous_focus {
            window.focus(&focus, cx);
        } else {
            window.focus(&self.focus_handle, cx);
        }
        cx.notify();
    }

    pub(crate) fn submit_task_speed_limit_action(
        &mut self,
        _: &SubmitTaskSpeedLimit,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.submit_task_speed_limit(cx);
    }

    pub(crate) fn submit_task_speed_limit(&mut self, cx: &mut Context<Self>) {
        if self.pending_task_command.is_some() {
            return;
        }
        let Some(identity) = self
            .task_speed_limit_dialog
            .as_ref()
            .map(|dialog| dialog.identity.clone())
        else {
            return;
        };
        let draft = SpeedLimitSettingsView {
            download_limit: self
                .task_inputs
                .download_limit
                .read(cx)
                .text()
                .trim()
                .into(),
            upload_limit: self.task_inputs.upload_limit.read(cx).text().trim().into(),
        };
        let (Some(download_limit), Some(upload_limit)) =
            (draft.parse_download_limit(), draft.parse_upload_limit())
        else {
            if let Some(dialog) = &mut self.task_speed_limit_dialog {
                dialog.error = Some(OperationErrorView {
                    code: "validation.invalid_speed_limit".into(),
                    summary: "Enter a speed as bytes/second or a K/M/G value, or leave it blank for unlimited."
                        .into(),
                    retryable: false,
                });
            }
            cx.notify();
            return;
        };
        let current_task = self
            .snapshot
            .tasks
            .iter()
            .find(|task| task.identity == identity);
        if self.selected.as_ref() != Some(&identity)
            || current_task.is_none_or(|task| !task.status.can_set_speed_limit())
        {
            if let Some(dialog) = &mut self.task_speed_limit_dialog {
                dialog.error = Some(OperationErrorView {
                    code: "command.task_changed".into(),
                    summary: "The task changed. Close this dialog and review its current state."
                        .into(),
                    retryable: false,
                });
            }
            cx.notify();
            return;
        }
        self.begin_task_command(
            TaskCommandView::SetSpeedLimit {
                download_limit,
                upload_limit,
            },
            cx,
        );
    }

    pub(crate) fn open_task_options(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.task_options_dialog.is_some() {
            window.focus(&self.task_inputs.seed_ratio.focus_handle(cx), cx);
            return;
        }
        if self.add_dialog.open
            || self.output_name_dialog.is_some()
            || self.task_speed_limit_dialog.is_some()
            || self.remove_confirmation.is_some()
            || self.batch_failure_details.is_some()
            || self.pending_task_command.is_some()
            || self.pending_batch_command.is_some()
        {
            return;
        }
        let Some(task) = self.selected_task_view() else {
            self.show_notice(self.t("notice-select-task-first"), true, cx);
            return;
        };
        if !task.status.can_set_speed_limit() || !self.snapshot.commands_available() {
            self.show_notice(
                "Task options can be changed only while the download is still live.",
                true,
                cx,
            );
            return;
        }
        let supports_seed_rules = matches!(
            task.source_kind,
            crate::TaskSourceKindView::Magnet | crate::TaskSourceKindView::BitTorrent
        ) || task.status == TaskStatusView::Seeding;
        // Prefill from the open details drawer options projection when present.
        let (seed_ratio, seed_time) = self
            .details_drawer
            .as_ref()
            .and_then(|drawer| match &drawer.state {
                TaskDetailsLoadState::Ready { details } => Some(details),
                _ => None,
            })
            .map(|details| {
                let value = |key: &str| {
                    details
                        .options
                        .iter()
                        .find(|option| option.key.eq_ignore_ascii_case(key))
                        .map(|option| option.value.clone())
                        .unwrap_or_default()
                };
                (value("seed-ratio"), value("seed-time"))
            })
            .unwrap_or_default();
        self.task_inputs.seed_ratio.update(cx, |input, cx| {
            input.set_text(
                if supports_seed_rules {
                    seed_ratio
                } else {
                    String::new()
                },
                cx,
            );
        });
        self.task_inputs.seed_time.update(cx, |input, cx| {
            input.set_text(
                if supports_seed_rules {
                    seed_time
                } else {
                    String::new()
                },
                cx,
            );
        });
        self.task_options_dialog = Some(TaskOptionsDialog {
            identity: task.identity.clone(),
            display_name: task_display_name(&task),
            supports_seed_rules,
            previous_focus: window.focused(cx).map(|focus| focus.downgrade()),
            error: None,
        });
        cx.notify();
        cx.defer_in(window, |this, window, cx| {
            if this.task_options_dialog.is_some() {
                window.focus(&this.task_inputs.seed_ratio.focus_handle(cx), cx);
            }
        });
    }

    pub(crate) fn close_task_options(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.task_options_dialog.is_none()
            || self.pending_task_command.as_ref().is_some_and(|pending| {
                matches!(&pending.command, TaskCommandView::SetOptions { .. })
            })
        {
            return;
        }
        let previous_focus = self
            .task_options_dialog
            .take()
            .and_then(|dialog| dialog.previous_focus)
            .and_then(|focus| focus.upgrade());
        if let Some(focus) = previous_focus {
            window.focus(&focus, cx);
        } else {
            window.focus(&self.focus_handle, cx);
        }
        cx.notify();
    }

    pub(crate) fn submit_task_options(&mut self, cx: &mut Context<Self>) {
        if self.pending_task_command.is_some() {
            return;
        }
        let Some(dialog) = self.task_options_dialog.as_ref() else {
            return;
        };
        let identity = dialog.identity.clone();
        let supports_seed_rules = dialog.supports_seed_rules;
        let seed_ratio_raw = self
            .task_inputs
            .seed_ratio
            .read(cx)
            .text()
            .trim()
            .to_owned();
        let seed_time_raw = self.task_inputs.seed_time.read(cx).text().trim().to_owned();
        let mut seed_ratio = None;
        let mut seed_time_minutes = None;
        if !supports_seed_rules {
            if let Some(dialog) = &mut self.task_options_dialog {
                dialog.error = Some(OperationErrorView {
                    code: "command.unsupported".into(),
                    summary: "Seed rules apply only to BitTorrent tasks.".into(),
                    retryable: false,
                });
            }
            cx.notify();
            return;
        }
        if !seed_ratio_raw.is_empty() {
            match seed_ratio_raw.parse::<f64>() {
                Ok(value) if value.is_finite() && value >= 0.0 => {
                    seed_ratio = Some(seed_ratio_raw.clone());
                }
                _ => {
                    if let Some(dialog) = &mut self.task_options_dialog {
                        dialog.error = Some(OperationErrorView {
                            code: "validation.invalid_seed_ratio".into(),
                            summary: "Seed ratio must be a number greater than or equal to 0."
                                .into(),
                            retryable: false,
                        });
                    }
                    cx.notify();
                    return;
                }
            }
        }
        if !seed_time_raw.is_empty() {
            match seed_time_raw.parse::<u64>() {
                Ok(_) => seed_time_minutes = Some(seed_time_raw.clone()),
                Err(_) => {
                    if let Some(dialog) = &mut self.task_options_dialog {
                        dialog.error = Some(OperationErrorView {
                            code: "validation.invalid_seed_time".into(),
                            summary: "Seed time must be a whole number of minutes.".into(),
                            retryable: false,
                        });
                    }
                    cx.notify();
                    return;
                }
            }
        }
        if seed_ratio.is_none() && seed_time_minutes.is_none() {
            if let Some(dialog) = &mut self.task_options_dialog {
                dialog.error = Some(OperationErrorView {
                    code: "validation.empty_task_options".into(),
                    summary: "Enter a seed ratio and/or seed time to apply.".into(),
                    retryable: false,
                });
            }
            cx.notify();
            return;
        }
        let current_task = self
            .snapshot
            .tasks
            .iter()
            .find(|task| task.identity == identity);
        if self.selected.as_ref() != Some(&identity)
            || current_task.is_none_or(|task| !task.status.can_set_speed_limit())
        {
            if let Some(dialog) = &mut self.task_options_dialog {
                dialog.error = Some(OperationErrorView {
                    code: "command.task_changed".into(),
                    summary: "The task changed. Close this dialog and review its current state."
                        .into(),
                    retryable: false,
                });
            }
            cx.notify();
            return;
        }
        self.begin_task_command(
            TaskCommandView::SetOptions {
                seed_ratio,
                seed_time_minutes,
                selected_file_indices: None,
            },
            cx,
        );
    }

    pub(crate) fn remove_selected(
        &mut self,
        _: &RemoveSelectedTask,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.confirm_remove_selected(window, cx);
    }

    pub(crate) fn move_selected_to_queue_top(
        &mut self,
        _: &MoveTaskToQueueTop,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.close_task_context_menu(cx);
        self.begin_task_command(TaskCommandView::MoveToQueueTop, cx);
    }

    pub(crate) fn move_selected_up_in_queue(
        &mut self,
        _: &MoveTaskUpInQueue,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.close_task_context_menu(cx);
        self.begin_task_command(TaskCommandView::MoveUpInQueue, cx);
    }

    pub(crate) fn move_selected_down_in_queue(
        &mut self,
        _: &MoveTaskDownInQueue,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.close_task_context_menu(cx);
        self.begin_task_command(TaskCommandView::MoveDownInQueue, cx);
    }

    pub(crate) fn move_selected_to_queue_bottom(
        &mut self,
        _: &MoveTaskToQueueBottom,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.close_task_context_menu(cx);
        self.begin_task_command(TaskCommandView::MoveToQueueBottom, cx);
    }

    pub(crate) fn open_task_context_menu(
        &mut self,
        index: usize,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.page != AppPage::Downloads
            || self.add_dialog.open
            || self.output_name_dialog.is_some()
            || self.task_speed_limit_dialog.is_some()
            || self.task_options_dialog.is_some()
            || self.remove_confirmation.is_some()
            || self.batch_failure_details.is_some()
        {
            return;
        }
        let Some(task) = self.snapshot.tasks.get(index).cloned() else {
            return;
        };
        // Right-click focuses the row without clearing a multi-selection that
        // already includes it (qBittorrent/Motrix parity).
        if !self.selected_tasks.contains(&task.identity) {
            self.select_at_with_modifiers(index, false, false, window, cx);
        } else {
            self.selected = Some(task.identity.clone());
        }
        self.sort_popover_open = false;
        self.speed_popover_open = false;
        self.text_field_context_menu = None;
        self.context_menu = Some(TaskContextMenu {
            identity: task.identity,
            position,
        });
        window.focus(&self.focus_handle, cx);
        cx.notify();
    }

    pub(crate) fn close_task_context_menu(&mut self, cx: &mut Context<Self>) {
        if self.context_menu.take().is_some() {
            cx.notify();
        }
    }

    /// Prefer the right-clicked menu identity so multi-selection still has a
    /// single authoritative target for copy/open/details actions.
    pub(crate) fn context_menu_task_view(&self) -> Option<DownloadRowView> {
        if let Some(menu) = self.context_menu.as_ref()
            && let Some(task) = self
                .snapshot
                .tasks
                .iter()
                .find(|task| task.identity == menu.identity)
        {
            return Some(task.clone());
        }
        self.selected_task_view()
            .or_else(|| self.command_target_task_view())
    }

    pub(crate) fn copy_task_source(&mut self, task: &DownloadRowView, cx: &mut Context<Self>) {
        let source = task
            .primary_source
            .as_deref()
            .filter(|value| !value.is_empty())
            .unwrap_or("");
        if source.is_empty() {
            self.show_notice(self.t("notice-no-copyable-source"), true, cx);
            return;
        }
        cx.write_to_clipboard(ClipboardItem::new_string(source.to_owned()));
        self.show_notice(self.t("notice-source-copied"), false, cx);
    }

    pub(crate) fn copy_task_gid(&mut self, task: &DownloadRowView, cx: &mut Context<Self>) {
        cx.write_to_clipboard(ClipboardItem::new_string(task.identity.gid.clone()));
        self.show_notice(self.t("notice-gid-copied"), false, cx);
    }

    /// Open a local path for the command-target task without requiring the
    /// details drawer (used by the task context menu).
    pub(crate) fn request_task_open_for_selection(
        &mut self,
        target: TaskOpenTargetView,
        cx: &mut Context<Self>,
    ) {
        if !self.snapshot.commands_available() || !self.snapshot.local_path_actions_available {
            self.show_notice(
                "Opening task paths is available only for the managed local engine.",
                true,
                cx,
            );
            return;
        }
        let Some(session) = self.snapshot.engine_session() else {
            return;
        };
        let Some(task) = self.context_menu_task_view() else {
            self.show_notice(self.t("notice-select-task-first"), true, cx);
            return;
        };
        if task.identity.profile_id != session.profile_id {
            return;
        }
        let request_id = self.allocate_request_id();
        cx.emit(AppShellEvent::TaskOpenRequested(TaskOpenRequestView {
            request_id,
            session,
            identity: task.identity,
            target,
        }));
        self.show_notice(self.t("notice-opening-path"), false, cx);
        cx.notify();
    }

    /// Queue reordering is authoritative only when the visible query is the
    /// full, unsearched, ascending queue order (D-014 Scope rule). aria2's
    /// queue is global across active/waiting/paused tasks, so relative movement
    /// inside a filtered, searched, reversed, or value-sorted projection would
    /// imply a position that is not authoritative.
    pub(crate) fn queue_reordering_available(&self) -> bool {
        self.snapshot.capabilities.queue_positioning
            && self.query.filter == WorkspaceFilter::All
            && self.query.search.trim().is_empty()
            && self.query.sort_key == WorkspaceSortKey::Queue
            && self.query.sort_direction == WorkspaceSortDirection::Ascending
    }

    pub(crate) fn begin_task_command(&mut self, command: TaskCommandView, cx: &mut Context<Self>) {
        if self.pending_task_command.is_some()
            || self.pending_global_task_command.is_some()
            || self.pending_batch_command.is_some()
            || self.batch_failure_details.is_some()
        {
            return;
        }
        let Some(task) = self.command_target_task_view() else {
            self.show_notice(self.t("notice-select-task-first"), true, cx);
            return;
        };
        let capability_block = match command {
            TaskCommandView::ForcePause if !self.snapshot.capabilities.force_pause => {
                Some(self.snapshot.capabilities.unsupported_force_pause_message())
            }
            TaskCommandView::ForceRemoveTask if !self.snapshot.capabilities.force_remove => Some(
                self.snapshot
                    .capabilities
                    .unsupported_force_remove_message(),
            ),
            TaskCommandView::MoveToQueueTop
            | TaskCommandView::MoveUpInQueue
            | TaskCommandView::MoveDownInQueue
            | TaskCommandView::MoveToQueueBottom
                if !self.snapshot.capabilities.queue_positioning =>
            {
                Some(self.snapshot.capabilities.unsupported_queue_message())
            }
            TaskCommandView::SetSpeedLimit { .. }
            | TaskCommandView::SetConnectionPolicy { .. }
            | TaskCommandView::SetOptions { .. }
                if !self.snapshot.capabilities.change_option =>
            {
                Some(
                    self.snapshot
                        .capabilities
                        .unsupported_change_option_message(),
                )
            }
            _ => None,
        };
        if let Some(message) = capability_block {
            self.show_notice(message, true, cx);
            return;
        }
        let allowed = match command {
            TaskCommandView::Pause | TaskCommandView::ForcePause => task.status.can_pause(),
            TaskCommandView::Resume => task.status.can_resume(),
            TaskCommandView::MoveToQueueTop
            | TaskCommandView::MoveUpInQueue
            | TaskCommandView::MoveDownInQueue
            | TaskCommandView::MoveToQueueBottom => {
                task.status.can_move_in_queue() && self.queue_reordering_available()
            }
            TaskCommandView::Retry => task.status.can_retry(),
            TaskCommandView::SetOutputName { .. } => task.can_set_output_name(),
            TaskCommandView::SetSpeedLimit { .. } => task.status.can_set_speed_limit(),
            TaskCommandView::SetConnectionPolicy { .. } => task.status.can_set_connection_policy(),
            TaskCommandView::SetOptions { .. } => task.status.can_set_speed_limit(),
            TaskCommandView::RemoveTask
            | TaskCommandView::ForceRemoveTask
            | TaskCommandView::RemoveTaskAndFiles => task.status.can_remove(),
        };
        if !allowed {
            self.show_notice(
                format!(
                    "{} is not available while the task is {}.",
                    task_command_label(&command),
                    task.status.label().to_lowercase()
                ),
                true,
                cx,
            );
            return;
        }
        let Some(session) = self
            .snapshot
            .commands_available()
            .then(|| self.snapshot.engine_session())
            .flatten()
        else {
            self.show_notice(self.t("notice-engine-not-ready"), true, cx);
            return;
        };

        let request_id = self.allocate_request_id();
        let identity = task.identity;
        self.pending_task_command = Some(PendingTaskCommand {
            request_id,
            session: session.clone(),
            identity: identity.clone(),
            command: command.clone(),
        });
        self.show_notice(self.t(command.progress_message_key()), false, cx);
        cx.emit(AppShellEvent::TaskCommandRequested(
            TaskCommandRequestView {
                request_id,
                session,
                identity,
                command,
            },
        ));
        cx.notify();
    }

    pub(crate) fn begin_global_task_command(
        &mut self,
        command: GlobalTaskCommandView,
        cx: &mut Context<Self>,
    ) {
        if self.pending_task_command.is_some()
            || self.pending_global_task_command.is_some()
            || self.pending_batch_command.is_some()
            || self.batch_failure_details.is_some()
        {
            return;
        }
        let Some(session) = self
            .snapshot
            .commands_available()
            .then(|| self.snapshot.engine_session())
            .flatten()
        else {
            self.show_notice(self.t("notice-engine-not-ready"), true, cx);
            return;
        };
        if matches!(command, GlobalTaskCommandView::ForcePauseAll)
            && !self.snapshot.capabilities.force_pause_all
        {
            self.show_notice(
                self.snapshot
                    .capabilities
                    .unsupported_force_pause_all_message(),
                true,
                cx,
            );
            return;
        }
        let request_id = self.allocate_request_id();
        self.pending_global_task_command = Some(PendingGlobalTaskCommand {
            request_id,
            session: session.clone(),
            command,
        });
        self.show_notice(self.t(command.progress_message_key()), false, cx);
        cx.emit(AppShellEvent::GlobalTaskCommandRequested(
            GlobalTaskCommandRequestView {
                request_id,
                session,
                command,
            },
        ));
        cx.notify();
    }

    pub(crate) fn begin_batch_task_command(
        &mut self,
        command: BatchTaskCommandView,
        cx: &mut Context<Self>,
    ) {
        if self.pending_task_command.is_some()
            || self.pending_global_task_command.is_some()
            || self.pending_batch_command.is_some()
            || self.batch_failure_details.is_some()
        {
            return;
        }
        let identities = self
            .snapshot
            .tasks
            .iter()
            .filter(|task| self.selected_tasks.contains(&task.identity))
            .map(|task| task.identity.clone())
            .collect::<Vec<_>>();
        if identities.len() < 2 {
            self.show_notice(
                "Select at least two visible tasks for a batch action.",
                true,
                cx,
            );
            return;
        }
        let Some(session) = self
            .snapshot
            .commands_available()
            .then(|| self.snapshot.engine_session())
            .flatten()
        else {
            self.show_notice(self.t("notice-engine-not-ready"), true, cx);
            return;
        };
        let capability_block = match command {
            BatchTaskCommandView::ForcePause if !self.snapshot.capabilities.force_pause => {
                Some(self.snapshot.capabilities.unsupported_force_pause_message())
            }
            BatchTaskCommandView::ForceRemoveTask if !self.snapshot.capabilities.force_remove => {
                Some(
                    self.snapshot
                        .capabilities
                        .unsupported_force_remove_message(),
                )
            }
            _ => None,
        };
        if let Some(message) = capability_block {
            self.show_notice(message, true, cx);
            return;
        }
        let request_id = self.allocate_request_id();
        self.pending_batch_command = Some(PendingBatchTaskCommand {
            request_id,
            session: session.clone(),
            identities: identities.clone(),
            command,
        });
        self.show_notice(self.t(command.progress_message_key()), false, cx);
        cx.emit(AppShellEvent::BatchTaskCommandRequested(
            BatchTaskCommandRequestView {
                request_id,
                session,
                identities,
                command,
            },
        ));
        cx.notify();
    }

    pub(crate) fn confirm_remove_selected(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.remove_confirmation.is_some()
            || self.output_name_dialog.is_some()
            || self.batch_failure_details.is_some()
            || self.pending_task_command.is_some()
            || self.pending_batch_command.is_some()
        {
            return;
        }
        let visible_selected_count = self.visible_selected_task_count();
        if visible_selected_count > 1 {
            let selected_tasks = self
                .snapshot
                .tasks
                .iter()
                .filter(|task| self.selected_tasks.contains(&task.identity))
                .collect::<Vec<_>>();
            let identities = selected_tasks
                .iter()
                .map(|task| task.identity.clone())
                .collect::<Vec<_>>();
            if identities.len() > 1 && self.snapshot.commands_available() {
                self.remove_confirmation = Some(RemoveConfirmation {
                    display_name: format!("{} selected tasks", identities.len()),
                    identities,
                    has_live_tasks: selected_tasks.iter().any(|task| !task.status.is_terminal()),
                    has_terminal_tasks: selected_tasks.iter().any(|task| task.status.is_terminal()),
                    delete_files: false,
                    previous_focus: window.focused(cx).map(|focus| focus.downgrade()),
                });
                cx.notify();
                cx.defer_in(window, |this, window, cx| {
                    if this.remove_confirmation.is_some() {
                        window.focus(&this.remove_cancel_focus, cx);
                    }
                });
            }
            return;
        }
        let Some(task) = self.command_target_task_view() else {
            if !self.selected_tasks.is_empty() {
                self.show_notice(
                    "Selected tasks are outside the current result. Clear the hidden selection or change the query.",
                    true,
                    cx,
                );
            }
            return;
        };
        if !task.status.can_remove() || !self.snapshot.commands_available() {
            self.show_notice(
                "The selected task cannot be removed in the current engine state.",
                true,
                cx,
            );
            return;
        }

        let display_name = task_display_name(&task);
        self.remove_confirmation = Some(RemoveConfirmation {
            identities: vec![task.identity],
            display_name,
            has_live_tasks: !task.status.is_terminal(),
            has_terminal_tasks: task.status.is_terminal(),
            delete_files: false,
            previous_focus: window.focused(cx).map(|focus| focus.downgrade()),
        });
        cx.notify();
        cx.defer_in(window, |this, window, cx| {
            if this.remove_confirmation.is_some() {
                window.focus(&this.remove_cancel_focus, cx);
            }
        });
    }

    pub(crate) fn close_remove_confirmation(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(confirmation) = self.remove_confirmation.take() else {
            return;
        };
        if let Some(focus) = confirmation
            .previous_focus
            .and_then(|focus| focus.upgrade())
        {
            window.focus(&focus, cx);
        } else {
            window.focus(&self.focus_handle, cx);
        }
        cx.notify();
    }

    pub(crate) fn submit_remove_confirmation(&mut self, cx: &mut Context<Self>) {
        let Some(confirmation) = self.remove_confirmation.take() else {
            return;
        };
        let selection_matches = if confirmation.identities.len() > 1 {
            confirmation
                .identities
                .iter()
                .all(|identity| self.selected_tasks.contains(identity))
        } else {
            confirmation
                .identities
                .first()
                .is_some_and(|identity| self.selected.as_ref() == Some(identity))
        };
        if !selection_matches {
            self.show_notice(
                "The task selection changed. Review it before removing tasks.",
                true,
                cx,
            );
            return;
        }
        if confirmation.identities.len() > 1 {
            self.begin_batch_task_command(
                if confirmation.delete_files {
                    BatchTaskCommandView::RemoveTaskAndFiles
                } else {
                    BatchTaskCommandView::RemoveTask
                },
                cx,
            );
        } else {
            self.begin_task_command(
                if confirmation.delete_files {
                    TaskCommandView::RemoveTaskAndFiles
                } else {
                    TaskCommandView::RemoveTask
                },
                cx,
            );
        }
    }

    pub(crate) fn toggle_remove_files(&mut self, cx: &mut Context<Self>) {
        if matches!(self.engine_health, EngineHealthView::External) {
            return;
        }
        if let Some(confirmation) = &mut self.remove_confirmation {
            confirmation.delete_files = !confirmation.delete_files;
            cx.notify();
        }
    }

    pub(crate) fn toggle_speed_popover(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.speed_popover_open {
            self.close_speed_popover(window, cx);
            return;
        }
        self.speed_popover_previous_focus = window.focused(cx).map(|focus| focus.downgrade());
        self.speed_popover_open = true;
        cx.notify();
    }

    pub(crate) fn close_speed_popover(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.speed_popover_open {
            return;
        }
        self.speed_popover_open = false;
        if let Some(focus) = self
            .speed_popover_previous_focus
            .take()
            .and_then(|focus| focus.upgrade())
        {
            window.focus(&focus, cx);
        }
        cx.notify();
    }

    pub(crate) fn selected_task_view(&self) -> Option<DownloadRowView> {
        let selected = self.selected.as_ref()?;
        self.snapshot
            .tasks
            .iter()
            .find(|task| &task.identity == selected)
            .cloned()
            .or_else(|| {
                self.details_drawer
                    .as_ref()
                    .filter(|drawer| &drawer.identity == selected)
                    .map(|drawer| drawer.overview.clone())
            })
    }

    pub(crate) fn command_target_task_view(&self) -> Option<DownloadRowView> {
        let mut visible_selected = self
            .snapshot
            .tasks
            .iter()
            .filter(|task| self.selected_tasks.contains(&task.identity));
        let first = visible_selected.next();
        if first.is_some() && visible_selected.next().is_none() {
            return first.cloned();
        }
        if first.is_none() && !self.selected_tasks.is_empty() {
            return None;
        }
        self.selected_task_view()
    }

    pub(crate) fn render_remove_confirmation(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let colors = self.theme.colors;
        let (display_name, has_live_tasks, has_terminal_tasks, delete_files) = self
            .remove_confirmation
            .as_ref()
            .map(|confirmation| {
                (
                    confirmation.display_name.clone(),
                    confirmation.has_live_tasks,
                    confirmation.has_terminal_tasks,
                    confirmation.delete_files,
                )
            })
            .unwrap_or_default();
        let local_files_available = !matches!(self.engine_health, EngineHealthView::External);
        let removal_description = match (has_live_tasks, has_terminal_tasks) {
            (true, true) => format!(
                "{display_name}: live tasks will be stopped and terminal records will be removed from aria2."
            ),
            (true, false) => {
                format!("{display_name} will be stopped and retained as a removed aria2 result.")
            }
            (false, true) => {
                format!("{display_name} will be removed from aria2's stopped results.")
            }
            (false, false) => format!("{display_name} will be removed from aria2."),
        };
        let file_choice = if local_files_available {
            div()
                .id("remove-task-files")
                .role(Role::CheckBox)
                .aria_label(self.t("dialog-remove-files-aria"))
                .aria_toggled(if delete_files {
                    Toggled::True
                } else {
                    Toggled::False
                })
                .flex()
                .items_start()
                .gap_2()
                .cursor_pointer()
                .on_click(cx.listener(|this, _, _, cx| {
                    this.toggle_remove_files(cx);
                }))
                .child(
                    Icon::new(if delete_files {
                        IconName::SquareCheckBig
                    } else {
                        IconName::Square
                    })
                    .size(IconSize::Small)
                    .color(if delete_files {
                        colors.danger
                    } else {
                        colors.text_muted
                    }),
                )
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_1()
                        .text_sm()
                        .text_color(colors.text_primary)
                        .child(self.t("dialog-remove-files-checkbox"))
                        .child(
                            div()
                                .text_xs()
                                .text_color(colors.text_muted)
                                .child(self.t("ui-remove-aria2-control")),
                        ),
                )
                .into_any_element()
        } else {
            div()
                .flex()
                .items_center()
                .gap_2()
                .text_xs()
                .text_color(colors.text_secondary)
                .child(Icon::new(IconName::Info).size(IconSize::Small))
                .child(self.t("ui-external-engine-files-kept"))
                .into_any_element()
        };
        Dialog::new(
            "remove-task-dialog",
            self.t("dialog-remove-title"),
            self.theme,
        )
        .description(removal_description)
        .key_context("RemoveTaskDialog")
        .track_focus(self.remove_dialog_focus.clone())
        .child(
            div()
                .flex()
                .items_center()
                .gap_2()
                .text_xs()
                .text_color(colors.text_secondary)
                .child(
                    Icon::new(IconName::TriangleAlert)
                        .size(IconSize::Small)
                        .color(colors.danger),
                )
                .child(if delete_files {
                    "Selected task files will be moved to the Recycle Bin."
                } else {
                    "Downloaded files will be kept."
                }),
        )
        .child(file_choice)
        .action(
            Button::new("cancel-remove-task", self.t("button-cancel"))
                .aria_label(self.t("dialog-remove-cancel-aria"))
                .style(ButtonStyle::Secondary)
                .track_focus(self.remove_cancel_focus.clone())
                .on_click(cx.listener(|this, _, window, cx| {
                    this.close_remove_confirmation(window, cx);
                }))
                .render(colors),
        )
        .action(
            Button::new(
                "confirm-remove-task",
                if delete_files {
                    self.t("dialog-remove-and-files")
                } else {
                    self.t("dialog-remove-confirm")
                },
            )
            .aria_label(if delete_files {
                self.t("dialog-remove-submit-aria")
            } else {
                self.t("dialog-remove-confirm")
            })
            .style(ButtonStyle::Danger)
            .track_focus(self.remove_submit_focus.clone())
            .on_click(cx.listener(|this, _, _, cx| {
                this.submit_remove_confirmation(cx);
            }))
            .render(colors),
        )
        .into_any_element()
    }

    pub(crate) fn render_batch_failure_details(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let colors = self.theme.colors;
        let Some(details) = self.batch_failure_details.as_ref() else {
            return div().into_any_element();
        };
        let command = details.command.label();
        let failures = details
            .failures
            .iter()
            .enumerate()
            .map(|(index, failure)| {
                let task_name = failure.identity.as_ref().map_or_else(
                    || self.t("dialog-batch-request"),
                    |identity| {
                        self.snapshot
                            .tasks
                            .iter()
                            .find(|task| task.identity == *identity)
                            .map(task_display_name)
                            .unwrap_or_else(|| format!("Task {}", identity.gid))
                    },
                );
                div()
                    .id(SharedString::from(format!("batch-failure-{index}")))
                    .role(Role::ListItem)
                    .flex()
                    .items_start()
                    .gap_2()
                    .child(
                        Icon::new(IconName::CircleAlert)
                            .size(IconSize::Small)
                            .color(colors.danger),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .child(
                                div()
                                    .text_xs()
                                    .font_weight(FontWeight::MEDIUM)
                                    .child(task_name),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(colors.text_muted)
                                    .child(self.te(&failure.error)),
                            ),
                    )
            })
            .collect::<Vec<_>>();
        Dialog::new(
            "batch-failure-dialog",
            self.t("dialog-batch-title"),
            self.theme,
        )
        .description(format!(
            "{} task{} failed. Failed tasks remain selected for follow-up.",
            details.failures.len(),
            if details.failures.len() == 1 { "" } else { "s" }
        ))
        .key_context("BatchFailureDialog")
        .track_focus(self.batch_failure_dialog_focus.clone())
        .width(560.0)
        .child(
            div()
                .id("batch-failure-list")
                .role(Role::List)
                .aria_label(format!("Failed {command} tasks"))
                .max_h(px(360.0))
                .flex()
                .flex_col()
                .gap_3()
                .children(failures),
        )
        .action(
            Button::new("close-batch-failures", "Close")
                .aria_label(self.t("dialog-batch-close-aria"))
                .style(ButtonStyle::Secondary)
                .track_focus(self.batch_failure_close_focus.clone())
                .on_click(cx.listener(|this, _, window, cx| {
                    this.close_batch_failure_details(window, cx);
                }))
                .render(colors),
        )
        .into_any_element()
    }

    pub(crate) fn render_task_output_name_dialog(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let colors = self.theme.colors;
        let Some(dialog) = self.output_name_dialog.as_ref() else {
            return div().into_any_element();
        };
        let identity = dialog.identity.clone();
        let display_name = dialog.display_name.clone();
        let active = dialog.active;
        let error = dialog.error.clone();
        let pending = self.pending_task_command.as_ref().is_some_and(|pending| {
            pending.identity == identity
                && matches!(&pending.command, TaskCommandView::SetOutputName { .. })
        });
        let content = div()
            .flex()
            .flex_col()
            .gap_2()
            .child(
                div()
                    .text_xs()
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(colors.text_secondary)
                    .child(self.t("dialog-output-name-filename")),
            )
            .child(self.output_name_input.clone())
            .when(active, |element| {
                element.child(
                    div()
                        .id("active-output-name-warning")
                        .role(Role::Status)
                        .flex()
                        .items_center()
                        .gap_2()
                        .text_xs()
                        .text_color(colors.warning)
                        .child(
                            Icon::new(IconName::TriangleAlert)
                                .size(IconSize::Small)
                                .color(colors.warning),
                        )
                        .child(self.t("ui-output-name-restart")),
                )
            })
            .when_some(error, |element, error| {
                element.child(
                    div()
                        .id("task-output-name-error")
                        .role(Role::Alert)
                        .aria_label(self.te(&error))
                        .text_xs()
                        .text_color(colors.danger)
                        .child(self.te(&error)),
                )
            });

        Dialog::new(
            "task-output-name-dialog",
            self.t("dialog-output-name-title"),
            self.theme,
        )
        .description(format!(
            "Set the filename used by aria2 for {display_name}."
        ))
        .key_context("TaskOutputNameDialog")
        .track_focus(self.output_name_dialog_focus.clone())
        .width(520.0)
        .child(content)
        .action(
            Button::new("cancel-task-output-name", "Cancel")
                .aria_label(self.t("dialog-output-name-cancel-aria"))
                .style(ButtonStyle::Secondary)
                .disabled(pending)
                .track_focus(self.output_name_cancel_focus.clone())
                .on_click(cx.listener(|this, _, window, cx| {
                    this.close_task_output_name(window, cx);
                }))
                .render(colors),
        )
        .action(
            Button::new(
                "submit-task-output-name",
                if pending { "Saving..." } else { "Save" },
            )
            .aria_label(if pending {
                self.t("dialog-output-name-saving")
            } else {
                self.t("dialog-output-name-save")
            })
            .style(ButtonStyle::Primary)
            .loading(pending)
            .track_focus(self.output_name_submit_focus.clone())
            .on_click(cx.listener(|this, _, _, cx| {
                this.submit_task_output_name(cx);
            }))
            .render(colors),
        )
        .into_any_element()
    }

    pub(crate) fn render_task_speed_limit_dialog(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let colors = self.theme.colors;
        let Some(dialog) = self.task_speed_limit_dialog.as_ref() else {
            return div().into_any_element();
        };
        let identity = dialog.identity.clone();
        let display_name = dialog.display_name.clone();
        let error = dialog.error.clone();
        let pending = self.pending_task_command.as_ref().is_some_and(|pending| {
            pending.identity == identity
                && matches!(&pending.command, TaskCommandView::SetSpeedLimit { .. })
        });
        let content = div()
            .flex()
            .flex_col()
            .gap_3()
            .child(
                div()
                    .flex()
                    .gap_3()
                    .child(
                        settings_labeled_input(
                            "Download limit",
                            self.task_inputs.download_limit.clone(),
                            colors,
                        )
                        .flex_1()
                        .min_w_0(),
                    )
                    .child(
                        settings_labeled_input(
                            "Upload limit",
                            self.task_inputs.upload_limit.clone(),
                            colors,
                        )
                        .flex_1()
                        .min_w_0(),
                    ),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(colors.text_muted)
                    .child(
                        "Applies to this download only. Leave a field blank for no limit; values accept a K/M/G suffix (for example 2M).",
                    ),
            )
            .when_some(error, |element, error| {
                element.child(
                    div()
                        .id("task-speed-limit-error")
                        .role(Role::Alert)
                        .aria_label(self.te(&error))
                        .text_xs()
                        .text_color(colors.danger)
                        .child(self.te(&error)),
                )
            });

        Dialog::new(
            "task-speed-limit-dialog",
            self.t("dialog-speed-limit-title"),
            self.theme,
        )
        .description(format!(
            "Throttle aria2's transfer rate for {display_name}."
        ))
        .key_context("TaskSpeedLimitDialog")
        .track_focus(self.task_speed_limit_dialog_focus.clone())
        .width(520.0)
        .child(content)
        .action(
            Button::new("cancel-task-speed-limit", "Cancel")
                .aria_label(self.t("dialog-speed-limit-cancel-aria"))
                .style(ButtonStyle::Secondary)
                .disabled(pending)
                .track_focus(self.task_speed_limit_cancel_focus.clone())
                .on_click(cx.listener(|this, _, window, cx| {
                    this.close_task_speed_limit(window, cx);
                }))
                .render(colors),
        )
        .action(
            Button::new(
                "submit-task-speed-limit",
                if pending { "Saving..." } else { "Save" },
            )
            .aria_label(if pending {
                self.t("dialog-speed-limit-saving")
            } else {
                self.t("dialog-speed-limit-save")
            })
            .style(ButtonStyle::Primary)
            .loading(pending)
            .track_focus(self.task_speed_limit_submit_focus.clone())
            .on_click(cx.listener(|this, _, _, cx| {
                this.submit_task_speed_limit(cx);
            }))
            .render(colors),
        )
        .into_any_element()
    }

    pub(crate) fn render_task_options_dialog(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let colors = self.theme.colors;
        let Some(dialog) = self.task_options_dialog.as_ref() else {
            return div().into_any_element();
        };
        let identity = dialog.identity.clone();
        let display_name = dialog.display_name.clone();
        let supports_seed_rules = dialog.supports_seed_rules;
        let error = dialog.error.clone();
        let pending = self.pending_task_command.as_ref().is_some_and(|pending| {
            pending.identity == identity
                && matches!(&pending.command, TaskCommandView::SetOptions { .. })
        });
        let content = div()
            .flex()
            .flex_col()
            .gap_3()
            .child(
                div()
                    .text_xs()
                    .text_color(colors.text_muted)
                    .child(if supports_seed_rules {
                        "Stops seeding when the first of seed-ratio or seed-time is reached. Use 0 for seed-ratio to disable the ratio condition."
                    } else {
                        "Seed-ratio and seed-time apply only to BitTorrent tasks."
                    }),
            )
            .when(supports_seed_rules, |element| {
                element.child(
                    div()
                        .flex()
                        .gap_3()
                        .child(
                            settings_labeled_input(
                                "Seed ratio",
                                self.task_inputs.seed_ratio.clone(),
                                colors,
                            )
                            .flex_1()
                            .min_w_0(),
                        )
                        .child(
                            settings_labeled_input(
                                "Seed time (minutes)",
                                self.task_inputs.seed_time.clone(),
                                colors,
                            )
                            .flex_1()
                            .min_w_0(),
                        ),
                )
            })
            .when_some(error, |element, error| {
                element.child(
                    div()
                        .id("task-options-error")
                        .role(Role::Alert)
                        .aria_label(self.te(&error))
                        .text_xs()
                        .text_color(colors.danger)
                        .child(self.te(&error)),
                )
            });

        Dialog::new(
            "task-options-dialog",
            self.t("dialog-task-options-title"),
            self.theme,
        )
        .description(format!("Change typed aria2 options for {display_name}."))
        .key_context("TaskOptionsDialog")
        .track_focus(self.task_options_dialog_focus.clone())
        .width(520.0)
        .child(content)
        .action(
            Button::new("cancel-task-options", "Cancel")
                .aria_label(self.t("dialog-task-options-cancel-aria"))
                .style(ButtonStyle::Secondary)
                .disabled(pending)
                .track_focus(self.task_options_cancel_focus.clone())
                .on_click(cx.listener(|this, _, window, cx| {
                    this.close_task_options(window, cx);
                }))
                .render(colors),
        )
        .action(
            Button::new(
                "submit-task-options",
                if pending { "Saving..." } else { "Save" },
            )
            .aria_label(if pending {
                self.t("dialog-task-options-saving")
            } else {
                self.t("dialog-task-options-save")
            })
            .style(ButtonStyle::Primary)
            .loading(pending)
            .disabled(!supports_seed_rules)
            .track_focus(self.task_options_submit_focus.clone())
            .on_click(cx.listener(|this, _, _, cx| {
                this.submit_task_options(cx);
            }))
            .render(colors),
        )
        .into_any_element()
    }
}
