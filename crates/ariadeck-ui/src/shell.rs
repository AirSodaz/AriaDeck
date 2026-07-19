use std::{ops::Range, sync::Arc, time::Duration};

use gpui::{
    AnyElement, App, ClipboardItem, Context, Div, Entity, FocusHandle, Focusable, FontFeatures,
    FontWeight, Hsla, IntoElement, Render, Role, ScrollStrategy, SharedString, Stateful,
    Subscription, UniformListScrollHandle, WeakFocusHandle, Window, WindowControlArea, div,
    prelude::*, px, relative, uniform_list,
};

use crate::{
    AddDownloadRequestView, AddDownloadResultView, Button, ButtonStyle, ClearSearch,
    CloseAddDownload, CloseSettings, ColorSchemeView, CommandOutcomeView, ConnectionView, Dialog,
    DownloadRowView, EngineHealthView, EngineSessionView, FocusNext, FocusPrevious, FocusSearch,
    Icon, IconButton, IconName, IconSize, OpenAddDownload, OpenSettings, OpenTaskDetails,
    OperationErrorView, PauseSelectedTask, RemoveSelectedTask, RequestId, ResumeSelectedTask,
    RetrySelectedTask, SaveSettings, SearchInputEvent, Segment, SegmentedControl, SelectNextTask,
    SelectPreviousTask, SettingsSaveOutcomeView, SettingsSaveRequestView, SettingsSaveResultView,
    SettingsView, SpeedSampleView, StatusIndicator, SubmitAddDownload, TaskCommandRequestView,
    TaskCommandResultView, TaskCommandView, TaskDetailsOutcomeView, TaskDetailsRequestView,
    TaskDetailsResultView, TaskDetailsView, TaskFileView, TaskIdentity, TaskStatusView, TextField,
    TextFieldConfig, Theme, ThemeMode, Toast, ToastKind, Tooltip, WorkspaceFilter, WorkspaceQuery,
    WorkspaceSnapshot, format_bytes, format_eta, format_percent, format_rate,
};

const SPEED_CHART_SAMPLES: usize = 120;
const TITLEBAR_HEIGHT: f32 = 56.0;
const TITLEBAR_SIDE_WIDTH: f32 = 240.0;
const TITLEBAR_HORIZONTAL_PADDING: f32 = 12.0;
const SEARCH_WIDTH: f32 = 460.0;
const SIDEBAR_WIDTH: f32 = 208.0;
const DETAILS_DRAWER_WIDTH: f32 = 360.0;
const TASK_LAYOUT_WIDE_MIN_WIDTH: f32 = 820.0;
const TASK_ROW_HEIGHT: f32 = 68.0;

#[cfg(target_os = "macos")]
const TITLEBAR_BRAND_INSET: f32 = 52.0;
#[cfg(not(target_os = "macos"))]
const TITLEBAR_BRAND_INSET: f32 = 0.0;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum TaskLayoutMode {
    Compact,
    Wide,
}

fn task_layout_mode(viewport_width: f32, details_open: bool) -> TaskLayoutMode {
    let details_width = if details_open {
        DETAILS_DRAWER_WIDTH
    } else {
        0.0
    };
    let main_width = viewport_width - SIDEBAR_WIDTH - details_width;
    if main_width >= TASK_LAYOUT_WIDE_MIN_WIDTH {
        TaskLayoutMode::Wide
    } else {
        TaskLayoutMode::Compact
    }
}

