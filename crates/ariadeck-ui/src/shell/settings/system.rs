//! Settings `system` section logic and rendering for `AppShell`.

use super::*;

impl AppShell {
    pub(crate) fn submit_notifications(&mut self, cx: &mut Context<Self>) {
        if self.page != AppPage::Settings || self.pending_settings_save.is_some() {
            return;
        }
        let draft = NotificationSettingsView {
            volume: self.settings_page.draft_notification_volume,
            notify_on_completion: self.settings_page.draft_notify_on_completion,
            notify_on_error: self.settings_page.draft_notify_on_error,
            notify_on_engine_events: self.settings_page.draft_notify_on_engine_events,
            os_notifications: self.settings_page.draft_os_notifications,
            notify_on_low_disk: self.settings_page.draft_notify_on_low_disk,
            low_disk_threshold_bytes: self.settings.notifications.low_disk_threshold_bytes,
        };
        if draft == self.settings.notifications {
            return;
        }
        let mut settings = self.settings.clone();
        settings.notifications = draft;
        self.request_settings_save(
            settings,
            ProxyPasswordUpdateView::Unchanged,
            SettingsSaveSource::Notifications,
            cx,
        );
    }

    pub(super) fn browser_bridge_draft(&self) -> BrowserBridgeSettingsView {
        BrowserBridgeSettingsView {
            allow_cookies: self.settings_page.draft_bridge_allow_cookies,
            auto_submit: self.settings_page.draft_bridge_auto_submit,
        }
    }

    pub(crate) fn submit_platform(&mut self, cx: &mut Context<Self>) {
        if self.page != AppPage::Settings || self.pending_settings_save.is_some() {
            return;
        }
        let draft = PlatformSettingsView {
            close_behavior: self.settings_page.draft_close_behavior,
            show_tray_icon: self.settings_page.draft_show_tray_icon,
            start_minimized_to_tray: self.settings_page.draft_start_minimized_to_tray,
            keep_awake_while_active: self.settings_page.draft_keep_awake_while_active,
            action_after_done: self.settings_page.draft_action_after_done,
        };
        let bridge_draft = self.browser_bridge_draft();
        if draft == self.settings.platform && bridge_draft == self.settings.browser_bridge {
            return;
        }
        let mut settings = self.settings.clone();
        settings.platform = draft;
        settings.browser_bridge = bridge_draft;
        self.request_settings_save(
            settings,
            ProxyPasswordUpdateView::Unchanged,
            SettingsSaveSource::Platform,
            cx,
        );
    }

    pub(crate) fn select_notification_volume(
        &mut self,
        volume: NotificationVolumeView,
        cx: &mut Context<Self>,
    ) {
        if self.page != AppPage::Settings || self.pending_settings_save.is_some() {
            return;
        }
        if self.settings_page.draft_notification_volume == volume {
            return;
        }
        self.settings_page.draft_notification_volume = volume;
        self.settings_page.error = None;
        cx.notify();
    }

    pub(crate) fn toggle_notify_on_completion(&mut self, cx: &mut Context<Self>) {
        if self.page != AppPage::Settings || self.pending_settings_save.is_some() {
            return;
        }
        self.settings_page.draft_notify_on_completion =
            !self.settings_page.draft_notify_on_completion;
        self.settings_page.error = None;
        cx.notify();
    }

    pub(crate) fn toggle_notify_on_error(&mut self, cx: &mut Context<Self>) {
        if self.page != AppPage::Settings || self.pending_settings_save.is_some() {
            return;
        }
        self.settings_page.draft_notify_on_error = !self.settings_page.draft_notify_on_error;
        self.settings_page.error = None;
        cx.notify();
    }

    pub(crate) fn toggle_notify_on_engine_events(&mut self, cx: &mut Context<Self>) {
        if self.page != AppPage::Settings || self.pending_settings_save.is_some() {
            return;
        }
        self.settings_page.draft_notify_on_engine_events =
            !self.settings_page.draft_notify_on_engine_events;
        self.settings_page.error = None;
        cx.notify();
    }

    pub(crate) fn toggle_os_notifications(&mut self, cx: &mut Context<Self>) {
        if self.page != AppPage::Settings || self.pending_settings_save.is_some() {
            return;
        }
        self.settings_page.draft_os_notifications = !self.settings_page.draft_os_notifications;
        self.settings_page.error = None;
        cx.notify();
    }

    pub(crate) fn toggle_notify_on_low_disk(&mut self, cx: &mut Context<Self>) {
        if self.page != AppPage::Settings || self.pending_settings_save.is_some() {
            return;
        }
        self.settings_page.draft_notify_on_low_disk = !self.settings_page.draft_notify_on_low_disk;
        self.settings_page.error = None;
        cx.notify();
    }

