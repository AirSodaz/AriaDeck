//! Free functions and small UI helpers used by AppShell render/logic paths.

use super::*;
// Re-exported via parent `use helpers::*` for AppShell and tests.

pub(crate) fn theme_for_scheme(scheme: ColorSchemeView) -> Theme {
    match scheme {
        ColorSchemeView::System | ColorSchemeView::Dark => Theme::dark(),
        ColorSchemeView::Light => Theme::light(),
    }
}

pub(crate) fn resolve_theme(scheme: ColorSchemeView, window: &Window) -> Theme {
    match scheme {
        ColorSchemeView::Light => Theme::light(),
        ColorSchemeView::Dark => Theme::dark(),
        ColorSchemeView::System => match window.appearance() {
            gpui::WindowAppearance::Light | gpui::WindowAppearance::VibrantLight => Theme::light(),
            gpui::WindowAppearance::Dark | gpui::WindowAppearance::VibrantDark => Theme::dark(),
        },
    }
}

pub(crate) fn speed_chart_window(history: &[SpeedSampleView]) -> &[SpeedSampleView] {
    if history.len() > SPEED_CHART_SAMPLES {
        &history[history.len() - SPEED_CHART_SAMPLES..]
    } else {
        history
    }
}

pub(crate) fn speed_chart_legend(
    label: impl Into<SharedString>,
    color: Hsla,
    colors: crate::ThemeColors,
) -> Div {
    div()
        .flex()
        .items_center()
        .gap_1()
        .child(div().size(px(6.0)).rounded_sm().bg(color))
        .child(div().text_color(colors.text_muted).child(label.into()))
}

