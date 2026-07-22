//! Activity log and status notices for AppShell.

use super::*;

impl AppShell {
    pub(crate) fn show_notice(
        &mut self,
        message: impl Into<String>,
        is_error: bool,
        cx: &mut Context<Self>,
    ) {
        // Command/action feedback always surfaces unless Silent is selected.
        self.show_notice_inner(message, is_error, false, cx);
    }

    pub(crate) fn show_automatic_notice(
        &mut self,
        message: impl Into<String>,
        is_error: bool,
        engine_event: bool,
        cx: &mut Context<Self>,
    ) {
        let prefs = self.settings.notifications;
        if prefs.volume == NotificationVolumeView::Silent {
            return;
        }
        if prefs.volume == NotificationVolumeView::Quiet {
            return;
        }
        if engine_event && !prefs.notify_on_engine_events {
            return;
        }
        if !engine_event && is_error && !prefs.notify_on_error {
            return;
        }
        if !engine_event && !is_error && !prefs.notify_on_completion {
            return;
        }
        self.show_notice_inner(message, is_error, true, cx);
    }

    pub(crate) fn show_notice_inner(
        &mut self,
        message: impl Into<String>,
        is_error: bool,
        automatic: bool,
        cx: &mut Context<Self>,
    ) {
        if self.settings.notifications.volume == NotificationVolumeView::Silent {
            // Silent still records history but suppresses every toast surface.
            return;
        }
        if automatic && self.settings.notifications.volume == NotificationVolumeView::Quiet {
            return;
        }
        let message = message.into();
        let id = self.next_notice_id;
        self.next_notice_id = self.next_notice_id.checked_add(1).unwrap_or(1);
        self.status_notice = Some(StatusNotice {
            id,
            message: message.clone(),
            is_error,
            automatic,
        });
        // OS-native toasts only for automatic, preference-gated events (PLAT-001).
        if automatic && self.settings.notifications.os_notifications {
            let title = if is_error {
                self.t("os-notification-title-error")
            } else {
                self.t("os-notification-title")
            };
            cx.emit(AppShellEvent::OsNotificationRequested {
                title,
                body: message,
                is_error,
            });
        }
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

    pub(crate) fn record_activity(
        &mut self,
        kind: ActivityKindView,
        summary: impl Into<String>,
        detail: Option<String>,
        task: Option<TaskIdentity>,
        count: u32,
        cx: &mut Context<Self>,
    ) {
        let id = self.next_activity_id;
        self.next_activity_id = self.next_activity_id.checked_add(1).unwrap_or(1);
        self.activity_log.insert(
            0,
            ActivityEntryView {
                id,
                kind,
                summary: summary.into(),
                detail,
                task,
                count: count.max(1),
            },
        );
        if self.activity_log.len() > ACTIVITY_HISTORY_LIMIT {
            self.activity_log.truncate(ACTIVITY_HISTORY_LIMIT);
        }
        cx.notify();
    }

    pub(crate) fn observe_task_status_transitions(
        &mut self,
        previous: &WorkspaceSnapshot,
        cx: &mut Context<Self>,
    ) {
        // First connected snapshot after connect/session change only seeds the map.
        let seed_only = previous.profile_id != self.snapshot.profile_id
            || previous.session_id != self.snapshot.session_id
            || previous.generation != self.snapshot.generation
            || self.known_task_status.is_empty();

        let mut completed: Vec<(TaskIdentity, String)> = Vec::new();
        let mut failed: Vec<(TaskIdentity, String, Option<String>)> = Vec::new();

        for task in &self.snapshot.tasks {
            let previous_status = self.known_task_status.get(&task.identity).copied();
            self.known_task_status
                .insert(task.identity.clone(), task.status);
            if seed_only {
                continue;
            }
            let Some(previous_status) = previous_status else {
                // First sighting of a task is not a transition event.
                continue;
            };
            if previous_status == task.status || previous_status.is_terminal() {
                continue;
            }
            match task.status {
                TaskStatusView::Complete => {
                    completed.push((task.identity.clone(), task_display_name(task)));
                }
                TaskStatusView::Failed => {
                    let detail = task.error.as_ref().map(|error| {
                        if let Some(details) = error.details.as_ref() {
                            format!("{} ({details})", error.summary)
                        } else {
                            error.summary.clone()
                        }
                    });
                    failed.push((task.identity.clone(), task_display_name(task), detail));
                }
                _ => {}
            }
        }

        // Drop identities that left the loaded workspace to bound memory.
        self.known_task_status.retain(|identity, _| {
            self.snapshot
                .tasks
                .iter()
                .any(|task| &task.identity == identity)
        });

        if seed_only {
            return;
        }

        if !completed.is_empty() {
            let count = completed.len() as u32;
            let summary = if count == 1 {
                format!("{} finished downloading.", completed[0].1)
            } else {
                format!("{count} downloads finished.")
            };
            let detail = if count == 1 {
                None
            } else {
                let listed = completed
                    .iter()
                    .take(5)
                    .map(|(_, name)| name.as_str())
                    .collect::<Vec<_>>()
                    .join(", ");
                Some(if count > 5 {
                    format!("{listed}, …")
                } else {
                    listed
                })
            };
            let task = (count == 1).then(|| completed[0].0.clone());
            self.record_activity(
                ActivityKindView::Completion,
                summary.clone(),
                detail,
                task,
                count,
                cx,
            );
            self.show_automatic_notice(summary, false, false, cx);
        }

        if !failed.is_empty() {
            let count = failed.len() as u32;
            let summary = if count == 1 {
                format!("{} failed.", failed[0].1)
            } else {
                format!("{count} downloads failed.")
            };
            let detail = if count == 1 {
                failed[0].2.clone()
            } else {
                let listed = failed
                    .iter()
                    .take(5)
                    .map(|(_, name, _)| name.as_str())
                    .collect::<Vec<_>>()
                    .join(", ");
                Some(if count > 5 {
                    format!("{listed}, …")
                } else {
                    listed
                })
            };
            let task = (count == 1).then(|| failed[0].0.clone());
            self.record_activity(
                ActivityKindView::Error,
                summary.clone(),
                detail,
                task,
                count,
                cx,
            );
            self.show_automatic_notice(summary, true, false, cx);
        }
    }

    pub(crate) fn toggle_activity_panel(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.activity_panel_open {
            self.close_activity_panel(window, cx);
            return;
        }
        self.speed_popover_open = false;
        self.sort_popover_open = false;
        self.context_menu = None;
        self.activity_panel_open = true;
        cx.notify();
        let _ = window;
    }

    pub(crate) fn close_activity_panel(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.activity_panel_open {
            return;
        }
        self.activity_panel_open = false;
        window.focus(&self.focus_handle, cx);
        cx.notify();
    }

    pub(crate) fn clear_activity_log(&mut self, cx: &mut Context<Self>) {
        if self.activity_log.is_empty() {
            return;
        }
        self.activity_log.clear();
        cx.notify();
    }

    pub(crate) fn expire_notice(&mut self, id: u64, cx: &mut Context<Self>) {
        if self
            .status_notice
            .as_ref()
            .is_some_and(|notice| notice.id == id && !notice.is_error)
        {
            self.status_notice = None;
            cx.notify();
        }
    }

    pub(crate) fn dismiss_notice(&mut self, cx: &mut Context<Self>) {
        if self.status_notice.take().is_some() {
            cx.notify();
        }
    }
}