    pub(crate) fn select_close_behavior(
        &mut self,
        behavior: CloseBehaviorView,
        cx: &mut Context<Self>,
    ) {
        if self.page != AppPage::Settings || self.pending_settings_save.is_some() {
            return;
        }
        if self.settings_page.draft_close_behavior == behavior {
            return;
        }
        self.settings_page.draft_close_behavior = behavior;
        self.settings_page.error = None;
        cx.notify();
    }

    pub(crate) fn toggle_show_tray_icon(&mut self, cx: &mut Context<Self>) {
        if self.page != AppPage::Settings || self.pending_settings_save.is_some() {
            return;
        }
        self.settings_page.draft_show_tray_icon = !self.settings_page.draft_show_tray_icon;
        // Closing to tray requires a tray icon so the user can restore the window.
        if !self.settings_page.draft_show_tray_icon
            && self.settings_page.draft_close_behavior == CloseBehaviorView::MinimizeToTray
        {
            self.settings_page.draft_close_behavior = CloseBehaviorView::Quit;
        }
        if !self.settings_page.draft_show_tray_icon {
            self.settings_page.draft_start_minimized_to_tray = false;
        }
        self.settings_page.error = None;
        cx.notify();
    }

    pub(crate) fn toggle_start_minimized_to_tray(&mut self, cx: &mut Context<Self>) {
        if self.page != AppPage::Settings || self.pending_settings_save.is_some() {
            return;
        }
        if !self.settings_page.draft_show_tray_icon {
            return;
        }
        self.settings_page.draft_start_minimized_to_tray =
            !self.settings_page.draft_start_minimized_to_tray;
        self.settings_page.error = None;
        cx.notify();
    }

    pub(crate) fn select_draft_completion_action(
        &mut self,
        action: CompletionPowerActionView,
        cx: &mut Context<Self>,
    ) {
        if self.page != AppPage::Settings || self.pending_settings_save.is_some() {
            return;
        }
        if self.settings_page.draft_action_after_done == action {
            return;
        }
        self.settings_page.draft_action_after_done = action;
        self.settings_page.error = None;
        cx.notify();
    }

    pub(crate) fn toggle_draft_keep_awake(&mut self, cx: &mut Context<Self>) {
        if self.page != AppPage::Settings || self.pending_settings_save.is_some() {
            return;
        }
        self.settings_page.draft_keep_awake_while_active =
            !self.settings_page.draft_keep_awake_while_active;
        self.settings_page.error = None;
        cx.notify();
    }

    pub(crate) fn toggle_bridge_allow_cookies(&mut self, cx: &mut Context<Self>) {
        if self.page != AppPage::Settings || self.pending_settings_save.is_some() {
            return;
        }
        self.settings_page.draft_bridge_allow_cookies =
            !self.settings_page.draft_bridge_allow_cookies;
        self.settings_page.error = None;
        cx.notify();
    }

    pub(crate) fn toggle_bridge_auto_submit(&mut self, cx: &mut Context<Self>) {
        if self.page != AppPage::Settings || self.pending_settings_save.is_some() {
            return;
        }
        self.settings_page.draft_bridge_auto_submit = !self.settings_page.draft_bridge_auto_submit;
        self.settings_page.error = None;
        cx.notify();
    }