pub(crate) fn toolbar_icon_button(
    id: impl Into<ElementId>,
    icon: IconName,
    label: impl Into<SharedString>,
    state: ToolbarButtonState,
    danger: bool,
    action: Option<&dyn gpui::Action>,
    _colors: crate::ThemeColors,
) -> KitButton {
    let label = label.into();
    let enabled = state == ToolbarButtonState::Enabled;
    let loading = state == ToolbarButtonState::Loading;
    let mut button = KitButton::new(id)
        .icon(gpui_kit::component::Icon::new(icon))
        .small()
        .compact()
        .accessibility_label(if enabled || loading {
            label.to_string()
        } else {
            format!("{label} unavailable")
        })
        .disabled(!enabled && !loading)
        .loading(loading);
    button = if danger {
        button.danger()
    } else {
        button.ghost()
    };
    if let Some(action) = action {
        button.tooltip_with_action(label, action, Some("DownloadWorkspace"))
    } else {
        button.tooltip(label)
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn queue_move_button(
    id: &'static str,
    icon: IconName,
    label: impl Into<SharedString>,
    command: TaskCommandView,
    enabled: bool,
    pending_command: Option<&TaskCommandView>,
    action: Option<&dyn gpui::Action>,
    colors: crate::ThemeColors,
    cx: &mut Context<AppShell>,
) -> KitButton {
    let loading = pending_command == Some(&command);
    toolbar_icon_button(
        id,
        icon,
        label,
        ToolbarButtonState::from_flags(enabled, loading),
        false,
        action,
        colors,
    )
    .when(enabled, move |button| {
        button.on_click(cx.listener(move |this, _, _window, cx| {
            this.begin_task_command(command.clone(), cx);
        }))
    })
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ContextMenuAction {
    Details,
    OpenDownload,
    OpenFolder,
    CopySource,
    CopyGid,
    Pause,
    ForcePause,
    Resume,
    Retry,
    MoveTop,
    MoveUp,
    MoveDown,
    MoveBottom,
    OutputName,
    SpeedLimit,
    TaskOptions,
    Remove,
}

impl ContextMenuAction {
    #[allow(dead_code)]
    pub(crate) const fn element_id(self) -> &'static str {
        match self {
            Self::Details => "ctx-menu-details",
            Self::OpenDownload => "ctx-menu-open-download",
            Self::OpenFolder => "ctx-menu-open-folder",
            Self::CopySource => "ctx-menu-copy-source",
            Self::CopyGid => "ctx-menu-copy-gid",
            Self::Pause => "ctx-menu-pause",
            Self::ForcePause => "ctx-menu-force-pause",
            Self::Resume => "ctx-menu-resume",
            Self::Retry => "ctx-menu-retry",
            Self::MoveTop => "ctx-menu-move-top",
            Self::MoveUp => "ctx-menu-move-up",
            Self::MoveDown => "ctx-menu-move-down",
            Self::MoveBottom => "ctx-menu-move-bottom",
            Self::OutputName => "ctx-menu-output-name",
            Self::SpeedLimit => "ctx-menu-speed-limit",
            Self::TaskOptions => "ctx-menu-task-options",
            Self::Remove => "ctx-menu-remove",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ToolbarButtonState {
    Enabled,
    Disabled,
    Loading,
}

impl ToolbarButtonState {
    pub(crate) fn from_flags(enabled: bool, loading: bool) -> Self {
        if loading {
            Self::Loading
        } else if enabled {
            Self::Enabled
        } else {
            Self::Disabled
        }
    }
}

pub(crate) fn settings_labeled_input(
    label: impl Into<SharedString>,
    input: &Entity<InputState>,
    colors: crate::ThemeColors,
) -> Div {
    let label = label.into();
    div()
        .flex()
        .flex_col()
        .gap_1()
        .child(div().text_xs().text_color(colors.text_muted).child(label))
        .child(Input::new(input))
}

pub(crate) fn settings_path_field_row(
    input: &Entity<InputState>,
    browse_id: impl Into<ElementId>,
    browse_label: impl Into<SharedString>,
    browse_aria: impl Into<SharedString>,
    target: PathPickTarget,
    _colors: crate::ThemeColors,
    cx: &mut Context<AppShell>,
) -> Div {
    let browse_label = browse_label.into();
    let browse_aria = browse_aria.into();
    div()
        .flex()
        .items_center()
        .gap_2()
        .child(div().flex_1().min_w_0().child(Input::new(input)))
        .child(
            KitButton::new(browse_id)
                .icon(gpui_kit::component::Icon::new(IconName::FolderDown))
                .label(browse_label)
                .accessibility_label(browse_aria)
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.pick_path_for_field(target, window, cx);
                })),
        )
}

/// A titled card used as the visual container for one settings group.
/// Children are appended with `.child()` on the returned `Div`.
pub(crate) fn filter_icon(filter: WorkspaceFilter) -> IconName {
    match filter {
        WorkspaceFilter::All => IconName::List,
        WorkspaceFilter::Active => IconName::Activity,
        WorkspaceFilter::Waiting => IconName::Clock3,
        WorkspaceFilter::Paused => IconName::Pause,
        WorkspaceFilter::Completed => IconName::CircleCheck,
        WorkspaceFilter::Failed => IconName::CircleAlert,
    }
}

pub(crate) fn task_status_icon(status: TaskStatusView) -> IconName {
    match status {
        TaskStatusView::Active => IconName::Activity,
        TaskStatusView::Seeding => IconName::ArrowUp,
        TaskStatusView::Waiting => IconName::Clock3,
        TaskStatusView::Paused => IconName::Pause,
        TaskStatusView::Complete => IconName::CircleCheck,
        TaskStatusView::Failed => IconName::CircleAlert,
        TaskStatusView::Verifying => IconName::ScanSearch,
        TaskStatusView::Removed => IconName::Trash2,
        TaskStatusView::Unknown => IconName::CircleHelp,
    }
}

pub(crate) fn task_display_name(task: &DownloadRowView) -> String {
    if task.name_state.is_resolving() {
        "Resolving filename...".into()
    } else {
        task.display_name.clone()
    }
}

pub(crate) fn parse_add_download_sources(input: &str) -> Vec<AddDownloadSourceView> {
    input
        .lines()
        .enumerate()
        .filter_map(|(index, line)| {
            let uri = line.trim();
            (!uri.is_empty()).then(|| AddDownloadSourceView::Uri {
                line: index + 1,
                uri: uri.to_owned(),
            })
        })
        .collect()
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

pub(crate) fn can_accept_metadata_drop(enabled: bool, paths: &[PathBuf]) -> bool {
    enabled
        && paths
            .iter()
            .any(|path| metadata_kind_from_path(path).is_some())
}

pub(crate) fn metadata_path_key(path: &Path) -> String {
    let key = path.to_string_lossy().replace('\\', "/");
    if cfg!(windows) {
        key.to_ascii_lowercase()
    } else {
        key
    }
}

pub(crate) fn selected_metadata_known_bytes(
    previews: &[AddDownloadMetadataPreviewView],
) -> Option<u64> {
    previews.iter().try_fold(0_u64, |total, preview| {
        preview.files.iter().try_fold(total, |total, file| {
            if preview
                .selected_file_indices
                .binary_search(&file.index)
                .is_ok()
            {
                file.length
                    .map_or(Some(total), |length| total.checked_add(length))
            } else {
                Some(total)
            }
        })
    })
}

pub(crate) fn successor_task(
    previous: &WorkspaceSnapshot,
    next: &WorkspaceSnapshot,
    selected: &TaskIdentity,
) -> Option<DownloadRowView> {
    if selected.profile_id != next.profile_id {
        return None;
    }

    let previous_task = previous
        .tasks
        .iter()
        .find(|task| task.identity == *selected);
    if let Some(previous_task) = previous_task
        && let Some(successor) = previous_task
            .followed_by
            .iter()
            .find_map(|gid| next.tasks.iter().find(|task| task.identity.gid == *gid))
    {
        return Some(successor.clone());
    }

    next.tasks
        .iter()
        .find(|task| task.belongs_to.as_deref() == Some(selected.gid.as_str()))
        .cloned()
}

pub(crate) fn task_overview_summary(task: &DownloadRowView, colors: crate::ThemeColors) -> Div {
    let basis_points = task.progress_basis_points();
    let progress = f32::from(basis_points.unwrap_or(0)) / 10_000.0;
    let seeding = task.status == TaskStatusView::Seeding;
    let size_label = if task.total_bytes == 0 {
        format_bytes(task.completed_bytes)
    } else {
        format!(
            "{} / {}",
            format_bytes(task.completed_bytes),
            format_bytes(task.total_bytes)
        )
    };
    let eta_label = if seeding {
        format!(
            "{} observed this session",
            format_eta(task.observed_seeding_seconds)
        )
    } else {
        task.eta_seconds.filter(|seconds| *seconds > 0).map_or_else(
            || task.status.label().to_owned(),
            |seconds| format!("{} remaining", format_eta(Some(seconds))),
        )
    };
    let error_label = task.error.as_ref().map(|error| {
        error.code.map_or_else(
            || error.summary.clone(),
            |code| format!("Error {code}: {}", error.summary),
        )
    });
    let error_id = SharedString::from(format!("task-error-{}", task.identity.gid));

    div()
        .flex_none()
        .flex()
        .flex_col()
        .gap_3()
        .p_4()
        .border_b_1()
        .border_color(colors.border)
        .bg(colors.elevated_surface)
        .child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .child(
                    div()
                        .text_xs()
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(colors.text_secondary)
                        .child(if seeding { "Share ratio" } else { "Progress" }),
                )
                .child(task_status_badge(task.status, task.status.label(), colors)),
        )
        .child(
            div()
                .flex()
                .items_baseline()
                .justify_between()
                .font_features(tabular_numbers())
                .child(
                    div()
                        .text_size(px(24.0))
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(if seeding {
                            format_share_ratio(task.share_ratio_milli())
                        } else {
                            format_percent(basis_points)
                        }),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(colors.text_muted)
                        .child(eta_label),
                ),
        )
        .child(
            task_progress_bar_with_id(
                SharedString::from(format!("task-overview-progress-{}", task.identity.gid)),
                progress,
                task.status,
                colors,
            )
            .h(px(5.0)),
        )
        .child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .font_features(tabular_numbers())
                .text_xs()
                .text_color(colors.text_muted)
                .child(if seeding {
                    format!("Uploaded {}", format_bytes(task.uploaded_bytes))
                } else {
                    size_label
                })
                .child(if seeding {
                    format!("Up {}", format_rate(task.upload_rate))
                } else {
                    format_rate(task.download_rate)
                }),
        )
        .when_some(error_label, |element, error| {
            element.child(
                div()
                    .id(error_id)
                    .role(Role::Alert)
                    .text_xs()
                    .text_color(colors.danger)
                    .child(error),
            )
        })
}

pub(crate) fn drawer_message(
    title: impl Into<SharedString>,
    detail: impl Into<SharedString>,
    colors: crate::ThemeColors,
) -> AnyElement {
    let title = title.into();
    let detail = detail.into();
    div()
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
            div()
                .text_sm()
                .font_weight(FontWeight::SEMIBOLD)
                .child(title),
        )
        .child(div().text_xs().text_color(colors.text_muted).child(detail))
        .into_any_element()
}

