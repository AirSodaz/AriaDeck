//! Task details panel child entity and drawer/sheet coordination for `AppShell`.

use super::*;

pub(super) enum TaskDetailsPanelEvent {
    Close,
    RetryDetails,
    OpenTask(TaskOpenTargetView),
    EditTask,
    ComputeChecksum {
        directory: String,
        file_path: String,
        algorithm: ariadeck_domain::ChecksumAlgorithm,
    },
    CancelChecksum(RequestId),
    Notice {
        message: String,
        is_error: bool,
    },
}

pub(super) struct TaskDetailsPanelView {
    pub(super) identity: TaskIdentity,
    pub(super) overview: DownloadRowView,
    pub(super) session: EngineSessionView,
    pub(super) state: TaskDetailsLoadState,
    pub(super) pending: Option<PendingTaskDetails>,
    pub(super) open_pending: Option<PendingTaskOpen>,
    pub(super) tab: TaskDetailsTab,
    pub(super) file_scroll: UniformListScrollHandle,
    pub(super) info_scroll: ScrollHandle,
    pub(super) network_scroll: ScrollHandle,
    pub(super) options_scroll: ScrollHandle,
    pub(super) verify_scroll: ScrollHandle,
    pub(super) rendered_file_range: Range<usize>,
    /// Last time a ready→ready details RPC was emitted (coalesce rapid revisions).
    pub(super) last_ready_refresh_at: Option<Instant>,
    /// True when a revision-driven refresh was deferred by the min interval.
    pub(super) refresh_coalesced: bool,
    pub(super) checksum_algorithm: ariadeck_domain::ChecksumAlgorithm,
    pub(super) checksum_file_index: usize,
    pub(super) checksum_expected_input: Option<Entity<InputState>>,
    pub(super) checksum_expected_pending_value: Option<String>,
    pub(super) checksum_prefilled_from_task: bool,
    pub(super) checksum_pending: Option<(RequestId, u64, u64)>,
    pub(super) checksum_result: Option<(
        ariadeck_domain::ChecksumAlgorithm,
        Result<String, OperationErrorView>,
    )>,
    _checksum_sub: Option<Subscription>,
    theme: Theme,
    translator: Translator,
    commands_available: bool,
    change_option_available: bool,
    local_path_actions_available: bool,
    drawer_width: f32,
}

impl gpui::EventEmitter<TaskDetailsPanelEvent> for TaskDetailsPanelView {}

pub(super) struct TaskDetailsPanelContext {
    pub(super) theme: Theme,
    pub(super) translator: Translator,
    pub(super) commands_available: bool,
    pub(super) change_option_available: bool,
    pub(super) local_path_actions_available: bool,
    pub(super) drawer_width: f32,
}

impl TaskDetailsPanelView {
    pub(crate) fn new(
        task: DownloadRowView,
        session: EngineSessionView,
        panel_cx: TaskDetailsPanelContext,
    ) -> Self {
        Self {
            identity: task.identity.clone(),
            overview: task,
            session,
            state: TaskDetailsLoadState::Stale,
            pending: None,
            open_pending: None,
            tab: TaskDetailsTab::Info,
            file_scroll: UniformListScrollHandle::new(),
            info_scroll: ScrollHandle::new(),
            network_scroll: ScrollHandle::new(),
            options_scroll: ScrollHandle::new(),
            verify_scroll: ScrollHandle::new(),
            rendered_file_range: 0..0,
            last_ready_refresh_at: None,
            refresh_coalesced: false,
            checksum_algorithm: ariadeck_domain::ChecksumAlgorithm::Sha256,
            checksum_file_index: 0,
            checksum_expected_input: None,
            checksum_expected_pending_value: None,
            checksum_prefilled_from_task: false,
            checksum_pending: None,
            checksum_result: None,
            _checksum_sub: None,
            theme: panel_cx.theme,
            translator: panel_cx.translator,
            commands_available: panel_cx.commands_available,
            change_option_available: panel_cx.change_option_available,
            local_path_actions_available: panel_cx.local_path_actions_available,
            drawer_width: panel_cx.drawer_width,
        }
    }

    pub(crate) fn sync_context(
        &mut self,
        panel_cx: TaskDetailsPanelContext,
        cx: &mut Context<Self>,
    ) {
        self.theme = panel_cx.theme;
        self.translator = panel_cx.translator;
        self.commands_available = panel_cx.commands_available;
        self.change_option_available = panel_cx.change_option_available;
        self.local_path_actions_available = panel_cx.local_path_actions_available;
        self.drawer_width = panel_cx.drawer_width;
        cx.notify();
    }

    pub(crate) fn switch_task(
        &mut self,
        task: DownloadRowView,
        session: EngineSessionView,
        panel_cx: TaskDetailsPanelContext,
        cx: &mut Context<Self>,
    ) {
        if self.identity == task.identity {
            self.overview = task;
            self.session = session;
            self.sync_context(panel_cx, cx);
            return;
        }
        self.identity = task.identity.clone();
        self.overview = task;
        self.session = session;
        self.state = TaskDetailsLoadState::Stale;
        self.pending = None;
        self.open_pending = None;
        self.rendered_file_range = 0..0;
        self.last_ready_refresh_at = None;
        self.refresh_coalesced = false;
        self.file_scroll = UniformListScrollHandle::new();
        self.info_scroll = ScrollHandle::new();
        self.network_scroll = ScrollHandle::new();
        self.options_scroll = ScrollHandle::new();
        self.verify_scroll = ScrollHandle::new();
        self.checksum_file_index = 0;
        self.checksum_pending = None;
        self.checksum_result = None;
        self.checksum_prefilled_from_task = false;
        self.checksum_expected_pending_value = Some(String::new());
        self.sync_context(panel_cx, cx);
    }

    pub(crate) fn prefill_checksum_from_options(&mut self, options: &[TaskOptionView], cx: &App) {
        if self.checksum_prefilled_from_task {
            return;
        }
        let Some(raw) = options
            .iter()
            .find(|opt| opt.key.eq_ignore_ascii_case("checksum") && !opt.redacted)
            .map(|opt| opt.value.trim())
            .filter(|v| !v.is_empty())
        else {
            return;
        };
        let input_empty = self.checksum_expected_pending_value.is_some()
            || self
                .checksum_expected_input
                .as_ref()
                .is_none_or(|input| input.read(cx).value().trim().is_empty());
        if !input_empty {
            self.checksum_prefilled_from_task = true;
            return;
        }
        if let Ok(parsed) = ariadeck_domain::parse_expected_checksum(raw)
            && let Some(algo) = parsed.algorithm
        {
            self.checksum_algorithm = algo;
        }
        self.checksum_expected_pending_value = Some(raw.to_owned());
        self.checksum_prefilled_from_task = true;
    }

