//! Free functions and small UI helpers used by AppShell render/logic paths.

use super::*;
// Re-exported via parent `use helpers::*` for AppShell and tests.

pub(crate) fn titlebar_drag_region() -> Div {
    let region = div().flex_1().min_w_0().h_full();
    #[cfg(any(target_os = "windows", target_os = "macos"))]
    {
        region.window_control_area(WindowControlArea::Drag)
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        region
    }
}

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

#[cfg(target_os = "windows")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum WindowControlKind {
    Minimize,
    Maximize,
    Close,
}

#[cfg(target_os = "windows")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct WindowControlConfig {
    pub(crate) id: &'static str,
    pub(crate) icon: IconName,
    pub(crate) label: &'static str,
    pub(crate) area: WindowControlArea,
    pub(crate) danger: bool,
}

#[cfg(target_os = "windows")]
pub(crate) fn window_control_config(
    kind: WindowControlKind,
    maximized: bool,
) -> WindowControlConfig {
    match kind {
        WindowControlKind::Minimize => WindowControlConfig {
            id: "window-minimize",
            icon: IconName::WindowMinimize,
            label: "Minimize window",
            area: WindowControlArea::Min,
            danger: false,
        },
        WindowControlKind::Maximize => WindowControlConfig {
            id: "window-maximize",
            icon: if maximized {
                IconName::WindowRestore
            } else {
                IconName::WindowMaximize
            },
            label: if maximized {
                "Restore window"
            } else {
                "Maximize window"
            },
            area: WindowControlArea::Max,
            danger: false,
        },
        WindowControlKind::Close => WindowControlConfig {
            id: "window-close",
            icon: IconName::WindowClose,
            label: "Close window",
            area: WindowControlArea::Close,
            danger: true,
        },
    }
}

/// Windows 11 caption button width (WinUI AppWindow title bar convention).
#[cfg(target_os = "windows")]
pub(crate) const WINDOW_CONTROL_WIDTH: f32 = 46.0;

#[cfg(target_os = "windows")]
pub(crate) fn window_control_button(
    id: &'static str,
    icon: IconName,
    label: &'static str,
    area: WindowControlArea,
    colors: crate::ThemeColors,
    danger: bool,
) -> Stateful<Div> {
    // Caption glyphs must set Icon.color explicitly: embedded SVGs use a fixed
    // stroke and do not inherit the parent's text_color, so omitting color made
    // them render black on the dark titlebar (invisible).
    let idle = colors.text_primary;
    let hover_bg = if danger {
        colors.danger
    } else {
        colors.surface_hover
    };
    let active_bg = if danger {
        colors.danger
    } else {
        colors.surface_active
    };
    // Fluent Close: red fill + light glyph. Other captions keep the idle glyph
    // color while the fill changes (GPUI recolors SVG strokes via text_color).
    let hover_fg = if danger {
        Theme::light().colors.text_inverse
    } else {
        idle
    };
    div()
        .id(id)
        .role(Role::Button)
        .aria_label(label)
        .window_control_area(area)
        .focusable()
        .tab_stop(true)
        .h(px(TITLEBAR_HEIGHT))
        .w(px(WINDOW_CONTROL_WIDTH))
        .min_w(px(WINDOW_CONTROL_WIDTH))
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .rounded_none()
        .cursor_pointer()
        .text_color(idle)
        .hover(move |style| style.bg(hover_bg).text_color(hover_fg))
        .active(move |style| style.bg(active_bg).text_color(hover_fg))
        .tooltip(Tooltip::text(label, None, colors))
        // Match toolbar IconButton optical size (Medium / 16px) so caption
        // glyphs and chrome actions feel consistent.
        .child(Icon::new(icon).size(IconSize::Medium).color(idle))
}

