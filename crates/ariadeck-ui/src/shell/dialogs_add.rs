//! Add-download dialog logic for AppShell.

use ariadeck_i18n::FluentValue;

use super::*;

fn fluent_number(value: usize) -> i64 {
    i64::try_from(value).unwrap_or(i64::MAX)
}

impl AppShell {
    pub(crate) fn open_add_download(
        &mut self,
        _: &OpenAddDownload,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.page = AppPage::Downloads;
        self.speed_popover_open = false;
        if self.add_dialog.open {
            window.focus(&self.add_input.focus_handle(cx), cx);
            return;
        }
        if self.output_name_dialog.is_some()
            || self.remove_confirmation.is_some()
            || self.batch_failure_details.is_some()
        {
            return;
        }
        if self.pending_task_command.is_some() || self.pending_batch_command.is_some() {
            return;
        }
        if !self.snapshot.commands_available() {
            self.show_notice(self.t("notice-connect-before-add"), true, cx);
            return;
        }

        self.add_input
            .update(cx, |input, cx| input.set_text("", cx));
        for input in [
            &self.add_inputs.referer,
            &self.add_inputs.user_agent,
            &self.add_inputs.headers,
            &self.add_inputs.cookie,
            &self.add_inputs.http_user,
            &self.add_inputs.http_passwd,
            &self.add_inputs.checksum,
        ] {
            input.update(cx, |input, cx| input.set_text("", cx));
        }
        self.add_dialog = AddDownloadDialog {
            open: true,
            input_mode: AddDownloadInputModeView::Links,
            mode: AddDownloadModeView::SeparateTasks,
            file_conflict: FileConflictPolicyView::AutoRename,
            advanced_open: false,
            metadata_files: Vec::new(),
            active_metadata_file: None,
            preview_pending: None,
            previous_focus: window.focused(cx).map(|focus| focus.downgrade()),
            pending: None,
            error: None,
            results: Vec::new(),
            updating_input_from_result: false,
        };
        cx.notify();
        cx.defer_in(window, |this, window, cx| {
            if this.add_dialog.open && this.add_dialog.input_mode == AddDownloadInputModeView::Links
            {
                window.focus(&this.add_input.focus_handle(cx), cx);
            }
        });
    }

    pub(crate) fn close_add_download_action(
        &mut self,
        _: &CloseAddDownload,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.close_add_download(window, cx);
    }

    pub(crate) fn close_add_download(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.add_dialog.open
            || self.add_dialog.pending.is_some()
            || self.add_dialog.preview_pending.is_some()
        {
            return;
        }
        let restore_focus = self.add_dialog_focus.contains_focused(window, cx)
            || self.add_input.focus_handle(cx).is_focused(window);
        let previous_focus = self.add_dialog.previous_focus.take();
        self.add_dialog = AddDownloadDialog::default();
        if restore_focus {
            if let Some(focus) = previous_focus.and_then(|focus| focus.upgrade()) {
                window.focus(&focus, cx);
            } else {
                window.focus(&self.focus_handle, cx);
            }
        }
        cx.notify();
    }

    pub(crate) fn submit_add_download_action(
        &mut self,
        _: &SubmitAddDownload,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.submit_add_download(cx);
    }

    pub(crate) fn submit_add_download(&mut self, cx: &mut Context<Self>) {
        if !self.add_dialog.open
            || self.add_dialog.pending.is_some()
            || self.add_dialog.preview_pending.is_some()
        {
            return;
        }
        let sources = match self.add_dialog.input_mode {
            AddDownloadInputModeView::Links => {
                parse_add_download_sources(self.add_input.read(cx).text())
            }
            AddDownloadInputModeView::MetadataFiles => self
                .add_dialog
                .metadata_files
                .iter()
                .map(|preview| AddDownloadSourceView::MetadataFile {
                    path: preview.path.clone(),
                    kind: preview.kind,
                    content_sha256: preview.content_sha256.clone(),
                    info_hash: preview.info_hash.clone(),
                    selected_file_indices: preview.selected_file_indices.clone(),
                })
                .collect(),
        };
        if sources.is_empty() {
            self.add_dialog.error = Some(OperationErrorView {
                code: "validation.invalid_request".into(),
                summary: match self.add_dialog.input_mode {
                    AddDownloadInputModeView::Links => self.t("notice-enter-url"),
                    AddDownloadInputModeView::MetadataFiles => self.t("notice-choose-metadata"),
                },
                retryable: false,
            });
            cx.notify();
            return;
        }
        if let Some(preview) = self
            .add_dialog
            .metadata_files
            .iter()
            .find(|preview| preview.selected_file_indices.is_empty())
        {
            self.add_dialog.error = Some(OperationErrorView {
                code: "validation.invalid_request".into(),
                summary: format!("Select at least one file from {}.", preview.path.display()),
                retryable: false,
            });
            cx.notify();
            return;
        }
        let required_bytes = if self.add_dialog.input_mode
            == AddDownloadInputModeView::MetadataFiles
        {
            match selected_metadata_known_bytes(&self.add_dialog.metadata_files) {
                Some(bytes) => Some(bytes),
                None => {
                    self.add_dialog.error = Some(OperationErrorView {
                        code: "validation.invalid_request".into(),
                        summary: "Selected metadata file sizes exceed the supported range.".into(),
                        retryable: false,
                    });
                    cx.notify();
                    return;
                }
            }
        } else {
            None
        };
        let Some(session) = self
            .snapshot
            .commands_available()
            .then(|| self.snapshot.engine_session())
            .flatten()
        else {
            self.add_dialog.error = Some(stale_session_error());
            cx.notify();
            return;
        };

        let request_id = self.allocate_request_id();
        self.add_dialog.pending = Some(PendingAddDownload {
            request_id,
            session: session.clone(),
        });
        self.add_dialog.error = None;
        let advanced = if self.add_dialog.input_mode == AddDownloadInputModeView::Links {
            self.collect_add_advanced_options(cx)
        } else {
            AddDownloadAdvancedOptionsView::default()
        };
        cx.emit(AppShellEvent::AddDownloadRequested(
            AddDownloadRequestView {
                request_id,
                session,
                sources,
                mode: if self.add_dialog.input_mode == AddDownloadInputModeView::Links {
                    self.add_dialog.mode
                } else {
                    AddDownloadModeView::SeparateTasks
                },
                destination: (!self.settings.download_directory.is_empty())
                    .then(|| self.settings.download_directory.clone()),
                required_bytes,
                file_conflict: if self.add_dialog.input_mode == AddDownloadInputModeView::Links {
                    self.add_dialog.file_conflict
                } else {
                    FileConflictPolicyView::Reject
                },
                advanced,
            },
        ));
        cx.notify();
    }