    fn ensure_checksum_input(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.checksum_expected_input.is_none() {
            let placeholder = self.t("dialog-details-verify-expected-placeholder");
            let input = cx.new(|cx| InputState::new(window, cx).placeholder(placeholder));
            let sub = cx.subscribe(&input, |_this, _input, event: &InputEvent, cx| {
                if matches!(event, InputEvent::Change) {
                    cx.notify();
                }
            });
            self.checksum_expected_input = Some(input);
            self._checksum_sub = Some(sub);
        }
        if let (Some(pending_val), Some(input)) = (
            self.checksum_expected_pending_value.take(),
            self.checksum_expected_input.as_ref(),
        ) {
            input.update(cx, |input, cx| {
                input.set_value(pending_val, window, cx);
            });
        }
    }

    fn t(&self, id: &str) -> String {
        self.translator.t(id)
    }

    fn t_args(&self, id: &str, args: &[(&str, FluentValue<'_>)]) -> String {
        let mut fluent_args = FluentArgs::new();
        for (key, value) in args {
            fluent_args.set(*key, value.clone());
        }
        self.translator.t_args(id, Some(&fluent_args))
    }

    fn t_count(&self, id: &str, n: u64) -> String {
        self.translator.t_count(id, n)
    }

    fn te(&self, error: &OperationErrorView) -> String {
        error.localized_summary(&self.translator)
    }

    fn format_seed_stop_rules(&self, options: &[TaskOptionView]) -> String {
        let value = |key: &str| {
            options
                .iter()
                .find(|option| option.key.eq_ignore_ascii_case(key))
                .map(|option| option.value.as_str())
        };
        let ratio_value = value("seed-ratio");
        let ratio = if ratio_value
            .and_then(|value| value.parse::<f64>().ok())
            .is_some_and(|value| value == 0.0)
        {
            self.t("dialog-details-seed-ratio-disabled")
        } else {
            self.t_args(
                "dialog-details-seed-ratio-value",
                &[(
                    "ratio",
                    FluentValue::from(
                        ratio_value
                            .map(str::to_owned)
                            .unwrap_or_else(|| self.t("dialog-details-not-reported")),
                    ),
                )],
            )
        };
        let time = value("seed-time")
            .map(str::to_owned)
            .unwrap_or_else(|| self.t("dialog-details-not-reported"));
        self.t_args(
            "dialog-details-seed-stop-rules",
            &[
                ("ratio", FluentValue::from(ratio)),
                ("time", FluentValue::from(time)),
            ],
        )
    }

    fn render_info_tab(
        &self,
        details: &TaskDetailsView,
        path_actions_enabled: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let colors = self.theme.colors;
        let gid = self.identity.gid.clone();
        let edit_task_enabled = self.commands_available
            && self.overview.can_edit_or_relink()
            && (self.overview.status == TaskStatusView::Failed || self.change_option_available);
        let is_bittorrent = matches!(
            self.overview.source_kind,
            crate::TaskSourceKindView::Magnet | crate::TaskSourceKindView::BitTorrent
        ) || self.overview.status == TaskStatusView::Seeding;
        let seed_stop_rules = self.format_seed_stop_rules(&details.options);
        let path_validation_label = match &details.path_validation {
            TaskPathValidationView::Unavailable => self.t("dialog-details-path-unavailable"),
            TaskPathValidationView::Valid {
                existing_files,
                missing_paths,
            } => self.t_args(
                "dialog-details-path-valid",
                &[
                    ("existing", FluentValue::from(*existing_files)),
                    ("missing", FluentValue::from(*missing_paths)),
                ],
            ),
            TaskPathValidationView::Warning(error) => self.te(error),
        };
        let info_scroll = self.info_scroll.clone();

        let mono_font_family = cx.theme().mono_font_family.clone();
        div()
            .relative()
            .flex_1()
            .min_h_0()
            .min_w_0()
            .overflow_hidden()
            .child(
                div()
                    .id("task-details-info-scroll")
                    .size_full()
                    .min_h_0()
                    .min_w_0()
                    .overflow_y_scroll()
                    .track_scroll(&info_scroll)
                    .p_4()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child(detail_line_with_action(
                        self.t("dialog-details-gid"),
                        gid.clone(),
                        {
                            let panel = cx.entity().downgrade();
                            Clipboard::new("copy-task-gid")
                                .value(gid.clone())
                                .tooltip(self.t("dialog-details-copy-gid-tooltip"))
                                .accessibility_label(self.t("dialog-details-copy-gid-aria"))
                                .on_copied(move |_, _, cx| {
                                    let _ = panel.update(cx, |this, cx| {
                                        cx.emit(TaskDetailsPanelEvent::Notice {
                                            message: this.t("notice-gid-copied-short"),
                                            is_error: false,
                                        });
                                    });
                                })
                        },
                        mono_font_family,
                        colors,
                    ))
                    .child(detail_line(
                        self.t("dialog-details-source-type"),
                        self.t(self.overview.source_kind.message_key()),
                        colors,
                    ))
                    .when_some(
                        details
                            .primary_source
                            .as_deref()
                            .or(self.overview.primary_source.as_deref()),
                        |element, source| {
                            element.child(detail_line(
                                self.t("dialog-details-source"),
                                source,
                                colors,
                            ))
                        },
                    )
                    .child(detail_line(
                        self.t("dialog-details-directory"),
                        details
                            .directory
                            .as_deref()
                            .unwrap_or(&self.t("dialog-details-not-reported")),
                        colors,
                    ))
                    .when_some(details.output_path.as_deref(), |element, path| {
                        element.child(detail_line(self.t("dialog-details-output"), path, colors))
                    })
                    .child(detail_line(
                        self.t("dialog-details-path-check"),
                        path_validation_label,
                        colors,
                    ))
                    .when_some(self.overview.error.as_ref(), |element, error| {
                        element
                            .child(detail_line(
                                self.t("dialog-details-failure"),
                                error.summary.clone(),
                                colors,
                            ))
                            .when_some(error.details.as_deref(), |element, details| {
                                element.child(detail_line(
                                    self.t("dialog-details-aria2-details"),
                                    details,
                                    colors,
                                ))
                            })
                    })
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .pt_2()
                            .child(
                                toolbar_icon_button(
                                    "open-task-download",
                                    IconName::Download,
                                    self.t("action-open-file"),
                                    if path_actions_enabled {
                                        ToolbarButtonState::Enabled
                                    } else {
                                        ToolbarButtonState::Disabled
                                    },
                                    false,
                                    None,
                                    colors,
                                    &self.translator,
                                )
                                .on_click(cx.listener(
                                    |_this, _, _, cx| {
                                        cx.emit(TaskDetailsPanelEvent::OpenTask(
                                            TaskOpenTargetView::Download,
                                        ));
                                    },
                                )),
                            )
                            .child(
                                toolbar_icon_button(
                                    "open-task-folder",
                                    IconName::FolderDown,
                                    self.t("action-open-folder"),
                                    if path_actions_enabled {
                                        ToolbarButtonState::Enabled
                                    } else {
                                        ToolbarButtonState::Disabled
                                    },
                                    false,
                                    None,
                                    colors,
                                    &self.translator,
                                )
                                .on_click(cx.listener(
                                    |_this, _, _, cx| {
                                        cx.emit(TaskDetailsPanelEvent::OpenTask(
                                            TaskOpenTargetView::Folder,
                                        ));
                                    },
                                )),
                            )
                            .child(
                                toolbar_icon_button(
                                    "edit-task-from-details",
                                    IconName::Pencil,
                                    self.t("action-edit-task"),
                                    if edit_task_enabled {
                                        ToolbarButtonState::Enabled
                                    } else {
                                        ToolbarButtonState::Disabled
                                    },
                                    false,
                                    None,
                                    colors,
                                    &self.translator,
                                )
                                .on_click(cx.listener(
                                    |_this, _, _, cx| {
                                        cx.emit(TaskDetailsPanelEvent::EditTask);
                                    },
                                )),
                            ),
                    )
                    .when_some(details.info_hash.as_deref(), |element, hash| {
                        element.child(detail_line(
                            self.t("dialog-details-info-hash"),
                            hash,
                            colors,
                        ))
                    })
                    .when(
                        details.piece_length.is_some() || details.piece_count.is_some(),
                        |element| {
                            element.child(detail_line(
                                self.t("dialog-details-pieces"),
                                self.t_args(
                                    "dialog-details-piece-layout",
                                    &[
                                        (
                                            "count",
                                            FluentValue::from(details.piece_count.map_or_else(
                                                || self.t("dialog-details-not-reported"),
                                                |value| value.to_string(),
                                            )),
                                        ),
                                        (
                                            "size",
                                            FluentValue::from(details.piece_length.map_or_else(
                                                || self.t("dialog-details-not-reported"),
                                                format_bytes,
                                            )),
                                        ),
                                    ],
                                ),
                                colors,
                            ))
                        },
                    )
                    .when(is_bittorrent, |element| {
                        element.child(detail_line(
                            self.t("dialog-details-seed-limits"),
                            seed_stop_rules,
                            colors,
                        ))
                    }),
            )
            .vertical_scrollbar(&info_scroll)
            .into_any_element()
    }

    fn render_files_tab(&self, file_count: usize, cx: &mut Context<Self>) -> AnyElement {
        let colors = self.theme.colors;
        if file_count == 0 {
            return drawer_message(
                self.t("dialog-details-no-files"),
                self.t("dialog-details-no-files-detail"),
                colors,
            );
        }
        let file_scroll = self.file_scroll.clone();
        let shell_id = SharedString::from(format!("task-files-shell:{}", self.identity.gid));
        let list_id = SharedString::from(format!("task-files:{}", self.identity.gid));
        div()
            .id(shell_id)
            .role(Role::List)
            .aria_label(self.t_count("dialog-details-files-aria", file_count as u64))
            .relative()
            .flex_1()
            .min_h_0()
            .min_w_0()
            .flex()
            .flex_col()
            .overflow_hidden()
            .child(
                uniform_list(
                    list_id,
                    file_count,
                    cx.processor(move |this, range: Range<usize>, _window, _cx| {
                        let colors = this.theme.colors;
                        this.rendered_file_range = range.clone();
                        let TaskDetailsLoadState::Ready { details } = &this.state else {
                            return Vec::new();
                        };
                        let gid = this.identity.gid.clone();
                        let rows = range
                            .filter_map(|index| {
                                details
                                    .files
                                    .get(index)
                                    .cloned()
                                    .map(|file| (gid.clone(), index, file))
                            })
                            .collect::<Vec<_>>();
                        rows.into_iter()
                            .map(|(gid, index, file)| {
                                let state = this.t(if file.selected {
                                    "dialog-details-file-enabled"
                                } else {
                                    "dialog-details-file-skipped"
                                });
                                let progress = if file.length == 0 {
                                    None
                                } else {
                                    let completed =
                                        u128::from(file.completed_length.min(file.length));
                                    Some(((completed * 10_000) / u128::from(file.length)) as u16)
                                };
                                let aria_label = this.t_args(
                                    "dialog-details-file-aria",
                                    &[
                                        ("path", FluentValue::from(file.path.clone())),
                                        ("state", FluentValue::from(state)),
                                        ("size", FluentValue::from(format_bytes(file.length))),
                                        ("progress", FluentValue::from(format_percent(progress))),
                                    ],
                                );
                                render_file_row(&gid, index, file, file_count, aria_label, colors)
                            })
                            .collect::<Vec<_>>()
                    }),
                )
                .track_scroll(&file_scroll)
                .size_full(),
            )
            .vertical_scrollbar(&file_scroll)
            .into_any_element()
    }

    fn render_network_tab(&self, details: &TaskDetailsView, cx: &App) -> AnyElement {
        let colors = self.theme.colors;
        let mono_font_family = cx.theme().mono_font_family.clone();
        let network_scroll = self.network_scroll.clone();
        div()
            .relative()
            .flex_1()
            .min_h_0()
            .min_w_0()
            .overflow_hidden()
            .child(
                div()
                    .id("task-details-network-scroll")
                    .size_full()
                    .min_h_0()
                    .min_w_0()
                    .overflow_y_scroll()
                    .track_scroll(&network_scroll)
                    .p_4()
                    .flex()
                    .flex_col()
                    .gap_4()
                    .child(detail_collection_section(
                        self.t("dialog-details-sources"),
                        self.t("dialog-details-no-sources"),
                        details
                            .uris
                            .iter()
                            .map(|source| {
                                let status = self.t(source.status.message_key());
                                detail_collection_row(
                                    source.uri.clone(),
                                    status,
                                    None,
                                    mono_font_family.clone(),
                                    colors,
                                )
                            })
                            .collect(),
                        colors,
                    ))
                    .child(detail_collection_section(
                        self.t("dialog-details-trackers"),
                        self.t("dialog-details-no-trackers"),
                        details
                            .trackers
                            .iter()
                            .map(|tracker| {
                                let tier = self.t_args(
                                    "dialog-details-tracker-tier",
                                    &[("tier", FluentValue::from(tracker.tier))],
                                );
                                detail_collection_row(
                                    tracker.uri.clone(),
                                    tier,
                                    None,
                                    mono_font_family.clone(),
                                    colors,
                                )
                            })
                            .collect(),
                        colors,
                    ))
                    .child(detail_collection_section(
                        self.t("dialog-details-servers"),
                        self.t("dialog-details-no-servers"),
                        details
                            .servers
                            .iter()
                            .map(|server| {
                                let current_uri = if server.current_uri.is_empty() {
                                    server.uri.clone()
                                } else {
                                    server.current_uri.clone()
                                };
                                let rate = format_rate(server.download_rate);
                                let secondary =
                                    if server.uri.is_empty() || server.uri == current_uri {
                                        self.t_args(
                                            "dialog-details-server-file-rate",
                                            &[
                                                ("file", FluentValue::from(server.file_index)),
                                                ("rate", FluentValue::from(rate)),
                                            ],
                                        )
                                    } else {
                                        self.t_args(
                                            "dialog-details-server-source-file-rate",
                                            &[
                                                ("source", FluentValue::from(server.uri.clone())),
                                                ("file", FluentValue::from(server.file_index)),
                                                ("rate", FluentValue::from(rate)),
                                            ],
                                        )
                                    };
                                detail_collection_row(
                                    current_uri,
                                    secondary,
                                    None,
                                    mono_font_family.clone(),
                                    colors,
                                )
                            })
                            .collect(),
                        colors,
                    ))
                    .child(detail_collection_section(
                        self.t("dialog-details-peers"),
                        self.t("dialog-details-no-peers"),
                        details
                            .peers
                            .iter()
                            .map(|peer| {
                                let address = if peer.address.contains(':') {
                                    format!("[{}]:{}", peer.address, peer.port)
                                } else {
                                    format!("{}:{}", peer.address, peer.port)
                                };
                                let rates = self.t_args(
                                    "dialog-details-peer-rates",
                                    &[
                                        (
                                            "download",
                                            FluentValue::from(format_rate(peer.download_rate)),
                                        ),
                                        (
                                            "upload",
                                            FluentValue::from(format_rate(peer.upload_rate)),
                                        ),
                                    ],
                                );
                                let badge = peer.seeder.then(|| self.t("dialog-details-peer-seed"));
                                detail_collection_row(
                                    address,
                                    rates,
                                    badge,
                                    mono_font_family.clone(),
                                    colors,
                                )
                            })
                            .collect(),
                        colors,
                    )),
            )
            .vertical_scrollbar(&network_scroll)
            .into_any_element()
    }

    fn render_options_tab(&self, options: &[TaskOptionView], cx: &App) -> AnyElement {
        let colors = self.theme.colors;
        let mono_font_family = cx.theme().mono_font_family.clone();
        let options_scroll = self.options_scroll.clone();
        div()
            .relative()
            .flex_1()
            .min_h_0()
            .min_w_0()
            .overflow_hidden()
            .child(
                detail_collection_section(
                    self.t("dialog-details-read-only-options"),
                    self.t("dialog-details-no-options"),
                    options
                        .iter()
                        .map(|option| {
                            let value = if option.redacted {
                                self.t("dialog-details-option-hidden")
                            } else if option.value.is_empty() {
                                self.t("dialog-details-option-empty")
                            } else {
                                option.value.clone()
                            };
                            let badge = option
                                .redacted
                                .then(|| self.t("dialog-details-option-sensitive"));
                            detail_collection_row(
                                option.key.clone(),
                                value,
                                badge,
                                mono_font_family.clone(),
                                colors,
                            )
                        })
                        .collect(),
                    colors,
                )
                .id("task-details-options-scroll")
                .size_full()
                .min_h_0()
                .min_w_0()
                .overflow_y_scroll()
                .track_scroll(&options_scroll)
                .p_4(),
            )
            .vertical_scrollbar(&options_scroll)
            .into_any_element()
    }

    fn render_verify_tab(&self, details: &TaskDetailsView, cx: &mut Context<Self>) -> AnyElement {
        let colors = self.theme.colors;
        if !self.local_path_actions_available {
            return drawer_message(
                self.t("dialog-details-verify-unavailable"),
                self.t("dialog-details-verify-remote-desc"),
                colors,
            );
        }
        let selectable_files: Vec<(usize, &TaskFileView)> = details
            .files
            .iter()
            .enumerate()
            .filter(|(_, file)| !file.path.trim().is_empty())
            .collect();
        if selectable_files.is_empty() {
            return drawer_message(
                self.t("dialog-details-no-files"),
                self.t("dialog-details-no-files-detail"),
                colors,
            );
        }

        let directory = details.directory.as_deref().unwrap_or("").trim().to_owned();
        let selected_pos = selectable_files
            .iter()
            .position(|(idx, _)| *idx == self.checksum_file_index)
            .unwrap_or(0);
        let (_, selected_file) = selectable_files[selected_pos];
        let selected_file_path = selected_file.path.clone();
        let selected_file_name = std::path::Path::new(&selected_file.path)
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or(&selected_file.path)
            .to_owned();
        let can_compute = !directory.is_empty()
            && !selected_file_path.is_empty()
            && self.checksum_pending.is_none();

        let algo_index = ariadeck_domain::ChecksumAlgorithm::ALL
            .iter()
            .position(|algo| *algo == self.checksum_algorithm)
            .unwrap_or(2);
        let panel_algo = cx.entity().downgrade();
        let algo_control = SegmentedControl::new(
            "task-details-checksum-algo",
            ariadeck_domain::ChecksumAlgorithm::ALL.map(|algo| Segment::new(algo.label())),
            algo_index,
            self.theme,
        )
        .aria_label(self.t("dialog-details-verify-algo-aria"))
        .disabled(self.checksum_pending.is_some())
        .on_select(move |index, _window, cx| {
            let algo = ariadeck_domain::ChecksumAlgorithm::ALL
                .get(index)
                .copied()
                .unwrap_or(ariadeck_domain::ChecksumAlgorithm::Sha256);
            let _ = panel_algo.update(cx, |this, cx| {
                this.checksum_algorithm = algo;
                cx.notify();
            });
        });

        let mono_font_family = cx.theme().mono_font_family.clone();
        let verify_scroll = self.verify_scroll.clone();
        let expected_value = self
            .checksum_expected_input
            .as_ref()
            .map(|input| input.read(cx).value().to_string())
            .unwrap_or_default();

        let panel_file_nav = cx.entity().downgrade();
        let file_count = selectable_files.len();
        let file_list = div()
            .flex()
            .flex_col()
            .gap_1()
            .children(selectable_files.iter().map(|(raw_idx, file)| {
                let is_selected = *raw_idx == selectable_files[selected_pos].0;
                let idx_val = *raw_idx;
                let panel_pick = panel_file_nav.clone();
                let label = std::path::Path::new(&file.path)
                    .file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or(&file.path)
                    .to_owned();
                div()
                    .id(SharedString::from(format!("verify-file-{idx_val}")))
                    .px_2()
                    .py_1()
                    .rounded_sm()
                    .border_1()
                    .border_color(if is_selected {
                        colors.accent
                    } else {
                        colors.border
                    })
                    .bg(if is_selected {
                        colors.elevated_surface
                    } else {
                        colors.surface
                    })
                    .cursor_pointer()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap_2()
                    .text_xs()
                    .child(div().truncate().flex_1().child(label))
                    .child(
                        div()
                            .text_color(colors.text_muted)
                            .child(format_bytes(file.length)),
                    )
                    .on_click(move |_, _, cx| {
                        let _ = panel_pick.update(cx, |this, cx| {
                            if this.checksum_file_index != idx_val {
                                this.checksum_file_index = idx_val;
                                this.checksum_result = None;
                                cx.notify();
                            }
                        });
                    })
            }));

        let dir_for_compute = directory.clone();
        let path_for_compute = selected_file_path.clone();
        let algo_for_compute = self.checksum_algorithm;
        let pending_state = self.checksum_pending;
        let result_state = self.checksum_result.clone();
        let comparison_banner = match (&result_state, expected_value.trim()) {
            (Some((computed_algo, Ok(hex))), expected) if !expected.is_empty() => {
                match ariadeck_domain::compare_checksum(hex, expected, *computed_algo) {
                    ariadeck_domain::ChecksumComparison::Empty => None,
                    ariadeck_domain::ChecksumComparison::Match => {
                        Some((colors.success, self.t("dialog-details-verify-match")))
                    }
                    ariadeck_domain::ChecksumComparison::Mismatch => {
                        Some((colors.danger, self.t("dialog-details-verify-mismatch")))
                    }
                    ariadeck_domain::ChecksumComparison::AlgorithmMismatch { expected, actual } => {
                        Some((
                            colors.warning,
                            self.t_args(
                                "dialog-details-verify-algo-mismatch",
                                &[
                                    ("expected", FluentValue::from(expected.label())),
                                    ("actual", FluentValue::from(actual.label())),
                                ],
                            ),
                        ))
                    }
                    ariadeck_domain::ChecksumComparison::InvalidExpected => Some((
                        colors.warning,
                        self.t("dialog-details-verify-invalid-expected"),
                    )),
                }
            }
            _ => None,
        };

        div()
            .relative()
            .flex_1()
            .min_h_0()
            .min_w_0()
            .overflow_hidden()
            .child(
                div()
                    .id("task-details-verify-scroll")
                    .size_full()
                    .min_h_0()
                    .min_w_0()
                    .overflow_y_scroll()
                    .track_scroll(&verify_scroll)
                    .p_4()
                    .flex()
                    .flex_col()
                    .gap_3()
                    .child(detail_line(
                        self.t("dialog-details-verify-file"),
                        selected_file_name,
                        colors,
                    ))
                    .when(file_count > 1, |element| {
                        element.child(
                            div()
                                .flex()
                                .flex_col()
                                .gap_1()
                                .child(
                                    div()
                                        .text_xs()
                                        .font_weight(FontWeight::MEDIUM)
                                        .text_color(colors.text_muted)
                                        .child(self.t("dialog-details-verify-select-file")),
                                )
                                .child(file_list),
                        )
                    })
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .child(
                                div()
                                    .text_xs()
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(colors.text_muted)
                                    .child(self.t("dialog-details-verify-algorithm")),
                            )
                            .child(algo_control),
                    )
                    .child(div().flex().items_center().gap_2().child(
                        if let Some((pending_req, _, _)) = pending_state {
                            KitButton::new("cancel-checksum-btn")
                                .secondary()
                                .small()
                                .label(self.t("button-cancel"))
                                .on_click(cx.listener(move |_this, _, _, cx| {
                                    cx.emit(TaskDetailsPanelEvent::CancelChecksum(pending_req));
                                }))
                                .into_any_element()
                        } else {
                            KitButton::new("compute-checksum-btn")
                                .primary()
                                .small()
                                .label(self.t("dialog-details-verify-calculate"))
                                .disabled(!can_compute)
                                .on_click(cx.listener(move |_this, _, _, cx| {
                                    cx.emit(TaskDetailsPanelEvent::ComputeChecksum {
                                        directory: dir_for_compute.clone(),
                                        file_path: path_for_compute.clone(),
                                        algorithm: algo_for_compute,
                                    });
                                }))
                                .into_any_element()
                        },
                    ))
                    .when_some(pending_state, |element, (_, processed, total)| {
                        let percent = if total > 0 {
                            ((processed as f32 / total as f32).clamp(0.0, 1.0)) * 100.0
                        } else {
                            0.0
                        };
                        element.child(
                            div()
                                .flex()
                                .flex_col()
                                .gap_1()
                                .child(
                                    KitProgress::new("checksum-progress")
                                        .value(percent)
                                        .xsmall()
                                        .color(colors.progress_download),
                                )
                                .child(div().text_xs().text_color(colors.text_muted).child(
                                    format!(
                                        "{} / {}",
                                        format_bytes(processed),
                                        format_bytes(total)
                                    ),
                                )),
                        )
                    })
                    .when_some(result_state, |element, (algo, outcome)| match outcome {
                        Ok(hex) => {
                            let panel_copy = cx.entity().downgrade();
                            element.child(detail_line_with_action(
                                self.t_args(
                                    "dialog-details-verify-computed",
                                    &[("algo", FluentValue::from(algo.label()))],
                                ),
                                hex.clone(),
                                Clipboard::new("copy-computed-checksum")
                                    .value(hex)
                                    .tooltip(self.t("dialog-details-verify-copy-tooltip"))
                                    .accessibility_label(self.t("dialog-details-verify-copy-aria"))
                                    .on_copied(move |_, _, cx| {
                                        let _ = panel_copy.update(cx, |this, cx| {
                                            cx.emit(TaskDetailsPanelEvent::Notice {
                                                message: this.t("notice-checksum-copied"),
                                                is_error: false,
                                            });
                                        });
                                    }),
                                mono_font_family.clone(),
                                colors,
                            ))
                        }
                        Err(error) => element.child(
                            div()
                                .px_3()
                                .py_2()
                                .rounded_md()
                                .border_1()
                                .border_color(colors.danger)
                                .text_xs()
                                .text_color(colors.danger)
                                .child(self.te(&error)),
                        ),
                    })
                    .when_some(self.checksum_expected_input.as_ref(), |element, input| {
                        element.child(
                            div()
                                .flex()
                                .flex_col()
                                .gap_1()
                                .child(
                                    div()
                                        .text_xs()
                                        .font_weight(FontWeight::MEDIUM)
                                        .text_color(colors.text_muted)
                                        .child(self.t("dialog-details-verify-expected")),
                                )
                                .child(Input::new(input).small()),
                        )
                    })
                    .when_some(comparison_banner, |element, (border_color, text)| {
                        element.child(
                            div()
                                .px_3()
                                .py_2()
                                .rounded_md()
                                .border_1()
                                .border_color(border_color)
                                .text_xs()
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(border_color)
                                .child(text),
                        )
                    }),
            )
            .vertical_scrollbar(&verify_scroll)
            .into_any_element()
    }
}