pub(crate) fn speed_chart_column(
    download_height: f32,
    upload_height: f32,
    colors: crate::ThemeColors,
) -> Div {
    div()
        .h_full()
        .flex_1()
        .min_w(px(1.0))
        .flex()
        .items_end()
        .child(
            div()
                .w(relative(0.5))
                .h(relative(download_height.clamp(0.0, 1.0)))
                .bg(colors.progress_download),
        )
        .child(
            div()
                .w(relative(0.5))
                .h(relative(upload_height.clamp(0.0, 1.0)))
                .bg(colors.progress_upload),
        )
}

pub(crate) fn speed_chart_window(history: &[SpeedSampleView]) -> &[SpeedSampleView] {
    if history.len() > SPEED_CHART_SAMPLES {
        &history[history.len() - SPEED_CHART_SAMPLES..]
    } else {
        history
    }
}

pub(crate) fn speed_chart_legend(
    label: &'static str,
    color: Hsla,
    colors: crate::ThemeColors,
) -> Div {
    div()
        .flex()
        .items_center()
        .gap_1()
        .child(div().size(px(6.0)).rounded_sm().bg(color))
        .child(div().text_color(colors.text_muted).child(label))
}

pub(crate) fn toolbar_icon_button(
    id: &'static str,
    icon: IconName,
    label: &'static str,
    state: ToolbarButtonState,
    danger: bool,
    shortcut: Option<&'static str>,
    colors: crate::ThemeColors,
) -> Stateful<Div> {
    let enabled = state == ToolbarButtonState::Enabled;
    let loading = state == ToolbarButtonState::Loading;
    let tooltip = shortcut.map_or_else(
        || Tooltip::new(label),
        |shortcut| Tooltip::new(label).meta(shortcut),
    );
    IconButton::new(id, icon)
        .aria_label(if enabled || loading {
            label.to_owned()
        } else {
            format!("{label} unavailable")
        })
        .style(if danger {
            ButtonStyle::Danger
        } else {
            ButtonStyle::Ghost
        })
        .disabled(!enabled && !loading)
        .loading(loading)
        .tooltip(tooltip)
        .render(colors)
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn queue_move_button(
    id: &'static str,
    icon: IconName,
    label: &'static str,
    command: TaskCommandView,
    enabled: bool,
    pending_command: Option<&TaskCommandView>,
    shortcut: Option<&'static str>,
    colors: crate::ThemeColors,
    cx: &mut Context<AppShell>,
) -> Stateful<Div> {
    let loading = pending_command == Some(&command);
    toolbar_icon_button(
        id,
        icon,
        label,
        ToolbarButtonState::from_flags(enabled, loading),
        false,
        shortcut,
        colors,
    )
    .when(enabled, move |button| {
        button.on_click(cx.listener(move |this, _, _window, cx| {
            this.begin_task_command(command.clone(), cx);
        }))
    })
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum TextFieldContextAction {
    Cut,
    Copy,
    Paste,
    SelectAll,
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

pub(crate) fn context_menu_item(
    action: ContextMenuAction,
    label: &'static str,
    shortcut: Option<&'static str>,
    enabled: bool,
    destructive: bool,
    colors: crate::ThemeColors,
    cx: &mut Context<AppShell>,
) -> AnyElement {
    div()
        .id(SharedString::from(format!("ctx-menu-{label}")))
        .role(Role::MenuItem)
        .aria_label(label)
        .w_full()
        .h(px(32.0))
        .px_3()
        .flex()
        .items_center()
        .justify_between()
        .gap_3()
        .rounded_sm()
        .cursor_pointer()
        .text_sm()
        .text_color(if !enabled {
            colors.text_muted
        } else if destructive {
            colors.danger
        } else {
            colors.text_primary
        })
        .when(enabled, |element| {
            element
                .hover(|style| style.bg(colors.surface_hover))
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.activate_context_menu_action(action, window, cx);
                }))
        })
        .child(label)
        .when_some(shortcut, |element, shortcut| {
            element.child(
                div()
                    .text_xs()
                    .text_color(colors.text_muted)
                    .child(shortcut),
            )
        })
        .into_any_element()
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

pub(crate) fn render_vertical_scrollbar(
    scroll: &ScrollHandle,
    colors: crate::ThemeColors,
) -> AnyElement {
    let bounds = scroll.bounds();
    let max_offset = scroll.max_offset();
    let offset = scroll.offset();
    let viewport = bounds.size.height;
    let content = viewport + max_offset.y;
    if max_offset.y <= px(0.0) || viewport <= px(0.0) || content <= px(0.0) {
        return div().w(px(10.0)).flex_none().into_any_element();
    }
    let thumb_ratio = (viewport / content).clamp(0.12, 1.0);
    let thumb_height = (viewport * thumb_ratio).max(px(28.0));
    let travel = (viewport - thumb_height).max(px(0.0));
    let progress = if max_offset.y.as_f32().abs() < f32::EPSILON {
        0.0
    } else {
        ((-offset.y) / max_offset.y).clamp(0.0, 1.0)
    };
    let thumb_top = travel * progress;
    let handle = scroll.clone();
    let max_y = max_offset.y;
    div()
        .id("vertical-scrollbar-track")
        .w(px(10.0))
        .flex_none()
        .h_full()
        .py_1()
        .pr_1()
        .child(
            div()
                .id("vertical-scrollbar-rail")
                .relative()
                .w(px(6.0))
                .h_full()
                .rounded_full()
                .bg(with_alpha(colors.border, 0.35))
                .child(
                    div()
                        .id("vertical-scrollbar-thumb")
                        .absolute()
                        .top(thumb_top)
                        .left_0()
                        .right_0()
                        .h(thumb_height)
                        .rounded_full()
                        .bg(with_alpha(colors.text_muted, 0.55))
                        .hover(|style| style.bg(with_alpha(colors.text_muted, 0.8)))
                        .on_mouse_down(MouseButton::Left, move |event: &MouseDownEvent, _, cx| {
                            cx.stop_propagation();
                            let track_y = event.position.y - bounds.origin.y;
                            let ratio = (track_y / viewport).clamp(0.0, 1.0);
                            let y = -(max_y * ratio);
                            handle.set_offset(point(px(0.0), y));
                        }),
                ),
        )
        .into_any_element()
}

pub(crate) fn settings_input_config(
    element_id: &'static str,
    accessibility_label: &'static str,
    placeholder: &'static str,
    leading_icon: Option<IconName>,
    secure: bool,
) -> TextFieldConfig {
    TextFieldConfig {
        element_id: element_id.into(),
        key_context: TEXT_FIELD_KEY_CONTEXT.into(),
        role: Role::TextInput,
        accessibility_label: accessibility_label.into(),
        placeholder: placeholder.into(),
        leading_icon,
        clearable: !secure,
        allow_newlines: false,
        secure,
    }
}

pub(crate) fn settings_labeled_input(
    label: &'static str,
    input: Entity<TextField>,
    colors: crate::ThemeColors,
) -> Div {
    div()
        .flex()
        .flex_col()
        .gap_1()
        .child(div().text_xs().text_color(colors.text_muted).child(label))
        .child(input)
}

pub(crate) fn settings_path_field_row(
    input: Entity<TextField>,
    browse_id: &'static str,
    browse_label: &'static str,
    browse_aria: &'static str,
    target: PathPickTarget,
    colors: crate::ThemeColors,
    cx: &mut Context<AppShell>,
) -> Div {
    div()
        .flex()
        .items_center()
        .gap_2()
        .child(div().flex_1().min_w_0().child(input))
        .child(
            Button::new(browse_id, browse_label)
                .icon(IconName::FolderDown)
                .aria_label(browse_aria)
                .style(ButtonStyle::Secondary)
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.pick_path_for_field(target, window, cx);
                }))
                .render(colors),
        )
}