fn centered_search_bounds(viewport_width: f32) -> (f32, f32) {
    let available_width =
        (viewport_width - 2.0 * (TITLEBAR_SIDE_WIDTH + TITLEBAR_HORIZONTAL_PADDING)).max(0.0);
    let width = available_width.min(SEARCH_WIDTH);
    let left = (viewport_width - width) / 2.0;
    (left, left + width)
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AppShellEvent {
    QueryChanged(WorkspaceQuery),
    RetryRequested,
    AddDownloadRequested(AddDownloadRequestView),
    TaskCommandRequested(TaskCommandRequestView),
    TaskDetailsRequested(TaskDetailsRequestView),
    SettingsSaveRequested(SettingsSaveRequestView),
}

struct PendingAddDownload {
    request_id: RequestId,
    session: EngineSessionView,
}

#[derive(Default)]
struct AddDownloadDialog {
    open: bool,
    previous_focus: Option<WeakFocusHandle>,
    pending: Option<PendingAddDownload>,
    error: Option<OperationErrorView>,
}

struct PendingTaskCommand {
    request_id: RequestId,
    session: EngineSessionView,
    identity: TaskIdentity,
    command: TaskCommandView,
}

enum TaskDetailsLoadState {
    Loading { request_id: RequestId },
    Ready { details: TaskDetailsView },
    Failed { error: OperationErrorView },
    Stale,
}

enum TaskDetailsPresentation {
    Loading,
    Ready {
        directory: Option<String>,
        info_hash: Option<String>,
        piece_length: Option<u64>,
        piece_count: Option<u32>,
        file_count: usize,
    },
    Failed(String),
    Stale,
}

struct TaskDetailsDrawer {
    identity: TaskIdentity,
    overview: DownloadRowView,
    session: EngineSessionView,
    state: TaskDetailsLoadState,
    file_scroll: UniformListScrollHandle,
    rendered_file_range: Range<usize>,
}

struct StatusNotice {
    id: u64,
    message: String,
    is_error: bool,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum AppPage {
    #[default]
    Downloads,
    Settings,
}

#[derive(Default)]
struct SettingsPage {
    previous_focus: Option<WeakFocusHandle>,
    draft_color_scheme: ColorSchemeView,
    error: Option<OperationErrorView>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SettingsSaveSource {
    Theme,
    Directory,
}

struct PendingSettingsSave {
    request_id: RequestId,
    settings: SettingsView,
    source: SettingsSaveSource,
}

struct RemoveConfirmation {
    identity: TaskIdentity,
    display_name: String,
    previous_focus: Option<WeakFocusHandle>,
}

pub struct AppShell {
    theme: Theme,
    settings: SettingsView,
    page: AppPage,
    engine_health: EngineHealthView,
    snapshot: WorkspaceSnapshot,
    query: WorkspaceQuery,
    selected: Option<TaskIdentity>,
    search_input: Entity<TextField>,
    add_input: Entity<TextField>,
    settings_directory_input: Entity<TextField>,
    add_dialog: AddDownloadDialog,
    add_dialog_focus: FocusHandle,
    add_cancel_focus: FocusHandle,
    add_submit_focus: FocusHandle,
    settings_page: SettingsPage,
    settings_save_focus: FocusHandle,
    pending_settings_save: Option<PendingSettingsSave>,
    pending_task_command: Option<PendingTaskCommand>,
    details_drawer: Option<TaskDetailsDrawer>,
    remove_confirmation: Option<RemoveConfirmation>,
    remove_dialog_focus: FocusHandle,
    remove_cancel_focus: FocusHandle,
    remove_submit_focus: FocusHandle,
    speed_popover_open: bool,
    speed_popover_previous_focus: Option<WeakFocusHandle>,
    status_notice: Option<StatusNotice>,
    next_notice_id: u64,
    next_request_id: u64,
    list_scroll: UniformListScrollHandle,
    focus_handle: FocusHandle,
    rendered_range: Range<usize>,
    _search_subscription: Subscription,
    _add_subscription: Subscription,
    _settings_subscription: Subscription,
    _window_bounds_subscription: Subscription,
}

impl gpui::EventEmitter<AppShellEvent> for AppShell {}

impl AppShell {
    #[must_use]
    pub fn new(theme: Theme, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let color_scheme = match theme.mode {
            ThemeMode::Light => ColorSchemeView::Light,
            ThemeMode::Dark | ThemeMode::System => ColorSchemeView::Dark,
        };
        Self::new_inner(
            theme,
            SettingsView {
                color_scheme,
                download_directory: String::new(),
            },
            window,
            cx,
        )
    }

    #[must_use]
    pub fn new_with_settings(
        settings: SettingsView,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        Self::new_inner(
            theme_for_scheme(settings.color_scheme),
            settings,
            window,
            cx,
        )
    }

    fn new_inner(
        theme: Theme,
        settings: SettingsView,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let search_input = cx.new(|cx| TextField::new("Search downloads or GID", theme, cx));
        let search_subscription = cx.subscribe(
            &search_input,
            |this: &mut Self, _input, event: &SearchInputEvent, cx| {
                if this.query.search != event.text {
                    this.query.search.clone_from(&event.text);
                    this.emit_query(cx);
                }
            },
        );
        let add_input = cx.new(|cx| {
            TextField::new_with_config(
                TextFieldConfig {
                    element_id: "add-download-uri".into(),
                    key_context: "AddDownloadInput".into(),
                    role: Role::TextInput,
                    accessibility_label: "Download URL or magnet link".into(),
                    placeholder: "https://example.com/file or magnet:?xt=...".into(),
                    leading_icon: Some(IconName::Link),
                    clearable: true,
                },
                theme,
                cx,
            )
        });
        let add_subscription = cx.subscribe(
            &add_input,
            |this: &mut Self, _input, _event: &SearchInputEvent, cx| {
                if this.add_dialog.open
                    && this.add_dialog.pending.is_none()
                    && this.add_dialog.error.take().is_some()
                {
                    cx.notify();
                }
            },
        );
        let settings_directory_input = cx.new(|cx| {
            TextField::new_with_config(
                TextFieldConfig {
                    element_id: "settings-download-directory".into(),
                    key_context: "SettingsDirectoryInput".into(),
                    role: Role::TextInput,
                    accessibility_label: "Default download directory".into(),
                    placeholder: "D:\\Downloads".into(),
                    leading_icon: Some(IconName::FolderDown),
                    clearable: true,
                },
                theme,
                cx,
            )
        });
        let settings_subscription = cx.subscribe(
            &settings_directory_input,
            |this: &mut Self, _input, _event: &SearchInputEvent, cx| {
                if this.page == AppPage::Settings
                    && this.pending_settings_save.is_none()
                    && this.settings_page.error.take().is_some()
                {
                    cx.notify();
                }
            },
        );
        let window_bounds_subscription = cx.observe_window_bounds(window, |_, _, cx| {
            cx.notify();
        });
        let focus_handle = cx.focus_handle();
        window.focus(&focus_handle, cx);
        Self {
            theme,
            settings,
            page: AppPage::Downloads,
            engine_health: EngineHealthView::External,
            snapshot: WorkspaceSnapshot::default(),
            query: WorkspaceQuery::default(),
            selected: None,
            search_input,
            add_input,
            settings_directory_input,
            add_dialog: AddDownloadDialog::default(),
            add_dialog_focus: cx.focus_handle(),
            add_cancel_focus: cx.focus_handle().tab_stop(true),
            add_submit_focus: cx.focus_handle().tab_stop(true),
            settings_page: SettingsPage::default(),
            settings_save_focus: cx.focus_handle().tab_stop(true),
            pending_settings_save: None,
            pending_task_command: None,
            details_drawer: None,
            remove_confirmation: None,
            remove_dialog_focus: cx.focus_handle(),
            remove_cancel_focus: cx.focus_handle().tab_stop(true),
            remove_submit_focus: cx.focus_handle().tab_stop(true),
            speed_popover_open: false,
            speed_popover_previous_focus: None,
            status_notice: None,
            next_notice_id: 1,
            next_request_id: 1,
            list_scroll: UniformListScrollHandle::new(),
            focus_handle,
            rendered_range: 0..0,
            _search_subscription: search_subscription,
            _add_subscription: add_subscription,
            _settings_subscription: settings_subscription,
            _window_bounds_subscription: window_bounds_subscription,
        }
    }

    pub fn set_snapshot(&mut self, snapshot: WorkspaceSnapshot, cx: &mut Context<Self>) {
        let previous_session = self.snapshot.engine_session();
        let previous_commands_available = self.snapshot.commands_available();
        let next_session = snapshot.engine_session();
        let session_changed = previous_session != next_session;
        let profile_changed = self
            .selected
            .as_ref()
            .is_some_and(|selected| selected.profile_id != snapshot.profile_id);

        if profile_changed {
            self.selected = None;
            self.details_drawer = None;
        }

        if session_changed {
            if self.add_dialog.pending.take().is_some() {
                self.add_dialog.error = Some(stale_session_error());
            }
            if self.pending_task_command.take().is_some() {
                self.show_notice(
                    "The engine session changed before the command completed. Its outcome was not replayed.",
                    true,
                    cx,
                );
            }
            if let (Some(drawer), Some(session)) = (&mut self.details_drawer, &next_session) {
                drawer.session = session.clone();
                drawer.state = TaskDetailsLoadState::Stale;
            }
        }

        self.snapshot = snapshot;

        if let Some(drawer) = &mut self.details_drawer {
            if let Some(task) = self
                .snapshot
                .tasks
                .iter()
                .find(|task| task.identity == drawer.identity)
            {
                drawer.overview = task.clone();
            }
            if !self.snapshot.commands_available() {
                drawer.state = TaskDetailsLoadState::Stale;
            }
        }

        let should_refresh_details = self.details_drawer.is_some()
            && self.snapshot.commands_available()
            && (session_changed || !previous_commands_available);
        if should_refresh_details {
            self.request_current_details(cx);
        }
        cx.notify();
    }

    pub fn set_engine_health(&mut self, health: EngineHealthView, cx: &mut Context<Self>) {
        if self.engine_health == health {
            return;
        }
        self.engine_health = health;
        match &self.engine_health {
            EngineHealthView::Running { restarts } if *restarts > 0 => self.show_notice(
                format!(
                    "Local aria2 recovered after {restarts} restart attempt{}.",
                    if *restarts == 1 { "" } else { "s" }
                ),
                false,
                cx,
            ),
            EngineHealthView::Failed { summary } => self.show_notice(
                format!("Local aria2 could not be restarted: {summary}"),
                true,
                cx,
            ),
            _ => cx.notify(),
        }
    }

    pub fn set_add_download_result(
        &mut self,
        result: AddDownloadResultView,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let matches_pending = self.add_dialog.pending.as_ref().is_some_and(|pending| {
            pending.request_id == result.request_id && pending.session == result.session
        });
        if !matches_pending {
            return;
        }

        self.add_dialog.pending = None;
        match result.outcome {
            CommandOutcomeView::Success { task } => {
                self.add_input
                    .update(cx, |input, cx| input.set_text("", cx));
                self.show_notice("Download accepted by aria2.", false, cx);
                if let Some(identity) = task {
                    self.selected = Some(identity);
                }
                self.close_add_download(window, cx);
            }
            CommandOutcomeView::Failure(error) => {
                self.add_dialog.error = Some(error);
                cx.notify();
            }
        }
    }

    pub fn set_task_command_result(
        &mut self,
        result: TaskCommandResultView,
        cx: &mut Context<Self>,
    ) {
        let matches_pending = self.pending_task_command.as_ref().is_some_and(|pending| {
            pending.request_id == result.request_id
                && pending.session == result.session
                && pending.identity == result.identity
                && pending.command == result.command
        });
        if !matches_pending {
            return;
        }

        self.pending_task_command = None;
        match result.outcome {
            CommandOutcomeView::Success { task } => {
                self.show_notice(result.command.success_label(), false, cx);
                if result.command == TaskCommandView::RemoveTask {
                    self.selected = None;
                    self.details_drawer = None;
                } else if result.command == TaskCommandView::Retry {
                    if let Some(identity) = task {
                        self.selected = Some(identity);
                    }
                    self.details_drawer = None;
                }
            }
            CommandOutcomeView::Failure(error) => {
                let message = if error.outcome_unknown() {
                    format!(
                        "Command outcome is unknown; AriaDeck will not retry it automatically. {}",
                        error.summary
                    )
                } else {
                    error.summary
                };
                self.show_notice(message, true, cx);
            }
        }
        cx.notify();
    }

    pub fn set_task_details_result(
        &mut self,
        result: TaskDetailsResultView,
        cx: &mut Context<Self>,
    ) {
        let Some(drawer) = &mut self.details_drawer else {
            return;
        };
        let request_matches = matches!(
            drawer.state,
            TaskDetailsLoadState::Loading { request_id } if request_id == result.request_id
        );
        if !request_matches
            || drawer.session != result.session
            || drawer.identity != result.identity
        {
            return;
        }

        drawer.state = match result.outcome {
            TaskDetailsOutcomeView::Ready(details) => TaskDetailsLoadState::Ready { details },
            TaskDetailsOutcomeView::Failed(error) => TaskDetailsLoadState::Failed { error },
        };
        cx.notify();
    }

    pub fn set_settings_save_result(
        &mut self,
        result: SettingsSaveResultView,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(pending) = self.pending_settings_save.as_ref() else {
            return;
        };
        if pending.request_id != result.request_id || pending.settings != result.settings {
            return;
        }
        let source = pending.source;
        self.pending_settings_save = None;

        match result.outcome {
            SettingsSaveOutcomeView::Success => {
                self.apply_settings(result.settings, cx);
                self.settings_page.error = None;
                let message = match source {
                    SettingsSaveSource::Theme => "Appearance updated.",
                    SettingsSaveSource::Directory => "Download directory saved.",
                };
                self.show_notice(message, false, cx);
            }
            SettingsSaveOutcomeView::Failure(error) => {
                self.settings_page.error = Some(error);
                cx.notify();
            }
        }
        let _ = window;
    }

    pub fn set_startup_notice(&mut self, message: String, is_error: bool, cx: &mut Context<Self>) {
        self.show_notice(message, is_error, cx);
    }

    #[must_use]
    pub fn settings(&self) -> &SettingsView {
        &self.settings
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

    fn set_filter(&mut self, filter: WorkspaceFilter, window: &mut Window, cx: &mut Context<Self>) {
        let query_changed = self.query.filter != filter;
        self.page = AppPage::Downloads;
        self.speed_popover_open = false;
        if query_changed {
            self.query.filter = filter;
        }
        self.list_scroll
            .scroll_to_item_strict(0, ScrollStrategy::Top);
        window.focus(&self.focus_handle, cx);
        if query_changed {
            self.emit_query(cx);
        } else {
            cx.notify();
        }
    }

    fn focus_search(&mut self, _: &FocusSearch, window: &mut Window, cx: &mut Context<Self>) {
        self.page = AppPage::Downloads;
        self.speed_popover_open = false;
        window.focus(&self.search_input.focus_handle(cx), cx);
        cx.notify();
    }

    fn clear_search(&mut self, _: &ClearSearch, window: &mut Window, cx: &mut Context<Self>) {
        if self.speed_popover_open {
            self.close_speed_popover(window, cx);
        } else if self.remove_confirmation.is_some() {
            self.close_remove_confirmation(window, cx);
        } else if self.page == AppPage::Settings {
            self.close_settings(window, cx);
        } else if !self.search_input.read(cx).text().is_empty() {
            self.search_input
                .update(cx, |input, cx| input.set_text("", cx));
        } else if self.details_drawer.take().is_some() {
            window.focus(&self.focus_handle, cx);
            cx.notify();
        } else {
            window.focus(&self.focus_handle, cx);
        }
    }

    fn select_next(&mut self, _: &SelectNextTask, window: &mut Window, cx: &mut Context<Self>) {
        if self.snapshot.tasks.is_empty() {
            return;
        }
        let next = match self.selected_index() {
            Some(current) => (current + 1).min(self.snapshot.tasks.len() - 1),
            None => 0,
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
        let previous = match self.selected_index() {
            Some(current) => current.saturating_sub(1),
            None => self.snapshot.tasks.len() - 1,
        };
        self.select_at(previous, window, cx);
    }

    fn select_at(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(task) = self.snapshot.tasks.get(index) else {
            return;
        };
        let task = task.clone();
        self.selected = Some(task.identity.clone());
        self.list_scroll
            .scroll_to_item(index, ScrollStrategy::Nearest);
        if self.details_drawer.is_some() {
            self.open_details_for(task, cx);
        }
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

    fn apply_settings(&mut self, settings: SettingsView, cx: &mut Context<Self>) {
        self.theme = theme_for_scheme(settings.color_scheme);
        self.settings = settings.clone();
        self.settings_page.draft_color_scheme = settings.color_scheme;
        self.search_input
            .update(cx, |input, cx| input.set_theme(self.theme, cx));
        self.add_input
            .update(cx, |input, cx| input.set_theme(self.theme, cx));
        self.settings_directory_input
            .update(cx, |input, cx| input.set_theme(self.theme, cx));
    }

    fn focus_next(&mut self, _: &FocusNext, window: &mut Window, cx: &mut Context<Self>) {
        window.focus_next(cx);
        if self.add_dialog.open && !self.add_dialog_focus.contains_focused(window, cx) {
            window.focus(&self.add_input.focus_handle(cx), cx);
        } else if self.remove_confirmation.is_some()
            && !self.remove_dialog_focus.contains_focused(window, cx)
        {
            window.focus(&self.remove_cancel_focus, cx);
        }
    }

    fn focus_previous(&mut self, _: &FocusPrevious, window: &mut Window, cx: &mut Context<Self>) {
        window.focus_prev(cx);
        if self.add_dialog.open && !self.add_dialog_focus.contains_focused(window, cx) {
            window.focus(&self.add_submit_focus, cx);
        } else if self.remove_confirmation.is_some()
            && !self.remove_dialog_focus.contains_focused(window, cx)
        {
            window.focus(&self.remove_submit_focus, cx);
        }
    }

    fn open_settings(&mut self, _: &OpenSettings, window: &mut Window, cx: &mut Context<Self>) {
        if self.page == AppPage::Settings {
            window.focus(&self.settings_directory_input.focus_handle(cx), cx);
            return;
        }
        if self.add_dialog.open || self.remove_confirmation.is_some() {
            return;
        }
        let download_directory = self.settings.download_directory.clone();
        self.settings_directory_input
            .update(cx, |input, cx| input.set_text(download_directory, cx));
        self.page = AppPage::Settings;
        self.details_drawer = None;
        self.speed_popover_open = false;
        self.settings_page = SettingsPage {
            previous_focus: window.focused(cx).map(|focus| focus.downgrade()),
            draft_color_scheme: self.settings.color_scheme,
            error: None,
        };
        cx.notify();
    }

    fn close_settings_action(
        &mut self,
        _: &CloseSettings,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.close_settings(window, cx);
    }

    fn close_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.page != AppPage::Settings {
            return;
        }
        let previous_focus = self.settings_page.previous_focus.take();
        self.page = AppPage::Downloads;
        if let Some(focus) = previous_focus.and_then(|focus| focus.upgrade()) {
            window.focus(&focus, cx);
        } else {
            window.focus(&self.focus_handle, cx);
        }
        cx.notify();
    }

    fn save_settings_action(
        &mut self,
        _: &SaveSettings,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.submit_settings(cx);
    }

    fn submit_settings(&mut self, cx: &mut Context<Self>) {
        if self.page != AppPage::Settings || self.pending_settings_save.is_some() {
            return;
        }
        let download_directory = self
            .settings_directory_input
            .read(cx)
            .text()
            .trim()
            .to_owned();
        if download_directory.is_empty() {
            self.settings_page.error = Some(OperationErrorView {
                code: "settings.invalid_download_directory".into(),
                summary: "Choose a non-empty download directory.".into(),
                retryable: false,
            });
            cx.notify();
            return;
        }
        self.request_settings_save(
            SettingsView {
                color_scheme: self.settings.color_scheme,
                download_directory,
            },
            SettingsSaveSource::Directory,
            cx,
        );
    }

    fn select_color_scheme(&mut self, scheme: ColorSchemeView, cx: &mut Context<Self>) {
        if self.pending_settings_save.is_some() || scheme == self.settings.color_scheme {
            return;
        }
        self.settings_page.draft_color_scheme = scheme;
        self.request_settings_save(
            SettingsView {
                color_scheme: scheme,
                download_directory: self.settings.download_directory.clone(),
            },
            SettingsSaveSource::Theme,
            cx,
        );
    }

    fn request_settings_save(
        &mut self,
        settings: SettingsView,
        source: SettingsSaveSource,
        cx: &mut Context<Self>,
    ) {
        if self.pending_settings_save.is_some() {
            return;
        }
        let request_id = self.allocate_request_id();
        self.pending_settings_save = Some(PendingSettingsSave {
            request_id,
            settings: settings.clone(),
            source,
        });
        self.settings_page.error = None;
        cx.emit(AppShellEvent::SettingsSaveRequested(
            SettingsSaveRequestView {
                request_id,
                settings,
            },
        ));
        cx.notify();
    }

    fn request_retry(&mut self, cx: &mut Context<Self>) {
        cx.emit(AppShellEvent::RetryRequested);
    }

    fn show_notice(&mut self, message: impl Into<String>, is_error: bool, cx: &mut Context<Self>) {
        let id = self.next_notice_id;
        self.next_notice_id = self.next_notice_id.checked_add(1).unwrap_or(1);
        self.status_notice = Some(StatusNotice {
            id,
            message: message.into(),
            is_error,
        });
        cx.notify();
        if !is_error {
            cx.spawn(async move |this, cx| {
                cx.background_executor().timer(Duration::from_secs(3)).await;
                this.update(cx, |this, cx| {
                    this.expire_notice(id, cx);
                })
                .ok();
            })
            .detach();
        }
    }

    fn expire_notice(&mut self, id: u64, cx: &mut Context<Self>) {
        if self
            .status_notice
            .as_ref()
            .is_some_and(|notice| notice.id == id && !notice.is_error)
        {
            self.status_notice = None;
            cx.notify();
        }
    }

    fn dismiss_notice(&mut self, cx: &mut Context<Self>) {
        if self.status_notice.take().is_some() {
            cx.notify();
        }
    }

    fn allocate_request_id(&mut self) -> RequestId {
        let request_id = RequestId::from_u64(self.next_request_id);
        self.next_request_id = self.next_request_id.checked_add(1).unwrap_or(1);
        request_id
    }

    fn open_add_download(
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
        if !self.snapshot.commands_available() {
            self.show_notice(
                "Connect and finish synchronization before adding a download.",
                true,
                cx,
            );
            return;
        }

        self.add_input
            .update(cx, |input, cx| input.set_text("", cx));
        self.add_dialog = AddDownloadDialog {
            open: true,
            previous_focus: window.focused(cx).map(|focus| focus.downgrade()),
            pending: None,
            error: None,
        };
        cx.notify();
        cx.defer_in(window, |this, window, cx| {
            if this.add_dialog.open {
                window.focus(&this.add_input.focus_handle(cx), cx);
            }
        });
    }

    fn close_add_download_action(
        &mut self,
        _: &CloseAddDownload,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.close_add_download(window, cx);
    }

    fn close_add_download(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.add_dialog.open || self.add_dialog.pending.is_some() {
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

    fn submit_add_download_action(
        &mut self,
        _: &SubmitAddDownload,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.submit_add_download(cx);
    }

    fn submit_add_download(&mut self, cx: &mut Context<Self>) {
        if !self.add_dialog.open || self.add_dialog.pending.is_some() {
            return;
        }
        let uri = self.add_input.read(cx).text().trim().to_owned();
        if uri.is_empty() {
            self.add_dialog.error = Some(OperationErrorView {
                code: "validation.invalid_request".into(),
                summary: "Enter a URL or magnet link.".into(),
                retryable: false,
            });
            cx.notify();
            return;
        }
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
        cx.emit(AppShellEvent::AddDownloadRequested(
            AddDownloadRequestView {
                request_id,
                session,
                uri,
                destination: (!self.settings.download_directory.is_empty())
                    .then(|| self.settings.download_directory.clone()),
            },
        ));
        cx.notify();
    }

    fn open_task_details_action(
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

    fn open_details_for(&mut self, task: DownloadRowView, cx: &mut Context<Self>) {
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
            file_scroll: UniformListScrollHandle::new(),
            rendered_file_range: 0..0,
        });
        if self.snapshot.commands_available() {
            self.request_current_details(cx);
        }
        cx.notify();
    }

    fn request_current_details(&mut self, cx: &mut Context<Self>) {
        let Some(session) = self.snapshot.engine_session() else {
            return;
        };
        let Some(identity) = self
            .details_drawer
            .as_ref()
            .map(|drawer| drawer.identity.clone())
        else {
            return;
        };
        if identity.profile_id != session.profile_id || !self.snapshot.commands_available() {
            return;
        }

        let request_id = self.allocate_request_id();
        if let Some(drawer) = &mut self.details_drawer {
            drawer.session = session.clone();
            drawer.state = TaskDetailsLoadState::Loading { request_id };
        }
        cx.emit(AppShellEvent::TaskDetailsRequested(
            TaskDetailsRequestView {
                request_id,
                session,
                identity,
            },
        ));
        cx.notify();
    }

    fn close_task_details(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.details_drawer.take().is_some() {
            window.focus(&self.focus_handle, cx);
            cx.notify();
        }
    }

    fn pause_selected(
        &mut self,
        _: &PauseSelectedTask,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.begin_task_command(TaskCommandView::Pause, cx);
    }

    fn resume_selected(
        &mut self,
        _: &ResumeSelectedTask,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.begin_task_command(TaskCommandView::Resume, cx);
    }

    fn retry_selected(
        &mut self,
        _: &RetrySelectedTask,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.begin_task_command(TaskCommandView::Retry, cx);
    }

    fn remove_selected(
        &mut self,
        _: &RemoveSelectedTask,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.confirm_remove_selected(window, cx);
    }

    fn begin_task_command(&mut self, command: TaskCommandView, cx: &mut Context<Self>) {
        if self.pending_task_command.is_some() {
            return;
        }
        let Some(task) = self.selected_task_view() else {
            self.show_notice("Select a visible task first.", true, cx);
            return;
        };
        let allowed = match command {
            TaskCommandView::Pause => task.status.can_pause(),
            TaskCommandView::Resume => task.status.can_resume(),
            TaskCommandView::Retry => task.status.can_retry(),
            TaskCommandView::RemoveTask => task.status.can_remove(),
        };
        if !allowed {
            self.show_notice(
                format!(
                    "{} is not available while the task is {}.",
                    task_command_label(command),
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
            self.show_notice("The engine is not ready for commands.", true, cx);
            return;
        };

        let request_id = self.allocate_request_id();
        let identity = task.identity;
        self.pending_task_command = Some(PendingTaskCommand {
            request_id,
            session: session.clone(),
            identity: identity.clone(),
            command,
        });
        self.show_notice(command.progress_label(), false, cx);
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

    fn confirm_remove_selected(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.remove_confirmation.is_some() || self.pending_task_command.is_some() {
            return;
        }
        let Some(task) = self.selected_task_view() else {
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

        self.remove_confirmation = Some(RemoveConfirmation {
            identity: task.identity,
            display_name: task.display_name,
            previous_focus: window.focused(cx).map(|focus| focus.downgrade()),
        });
        cx.notify();
        cx.defer_in(window, |this, window, cx| {
            if this.remove_confirmation.is_some() {
                window.focus(&this.remove_cancel_focus, cx);
            }
        });
    }

    fn close_remove_confirmation(&mut self, window: &mut Window, cx: &mut Context<Self>) {
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

    fn submit_remove_confirmation(&mut self, cx: &mut Context<Self>) {
        let Some(confirmation) = self.remove_confirmation.take() else {
            return;
        };
        if self.selected.as_ref() != Some(&confirmation.identity) {
            self.show_notice(
                "The selected task changed. Review the task before removing it.",
                true,
                cx,
            );
            return;
        }
        self.begin_task_command(TaskCommandView::RemoveTask, cx);
    }

    fn toggle_speed_popover(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.speed_popover_open {
            self.close_speed_popover(window, cx);
            return;
        }
        self.speed_popover_previous_focus = window.focused(cx).map(|focus| focus.downgrade());
        self.speed_popover_open = true;
        cx.notify();
    }

    fn close_speed_popover(&mut self, window: &mut Window, cx: &mut Context<Self>) {
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

    fn selected_task_view(&self) -> Option<DownloadRowView> {
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

    fn render_header(&mut self, _window: &Window, cx: &mut Context<Self>) -> Div {
        let colors = self.theme.colors;
        let (search_left, search_right) =
            centered_search_bounds(f32::from(_window.viewport_size().width));
        let search_width = search_right - search_left;
        let brand = div()
            .w(px(TITLEBAR_SIDE_WIDTH))
            .flex_none()
            .flex()
            .items_center()
            .h_full()
            .gap_2()
            .pl(px(TITLEBAR_BRAND_INSET))
            .window_control_area(WindowControlArea::Drag)
            .child(
                Icon::new(IconName::Download)
                    .size(IconSize::Medium)
                    .color(colors.accent),
            )
            .child(
                div()
                    .text_sm()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child("AriaDeck"),
            );
        div()
            .h(px(TITLEBAR_HEIGHT))
            .flex_none()
            .flex()
            .items_center()
            .px_3()
            .border_b_1()
            .border_color(colors.border)
            .bg(colors.toolbar_surface)
            .child(brand)
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .flex()
                    .items_center()
                    .child(titlebar_drag_region())
                    .child(
                        div()
                            .w(px(search_width))
                            .flex_none()
                            .child(self.search_input.clone()),
                    )
                    .child(titlebar_drag_region()),
            )
            .child(
                div()
                    .w(px(TITLEBAR_SIDE_WIDTH))
                    .flex_none()
                    .flex()
                    .items_center()
                    .justify_end()
                    .gap_2()
                    .child(self.render_add_button(cx))
                    .when(cfg!(target_os = "windows"), |actions| {
                        #[cfg(target_os = "windows")]
                        {
                            actions.child(self.render_window_controls(_window))
                        }
                        #[cfg(not(target_os = "windows"))]
                        {
                            actions
                        }
                    }),
            )
    }

    #[cfg(target_os = "windows")]
    fn render_window_controls(&self, window: &Window) -> Div {
        let colors = self.theme.colors;
        let maximized = window.is_maximized();
        div()
            .h(px(TITLEBAR_HEIGHT))
            .flex_none()
            .flex()
            .items_center()
            .children(
                [
                    WindowControlKind::Minimize,
                    WindowControlKind::Maximize,
                    WindowControlKind::Close,
                ]
                .map(|kind| {
                    let control = window_control_config(kind, maximized);
                    window_control_button(
                        control.id,
                        control.icon,
                        control.label,
                        control.area,
                        colors,
                        control.danger,
                    )
                }),
            )
    }

    fn render_add_button(&self, cx: &mut Context<Self>) -> Stateful<Div> {
        let colors = self.theme.colors;
        let enabled = self.snapshot.commands_available() && !self.add_dialog.open;
        Button::new("open-add-download", "Add")
            .icon(IconName::Plus)
            .aria_label(if enabled {
                "Add a URL or magnet download"
            } else {
                "Add download unavailable"
            })
            .tooltip(Tooltip::new("Add download").meta("Ctrl/Cmd+N"))
            .style(ButtonStyle::Primary)
            .disabled(!enabled)
            .on_click(cx.listener(|this, _, window, cx| {
                this.open_add_download(&OpenAddDownload, window, cx);
            }))
            .render(colors)
    }

    fn render_sidebar(&mut self, cx: &mut Context<Self>) -> Div {
        let colors = self.theme.colors;
        let mut filters = Vec::with_capacity(WorkspaceFilter::ALL.len());
        for filter in WorkspaceFilter::ALL {
            let count = filter.count(self.snapshot.counts);
            let selected = self.page == AppPage::Downloads && self.query.filter == filter;
            let icon = filter_icon(filter);
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
                    .h(px(38.0))
                    .w_full()
                    .px_3()
                    .flex()
                    .items_center()
                    .gap_3()
                    .rounded_md()
                    .text_xs()
                    .text_color(if selected {
                        colors.accent
                    } else {
                        colors.text_secondary
                    })
                    .when(selected, |element| {
                        element.bg(with_alpha(colors.accent, 0.09))
                    })
                    .when(!selected, |element| {
                        element.hover(|style| style.bg(colors.surface_hover))
                    })
                    .focus_visible(|style| style.border_1().border_color(colors.focus_ring))
                    .cursor_pointer()
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.set_filter(filter, window, cx);
                    }))
                    .child(Icon::new(icon).size(IconSize::Small).color(if selected {
                        colors.accent
                    } else {
                        colors.text_muted
                    }))
                    .child(div().flex_1().child(filter.short_label()))
                    .child(
                        div()
                            .h(px(22.0))
                            .min_w(px(22.0))
                            .px_1()
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded_full()
                            .bg(if selected {
                                with_alpha(colors.accent, 0.12)
                            } else {
                                colors.surface_active
                            })
                            .font_features(tabular_numbers())
                            .text_xs()
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(if selected {
                                colors.accent
                            } else {
                                colors.text_muted
                            })
                            .child(count.to_string()),
                    ),
            );
        }

        div()
            .w(px(SIDEBAR_WIDTH))
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
                    .id("open-settings")
                    .focusable()
                    .tab_stop(true)
                    .role(Role::Button)
                    .aria_label("Open application settings")
                    .h(px(38.0))
                    .w_full()
                    .px_3()
                    .flex()
                    .items_center()
                    .gap_3()
                    .rounded_md()
                    .text_xs()
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(if self.page == AppPage::Settings {
                        colors.accent
                    } else {
                        colors.text_secondary
                    })
                    .when(self.page == AppPage::Settings, |element| {
                        element.bg(with_alpha(colors.accent, 0.09))
                    })
                    .cursor_pointer()
                    .hover(|style| style.bg(colors.surface_hover))
                    .focus_visible(|style| style.border_1().border_color(colors.focus_ring))
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.open_settings(&OpenSettings, window, cx);
                    }))
                    .child(Icon::new(IconName::Settings).size(IconSize::Small).color(
                        if self.page == AppPage::Settings {
                            colors.accent
                        } else {
                            colors.text_muted
                        },
                    ))
                    .child("Settings"),
            )
    }

    fn render_speed_chart(&self) -> Stateful<Div> {
        let colors = self.theme.colors;
        let visible = speed_chart_window(&self.snapshot.speed_history);
        let max_rate = visible
            .iter()
            .map(|sample| sample.download_rate.max(sample.upload_rate))
            .max()
            .unwrap_or(0);
        let scale = max_rate.max(1) as f32;
        let mut columns = Vec::with_capacity(SPEED_CHART_SAMPLES);
        columns.extend(
            (visible.len()..SPEED_CHART_SAMPLES).map(|_| speed_chart_column(0.0, 0.0, colors)),
        );
        columns.extend(visible.iter().map(|sample| {
            speed_chart_column(
                sample.download_rate as f32 / scale,
                sample.upload_rate as f32 / scale,
                colors,
            )
        }));

        div()
            .id("speed-history-chart")
            .role(Role::Group)
            .aria_label(format!(
                "Transfer speed for the last minute, current download {}, current upload {}, peak {}",
                format_rate(self.snapshot.download_rate),
                format_rate(self.snapshot.upload_rate),
                format_rate(max_rate)
            ))
            .h(px(144.0))
            .w(px(280.0))
            .flex_none()
            .flex()
            .flex_col()
                    .gap_2()
                    .p_3()
                    .rounded_md()
            .border_1()
            .border_color(colors.border_strong)
            .bg(colors.elevated_surface)
            .child(
                div()
                    .flex()
                    .items_baseline()
                    .justify_between()
                    .child(
                        div()
                            .text_xs()
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(colors.text_secondary)
                            .child("Last minute"),
                    )
                    .child(
                        div()
                            .font_features(tabular_numbers())
                            .text_xs()
                            .text_color(colors.text_muted)
                            .child(format_rate(max_rate)),
                    ),
            )
            .child(
                div()
                    .h(px(58.0))
                    .w_full()
                    .flex_none()
                    .flex()
                    .items_end()
                    .children(columns),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_3()
                    .text_xs()
                    .text_color(colors.text_muted)
                    .child(speed_chart_legend("Down", colors.progress_download, colors))
                    .child(speed_chart_legend("Up", colors.progress_upload, colors)),
            )
    }

    fn render_task_header(&self, layout: TaskLayoutMode) -> Div {
        let colors = self.theme.colors;
        let header = div()
            .h(px(36.0))
            .flex_none()
            .flex()
            .items_center()
            .gap_3()
            .px_3()
            .border_b_1()
            .border_color(colors.border)
            .bg(colors.toolbar_surface)
            .text_xs()
            .font_weight(FontWeight::MEDIUM)
            .text_color(colors.text_muted)
            .child(div().w(px(32.0)).flex_none());

        match layout {
            TaskLayoutMode::Wide => header
                .child(div().flex_1().min_w_0().child("Name"))
                .child(div().w(px(132.0)).flex_none().child("Progress"))
                .child(div().w(px(88.0)).flex_none().child("Speed"))
                .child(div().w(px(124.0)).flex_none().child("Size"))
                .child(div().w(px(72.0)).flex_none().child("ETA"))
                .child(div().w(px(86.0)).flex_none().text_center().child("Status")),
            TaskLayoutMode::Compact => header
                .child(div().flex_1().min_w_0().child("Task"))
                .child(div().w(px(112.0)).flex_none().child("Progress"))
                .child(div().w(px(78.0)).flex_none().text_center().child("Status")),
        }
    }

    fn render_main(&mut self, layout: TaskLayoutMode, cx: &mut Context<Self>) -> Div {
        let colors = self.theme.colors;
        let task_count = self.snapshot.tasks.len();
        let content =
            if task_count == 0 {
                self.render_empty_state(cx)
            } else {
                div()
                    .id("download-task-list")
                    .role(Role::List)
                    .aria_label(format!("Downloads, {task_count} visible tasks"))
                    .size_full()
                    .child(
                        uniform_list(
                            "download-tasks",
                            task_count,
                            cx.processor(move |this, range: Range<usize>, _window, cx| {
                                this.rendered_range = range.clone();
                                range
                                    .filter_map(|index| {
                                        this.snapshot.tasks.get(index).cloned().map(|task| {
                                            this.render_task_row(index, task, layout, cx)
                                        })
                                    })
                                    .collect::<Vec<_>>()
                            }),
                        )
                        .track_scroll(&self.list_scroll)
                        .h_full()
                        .w_full(),
                    )
                    .into_any_element()
            };

        let center = div()
            .flex_1()
            .min_w_0()
            .min_h_0()
            .flex()
            .flex_col()
            .bg(colors.background)
            .child(
                div()
                    .h(px(52.0))
                    .flex_none()
                    .flex()
                    .items_center()
                    .justify_between()
                    .px_4()
                    .bg(colors.toolbar_surface)
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
                    .child(self.render_task_toolbar(cx)),
            )
            .child(self.render_task_header(layout))
            .child(div().flex_1().min_h_0().child(content));

        div()
            .flex_1()
            .min_w_0()
            .min_h_0()
            .flex()
            .child(center)
            .when(self.details_drawer.is_some(), |element| {
                element.child(self.render_task_details_drawer(cx))
            })
    }

    fn render_status_bar(&mut self, cx: &mut Context<Self>) -> Div {
        let colors = self.theme.colors;
        let connection_color = connection_color(&self.snapshot.connection, colors);
        let connection_label = match &self.snapshot.connection {
            ConnectionView::Reconnecting { attempt } => format!("Reconnecting · {attempt}"),
            connection => connection.label().to_owned(),
        };
        let status_button = div()
            .id("connection-status")
            .role(if self.snapshot.connection.can_retry() {
                Role::Button
            } else {
                Role::Status
            })
            .aria_label(if self.snapshot.connection.can_retry() {
                "Retry aria2 connection".to_owned()
            } else {
                format!("Connection status: {connection_label}")
            })
            .h_full()
            .px_2()
            .flex()
            .items_center()
            .gap_1()
            .text_xs()
            .text_color(colors.text_muted)
            .child(StatusIndicator::new(connection_color))
            .child(connection_label)
            .when(self.snapshot.connection.can_retry(), |element| {
                element
                    .focusable()
                    .tab_stop(true)
                    .cursor_pointer()
                    .hover(|style| style.bg(colors.surface_hover))
                    .on_click(cx.listener(|this, _, _, cx| this.request_retry(cx)))
            });

        div()
            .h(px(28.0))
            .flex_none()
            .flex()
            .items_center()
            .border_t_1()
            .border_color(colors.border)
            .bg(colors.toolbar_surface)
            .child(status_button)
            .child(
                div()
                    .id("engine-status")
                    .role(Role::Status)
                    .aria_label(self.engine_health.label())
                    .h_full()
                    .px_2()
                    .flex()
                    .items_center()
                    .gap_1()
                    .text_xs()
                    .text_color(colors.text_muted)
                    .child(StatusIndicator::new(engine_health_color(
                        &self.engine_health,
                        colors,
                    )))
                    .child(self.engine_health.label()),
            )
            .when(self.snapshot.stale, |element| {
                element.child(
                    div()
                        .id("stale-status")
                        .role(Role::Status)
                        .h_full()
                        .px_2()
                        .flex()
                        .items_center()
                        .gap_1()
                        .text_xs()
                        .text_color(colors.warning)
                        .child(
                            Icon::new(IconName::TriangleAlert)
                                .size(IconSize::XSmall)
                                .color(colors.warning),
                        )
                        .child("Last known data"),
                )
            })
            .child(
                div()
                    .id("transfer-status")
                    .focusable()
                    .tab_stop(true)
                    .role(Role::Button)
                    .aria_label(format!(
                        "Transfer speed, download {}, upload {}; show last minute chart",
                        format_rate(self.snapshot.download_rate),
                        format_rate(self.snapshot.upload_rate)
                    ))
                    .aria_expanded(self.speed_popover_open)
                    .ml_auto()
                    .h_full()
                    .px_2()
                    .flex()
                    .items_center()
                    .gap_2()
                    .font_features(tabular_numbers())
                    .text_xs()
                    .text_color(colors.text_muted)
                    .cursor_pointer()
                    .hover(|style| style.bg(colors.surface_hover))
                    .focus_visible(|style| style.bg(colors.surface_active))
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.toggle_speed_popover(window, cx);
                    }))
                    .child(
                        Icon::new(IconName::ArrowDown)
                            .size(IconSize::XSmall)
                            .color(colors.progress_download),
                    )
                    .child(format_rate(self.snapshot.download_rate))
                    .child(
                        Icon::new(IconName::ArrowUp)
                            .size(IconSize::XSmall)
                            .color(colors.progress_upload),
                    )
                    .child(format_rate(self.snapshot.upload_rate)),
            )
    }

    fn render_speed_popover(&self, cx: &mut Context<Self>) -> Stateful<Div> {
        let colors = self.theme.colors;
        div()
            .id("speed-popover-layer")
            .absolute()
            .inset_0()
            .occlude()
            .on_click(cx.listener(|this, _, window, cx| {
                this.close_speed_popover(window, cx);
            }))
            .child(
                div()
                    .id("speed-popover")
                    .absolute()
                    .right(px(8.0))
                    .bottom(px(32.0))
                    .on_click(|_, _, cx| cx.stop_propagation())
                    .bg(colors.elevated_surface)
                    .child(self.render_speed_chart()),
            )
    }

    fn render_task_toolbar(&mut self, cx: &mut Context<Self>) -> Div {
        let colors = self.theme.colors;
        let Some(task) = self.selected_task_view() else {
            return div();
        };
        let idle = self.pending_task_command.is_none() && self.remove_confirmation.is_none();
        let pending_command = self
            .pending_task_command
            .as_ref()
            .map(|pending| pending.command);
        let commands_available = self.snapshot.commands_available() && idle;
        let details_enabled = self.snapshot.commands_available();
        let pause_enabled = commands_available && task.status.can_pause();
        let resume_enabled = commands_available && task.status.can_resume();
        let retry_enabled = commands_available && task.status.can_retry();
        let remove_enabled = commands_available && task.status.can_remove();

        div()
            .flex()
            .items_center()
            .gap_1()
            .child(
                toolbar_icon_button(
                    "task-details-action",
                    IconName::PanelRight,
                    "Details",
                    ToolbarButtonState::from_flags(details_enabled, false),
                    false,
                    Some("Enter"),
                    colors,
                )
                .when(details_enabled, |button| {
                    button.on_click(cx.listener(|this, _, _window, cx| {
                        if let Some(task) = this.selected_task_view() {
                            this.open_details_for(task, cx);
                        }
                    }))
                }),
            )
            .when(task.status.can_pause(), |element| {
                element.child(
                    toolbar_icon_button(
                        "pause-task-action",
                        IconName::Pause,
                        "Pause",
                        ToolbarButtonState::from_flags(
                            pause_enabled,
                            pending_command == Some(TaskCommandView::Pause),
                        ),
                        false,
                        Some("Cmd+Shift+P"),
                        colors,
                    )
                    .when(pause_enabled, |button| {
                        button.on_click(cx.listener(|this, _, _window, cx| {
                            this.begin_task_command(TaskCommandView::Pause, cx);
                        }))
                    }),
                )
            })
            .when(task.status.can_resume(), |element| {
                element.child(
                    toolbar_icon_button(
                        "resume-task-action",
                        IconName::Play,
                        "Resume",
                        ToolbarButtonState::from_flags(
                            resume_enabled,
                            pending_command == Some(TaskCommandView::Resume),
                        ),
                        false,
                        Some("Cmd+Shift+R"),
                        colors,
                    )
                    .when(resume_enabled, |button| {
                        button.on_click(cx.listener(|this, _, _window, cx| {
                            this.begin_task_command(TaskCommandView::Resume, cx);
                        }))
                    }),
                )
            })
            .when(task.status.can_retry(), |element| {
                element.child(
                    toolbar_icon_button(
                        "retry-task-action",
                        IconName::RotateCcw,
                        "Retry",
                        ToolbarButtonState::from_flags(
                            retry_enabled,
                            pending_command == Some(TaskCommandView::Retry),
                        ),
                        false,
                        Some("Cmd+Alt+R"),
                        colors,
                    )
                    .when(retry_enabled, |button| {
                        button.on_click(cx.listener(|this, _, _window, cx| {
                            this.begin_task_command(TaskCommandView::Retry, cx);
                        }))
                    }),
                )
            })
            .child(
                toolbar_icon_button(
                    "remove-task-action",
                    IconName::Trash2,
                    "Remove",
                    ToolbarButtonState::from_flags(
                        remove_enabled,
                        pending_command == Some(TaskCommandView::RemoveTask),
                    ),
                    true,
                    Some("Delete"),
                    colors,
                )
                .when(remove_enabled, |button| {
                    button.on_click(cx.listener(|this, _, window, cx| {
                        this.confirm_remove_selected(window, cx);
                    }))
                }),
            )
    }

    fn render_task_details_drawer(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let colors = self.theme.colors;
        let Some(drawer) = self.details_drawer.as_ref() else {
            return div().into_any_element();
        };
        let identity = drawer.identity.clone();
        let overview = drawer.overview.clone();
        let overview_progress = overview.progress_basis_points();
        let presentation = match &drawer.state {
            TaskDetailsLoadState::Loading { .. } => TaskDetailsPresentation::Loading,
            TaskDetailsLoadState::Ready { details } => TaskDetailsPresentation::Ready {
                directory: details.directory.clone(),
                info_hash: details.info_hash.clone(),
                piece_length: details.piece_length,
                piece_count: details.piece_count,
                file_count: details.files.len(),
            },
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
            TaskDetailsPresentation::Ready {
                directory,
                info_hash,
                piece_length,
                piece_count,
                file_count,
            } => {
                let gid = identity.gid.clone();
                let files = if file_count == 0 {
                    div()
                        .flex_1()
                        .min_h_0()
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_xs()
                        .text_color(colors.text_muted)
                        .child("No files reported by aria2.")
                        .into_any_element()
                } else {
                    let list_id = SharedString::from(format!("task-files:{}", identity.gid));
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
                                cx.processor(move |this, range: Range<usize>, _window, _cx| {
                                    let colors = this.theme.colors;
                                    let Some(drawer) = &mut this.details_drawer else {
                                        return Vec::new();
                                    };
                                    drawer.rendered_file_range = range.clone();
                                    let TaskDetailsLoadState::Ready { details } = &drawer.state
                                    else {
                                        return Vec::new();
                                    };
                                    let gid = drawer.identity.gid.clone();
                                    range
                                        .filter_map(|index| {
                                            details.files.get(index).cloned().map(|file| {
                                                render_file_row(
                                                    &gid, index, file, file_count, colors,
                                                )
                                            })
                                        })
                                        .collect::<Vec<_>>()
                                }),
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
                };

                div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .h(px(34.0))
                            .flex_none()
                            .flex()
                            .items_center()
                            .px_4()
                            .border_b_1()
                            .border_color(colors.border)
                            .bg(colors.toolbar_surface)
                            .text_xs()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(colors.text_secondary)
                            .child("Details"),
                    )
                    .child(
                        div()
                            .flex_none()
                            .flex()
                            .flex_col()
                            .gap_2()
                            .p_4()
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
                                "Directory",
                                directory.as_deref().unwrap_or("Not reported"),
                                colors,
                            ))
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
                            }),
                    )
                    .child(
                        div()
                            .h(px(42.0))
                            .flex_none()
                            .flex()
                            .items_center()
                            .justify_between()
                            .px_4()
                            .border_t_1()
                            .border_b_1()
                            .border_color(colors.border)
                            .bg(colors.toolbar_surface)
                            .child(
                                div()
                                    .text_xs()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(colors.text_secondary)
                                    .child("Files"),
                            )
                            .child(
                                div()
                                    .h(px(22.0))
                                    .min_w(px(22.0))
                                    .px_1()
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .rounded_full()
                                    .bg(colors.surface_active)
                                    .font_features(tabular_numbers())
                                    .text_xs()
                                    .text_color(colors.text_muted)
                                    .child(file_count.to_string()),
                            ),
                    )
                    .child(files)
                    .into_any_element()
            }
        };

        div()
            .id("task-details-drawer")
            .role(Role::Complementary)
            .aria_label(format!("Task details for {}", overview.display_name))
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
                                    .child(overview.display_name.clone()),
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

    fn render_task_row(
        &mut self,
        index: usize,
        task: DownloadRowView,
        layout: TaskLayoutMode,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let colors = self.theme.colors;
        let selected = self.selected.as_ref() == Some(&task.identity);
        let stable_id = SharedString::from(format!(
            "task-row:{}:{}",
            task.identity.profile_id, task.identity.gid
        ));
        let task_count = self.snapshot.tasks.len();
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
        let progress_label = format_percent(basis_points);
        let rate_label = format_rate(task.download_rate);
        let eta_label = format_eta(task.eta_seconds);
        let status_badge = task_status_badge(task.status, colors);
        let row = div()
            .id(stable_id)
            .role(Role::ListItem)
            .aria_label(aria_label)
            .aria_selected(selected)
            .aria_position_in_set(index + 1)
            .aria_size_of_set(task_count)
            .when(selected, |row| row.aria_active_descendant())
            .h(px(TASK_ROW_HEIGHT))
            .w_full()
            .flex_none()
            .flex()
            .items_center()
            .gap_3()
            .px_3()
            .border_b_1()
            .border_color(colors.border)
            .bg(if selected {
                with_alpha(colors.accent, 0.07)
            } else {
                colors.background
            })
            .when(selected, |row| {
                row.border_1().border_color(with_alpha(colors.accent, 0.72))
            })
            .hover(|style| style.bg(colors.surface_hover))
            .cursor_pointer()
            .on_click(cx.listener(move |this, _, window, cx| {
                this.select_at(index, window, cx);
            }))
            .child(
                div()
                    .size(px(32.0))
                    .flex_none()
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded_md()
                    .border_1()
                    .border_color(colors.border)
                    .bg(colors.elevated_surface)
                    .child(
                        Icon::new(task_status_icon(task.status))
                            .size(IconSize::Small)
                            .color(status_color),
                    ),
            );

        match layout {
            TaskLayoutMode::Wide => row
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
                                .font_weight(FontWeight::MEDIUM)
                                .child(task.display_name),
                        )
                        .child(
                            div()
                                .truncate()
                                .font_features(tabular_numbers())
                                .text_xs()
                                .text_color(colors.text_muted)
                                .child(format!("GID {}", task.identity.gid)),
                        ),
                )
                .child(
                    div()
                        .w(px(132.0))
                        .flex_none()
                        .flex()
                        .flex_col()
                        .gap_2()
                        .font_features(tabular_numbers())
                        .text_xs()
                        .text_color(colors.text_secondary)
                        .child(progress_label)
                        .child(task_progress_bar(progress, task.status, colors)),
                )
                .child(task_table_value(88.0, rate_label, colors))
                .child(task_table_value(124.0, size_label, colors))
                .child(task_table_value(72.0, eta_label, colors))
                .child(
                    div()
                        .w(px(86.0))
                        .flex_none()
                        .flex()
                        .justify_center()
                        .child(status_badge),
                ),
            TaskLayoutMode::Compact => row
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
                                .font_weight(FontWeight::MEDIUM)
                                .child(task.display_name),
                        )
                        .child(
                            div()
                                .truncate()
                                .font_features(tabular_numbers())
                                .text_xs()
                                .text_color(colors.text_muted)
                                .child(format!("{size_label} · {rate_label} · {eta_label}")),
                        ),
                )
                .child(
                    div()
                        .w(px(112.0))
                        .flex_none()
                        .flex()
                        .flex_col()
                        .gap_2()
                        .font_features(tabular_numbers())
                        .text_xs()
                        .text_color(colors.text_secondary)
                        .child(progress_label)
                        .child(task_progress_bar(progress, task.status, colors)),
                )
                .child(
                    div()
                        .w(px(78.0))
                        .flex_none()
                        .flex()
                        .justify_center()
                        .child(status_badge),
                ),
        }
    }

    fn render_add_download_dialog(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let colors = self.theme.colors;
        let pending = self.add_dialog.pending.is_some();
        let error = self.add_dialog.error.clone();
        let content = div()
            .flex()
            .flex_col()
            .gap_2()
            .child(
                div()
                    .text_xs()
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(colors.text_secondary)
                    .child("URL or magnet link"),
            )
            .child(self.add_input.clone())
            .when_some(error, |element, error| {
                element.child(
                    div()
                        .id("add-download-error")
                        .role(Role::Alert)
                        .aria_label(error.summary.clone())
                        .text_xs()
                        .text_color(colors.danger)
                        .child(error.summary),
                )
            });

        Dialog::new("add-download-dialog", "Add download", self.theme)
            .key_context("AddDownloadDialog")
            .track_focus(self.add_dialog_focus.clone())
            .width(560.0)
            .child(content)
            .action(
                Button::new("cancel-add-download", "Cancel")
                    .aria_label("Cancel adding a download")
                    .style(ButtonStyle::Secondary)
                    .disabled(pending)
                    .track_focus(self.add_cancel_focus.clone())
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.close_add_download(window, cx);
                    }))
                    .render(colors),
            )
            .action(
                Button::new("submit-add-download", "Add")
                    .aria_label(if pending {
                        "Adding download"
                    } else {
                        "Add download"
                    })
                    .style(ButtonStyle::Primary)
                    .loading(pending)
                    .track_focus(self.add_submit_focus.clone())
                    .on_click(cx.listener(|this, _, _window, cx| {
                        this.submit_add_download(cx);
                    }))
                    .render(colors),
            )
            .into_any_element()
    }

    fn render_settings_page(&mut self, cx: &mut Context<Self>) -> Stateful<Div> {
        let colors = self.theme.colors;
        let pending = self.pending_settings_save.is_some();
        let directory_saving = self
            .pending_settings_save
            .as_ref()
            .is_some_and(|pending| pending.source == SettingsSaveSource::Directory);
        let error = self.settings_page.error.clone();
        let draft_scheme = self.settings_page.draft_color_scheme;
        let directory_dirty = self.settings_directory_input.read(cx).text().trim()
            != self.settings.download_directory;
        let selected_scheme = usize::from(draft_scheme == ColorSchemeView::Dark);
        let shell = cx.entity().downgrade();
        let scheme_control = SegmentedControl::new(
            "settings-theme",
            [
                Segment::new("Light").icon(IconName::Sun),
                Segment::new("Dark").icon(IconName::Moon),
            ],
            selected_scheme,
            self.theme,
        )
        .disabled(pending)
        .on_select(move |index, _window, cx| {
            let scheme = if index == 0 {
                ColorSchemeView::Light
            } else {
                ColorSchemeView::Dark
            };
            shell
                .update(cx, |shell, cx| shell.select_color_scheme(scheme, cx))
                .ok();
        });

        div()
            .id("settings-page")
            .key_context("SettingsPage")
            .role(Role::Main)
            .aria_label("Application settings")
            .size_full()
            .flex()
            .flex_col()
            .bg(colors.background)
            .child(
                div()
                    .h(px(44.0))
                    .flex_none()
                    .flex()
                    .items_center()
                    .px_4()
                    .bg(colors.toolbar_surface)
                    .child(
                        div()
                            .text_base()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child("Settings"),
                    ),
            )
            .child(
                div().flex_1().min_h_0().px_6().py_5().child(
                    div()
                        .max_w(px(720.0))
                        .flex()
                        .flex_col()
                        .gap_8()
                        .child(
                            settings_section(
                                "Appearance",
                                "Choose the interface color scheme.",
                                colors,
                            )
                            .child(div().mt_3().flex().items_start().child(scheme_control)),
                        )
                        .child(
                            settings_section(
                                "Downloads",
                                "New tasks use this directory unless aria2 overrides it.",
                                colors,
                            )
                            .child(
                                div()
                                    .mt_3()
                                    .max_w(px(620.0))
                                    .flex()
                                    .items_center()
                                    .gap_2()
                                    .child(
                                        div()
                                            .flex_1()
                                            .min_w_0()
                                            .child(self.settings_directory_input.clone()),
                                    )
                                    .child(
                                        Button::new(
                                            "save-settings",
                                            if directory_saving {
                                                "Saving..."
                                            } else {
                                                "Save"
                                            },
                                        )
                                        .aria_label(if directory_saving {
                                            "Saving download directory"
                                        } else {
                                            "Save download directory"
                                        })
                                        .style(ButtonStyle::Primary)
                                        .disabled(pending || !directory_dirty)
                                        .loading(directory_saving)
                                        .track_focus(self.settings_save_focus.clone())
                                        .on_click(
                                            cx.listener(|this, _, _, cx| this.submit_settings(cx)),
                                        )
                                        .render(colors),
                                    ),
                            )
                            .when_some(error, |element, error| {
                                element.child(
                                    div()
                                        .id("settings-error")
                                        .role(Role::Alert)
                                        .aria_label(error.summary.clone())
                                        .mt_2()
                                        .flex()
                                        .items_center()
                                        .gap_1()
                                        .text_xs()
                                        .text_color(colors.danger)
                                        .child(
                                            Icon::new(IconName::CircleAlert)
                                                .size(IconSize::XSmall)
                                                .color(colors.danger),
                                        )
                                        .child(error.summary),
                                )
                            }),
                        ),
                ),
            )
    }

    fn render_remove_confirmation(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let colors = self.theme.colors;
        let display_name = self
            .remove_confirmation
            .as_ref()
            .map(|confirmation| confirmation.display_name.clone())
            .unwrap_or_default();
        Dialog::new("remove-task-dialog", "Remove task?", self.theme)
            .description(format!("{display_name} will be removed from aria2."))
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
                    .child("Downloaded files will be kept."),
            )
            .action(
                Button::new("cancel-remove-task", "Cancel")
                    .aria_label("Cancel task removal")
                    .style(ButtonStyle::Secondary)
                    .track_focus(self.remove_cancel_focus.clone())
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.close_remove_confirmation(window, cx);
                    }))
                    .render(colors),
            )
            .action(
                Button::new("confirm-remove-task", "Remove")
                    .aria_label("Remove task from aria2")
                    .style(ButtonStyle::Danger)
                    .track_focus(self.remove_submit_focus.clone())
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.submit_remove_confirmation(cx);
                    }))
                    .render(colors),
            )
            .into_any_element()
    }

    fn render_toast(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let Some(notice) = self.status_notice.as_ref() else {
            return div().into_any_element();
        };
        let kind = if notice.is_error {
            ToastKind::Error
        } else {
            ToastKind::Success
        };
        div()
            .absolute()
            .right(px(16.0))
            .bottom(px(44.0))
            .child(
                Toast::new("operation-toast", notice.message.clone(), kind, self.theme)
                    .on_close(cx.listener(|this, _, _, cx| this.dismiss_notice(cx))),
            )
            .into_any_element()
    }

    fn render_empty_state(&self, cx: &mut Context<Self>) -> AnyElement {
        let colors = self.theme.colors;
        let (icon, title, show_clear) = match &self.snapshot.connection {
            ConnectionView::Connecting
            | ConnectionView::Authenticating
            | ConnectionView::Synchronizing
            | ConnectionView::Reconnecting { .. }
                if self.snapshot.tasks.is_empty() =>
            {
                (
                    IconName::LoaderCircle,
                    "Connecting to aria2".to_owned(),
                    false,
                )
            }
            ConnectionView::Failed { .. } => {
                (IconName::CloudOff, "Connection failed".to_owned(), false)
            }
            ConnectionView::Disconnected if self.snapshot.tasks.is_empty() => {
                (IconName::CloudOff, "aria2 is unavailable".to_owned(), false)
            }
            _ if !self.query.search.trim().is_empty() => {
                (IconName::SearchX, "No matching downloads".to_owned(), true)
            }
            _ if self.query.filter != WorkspaceFilter::All => (
                IconName::Inbox,
                format!(
                    "No {} tasks",
                    self.query.filter.short_label().to_lowercase()
                ),
                true,
            ),
            _ => (IconName::Inbox, "No downloads".to_owned(), false),
        };
        let show_add = self.query.filter == WorkspaceFilter::All
            && self.query.search.trim().is_empty()
            && self.snapshot.commands_available()
            && !self.add_dialog.open;

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
                    .gap_3()
                    .text_center()
                    .child(
                        div()
                            .size(px(48.0))
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded_md()
                            .border_1()
                            .border_color(colors.border)
                            .bg(colors.elevated_surface)
                            .child(
                                Icon::new(icon)
                                    .size(IconSize::Large)
                                    .color(colors.text_muted),
                            ),
                    )
                    .child(
                        div()
                            .text_base()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(title),
                    )
                    .when(show_clear, |element| {
                        element.child(
                            Button::new("clear-empty-filter", "Clear filter")
                                .aria_label("Clear search and task filter")
                                .style(ButtonStyle::Secondary)
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.query.filter = WorkspaceFilter::All;
                                    this.search_input
                                        .update(cx, |input, cx| input.set_text("", cx));
                                    window.focus(&this.focus_handle, cx);
                                    this.emit_query(cx);
                                }))
                                .render(colors),
                        )
                    })
                    .when(show_add, |element| {
                        element.child(
                            Button::new("add-download-empty-state", "Add download")
                                .icon(IconName::Plus)
                                .aria_label("Add a URL or magnet download")
                                .style(ButtonStyle::Primary)
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.open_add_download(&OpenAddDownload, window, cx);
                                }))
                                .render(colors),
                        )
                    })
                    .when(self.snapshot.connection.can_retry(), |element| {
                        element.child(
                            Button::new("retry-connection", "Retry")
                                .aria_label("Retry aria2 connection now")
                                .style(ButtonStyle::Primary)
                                .on_click(cx.listener(|this, _, _, cx| this.request_retry(cx)))
                                .render(colors),
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
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = self.theme.colors;
        let task_layout = task_layout_mode(
            f32::from(window.viewport_size().width),
            self.details_drawer.is_some(),
        );
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
            .on_action(cx.listener(Self::open_add_download))
            .on_action(cx.listener(Self::close_add_download_action))
            .on_action(cx.listener(Self::submit_add_download_action))
            .on_action(cx.listener(Self::open_settings))
            .on_action(cx.listener(Self::close_settings_action))
            .on_action(cx.listener(Self::save_settings_action))
            .on_action(cx.listener(Self::open_task_details_action))
            .on_action(cx.listener(Self::pause_selected))
            .on_action(cx.listener(Self::resume_selected))
            .on_action(cx.listener(Self::retry_selected))
            .on_action(cx.listener(Self::remove_selected))
            .on_action(cx.listener(Self::focus_next))
            .on_action(cx.listener(Self::focus_previous))
            .relative()
            .size_full()
            .flex()
            .flex_col()
            .bg(colors.background)
            .text_color(colors.text_primary)
            .child(self.render_header(window, cx))
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .child(self.render_sidebar(cx))
                    .child(match self.page {
                        AppPage::Downloads => self.render_main(task_layout, cx).into_any_element(),
                        AppPage::Settings => self.render_settings_page(cx).into_any_element(),
                    }),
            )
            .child(self.render_status_bar(cx))
            .when(self.add_dialog.open, |element| {
                element.child(self.render_add_download_dialog(cx))
            })
            .when(self.remove_confirmation.is_some(), |element| {
                element.child(self.render_remove_confirmation(cx))
            })
            .when(self.speed_popover_open, |element| {
                element.child(self.render_speed_popover(cx))
            })
            .when(self.status_notice.is_some(), |element| {
                element.child(self.render_toast(cx))
            })
    }
}