impl Render for TaskDetailsPanelView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.ensure_checksum_input(window, cx);
        let colors = self.theme.colors;
        let reduced_motion = crate::accessibility::is_reduced_motion(cx);
        let inline = shell_layout(f32::from(window.viewport_size().width)).details_presentation
            == DetailsPresentation::Inline;
        let display_name = task_display_name(&self.overview, &self.translator);
        let overview_progress = self.overview.progress_basis_points();
        let path_actions_available = self.commands_available && self.local_path_actions_available;
        let path_open_pending = self.open_pending.is_some();
        let presentation = match &self.state {
            TaskDetailsLoadState::Loading => TaskDetailsPresentation::Loading,
            TaskDetailsLoadState::Ready { details } => {
                TaskDetailsPresentation::Ready(Arc::clone(details))
            }
            TaskDetailsLoadState::Failed { error } => {
                TaskDetailsPresentation::Failed(self.te(error))
            }
            TaskDetailsLoadState::Stale => TaskDetailsPresentation::Stale,
        };

        let body = match presentation {
            TaskDetailsPresentation::Loading => animate_fade_settle(
                div()
                    .id("task-details-state-loading")
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .flex_col()
                    .items_center()
                    .justify_center()
                    .gap_2()
                    .px_5()
                    .text_center()
                    .child(
                        crate::LoadingIndicator::new(colors.accent)
                            .size(IconSize::Large)
                            .label(self.t("dialog-details-loading")),
                    )
                    .child(
                        div()
                            .text_sm()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(self.t("dialog-details-loading")),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(colors.text_muted)
                            .child(self.t("ui-requesting-details")),
                    ),
                "task-details-state-loading-enter",
                px(4.0),
                140,
                reduced_motion,
            ),
            TaskDetailsPresentation::Failed(summary) => div()
                .flex_1()
                .min_h_0()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap_3()
                .px_5()
                .text_center()
                .child(
                    div()
                        .text_sm()
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(colors.danger)
                        .child(self.t("dialog-details-load-failed")),
                )
                .child(div().text_xs().text_color(colors.text_muted).child(summary))
                .child(
                    toolbar_icon_button(
                        "retry-task-details",
                        IconName::RotateCcw,
                        self.t("action-retry"),
                        ToolbarButtonState::Enabled,
                        false,
                        None,
                        colors,
                        &self.translator,
                    )
                    .on_click(cx.listener(|_this, _, _window, cx| {
                        cx.emit(TaskDetailsPanelEvent::RetryDetails);
                    })),
                )
                .into_any_element(),
            TaskDetailsPresentation::Stale => div()
                .flex_1()
                .min_h_0()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap_3()
                .px_5()
                .text_center()
                .child(
                    div()
                        .text_sm()
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(colors.warning)
                        .child(self.t("dialog-details-stale")),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(colors.text_muted)
                        .child(self.t("ui-reconnect-refresh-details")),
                )
                .when(self.commands_available, |element| {
                    element.child(
                        toolbar_icon_button(
                            "refresh-task-details",
                            IconName::RefreshCw,
                            self.t("dialog-details-refresh"),
                            ToolbarButtonState::Enabled,
                            false,
                            None,
                            colors,
                            &self.translator,
                        )
                        .on_click(cx.listener(|_this, _, _window, cx| {
                            cx.emit(TaskDetailsPanelEvent::RetryDetails);
                        })),
                    )
                })
                .into_any_element(),
            TaskDetailsPresentation::Ready(details) => {
                let panel = cx.entity().downgrade();
                let tabs = SegmentedControl::new(
                    "task-details-tabs",
                    [
                        Segment::new(self.t("dialog-details-info")),
                        Segment::new(self.t("ui-files")),
                        Segment::new(self.t("dialog-details-network")),
                        Segment::new(self.t("dialog-details-options")),
                        Segment::new(self.t("dialog-details-verify")),
                    ],
                    match self.tab {
                        TaskDetailsTab::Info => 0,
                        TaskDetailsTab::Files => 1,
                        TaskDetailsTab::Network => 2,
                        TaskDetailsTab::Options => 3,
                        TaskDetailsTab::Verify => 4,
                    },
                    self.theme,
                )
                .aria_label(self.t("dialog-details-tabs-aria"))
                .on_select(move |index, _window, cx| {
                    let tab = match index {
                        1 => TaskDetailsTab::Files,
                        2 => TaskDetailsTab::Network,
                        3 => TaskDetailsTab::Options,
                        4 => TaskDetailsTab::Verify,
                        _ => TaskDetailsTab::Info,
                    };
                    let _ = panel.update(cx, |this, cx| {
                        this.tab = tab;
                        cx.notify();
                    });
                });

                let (tab_key, content) = match self.tab {
                    TaskDetailsTab::Info => (
                        "info",
                        self.render_info_tab(
                            &details,
                            path_actions_available && !path_open_pending,
                            cx,
                        ),
                    ),
                    TaskDetailsTab::Files => {
                        ("files", self.render_files_tab(details.files.len(), cx))
                    }
                    TaskDetailsTab::Network => ("network", self.render_network_tab(&details, cx)),
                    TaskDetailsTab::Options => {
                        ("options", self.render_options_tab(&details.options, cx))
                    }
                    TaskDetailsTab::Verify => ("verify", self.render_verify_tab(&details, cx)),
                };
                let animated_content = animate_fade_settle(
                    div()
                        .id(SharedString::from(format!("task-details-tab-{tab_key}")))
                        .flex_1()
                        .min_w_0()
                        .min_h_0()
                        .flex()
                        .flex_col()
                        .child(content),
                    SharedString::from(format!("task-details-tab-enter:{tab_key}")),
                    px(4.0),
                    140,
                    reduced_motion,
                );

                div()
                    .flex_1()
                    .min_w_0()
                    .min_h_0()
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .flex_none()
                            .p_3()
                            .border_b_1()
                            .border_color(colors.border)
                            .bg(colors.toolbar_surface)
                            .child(tabs),
                    )
                    .child(animated_content)
                    .into_any_element()
            }
        };

        let drawer = div()
            .id("task-details-drawer")
            .role(Role::Complementary)
            .aria_label(self.t_args(
                "dialog-details-drawer-aria",
                &[("name", FluentValue::from(display_name.clone()))],
            ))
            .when(inline, |drawer| {
                drawer
                    .relative()
                    .w(px(self.drawer_width))
                    .max_w_full()
                    .h_full()
                    .flex_none()
                    .border_l_1()
                    .border_color(colors.border)
            })
            .when(!inline, |drawer| drawer.size_full())
            .min_w_0()
            .min_h_0()
            .flex()
            .flex_col()
            .bg(colors.surface)
            .when(inline, |drawer| {
                drawer.child(
                    resize_handle("details-drawer-resize-handle", Axis::Horizontal)
                        .inside(HandleEdge::Leading)
                        .on_drag(ResizeDetailsDrawer, |drag, _, _, cx| {
                            cx.new(|_| drag.clone())
                        }),
                )
            })
            .child(
                div()
                    .h(px(68.0))
                    .flex_none()
                    .flex()
                    .items_center()
                    .gap_3()
                    .px_4()
                    .border_b_1()
                    .border_color(colors.border)
                    .child(
                        div()
                            .size(px(36.0))
                            .flex_none()
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded_md()
                            .border_1()
                            .border_color(colors.border)
                            .bg(colors.elevated_surface)
                            .child(
                                Icon::new(task_status_icon(self.overview.status))
                                    .size(IconSize::Small)
                                    .color(task_status_color(self.overview.status, colors)),
                            ),
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
                                    .truncate()
                                    .text_sm()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(display_name),
                            )
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap_2()
                                    .text_xs()
                                    .text_color(colors.text_muted)
                                    .child(self.t(self.overview.status.message_key()))
                                    .child(format_percent(overview_progress)),
                            ),
                    )
                    .when(inline, |header| {
                        header.child(
                            toolbar_icon_button(
                                "close-task-details",
                                IconName::X,
                                self.t("dialog-details-close"),
                                ToolbarButtonState::Enabled,
                                false,
                                None,
                                colors,
                                &self.translator,
                            )
                            .on_click(cx.listener(
                                |_this, _, _window, cx| {
                                    cx.emit(TaskDetailsPanelEvent::Close);
                                },
                            )),
                        )
                    }),
            )
            .child(task_overview_summary(
                &self.overview,
                &self.translator,
                colors,
            ))
            .child(body);

        if inline {
            animate_fade_slide_x(
                drawer,
                "task-details-drawer-enter",
                px(14.0),
                160,
                reduced_motion,
            )
        } else {
            drawer.into_any_element()
        }
    }
}

