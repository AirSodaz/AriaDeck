//! Task details drawer for AppShell.

use super::*;

impl AppShell {
    pub(crate) fn open_task_details_action(
        &mut self,
        _: &OpenTaskDetails,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(task) = self.selected_task_view() else {
            self.show_notice("Select a visible task to open its details.", true, cx);
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
        });
        if self.snapshot.commands_available() {
            self.request_current_details(cx);
        }
        cx.notify();
    }

    pub(crate) fn request_current_details(&mut self, cx: &mut Context<Self>) {
        let Some(session) = self.snapshot.engine_session() else {
            return;
        };
        let Some((identity, source_revision, active, is_bittorrent)) =
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
                    )
                })
            })
        else {
            return;
        };
        if identity.profile_id != session.profile_id || !self.snapshot.commands_available() {
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
        let display_name = task_display_name(&overview);
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
                TaskDetailsPresentation::Failed(error.summary.clone())
            }
            TaskDetailsLoadState::Stale => TaskDetailsPresentation::Stale,
        };

        let body = match presentation {
            TaskDetailsPresentation::Loading => drawer_message(
                "Loading task details",
                "Requesting file metadata from this aria2 session.",
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
                        .child("Could not load task details"),
                )
                .child(div().text_xs().text_color(colors.text_muted).child(summary))
                .child(
                    toolbar_icon_button(
                        "retry-task-details",
                        IconName::RotateCcw,
                        "Retry",
                        ToolbarButtonState::Enabled,
                        false,
                        None,
                        colors,
                    )
                    .on_click(cx.listener(|this, _, _window, cx| {
                        this.request_current_details(cx);
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
                        .child("Details are stale"),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(colors.text_muted)
                        .child("Reconnect to refresh files for the current engine session."),
                )
                .when(self.snapshot.commands_available(), |element| {
                    element.child(
                        toolbar_icon_button(
                            "refresh-task-details",
                            IconName::RefreshCw,
                            "Refresh",
                            ToolbarButtonState::Enabled,
                            false,
                            None,
                            colors,
                        )
                        .on_click(cx.listener(|this, _, _window, cx| {
                            this.request_current_details(cx);
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
                let seed_stop_rules = format_seed_stop_rules(&options);
                let path_validation_label = match path_validation {
                    TaskPathValidationView::Unavailable => {
                        "Unavailable for an external or remote engine profile.".into()
                    }
                    TaskPathValidationView::Valid {
                        existing_files,
                        missing_paths,
                    } => format!(
                        "Validated locally: {existing_files} existing, {missing_paths} missing."
                    ),
                    TaskPathValidationView::Warning(error) => error.summary,
                };
                let shell = cx.entity().downgrade();
                let tabs = SegmentedControl::new(
                    "task-details-tabs",
                    [
                        Segment::new("Info"),
                        Segment::new("Files"),
                        Segment::new("Network"),
                        Segment::new("Options"),
                    ],
                    match selected_tab {
                        TaskDetailsTab::Info => 0,
                        TaskDetailsTab::Files => 1,
                        TaskDetailsTab::Network => 2,
                        TaskDetailsTab::Options => 3,
                    },
                    self.theme,
                )
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
                            "GID",
                            gid.clone(),
                            IconButton::new("copy-task-gid", IconName::Copy)
                                .aria_label("Copy task GID")
                                .tooltip(Tooltip::new("Copy GID"))
                                .on_click({
                                    let gid = gid.clone();
                                    cx.listener(move |this, _, _, cx| {
                                        cx.write_to_clipboard(ClipboardItem::new_string(
                                            gid.clone(),
                                        ));
                                        this.show_notice("GID copied.", false, cx);
                                    })
                                })
                                .render(colors),
                            colors,
                        ))
                        .child(detail_line(
                            "Source type",
                            overview.source_kind.label(),
                            colors,
                        ))
                        .when_some(
                            primary_source
                                .as_deref()
                                .or(overview.primary_source.as_deref()),
                            |element, source| element.child(detail_line("Source", source, colors)),
                        )
                        .child(detail_line(
                            "Directory",
                            directory.as_deref().unwrap_or("Not reported"),
                            colors,
                        ))
                        .when_some(output_path.as_deref(), |element, path| {
                            element.child(detail_line("Output", path, colors))
                        })
                        .child(detail_line(
                            "Local path check",
                            path_validation_label,
                            colors,
                        ))
                        .when_some(overview.error.as_ref(), |element, error| {
                            element
                                .child(detail_line("Failure", error.summary.clone(), colors))
                                .when_some(error.details.as_deref(), |element, details| {
                                    element.child(detail_line("aria2 details", details, colors))
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
                                        "Open download",
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
                                        "Open folder",
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
                            element.child(detail_line("Info hash", hash, colors))
                        })
                        .when(piece_length.is_some() || piece_count.is_some(), |element| {
                            element.child(detail_line(
                                "Pieces",
                                format!(
                                    "{} x {}",
                                    piece_count
                                        .map_or_else(|| "?".into(), |value| value.to_string()),
                                    piece_length.map_or_else(|| "unknown".into(), format_bytes)
                                ),
                                colors,
                            ))
                        })
                        .when(is_bittorrent, |element| {
                            element.child(detail_line(
                                "Effective seed limits",
                                seed_stop_rules,
                                colors,
                            ))
                        })
                        .into_any_element(),
                    TaskDetailsTab::Files => {
                        if file_count == 0 {
                            drawer_message(
                                "No files reported",
                                "aria2 did not return any file entries for this task.",
                                colors,
                            )
                        } else {
                            let list_id =
                                SharedString::from(format!("task-files:{}", identity.gid));
                            div()
                                .id(list_id.clone())
                                .role(Role::List)
                                .aria_label(format!("Task files, {file_count} items"))
                                .flex_1()
                                .min_h_0()
                                .child(
                                    uniform_list(
                                        list_id,
                                        file_count,
                                        cx.processor(
                                            move |this, range: Range<usize>, _window, _cx| {
                                                let colors = this.theme.colors;
                                                let Some(drawer) = &mut this.details_drawer else {
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
                                                            |file| {
                                                                render_file_row(
                                                                    &gid, index, file, file_count,
                                                                    colors,
                                                                )
                                                            },
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
                            "Sources and mirrors",
                            "No source URIs reported.",
                            uris.into_iter()
                                .map(|source| render_task_uri(source, colors))
                                .collect(),
                            colors,
                        ))
                        .child(detail_collection_section(
                            "Trackers",
                            "No BitTorrent trackers reported.",
                            trackers
                                .into_iter()
                                .map(|tracker| render_task_tracker(tracker, colors))
                                .collect(),
                            colors,
                        ))
                        .child(detail_collection_section(
                            "Servers",
                            "No active HTTP, HTTPS, or FTP servers.",
                            servers
                                .into_iter()
                                .map(|server| render_task_server(server, colors))
                                .collect(),
                            colors,
                        ))
                        .child(detail_collection_section(
                            "Peers",
                            "No active BitTorrent peers.",
                            peers
                                .into_iter()
                                .map(|peer| render_task_peer(peer, colors))
                                .collect(),
                            colors,
                        ))
                        .into_any_element(),
                    TaskDetailsTab::Options => detail_collection_section(
                        "Read-only task options",
                        "No task-specific options reported.",
                        options
                            .into_iter()
                            .map(|option| render_task_option(option, colors))
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
            .aria_label(format!("Task details for {display_name}"))
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
                                    .child(overview.status.label())
                                    .child(format_percent(overview_progress)),
                            ),
                    )
                    .child(
                        toolbar_icon_button(
                            "close-task-details",
                            IconName::X,
                            "Close details",
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