pub(crate) fn detail_line(
    label: impl Into<SharedString>,
    value: impl Into<SharedString>,
    colors: crate::ThemeColors,
) -> Div {
    let label = label.into();
    div()
        .flex()
        .items_start()
        .gap_3()
        .child(
            div()
                .w(px(76.0))
                .flex_none()
                .text_xs()
                .text_color(colors.text_muted)
                .child(label),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .text_xs()
                .text_color(colors.text_secondary)
                .child(value.into()),
        )
}

pub(crate) fn detail_line_with_action(
    label: impl Into<SharedString>,
    value: impl Into<SharedString>,
    action: impl IntoElement,
    colors: crate::ThemeColors,
) -> Div {
    let label = label.into();
    div()
        .flex()
        .items_center()
        .gap_3()
        .child(
            div()
                .w(px(76.0))
                .flex_none()
                .text_xs()
                .text_color(colors.text_muted)
                .child(label),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .font_family("monospace")
                .text_xs()
                .text_color(colors.text_secondary)
                .child(value.into()),
        )
        .child(action)
}

pub(crate) fn detail_collection_section(
    title: impl Into<SharedString>,
    empty_message: impl Into<SharedString>,
    rows: Vec<AnyElement>,
    colors: crate::ThemeColors,
) -> Div {
    let title = title.into();
    let empty_message = empty_message.into();
    let count = rows.len();
    div()
        .flex()
        .flex_col()
        .gap_2()
        .child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .child(
                    div()
                        .text_xs()
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(colors.text_secondary)
                        .child(title),
                )
                .child(
                    div()
                        .font_features(tabular_numbers())
                        .text_xs()
                        .text_color(colors.text_muted)
                        .child(count.to_string()),
                ),
        )
        .when(count == 0, |element| {
            element.child(
                div()
                    .rounded_md()
                    .border_1()
                    .border_color(colors.border)
                    .bg(colors.elevated_surface)
                    .p_3()
                    .text_xs()
                    .text_color(colors.text_muted)
                    .child(empty_message),
            )
        })
        .when(count != 0, |element| {
            element.child(div().flex().flex_col().gap_2().children(rows))
        })
}

