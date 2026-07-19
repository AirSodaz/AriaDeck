use std::{ops::Range, sync::Arc};

use gpui::{
    AnyElement, App, Context, Div, Entity, FocusHandle, Focusable, FontFeatures, FontWeight, Hsla,
    IntoElement, Render, Role, ScrollStrategy, SharedString, Stateful, Subscription,
    UniformListScrollHandle, Window, div, prelude::*, px, relative, uniform_list,
};

use crate::{
    ClearSearch, ConnectionView, DownloadRowView, FocusNext, FocusPrevious, FocusSearch,
    SearchInput, SearchInputEvent, SelectNextTask, SelectPreviousTask, TaskIdentity,
    TaskStatusView, Theme, ThemeMode, ToggleTheme, WorkspaceFilter, WorkspaceQuery,
    WorkspaceSnapshot, format_bytes, format_eta, format_percent, format_rate,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AppShellEvent {
    QueryChanged(WorkspaceQuery),
    RetryRequested,
}

pub struct AppShell {
    theme: Theme,
    snapshot: WorkspaceSnapshot,
    query: WorkspaceQuery,
    selected: Option<TaskIdentity>,
    search_input: Entity<SearchInput>,
    list_scroll: UniformListScrollHandle,
    focus_handle: FocusHandle,
    rendered_range: Range<usize>,
    _search_subscription: Subscription,
}

impl gpui::EventEmitter<AppShellEvent> for AppShell {}

impl AppShell {
    #[must_use]
    pub fn new(theme: Theme, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let search_input = cx.new(|cx| SearchInput::new("Search downloads or GID", theme, cx));
        let search_subscription = cx.subscribe(
            &search_input,
            |this: &mut Self, _input, event: &SearchInputEvent, cx| {
                if this.query.search != event.text {
                    this.query.search.clone_from(&event.text);
                    this.emit_query(cx);
                }
            },
        );
        let focus_handle = cx.focus_handle();
        window.focus(&focus_handle, cx);
        Self {
            theme,
            snapshot: WorkspaceSnapshot::default(),
            query: WorkspaceQuery::default(),
            selected: None,
            search_input,
            list_scroll: UniformListScrollHandle::new(),
            focus_handle,
            rendered_range: 0..0,
            _search_subscription: search_subscription,
        }
    }

    pub fn set_snapshot(&mut self, snapshot: WorkspaceSnapshot, cx: &mut Context<Self>) {
        if self
            .selected
            .as_ref()
            .is_some_and(|selected| selected.profile_id != snapshot.profile_id)
        {
            self.selected = None;
        }
        self.snapshot = snapshot;
        cx.notify();
    }

    #[must_use]
    pub fn query(&self) -> WorkspaceQuery {
        self.query.clone()
    }

    #[must_use]
    pub fn selected_identity(&self) -> Option<&TaskIdentity> {
        self.selected.as_ref()
    }

    #[must_use]
    pub fn rendered_range(&self) -> Range<usize> {
        self.rendered_range.clone()
    }

    fn emit_query(&self, cx: &mut Context<Self>) {
        cx.emit(AppShellEvent::QueryChanged(self.query.clone()));
        cx.notify();
    }

    fn set_filter(
        &mut self,
        filter: WorkspaceFilter,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.query.filter == filter {
            return;
        }
        self.query.filter = filter;
        self.list_scroll
            .scroll_to_item_strict(0, ScrollStrategy::Top);
        self.emit_query(cx);
    }

    fn focus_search(&mut self, _: &FocusSearch, window: &mut Window, cx: &mut Context<Self>) {
        window.focus(&self.search_input.focus_handle(cx), cx);
    }

    fn clear_search(&mut self, _: &ClearSearch, window: &mut Window, cx: &mut Context<Self>) {
        if self.search_input.read(cx).text().is_empty() {
            window.focus(&self.focus_handle, cx);
        } else {
            self.search_input
                .update(cx, |input, cx| input.set_text("", cx));
        }
    }

    fn select_next(&mut self, _: &SelectNextTask, window: &mut Window, cx: &mut Context<Self>) {
        if self.snapshot.tasks.is_empty() {
            return;
        }
        let current = self.selected_index().unwrap_or(0);
        let next = if self.selected.is_none() {
            0
        } else {
            (current + 1).min(self.snapshot.tasks.len() - 1)
        };
        self.select_at(next, window, cx);
    }

    fn select_previous(
        &mut self,
        _: &SelectPreviousTask,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.snapshot.tasks.is_empty() {
            return;
        }
        let previous = self.selected_index().unwrap_or(0).saturating_sub(1);
        self.select_at(previous, window, cx);
    }

    fn select_at(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(task) = self.snapshot.tasks.get(index) else {
            return;
        };
        self.selected = Some(task.identity.clone());
        self.list_scroll
            .scroll_to_item(index, ScrollStrategy::Nearest);
        window.focus(&self.focus_handle, cx);
        cx.notify();
    }

    fn selected_index(&self) -> Option<usize> {
        let selected = self.selected.as_ref()?;
        self.snapshot
            .tasks
            .iter()
            .position(|task| &task.identity == selected)
    }

    fn toggle_theme(&mut self, _: &ToggleTheme, _window: &mut Window, cx: &mut Context<Self>) {
        self.theme = match self.theme.mode {
            ThemeMode::Dark => Theme::light(),
            ThemeMode::Light | ThemeMode::System => Theme::dark(),
        };
        self.search_input
            .update(cx, |input, cx| input.set_theme(self.theme, cx));
        cx.notify();
    }

    fn focus_next(&mut self, _: &FocusNext, window: &mut Window, cx: &mut Context<Self>) {
        window.focus_next(cx);
    }

    fn focus_previous(&mut self, _: &FocusPrevious, window: &mut Window, cx: &mut Context<Self>) {
        window.focus_prev(cx);
    }

    fn request_retry(&mut self, cx: &mut Context<Self>) {
        cx.emit(AppShellEvent::RetryRequested);
    }

    fn render_header(&mut self, cx: &mut Context<Self>) -> Div {
        let colors = self.theme.colors;
        div()
            .h(px(64.0))
            .flex_none()
            .flex()
            .items_center()
            .gap_4()
            .px_4()
            .border_b_1()
            .border_color(colors.border)
            .bg(colors.surface)
            .child(
                div()
                    .w(px(192.0))
                    .flex_none()
                    .flex()
                    .items_baseline()
                    .gap_2()
                    .child(
                        div()
                            .text_lg()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child("AriaDeck"),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(colors.text_muted)
                            .child("Downloads"),
                    ),
            )
            .child(div().flex_1().min_w_0().child(self.search_input.clone()))
            .child(
                div()
                    .flex_none()
                    .flex()
                    .items_center()
                    .gap_3()
                    .child(metric(
                        "Down",
                        format_rate(self.snapshot.download_rate),
                        colors.text_secondary,
                    ))
                    .child(metric(
                        "Up",
                        format_rate(self.snapshot.upload_rate),
                        colors.text_secondary,
                    ))
                    .child(self.render_connection_badge(cx))
                    .child(
                        div()
                            .id("toggle-theme")
                            .focusable()
                            .tab_stop(true)
                            .role(Role::Button)
                            .aria_label("Toggle light and dark theme")
                            .h(px(34.0))
                            .px_3()
                            .flex()
                            .items_center()
                            .rounded_md()
                            .border_1()
                            .border_color(colors.border)
                            .bg(colors.elevated_surface)
                            .text_xs()
                            .text_color(colors.text_secondary)
                            .cursor_pointer()
                            .hover(|style| style.bg(colors.surface_hover))
                            .focus_visible(|style| style.border_color(colors.focus_ring))
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.toggle_theme(&ToggleTheme, window, cx);
                            }))
                            .child(match self.theme.mode {
                                ThemeMode::Dark => "Light",
                                ThemeMode::Light | ThemeMode::System => "Dark",
                            }),
                    ),
            )
    }

    fn render_connection_badge(&self, cx: &mut Context<Self>) -> AnyElement {
        let colors = self.theme.colors;
        let label = match &self.snapshot.connection {
            ConnectionView::Reconnecting { attempt } => format!("Reconnecting {attempt}"),
            connection => connection.label().to_owned(),
        };
        let color = connection_color(&self.snapshot.connection, colors);
        let badge = div()
            .id("connection-state")
            .h(px(34.0))
            .px_3()
            .flex()
            .items_center()
            .rounded_md()
            .bg(with_alpha(color, 0.12))
            .text_xs()
            .font_weight(FontWeight::MEDIUM)
            .text_color(color)
            .child(label);
        if self.snapshot.connection.can_retry() {
            badge
                .focusable()
                .tab_stop(true)
                .role(Role::Button)
                .aria_label("Retry aria2 connection")
                .cursor_pointer()
                .hover(|style| style.bg(with_alpha(color, 0.2)))
                .focus_visible(|style| style.border_1().border_color(colors.focus_ring))
                .on_click(cx.listener(|this, _, _, cx| this.request_retry(cx)))
                .into_any_element()
        } else {
            badge
                .role(Role::Status)
                .aria_label(format!(
                    "Connection status: {}",
                    self.snapshot.connection.label()
                ))
                .into_any_element()
        }
    }

    fn render_sidebar(&mut self, cx: &mut Context<Self>) -> Div {
        let colors = self.theme.colors;
        let mut filters = Vec::with_capacity(WorkspaceFilter::ALL.len());
        for filter in WorkspaceFilter::ALL {
            let count = filter.count(self.snapshot.counts);
            let selected = self.query.filter == filter;
            filters.push(
                div()
                    .id(SharedString::from(format!(
                        "sidebar-filter-{}",
                        filter.key()
                    )))
                    .focusable()
                    .tab_stop(true)
                    .role(Role::Button)
                    .aria_label(format!("{}, {count} tasks", filter.label()))
                    .h(px(36.0))
                    .w_full()
                    .px_3()
                    .flex()
                    .items_center()
                    .justify_between()
                    .rounded_md()
                    .text_sm()
                    .text_color(if selected {
                        colors.text_primary
                    } else {
                        colors.text_secondary
                    })
                    .when(selected, |element| element.bg(colors.surface_active))
                    .when(!selected, |element| {
                        element.hover(|style| style.bg(colors.surface_hover))
                    })
                    .focus_visible(|style| style.border_1().border_color(colors.focus_ring))
                    .cursor_pointer()
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.set_filter(filter, window, cx);
                    }))
                    .child(filter.short_label())
                    .child(
                        div()
                            .font_features(tabular_numbers())
                            .text_xs()
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(colors.text_muted)
                            .child(count.to_string()),
                    ),
            );
        }

        div()
            .w(px(208.0))
            .flex_none()
            .flex()
            .flex_col()
            .justify_between()
            .border_r_1()
            .border_color(colors.border)
            .bg(colors.surface)
            .p_3()
            .child(div().flex().flex_col().gap_1().children(filters))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .px_2()
                    .pb_1()
                    .child(
                        div()
                            .text_xs()
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(colors.text_secondary)
                            .child("Default profile"),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(colors.text_muted)
                            .child("External aria2 RPC"),
                    ),
            )
    }

    fn render_main(&mut self, cx: &mut Context<Self>) -> Div {
        let colors = self.theme.colors;
        let task_count = self.snapshot.tasks.len();
        let content = if task_count == 0 {
            self.render_empty_state(cx)
        } else {
            uniform_list(
                "download-tasks",
                task_count,
                cx.processor(|this, range: Range<usize>, _window, cx| {
                    this.rendered_range = range.clone();
                    range
                        .filter_map(|index| {
                            this.snapshot
                                .tasks
                                .get(index)
                                .cloned()
                                .map(|task| this.render_task_row(index, task, cx))
                        })
                        .collect::<Vec<_>>()
                }),
            )
            .track_scroll(&self.list_scroll)
            .h_full()
            .w_full()
            .into_any_element()
        };

        div()
            .flex_1()
            .min_w_0()
            .min_h_0()
            .flex()
            .flex_col()
            .bg(colors.background)
            .when(self.snapshot.stale, |element| {
                element.child(
                    div()
                        .id("stale-state-banner")
                        .h(px(34.0))
                        .flex_none()
                        .flex()
                        .items_center()
                        .justify_between()
                        .px_4()
                        .bg(with_alpha(colors.warning, 0.1))
                        .border_b_1()
                        .border_color(with_alpha(colors.warning, 0.28))
                        .text_xs()
                        .text_color(colors.warning)
                        .role(Role::Status)
                        .aria_label("Showing last known data while reconnecting")
                        .child("Showing last known data while aria2 reconnects")
                        .child(format!("Generation {}", self.snapshot.generation)),
                )
            })
            .child(
                div()
                    .h(px(48.0))
                    .flex_none()
                    .flex()
                    .items_center()
                    .justify_between()
                    .px_4()
                    .border_b_1()
                    .border_color(colors.border)
                    .child(
                        div()
                            .flex()
                            .items_baseline()
                            .gap_2()
                            .child(
                                div()
                                    .text_base()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(self.query.filter.label()),
                            )
                            .child(
                                div()
                                    .font_features(tabular_numbers())
                                    .text_xs()
                                    .text_color(colors.text_muted)
                                    .child(format!("{task_count} visible")),
                            ),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(colors.text_muted)
                            .child("Queue order"),
                    ),
            )
            .child(div().flex_1().min_h_0().child(content))
    }

    fn render_task_row(
        &mut self,
        index: usize,
        task: DownloadRowView,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let colors = self.theme.colors;
        let selected = self.selected.as_ref() == Some(&task.identity);
        let identity = task.identity.clone();
        let basis_points = task.progress_basis_points();
        let progress = f32::from(basis_points.unwrap_or(0)) / 10_000.0;
        let status_color = task_status_color(task.status, colors);
        let size_label = if task.total_bytes == 0 {
            format_bytes(task.completed_bytes)
        } else {
            format!(
                "{} / {}",
                format_bytes(task.completed_bytes),
                format_bytes(task.total_bytes)
            )
        };
        let aria_label = format!(
            "{}, {}, {}, download speed {}, ETA {}",
            task.display_name,
            task.status.label(),
            format_percent(basis_points),
            format_rate(task.download_rate),
            format_eta(task.eta_seconds)
        );
        div()
            .id(("task-row", index))
            .role(Role::ListItem)
            .aria_label(aria_label)
            .aria_selected(selected)
            .h(px(72.0))
            .w_full()
            .flex_none()
            .flex()
            .items_center()
            .gap_4()
            .px_4()
            .border_b_1()
            .border_color(colors.border)
            .bg(if selected {
                colors.surface_active
            } else {
                colors.background
            })
            .hover(|style| style.bg(colors.surface_hover))
            .cursor_pointer()
            .on_click(cx.listener(move |this, _, window, cx| {
                this.selected = Some(identity.clone());
                window.focus(&this.focus_handle, cx);
                cx.notify();
            }))
            .child(
                div().w(px(86.0)).flex_none().flex().items_center().child(
                    div()
                        .px_2()
                        .py_1()
                        .rounded_sm()
                        .bg(with_alpha(status_color, 0.11))
                        .text_xs()
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(status_color)
                        .child(task.status.label()),
                ),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_3()
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .truncate()
                                    .text_sm()
                                    .font_weight(FontWeight::MEDIUM)
                                    .child(task.display_name.clone()),
                            )
                            .child(
                                div()
                                    .flex_none()
                                    .font_features(tabular_numbers())
                                    .text_xs()
                                    .text_color(colors.text_muted)
                                    .child(size_label),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_3()
                            .child(
                                div()
                                    .flex_1()
                                    .h(px(4.0))
                                    .rounded_full()
                                    .overflow_hidden()
                                    .bg(colors.progress_track)
                                    .child(div().h_full().w(relative(progress)).rounded_full().bg(
                                        if task.status == TaskStatusView::Failed {
                                            colors.danger
                                        } else if task.status == TaskStatusView::Complete {
                                            colors.success
                                        } else {
                                            colors.progress_download
                                        },
                                    )),
                            )
                            .child(
                                div()
                                    .w(px(52.0))
                                    .flex_none()
                                    .font_features(tabular_numbers())
                                    .text_right()
                                    .text_xs()
                                    .text_color(colors.text_secondary)
                                    .child(format_percent(basis_points)),
                            )
                            .child(
                                div()
                                    .max_w(px(170.0))
                                    .truncate()
                                    .text_xs()
                                    .text_color(colors.text_muted)
                                    .child(task.identity.gid.clone()),
                            ),
                    ),
            )
            .child(
                div()
                    .w(px(190.0))
                    .flex_none()
                    .grid()
                    .grid_cols(2)
                    .gap_x_4()
                    .child(metric(
                        "Down",
                        format_rate(task.download_rate),
                        colors.text_secondary,
                    ))
                    .child(metric(
                        "ETA",
                        format_eta(task.eta_seconds),
                        colors.text_secondary,
                    )),
            )
    }

    fn render_empty_state(&self, cx: &mut Context<Self>) -> AnyElement {
        let colors = self.theme.colors;
        let (title, detail) = match &self.snapshot.connection {
            ConnectionView::Connecting
            | ConnectionView::Authenticating
            | ConnectionView::Synchronizing
            | ConnectionView::Reconnecting { .. }
                if self.snapshot.tasks.is_empty() =>
            {
                (
                    "Connecting to aria2",
                    "The queue will appear after the first synchronized snapshot.".to_owned(),
                )
            }
            ConnectionView::Failed { summary, .. } => (
                "aria2 connection failed",
                if summary.is_empty() {
                    "Review the RPC endpoint and authentication secret.".to_owned()
                } else {
                    summary.clone()
                },
            ),
            ConnectionView::Disconnected if self.snapshot.tasks.is_empty() => (
                "aria2 is unavailable",
                "AriaDeck will preserve known tasks and continue reconnecting.".to_owned(),
            ),
            _ if !self.query.search.trim().is_empty() => (
                "No matching downloads",
                "Try a different name, GID, or task category.".to_owned(),
            ),
            _ if self.query.filter != WorkspaceFilter::All => (
                "Nothing in this view",
                format!(
                    "No {} tasks are currently visible.",
                    self.query.filter.short_label()
                ),
            ),
            _ => (
                "Queue is clear",
                "New downloads will appear here as soon as aria2 accepts them.".to_owned(),
            ),
        };

        div()
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .child(
                div()
                    .max_w(px(420.0))
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap_2()
                    .text_center()
                    .child(
                        div()
                            .text_lg()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(title),
                    )
                    .child(div().text_sm().text_color(colors.text_muted).child(detail))
                    .when(self.snapshot.connection.can_retry(), |element| {
                        element.child(
                            div()
                                .id("retry-connection")
                                .focusable()
                                .tab_stop(true)
                                .role(Role::Button)
                                .aria_label("Retry aria2 connection now")
                                .mt_2()
                                .h(px(34.0))
                                .px_3()
                                .flex()
                                .items_center()
                                .rounded_md()
                                .bg(colors.accent)
                                .text_color(colors.text_inverse)
                                .text_sm()
                                .font_weight(FontWeight::MEDIUM)
                                .cursor_pointer()
                                .hover(|style| style.bg(colors.accent_hover))
                                .focus_visible(|style| {
                                    style.border_1().border_color(colors.focus_ring)
                                })
                                .on_click(cx.listener(|this, _, _, cx| this.request_retry(cx)))
                                .child("Retry now"),
                        )
                    }),
            )
            .into_any_element()
    }
}