/// A titled card used as the visual container for one settings group.
/// Children are appended with `.child()` on the returned `Div`.
pub(crate) fn settings_card(title: &'static str, colors: crate::ThemeColors) -> Div {
    div()
        .flex()
        .flex_col()
        .gap_3()
        .px_4()
        .py_4()
        .rounded_lg()
        .border_1()
        .border_color(colors.border)
        .bg(colors.elevated_surface)
        .child(
            div()
                .text_sm()
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(colors.text_primary)
                .child(title),
        )
}

/// A settings row with a label (+ optional muted description) on the left
/// and an arbitrary control (`toggle`, `Button`, `SegmentedControl`, …) on the right.
pub(crate) fn settings_row(
    label: &'static str,
    description: Option<&'static str>,
    control: impl IntoElement,
    colors: crate::ThemeColors,
) -> Div {
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

pub(crate) fn metadata_selection_summary(preview: &AddDownloadMetadataPreviewView) -> String {
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
    let count = preview.selected_file_indices.len();
    let total = preview.files.len();
    if unknown_sizes == 0 {
        format!(
            "{count} of {total} selected · {}",
            format_bytes(known_bytes)
        )
    } else {
        format!(
            "{count} of {total} selected · {} + {unknown_sizes} unknown",
            format_bytes(known_bytes)
        )
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
                .child(task_status_badge(task.status, colors)),
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
        .child(task_progress_bar(progress, task.status, colors).h(px(5.0)))
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
    title: &'static str,
    detail: &'static str,
    colors: crate::ThemeColors,
) -> AnyElement {
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
    label: &'static str,
    value: impl Into<SharedString>,
    colors: crate::ThemeColors,
) -> Div {
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
    label: &'static str,
    value: impl Into<SharedString>,
    action: impl IntoElement,
    colors: crate::ThemeColors,
) -> Div {
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
    title: &'static str,
    empty_message: &'static str,
    rows: Vec<AnyElement>,
    colors: crate::ThemeColors,
) -> Div {
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
    badge: Option<&'static str>,
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

pub(crate) fn render_task_uri(source: TaskUriView, colors: crate::ThemeColors) -> AnyElement {
    detail_collection_row(source.uri, source.status.label(), None, colors)
}

pub(crate) fn render_task_tracker(
    tracker: TaskTrackerView,
    colors: crate::ThemeColors,
) -> AnyElement {
    detail_collection_row(
        tracker.uri,
        format!("Announce tier {}", tracker.tier),
        None,
        colors,
    )
}

pub(crate) fn render_task_server(server: TaskServerView, colors: crate::ThemeColors) -> AnyElement {
    let current_uri = if server.current_uri.is_empty() {
        server.uri.clone()
    } else {
        server.current_uri.clone()
    };
    let secondary = if server.uri.is_empty() || server.uri == current_uri {
        format!(
            "File {} · Download {}",
            server.file_index,
            format_rate(server.download_rate)
        )
    } else {
        format!(
            "From {} · File {} · Download {}",
            server.uri,
            server.file_index,
            format_rate(server.download_rate)
        )
    };
    detail_collection_row(current_uri, secondary, None, colors)
}

pub(crate) fn render_task_peer(peer: TaskPeerView, colors: crate::ThemeColors) -> AnyElement {
    let address = if peer.address.contains(':') {
        format!("[{}]:{}", peer.address, peer.port)
    } else {
        format!("{}:{}", peer.address, peer.port)
    };
    detail_collection_row(
        address,
        format!(
            "Down {} · Up {}",
            format_rate(peer.download_rate),
            format_rate(peer.upload_rate)
        ),
        peer.seeder.then_some("Seed"),
        colors,
    )
}

pub(crate) fn render_task_option(option: TaskOptionView, colors: crate::ThemeColors) -> AnyElement {
    let value = if option.redacted {
        "Hidden".to_owned()
    } else if option.value.is_empty() {
        "Empty".to_owned()
    } else {
        option.value
    };
    detail_collection_row(
        option.key,
        value,
        option.redacted.then_some("Sensitive"),
        colors,
    )
}

pub(crate) fn format_seed_stop_rules(options: &[TaskOptionView]) -> String {
    let value = |key: &str| {
        options
            .iter()
            .find(|option| option.key.eq_ignore_ascii_case(key))
            .map(|option| option.value.as_str())
            .unwrap_or("not reported")
    };
    let ratio = value("seed-ratio");
    let ratio = if ratio.parse::<f64>().is_ok_and(|value| value == 0.0) {
        "ratio disabled (0.0)".to_owned()
    } else {
        format!("ratio {ratio}")
    };
    format!(
        "Stops at the first reached limit: {ratio} · time {} min",
        value("seed-time")
    )
}

pub(crate) fn render_file_row(
    gid: &str,
    index: usize,
    file: TaskFileView,
    file_count: usize,
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
        .aria_label(format!(
            "{}, {}, {}, {}",
            file.path,
            if file.selected { "enabled" } else { "skipped" },
            format_bytes(file.length),
            format_percent(basis_points)
        ))
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

pub(crate) fn task_command_label(command: &TaskCommandView) -> &'static str {
    match command {
        TaskCommandView::Pause => "Pause",
        TaskCommandView::ForcePause => "Force pause",
        TaskCommandView::Resume => "Resume",
        TaskCommandView::MoveToQueueTop => "Move to top",
        TaskCommandView::MoveUpInQueue => "Move up",
        TaskCommandView::MoveDownInQueue => "Move down",
        TaskCommandView::MoveToQueueBottom => "Move to bottom",
        TaskCommandView::Retry => "Retry",
        TaskCommandView::SetOutputName { .. } => "Change output name",
        TaskCommandView::SetSpeedLimit { .. } => "Set speed limits",
        TaskCommandView::SetConnectionPolicy { .. } => "Set connection policy",
        TaskCommandView::SetOptions { .. } => "Edit task options",
        TaskCommandView::RemoveTask | TaskCommandView::RemoveTaskAndFiles => "Remove",
        TaskCommandView::ForceRemoveTask => "Force remove",
    }
}

pub(crate) fn output_name_validation_error(output_name: &str) -> Option<&'static str> {
    if output_name.is_empty() {
        Some("Enter a filename.")
    } else if output_name == "." || output_name == ".." {
        Some("A filename cannot be '.' or '..'.")
    } else if output_name.contains(['/', '\\', '\0']) {
        Some("Use a filename without path separators.")
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

pub(crate) fn task_progress_bar(
    progress: f32,
    status: TaskStatusView,
    colors: crate::ThemeColors,
) -> Div {
    let fill = match status {
        TaskStatusView::Failed | TaskStatusView::Removed => colors.danger,
        TaskStatusView::Complete => colors.success,
        TaskStatusView::Seeding => colors.progress_upload,
        _ => colors.progress_download,
    };
    div()
        .h(px(4.0))
        .w_full()
        .rounded_full()
        .overflow_hidden()
        .bg(colors.progress_track)
        .child(
            div()
                .h_full()
                .w(relative(progress.clamp(0.0, 1.0)))
                .rounded_full()
                .bg(fill),
        )
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

pub(crate) fn task_status_badge(status: TaskStatusView, colors: crate::ThemeColors) -> Div {
    let color = task_status_color(status, colors);
    div()
        .h(px(22.0))
        .max_w_full()
        .px_2()
        .flex()
        .items_center()
        .justify_center()
        .rounded_sm()
        .border_1()
        .border_color(with_alpha(color, 0.28))
        .bg(with_alpha(color, 0.1))
        .text_size(px(11.0))
        .font_weight(FontWeight::MEDIUM)
        .text_color(color)
        .child(status.label())
}

pub(crate) fn with_alpha(mut color: Hsla, alpha: f32) -> Hsla {
    color.a = alpha;
    color
}