impl AppShell {
    #[cfg(test)]
    pub(super) fn details_panel(&self) -> Option<&Entity<TaskDetailsPanelView>> {
        self.details_panel.as_ref()
    }

    pub(crate) fn clear_details_panel(&mut self) {
        self.details_panel = None;
        self._details_panel_sub = None;
    }

    fn cancel_active_checksum(&mut self, cx: &mut Context<Self>) {
        if let Some((request_id, _, _)) = self
            .details_panel
            .as_ref()
            .and_then(|panel| panel.read(cx).checksum_pending)
        {
            cx.emit(AppShellEvent::FileChecksumCancelled(request_id));
        }
    }

    fn details_panel_context(&self) -> TaskDetailsPanelContext {
        TaskDetailsPanelContext {
            theme: self.theme,
            translator: self.translator.clone(),
            commands_available: self.snapshot.commands_available(),
            change_option_available: self.snapshot.capabilities.change_option,
            local_path_actions_available: self.snapshot.local_path_actions_available,
            drawer_width: self.details_drawer_width,
        }
    }

    pub(crate) fn sync_details_panel_context(&self, cx: &mut Context<Self>) {
        if let Some(panel) = &self.details_panel {
            let panel_cx = self.details_panel_context();
            panel.update(cx, |panel, cx| {
                panel.sync_context(panel_cx, cx);
            });
        }
    }