    pub(crate) fn set_add_download_mode(
        &mut self,
        mode: AddDownloadModeView,
        cx: &mut Context<Self>,
    ) {
        if self.add_dialog.pending.is_some()
            || self.add_dialog.preview_pending.is_some()
            || self.add_dialog.mode == mode
        {
            return;
        }
        self.add_dialog.mode = mode;
        self.add_dialog.error = None;
        self.add_dialog.results.clear();
        cx.notify();
    }

    pub(crate) fn set_add_input_mode(
        &mut self,
        mode: AddDownloadInputModeView,
        cx: &mut Context<Self>,
    ) {
        if self.add_dialog.pending.is_some()
            || self.add_dialog.preview_pending.is_some()
            || self.add_dialog.input_mode == mode
        {
            return;
        }
        self.add_dialog.input_mode = mode;
        self.add_dialog.error = None;
        self.add_dialog.results.clear();
        cx.notify();
    }

    pub(crate) fn choose_metadata_files(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.add_dialog.open
            || self.add_dialog.pending.is_some()
            || self.add_dialog.preview_pending.is_some()
        {
            return;
        }
        let selected = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: true,
            prompt: Some(self.t("dialog-add-choose-metadata").into()),
        });
        cx.spawn_in(window, async move |this, cx| {
            let selected = selected.await;
            let _ = this.update_in(cx, |this, window, cx| match selected {
                Ok(Ok(Some(paths))) => this.add_metadata_paths(paths, window, cx),
                Ok(Ok(None)) => {}
                Ok(Err(error)) => {
                    this.set_add_dialog_error(format!("File picker failed: {error}"), cx);
                }
                Err(error) => {
                    this.set_add_dialog_error(
                        format!("File picker closed unexpectedly: {error}"),
                        cx,
                    );
                }
            });
        })
        .detach();
    }

    pub(crate) fn add_metadata_paths(
        &mut self,
        paths: Vec<PathBuf>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.add_dialog.open {
            self.open_add_download(&OpenAddDownload, window, cx);
        }
        if !self.add_dialog.open
            || self.add_dialog.pending.is_some()
            || self.add_dialog.preview_pending.is_some()
        {
            return;
        }

        let mut known = self
            .add_dialog
            .metadata_files
            .iter()
            .map(|preview| metadata_path_key(&preview.path))
            .collect::<HashSet<_>>();
        let mut invalid = Vec::new();
        let mut accepted = Vec::new();
        for path in paths {
            if metadata_kind_from_path(&path).is_none() {
                invalid.push(path);
                continue;
            }
            if known.insert(metadata_path_key(&path)) {
                accepted.push(path);
            }
        }
        self.add_dialog.input_mode = AddDownloadInputModeView::MetadataFiles;
        self.add_dialog.mode = AddDownloadModeView::SeparateTasks;
        self.add_dialog.file_conflict = FileConflictPolicyView::Reject;
        self.add_dialog.results.clear();
        self.add_dialog.error = if invalid.is_empty() {
            None
        } else {
            Some(OperationErrorView {
                code: "validation.unsupported_metadata_file".into(),
                summary: format!(
                    "Skipped {} file{}; supported extensions are .torrent, .metalink, and .meta4.",
                    invalid.len(),
                    if invalid.len() == 1 { "" } else { "s" }
                ),
                retryable: false,
            })
        };
        if !accepted.is_empty() {
            let request_id = self.allocate_request_id();
            self.add_dialog.preview_pending = Some(PendingMetadataPreview {
                request_id,
                paths: accepted.clone(),
            });
            cx.emit(AppShellEvent::AddDownloadMetadataPreviewRequested(
                AddDownloadMetadataPreviewRequestView {
                    request_id,
                    paths: accepted,
                },
            ));
        }
        cx.notify();
    }

    pub(crate) fn remove_metadata_file(&mut self, index: usize, cx: &mut Context<Self>) {
        if self.add_dialog.pending.is_some()
            || self.add_dialog.preview_pending.is_some()
            || index >= self.add_dialog.metadata_files.len()
        {
            return;
        }
        self.add_dialog.metadata_files.remove(index);
        self.add_dialog.active_metadata_file = if self.add_dialog.metadata_files.is_empty() {
            None
        } else {
            Some(
                self.add_dialog
                    .active_metadata_file
                    .unwrap_or_default()
                    .min(self.add_dialog.metadata_files.len() - 1),
            )
        };
        self.add_dialog.error = None;
        self.add_dialog.results.clear();
        cx.notify();
    }

    pub(crate) fn select_metadata_file(&mut self, index: usize, cx: &mut Context<Self>) {
        if self.add_dialog.pending.is_none()
            && self.add_dialog.preview_pending.is_none()
            && index < self.add_dialog.metadata_files.len()
            && self.add_dialog.active_metadata_file != Some(index)
        {
            self.add_dialog.active_metadata_file = Some(index);
            cx.notify();
        }
    }

    pub(crate) fn toggle_metadata_file_entry(
        &mut self,
        preview_index: usize,
        file_index: u32,
        cx: &mut Context<Self>,
    ) {
        if self.add_dialog.pending.is_some() || self.add_dialog.preview_pending.is_some() {
            return;
        }
        let Some(preview) = self.add_dialog.metadata_files.get_mut(preview_index) else {
            return;
        };
        match preview.selected_file_indices.binary_search(&file_index) {
            Ok(position) => {
                preview.selected_file_indices.remove(position);
            }
            Err(position) if preview.files.iter().any(|file| file.index == file_index) => {
                preview.selected_file_indices.insert(position, file_index);
            }
            Err(_) => return,
        }
        self.add_dialog.error = None;
        self.add_dialog.results.clear();
        cx.notify();
    }

    pub(crate) fn toggle_all_metadata_file_entries(
        &mut self,
        preview_index: usize,
        cx: &mut Context<Self>,
    ) {
        if self.add_dialog.pending.is_some() || self.add_dialog.preview_pending.is_some() {
            return;
        }
        let Some(preview) = self.add_dialog.metadata_files.get_mut(preview_index) else {
            return;
        };
        if preview.selected_file_indices.len() == preview.files.len() {
            preview.selected_file_indices.clear();
        } else {
            preview.selected_file_indices = preview.files.iter().map(|file| file.index).collect();
        }
        self.add_dialog.error = None;
        self.add_dialog.results.clear();
        cx.notify();
    }

    pub(crate) fn set_add_dialog_error(&mut self, summary: String, cx: &mut Context<Self>) {
        if self.add_dialog.open {
            self.add_dialog.error = Some(OperationErrorView {
                code: "filesystem.operation_failed".into(),
                summary,
                retryable: true,
            });
            cx.notify();
        }
    }

    pub(crate) fn toggle_add_advanced(&mut self, cx: &mut Context<Self>) {
        if self.add_dialog.pending.is_some() || self.add_dialog.preview_pending.is_some() {
            return;
        }
        self.add_dialog.advanced_open = !self.add_dialog.advanced_open;
        cx.notify();
    }

    pub(crate) fn collect_add_advanced_options(&self, cx: &App) -> AddDownloadAdvancedOptionsView {
        let cookie = self.add_inputs.cookie.read(cx).text().trim().to_owned();
        let http_passwd = self.add_inputs.http_passwd.read(cx).text();
        AddDownloadAdvancedOptionsView {
            referer: self.add_inputs.referer.read(cx).text().trim().to_owned(),
            user_agent: self.add_inputs.user_agent.read(cx).text().trim().to_owned(),
            headers: self.add_inputs.headers.read(cx).text().to_owned(),
            cookie: (!cookie.is_empty()).then(|| SecretStringView::new(cookie)),
            http_user: self.add_inputs.http_user.read(cx).text().trim().to_owned(),
            http_passwd: (!http_passwd.is_empty()).then(|| SecretStringView::new(http_passwd)),
            checksum: self.add_inputs.checksum.read(cx).text().trim().to_owned(),
        }
    }

    pub(crate) fn set_file_conflict_policy(
        &mut self,
        policy: FileConflictPolicyView,
        cx: &mut Context<Self>,
    ) {
        if self.add_dialog.pending.is_some() || self.add_dialog.file_conflict == policy {
            return;
        }
        self.add_dialog.file_conflict = policy;
        self.add_dialog.error = None;
        self.add_dialog.results.clear();
        cx.notify();
    }

    pub(crate) fn render_add_download_dialog(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let colors = self.theme.colors;
        let add_pending = self.add_dialog.pending.is_some();
        let preview_pending = self.add_dialog.preview_pending.is_some();
        let pending = add_pending || preview_pending;
        let error = self.add_dialog.error.clone();
        let sources = parse_add_download_sources(self.add_input.read(cx).text());
        let input_mode = self.add_dialog.input_mode;
        let mode = self.add_dialog.mode;
        let file_conflict = self.add_dialog.file_conflict;
        let shell = cx.entity().downgrade();
        let input_shell = shell.clone();
        let conflict_shell = shell.clone();
        let input_mode_control = SegmentedControl::new(
            "add-download-input-mode",
            [
                Segment::new(self.t(AddDownloadInputModeView::Links.message_key())),
                Segment::new(self.t(AddDownloadInputModeView::MetadataFiles.message_key())),
            ],
            usize::from(input_mode == AddDownloadInputModeView::MetadataFiles),
            self.theme,
        )
        .disabled(pending)
        .on_select(move |index, _window, cx| {
            let mode = if index == 0 {
                AddDownloadInputModeView::Links
            } else {
                AddDownloadInputModeView::MetadataFiles
            };
            input_shell
                .update(cx, |shell, cx| shell.set_add_input_mode(mode, cx))
                .ok();
        });
        let mode_control = SegmentedControl::new(
            "add-download-mode",
            [
                Segment::new(self.t(AddDownloadModeView::SeparateTasks.message_key())),
                Segment::new(self.t(AddDownloadModeView::Mirrors.message_key())),
            ],
            usize::from(mode == AddDownloadModeView::Mirrors),
            self.theme,
        )
        .disabled(pending)
        .on_select(move |index, _window, cx| {
            let mode = if index == 0 {
                AddDownloadModeView::SeparateTasks
            } else {
                AddDownloadModeView::Mirrors
            };
            shell
                .update(cx, |shell, cx| shell.set_add_download_mode(mode, cx))
                .ok();
        });
        let conflict_control = SegmentedControl::new(
            "add-download-file-conflict",
            [
                Segment::new(self.t(FileConflictPolicyView::AutoRename.message_key())),
                Segment::new(self.t(FileConflictPolicyView::Reject.message_key())),
                Segment::new(self.t(FileConflictPolicyView::Overwrite.message_key())),
            ],
            match file_conflict {
                FileConflictPolicyView::AutoRename => 0,
                FileConflictPolicyView::Reject => 1,
                FileConflictPolicyView::Overwrite => 2,
            },
            self.theme,
        )
        .disabled(pending)
        .on_select(move |index, _window, cx| {
            let policy = match index {
                0 => FileConflictPolicyView::AutoRename,
                1 => FileConflictPolicyView::Reject,
                _ => FileConflictPolicyView::Overwrite,
            };
            conflict_shell
                .update(cx, |shell, cx| {
                    shell.set_file_conflict_policy(policy, cx);
                })
                .ok();
        });
        let content = div()
            .flex()
            .flex_col()
            .gap_2()
            .child(input_mode_control)
            .child(match input_mode {
                AddDownloadInputModeView::Links => div()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child(
                        div()
                            .text_xs()
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(colors.text_secondary)
                            .child(self.t("dialog-add-url-or-magnet")),
                    )
                    .child(self.add_input.clone())
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .gap_3()
                            .child(div().text_xs().text_color(colors.text_muted).child(
                                if sources.is_empty() {
                                    self.t("dialog-add-no-sources")
                                } else {
                                    self.t_count(
                                        "dialog-add-sources-detected",
                                        u64::try_from(sources.len()).unwrap_or(u64::MAX),
                                    )
                                },
                            ))
                            .child(mode_control),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .gap_3()
                            .child(
                                div()
                                    .text_xs()
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(colors.text_secondary)
                                    .child(self.t("dialog-add-if-file-exists")),
                            )
                            .child(conflict_control),
                    )
                    .when(
                        file_conflict == FileConflictPolicyView::Overwrite,
                        |element| {
                            element.child(
                                div()
                                    .id("add-download-overwrite-warning")
                                    .role(Role::Alert)
                                    .text_xs()
                                    .text_color(colors.danger)
                                    .child(self.t("ui-overwrite-warning")),
                            )
                        },
                    )
                    .child(self.render_add_advanced_section(pending, colors, cx))
                    .into_any_element(),
                AddDownloadInputModeView::MetadataFiles => {
                    self.render_metadata_file_picker(pending, preview_pending, cx)
                }
            })
            .when(!self.add_dialog.results.is_empty(), |element| {
                element.child(self.render_add_result_list(colors))
            })
            .when_some(error, |element, error| {
                let message = self.te(&error);
                element.child(
                    div()
                        .id("add-download-error")
                        .role(Role::Alert)
                        .aria_label(message.clone())
                        .text_xs()
                        .text_color(colors.danger)
                        .child(message),
                )
            });

        Dialog::new(
            "add-download-dialog",
            self.t("dialog-add-download"),
            self.theme,
        )
        .key_context("AddDownloadDialog")
        .track_focus(self.add_dialog_focus.clone())
        .width(if input_mode == AddDownloadInputModeView::MetadataFiles {
            720.0
        } else if self.add_dialog.advanced_open {
            640.0
        } else {
            560.0
        })
        .child(content)
        .action(
            Button::new("cancel-add-download", self.t("button-cancel"))
                .aria_label(self.t("dialog-add-cancel-aria"))
                .style(ButtonStyle::Secondary)
                .disabled(pending)
                .track_focus(self.add_cancel_focus.clone())
                .on_click(cx.listener(|this, _, window, cx| {
                    this.close_add_download(window, cx);
                }))
                .render(colors),
        )
        .action(
            Button::new("submit-add-download", self.t("dialog-add-submit"))
                .aria_label(if add_pending {
                    self.t("dialog-add-submitting")
                } else {
                    self.t("dialog-add-submit")
                })
                .style(ButtonStyle::Primary)
                .disabled(preview_pending)
                .loading(add_pending)
                .track_focus(self.add_submit_focus.clone())
                .on_click(cx.listener(|this, _, _window, cx| {
                    this.submit_add_download(cx);
                }))
                .render(colors),
        )
        .into_any_element()
    }

    pub(crate) fn render_add_advanced_section(
        &mut self,
        pending: bool,
        colors: crate::ThemeColors,
        cx: &mut Context<Self>,
    ) -> Div {
        let open = self.add_dialog.advanced_open;
        div()
            .flex()
            .flex_col()
            .gap_2()
            .child(
                div()
                    .id("add-download-advanced-toggle")
                    .role(Role::Button)
                    .aria_label(if open {
                        self.t("dialog-add-hide-advanced-aria")
                    } else {
                        self.t("dialog-add-show-advanced-aria")
                    })
                    .aria_expanded(open)
                    .focusable()
                    .tab_stop(true)
                    .cursor_pointer()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap_2()
                    .rounded_md()
                    .border_1()
                    .border_color(colors.border)
                    .bg(colors.elevated_surface)
                    .px_3()
                    .py_2()
                    .hover(|style| style.bg(colors.surface_hover))
                    .on_click(cx.listener(|this, _, _, cx| this.toggle_add_advanced(cx)))
                    .child(
                        div()
                            .text_xs()
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(colors.text_secondary)
                            .child(self.t("ui-advanced-options")),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(colors.text_muted)
                            .child(if open {
                                self.t("dialog-add-hide-advanced")
                            } else {
                                self.t("dialog-add-show-advanced")
                            }),
                    ),
            )
            .when(open, |element| {
                element
                    .child(
                        div()
                            .text_xs()
                            .text_color(colors.text_muted)
                            .child(self.t("dialog-add-advanced-hint")),
                    )
                    .child(
                        div()
                            .flex()
                            .gap_3()
                            .child(
                                settings_labeled_input(
                                    self.t("dialog-add-referer"),
                                    self.add_inputs.referer.clone(),
                                    colors,
                                )
                                .flex_1()
                                .min_w_0(),
                            )
                            .child(
                                settings_labeled_input(
                                    self.t("dialog-add-user-agent"),
                                    self.add_inputs.user_agent.clone(),
                                    colors,
                                )
                                .flex_1()
                                .min_w_0(),
                            ),
                    )
                    .child(settings_labeled_input(
                        self.t("dialog-add-custom-headers"),
                        self.add_inputs.headers.clone(),
                        colors,
                    ))
                    .child(settings_labeled_input(
                        self.t("dialog-add-cookie"),
                        self.add_inputs.cookie.clone(),
                        colors,
                    ))
                    .child(
                        div()
                            .flex()
                            .gap_3()
                            .child(
                                settings_labeled_input(
                                    self.t("dialog-add-http-username"),
                                    self.add_inputs.http_user.clone(),
                                    colors,
                                )
                                .flex_1()
                                .min_w_0(),
                            )
                            .child(
                                settings_labeled_input(
                                    self.t("dialog-add-http-password"),
                                    self.add_inputs.http_passwd.clone(),
                                    colors,
                                )
                                .flex_1()
                                .min_w_0(),
                            ),
                    )
                    .child(settings_labeled_input(
                        self.t("dialog-add-checksum"),
                        self.add_inputs.checksum.clone(),
                        colors,
                    ))
                    .when(pending, |element| {
                        // Keep the section visible while submitting, but inputs stay
                        // disabled through the dialog pending state of TextField focus.
                        element
                    })
            })
    }

    pub(crate) fn render_metadata_file_picker(
        &mut self,
        pending: bool,
        preview_pending: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let colors = self.theme.colors;
        let rows =
            self.add_dialog
                .metadata_files
                .iter()
                .enumerate()
                .map(|(index, preview)| {
                    let active = self.add_dialog.active_metadata_file == Some(index);
                    let name = preview.path.file_name().map_or_else(
                        || preview.path.display().to_string(),
                        |name| name.to_string_lossy().into(),
                    );
                    let full_path = preview.path.display().to_string();
                    let kind = preview.kind;
                    let kind_label = self.t(kind.message_key());
                    let selected = preview.selected_file_indices.len();
                    let total = preview.files.len();
                    let row_aria = self.t_args(
                        "dialog-add-metadata-row-aria",
                        &[
                            ("kind", FluentValue::from(kind_label.clone())),
                            ("name", FluentValue::from(name.clone())),
                            ("selected", FluentValue::from(fluent_number(selected))),
                            ("total", FluentValue::from(fluent_number(total))),
                        ],
                    );
                    let row_summary = self.t_args(
                        "dialog-add-metadata-row-summary",
                        &[
                            ("kind", FluentValue::from(kind_label.clone())),
                            ("selected", FluentValue::from(fluent_number(selected))),
                            ("total", FluentValue::from(fluent_number(total))),
                            ("path", FluentValue::from(full_path)),
                        ],
                    );
                    let remove_aria = self.t_args(
                        "dialog-add-remove-kind-aria",
                        &[("kind", FluentValue::from(kind_label))],
                    );
                    div()
                        .id(SharedString::from(format!("metadata-file-{index}")))
                        .role(Role::ListItem)
                        .aria_label(row_aria)
                        .h(px(48.0))
                        .flex_none()
                        .flex()
                        .items_center()
                        .gap_2()
                        .px_2()
                        .border_b_1()
                        .border_color(if active { colors.accent } else { colors.border })
                        .bg(if active {
                            colors.surface_active
                        } else {
                            colors.surface
                        })
                        .cursor_pointer()
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.select_metadata_file(index, cx);
                        }))
                        .child(Icon::new(IconName::Download).size(IconSize::Small).color(
                            if active {
                                colors.accent
                            } else {
                                colors.text_muted
                            },
                        ))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .flex()
                                .flex_col()
                                .child(div().truncate().text_sm().child(name))
                                .child(
                                    div()
                                        .truncate()
                                        .text_xs()
                                        .text_color(colors.text_muted)
                                        .child(row_summary),
                                ),
                        )
                        .child(
                            IconButton::new(
                                SharedString::from(format!("remove-metadata-file-{index}")),
                                IconName::X,
                            )
                            .aria_label(remove_aria)
                            .disabled(pending)
                            .tooltip(Tooltip::new(self.t("dialog-add-remove-file")))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.remove_metadata_file(index, cx);
                            }))
                            .render(colors),
                        )
                        .into_any_element()
                })
                .collect::<Vec<_>>();
        let count = self.add_dialog.metadata_files.len();
        let active_index = self.add_dialog.active_metadata_file;
        let active_summary = active_index
            .and_then(|index| self.add_dialog.metadata_files.get(index))
            .map(|preview| self.localized_metadata_selection_summary(preview));
        let active_file_count = active_index
            .and_then(|index| self.add_dialog.metadata_files.get(index))
            .map_or(0, |preview| preview.files.len());
        let active_selection_state = active_index
            .and_then(|index| self.add_dialog.metadata_files.get(index))
            .map_or(Toggled::False, |preview| {
                if preview.selected_file_indices.is_empty() {
                    Toggled::False
                } else if preview.selected_file_indices.len() == preview.files.len() {
                    Toggled::True
                } else {
                    Toggled::Mixed
                }
            });
        let active_selection_icon = match active_selection_state {
            Toggled::False => IconName::Square,
            Toggled::True => IconName::SquareCheckBig,
            Toggled::Mixed => IconName::SquareMinus,
        };
        let file_list = active_index.map(|preview_index| {
            let list_id = SharedString::from(format!("metadata-preview-files-{preview_index}"));
            div()
                .h(px(220.0))
                .min_h_0()
                .child(
                    uniform_list(
                        list_id.clone(),
                        active_file_count,
                        cx.processor(move |this, range: Range<usize>, _window, cx| {
                            let colors = this.theme.colors;
                            let Some(preview) = this.add_dialog.metadata_files.get(preview_index)
                            else {
                                return Vec::new();
                            };
                            range
                                .filter_map(|position| {
                                    let file = preview.files.get(position)?.clone();
                                    let selected = preview
                                        .selected_file_indices
                                        .binary_search(&file.index)
                                        .is_ok();
                                    let file_index = file.index;
                                    Some(
                                        div()
                                            .id(SharedString::from(format!(
                                                "metadata-preview-file:{preview_index}:{file_index}"
                                            )))
                                            .role(Role::CheckBox)
                                            .aria_position_in_set(position + 1)
                                            .aria_size_of_set(active_file_count)
                                            .aria_toggled(if selected {
                                                Toggled::True
                                            } else {
                                                Toggled::False
                                            })
                                            .aria_label({
                                                let size = file.length.map_or_else(
                                                    || this.t("dialog-add-size-unknown"),
                                                    format_bytes,
                                                );
                                                this.t_args(
                                                    "dialog-add-file-row-aria",
                                                    &[
                                                        (
                                                            "index",
                                                            FluentValue::from(i64::from(
                                                                file_index,
                                                            )),
                                                        ),
                                                        (
                                                            "path",
                                                            FluentValue::from(file.path.clone()),
                                                        ),
                                                        ("size", FluentValue::from(size)),
                                                    ],
                                                )
                                            })
                                            .h(px(40.0))
                                            .w_full()
                                            .flex_none()
                                            .flex()
                                            .items_center()
                                            .gap_2()
                                            .px_3()
                                            .border_b_1()
                                            .border_color(colors.border)
                                            .cursor_pointer()
                                            .on_click(cx.listener(move |this, _, _, cx| {
                                                this.toggle_metadata_file_entry(
                                                    preview_index,
                                                    file_index,
                                                    cx,
                                                );
                                            }))
                                            .child(
                                                Icon::new(if selected {
                                                    IconName::SquareCheckBig
                                                } else {
                                                    IconName::Square
                                                })
                                                .size(IconSize::Small)
                                                .color(if selected {
                                                    colors.accent
                                                } else {
                                                    colors.text_muted
                                                }),
                                            )
                                            .child(
                                                div()
                                                    .w(px(34.0))
                                                    .flex_none()
                                                    .font_features(tabular_numbers())
                                                    .text_xs()
                                                    .text_color(colors.text_muted)
                                                    .child(file_index.to_string()),
                                            )
                                            .child(
                                                div()
                                                    .flex_1()
                                                    .min_w_0()
                                                    .truncate()
                                                    .text_xs()
                                                    .text_color(colors.text_secondary)
                                                    .child(file.path),
                                            )
                                            .child(
                                                div()
                                                    .w(px(84.0))
                                                    .flex_none()
                                                    .text_right()
                                                    .font_features(tabular_numbers())
                                                    .text_xs()
                                                    .text_color(colors.text_muted)
                                                    .child(file.length.map_or_else(
                                                        || this.t("dialog-add-size-unknown"),
                                                        format_bytes,
                                                    )),
                                            )
                                            .into_any_element(),
                                    )
                                })
                                .collect::<Vec<_>>()
                        }),
                    )
                    .track_scroll(&self.metadata_file_scroll)
                    .size_full(),
                )
                .border_1()
                .border_color(colors.border)
                .rounded_md()
                .into_any_element()
        });
        div()
            .flex()
            .flex_col()
            .gap_2()
            .child(
                div()
                    .id("metadata-file-drop-target")
                    .role(Role::Group)
                    .aria_label(self.t("dialog-add-drop-target-aria"))
                    .min_h(px(82.0))
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap_3()
                    .p_3()
                    .border_1()
                    .border_color(colors.border)
                    .rounded_md()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(
                                Icon::new(IconName::Inbox)
                                    .size(IconSize::Medium)
                                    .color(colors.text_muted),
                            )
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .child(self.t("ui-torrent-metalink-files"))
                                    .child(div().text_xs().text_color(colors.text_muted).child(
                                        if preview_pending {
                                            self.t("dialog-add-reading-metadata")
                                        } else {
                                            self.t_count(
                                                "dialog-add-sources-ready",
                                                u64::try_from(count).unwrap_or(u64::MAX),
                                            )
                                        },
                                    )),
                            ),
                    )
                    .child(
                        Button::new(
                            "choose-metadata-files",
                            self.t("dialog-add-choose-metadata"),
                        )
                        .icon(IconName::FolderDown)
                        .aria_label(self.t("dialog-add-choose-files-aria"))
                        .style(ButtonStyle::Secondary)
                        .disabled(pending)
                        .loading(preview_pending)
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.choose_metadata_files(window, cx);
                        }))
                        .render(colors),
                    ),
            )
            .when(!rows.is_empty(), |element| {
                element.child(
                    div()
                        .id("metadata-file-list")
                        .role(Role::List)
                        .aria_label(self.t("dialog-add-selected-files-aria"))
                        .max_h(px(112.0))
                        .border_1()
                        .border_color(colors.border)
                        .rounded_md()
                        .children(rows),
                )
            })
            .when_some(active_summary, |element, summary| {
                element
                    .child(
                        div()
                            .h(px(36.0))
                            .flex_none()
                            .flex()
                            .items_center()
                            .justify_between()
                            .gap_3()
                            .child(
                                div()
                                    .id("toggle-all-metadata-files")
                                    .role(Role::CheckBox)
                                    .aria_toggled(active_selection_state)
                                    .aria_label(match active_selection_state {
                                        Toggled::True => self.t("dialog-add-clear-file-selection"),
                                        Toggled::False | Toggled::Mixed => self.t("select-all"),
                                    })
                                    .flex()
                                    .items_center()
                                    .gap_2()
                                    .cursor_pointer()
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        if let Some(index) = active_index {
                                            this.toggle_all_metadata_file_entries(index, cx);
                                        }
                                    }))
                                    .child(
                                        Icon::new(active_selection_icon)
                                            .size(IconSize::Small)
                                            .color(colors.accent),
                                    )
                                    .child(
                                        div()
                                            .text_xs()
                                            .font_weight(FontWeight::MEDIUM)
                                            .text_color(colors.text_secondary)
                                            .child(self.t("ui-files")),
                                    ),
                            )
                            .child(
                                div()
                                    .font_features(tabular_numbers())
                                    .text_xs()
                                    .text_color(colors.text_muted)
                                    .child(summary),
                            ),
                    )
                    .when_some(file_list, |element, list| element.child(list))
            })
            .into_any_element()
    }

    fn localized_metadata_selection_summary(
        &self,
        preview: &AddDownloadMetadataPreviewView,
    ) -> String {
        let mut known_bytes = 0_u64;
        let mut unknown_sizes = 0_usize;
        for file in &preview.files {
            if preview
                .selected_file_indices
                .binary_search(&file.index)
                .is_ok()
            {
                if let Some(length) = file.length {
                    known_bytes = known_bytes.saturating_add(length);
                } else {
                    unknown_sizes = unknown_sizes.saturating_add(1);
                }
            }
        }

        let selected = fluent_number(preview.selected_file_indices.len());
        let total = fluent_number(preview.files.len());
        let size = format_bytes(known_bytes);
        if unknown_sizes == 0 {
            self.t_args(
                "dialog-add-selection-summary",
                &[
                    ("selected", FluentValue::from(selected)),
                    ("total", FluentValue::from(total)),
                    ("size", FluentValue::from(size)),
                ],
            )
        } else {
            self.t_args(
                "dialog-add-selection-summary-with-unknown",
                &[
                    ("selected", FluentValue::from(selected)),
                    ("total", FluentValue::from(total)),
                    ("size", FluentValue::from(size)),
                    ("unknown", FluentValue::from(fluent_number(unknown_sizes))),
                ],
            )
        }
    }

    fn localized_add_source_label(&self, source: &AddDownloadSourceView) -> String {
        match source {
            AddDownloadSourceView::Uri { line, uri } => self.t_args(
                "dialog-add-source-uri",
                &[
                    ("line", FluentValue::from(fluent_number(*line))),
                    (
                        "source",
                        FluentValue::from(ariadeck_domain::redact_source_uri(uri)),
                    ),
                ],
            ),
            AddDownloadSourceView::MetadataFile { path, kind, .. } => {
                let name = path.file_name().map_or_else(
                    || path.display().to_string(),
                    |name| name.to_string_lossy().into(),
                );
                self.t_args(
                    "dialog-add-source-metadata",
                    &[
                        ("kind", FluentValue::from(self.t(kind.message_key()))),
                        ("name", FluentValue::from(name)),
                    ],
                )
            }
        }
    }

    pub(crate) fn render_add_result_list(&self, colors: crate::ThemeColors) -> Stateful<Div> {
        let rows = self
            .add_dialog
            .results
            .iter()
            .enumerate()
            .map(|(index, item)| {
                let source_label = item
                    .sources
                    .iter()
                    .map(|source| self.localized_add_source_label(source))
                    .collect::<Vec<_>>()
                    .join("  |  ");
                let (icon, label, color) = match &item.outcome {
                    CommandOutcomeView::Success { tasks } => (
                        IconName::CircleCheck,
                        match tasks.as_slice() {
                            [] => self.t("dialog-add-result-accepted"),
                            [task] => self.t_args(
                                "dialog-add-result-accepted-gid",
                                &[("gid", FluentValue::from(task.gid.clone()))],
                            ),
                            tasks => self.t_count(
                                "dialog-add-result-accepted-tasks",
                                u64::try_from(tasks.len()).unwrap_or(u64::MAX),
                            ),
                        },
                        colors.success,
                    ),
                    CommandOutcomeView::Failure(error) if error.outcome_unknown() => {
                        (IconName::TriangleAlert, self.te(error), colors.warning)
                    }
                    CommandOutcomeView::Failure(error) => {
                        (IconName::CircleAlert, self.te(error), colors.danger)
                    }
                };
                div()
                    .id(SharedString::from(format!("add-result-{index}")))
                    .role(Role::ListItem)
                    .flex()
                    .items_start()
                    .gap_2()
                    .child(Icon::new(icon).size(IconSize::XSmall).color(color))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .child(div().truncate().text_xs().child(source_label))
                            .child(div().text_xs().text_color(color).child(label)),
                    )
            })
            .collect::<Vec<_>>();
        div()
            .id("add-download-results")
            .role(Role::List)
            .aria_label(self.t("dialog-add-results-aria"))
            .max_h(px(220.0))
            .flex()
            .flex_col()
            .gap_2()
            .p_2()
            .border_1()
            .border_color(colors.border)
            .rounded_md()
            .children(rows)
    }
}