pub(crate) fn detail_collection_row(
    primary: impl Into<SharedString>,
    secondary: impl Into<SharedString>,
    badge: Option<String>,
    colors: crate::ThemeColors,
) -> AnyElement {
    div()
        .rounded_md()
        .border_1()
        .border_color(colors.border)
        .bg(colors.elevated_surface)
        .p_3()
        .flex()
        .items_start()
        .gap_2()
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
                        .text_color(colors.text_secondary)
                        .font_family("monospace")
                        .child(primary.into()),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(colors.text_muted)
                        .child(secondary.into()),
                ),
        )
        .when_some(badge, |element, badge| {
            element.child(
                div()
                    .flex_none()
                    .rounded_full()
                    .bg(colors.surface_active)
                    .px_2()
                    .py_0p5()
                    .text_xs()
                    .text_color(colors.text_muted)
                    .child(badge),
            )
        })
        .into_any_element()
}

pub(crate) fn render_file_row(
    gid: &str,
    index: usize,
    file: TaskFileView,
    file_count: usize,
    aria_label: String,
    colors: crate::ThemeColors,
) -> Stateful<Div> {
    let basis_points = if file.length == 0 {
        None
    } else {
        let completed = u128::from(file.completed_length.min(file.length));
        Some(((completed * 10_000) / u128::from(file.length)) as u16)
    };
    let stable_id = SharedString::from(format!("task-file:{gid}:{}", file.index));
    div()
        .id(stable_id)
        .role(Role::ListItem)
        .aria_position_in_set(index + 1)
        .aria_size_of_set(file_count)
        .aria_label(aria_label)
        .h(px(52.0))
        .w_full()
        .flex_none()
        .flex()
        .items_center()
        .gap_3()
        .px_4()
        .child(
            div()
                .w(px(18.0))
                .flex_none()
                .flex()
                .items_center()
                .justify_center()
                .child(
                    Icon::new(if file.selected {
                        IconName::CircleCheck
                    } else {
                        IconName::CircleX
                    })
                    .size(IconSize::Small)
                    .color(if file.selected {
                        colors.success
                    } else {
                        colors.text_muted
                    }),
                ),
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
                .w(px(78.0))
                .flex_none()
                .text_right()
                .font_features(tabular_numbers())
                .text_xs()
                .text_color(colors.text_muted)
                .child(format_percent(basis_points)),
        )
}