    pub(crate) fn open_task_details_action(
        &mut self,
        _: &OpenTaskDetails,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(task) = self.selected_task_view(cx) else {
            self.show_notice(self.t("notice-select-task-details"), true, cx);
            return;
        };
        self.open_details_for(task, cx);
    }

    pub(crate) fn open_details_for(&mut self, task: DownloadRowView, cx: &mut Context<Self>) {
        let session = self
            .snapshot
            .engine_session()
            .unwrap_or_else(|| EngineSessionView {
                profile_id: task.identity.profile_id.clone(),
                session_id: String::new(),
                generation: self.snapshot.generation,
            });
        let panel_cx = self.details_panel_context();
        if let Some(panel) = self.details_panel.clone() {
            let same_task = panel.read(cx).identity == task.identity;
            if !same_task {
                self.cancel_active_checksum(cx);
            }
            let has_ready_details =
                matches!(panel.read(cx).state, TaskDetailsLoadState::Ready { .. });
            panel.update(cx, |panel, cx| {
                panel.switch_task(task, session, panel_cx, cx);
            });
            if self.snapshot.commands_available() && (!same_task || !has_ready_details) {
                self.request_current_details(false, cx);
            }
            self.sync_details_sheet_deferred(cx);
            cx.notify();
            return;
        }
        let panel = cx.new(|_| TaskDetailsPanelView::new(task, session, panel_cx));
        let sub = cx.subscribe(&panel, |this, panel, event, cx| match event {
            TaskDetailsPanelEvent::Close => {
                this.close_task_details_deferred(cx);
            }
            TaskDetailsPanelEvent::RetryDetails => {
                this.request_current_details(false, cx);
            }
            TaskDetailsPanelEvent::OpenTask(target) => {
                this.request_task_open(*target, cx);
            }
            TaskDetailsPanelEvent::EditTask => {
                let target = panel.read(cx).overview.clone();
                let shell = cx.entity().downgrade();
                let handle = this.window_handle;
                cx.defer(move |cx| {
                    let _ = handle.update(cx, |_, window, cx| {
                        let _ = shell.update(cx, |this, cx| {
                            this.open_edit_task_for(target, window, cx);
                        });
                    });
                });
            }
            TaskDetailsPanelEvent::ComputeChecksum {
                directory,
                file_path,
                algorithm,
            } => {
                this.request_file_checksum(directory.clone(), file_path.clone(), *algorithm, cx);
            }
            TaskDetailsPanelEvent::CancelChecksum(request_id) => {
                this.cancel_file_checksum(*request_id, cx);
            }
            TaskDetailsPanelEvent::Notice { message, is_error } => {
                this.show_notice(message.clone(), *is_error, cx);
            }
        });
        self.details_panel = Some(panel);
        self._details_panel_sub = Some(sub);
        if self.snapshot.commands_available() {
            self.request_current_details(false, cx);
        }
        self.sync_details_sheet_deferred(cx);
        cx.notify();
    }

