//! Task details drawer for AppShell.

use super::*;

impl AppShell {
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

    pub(crate) fn open_task_details_action(
        &mut self,
        _: &OpenTaskDetails,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(task) = self.selected_task_view() else {
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
        self.details_drawer = Some(TaskDetailsDrawer {
            identity: task.identity.clone(),
            overview: task,
            session,
            state: TaskDetailsLoadState::Stale,
            pending: None,
            open_pending: None,
            tab: TaskDetailsTab::Info,
            file_scroll: UniformListScrollHandle::new(),
            rendered_file_range: 0..0,
            last_ready_refresh_at: None,
            refresh_coalesced: false,
        });
        if self.snapshot.commands_available() {
            self.request_current_details(false, cx);
        }
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
        let Some((identity, source_revision, active, is_bittorrent, ready)) =
            self.details_drawer.as_ref().and_then(|drawer| {
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
        if rate_limited
            && ready
            && let Some(drawer) = self.details_drawer.as_mut()
            && let Some(last) = drawer.last_ready_refresh_at
            && last.elapsed() < DETAILS_REFRESH_MIN_INTERVAL
        {
            drawer.refresh_coalesced = true;
            return;
        }

        let request_id = self.allocate_request_id();
        if let Some(drawer) = &mut self.details_drawer {
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
        }
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
        let Some(identity) = self.details_drawer.as_ref().and_then(|drawer| {
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
        if let Some(drawer) = &mut self.details_drawer {
            drawer.open_pending = Some(PendingTaskOpen { request_id, target });
        }
        cx.emit(AppShellEvent::TaskOpenRequested(TaskOpenRequestView {
            request_id,
            session,
            identity,
            target,
        }));
        cx.notify();
    }

    pub(crate) fn close_task_details(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.details_drawer.take().is_some() {
            window.focus(&self.focus_handle, cx);
            cx.notify();
        }
    }

    pub(crate) fn render_task_details_drawer(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let colors = self.theme.colors;
        let Some(drawer) = self.details_drawer.as_ref() else {
            return div().into_any_element();
        };
        let identity = drawer.identity.clone();
        let overview = drawer.overview.clone();
        let selected_tab = drawer.tab;
        let display_name = if overview.name_state.is_resolving() {
            self.t("task-name-resolving")
        } else {
            overview.display_name.clone()
        };
        let overview_progress = overview.progress_basis_points();
        let path_actions_available =
            self.snapshot.commands_available() && self.snapshot.local_path_actions_available;
        let path_open_pending = drawer.open_pending.is_some();
        let presentation = match &drawer.state {
            TaskDetailsLoadState::Loading => TaskDetailsPresentation::Loading,
            TaskDetailsLoadState::Ready { details } => {
                TaskDetailsPresentation::Ready(details.clone())
            }
            TaskDetailsLoadState::Failed { error } => {
                TaskDetailsPresentation::Failed(self.te(error))
            }
            TaskDetailsLoadState::Stale => TaskDetailsPresentation::Stale,
        };

        let body = match presentation {
            TaskDetailsPresentation::Loading => drawer_message(
                self.t("dialog-details-loading"),
                self.t("ui-requesting-details"),
                colors,
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
                    )
                    .on_click(cx.listener(|this, _, _window, cx| {
                        this.request_current_details(false, cx);
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
                .when(self.snapshot.commands_available(), |element| {
                    element.child(
                        toolbar_icon_button(
                            "refresh-task-details",
                            IconName::RefreshCw,
                            self.t("dialog-details-refresh"),
                            ToolbarButtonState::Enabled,
                            false,
                            None,
                            colors,
                        )
                        .on_click(cx.listener(|this, _, _window, cx| {
                            this.request_current_details(false, cx);
                        })),
                    )
                })
                .into_any_element(),
            TaskDetailsPresentation::Ready(details) => {
                let TaskDetailsView {
                    directory,
                    primary_source,
                    output_path,
                    path_validation,
                    info_hash,
                    piece_length,
                    piece_count,
                    trackers,
                    uris,
                    servers,
                    peers,
                    options,
                    files,
                } = *details;
                let file_count = files.len();
                let gid = identity.gid.clone();
                let is_bittorrent = matches!(
                    overview.source_kind,
                    crate::TaskSourceKindView::Magnet | crate::TaskSourceKindView::BitTorrent
                ) || overview.status == TaskStatusView::Seeding;
                let seed_stop_rules = self.format_seed_stop_rules(&options);
                let path_validation_label = match path_validation {
                    TaskPathValidationView::Unavailable => {
                        self.t("dialog-details-path-unavailable")
                    }
                    TaskPathValidationView::Valid {
                        existing_files,
                        missing_paths,
                    } => self.t_args(
                        "dialog-details-path-valid",
                        &[
                            ("existing", FluentValue::from(existing_files)),
                            ("missing", FluentValue::from(missing_paths)),
                        ],
                    ),
                    TaskPathValidationView::Warning(error) => self.te(&error),
                };
                let shell = cx.entity().downgrade();
                let tabs = SegmentedControl::new(
                    "task-details-tabs",
                    [
                        Segment::new(self.t("dialog-details-info")),
                        Segment::new(self.t("ui-files")),
                        Segment::new(self.t("dialog-details-network")),
                        Segment::new(self.t("dialog-details-options")),
                    ],
                    match selected_tab {
                        TaskDetailsTab::Info => 0,
                        TaskDetailsTab::Files => 1,
                        TaskDetailsTab::Network => 2,
                        TaskDetailsTab::Options => 3,
                    },
                    self.theme,
                )
                .aria_label(self.t("dialog-details-tabs-aria"))
                .on_select(move |index, _window, cx| {
                    let tab = match index {
                        1 => TaskDetailsTab::Files,
                        2 => TaskDetailsTab::Network,
                        3 => TaskDetailsTab::Options,
                        _ => TaskDetailsTab::Info,
                    };
                    shell
                        .update(cx, |shell, cx| {
                            if let Some(drawer) = &mut shell.details_drawer {
                                drawer.tab = tab;
                                cx.notify();
                            }
                        })
                        .ok();
                });

                let content = match selected_tab {
                    TaskDetailsTab::Info => div()
                        .id("task-details-info-scroll")
                        .flex_1()
                        .min_h_0()
                        .overflow_y_scroll()
                        .p_4()
                        .flex()
                        .flex_col()
                        .gap_2()
                        .child(detail_line_with_action(
                            self.t("dialog-details-gid"),
                            gid.clone(),
                            IconButton::new("copy-task-gid", IconName::Copy)
                                .aria_label(self.t("dialog-details-copy-gid-aria"))
                                .tooltip(Tooltip::new(self.t("dialog-details-copy-gid-tooltip")))
                                .on_click({
                                    let gid = gid.clone();
                                    cx.listener(move |this, _, _, cx| {
                                        cx.write_to_clipboard(ClipboardItem::new_string(
                                            gid.clone(),
                                        ));
                                        this.show_notice(
                                            this.t("notice-gid-copied-short"),
                                            false,
                                            cx,
                                        );
                                    })
                                })
                                .render(colors),
                            colors,
                        ))
                        .child(detail_line(
                            self.t("dialog-details-source-type"),
                            self.t(overview.source_kind.message_key()),
                            colors,
                        ))
                        .when_some(
                            primary_source
                                .as_deref()
                                .or(overview.primary_source.as_deref()),
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
                            directory
                                .as_deref()
                                .unwrap_or(&self.t("dialog-details-not-reported")),
                            colors,
                        ))
                        .when_some(output_path.as_deref(), |element, path| {
                            element.child(detail_line(
                                self.t("dialog-details-output"),
                                path,
                                colors,
                            ))
                        })
                        .child(detail_line(
                            self.t("dialog-details-path-check"),
                            path_validation_label,
                            colors,
                        ))
                        .when_some(overview.error.as_ref(), |element, error| {
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
                                        if path_actions_available && !path_open_pending {
                                            ToolbarButtonState::Enabled
                                        } else {
                                            ToolbarButtonState::Disabled
                                        },
                                        false,
                                        None,
                                        colors,
                                    )
                                    .on_click(cx.listener(
                                        |this, _, _, cx| {
                                            this.request_task_open(
                                                TaskOpenTargetView::Download,
                                                cx,
                                            );
                                        },
                                    )),
                                )
                                .child(
                                    toolbar_icon_button(
                                        "open-task-folder",
                                        IconName::FolderDown,
                                        self.t("action-open-folder"),
                                        if path_actions_available && !path_open_pending {
                                            ToolbarButtonState::Enabled
                                        } else {
                                            ToolbarButtonState::Disabled
                                        },
                                        false,
                                        None,
                                        colors,
                                    )
                                    .on_click(cx.listener(
                                        |this, _, _, cx| {
                                            this.request_task_open(TaskOpenTargetView::Folder, cx);
                                        },
                                    )),
                                ),
                        )
                        .when_some(info_hash.as_deref(), |element, hash| {
                            element.child(detail_line(
                                self.t("dialog-details-info-hash"),
                                hash,
                                colors,
                            ))
                        })
                        .when(piece_length.is_some() || piece_count.is_some(), |element| {
                            element.child(detail_line(
                                self.t("dialog-details-pieces"),
                                self.t_args(
                                    "dialog-details-piece-layout",
                                    &[
                                        (
                                            "count",
                                            FluentValue::from(piece_count.map_or_else(
                                                || self.t("dialog-details-not-reported"),
                                                |value| value.to_string(),
                                            )),
                                        ),
                                        (
                                            "size",
                                            FluentValue::from(piece_length.map_or_else(
                                                || self.t("dialog-details-not-reported"),
                                                format_bytes,
                                            )),
                                        ),
                                    ],
                                ),
                                colors,
                            ))
                        })
                        .when(is_bittorrent, |element| {
                            element.child(detail_line(
                                self.t("dialog-details-seed-limits"),
                                seed_stop_rules,
                                colors,
                            ))
                        })
                        .into_any_element(),
                    TaskDetailsTab::Files => {
                        if file_count == 0 {
                            drawer_message(
                                self.t("dialog-details-no-files"),
                                self.t("dialog-details-no-files-detail"),
                                colors,
                            )
                        } else {
                            let list_id =
                                SharedString::from(format!("task-files:{}", identity.gid));
                            div()
                                .id(list_id.clone())
                                .role(Role::List)
                                .aria_label(
                                    self.t_count("dialog-details-files-aria", file_count as u64),
                                )
                                .flex_1()
                                .min_h_0()
                                .child(
                                    uniform_list(
                                        list_id,
                                        file_count,
                                        cx.processor(
                                            move |this, range: Range<usize>, _window, _cx| {
                                                let colors = this.theme.colors;
                                                let rows = {
                                                    let Some(drawer) = &mut this.details_drawer
                                                    else {
                                                        return Vec::new();
                                                    };
                                                    drawer.rendered_file_range = range.clone();
                                                    let TaskDetailsLoadState::Ready { details } =
                                                        &drawer.state
                                                    else {
                                                        return Vec::new();
                                                    };
                                                    let gid = drawer.identity.gid.clone();
                                                    range
                                                        .filter_map(|index| {
                                                            details.files.get(index).cloned().map(
                                                                |file| (gid.clone(), index, file),
                                                            )
                                                        })
                                                        .collect::<Vec<_>>()
                                                };
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
                                                            let completed = u128::from(
                                                                file.completed_length
                                                                    .min(file.length),
                                                            );
                                                            Some(
                                                                ((completed * 10_000)
                                                                    / u128::from(file.length))
                                                                    as u16,
                                                            )
                                                        };
                                                        let aria_label = this.t_args(
                                                            "dialog-details-file-aria",
                                                            &[
                                                                (
                                                                    "path",
                                                                    FluentValue::from(
                                                                        file.path.clone(),
                                                                    ),
                                                                ),
                                                                ("state", FluentValue::from(state)),
                                                                (
                                                                    "size",
                                                                    FluentValue::from(
                                                                        format_bytes(file.length),
                                                                    ),
                                                                ),
                                                                (
                                                                    "progress",
                                                                    FluentValue::from(
                                                                        format_percent(progress),
                                                                    ),
                                                                ),
                                                            ],
                                                        );
                                                        render_file_row(
                                                            &gid, index, file, file_count,
                                                            aria_label, colors,
                                                        )
                                                    })
                                                    .collect::<Vec<_>>()
                                            },
                                        ),
                                    )
                                    .track_scroll(
                                        &self
                                            .details_drawer
                                            .as_ref()
                                            .expect("details drawer exists while rendering files")
                                            .file_scroll,
                                    )
                                    .size_full(),
                                )
                                .into_any_element()
                        }
                    }
                    TaskDetailsTab::Network => div()
                        .id("task-details-network-scroll")
                        .flex_1()
                        .min_h_0()
                        .overflow_y_scroll()
                        .p_4()
                        .flex()
                        .flex_col()
                        .gap_4()
                        .child(detail_collection_section(
                            self.t("dialog-details-sources"),
                            self.t("dialog-details-no-sources"),
                            uris.into_iter()
                                .map(|source| {
                                    let status = self.t(source.status.message_key());
                                    detail_collection_row(source.uri, status, None, colors)
                                })
                                .collect(),
                            colors,
                        ))
                        .child(detail_collection_section(
                            self.t("dialog-details-trackers"),
                            self.t("dialog-details-no-trackers"),
                            trackers
                                .into_iter()
                                .map(|tracker| {
                                    let tier = self.t_args(
                                        "dialog-details-tracker-tier",
                                        &[("tier", FluentValue::from(tracker.tier))],
                                    );
                                    detail_collection_row(tracker.uri, tier, None, colors)
                                })
                                .collect(),
                            colors,
                        ))
                        .child(detail_collection_section(
                            self.t("dialog-details-servers"),
                            self.t("dialog-details-no-servers"),
                            servers
                                .into_iter()
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
                                                    ("source", FluentValue::from(server.uri)),
                                                    ("file", FluentValue::from(server.file_index)),
                                                    ("rate", FluentValue::from(rate)),
                                                ],
                                            )
                                        };
                                    detail_collection_row(current_uri, secondary, None, colors)
                                })
                                .collect(),
                            colors,
                        ))
                        .child(detail_collection_section(
                            self.t("dialog-details-peers"),
                            self.t("dialog-details-no-peers"),
                            peers
                                .into_iter()
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
                                    let badge =
                                        peer.seeder.then(|| self.t("dialog-details-peer-seed"));
                                    detail_collection_row(address, rates, badge, colors)
                                })
                                .collect(),
                            colors,
                        ))
                        .into_any_element(),
                    TaskDetailsTab::Options => detail_collection_section(
                        self.t("dialog-details-read-only-options"),
                        self.t("dialog-details-no-options"),
                        options
                            .into_iter()
                            .map(|option| {
                                let value = if option.redacted {
                                    self.t("dialog-details-option-hidden")
                                } else if option.value.is_empty() {
                                    self.t("dialog-details-option-empty")
                                } else {
                                    option.value
                                };
                                let badge = option
                                    .redacted
                                    .then(|| self.t("dialog-details-option-sensitive"));
                                detail_collection_row(option.key, value, badge, colors)
                            })
                            .collect(),
                        colors,
                    )
                    .id("task-details-options-scroll")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .p_4()
                    .into_any_element(),
                };

                div()
                    .flex_1()
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
                    .child(content)
                    .into_any_element()
            }
        };

        div()
            .id("task-details-drawer")
            .role(Role::Complementary)
            .aria_label(self.t_args(
                "dialog-details-drawer-aria",
                &[("name", FluentValue::from(display_name.clone()))],
            ))
            .w(px(DETAILS_DRAWER_WIDTH))
            .flex_none()
            .min_h_0()
            .flex()
            .flex_col()
            .border_l_1()
            .border_color(colors.border)
            .bg(colors.surface)
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
                                Icon::new(task_status_icon(overview.status))
                                    .size(IconSize::Small)
                                    .color(task_status_color(overview.status, colors)),
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
                                    .child(self.t(overview.status.message_key()))
                                    .child(format_percent(overview_progress)),
                            ),
                    )
                    .child(
                        toolbar_icon_button(
                            "close-task-details",
                            IconName::X,
                            self.t("dialog-details-close"),
                            ToolbarButtonState::Enabled,
                            false,
                            None,
                            colors,
                        )
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.close_task_details(window, cx);
                        })),
                    ),
            )
            .child(task_overview_summary(&overview, colors))
            .child(body)
            .into_any_element()
    }
}