pub(crate) fn output_name_validation_error(output_name: &str) -> Option<&'static str> {
    if output_name.is_empty() {
        Some("notice-filename-empty")
    } else if output_name == "." || output_name == ".." {
        Some("notice-filename-dot")
    } else if output_name.contains(['/', '\\', '\0']) {
        Some("notice-filename-separators")
    } else {
        None
    }
}

pub(crate) fn stale_session_error() -> OperationErrorView {
    OperationErrorView {
        code: "command.stale_session".into(),
        summary: "The aria2 session changed. Review current state before submitting again.".into(),
        retryable: false,
    }
}

pub(crate) fn tabular_numbers() -> FontFeatures {
    FontFeatures(Arc::new(vec![("tnum".into(), 1)]))
}

pub(crate) fn connection_color(connection: &ConnectionView, colors: crate::ThemeColors) -> Hsla {
    match connection {
        ConnectionView::Connected => colors.success,
        ConnectionView::Failed { .. } => colors.danger,
        ConnectionView::Disconnected => colors.text_muted,
        ConnectionView::Connecting
        | ConnectionView::Authenticating
        | ConnectionView::Synchronizing
        | ConnectionView::Reconnecting { .. } => colors.information,
    }
}

pub(crate) fn engine_health_color(health: &EngineHealthView, colors: crate::ThemeColors) -> Hsla {
    match health {
        EngineHealthView::External => colors.information,
        EngineHealthView::Running { restarts: 0 } => colors.success,
        EngineHealthView::Running { .. } | EngineHealthView::Restarting { .. } => colors.warning,
        EngineHealthView::Failed { .. } => colors.danger,
    }
}

pub(crate) fn task_status_color(status: TaskStatusView, colors: crate::ThemeColors) -> Hsla {
    match status {
        TaskStatusView::Active => colors.accent,
        TaskStatusView::Seeding => colors.progress_upload,
        TaskStatusView::Waiting | TaskStatusView::Paused => colors.warning,
        TaskStatusView::Complete => colors.success,
        TaskStatusView::Failed | TaskStatusView::Removed => colors.danger,
        TaskStatusView::Verifying => colors.information,
        TaskStatusView::Unknown => colors.text_muted,
    }
}