    pub(crate) fn render_settings_notifications(&mut self, cx: &mut Context<Self>) -> Div {
        let colors = self.theme.colors;
        let pending = self.pending_settings_save.is_some();
        let volume_selected = NotificationVolumeView::all()
            .iter()
            .position(|volume| *volume == self.settings_page.draft_notification_volume)
            .unwrap_or(0);
        let volume_shell = cx.entity().downgrade();
        let volume_control = SegmentedControl::new(
            "settings-notification-volume",
            NotificationVolumeView::all().map(|volume| {
                Segment::new(self.t(match volume {
                    NotificationVolumeView::Normal => "settings-volume-normal",
                    NotificationVolumeView::Quiet => "settings-volume-quiet",
                    NotificationVolumeView::Silent => "settings-volume-silent",
                }))
            }),
            volume_selected,
            self.theme,
        )
        .aria_label(self.t("settings-volume-aria"))
        .disabled(pending)
        .on_select(move |index, _window, cx| {
            let volume = NotificationVolumeView::all()
                .get(index)
                .copied()
                .unwrap_or_default();
            volume_shell
                .update(cx, |shell, cx| shell.select_notification_volume(volume, cx))
                .ok();
        });
        let draft_completion = self.settings_page.draft_notify_on_completion;
        let draft_error = self.settings_page.draft_notify_on_error;
        let draft_engine = self.settings_page.draft_notify_on_engine_events;
        let draft_os = self.settings_page.draft_os_notifications;
        let draft_low_disk = self.settings_page.draft_notify_on_low_disk;
        let s1 = cx.entity().downgrade();
        let s2 = cx.entity().downgrade();
        let s3 = cx.entity().downgrade();
        let s4 = cx.entity().downgrade();
        let s5 = cx.entity().downgrade();
        settings_section_owned(self.t("settings-notifications"), colors)
            .child(settings_section_row_owned(
                self.t("settings-volume"),
                Some(self.t("settings-volume-desc")),
                volume_control,
                colors,
            ))
            .child(settings_section_row_owned(
                self.t("settings-notify-completion"),
                Some(self.t("settings-notify-completion-desc")),
                Switch::new("toggle-notify-completion")
                    .checked(draft_completion)
                    .accessibility_label(self.t("settings-notify-completion"))
                    .disabled(pending)
                    .on_change(move |_, _, cx| {
                        s1.update(cx, |shell, cx| shell.toggle_notify_on_completion(cx))
                            .ok();
                    }),
                colors,
            ))
            .child(settings_section_row_owned(
                self.t("settings-notify-error"),
                Some(self.t("settings-notify-error-desc")),
                Switch::new("toggle-notify-error")
                    .checked(draft_error)
                    .accessibility_label(self.t("settings-notify-error"))
                    .disabled(pending)
                    .on_change(move |_, _, cx| {
                        s2.update(cx, |shell, cx| shell.toggle_notify_on_error(cx))
                            .ok();
                    }),
                colors,
            ))
            .child(settings_section_row_owned(
                self.t("settings-notify-engine"),
                Some(self.t("settings-notify-engine-desc")),
                Switch::new("toggle-notify-engine")
                    .checked(draft_engine)
                    .accessibility_label(self.t("settings-notify-engine"))
                    .disabled(pending)
                    .on_change(move |_, _, cx| {
                        s3.update(cx, |shell, cx| shell.toggle_notify_on_engine_events(cx))
                            .ok();
                    }),
                colors,
            ))
            .child(settings_section_row_owned(
                self.t("settings-notify-os"),
                Some(self.t("settings-notify-os-desc")),
                Switch::new("toggle-os-notifications")
                    .checked(draft_os)
                    .accessibility_label(self.t("settings-notify-os"))
                    .disabled(pending)
                    .on_change(move |_, _, cx| {
                        s4.update(cx, |shell, cx| shell.toggle_os_notifications(cx))
                            .ok();
                    }),
                colors,
            ))
            .child(settings_section_row_owned(
                self.t("settings-notify-low-disk"),
                Some(self.t("settings-notify-low-disk-desc")),
                Switch::new("toggle-notify-low-disk")
                    .checked(draft_low_disk)
                    .accessibility_label(self.t("settings-notify-low-disk"))
                    .disabled(pending)
                    .on_change(move |_, _, cx| {
                        s5.update(cx, |shell, cx| shell.toggle_notify_on_low_disk(cx))
                            .ok();
                    }),
                colors,
            ))
    }