fn titlebar_drag_region() -> Div {
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

fn theme_for_scheme(scheme: ColorSchemeView) -> Theme {
    match scheme {
        ColorSchemeView::Light => Theme::light(),
        ColorSchemeView::Dark => Theme::dark(),
    }
}

#[cfg(target_os = "windows")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum WindowControlKind {
    Minimize,
    Maximize,
    Close,
}

#[cfg(target_os = "windows")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct WindowControlConfig {
    id: &'static str,
    icon: IconName,
    label: &'static str,
    area: WindowControlArea,
    danger: bool,
}

#[cfg(target_os = "windows")]
fn window_control_config(kind: WindowControlKind, maximized: bool) -> WindowControlConfig {
    match kind {
        WindowControlKind::Minimize => WindowControlConfig {
            id: "window-minimize",
            icon: IconName::Minus,
            label: "Minimize window",
            area: WindowControlArea::Min,
            danger: false,
        },
        WindowControlKind::Maximize => WindowControlConfig {
            id: "window-maximize",
            icon: if maximized {
                IconName::Copy
            } else {
                IconName::Square
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
            icon: IconName::X,
            label: "Close window",
            area: WindowControlArea::Close,
            danger: true,
        },
    }
}

#[cfg(target_os = "windows")]
fn window_control_button(
    id: &'static str,
    icon: IconName,
    label: &'static str,
    area: WindowControlArea,
    colors: crate::ThemeColors,
    danger: bool,
) -> Stateful<Div> {
    let button = IconButton::new(id, icon)
        .aria_label(label)
        .tooltip(Tooltip::new(label));
    let button = if danger {
        button
            .hover_background(colors.danger)
            .active_background(colors.danger)
    } else {
        button
    };
    button
        .render(colors)
        .h(px(TITLEBAR_HEIGHT))
        .w(px(46.0))
        .min_w(px(46.0))
        .px_0()
        .rounded_none()
        .window_control_area(area)
}

fn speed_chart_column(download_height: f32, upload_height: f32, colors: crate::ThemeColors) -> Div {
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

fn speed_chart_window(history: &[SpeedSampleView]) -> &[SpeedSampleView] {
    if history.len() > SPEED_CHART_SAMPLES {
        &history[history.len() - SPEED_CHART_SAMPLES..]
    } else {
        history
    }
}

fn speed_chart_legend(label: &'static str, color: Hsla, colors: crate::ThemeColors) -> Div {
    div()
        .flex()
        .items_center()
        .gap_1()
        .child(div().size(px(6.0)).rounded_sm().bg(color))
        .child(div().text_color(colors.text_muted).child(label))
}

fn toolbar_icon_button(
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ToolbarButtonState {
    Enabled,
    Disabled,
    Loading,
}

impl ToolbarButtonState {
    fn from_flags(enabled: bool, loading: bool) -> Self {
        if loading {
            Self::Loading
        } else if enabled {
            Self::Enabled
        } else {
            Self::Disabled
        }
    }
}

fn settings_section(title: &'static str, detail: &'static str, colors: crate::ThemeColors) -> Div {
    div()
        .flex()
        .flex_col()
        .child(
            div()
                .text_sm()
                .font_weight(FontWeight::SEMIBOLD)
                .child(title),
        )
        .child(
            div()
                .mt_1()
                .text_xs()
                .text_color(colors.text_muted)
                .child(detail),
        )
}

fn filter_icon(filter: WorkspaceFilter) -> IconName {
    match filter {
        WorkspaceFilter::All => IconName::List,
        WorkspaceFilter::Active => IconName::Activity,
        WorkspaceFilter::Waiting => IconName::Clock3,
        WorkspaceFilter::Paused => IconName::Pause,
        WorkspaceFilter::Completed => IconName::CircleCheck,
        WorkspaceFilter::Failed => IconName::CircleAlert,
    }
}

fn task_status_icon(status: TaskStatusView) -> IconName {
    match status {
        TaskStatusView::Active => IconName::Activity,
        TaskStatusView::Waiting => IconName::Clock3,
        TaskStatusView::Paused => IconName::Pause,
        TaskStatusView::Complete => IconName::CircleCheck,
        TaskStatusView::Failed => IconName::CircleAlert,
        TaskStatusView::Verifying => IconName::ScanSearch,
        TaskStatusView::Removed => IconName::Trash2,
        TaskStatusView::Unknown => IconName::CircleHelp,
    }
}

fn task_overview_summary(task: &DownloadRowView, colors: crate::ThemeColors) -> Div {
    let basis_points = task.progress_basis_points();
    let progress = f32::from(basis_points.unwrap_or(0)) / 10_000.0;
    let size_label = if task.total_bytes == 0 {
        format_bytes(task.completed_bytes)
    } else {
        format!(
            "{} / {}",
            format_bytes(task.completed_bytes),
            format_bytes(task.total_bytes)
        )
    };
    let eta_label = task.eta_seconds.filter(|seconds| *seconds > 0).map_or_else(
        || task.status.label().to_owned(),
        |seconds| format!("{} remaining", format_eta(Some(seconds))),
    );

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
                        .child("Progress"),
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
                        .child(format_percent(basis_points)),
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
                .child(size_label)
                .child(format_rate(task.download_rate)),
        )
}

fn drawer_message(
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

fn detail_line(
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

fn detail_line_with_action(
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

fn render_file_row(
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

fn task_command_label(command: TaskCommandView) -> &'static str {
    match command {
        TaskCommandView::Pause => "Pause",
        TaskCommandView::Resume => "Resume",
        TaskCommandView::Retry => "Retry",
        TaskCommandView::RemoveTask => "Remove",
    }
}

fn stale_session_error() -> OperationErrorView {
    OperationErrorView {
        code: "command.stale_session".into(),
        summary: "The aria2 session changed. Review current state before submitting again.".into(),
        retryable: false,
    }
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

fn engine_health_color(health: &EngineHealthView, colors: crate::ThemeColors) -> Hsla {
    match health {
        EngineHealthView::External => colors.information,
        EngineHealthView::Running { restarts: 0 } => colors.success,
        EngineHealthView::Running { .. } | EngineHealthView::Restarting { .. } => colors.warning,
        EngineHealthView::Failed { .. } => colors.danger,
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

fn task_progress_bar(progress: f32, status: TaskStatusView, colors: crate::ThemeColors) -> Div {
    let fill = match status {
        TaskStatusView::Failed | TaskStatusView::Removed => colors.danger,
        TaskStatusView::Complete => colors.success,
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

fn task_table_value(width: f32, value: String, colors: crate::ThemeColors) -> Div {
    div()
        .w(px(width))
        .flex_none()
        .truncate()
        .font_features(tabular_numbers())
        .text_xs()
        .text_color(colors.text_secondary)
        .child(value)
}

fn task_status_badge(status: TaskStatusView, colors: crate::ThemeColors) -> Div {
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

fn with_alpha(mut color: Hsla, alpha: f32) -> Hsla {
    color.a = alpha;
    color
}

#[cfg(test)]
mod tests {
    use gpui::{TestAppContext, point, px};

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
            session_id: "session".into(),
            generation: 1,
            source_revision: 1,
            connection: ConnectionView::Connected,
            stale: false,
            download_rate: 0,
            upload_rate: 0,
            speed_history: Vec::new(),
            counts: TaskCountsView {
                all: count,
                completed: count,
                ..TaskCountsView::default()
            },
            tasks: (0..count).map(task).collect(),
        }
    }

    fn details(file_count: usize) -> TaskDetailsView {
        TaskDetailsView {
            directory: Some("C:/downloads".into()),
            info_hash: Some("0123456789abcdef".into()),
            piece_length: Some(1_048_576),
            piece_count: Some(file_count as u32),
            files: (0..file_count)
                .map(|index| TaskFileView {
                    index: index as u32 + 1,
                    path: format!("C:/downloads/file-{index:05}.bin"),
                    length: 1_048_576,
                    completed_length: 524_288,
                    selected: true,
                })
                .collect(),
        }
    }

    #[test]
    fn task_layout_uses_the_remaining_main_pane_width() {
        assert_eq!(task_layout_mode(1_180.0, false), TaskLayoutMode::Wide);
        assert_eq!(task_layout_mode(1_180.0, true), TaskLayoutMode::Compact);
        assert_eq!(task_layout_mode(960.0, false), TaskLayoutMode::Compact);
        assert_eq!(task_layout_mode(1_400.0, true), TaskLayoutMode::Wide);
    }

    #[test]
    fn search_bounds_are_centered_and_ignore_workspace_drawers() {
        for viewport_width in [960.0, 1_180.0, 1_600.0] {
            let (left, right) = centered_search_bounds(viewport_width);
            assert!(((left + right) / 2.0 - viewport_width / 2.0).abs() < f32::EPSILON);
            assert!(right - left <= SEARCH_WIDTH);
        }
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn window_controls_map_to_native_areas_and_accessible_labels() {
        let minimize = window_control_config(WindowControlKind::Minimize, false);
        assert_eq!(minimize.area, WindowControlArea::Min);
        assert_eq!(minimize.icon, IconName::Minus);
        assert_eq!(minimize.label, "Minimize window");
        assert!(!minimize.danger);

        let maximize = window_control_config(WindowControlKind::Maximize, false);
        assert_eq!(maximize.area, WindowControlArea::Max);
        assert_eq!(maximize.icon, IconName::Square);
        assert_eq!(maximize.label, "Maximize window");

        let restore = window_control_config(WindowControlKind::Maximize, true);
        assert_eq!(restore.icon, IconName::Copy);
        assert_eq!(restore.label, "Restore window");

        let close = window_control_config(WindowControlKind::Close, false);
        assert_eq!(close.area, WindowControlArea::Close);
        assert_eq!(close.icon, IconName::X);
        assert_eq!(close.label, "Close window");
        assert!(close.danger);
    }

    #[test]
    fn speed_chart_uses_only_the_latest_bounded_window() {
        let history = (0..=SPEED_CHART_SAMPLES)
            .map(|index| SpeedSampleView {
                download_rate: index as u64,
                upload_rate: 0,
            })
            .collect::<Vec<_>>();

        let visible = speed_chart_window(&history);
        assert_eq!(visible.len(), SPEED_CHART_SAMPLES);
        assert_eq!(visible.first().map(|sample| sample.download_rate), Some(1));
        assert_eq!(
            visible.last().map(|sample| sample.download_rate),
            Some(SPEED_CHART_SAMPLES as u64)
        );
    }

    #[gpui::test]
    fn local_engine_health_surfaces_recovery_and_terminal_failure(cx: &mut TestAppContext) {
        let (view, cx) = cx.add_window_view(|window, cx| AppShell::new(Theme::dark(), window, cx));

        view.update(cx, |shell, cx| {
            shell.set_engine_health(EngineHealthView::Running { restarts: 0 }, cx);
            shell.set_engine_health(EngineHealthView::Restarting { attempt: 1 }, cx);
        });
        view.read_with(cx, |shell, _| {
            assert_eq!(shell.engine_health.label(), "Local engine restarting");
            assert!(
                shell.status_notice.is_none(),
                "persistent restart state belongs in the status bar"
            );
        });

        view.update(cx, |shell, cx| {
            shell.set_engine_health(EngineHealthView::Running { restarts: 1 }, cx);
        });
        view.read_with(cx, |shell, _| {
            let notice = shell.status_notice.as_ref().expect("recovery notice");
            assert!(!notice.is_error);
            assert_eq!(
                notice.message,
                "Local aria2 recovered after 1 restart attempt."
            );
        });

        view.update(cx, |shell, cx| {
            shell.set_engine_health(
                EngineHealthView::Failed {
                    summary: "restart budget exhausted".into(),
                },
                cx,
            );
        });
        view.read_with(cx, |shell, _| {
            let notice = shell.status_notice.as_ref().expect("failure notice");
            assert!(notice.is_error);
            assert_eq!(shell.engine_health.label(), "Local engine stopped");
        });
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

    #[gpui::test]
    fn hidden_selection_arrows_start_at_the_visible_edges(cx: &mut TestAppContext) {
        let (view, cx) = cx.add_window_view(|window, cx| {
            let mut shell = AppShell::new(Theme::dark(), window, cx);
            shell.snapshot = snapshot(3);
            shell.selected = Some(task(99).identity);
            shell
        });

        view.update_in(cx, |shell, window, cx| {
            shell.select_next(&SelectNextTask, window, cx);
        });
        view.read_with(cx, |shell, _| {
            assert_eq!(shell.selected, Some(task(0).identity));
        });

        view.update(cx, |shell, _| {
            shell.selected = Some(task(99).identity);
        });
        view.update_in(cx, |shell, window, cx| {
            shell.select_previous(&SelectPreviousTask, window, cx);
        });
        view.read_with(cx, |shell, _| {
            assert_eq!(shell.selected, Some(task(2).identity));
        });
    }

    #[gpui::test]
    fn add_download_submission_is_single_flight(cx: &mut TestAppContext) {
        let (view, cx) = cx.add_window_view(|window, cx| {
            let mut shell = AppShell::new(Theme::dark(), window, cx);
            shell.snapshot = snapshot(1);
            shell.add_dialog.open = true;
            shell
        });

        view.update(cx, |shell, cx| {
            shell.add_input.update(cx, |input, cx| {
                input.set_text("https://example.com/archive.bin", cx);
            });
            shell.submit_add_download(cx);
            let first = shell
                .add_dialog
                .pending
                .as_ref()
                .expect("first submit must become pending")
                .request_id;
            shell.submit_add_download(cx);
            assert_eq!(
                shell
                    .add_dialog
                    .pending
                    .as_ref()
                    .expect("second submit must retain pending request")
                    .request_id,
                first
            );
            assert_eq!(shell.next_request_id, first.get() + 1);
        });
    }

    #[gpui::test]
    fn add_download_dialog_accepts_keyboard_input(cx: &mut TestAppContext) {
        let (view, cx) = cx.add_window_view(|window, cx| {
            let mut shell = AppShell::new(Theme::dark(), window, cx);
            shell.snapshot = snapshot(1);
            shell
        });

        view.update_in(cx, |shell, window, cx| {
            shell.open_add_download(&OpenAddDownload, window, cx);
        });
        cx.simulate_input("https://example.com/file");

        view.read_with(cx, |shell, cx| {
            assert_eq!(shell.add_input.read(cx).text(), "https://example.com/file");
        });
    }

    #[gpui::test]
    fn add_download_dialog_input_can_be_clicked_before_typing(cx: &mut TestAppContext) {
        let (view, cx) = cx.add_window_view(|window, cx| {
            let mut shell = AppShell::new(Theme::dark(), window, cx);
            shell.snapshot = snapshot(1);
            shell
        });

        view.update_in(cx, |shell, window, cx| {
            shell.open_add_download(&OpenAddDownload, window, cx);
        });
        let input_bounds = view.read_with(cx, |shell, cx| {
            shell
                .add_input
                .read(cx)
                .text_bounds()
                .expect("add-download input must be painted")
        });
        view.update_in(cx, |shell, window, cx| {
            window.focus(&shell.search_input.focus_handle(cx), cx);
        });
        cx.simulate_click(
            point(input_bounds.left() - px(16.0), input_bounds.center().y),
            Default::default(),
        );
        cx.simulate_input("https://example.com/file");

        view.read_with(cx, |shell, cx| {
            assert_eq!(shell.add_input.read(cx).text(), "https://example.com/file");
        });
    }

    #[gpui::test]
    fn add_download_dialog_supports_standard_clipboard_shortcuts(cx: &mut TestAppContext) {
        cx.update(crate::init);
        let (view, cx) = cx.add_window_view(|window, cx| {
            let mut shell = AppShell::new(Theme::dark(), window, cx);
            shell.snapshot = snapshot(1);
            shell
        });

        view.update_in(cx, |shell, window, cx| {
            shell.open_add_download(&OpenAddDownload, window, cx);
            shell.add_input.update(cx, |input, cx| {
                input.set_text("https://example.com/file", cx);
            });
        });
        cx.simulate_keystrokes("secondary-a secondary-c");
        assert_eq!(
            cx.read_from_clipboard().and_then(|item| item.text()),
            Some("https://example.com/file".to_owned())
        );

        cx.write_to_clipboard(ClipboardItem::new_string(
            "magnet:?xt=urn:btih:test".to_owned(),
        ));
        cx.simulate_keystrokes("secondary-v");
        view.read_with(cx, |shell, cx| {
            assert_eq!(shell.add_input.read(cx).text(), "magnet:?xt=urn:btih:test");
        });

        cx.simulate_keystrokes("secondary-a secondary-x");
        view.read_with(cx, |shell, cx| {
            assert!(shell.add_input.read(cx).text().is_empty());
        });
        assert_eq!(
            cx.read_from_clipboard().and_then(|item| item.text()),
            Some("magnet:?xt=urn:btih:test".to_owned())
        );
    }

    #[gpui::test]
    fn successful_retry_selects_the_new_task_identity(cx: &mut TestAppContext) {
        let (view, cx) = cx.add_window_view(|window, cx| {
            let mut shell = AppShell::new(Theme::dark(), window, cx);
            shell.snapshot = snapshot(1);
            shell.snapshot.tasks[0].status = TaskStatusView::Failed;
            shell.selected = Some(shell.snapshot.tasks[0].identity.clone());
            shell
        });
        let (request_id, session, old_identity) = view.update(cx, |shell, cx| {
            shell.begin_task_command(TaskCommandView::Retry, cx);
            let pending = shell
                .pending_task_command
                .as_ref()
                .expect("retry must become pending");
            (
                pending.request_id,
                pending.session.clone(),
                pending.identity.clone(),
            )
        });
        let new_identity = TaskIdentity {
            profile_id: old_identity.profile_id.clone(),
            gid: "0000000000000063".into(),
        };

        view.update(cx, |shell, cx| {
            shell.set_task_command_result(
                TaskCommandResultView {
                    request_id,
                    session,
                    identity: old_identity,
                    command: TaskCommandView::Retry,
                    outcome: CommandOutcomeView::Success {
                        task: Some(new_identity.clone()),
                    },
                },
                cx,
            );
        });
        view.read_with(cx, |shell, _| {
            assert_eq!(shell.selected.as_ref(), Some(&new_identity));
            assert!(shell.pending_task_command.is_none());
            assert!(shell.details_drawer.is_none());
        });
    }

    #[gpui::test]
    fn theme_applies_only_after_the_matching_save_succeeds(cx: &mut TestAppContext) {
        let initial = SettingsView {
            color_scheme: ColorSchemeView::Dark,
            download_directory: "C:/Downloads".into(),
        };
        let expected_initial = initial.clone();
        let (view, cx) =
            cx.add_window_view(move |window, cx| AppShell::new_with_settings(initial, window, cx));
        let (request_id, requested) = view.update(cx, |shell, cx| {
            shell.page = AppPage::Settings;
            shell.select_color_scheme(ColorSchemeView::Light, cx);
            let pending = shell
                .pending_settings_save
                .as_ref()
                .expect("settings save must become pending");
            (pending.request_id, pending.settings.clone())
        });

        view.update_in(cx, |shell, window, cx| {
            shell.set_settings_save_result(
                SettingsSaveResultView {
                    request_id: RequestId::from_u64(request_id.get() + 1),
                    settings: requested.clone(),
                    outcome: SettingsSaveOutcomeView::Success,
                },
                window,
                cx,
            );
        });
        view.read_with(cx, |shell, _| {
            assert_eq!(shell.settings, expected_initial);
            assert!(shell.pending_settings_save.is_some());
        });

        view.update_in(cx, |shell, window, cx| {
            shell.set_settings_save_result(
                SettingsSaveResultView {
                    request_id,
                    settings: requested.clone(),
                    outcome: SettingsSaveOutcomeView::Success,
                },
                window,
                cx,
            );
        });
        view.read_with(cx, |shell, _| {
            assert_eq!(shell.settings, requested);
            assert_eq!(shell.theme.mode, ThemeMode::Light);
            assert!(shell.pending_settings_save.is_none());
            assert_eq!(shell.page, AppPage::Settings);
        });
    }

    #[gpui::test]
    fn stale_details_result_cannot_replace_the_active_request(cx: &mut TestAppContext) {
        let (view, cx) = cx.add_window_view(|window, cx| {
            let mut shell = AppShell::new(Theme::dark(), window, cx);
            shell.snapshot = snapshot(1);
            shell.selected = Some(task(0).identity);
            shell.open_details_for(task(0), cx);
            shell
        });
        let (request_id, session, identity) = view.read_with(cx, |shell, _| {
            let drawer = shell.details_drawer.as_ref().expect("drawer must exist");
            let TaskDetailsLoadState::Loading { request_id } = drawer.state else {
                panic!("drawer must be loading")
            };
            (request_id, drawer.session.clone(), drawer.identity.clone())
        });

        view.update(cx, |shell, cx| {
            shell.set_task_details_result(
                TaskDetailsResultView {
                    request_id: RequestId::from_u64(request_id.get() + 1),
                    session: session.clone(),
                    identity: identity.clone(),
                    outcome: TaskDetailsOutcomeView::Ready(details(1)),
                },
                cx,
            );
        });
        view.read_with(cx, |shell, _| {
            assert!(matches!(
                shell.details_drawer.as_ref().map(|drawer| &drawer.state),
                Some(TaskDetailsLoadState::Loading { request_id: current }) if *current == request_id
            ));
        });

        view.update(cx, |shell, cx| {
            shell.set_task_details_result(
                TaskDetailsResultView {
                    request_id,
                    session,
                    identity,
                    outcome: TaskDetailsOutcomeView::Ready(details(1)),
                },
                cx,
            );
        });
        view.read_with(cx, |shell, _| {
            assert!(matches!(
                shell.details_drawer.as_ref().map(|drawer| &drawer.state),
                Some(TaskDetailsLoadState::Ready { .. })
            ));
        });
    }

    #[gpui::test]
    fn details_drawer_survives_filtering_that_hides_its_task(cx: &mut TestAppContext) {
        let (view, cx) = cx.add_window_view(|window, cx| {
            let mut shell = AppShell::new(Theme::dark(), window, cx);
            shell.snapshot = snapshot(3);
            shell.selected = Some(task(1).identity);
            shell.open_details_for(task(1), cx);
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
            assert_eq!(
                shell.details_drawer.as_ref().map(|drawer| &drawer.identity),
                selected.as_ref()
            );
        });
    }

    #[gpui::test]
    fn ten_thousand_detail_files_render_only_a_viewport_window(cx: &mut TestAppContext) {
        let (view, cx) = cx.add_window_view(|window, cx| {
            let mut shell = AppShell::new(Theme::dark(), window, cx);
            shell.snapshot = snapshot(1);
            let overview = task(0);
            shell.selected = Some(overview.identity.clone());
            shell.details_drawer = Some(TaskDetailsDrawer {
                identity: overview.identity.clone(),
                overview,
                session: shell.snapshot.engine_session().expect("test session"),
                state: TaskDetailsLoadState::Ready {
                    details: details(10_000),
                },
                file_scroll: UniformListScrollHandle::new(),
                rendered_file_range: 0..0,
            });
            shell
        });

        view.read_with(cx, |shell, _| {
            let drawer = shell.details_drawer.as_ref().expect("drawer must exist");
            assert!(!drawer.rendered_file_range.is_empty());
            assert!(
                drawer.rendered_file_range.len() < 64,
                "rendered {} files",
                drawer.rendered_file_range.len()
            );
            let TaskDetailsLoadState::Ready { details } = &drawer.state else {
                panic!("drawer must be ready")
            };
            assert_eq!(details.files.len(), 10_000);
        });
    }

    #[gpui::test]
    fn task_removal_requires_the_matching_internal_confirmation(cx: &mut TestAppContext) {
        let (view, cx) = cx.add_window_view(|window, cx| {
            let mut shell = AppShell::new(Theme::dark(), window, cx);
            shell.snapshot = snapshot(1);
            shell.selected = Some(shell.snapshot.tasks[0].identity.clone());
            shell.confirm_remove_selected(window, cx);
            shell
        });
        view.read_with(cx, |shell, _| {
            assert!(shell.remove_confirmation.is_some());
            assert!(shell.pending_task_command.is_none());
        });
        view.update(cx, |shell, cx| shell.submit_remove_confirmation(cx));
        view.read_with(cx, |shell, _| {
            assert!(shell.remove_confirmation.is_none());
            assert!(matches!(
                shell
                    .pending_task_command
                    .as_ref()
                    .map(|pending| pending.command),
                Some(TaskCommandView::RemoveTask)
            ));
        });
    }

    #[gpui::test]
    fn navigation_shortcuts_return_to_downloads_and_preserve_selection(cx: &mut TestAppContext) {
        let (view, cx) = cx.add_window_view(|window, cx| {
            let mut shell = AppShell::new(Theme::dark(), window, cx);
            shell.snapshot = snapshot(2);
            shell.selected = Some(shell.snapshot.tasks[1].identity.clone());
            shell.page = AppPage::Settings;
            shell
        });
        let selected = view.read_with(cx, |shell, _| shell.selected.clone());

        view.update_in(cx, |shell, window, cx| {
            shell.focus_search(&FocusSearch, window, cx);
        });
        view.read_with(cx, |shell, _| {
            assert_eq!(shell.page, AppPage::Downloads);
            assert_eq!(shell.selected, selected);
        });

        view.update_in(cx, |shell, window, cx| {
            shell.page = AppPage::Settings;
            shell.open_add_download(&OpenAddDownload, window, cx);
        });
        view.read_with(cx, |shell, _| {
            assert_eq!(shell.page, AppPage::Downloads);
            assert!(shell.add_dialog.open);
            assert_eq!(shell.selected, selected);
        });
    }

    #[gpui::test]
    fn escape_priority_closes_popover_then_settings_then_search(cx: &mut TestAppContext) {
        let (view, cx) = cx.add_window_view(|window, cx| {
            let mut shell = AppShell::new(Theme::dark(), window, cx);
            shell.page = AppPage::Settings;
            shell.speed_popover_open = true;
            shell.search_input.update(cx, |input, cx| {
                input.set_text("archive", cx);
            });
            shell
        });

        view.update_in(cx, |shell, window, cx| {
            shell.clear_search(&ClearSearch, window, cx);
        });
        view.read_with(cx, |shell, cx| {
            assert!(!shell.speed_popover_open);
            assert_eq!(shell.page, AppPage::Settings);
            assert_eq!(shell.search_input.read(cx).text(), "archive");
        });

        view.update_in(cx, |shell, window, cx| {
            shell.clear_search(&ClearSearch, window, cx);
        });
        view.read_with(cx, |shell, cx| {
            assert_eq!(shell.page, AppPage::Downloads);
            assert_eq!(shell.search_input.read(cx).text(), "archive");
        });

        view.update_in(cx, |shell, window, cx| {
            shell.clear_search(&ClearSearch, window, cx);
        });
        view.read_with(cx, |shell, cx| {
            assert!(shell.search_input.read(cx).text().is_empty());
        });
    }

    #[gpui::test]
    fn failed_directory_save_keeps_the_draft(cx: &mut TestAppContext) {
        let initial = SettingsView {
            color_scheme: ColorSchemeView::Dark,
            download_directory: "C:/Downloads".into(),
        };
        let (view, cx) =
            cx.add_window_view(move |window, cx| AppShell::new_with_settings(initial, window, cx));
        let (request_id, requested) = view.update(cx, |shell, cx| {
            shell.page = AppPage::Settings;
            shell.settings_directory_input.update(cx, |input, cx| {
                input.set_text("D:/Transfers", cx);
            });
            shell.submit_settings(cx);
            let pending = shell
                .pending_settings_save
                .as_ref()
                .expect("settings save must become pending");
            (pending.request_id, pending.settings.clone())
        });

        view.update_in(cx, |shell, window, cx| {
            shell.set_settings_save_result(
                SettingsSaveResultView {
                    request_id,
                    settings: requested,
                    outcome: SettingsSaveOutcomeView::Failure(OperationErrorView {
                        code: "settings.write_failed".into(),
                        summary: "Could not write settings.".into(),
                        retryable: true,
                    }),
                },
                window,
                cx,
            );
        });
        view.read_with(cx, |shell, cx| {
            assert_eq!(shell.settings.download_directory, "C:/Downloads");
            assert_eq!(
                shell.settings_directory_input.read(cx).text(),
                "D:/Transfers"
            );
            assert_eq!(shell.page, AppPage::Settings);
            assert!(shell.settings_page.error.is_some());
        });
    }

    #[gpui::test]
    fn speed_popover_toggles_and_restores_previous_focus(cx: &mut TestAppContext) {
        let (view, cx) = cx.add_window_view(|window, cx| {
            let shell = AppShell::new(Theme::dark(), window, cx);
            window.focus(&shell.focus_handle, cx);
            shell
        });

        view.update_in(cx, |shell, window, cx| {
            shell.toggle_speed_popover(window, cx);
            assert!(shell.speed_popover_open);
            shell.close_speed_popover(window, cx);
        });
        view.read_with(cx, |shell, _| {
            assert!(!shell.speed_popover_open);
            assert!(shell.speed_popover_previous_focus.is_none());
        });
    }

    #[gpui::test]
    fn notice_expiration_only_removes_the_matching_success(cx: &mut TestAppContext) {
        let (view, cx) = cx.add_window_view(|window, cx| AppShell::new(Theme::dark(), window, cx));

        let first_id = view.update(cx, |shell, cx| {
            shell.show_notice("Saved.", false, cx);
            shell.status_notice.as_ref().expect("success notice").id
        });
        let error_id = view.update(cx, |shell, cx| {
            shell.show_notice("Failed.", true, cx);
            shell.status_notice.as_ref().expect("error notice").id
        });
        view.update(cx, |shell, cx| shell.expire_notice(first_id, cx));
        view.read_with(cx, |shell, _| {
            assert!(
                shell
                    .status_notice
                    .as_ref()
                    .is_some_and(|notice| notice.is_error)
            );
        });
        view.update(cx, |shell, cx| shell.expire_notice(error_id, cx));
        view.read_with(cx, |shell, _| {
            assert!(
                shell.status_notice.is_some(),
                "errors require explicit dismissal"
            );
        });
    }
}
