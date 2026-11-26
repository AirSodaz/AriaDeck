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
        if let Some(ActiveModal::OutputName { input, .. }) = &self.active_modal {
            window.focus(&input.focus_handle(cx), cx);
            return;
        }
        if self.active_modal.is_some()
            || self.pending_task_command.is_some()
            || self.pending_batch_command.is_some()
        {
            return;
        }
        let Some(task) = self.selected_task_view(cx) else {
            self.show_notice(self.t("notice-select-task-first"), true, cx);
            return;
        };
        if !task.can_set_output_name() || !self.snapshot.commands_available() {
            self.show_notice(self.t("notice-output-name-unavailable"), true, cx);
            return;
        }

        let initial_name = if task.name_state.is_resolving() {
            String::new()
        } else {
            task.display_name.clone()
        };
        let input = make_input("archive.iso".into(), false, window, cx);
        input.update(cx, |input, cx| input.set_value(initial_name, window, cx));
        let sub = cx.subscribe(&input, |this: &mut Self, _input, event: &InputEvent, cx| {
            if let InputEvent::Change = event
                && this.pending_task_command.is_none()
                && let Some(ActiveModal::OutputName { dialog, .. }) = &mut this.active_modal
                && dialog.error.take().is_some()
            {
                cx.notify();
            }
        });
        self.active_modal = Some(ActiveModal::OutputName {
            dialog: TaskOutputNameDialog {
                identity: task.identity.clone(),
                display_name: task_display_name(&task, &self.translator),
                active: task.status.uses_active_connections(),
                error: None,
            },
            input: input.clone(),
            _sub: sub,
        });
        self.sync_kit_modals(window, cx);
        cx.notify();
        if !has_kit_root(window) {
            cx.defer_in(window, move |this, window, cx| {
                if let Some(ActiveModal::OutputName { input: active, .. }) = &this.active_modal
                    && active == &input
                {
                    window.focus(&active.focus_handle(cx), cx);
                }
            });
        }
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
        if !matches!(self.active_modal, Some(ActiveModal::OutputName { .. }))
            || self.pending_task_command.as_ref().is_some_and(|pending| {
                matches!(&pending.command, TaskCommandView::SetOutputName { .. })
            })
        {
            return;
        }
        self.active_modal = None;
        self.sync_kit_modals(window, cx);
        if !has_kit_root(window) {
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
        let Some((identity, output_name)) = (match &self.active_modal {
            Some(ActiveModal::OutputName { dialog, input, .. }) => Some((
                dialog.identity.clone(),
                input.read(cx).value().trim().to_owned(),
            )),
            _ => None,
        }) else {
            return;
        };
        if let Some(key) = output_name_validation_error(&output_name) {
            let summary = self.t(key);
            if let Some(ActiveModal::OutputName { dialog, .. }) = &mut self.active_modal {
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
            let summary = self.t("error-command-task-changed");
            if let Some(ActiveModal::OutputName { dialog, .. }) = &mut self.active_modal {
                dialog.error = Some(OperationErrorView {
                    code: "command.task_changed".into(),
                    summary,
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
        if let Some(ActiveModal::TaskSpeedLimit { download_input, .. }) = &self.active_modal {
            window.focus(&download_input.focus_handle(cx), cx);
            return;
        }
        if self.active_modal.is_some()
            || self.pending_task_command.is_some()
            || self.pending_batch_command.is_some()
        {
            return;
        }
        let Some(task) = self.selected_task_view(cx) else {
            self.show_notice(self.t("notice-select-task-first"), true, cx);
            return;
        };
        if !task.status.can_set_speed_limit() || !self.snapshot.commands_available() {
            self.show_notice(self.t("notice-speed-limit-unavailable"), true, cx);
            return;
        }
        // The list projection does not carry per-task limits (that is DETAIL-001's
        // getOption surface), so the fields start blank and set a fresh value.
        let download_input = make_input(
            self.t("input-download-limit-placeholder").into(),
            false,
            window,
            cx,
        );
        let upload_input = make_input(
            self.t("input-upload-limit-placeholder").into(),
            false,
            window,
            cx,
        );
        let subs = [&download_input, &upload_input].map(|input| {
            cx.subscribe(input, |this: &mut Self, _input, event: &InputEvent, cx| {
                if let InputEvent::Change = event
                    && this.pending_task_command.is_none()
                    && let Some(ActiveModal::TaskSpeedLimit { dialog, .. }) = &mut this.active_modal
                    && dialog.error.take().is_some()
                {
                    cx.notify();
                }
            })
        });
        self.active_modal = Some(ActiveModal::TaskSpeedLimit {
            dialog: TaskSpeedLimitDialog {
                identity: task.identity.clone(),
                display_name: task_display_name(&task, &self.translator),
                error: None,
            },
            download_input: download_input.clone(),
            upload_input,
            _subs: subs,
        });
        self.sync_kit_modals(window, cx);
        cx.notify();
        if !has_kit_root(window) {
            cx.defer_in(window, move |this, window, cx| {
                if let Some(ActiveModal::TaskSpeedLimit {
                    download_input: active,
                    ..
                }) = &this.active_modal
                    && active == &download_input
                {
                    window.focus(&active.focus_handle(cx), cx);
                }
            });
        }
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
        if !matches!(self.active_modal, Some(ActiveModal::TaskSpeedLimit { .. }))
            || self.pending_task_command.as_ref().is_some_and(|pending| {
                matches!(&pending.command, TaskCommandView::SetSpeedLimit { .. })
            })
        {
            return;
        }
        self.active_modal = None;
        self.sync_kit_modals(window, cx);
        if !has_kit_root(window) {
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
        let Some((identity, draft)) = (match &self.active_modal {
            Some(ActiveModal::TaskSpeedLimit {
                dialog,
                download_input,
                upload_input,
                ..
            }) => Some((
                dialog.identity.clone(),
                SpeedLimitSettingsView {
                    download_limit: download_input.read(cx).value().trim().into(),
                    upload_limit: upload_input.read(cx).value().trim().into(),
                },
            )),
            _ => None,
        }) else {
            return;
        };
        let (Some(download_limit), Some(upload_limit)) =
            (draft.parse_download_limit(), draft.parse_upload_limit())
        else {
            let summary = self.t("error-validation-invalid-speed-limit");
            if let Some(ActiveModal::TaskSpeedLimit { dialog, .. }) = &mut self.active_modal {
                dialog.error = Some(OperationErrorView {
                    code: "validation.invalid_speed_limit".into(),
                    summary,
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
            let summary = self.t("error-command-task-changed");
            if let Some(ActiveModal::TaskSpeedLimit { dialog, .. }) = &mut self.active_modal {
                dialog.error = Some(OperationErrorView {
                    code: "command.task_changed".into(),
                    summary,
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

    pub(crate) fn close_task_options_action(
        &mut self,
        _: &CloseTaskOptions,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.close_task_options(window, cx);
    }

    pub(crate) fn submit_task_options_action(
        &mut self,
        _: &SubmitTaskOptions,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.submit_task_options(cx);
    }

    pub(crate) fn open_task_options(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(ActiveModal::TaskOptions {
            seed_ratio_input, ..
        }) = &self.active_modal
        {
            window.focus(&seed_ratio_input.focus_handle(cx), cx);
            return;
        }
        if self.active_modal.is_some()
            || self.pending_task_command.is_some()
            || self.pending_batch_command.is_some()
        {
            return;
        }
        let Some(task) = self.selected_task_view(cx) else {
            self.show_notice(self.t("notice-select-task-first"), true, cx);
            return;
        };
        if !task.status.can_set_speed_limit() || !self.snapshot.commands_available() {
            self.show_notice(self.t("notice-task-options-unavailable"), true, cx);
            return;
        }
        let supports_seed_rules = matches!(
            task.source_kind,
            crate::TaskSourceKindView::Magnet | crate::TaskSourceKindView::BitTorrent
        ) || task.status == TaskStatusView::Seeding;
        // Prefill from the open details drawer options projection when present.
        let (seed_ratio, seed_time) = self
            .details_panel
            .as_ref()
            .and_then(|panel| match &panel.read(cx).state {
                TaskDetailsLoadState::Ready { details } => Some(Arc::clone(details)),
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
        let seed_ratio_input = make_input(
            self.t("dialog-task-options-seed-ratio-placeholder").into(),
            false,
            window,
            cx,
        );
        let seed_time_input = make_input(
            self.t("dialog-task-options-seed-time-placeholder").into(),
            false,
            window,
            cx,
        );
        if supports_seed_rules {
            seed_ratio_input.update(cx, |input, cx| input.set_value(seed_ratio, window, cx));
            seed_time_input.update(cx, |input, cx| input.set_value(seed_time, window, cx));
        }
        let subs = [&seed_ratio_input, &seed_time_input].map(|input| {
            cx.subscribe(input, |this: &mut Self, _input, event: &InputEvent, cx| {
                if let InputEvent::Change = event
                    && this.pending_task_command.is_none()
                    && let Some(ActiveModal::TaskOptions { dialog, .. }) = &mut this.active_modal
                    && dialog.error.take().is_some()
                {
                    cx.notify();
                }
            })
        });
        self.active_modal = Some(ActiveModal::TaskOptions {
            dialog: TaskOptionsDialog {
                identity: task.identity.clone(),
                display_name: task_display_name(&task, &self.translator),
                supports_seed_rules,
                error: None,
            },
            seed_ratio_input: seed_ratio_input.clone(),
            seed_time_input,
            _subs: subs,
        });
        self.sync_kit_modals(window, cx);
        cx.notify();
        if !has_kit_root(window) {
            cx.defer_in(window, move |this, window, cx| {
                if let Some(ActiveModal::TaskOptions {
                    seed_ratio_input: active,
                    ..
                }) = &this.active_modal
                    && active == &seed_ratio_input
                {
                    window.focus(&active.focus_handle(cx), cx);
                }
            });
        }
    }

    pub(crate) fn close_task_options(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !matches!(self.active_modal, Some(ActiveModal::TaskOptions { .. }))
            || self.pending_task_command.as_ref().is_some_and(|pending| {
                matches!(&pending.command, TaskCommandView::SetOptions { .. })
            })
        {
            return;
        }
        self.active_modal = None;
        self.sync_kit_modals(window, cx);
        if !has_kit_root(window) {
            window.focus(&self.focus_handle, cx);
        }
        cx.notify();
    }

    pub(crate) fn submit_task_options(&mut self, cx: &mut Context<Self>) {
        if self.pending_task_command.is_some() {
            return;
        }
        let Some((identity, supports_seed_rules, seed_ratio_raw, seed_time_raw)) =
            (match &self.active_modal {
                Some(ActiveModal::TaskOptions {
                    dialog,
                    seed_ratio_input,
                    seed_time_input,
                    ..
                }) => Some((
                    dialog.identity.clone(),
                    dialog.supports_seed_rules,
                    seed_ratio_input.read(cx).value().trim().to_owned(),
                    seed_time_input.read(cx).value().trim().to_owned(),
                )),
                _ => None,
            })
        else {
            return;
        };
        let mut seed_ratio = None;
        let mut seed_time_minutes = None;
        if !supports_seed_rules {
            let summary = self.t("error-command-seed-rules-unsupported");
            if let Some(ActiveModal::TaskOptions { dialog, .. }) = &mut self.active_modal {
                dialog.error = Some(OperationErrorView {
                    code: "command.seed_rules_unsupported".into(),
                    summary,
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
                    let summary = self.t("error-validation-invalid-seed-ratio");
                    if let Some(ActiveModal::TaskOptions { dialog, .. }) = &mut self.active_modal {
                        dialog.error = Some(OperationErrorView {
                            code: "validation.invalid_seed_ratio".into(),
                            summary,
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
                    let summary = self.t("error-validation-invalid-seed-time");
                    if let Some(ActiveModal::TaskOptions { dialog, .. }) = &mut self.active_modal {
                        dialog.error = Some(OperationErrorView {
                            code: "validation.invalid_seed_time".into(),
                            summary,
                            retryable: false,
                        });
                    }
                    cx.notify();
                    return;
                }
            }
        }
        if seed_ratio.is_none() && seed_time_minutes.is_none() {
            let summary = self.t("error-validation-empty-task-options");
            if let Some(ActiveModal::TaskOptions { dialog, .. }) = &mut self.active_modal {
                dialog.error = Some(OperationErrorView {
                    code: "validation.empty_task_options".into(),
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
            || current_task.is_none_or(|task| !task.status.can_set_speed_limit())
        {
            let summary = self.t("error-command-task-changed");
            if let Some(ActiveModal::TaskOptions { dialog, .. }) = &mut self.active_modal {
                dialog.error = Some(OperationErrorView {
                    code: "command.task_changed".into(),
                    summary,
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

    #[cfg(test)]
    pub(crate) fn open_task_context_menu(
        &mut self,
        index: usize,
        _position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(identity) = self
            .snapshot
            .tasks
            .get(index)
            .map(|task| task.identity.clone())
        else {
            return;
        };
        let _ = self.prepare_task_context_menu(&identity, window, cx);
    }

    pub(crate) fn prepare_task_context_menu(
        &mut self,
        identity: &TaskIdentity,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<TaskIdentity> {
        if self.page != AppPage::Downloads || self.active_modal.is_some() {
            return None;
        }
        let index = self
            .snapshot
            .tasks
            .iter()
            .position(|task| &task.identity == identity)?;
        // Right-click focuses the row without clearing a multi-selection that
        // already includes it (qBittorrent/Motrix parity).
        if !self.selected_tasks.contains(identity) {
            self.select_at_with_modifiers(index, false, false, window, cx);
        } else {
            self.selected = Some(identity.clone());
        }
        self.active_status_popover = None;
        self.context_menu_target = Some(identity.clone());
        window.focus(&self.focus_handle, cx);
        cx.notify();
        Some(identity.clone())
    }

    pub(crate) fn close_task_context_menu(&mut self, cx: &mut Context<Self>) {
        if self.context_menu_target.take().is_some() {
            cx.notify();
        }
    }

    /// Prefer the right-clicked menu identity so multi-selection still has a
    /// single authoritative target for copy/open/details actions.
    pub(crate) fn context_menu_task_view(&self, cx: &App) -> Option<DownloadRowView> {
        if let Some(target) = self.context_menu_target.as_ref()
            && let Some(task) = self
                .snapshot
                .tasks
                .iter()
                .find(|task| &task.identity == target)
        {
            return Some(task.clone());
        }
        self.selected_task_view(cx)
            .or_else(|| self.command_target_task_view(cx))
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
            self.show_notice(self.t("notice-open-path-unavailable"), true, cx);
            return;
        }
        let Some(session) = self.snapshot.engine_session() else {
            return;
        };
        let Some(task) = self.context_menu_task_view(cx) else {
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
            || matches!(self.active_modal, Some(ActiveModal::BatchFailureDetails(_)))
        {
            return;
        }
        let Some(task) = self.command_target_task_view(cx) else {
            self.show_notice(self.t("notice-select-task-first"), true, cx);
            return;
        };
        let capability_block = match command {
            TaskCommandView::ForcePause if !self.snapshot.capabilities.force_pause => {
                Some("notice-force-pause-unsupported")
            }
            TaskCommandView::ForceRemoveTask if !self.snapshot.capabilities.force_remove => {
                Some("notice-force-remove-unsupported")
            }
            TaskCommandView::MoveToQueueTop
            | TaskCommandView::MoveUpInQueue
            | TaskCommandView::MoveDownInQueue
            | TaskCommandView::MoveToQueueBottom
                if !self.snapshot.capabilities.queue_positioning =>
            {
                Some("notice-queue-positioning-unsupported")
            }
            TaskCommandView::SetSpeedLimit { .. } | TaskCommandView::SetOptions { .. }
                if !self.snapshot.capabilities.change_option =>
            {
                Some("notice-change-option-unsupported")
            }
            _ => None,
        };
        if let Some(key) = capability_block {
            self.show_notice(self.t(key), true, cx);
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
            TaskCommandView::SetOptions { .. } => task.status.can_set_speed_limit(),
            TaskCommandView::RemoveTask
            | TaskCommandView::ForceRemoveTask
            | TaskCommandView::RemoveTaskAndFiles => task.status.can_remove(),
        };
        if !allowed {
            let action = self.t(task_command_label_key(&command));
            let status = self.t(task.status.message_key());
            self.show_notice(
                self.t_args(
                    "notice-command-status-unavailable",
                    &[
                        ("action", FluentValue::from(action)),
                        ("status", FluentValue::from(status)),
                    ],
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
            || matches!(self.active_modal, Some(ActiveModal::BatchFailureDetails(_)))
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
                self.t("notice-capability-force-pause-all-unsupported"),
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
            || matches!(self.active_modal, Some(ActiveModal::BatchFailureDetails(_)))
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
            self.show_notice(self.t("notice-batch-select-two"), true, cx);
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
                Some("notice-force-pause-unsupported")
            }
            BatchTaskCommandView::ForceRemoveTask if !self.snapshot.capabilities.force_remove => {
                Some("notice-force-remove-unsupported")
            }
            _ => None,
        };
        if let Some(key) = capability_block {
            self.show_notice(self.t(key), true, cx);
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
        if self.active_modal.is_some()
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
                self.active_modal = Some(ActiveModal::RemoveConfirmation(RemoveConfirmation {
                    display_name: self.t_count(
                        "dialog-selected-task-count",
                        u64::try_from(identities.len()).unwrap_or(u64::MAX),
                    ),
                    identities,
                    has_live_tasks: selected_tasks.iter().any(|task| !task.status.is_terminal()),
                    has_terminal_tasks: selected_tasks.iter().any(|task| task.status.is_terminal()),
                    delete_files: false,
                }));
                self.sync_kit_modals(window, cx);
                cx.notify();
            }
            return;
        }
        let Some(task) = self.command_target_task_view(cx) else {
            if !self.selected_tasks.is_empty() {
                self.show_notice(self.t("notice-hidden-selection"), true, cx);
            }
            return;
        };
        if !task.status.can_remove() || !self.snapshot.commands_available() {
            self.show_notice(self.t("notice-remove-unavailable"), true, cx);
            return;
        }

        let display_name = task_display_name(&task, &self.translator);
        self.active_modal = Some(ActiveModal::RemoveConfirmation(RemoveConfirmation {
            identities: vec![task.identity],
            display_name,
            has_live_tasks: !task.status.is_terminal(),
            has_terminal_tasks: task.status.is_terminal(),
            delete_files: false,
        }));
        self.sync_kit_modals(window, cx);
        cx.notify();
    }

    pub(crate) fn close_remove_confirmation(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !matches!(self.active_modal, Some(ActiveModal::RemoveConfirmation(_))) {
            return;
        }
        self.active_modal = None;
        self.sync_kit_modals(window, cx);
        if !has_kit_root(window) {
            window.focus(&self.focus_handle, cx);
        }
        cx.notify();
    }

    pub(crate) fn submit_remove_confirmation(&mut self, cx: &mut Context<Self>) {
        let confirmation = match self.active_modal.take() {
            Some(ActiveModal::RemoveConfirmation(confirmation)) => confirmation,
            other => {
                self.active_modal = other;
                return;
            }
        };
        self.sync_kit_modals_deferred(cx);
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
            self.show_notice(self.t("notice-removal-selection-changed"), true, cx);
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
        if let Some(ActiveModal::RemoveConfirmation(confirmation)) = &mut self.active_modal {
            confirmation.delete_files = !confirmation.delete_files;
            cx.notify();
        }
    }

    pub(crate) fn open_status_popover(
        &mut self,
        popover: StatusBarPopover,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let previous = window.focused(cx).map(|focus| focus.downgrade());
        match popover {
            StatusBarPopover::Speed => self.speed_popover_previous_focus = previous,
            StatusBarPopover::Power => self.power_popover_previous_focus = previous,
            StatusBarPopover::Activity => self.activity_popover_previous_focus = previous,
        }
        self.context_menu_target = None;
        self.active_status_popover = Some(popover);
        cx.notify();
    }

    pub(crate) fn restore_popover_focus(
        &mut self,
        popover: StatusBarPopover,
        window: &mut Window,
        cx: &mut App,
    ) {
        let slot = match popover {
            StatusBarPopover::Speed => &mut self.speed_popover_previous_focus,
            StatusBarPopover::Power => &mut self.power_popover_previous_focus,
            StatusBarPopover::Activity => &mut self.activity_popover_previous_focus,
        };
        if let Some(focus) = slot.take().and_then(|focus| focus.upgrade()) {
            window.focus(&focus, cx);
        } else {
            window.focus(&self.focus_handle, cx);
        }
    }

    #[cfg(test)]
    pub(crate) fn toggle_speed_popover(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.active_status_popover == Some(StatusBarPopover::Speed) {
            self.close_speed_popover(window, cx);
            return;
        }
        self.open_status_popover(StatusBarPopover::Speed, window, cx);
    }

    pub(crate) fn close_speed_popover(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.active_status_popover != Some(StatusBarPopover::Speed) {
            return;
        }
        self.active_status_popover = None;
        self.restore_popover_focus(StatusBarPopover::Speed, window, cx);
        cx.notify();
    }

    pub(crate) fn selected_task_view(&self, cx: &App) -> Option<DownloadRowView> {
        let selected = self.selected.as_ref()?;
        self.snapshot
            .tasks
            .iter()
            .find(|task| &task.identity == selected)
            .cloned()
            .or_else(|| {
                self.details_panel
                    .as_ref()
                    .map(|panel| panel.read(cx))
                    .filter(|drawer| &drawer.identity == selected)
                    .map(|drawer| drawer.overview.clone())
            })
    }

    pub(crate) fn command_target_task_view(&self, cx: &App) -> Option<DownloadRowView> {
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
        self.selected_task_view(cx)
    }

    pub(crate) fn open_edit_task_action(
        &mut self,
        _: &OpenEditTask,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_edit_task(window, cx);
    }

    pub(crate) fn close_edit_task_action(
        &mut self,
        _: &CloseEditTask,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.close_edit_task(window, cx);
    }

    pub(crate) fn submit_edit_task_action(
        &mut self,
        _: &SubmitEditTask,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.submit_edit_task(cx);
    }

    pub(crate) fn open_edit_task(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(task) = self.selected_task_view(cx) {
            self.open_edit_task_for(task, window, cx);
        } else {
            self.show_notice(self.t("notice-select-task-first"), true, cx);
        }
    }

    pub(crate) fn open_edit_task_for(
        &mut self,
        task: DownloadRowView,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(ActiveModal::EditTask { url_input, .. }) = &self.active_modal {
            window.focus(&url_input.focus_handle(cx), cx);
            return;
        }
        if self.active_modal.is_some()
            || self.pending_task_command.is_some()
            || self.pending_batch_command.is_some()
        {
            return;
        }
        if !self.snapshot.commands_available() || !task.can_edit_or_relink() {
            self.show_notice(self.t("notice-edit-task-unavailable"), true, cx);
            return;
        }
        let Some(session) = self.snapshot.engine_session() else {
            self.show_notice(self.t("notice-connect-before-command"), true, cx);
            return;
        };

        let is_error_resume = task.status == TaskStatusView::Failed;
        let can_relink_url = task.can_relink_url();
        let can_edit_out = task.can_set_output_name()
            && matches!(
                task.status,
                TaskStatusView::Paused | TaskStatusView::Waiting | TaskStatusView::Active
            );

        let url_input = make_input("https://example.test/file.zip".into(), false, window, cx);
        if let Some(primary) = task.primary_source.as_deref()
            && primary != ariadeck_domain::REDACTED_SOURCE_PLACEHOLDER
        {
            url_input.update(cx, |input, cx| {
                input.set_value(primary.to_owned(), window, cx)
            });
        }
        let referer_input = make_input("https://example.test/".into(), false, window, cx);
        let user_agent_input = make_input(
            self.t("dialog-add-user-agent-placeholder").into(),
            false,
            window,
            cx,
        );
        let headers_input = cx.new(|cx| {
            TextareaState::new(window, cx).placeholder(self.t("dialog-add-headers-placeholder"))
        });
        let proxy_input = make_input("http://proxy.example:8080".into(), false, window, cx);
        let speed_limit_input = make_input(
            self.t("input-download-limit-placeholder").into(),
            false,
            window,
            cx,
        );
        let connections_input = make_input("1-16".into(), false, window, cx);
        let split_input = make_input("1-64".into(), false, window, cx);
        let out_input = make_input(task.display_name.clone().into(), false, window, cx);
        if can_edit_out && !task.name_state.is_resolving() {
            out_input.update(cx, |input, cx| {
                input.set_value(task.display_name.clone(), window, cx)
            });
        }

        let mut subs = Vec::new();
        for input in [
            &url_input,
            &referer_input,
            &user_agent_input,
            &proxy_input,
            &speed_limit_input,
            &connections_input,
            &split_input,
            &out_input,
        ] {
            let is_url = input == &url_input;
            subs.push(
                cx.subscribe(input, move |this: &mut Self, _, event: &InputEvent, cx| {
                    if let InputEvent::Change = event
                        && let Some(ActiveModal::EditTask { dialog, .. }) = &mut this.active_modal
                        && dialog.submit_request.is_none()
                    {
                        let mut changed = dialog.error.take().is_some();
                        if is_url {
                            changed |= dialog.mismatch_warning.take().is_some();
                        }
                        if changed {
                            cx.notify();
                        }
                    }
                }),
            );
        }
        subs.push(cx.subscribe(
            &headers_input,
            |this: &mut Self, _, event: &InputEvent, cx| {
                if let InputEvent::Change = event
                    && let Some(ActiveModal::EditTask { dialog, .. }) = &mut this.active_modal
                    && dialog.submit_request.is_none()
                    && dialog.error.take().is_some()
                {
                    cx.notify();
                }
            },
        ));

        let loading_id = self.allocate_request_id();
        let expected_total_bytes = (task.total_bytes > 0).then_some(task.total_bytes);
        let expected_filename =
            (!task.name_state.is_resolving()).then(|| task.display_name.clone());
        self.active_modal = Some(ActiveModal::EditTask {
            dialog: Box::new(EditTaskDialog {
                identity: task.identity.clone(),
                display_name: task_display_name(&task, &self.translator),
                can_relink_url,
                is_error_resume,
                can_edit_out,
                original_primary_uri: task.primary_source.clone(),
                expected_total_bytes,
                expected_filename,
                initial_state: None,
                loading_request: Some(loading_id),
                submit_request: None,
                mismatch_warning: None,
                error: None,
            }),
            url_input: url_input.clone(),
            referer_input,
            user_agent_input,
            headers_input,
            proxy_input,
            speed_limit_input,
            connections_input,
            split_input,
            out_input,
            _subs: subs,
        });
        self.sync_kit_modals(window, cx);
        cx.emit(AppShellEvent::PrepareEditTaskRequested(
            PrepareEditTaskRequestView {
                request_id: loading_id,
                session,
                identity: task.identity,
            },
        ));
        cx.notify();
    }

    pub(crate) fn close_edit_task(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !matches!(self.active_modal, Some(ActiveModal::EditTask { .. }))
            || matches!(
                &self.active_modal,
                Some(ActiveModal::EditTask { dialog, .. }) if dialog.submit_request.is_some()
            )
        {
            return;
        }
        self.active_modal = None;
        self.sync_kit_modals(window, cx);
        if !has_kit_root(window) {
            window.focus(&self.focus_handle, cx);
        }
        cx.notify();
    }

    pub fn set_prepare_edit_task_result(
        &mut self,
        result: PrepareEditTaskResultView,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(ActiveModal::EditTask {
            dialog,
            url_input,
            referer_input,
            user_agent_input,
            headers_input,
            proxy_input,
            speed_limit_input,
            connections_input,
            split_input,
            out_input,
            ..
        }) = &mut self.active_modal
        else {
            return;
        };
        if dialog.loading_request != Some(result.request_id) || dialog.identity != result.identity {
            return;
        }
        dialog.loading_request = None;
        match result.outcome {
            Ok(state) => {
                if let Some(first_uri) = state.uris.first() {
                    dialog.original_primary_uri = Some(first_uri.clone());
                    url_input.update(cx, |input, cx| {
                        input.set_value(first_uri.clone(), window, cx)
                    });
                }
                referer_input.update(cx, |input, cx| {
                    input.set_value(state.referer.clone(), window, cx)
                });
                user_agent_input.update(cx, |input, cx| {
                    input.set_value(state.user_agent.clone(), window, cx)
                });
                headers_input.update(cx, |input, cx| {
                    input.set_value(state.headers.clone(), window, cx)
                });
                proxy_input.update(cx, |input, cx| {
                    input.set_value(state.proxy.clone(), window, cx)
                });
                speed_limit_input.update(cx, |input, cx| {
                    input.set_value(state.max_download_limit.clone(), window, cx)
                });
                connections_input.update(cx, |input, cx| {
                    input.set_value(state.max_connection_per_server.clone(), window, cx)
                });
                split_input.update(cx, |input, cx| {
                    input.set_value(state.split.clone(), window, cx)
                });
                if !state.out.is_empty() {
                    out_input.update(cx, |input, cx| {
                        input.set_value(state.out.clone(), window, cx)
                    });
                }
                dialog.initial_state = Some(state);
            }
            Err(error) => {
                // Non-fatal on Error tasks whose GID was purged from aria2 memory.
                if !dialog.is_error_resume {
                    dialog.error = Some(error);
                }
            }
        }
        cx.notify();
    }

    pub(crate) fn submit_edit_task(&mut self, cx: &mut Context<Self>) {
        let Some(session) = self.snapshot.engine_session() else {
            return;
        };
        let Some(ActiveModal::EditTask {
            dialog,
            url_input,
            referer_input,
            user_agent_input,
            headers_input,
            proxy_input,
            speed_limit_input,
            connections_input,
            split_input,
            out_input,
            ..
        }) = &self.active_modal
        else {
            return;
        };
        if dialog.loading_request.is_some() || dialog.submit_request.is_some() {
            return;
        }

        let new_url_raw = url_input.read(cx).value().trim().to_owned();
        let referer_raw = referer_input.read(cx).value().trim().to_owned();
        let ua_raw = user_agent_input.read(cx).value().trim().to_owned();
        let headers_raw = headers_input.read(cx).value().to_string();
        let proxy_raw = proxy_input.read(cx).value().trim().to_owned();
        let speed_raw = speed_limit_input.read(cx).value().trim().to_owned();
        let connections_raw = connections_input.read(cx).value().trim().to_owned();
        let split_raw = split_input.read(cx).value().trim().to_owned();
        let out_raw = out_input.read(cx).value().trim().to_owned();

        let initial = dialog.initial_state.clone().unwrap_or_default();
        let initial_url = initial
            .uris
            .first()
            .cloned()
            .or_else(|| dialog.original_primary_uri.clone())
            .unwrap_or_default();

        let new_url = if dialog.can_relink_url
            && !new_url_raw.is_empty()
            && (new_url_raw != initial_url || dialog.is_error_resume)
        {
            Some(new_url_raw)
        } else {
            None
        };
        let referer = (referer_raw != initial.referer).then_some(referer_raw);
        let user_agent = (ua_raw != initial.user_agent).then_some(ua_raw);
        let headers = (headers_raw.trim() != initial.headers.trim()).then_some(headers_raw);
        let proxy = (proxy_raw != initial.proxy).then_some(proxy_raw);
        let max_download_limit = (speed_raw != initial.max_download_limit).then_some(speed_raw);
        let max_connection_per_server =
            (connections_raw != initial.max_connection_per_server).then_some(connections_raw);
        let split = (split_raw != initial.split).then_some(split_raw);
        let out_unchanged = if !initial.out.is_empty() {
            out_raw == initial.out
        } else {
            out_raw == dialog.display_name
        };
        let out = (dialog.can_edit_out && !out_raw.is_empty() && !out_unchanged).then_some(out_raw);

        if new_url.is_none()
            && referer.is_none()
            && user_agent.is_none()
            && headers.is_none()
            && proxy.is_none()
            && max_download_limit.is_none()
            && max_connection_per_server.is_none()
            && split.is_none()
            && out.is_none()
        {
            let summary = self.t("dialog-edit-task-no-changes");
            if let Some(ActiveModal::EditTask { dialog, .. }) = &mut self.active_modal {
                dialog.error = Some(OperationErrorView {
                    code: "validation.empty_task_options".into(),
                    summary,
                    retryable: false,
                });
            }
            cx.notify();
            return;
        }

        if new_url.is_some() && !dialog.is_error_resume && !self.snapshot.capabilities.change_uri {
            let summary = self
                .snapshot
                .capabilities
                .unsupported_change_uri_message()
                .to_owned();
            if let Some(ActiveModal::EditTask { dialog, .. }) = &mut self.active_modal {
                dialog.error = Some(OperationErrorView {
                    code: "command.unsupported".into(),
                    summary,
                    retryable: false,
                });
            }
            cx.notify();
            return;
        }

        let identity = dialog.identity.clone();
        let original_primary_uri = dialog.original_primary_uri.clone();
        let expected_total_bytes = dialog.expected_total_bytes;
        let expected_filename = dialog.expected_filename.clone();
        let allow_error_relink_resume = dialog.is_error_resume;
        let force_mismatch = dialog.mismatch_warning.is_some();

        let request_id = self.allocate_request_id();
        let req = RelinkEditTaskRequestView {
            request_id,
            session,
            identity,
            original_primary_uri,
            expected_total_bytes,
            expected_filename,
            new_url,
            referer,
            user_agent,
            headers,
            proxy,
            max_download_limit,
            max_connection_per_server,
            split,
            out,
            allow_error_relink_resume,
            force_mismatch,
        };
        if let Some(ActiveModal::EditTask { dialog, .. }) = &mut self.active_modal {
            dialog.submit_request = Some(request_id);
            dialog.error = None;
        }
        cx.emit(AppShellEvent::RelinkEditTaskRequested(req));
        cx.notify();
    }

    pub fn set_relink_edit_task_result(
        &mut self,
        result: RelinkEditTaskResultView,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(ActiveModal::EditTask { dialog, .. }) = &mut self.active_modal else {
            return;
        };
        if dialog.submit_request != Some(result.request_id) {
            return;
        }
        dialog.submit_request = None;
        match result.outcome {
            RelinkEditOutcomeView::NeedsMismatchConfirmation {
                warning_message,
                probed_filename,
                probed_total_bytes,
            } => {
                dialog.mismatch_warning =
                    Some((warning_message, probed_filename, probed_total_bytes));
                cx.notify();
            }
            RelinkEditOutcomeView::Succeeded(CommandOutcomeView::Success { tasks }) => {
                self.active_modal = None;
                self.sync_kit_modals(window, cx);
                if !has_kit_root(window) {
                    window.focus(&self.focus_handle, cx);
                }
                let activity_target = tasks.first().cloned().unwrap_or(result.identity);
                let message = self.t("notice-task-relinked");
                self.record_activity(
                    ActivityKindView::Command,
                    message.clone(),
                    None,
                    Some(activity_target),
                    1,
                    cx,
                );
                self.show_notice(message, false, cx);
                cx.notify();
            }
            RelinkEditOutcomeView::Succeeded(CommandOutcomeView::Failure(error)) => {
                dialog.error = Some(error);
                cx.notify();
            }
        }
    }
}

const fn task_command_label_key(command: &TaskCommandView) -> &'static str {
    match command {
        TaskCommandView::Pause => "action-pause",
        TaskCommandView::ForcePause => "action-force-pause",
        TaskCommandView::Resume => "action-resume",
        TaskCommandView::MoveToQueueTop => "action-move-queue-top",
        TaskCommandView::MoveUpInQueue => "action-move-queue-up",
        TaskCommandView::MoveDownInQueue => "action-move-queue-down",
        TaskCommandView::MoveToQueueBottom => "action-move-queue-bottom",
        TaskCommandView::Retry => "action-retry",
        TaskCommandView::SetOutputName { .. } => "action-change-output-name",
        TaskCommandView::SetSpeedLimit { .. } => "action-set-speed-limits",
        TaskCommandView::SetOptions { .. } => "action-edit-task-options",
        TaskCommandView::RemoveTask => "action-remove",
        TaskCommandView::ForceRemoveTask => "action-force-remove",
        TaskCommandView::RemoveTaskAndFiles => "action-remove-with-files",
    }
}

pub(super) const fn batch_task_command_label_key(command: BatchTaskCommandView) -> &'static str {
    match command {
        BatchTaskCommandView::Pause => "action-pause",
        BatchTaskCommandView::ForcePause => "action-force-pause",
        BatchTaskCommandView::Resume => "action-resume",
        BatchTaskCommandView::Retry => "action-retry",
        BatchTaskCommandView::RemoveTask => "action-remove",
        BatchTaskCommandView::ForceRemoveTask => "action-force-remove",
        BatchTaskCommandView::RemoveTaskAndFiles => "action-remove-with-files",
    }
}