    pub(crate) fn render_settings_system(&mut self, cx: &mut Context<Self>) -> Div {
        let colors = self.theme.colors;
        let pending = self.pending_settings_save.is_some();
        let draft_keep_awake = self.settings_page.draft_keep_awake_while_active;
        let action_after_done_selected = CompletionPowerActionView::ALL
            .iter()
            .position(|action| *action == self.settings_page.draft_action_after_done)
            .unwrap_or(0);
        let action_shell = cx.entity().downgrade();
        let action_control = SegmentedControl::new(
            "settings-action-after-done",
            CompletionPowerActionView::ALL.map(|action| Segment::new(self.t(action.message_key()))),
            action_after_done_selected,
            self.theme,
        )
        .aria_label(self.t("settings-action-after-done"))
        .disabled(pending)
        .on_select(move |index, _window, cx| {
            let action = CompletionPowerActionView::ALL
                .get(index)
                .copied()
                .unwrap_or_default();
            action_shell
                .update(cx, |shell, cx| {
                    shell.select_draft_completion_action(action, cx)
                })
                .ok();
        });
        let s_power = cx.entity().downgrade();
        let power_section = settings_section_owned(self.t("settings-power"), colors)
            .child(settings_section_row_owned(
                self.t("settings-keep-awake"),
                Some(self.t("settings-keep-awake-desc")),
                Switch::new("toggle-keep-awake")
                    .checked(draft_keep_awake)
                    .accessibility_label(self.t("settings-keep-awake"))
                    .disabled(pending)
                    .on_change(move |_, _, cx| {
                        s_power
                            .update(cx, |shell, cx| shell.toggle_draft_keep_awake(cx))
                            .ok();
                    }),
                colors,
            ))
            .child(settings_section_row_owned(
                self.t("settings-action-after-done"),
                Some(self.t("settings-action-after-done-desc")),
                action_control,
                colors,
            ));

        let draft_show_tray = self.settings_page.draft_show_tray_icon;
        let draft_start_minimized = self.settings_page.draft_start_minimized_to_tray;
        let close_behavior_selected = CloseBehaviorView::all()
            .iter()
            .position(|behavior| *behavior == self.settings_page.draft_close_behavior)
            .unwrap_or(0);
        let close_shell = cx.entity().downgrade();
        let close_control = SegmentedControl::new(
            "settings-close-behavior",
            CloseBehaviorView::all().map(|behavior| {
                Segment::new(self.t(match behavior {
                    CloseBehaviorView::MinimizeToTray => "settings-close-minimize",
                    CloseBehaviorView::Quit => "settings-close-quit",
                }))
            }),
            close_behavior_selected,
            self.theme,
        )
        .aria_label(self.t("settings-close-behavior-aria"))
        .disabled(pending || !draft_show_tray)
        .on_select(move |index, _window, cx| {
            let behavior = CloseBehaviorView::all()
                .get(index)
                .copied()
                .unwrap_or_default();
            close_shell
                .update(cx, |shell, cx| shell.select_close_behavior(behavior, cx))
                .ok();
        });
        let s1 = cx.entity().downgrade();
        let s2 = cx.entity().downgrade();
        let s3 = cx.entity().downgrade();
        let s4 = cx.entity().downgrade();
        let draft_allow_cookies = self.settings_page.draft_bridge_allow_cookies;
        let draft_auto_submit = self.settings_page.draft_bridge_auto_submit;
        let tray_section = settings_section_owned(self.t("settings-window-tray"), colors)
            .child(settings_section_row_owned(
                self.t("settings-tray-icon"),
                Some(self.t("settings-tray-icon-desc")),
                Switch::new("toggle-show-tray")
                    .checked(draft_show_tray)
                    .accessibility_label(self.t("settings-tray-icon"))
                    .disabled(pending)
                    .on_change(move |_, _, cx| {
                        s1.update(cx, |shell, cx| shell.toggle_show_tray_icon(cx))
                            .ok();
                    }),
                colors,
            ))
            .child(settings_section_row_owned(
                self.t("settings-start-minimized"),
                Some(self.t("settings-start-minimized-desc")),
                Switch::new("toggle-start-minimized")
                    .checked(draft_start_minimized)
                    .accessibility_label(self.t("settings-start-minimized"))
                    .disabled(pending || !draft_show_tray)
                    .on_change(move |_, _, cx| {
                        s2.update(cx, |shell, cx| shell.toggle_start_minimized_to_tray(cx))
                            .ok();
                    }),
                colors,
            ))
            .child(settings_section_row_owned(
                self.t("settings-close-behavior"),
                Some(self.t("settings-close-behavior-desc")),
                close_control,
                colors,
            ))
            .child(
                div()
                    .text_xs()
                    .text_color(colors.text_muted)
                    .child(self.t("settings-tray-hint")),
            );

        // D-045: cookie forwarding and auto-submit are the two decisions that
        // widen what a browser hand-off can do, so they live together with the
        // caveats spelled out rather than buried in a tooltip.
        let bridge_section = settings_section_owned(self.t("settings-bridge"), colors)
            .child(settings_section_row_owned(
                self.t("settings-bridge-auto-submit"),
                Some(self.t("settings-bridge-auto-submit-desc")),
                Switch::new("toggle-bridge-auto-submit")
                    .checked(draft_auto_submit)
                    .accessibility_label(self.t("settings-bridge-auto-submit"))
                    .disabled(pending)
                    .on_change(move |_, _, cx| {
                        s3.update(cx, |shell, cx| shell.toggle_bridge_auto_submit(cx))
                            .ok();
                    }),
                colors,
            ))
            .child(settings_section_row_owned(
                self.t("settings-bridge-cookies"),
                Some(self.t("settings-bridge-cookies-desc")),
                Switch::new("toggle-bridge-cookies")
                    .checked(draft_allow_cookies)
                    .accessibility_label(self.t("settings-bridge-cookies"))
                    .disabled(pending)
                    .on_change(move |_, _, cx| {
                        s4.update(cx, |shell, cx| shell.toggle_bridge_allow_cookies(cx))
                            .ok();
                    }),
                colors,
            ))
            .child(
                div()
                    .text_xs()
                    .text_color(colors.text_muted)
                    .child(self.t("settings-bridge-hint")),
            );

        div()
            .flex()
            .flex_col()
            .gap_6()
            .child(power_section)
            .child(tray_section)
            .child(bridge_section)
    }
}
