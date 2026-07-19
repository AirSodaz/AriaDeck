use std::{ops::Range, sync::Arc};

use gpui::{
    AnyElement, App, Context, Div, Entity, FocusHandle, Focusable, FontFeatures, FontWeight, Hsla,
    IntoElement, PromptButton, PromptLevel, Render, Role, ScrollStrategy, SharedString, Stateful,
    Subscription, UniformListScrollHandle, WeakFocusHandle, Window, div, prelude::*, px, relative,
    uniform_list,
};

use crate::{
    AddDownloadRequestView, AddDownloadResultView, ClearSearch, CloseAddDownload,
    CommandOutcomeView, ConnectionView, DownloadRowView, EngineSessionView, FocusNext,
    FocusPrevious, FocusSearch, OpenAddDownload, OpenTaskDetails, OperationErrorView,
    PauseSelectedTask, RemoveSelectedTask, RequestId, ResumeSelectedTask, SearchInputEvent,
    SelectNextTask, SelectPreviousTask, SubmitAddDownload, TaskCommandRequestView,
    TaskCommandResultView, TaskCommandView, TaskDetailsOutcomeView, TaskDetailsRequestView,
    TaskDetailsResultView, TaskDetailsView, TaskFileView, TaskIdentity, TaskStatusView, TextField,
    TextFieldConfig, Theme, ThemeMode, ToggleTheme, WorkspaceFilter, WorkspaceQuery,
    WorkspaceSnapshot, format_bytes, format_eta, format_percent, format_rate,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AppShellEvent {
    QueryChanged(WorkspaceQuery),
    RetryRequested,
    AddDownloadRequested(AddDownloadRequestView),
    TaskCommandRequested(TaskCommandRequestView),
    TaskDetailsRequested(TaskDetailsRequestView),
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
    message: String,
    is_error: bool,
}

pub struct AppShell {
    theme: Theme,
    snapshot: WorkspaceSnapshot,
    query: WorkspaceQuery,
    selected: Option<TaskIdentity>,
    search_input: Entity<TextField>,
    add_input: Entity<TextField>,
    add_dialog: AddDownloadDialog,
    add_dialog_focus: FocusHandle,
    add_cancel_focus: FocusHandle,
    add_submit_focus: FocusHandle,
    pending_task_command: Option<PendingTaskCommand>,
    details_drawer: Option<TaskDetailsDrawer>,
    confirmation_pending: bool,
    status_notice: Option<StatusNotice>,
    next_request_id: u64,
    list_scroll: UniformListScrollHandle,
    focus_handle: FocusHandle,
    rendered_range: Range<usize>,
    _search_subscription: Subscription,
    _add_subscription: Subscription,
}

impl gpui::EventEmitter<AppShellEvent> for AppShell {}

impl AppShell {
    #[must_use]
    pub fn new(theme: Theme, window: &mut Window, cx: &mut Context<Self>) -> Self {
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
        let focus_handle = cx.focus_handle();
        window.focus(&focus_handle, cx);
        Self {
            theme,
            snapshot: WorkspaceSnapshot::default(),
            query: WorkspaceQuery::default(),
            selected: None,
            search_input,
            add_input,
            add_dialog: AddDownloadDialog::default(),
            add_dialog_focus: cx.focus_handle(),
            add_cancel_focus: cx.focus_handle().tab_stop(true),
            add_submit_focus: cx.focus_handle().tab_stop(true),
            pending_task_command: None,
            details_drawer: None,
            confirmation_pending: false,
            status_notice: None,
            next_request_id: 1,
            list_scroll: UniformListScrollHandle::new(),
            focus_handle,
            rendered_range: 0..0,
            _search_subscription: search_subscription,
            _add_subscription: add_subscription,
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
                self.status_notice = Some(StatusNotice {
                    message: "The engine session changed before the command completed. Its outcome was not replayed."
                        .into(),
                    is_error: true,
                });
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
                self.status_notice = Some(StatusNotice {
                    message: "Download accepted by aria2.".into(),
                    is_error: false,
                });
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
            CommandOutcomeView::Success { .. } => {
                self.status_notice = Some(StatusNotice {
                    message: result.command.success_label().into(),
                    is_error: false,
                });
                if result.command == TaskCommandView::RemoveTask {
                    self.selected = None;
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
                self.status_notice = Some(StatusNotice {
                    message,
                    is_error: true,
                });
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
        if !self.search_input.read(cx).text().is_empty() {
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

    fn toggle_theme(&mut self, _: &ToggleTheme, _window: &mut Window, cx: &mut Context<Self>) {
        self.theme = match self.theme.mode {
            ThemeMode::Dark => Theme::light(),
            ThemeMode::Light | ThemeMode::System => Theme::dark(),
        };
        self.search_input
            .update(cx, |input, cx| input.set_theme(self.theme, cx));
        self.add_input
            .update(cx, |input, cx| input.set_theme(self.theme, cx));
        cx.notify();
    }

    fn focus_next(&mut self, _: &FocusNext, window: &mut Window, cx: &mut Context<Self>) {
        window.focus_next(cx);
        if self.add_dialog.open && !self.add_dialog_focus.contains_focused(window, cx) {
            window.focus(&self.add_input.focus_handle(cx), cx);
        }
    }

    fn focus_previous(&mut self, _: &FocusPrevious, window: &mut Window, cx: &mut Context<Self>) {
        window.focus_prev(cx);
        if self.add_dialog.open && !self.add_dialog_focus.contains_focused(window, cx) {
            window.focus(&self.add_submit_focus, cx);
        }
    }

    fn request_retry(&mut self, cx: &mut Context<Self>) {
        cx.emit(AppShellEvent::RetryRequested);
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
        if self.add_dialog.open {
            window.focus(&self.add_input.focus_handle(cx), cx);
            return;
        }
        if !self.snapshot.commands_available() {
            self.status_notice = Some(StatusNotice {
                message: "Connect and finish synchronization before adding a download.".into(),
                is_error: true,
            });
            cx.notify();
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
            self.status_notice = Some(StatusNotice {
                message: "Select a visible task to open its details.".into(),
                is_error: true,
            });
            cx.notify();
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
            self.status_notice = Some(StatusNotice {
                message: "Select a visible task first.".into(),
                is_error: true,
            });
            cx.notify();
            return;
        };
        let allowed = match command {
            TaskCommandView::Pause => task.status.can_pause(),
            TaskCommandView::Resume => task.status.can_resume(),
            TaskCommandView::RemoveTask => task.status.can_remove(),
        };
        if !allowed {
            self.status_notice = Some(StatusNotice {
                message: format!(
                    "{} is not available while the task is {}.",
                    task_command_label(command),
                    task.status.label().to_lowercase()
                ),
                is_error: true,
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
            self.status_notice = Some(StatusNotice {
                message: "The engine is not ready for commands.".into(),
                is_error: true,
            });
            cx.notify();
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
        self.status_notice = Some(StatusNotice {
            message: command.progress_label().into(),
            is_error: false,
        });
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
        if self.confirmation_pending || self.pending_task_command.is_some() {
            return;
        }
        let Some(task) = self.selected_task_view() else {
            return;
        };
        if !task.status.can_remove() || !self.snapshot.commands_available() {
            self.status_notice = Some(StatusNotice {
                message: "The selected task cannot be removed in the current engine state.".into(),
                is_error: true,
            });
            cx.notify();
            return;
        }

        self.confirmation_pending = true;
        let answer = window.prompt(
            PromptLevel::Warning,
            "Remove this task from aria2?",
            Some("The task entry will be removed. Downloaded files will be kept."),
            &[
                PromptButton::cancel("Cancel"),
                PromptButton::ok("Remove task"),
            ],
            cx,
        );
        cx.spawn_in(window, async move |this, cx| {
            let confirmed = task_removal_confirmed(answer.await.ok());
            this.update_in(cx, |this, _window, cx| {
                this.confirmation_pending = false;
                if confirmed {
                    this.begin_task_command(TaskCommandView::RemoveTask, cx);
                } else {
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
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
                    .child(self.render_add_button(cx))
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

    fn render_add_button(&self, cx: &mut Context<Self>) -> Stateful<Div> {
        let colors = self.theme.colors;
        let enabled = self.snapshot.commands_available() && !self.add_dialog.open;
        div()
            .id("open-add-download")
            .focusable()
            .tab_stop(enabled)
            .role(Role::Button)
            .aria_label(if enabled {
                "Add a URL or magnet download"
            } else {
                "Add download unavailable"
            })
            .h(px(34.0))
            .px_3()
            .flex()
            .items_center()
            .rounded_md()
            .bg(if enabled {
                colors.accent
            } else {
                colors.elevated_surface
            })
            .text_xs()
            .font_weight(FontWeight::MEDIUM)
            .text_color(if enabled {
                colors.text_inverse
            } else {
                colors.text_muted
            })
            .when(enabled, |element| {
                element
                    .cursor_pointer()
                    .hover(|style| style.bg(colors.accent_hover))
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.open_add_download(&OpenAddDownload, window, cx);
                    }))
            })
            .focus_visible(|style| style.border_1().border_color(colors.focus_ring))
            .child("Add task")
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
            div()
                .id("download-task-list")
                .role(Role::List)
                .aria_label(format!("Downloads, {task_count} visible tasks"))
                .size_full()
                .child(
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
                    .child(self.render_task_toolbar(cx)),
            )
            .when_some(self.status_notice.as_ref(), |element, notice| {
                let color = if notice.is_error {
                    colors.danger
                } else {
                    colors.success
                };
                element.child(
                    div()
                        .id("operation-status")
                        .role(if notice.is_error {
                            Role::Alert
                        } else {
                            Role::Status
                        })
                        .aria_label(notice.message.clone())
                        .min_h(px(32.0))
                        .flex_none()
                        .flex()
                        .items_center()
                        .px_4()
                        .border_b_1()
                        .border_color(with_alpha(color, 0.3))
                        .bg(with_alpha(color, 0.08))
                        .text_xs()
                        .text_color(color)
                        .child(notice.message.clone()),
                )
            })
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

    fn render_task_toolbar(&mut self, cx: &mut Context<Self>) -> Div {
        let colors = self.theme.colors;
        let Some(task) = self.selected_task_view() else {
            return div()
                .text_xs()
                .text_color(colors.text_muted)
                .child("Queue order");
        };
        let idle = self.pending_task_command.is_none() && !self.confirmation_pending;
        let commands_available = self.snapshot.commands_available() && idle;
        let details_enabled = self.snapshot.commands_available();
        let pause_enabled = commands_available && task.status.can_pause();
        let resume_enabled = commands_available && task.status.can_resume();
        let remove_enabled = commands_available && task.status.can_remove();

        div()
            .flex()
            .items_center()
            .gap_2()
            .child(
                toolbar_button(
                    "task-details-action",
                    "Details",
                    details_enabled,
                    false,
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
                    toolbar_button("pause-task-action", "Pause", pause_enabled, false, colors)
                        .when(pause_enabled, |button| {
                            button.on_click(cx.listener(|this, _, _window, cx| {
                                this.begin_task_command(TaskCommandView::Pause, cx);
                            }))
                        }),
                )
            })
            .when(task.status.can_resume(), |element| {
                element.child(
                    toolbar_button(
                        "resume-task-action",
                        "Resume",
                        resume_enabled,
                        false,
                        colors,
                    )
                    .when(resume_enabled, |button| {
                        button.on_click(cx.listener(|this, _, _window, cx| {
                            this.begin_task_command(TaskCommandView::Resume, cx);
                        }))
                    }),
                )
            })
            .child(
                toolbar_button("remove-task-action", "Remove", remove_enabled, true, colors).when(
                    remove_enabled,
                    |button| {
                        button.on_click(cx.listener(|this, _, window, cx| {
                            this.confirm_remove_selected(window, cx);
                        }))
                    },
                ),
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
                    toolbar_button("retry-task-details", "Retry", true, false, colors).on_click(
                        cx.listener(|this, _, _window, cx| {
                            this.request_current_details(cx);
                        }),
                    ),
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
                        toolbar_button("refresh-task-details", "Refresh", true, false, colors)
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
                            .flex_none()
                            .flex()
                            .flex_col()
                            .gap_2()
                            .p_4()
                            .border_b_1()
                            .border_color(colors.border)
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
                            .h(px(38.0))
                            .flex_none()
                            .flex()
                            .items_center()
                            .justify_between()
                            .px_4()
                            .border_b_1()
                            .border_color(colors.border)
                            .child(
                                div()
                                    .text_sm()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child("Files"),
                            )
                            .child(
                                div()
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
            .w(px(392.0))
            .min_w(px(320.0))
            .max_w(px(440.0))
            .flex_none()
            .min_h_0()
            .flex()
            .flex_col()
            .border_l_1()
            .border_color(colors.border)
            .bg(colors.surface)
            .child(
                div()
                    .h(px(58.0))
                    .flex_none()
                    .flex()
                    .items_center()
                    .gap_3()
                    .px_4()
                    .border_b_1()
                    .border_color(colors.border)
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
                                    .child(format_percent(overview_progress))
                                    .child(identity.gid),
                            ),
                    )
                    .child(
                        toolbar_button("close-task-details", "Close", true, false, colors)
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.close_task_details(window, cx);
                            })),
                    ),
            )
            .child(body)
            .into_any_element()
    }

    fn render_task_row(
        &mut self,
        index: usize,
        task: DownloadRowView,
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
        div()
            .id(stable_id)
            .role(Role::ListItem)
            .aria_label(aria_label)
            .aria_selected(selected)
            .aria_position_in_set(index + 1)
            .aria_size_of_set(task_count)
            .when(selected, |row| row.aria_active_descendant())
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
                this.select_at(index, window, cx);
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

    fn render_add_download_dialog(&mut self, cx: &mut Context<Self>) -> Stateful<Div> {
        let colors = self.theme.colors;
        let pending = self.add_dialog.pending.is_some();
        let error = self.add_dialog.error.clone();

        div()
            .id("add-download-overlay")
            .absolute()
            .inset_0()
            .occlude()
            .flex()
            .items_start()
            .justify_center()
            .pt(px(96.0))
            .bg(with_alpha(colors.background, 0.78))
            .child(
                div()
                    .id("add-download-dialog")
                    .key_context("AddDownloadDialog")
                    .role(Role::Dialog)
                    .aria_label("Add download")
                    .track_focus(&self.add_dialog_focus)
                    .w(px(560.0))
                    .max_w_full()
                    .flex()
                    .flex_col()
                    .gap_4()
                    .p_5()
                    .rounded_md()
                    .border_1()
                    .border_color(colors.border_strong)
                    .bg(colors.elevated_surface)
                    .text_color(colors.text_primary)
                    .child(
                        div()
                            .text_base()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child("Add download"),
                    )
                    .child(
                        div()
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
                            }),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_end()
                            .gap_2()
                            .child(
                                div()
                                    .id("cancel-add-download")
                                    .track_focus(&self.add_cancel_focus)
                                    .role(Role::Button)
                                    .aria_label("Cancel adding a download")
                                    .h(px(34.0))
                                    .px_3()
                                    .flex()
                                    .items_center()
                                    .rounded_md()
                                    .border_1()
                                    .border_color(colors.border)
                                    .text_sm()
                                    .text_color(if pending {
                                        colors.text_muted
                                    } else {
                                        colors.text_secondary
                                    })
                                    .when(!pending, |button| {
                                        button
                                            .cursor_pointer()
                                            .hover(|style| style.bg(colors.surface_hover))
                                            .on_click(cx.listener(|this, _, window, cx| {
                                                this.close_add_download(window, cx);
                                            }))
                                    })
                                    .focus_visible(|style| style.border_color(colors.focus_ring))
                                    .child("Cancel"),
                            )
                            .child(
                                div()
                                    .id("submit-add-download")
                                    .track_focus(&self.add_submit_focus)
                                    .role(Role::Button)
                                    .aria_label(if pending {
                                        "Adding download"
                                    } else {
                                        "Add download"
                                    })
                                    .h(px(34.0))
                                    .px_3()
                                    .flex()
                                    .items_center()
                                    .rounded_md()
                                    .bg(if pending {
                                        colors.surface_active
                                    } else {
                                        colors.accent
                                    })
                                    .text_sm()
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(if pending {
                                        colors.text_muted
                                    } else {
                                        colors.text_inverse
                                    })
                                    .when(!pending, |button| {
                                        button
                                            .cursor_pointer()
                                            .hover(|style| style.bg(colors.accent_hover))
                                            .on_click(cx.listener(|this, _, _window, cx| {
                                                this.submit_add_download(cx);
                                            }))
                                    })
                                    .focus_visible(|style| {
                                        style.border_1().border_color(colors.focus_ring)
                                    })
                                    .child(if pending { "Adding..." } else { "Add" }),
                            ),
                    ),
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
            .on_action(cx.listener(Self::open_add_download))
            .on_action(cx.listener(Self::close_add_download_action))
            .on_action(cx.listener(Self::submit_add_download_action))
            .on_action(cx.listener(Self::open_task_details_action))
            .on_action(cx.listener(Self::pause_selected))
            .on_action(cx.listener(Self::resume_selected))
            .on_action(cx.listener(Self::remove_selected))
            .on_action(cx.listener(Self::toggle_theme))
            .on_action(cx.listener(Self::focus_next))
            .on_action(cx.listener(Self::focus_previous))
            .relative()
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
            .when(self.add_dialog.open, |element| {
                element.child(self.render_add_download_dialog(cx))
            })
    }
}

fn toolbar_button(
    id: &'static str,
    label: &'static str,
    enabled: bool,
    danger: bool,
    colors: crate::ThemeColors,
) -> Stateful<Div> {
    let foreground = if !enabled {
        colors.text_muted
    } else if danger {
        colors.danger
    } else {
        colors.text_secondary
    };
    div()
        .id(id)
        .focusable()
        .tab_stop(enabled)
        .role(Role::Button)
        .aria_label(if enabled { label } else { "Action unavailable" })
        .h(px(30.0))
        .px_2()
        .flex()
        .items_center()
        .rounded_sm()
        .border_1()
        .border_color(if danger && enabled {
            with_alpha(colors.danger, 0.45)
        } else {
            colors.border
        })
        .bg(colors.elevated_surface)
        .text_xs()
        .font_weight(FontWeight::MEDIUM)
        .text_color(foreground)
        .when(enabled, |button| {
            button
                .cursor_pointer()
                .hover(|style| style.bg(colors.surface_hover))
        })
        .focus_visible(|style| style.border_color(colors.focus_ring))
        .child(label)
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
            "{}, {}, {}",
            file.path,
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
        .border_b_1()
        .border_color(colors.border)
        .child(
            div()
                .w(px(18.0))
                .flex_none()
                .text_center()
                .text_xs()
                .text_color(if file.selected {
                    colors.success
                } else {
                    colors.text_muted
                })
                .child(if file.selected { "On" } else { "Off" }),
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
        TaskCommandView::RemoveTask => "Remove",
    }
}

fn task_removal_confirmed(prompt_result: Option<usize>) -> bool {
    prompt_result == Some(1)
}

fn stale_session_error() -> OperationErrorView {
    OperationErrorView {
        code: "command.stale_session".into(),
        summary: "The aria2 session changed. Review current state before submitting again.".into(),
        retryable: false,
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
            session_id: "session".into(),
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

    #[test]
    fn task_removal_requires_the_explicit_confirmation_button() {
        assert!(!task_removal_confirmed(None));
        assert!(!task_removal_confirmed(Some(0)));
        assert!(task_removal_confirmed(Some(1)));
    }
}