    fn close_task_details_deferred(&mut self, cx: &mut Context<Self>) {
        if self.details_panel.is_none() {
            return;
        }
        self.cancel_active_checksum(cx);
        self.clear_details_panel();
        let shell = cx.entity().downgrade();
        let handle = self.window_handle;
        cx.defer(move |cx| {
            let _ = handle.update(cx, |_, window, cx| {
                let _ = shell.update(cx, |this, cx| {
                    this.sync_details_sheet(window, cx);
                    window.focus(&this.focus_handle, cx);
                });
            });
        });
        cx.notify();
    }

    /// Request details for the open drawer.
    ///
    /// When `rate_limited` is true and a recent fetch already ran for a Ready
    /// drawer, the request is coalesced until the min interval elapses or the
    /// next non-limited catch-up path runs (PERF-001).
    pub(crate) fn request_current_details(&mut self, rate_limited: bool, cx: &mut Context<Self>) {
        let Some(session) = self.snapshot.engine_session() else {
            return;
        };
        let Some(panel) = self.details_panel.clone() else {
            return;
        };
        let Some((identity, source_revision, active, is_bittorrent, ready)) =
            panel.read_with(cx, |drawer, _| {
                drawer.pending.is_none().then(|| {
                    (
                        drawer.identity.clone(),
                        drawer.overview.revision,
                        drawer.overview.status.uses_active_connections(),
                        matches!(
                            drawer.overview.source_kind,
                            crate::TaskSourceKindView::Magnet
                                | crate::TaskSourceKindView::BitTorrent
                        ) || drawer.overview.status == TaskStatusView::Seeding,
                        matches!(drawer.state, TaskDetailsLoadState::Ready { .. }),
                    )
                })
            })
        else {
            return;
        };
        if identity.profile_id != session.profile_id || !self.snapshot.commands_available() {
            return;
        }

        // Only throttle ready→ready revision refreshes (PERF-001). Initial loads
        // and forced catch-ups always proceed so D-017 stays responsive.
        if rate_limited && ready {
            let throttled = panel.update(cx, |drawer, _| {
                if drawer
                    .last_ready_refresh_at
                    .is_some_and(|last| last.elapsed() < DETAILS_REFRESH_MIN_INTERVAL)
                {
                    drawer.refresh_coalesced = true;
                    true
                } else {
                    false
                }
            });
            if throttled {
                return;
            }
        }

        let request_id = self.allocate_request_id();
        panel.update(cx, |drawer, cx| {
            drawer.session = session.clone();
            if !matches!(drawer.state, TaskDetailsLoadState::Ready { .. }) {
                drawer.state = TaskDetailsLoadState::Loading;
            }
            drawer.pending = Some(PendingTaskDetails {
                request_id,
                source_revision,
            });
            if ready {
                drawer.last_ready_refresh_at = Some(Instant::now());
            }
            drawer.refresh_coalesced = false;
            cx.notify();
        });
        cx.emit(AppShellEvent::TaskDetailsRequested(
            TaskDetailsRequestView {
                request_id,
                session,
                identity,
                active,
                is_bittorrent,
            },
        ));
        cx.notify();
    }