impl Focusable for AppShell {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for AppShell {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = self.theme.colors;
        div()
            .id("download-workspace")
            .key_context("DownloadWorkspace")
            .role(Role::Application)
            .aria_label("AriaDeck download workspace")
            .track_focus(&self.focus_handle)
            .on_action(cx.listener(Self::focus_search))
            .on_action(cx.listener(Self::clear_search))
            .on_action(cx.listener(Self::select_next))
            .on_action(cx.listener(Self::select_previous))
            .on_action(cx.listener(Self::toggle_theme))
            .on_action(cx.listener(Self::focus_next))
            .on_action(cx.listener(Self::focus_previous))
            .size_full()
            .flex()
            .flex_col()
            .bg(colors.background)
            .text_color(colors.text_primary)
            .child(self.render_header(cx))
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .child(self.render_sidebar(cx))
                    .child(self.render_main(cx)),
            )
    }
}

fn metric(label: &'static str, value: String, text_color: Hsla) -> Div {
    div()
        .flex()
        .flex_col()
        .gap_0p5()
        .child(
            div()
                .text_xs()
                .text_color(with_alpha(text_color, 0.7))
                .child(label),
        )
        .child(
            div()
                .font_features(tabular_numbers())
                .text_xs()
                .font_weight(FontWeight::MEDIUM)
                .text_color(text_color)
                .child(value),
        )
}

