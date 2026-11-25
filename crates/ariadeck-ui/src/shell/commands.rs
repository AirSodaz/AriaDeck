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
