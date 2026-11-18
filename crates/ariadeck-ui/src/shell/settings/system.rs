//! Settings `system` and `notifications` section logic and rendering for `SettingsPanelView`.

use super::*;

impl SettingsPanelView {
    pub(crate) fn submit_notifications(&mut self, cx: &mut Context<Self>) {
        if self.is_pending() {
            return;
        }
        let draft = self.settings_page.draft_notifications;
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

    pub(crate) fn submit_platform(&mut self, cx: &mut Context<Self>) {
        if self.is_pending() {
            return;
        }
        let draft = self.settings_page.draft_platform;
        let bridge_draft = self.settings_page.draft_browser_bridge;
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
        if self.is_pending() || self.settings_page.draft_notifications.volume == volume {
            return;
        }
        self.settings_page.draft_notifications.volume = volume;
        self.settings_page.error = None;
        cx.notify();
    }

    pub(crate) fn toggle_notify_on_completion(&mut self, cx: &mut Context<Self>) {
        if self.is_pending() {
            return;
        }
        self.settings_page.draft_notifications.notify_on_completion =
            !self.settings_page.draft_notifications.notify_on_completion;
        self.settings_page.error = None;
        cx.notify();
    }

    pub(crate) fn toggle_notify_on_error(&mut self, cx: &mut Context<Self>) {
        if self.is_pending() {
            return;
        }
        self.settings_page.draft_notifications.notify_on_error =
            !self.settings_page.draft_notifications.notify_on_error;
        self.settings_page.error = None;
        cx.notify();
    }

    pub(crate) fn toggle_notify_on_engine_events(&mut self, cx: &mut Context<Self>) {
        if self.is_pending() {
            return;
        }
        self.settings_page
            .draft_notifications
            .notify_on_engine_events = !self
            .settings_page
            .draft_notifications
            .notify_on_engine_events;
        self.settings_page.error = None;
        cx.notify();
    }

    pub(crate) fn toggle_os_notifications(&mut self, cx: &mut Context<Self>) {
        if self.is_pending() {
            return;
        }
        self.settings_page.draft_notifications.os_notifications =
            !self.settings_page.draft_notifications.os_notifications;
        self.settings_page.error = None;
        cx.notify();
    }

    pub(crate) fn toggle_notify_on_low_disk(&mut self, cx: &mut Context<Self>) {
        if self.is_pending() {
            return;
        }
        self.settings_page.draft_notifications.notify_on_low_disk =
            !self.settings_page.draft_notifications.notify_on_low_disk;
        self.settings_page.error = None;
        cx.notify();
    }

    pub(crate) fn select_close_behavior(
        &mut self,
        behavior: CloseBehaviorView,
        cx: &mut Context<Self>,
    ) {
        if self.is_pending() || self.settings_page.draft_platform.close_behavior == behavior {
            return;
        }
        self.settings_page.draft_platform.close_behavior = behavior;
        self.settings_page.error = None;
        cx.notify();
    }

    pub(crate) fn toggle_show_tray_icon(&mut self, cx: &mut Context<Self>) {
        if self.is_pending() {
            return;
        }
        self.settings_page.draft_platform.show_tray_icon =
            !self.settings_page.draft_platform.show_tray_icon;
        if !self.settings_page.draft_platform.show_tray_icon
            && self.settings_page.draft_platform.close_behavior == CloseBehaviorView::MinimizeToTray
        {
            self.settings_page.draft_platform.close_behavior = CloseBehaviorView::Quit;
        }
        if !self.settings_page.draft_platform.show_tray_icon {
            self.settings_page.draft_platform.start_minimized_to_tray = false;
        }
        self.settings_page.error = None;
        cx.notify();
    }

    pub(crate) fn toggle_start_minimized_to_tray(&mut self, cx: &mut Context<Self>) {
        if self.is_pending() || !self.settings_page.draft_platform.show_tray_icon {
            return;
        }
        self.settings_page.draft_platform.start_minimized_to_tray =
            !self.settings_page.draft_platform.start_minimized_to_tray;
        self.settings_page.error = None;
        cx.notify();
    }

    pub(crate) fn select_draft_completion_action(
        &mut self,
        action: CompletionPowerActionView,
        cx: &mut Context<Self>,
    ) {
        if self.is_pending() || self.settings_page.draft_platform.action_after_done == action {
            return;
        }
        self.settings_page.draft_platform.action_after_done = action;
        self.settings_page.error = None;
        cx.notify();
    }

    pub(crate) fn toggle_draft_keep_awake(&mut self, cx: &mut Context<Self>) {
        if self.is_pending() || !self.platform_capabilities.keep_awake_supported {
            return;
        }
        self.settings_page.draft_platform.keep_awake_while_active =
            !self.settings_page.draft_platform.keep_awake_while_active;
        self.settings_page.error = None;
        cx.notify();
    }

    pub(crate) fn toggle_bridge_allow_cookies(&mut self, cx: &mut Context<Self>) {
        if self.is_pending() || !self.platform_capabilities.browser_bridge_supported {
            return;
        }
        self.settings_page.draft_browser_bridge.allow_cookies =
            !self.settings_page.draft_browser_bridge.allow_cookies;
        self.settings_page.error = None;
        cx.notify();
    }

    pub(crate) fn toggle_bridge_auto_submit(&mut self, cx: &mut Context<Self>) {
        if self.is_pending() || !self.platform_capabilities.browser_bridge_supported {
            return;
        }
        self.settings_page.draft_browser_bridge.auto_submit =
            !self.settings_page.draft_browser_bridge.auto_submit;
        self.settings_page.error = None;
        cx.notify();
    }

    pub(super) fn render_settings_notifications(&mut self, cx: &mut Context<Self>) -> Div {
        let colors = self.theme.colors;
        let pending = self.is_pending();
        let notifications = self.settings_page.draft_notifications;
        let volume_selected = NotificationVolumeView::all()
            .iter()
            .position(|volume| *volume == notifications.volume)
            .unwrap_or(0);
        let volume_panel = cx.entity().downgrade();
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
            volume_panel
                .update(cx, |panel, cx| panel.select_notification_volume(volume, cx))
                .ok();
        });
        let draft_completion = notifications.notify_on_completion;
        let draft_error = notifications.notify_on_error;
        let draft_engine = notifications.notify_on_engine_events;
        let draft_os = notifications.os_notifications;
        let draft_low_disk = notifications.notify_on_low_disk;
        let p1 = cx.entity().downgrade();
        let p2 = cx.entity().downgrade();
        let p3 = cx.entity().downgrade();
        let p4 = cx.entity().downgrade();
        let p5 = cx.entity().downgrade();
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
                        p1.update(cx, |panel, cx| panel.toggle_notify_on_completion(cx))
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
                        p2.update(cx, |panel, cx| panel.toggle_notify_on_error(cx))
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
                        p3.update(cx, |panel, cx| panel.toggle_notify_on_engine_events(cx))
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
                        p4.update(cx, |panel, cx| panel.toggle_os_notifications(cx))
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
                        p5.update(cx, |panel, cx| panel.toggle_notify_on_low_disk(cx))
                            .ok();
                    }),
                colors,
            ))
    }

    pub(super) fn render_settings_system(&mut self, cx: &mut Context<Self>) -> Div {
        let colors = self.theme.colors;
        let pending = self.is_pending();
        let platform = self.settings_page.draft_platform;
        let draft_keep_awake = platform.keep_awake_while_active;
        let action_after_done_selected = CompletionPowerActionView::ALL
            .iter()
            .position(|action| *action == platform.action_after_done)
            .unwrap_or(0);
        let action_panel = cx.entity().downgrade();
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
            action_panel
                .update(cx, |panel, cx| {
                    panel.select_draft_completion_action(action, cx)
                })
                .ok();
        });
        let p_power = cx.entity().downgrade();
        let keep_awake_supported = self.platform_capabilities.keep_awake_supported;
        let keep_awake_desc = if keep_awake_supported {
            self.t("settings-keep-awake-desc")
        } else {
            self.t("settings-keep-awake-unsupported-desc")
        };
        let power_section = settings_section_owned(self.t("settings-power"), colors)
            .child(settings_section_row_owned(
                self.t("settings-keep-awake"),
                Some(keep_awake_desc),
                Switch::new("toggle-keep-awake")
                    .checked(keep_awake_supported && draft_keep_awake)
                    .accessibility_label(self.t("settings-keep-awake"))
                    .disabled(pending || !keep_awake_supported)
                    .on_change(move |_, _, cx| {
                        p_power
                            .update(cx, |panel, cx| panel.toggle_draft_keep_awake(cx))
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

        let draft_show_tray = platform.show_tray_icon;
        let draft_start_minimized = platform.start_minimized_to_tray;
        let close_behavior_selected = CloseBehaviorView::all()
            .iter()
            .position(|behavior| *behavior == platform.close_behavior)
            .unwrap_or(0);
        let close_panel = cx.entity().downgrade();
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
            close_panel
                .update(cx, |panel, cx| panel.select_close_behavior(behavior, cx))
                .ok();
        });
        let p1 = cx.entity().downgrade();
        let p2 = cx.entity().downgrade();
        let p3 = cx.entity().downgrade();
        let p4 = cx.entity().downgrade();
        let draft_allow_cookies = self.settings_page.draft_browser_bridge.allow_cookies;
        let draft_auto_submit = self.settings_page.draft_browser_bridge.auto_submit;
        let tray_section = settings_section_owned(self.t("settings-window-tray"), colors)
            .child(settings_section_row_owned(
                self.t("settings-tray-icon"),
                Some(self.t("settings-tray-icon-desc")),
                Switch::new("toggle-show-tray")
                    .checked(draft_show_tray)
                    .accessibility_label(self.t("settings-tray-icon"))
                    .disabled(pending)
                    .on_change(move |_, _, cx| {
                        p1.update(cx, |panel, cx| panel.toggle_show_tray_icon(cx))
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
                        p2.update(cx, |panel, cx| panel.toggle_start_minimized_to_tray(cx))
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

        let bridge_supported = self.platform_capabilities.browser_bridge_supported;
        let bridge_section = settings_section_owned(self.t("settings-bridge"), colors)
            .child(settings_section_row_owned(
                self.t("settings-bridge-auto-submit"),
                Some(self.t("settings-bridge-auto-submit-desc")),
                Switch::new("toggle-bridge-auto-submit")
                    .checked(bridge_supported && draft_auto_submit)
                    .accessibility_label(self.t("settings-bridge-auto-submit"))
                    .disabled(pending || !bridge_supported)
                    .on_change(move |_, _, cx| {
                        p3.update(cx, |panel, cx| panel.toggle_bridge_auto_submit(cx))
                            .ok();
                    }),
                colors,
            ))
            .child(settings_section_row_owned(
                self.t("settings-bridge-cookies"),
                Some(self.t("settings-bridge-cookies-desc")),
                Switch::new("toggle-bridge-cookies")
                    .checked(bridge_supported && draft_allow_cookies)
                    .accessibility_label(self.t("settings-bridge-cookies"))
                    .disabled(pending || !bridge_supported)
                    .on_change(move |_, _, cx| {
                        p4.update(cx, |panel, cx| panel.toggle_bridge_allow_cookies(cx))
                            .ok();
                    }),
                colors,
            ))
            .child(
                div()
                    .text_xs()
                    .text_color(colors.text_muted)
                    .child(if bridge_supported {
                        self.t("settings-bridge-hint")
                    } else {
                        self.t("settings-bridge-unsupported-desc")
                    }),
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