    pub(crate) fn request_task_open(&mut self, target: TaskOpenTargetView, cx: &mut Context<Self>) {
        if !self.snapshot.commands_available() || !self.snapshot.local_path_actions_available {
            self.show_notice(self.t("notice-open-path-managed-local-only"), true, cx);
            return;
        }
        let Some(session) = self.snapshot.engine_session() else {
            return;
        };
        let Some(panel) = self.details_panel.clone() else {
            return;
        };
        let Some(identity) = panel.read_with(cx, |drawer, _| {
            drawer
                .open_pending
                .is_none()
                .then(|| drawer.identity.clone())
        }) else {
            return;
        };
        if identity.profile_id != session.profile_id {
            return;
        }
        let request_id = self.allocate_request_id();
        panel.update(cx, |drawer, cx| {
            drawer.open_pending = Some(PendingTaskOpen { request_id, target });
            cx.notify();
        });
        cx.emit(AppShellEvent::TaskOpenRequested(TaskOpenRequestView {
            request_id,
            session,
            identity,
            target,
        }));
        cx.notify();
    }

    pub(crate) fn close_task_details(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.details_panel.is_some() {
            self.cancel_active_checksum(cx);
            self.clear_details_panel();
            self.sync_details_sheet(window, cx);
            window.focus(&self.focus_handle, cx);
            cx.notify();
        }
    }