fn tabular_numbers() -> FontFeatures {
    FontFeatures(Arc::new(vec![("tnum".into(), 1)]))
}

fn connection_color(connection: &ConnectionView, colors: crate::ThemeColors) -> Hsla {
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

fn task_status_color(status: TaskStatusView, colors: crate::ThemeColors) -> Hsla {
    match status {
        TaskStatusView::Active => colors.accent,
        TaskStatusView::Waiting | TaskStatusView::Paused => colors.warning,
        TaskStatusView::Complete => colors.success,
        TaskStatusView::Failed | TaskStatusView::Removed => colors.danger,
        TaskStatusView::Verifying => colors.information,
        TaskStatusView::Unknown => colors.text_muted,
    }
}

fn with_alpha(mut color: Hsla, alpha: f32) -> Hsla {
    color.a = alpha;
    color
}

#[cfg(test)]
mod tests {
    use gpui::TestAppContext;

    use super::*;
    use crate::{TaskCountsView, TaskStatusView};

    fn task(index: usize) -> DownloadRowView {
        DownloadRowView {
            identity: TaskIdentity {
                profile_id: "profile".into(),
                gid: format!("{index:016x}"),
            },
            display_name: format!("archive-{index:05}.bin"),
            status: TaskStatusView::Complete,
            total_bytes: 1_048_576,
            completed_bytes: 1_048_576,
            download_rate: 0,
            upload_rate: 0,
            eta_seconds: Some(0),
            revision: 1,
        }
    }

    fn snapshot(count: usize) -> WorkspaceSnapshot {
        WorkspaceSnapshot {
            profile_id: "profile".into(),
            generation: 1,
            source_revision: 1,
            connection: ConnectionView::Connected,
            stale: false,
            download_rate: 0,
            upload_rate: 0,
            counts: TaskCountsView {
                all: count,
                completed: count,
                ..TaskCountsView::default()
            },
            tasks: (0..count).map(task).collect(),
        }
    }

    #[gpui::test]
    fn ten_thousand_tasks_render_only_a_viewport_window(cx: &mut TestAppContext) {
        let (view, cx) = cx.add_window_view(|window, cx| {
            let mut shell = AppShell::new(Theme::dark(), window, cx);
            shell.snapshot = snapshot(10_000);
            shell
        });

        view.read_with(cx, |shell, _| {
            let rendered = shell.rendered_range();
            assert!(!rendered.is_empty());
            assert!(rendered.len() < 64, "rendered {} rows", rendered.len());
            assert_eq!(shell.snapshot.tasks.len(), 10_000);
        });
    }

    #[gpui::test]
    fn selection_survives_filtered_snapshots_for_the_same_profile(cx: &mut TestAppContext) {
        let (view, cx) = cx.add_window_view(|window, cx| {
            let mut shell = AppShell::new(Theme::dark(), window, cx);
            shell.snapshot = snapshot(3);
            shell.selected = Some(shell.snapshot.tasks[1].identity.clone());
            shell
        });
        let selected = view.read_with(cx, |shell, _| shell.selected.clone());

        view.update(cx, |shell, cx| {
            let mut filtered = snapshot(1);
            filtered.tasks[0] = task(2);
            shell.set_snapshot(filtered, cx);
        });
        view.read_with(cx, |shell, _| {
            assert_eq!(shell.selected, selected);
        });
    }
}