pub(crate) fn task_progress_bar_with_id(
    id: impl Into<gpui::ElementId>,
    progress: f32,
    status: TaskStatusView,
    colors: crate::ThemeColors,
) -> KitProgress {
    let fill = match status {
        TaskStatusView::Failed | TaskStatusView::Removed => colors.danger,
        TaskStatusView::Complete => colors.success,
        TaskStatusView::Seeding => colors.progress_upload,
        _ => colors.progress_download,
    };
    let percent = (progress.clamp(0.0, 1.0)) * 100.0;
    KitProgress::new(id)
        .value(percent)
        .xsmall()
        .color(fill)
        .loading(status == TaskStatusView::Verifying && percent <= 0.0)
}

pub(crate) fn task_table_value(width: f32, value: String, colors: crate::ThemeColors) -> Div {
    div()
        .w(px(width))
        .flex_none()
        .truncate()
        .font_features(tabular_numbers())
        .text_xs()
        .text_color(colors.text_secondary)
        .child(value)
}

pub(crate) fn task_status_badge(
    status: TaskStatusView,
    label: impl Into<SharedString>,
    colors: crate::ThemeColors,
) -> Div {
    let color = task_status_color(status, colors);
    // Icon + text so status is never color-only (ACCESS-001).
    // Visible label is enough for SR; parent task row already has a full aria_label.
    div()
        .h(px(22.0))
        .min_w(px(64.0))
        .max_w_full()
        .px_2()
        .flex()
        .items_center()
        .justify_center()
        .gap_1()
        .rounded_sm()
        .border_1()
        .border_color(with_alpha(color, 0.28))
        .bg(with_alpha(color, 0.1))
        .text_size(px(11.0))
        .font_weight(FontWeight::MEDIUM)
        .text_color(color)
        .child(
            Icon::new(task_status_icon(status))
                .size(IconSize::XSmall)
                .color(color),
        )
        .child(label.into())
}

pub(crate) fn with_alpha(mut color: Hsla, alpha: f32) -> Hsla {
    color.a = alpha;
    color
}

/// Owned-string variant of [`settings_row`] for translated labels.
pub(crate) fn settings_row_owned(
    label: impl Into<SharedString>,
    description: Option<impl Into<SharedString>>,
    control: impl IntoElement,
    colors: crate::ThemeColors,
) -> Div {
    let label = label.into();
    let description = description.map(Into::into);
    div()
        .flex()
        .items_center()
        .gap_4()
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .gap_0p5()
                .child(div().text_sm().text_color(colors.text_primary).child(label))
                .when_some(description, |col, desc| {
                    col.child(div().text_xs().text_color(colors.text_muted).child(desc))
                }),
        )
        .child(div().flex_none().child(control))
}

/// Read-only label + value row for About and other non-editable info panels.
pub(crate) fn settings_info_row_owned(
    label: impl Into<SharedString>,
    value: impl Into<SharedString>,
    colors: crate::ThemeColors,
) -> Div {
    let label = label.into();
    let value = value.into();
    div()
        .flex()
        .items_start()
        .gap_4()
        .child(
            div()
                .flex_1()
                .min_w_0()
                .text_sm()
                .text_color(colors.text_primary)
                .child(label),
        )
        .child(
            div()
                .flex_none()
                .max_w(px(320.0))
                .text_sm()
                .text_color(colors.text_secondary)
                .child(value),
        )
}

pub(crate) fn category_icon(name: &str) -> IconName {
    match name.to_ascii_lowercase().as_str() {
        "compressed" | "压缩包" => IconName::FolderArchive,
        "documents" | "文档" => IconName::FileText,
        "music" | "音乐" => IconName::Music,
        "video" | "视频" => IconName::Video,
        "programs" | "程序" => IconName::AppWindow,
        "images" | "图片" => IconName::Image,
        _ => IconName::FolderDown,
    }
}