    pub(crate) fn request_file_checksum(
        &mut self,
        directory: String,
        file_path: String,
        algorithm: ariadeck_domain::ChecksumAlgorithm,
        cx: &mut Context<Self>,
    ) {
        if !self.snapshot.local_path_actions_available {
            return;
        }
        let Some(panel) = self.details_panel.clone() else {
            return;
        };
        self.cancel_active_checksum(cx);
        let identity = panel.read(cx).identity.clone();
        let request_id = self.allocate_request_id();
        let total_bytes = panel.read_with(cx, |drawer, _| match &drawer.state {
            TaskDetailsLoadState::Ready { details } => details
                .files
                .iter()
                .find(|f| f.path == file_path)
                .map_or(0, |f| f.length),
            _ => 0,
        });
        panel.update(cx, |drawer, cx| {
            drawer.checksum_pending = Some((request_id, 0, total_bytes));
            drawer.checksum_result = None;
            cx.notify();
        });
        cx.emit(AppShellEvent::FileChecksumRequested(
            FileChecksumRequestView {
                request_id,
                identity,
                directory,
                file_path,
                algorithm,
            },
        ));
        cx.notify();
    }

    pub(crate) fn cancel_file_checksum(&mut self, request_id: RequestId, cx: &mut Context<Self>) {
        let Some(panel) = self.details_panel.clone() else {
            return;
        };
        let cancelled = panel.update(cx, |drawer, cx| {
            if drawer
                .checksum_pending
                .is_some_and(|(pending_id, _, _)| pending_id == request_id)
            {
                drawer.checksum_pending = None;
                cx.notify();
                true
            } else {
                false
            }
        });
        if cancelled {
            cx.emit(AppShellEvent::FileChecksumCancelled(request_id));
            cx.notify();
        }
    }

    pub fn set_file_checksum_progress(
        &mut self,
        progress: FileChecksumProgressView,
        cx: &mut Context<Self>,
    ) {
        let Some(panel) = self.details_panel.clone() else {
            return;
        };
        panel.update(cx, |drawer, cx| {
            if drawer.identity == progress.identity
                && drawer
                    .checksum_pending
                    .is_some_and(|(id, _, _)| id == progress.request_id)
            {
                drawer.checksum_pending = Some((
                    progress.request_id,
                    progress.processed_bytes,
                    progress.total_bytes,
                ));
                cx.notify();
            }
        });
    }

    pub fn set_file_checksum_result(
        &mut self,
        result: FileChecksumResultView,
        cx: &mut Context<Self>,
    ) {
        let Some(panel) = self.details_panel.clone() else {
            return;
        };
        panel.update(cx, |drawer, cx| {
            if drawer.identity == result.identity
                && drawer
                    .checksum_pending
                    .is_some_and(|(id, _, _)| id == result.request_id)
            {
                drawer.checksum_pending = None;
                drawer.checksum_result = Some((result.algorithm, result.outcome));
                cx.notify();
            }
        });
    }

    pub(crate) fn desired_details_sheet_open(&self, window: &Window) -> bool {
        self.details_panel.is_some()
            && self.page == AppPage::Downloads
            && shell_layout(f32::from(window.viewport_size().width)).details_presentation
                == DetailsPresentation::Overlay
    }

    pub(crate) fn sync_details_sheet(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !has_kit_root(window) {
            return;
        }
        let desired = self.desired_details_sheet_open(window);
        if desired == self.details_sheet_open {
            return;
        }
        if desired {
            let Some(panel) = self.details_panel.clone() else {
                return;
            };
            self.details_sheet_open = true;
            let shell_close = cx.entity().downgrade();
            let title = self.t("action-details");
            let width = self
                .details_drawer_width
                .min((f32::from(window.viewport_size().width) - 24.0).max(280.0));
            window.open_sheet(cx, move |sheet, _, _| {
                let shell_close = shell_close.clone();
                sheet
                    .title(title.clone())
                    .size(px(width))
                    .p_0()
                    .overlay(true)
                    .overlay_closable(true)
                    .child(panel.clone())
                    .on_close(move |_, _, cx| {
                        let _ = shell_close.update(cx, |this, cx| {
                            if this.details_sheet_open {
                                this.details_sheet_open = false;
                                this.clear_details_panel();
                                cx.notify();
                            }
                        });
                    })
            });
        } else {
            self.details_sheet_open = false;
            window.close_sheet(cx);
        }
    }

    pub(crate) fn sync_details_sheet_deferred(&self, cx: &mut Context<Self>) {
        let shell = cx.entity().downgrade();
        let handle = self.window_handle;
        cx.defer(move |cx| {
            let _ = handle.update(cx, |_, window, cx| {
                let _ = shell.update(cx, |this, cx| {
                    this.sync_details_sheet(window, cx);
                });
            });
        });
    }
}