pub(crate) fn file_type_icon(filename: &str) -> IconName {
    let ext = filename
        .rsplit_once('.')
        .map(|(_, ext)| ext)
        .unwrap_or_default();
    if ext.eq_ignore_ascii_case("zip")
        || ext.eq_ignore_ascii_case("tar")
        || ext.eq_ignore_ascii_case("gz")
        || ext.eq_ignore_ascii_case("7z")
        || ext.eq_ignore_ascii_case("rar")
        || ext.eq_ignore_ascii_case("bz2")
        || ext.eq_ignore_ascii_case("xz")
        || ext.eq_ignore_ascii_case("zst")
        || ext.eq_ignore_ascii_case("iso")
        || ext.eq_ignore_ascii_case("cab")
        || ext.eq_ignore_ascii_case("dmg")
        || ext.eq_ignore_ascii_case("torrent")
    {
        IconName::FolderArchive
    } else if ext.eq_ignore_ascii_case("mp4")
        || ext.eq_ignore_ascii_case("mkv")
        || ext.eq_ignore_ascii_case("avi")
        || ext.eq_ignore_ascii_case("mov")
        || ext.eq_ignore_ascii_case("wmv")
        || ext.eq_ignore_ascii_case("flv")
        || ext.eq_ignore_ascii_case("webm")
        || ext.eq_ignore_ascii_case("m4v")
        || ext.eq_ignore_ascii_case("rmvb")
    {
        IconName::Video
    } else if ext.eq_ignore_ascii_case("mp3")
        || ext.eq_ignore_ascii_case("flac")
        || ext.eq_ignore_ascii_case("wav")
        || ext.eq_ignore_ascii_case("aac")
        || ext.eq_ignore_ascii_case("ogg")
        || ext.eq_ignore_ascii_case("m4a")
        || ext.eq_ignore_ascii_case("wma")
        || ext.eq_ignore_ascii_case("ape")
    {
        IconName::Music
    } else if ext.eq_ignore_ascii_case("pdf")
        || ext.eq_ignore_ascii_case("doc")
        || ext.eq_ignore_ascii_case("docx")
        || ext.eq_ignore_ascii_case("txt")
        || ext.eq_ignore_ascii_case("md")
        || ext.eq_ignore_ascii_case("epub")
        || ext.eq_ignore_ascii_case("mobi")
        || ext.eq_ignore_ascii_case("ppt")
        || ext.eq_ignore_ascii_case("pptx")
        || ext.eq_ignore_ascii_case("xls")
        || ext.eq_ignore_ascii_case("xlsx")
        || ext.eq_ignore_ascii_case("csv")
    {
        IconName::FileText
    } else if ext.eq_ignore_ascii_case("exe")
        || ext.eq_ignore_ascii_case("msi")
        || ext.eq_ignore_ascii_case("pkg")
        || ext.eq_ignore_ascii_case("deb")
        || ext.eq_ignore_ascii_case("rpm")
        || ext.eq_ignore_ascii_case("apk")
        || ext.eq_ignore_ascii_case("appimage")
    {
        IconName::AppWindow
    } else if ext.eq_ignore_ascii_case("jpg")
        || ext.eq_ignore_ascii_case("jpeg")
        || ext.eq_ignore_ascii_case("png")
        || ext.eq_ignore_ascii_case("gif")
        || ext.eq_ignore_ascii_case("webp")
        || ext.eq_ignore_ascii_case("svg")
        || ext.eq_ignore_ascii_case("bmp")
        || ext.eq_ignore_ascii_case("ico")
        || ext.eq_ignore_ascii_case("tiff")
    {
        IconName::Image
    } else {
        IconName::File
    }
}

pub(crate) fn category_display_name(name: &str, shell: &AppShell) -> String {
    match name.to_ascii_lowercase().as_str() {
        "general" => shell.t("category-general"),
        "compressed" => shell.t("category-compressed"),
        "documents" => shell.t("category-documents"),
        "music" => shell.t("category-music"),
        "video" => shell.t("category-video"),
        "programs" => shell.t("category-programs"),
        "images" => shell.t("category-images"),
        _ => name.to_owned(),
    }
}
