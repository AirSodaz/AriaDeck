use std::time::Duration;

use ariadeck_i18n::FluentValue;
use gpui::{Anchor, Context, Div, FontWeight, Role, SharedString, Window, div, prelude::*, px};
use gpui_kit::component::{
    Disableable as _, Sizable as _,
    button::{Button as KitButton, ButtonVariants as _},
    popover::Popover,
    switch::Switch,
};

use super::{
    ActiveModal, AppShell, AppShellEvent, ProxyPasswordUpdateView, SettingsSaveSource,
    StatusBarPopover, is_activation_key,
};
use crate::{CompletionPowerActionView, Icon, IconName, IconSize};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PowerCountdownState {
    pub action: CompletionPowerActionView,
    pub remaining_seconds: u32,
}

impl AppShell {
    pub fn report_power_action_failed(&mut self, summary: String, cx: &mut Context<Self>) {
        let message = self.t_args(
            "notice-power-action-failed",
            &[("summary", FluentValue::from(summary.as_str()))],
        );
        self.record_activity(
            crate::ActivityKindView::Error,
            message.clone(),
            None,
            None,
            1,
            cx,
        );
        self.show_notice(message, true, cx);
        cx.notify();
    }

    pub(crate) fn power_countdown(&self) -> Option<PowerCountdownState> {
        match &self.active_modal {
            Some(ActiveModal::PowerCountdown(state)) => Some(*state),
            _ => None,
        }
    }

    fn clear_power_countdown_modal(&mut self) -> Option<PowerCountdownState> {
        match self.active_modal.take() {
            Some(ActiveModal::PowerCountdown(state)) => Some(state),
            other => {
                self.active_modal = other;
                None
            }
        }
    }

    pub(crate) fn check_power_action_trigger(&mut self, cx: &mut Context<Self>) {
        if !self.settings.platform.action_after_done.is_armed() {
            return;
        }

        if self.snapshot.counts.active > 0 {
            self.power_had_active_tasks = true;
            // If a countdown was running and a new task started downloading, cancel it.
            if self.clear_power_countdown_modal().is_some() {
                self.power_countdown_generation = self.power_countdown_generation.wrapping_add(1);
                self.sync_kit_modals_deferred(cx);
                cx.notify();
            }
        } else if self.power_had_active_tasks
            && self.snapshot.counts.active == 0
            && self.snapshot.counts.waiting == 0
            && self.power_countdown().is_none()
        {
            self.start_power_countdown(self.settings.platform.action_after_done, cx);
        }
    }

    pub(crate) fn start_power_countdown(
        &mut self,
        action: CompletionPowerActionView,
        cx: &mut Context<Self>,
    ) {
        self.active_modal = Some(ActiveModal::PowerCountdown(PowerCountdownState {
            action,
            remaining_seconds: 30,
        }));
        self.power_countdown_generation = self.power_countdown_generation.wrapping_add(1);
        let generation = self.power_countdown_generation;

        // Bring window out of tray so user sees the countdown dialog.
        cx.emit(AppShellEvent::ShowFromTrayRequested);

        // Emit native OS desktop notification.
        let action_verb = self.action_verb_label(action);
        let title = self.t("dialog-power-countdown-title");
        let body = self.t_args(
            "dialog-power-countdown-desc",
            &[
                ("action", FluentValue::from(action_verb.as_str())),
                ("seconds", FluentValue::from("30")),
            ],
        );
        cx.emit(AppShellEvent::OsNotificationRequested {
            title,
            body,
            is_error: false,
        });

        self.sync_kit_modals_deferred(cx);
        cx.notify();

        // Spawn 1-second countdown ticker
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(Duration::from_secs(1)).await;
                let should_continue = this
                    .update(cx, |this, cx| {
                        if this.power_countdown_generation == generation {
                            this.tick_power_countdown(cx)
                        } else {
                            false
                        }
                    })
                    .unwrap_or(false);
                if !should_continue {
                    break;
                }
            }
        })
        .detach();
    }

    pub(crate) fn tick_power_countdown(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(ActiveModal::PowerCountdown(countdown)) = &mut self.active_modal else {
            return false;
        };

        if countdown.remaining_seconds > 1 {
            countdown.remaining_seconds -= 1;
            cx.notify();
            true
        } else {
            let action = countdown.action;
            self.active_modal = None;
            self.power_had_active_tasks = false;
            self.settings.platform.action_after_done = CompletionPowerActionView::None;
            self.sync_settings_panel_power_draft(cx);
            let settings = self.settings.clone();
            self.request_settings_save(
                settings,
                ProxyPasswordUpdateView::Unchanged,
                SettingsSaveSource::Platform,
                cx,
            );
            cx.emit(AppShellEvent::CompletionPowerActionRequested(action));
            self.sync_kit_modals_deferred(cx);
            cx.notify();
            false
        }
    }

    pub(crate) fn cancel_power_countdown(&mut self, cx: &mut Context<Self>) {
        self.clear_power_countdown_modal();
        self.power_countdown_generation = self.power_countdown_generation.wrapping_add(1);
        self.power_had_active_tasks = false;
        self.settings.platform.action_after_done = CompletionPowerActionView::None;
        self.sync_settings_panel_power_draft(cx);
        let settings = self.settings.clone();
        self.request_settings_save(
            settings,
            ProxyPasswordUpdateView::Unchanged,
            SettingsSaveSource::Platform,
            cx,
        );
        self.show_notice(self.t("notice-power-action-cancelled"), false, cx);
        self.sync_kit_modals_deferred(cx);
        cx.notify();
    }

    pub(crate) fn execute_power_action_now(&mut self, cx: &mut Context<Self>) {
        let Some(countdown) = self.clear_power_countdown_modal() else {
            return;
        };
        self.power_countdown_generation = self.power_countdown_generation.wrapping_add(1);
        let action = countdown.action;
        self.power_had_active_tasks = false;
        self.settings.platform.action_after_done = CompletionPowerActionView::None;
        self.sync_settings_panel_power_draft(cx);
        let settings = self.settings.clone();
        self.request_settings_save(
            settings,
            ProxyPasswordUpdateView::Unchanged,
            SettingsSaveSource::Platform,
            cx,
        );
        cx.emit(AppShellEvent::CompletionPowerActionRequested(action));
        self.sync_kit_modals_deferred(cx);
        cx.notify();
    }

    pub(crate) fn select_completion_action(
        &mut self,
        action: CompletionPowerActionView,
        cx: &mut Context<Self>,
    ) {
        if self.settings.platform.action_after_done == action {
            return;
        }
        self.settings.platform.action_after_done = action;
        self.sync_settings_panel_power_draft(cx);
        if action.is_armed() {
            if let Some(ActiveModal::PowerCountdown(countdown)) = &mut self.active_modal {
                countdown.action = action;
            } else {
                self.power_had_active_tasks = self.snapshot.counts.active > 0;
            }
            let action_verb = self.action_verb_label(action);
            self.show_notice(
                self.t_args(
                    "notice-power-action-armed",
                    &[("action", FluentValue::from(action_verb.as_str()))],
                ),
                false,
                cx,
            );
        } else {
            self.power_had_active_tasks = false;
            self.clear_power_countdown_modal();
            self.power_countdown_generation = self.power_countdown_generation.wrapping_add(1);
            self.sync_kit_modals_deferred(cx);
        }
        let settings = self.settings.clone();
        self.request_settings_save(
            settings,
            ProxyPasswordUpdateView::Unchanged,
            SettingsSaveSource::Platform,
            cx,
        );
        cx.notify();
    }

    pub(crate) fn toggle_keep_awake(&mut self, cx: &mut Context<Self>) {
        self.settings.platform.keep_awake_while_active =
            !self.settings.platform.keep_awake_while_active;
        self.sync_settings_panel_power_draft(cx);
        let settings = self.settings.clone();
        self.request_settings_save(
            settings,
            ProxyPasswordUpdateView::Unchanged,
            SettingsSaveSource::Platform,
            cx,
        );
        cx.notify();
    }

    fn sync_settings_panel_power_draft(&self, cx: &mut Context<Self>) {
        if let Some((panel, _)) = &self.settings_panel {
            let action = self.settings.platform.action_after_done;
            let keep_awake = self.settings.platform.keep_awake_while_active;
            panel.update(cx, |panel, cx| {
                panel.sync_power_draft(action, keep_awake, cx);
            });
        }
    }

    #[cfg(test)]
    pub(crate) fn toggle_power_popover(&mut self, cx: &mut Context<Self>) {
        if self.active_status_popover == Some(StatusBarPopover::Power) {
            self.active_status_popover = None;
        } else {
            self.context_menu_target = None;
            self.active_status_popover = Some(StatusBarPopover::Power);
        }
        cx.notify();
    }

    #[cfg(test)]
    pub(crate) fn close_power_popover(&mut self, cx: &mut Context<Self>) {
        if self.active_status_popover == Some(StatusBarPopover::Power) {
            self.active_status_popover = None;
            self.power_popover_previous_focus = None;
            cx.notify();
        }
    }

    pub(crate) fn close_power_popover_with_window(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.active_status_popover != Some(StatusBarPopover::Power) {
            return;
        }
        self.active_status_popover = None;
        self.restore_popover_focus(StatusBarPopover::Power, window, cx);
        cx.notify();
    }

    pub(crate) fn action_verb_label(&self, action: CompletionPowerActionView) -> String {
        match action {
            CompletionPowerActionView::None => self.t("settings-action-none"),
            CompletionPowerActionView::Sleep => self.t("power-action-verb-sleep"),
            CompletionPowerActionView::Hibernate => self.t("power-action-verb-hibernate"),
            CompletionPowerActionView::Shutdown => self.t("power-action-verb-shutdown"),
        }
    }

    pub(crate) fn action_btn_label(&self, action: CompletionPowerActionView) -> String {
        match action {
            CompletionPowerActionView::None => self.t("settings-action-none"),
            CompletionPowerActionView::Sleep => self.t("power-action-btn-sleep"),
            CompletionPowerActionView::Hibernate => self.t("power-action-btn-hibernate"),
            CompletionPowerActionView::Shutdown => self.t("power-action-btn-shutdown"),
        }
    }

    pub(crate) fn render_power_status_bar_item(&mut self, cx: &mut Context<Self>) -> Div {
        let colors = self.theme.colors;
        let open = self.active_status_popover == Some(StatusBarPopover::Power);
        let armed = self.settings.platform.action_after_done.is_armed();
        let action_name = self.t(self.settings.platform.action_after_done.message_key());
        let label = self.t_args(
            "power-status-label",
            &[("action", FluentValue::from(action_name.as_str()))],
        );
        let aria_label = self.t_args(
            "power-status-aria",
            &[("action", FluentValue::from(action_name.as_str()))],
        );

        let icon_color = if armed {
            colors.warning
        } else if open {
            colors.information
        } else {
            colors.text_muted
        };
        let trigger = KitButton::new("power-status")
            .ghost()
            .xsmall()
            .compact()
            .accessibility_label(aria_label)
            .icon(gpui_kit::component::Icon::new(IconName::Moon).text_color(icon_color))
            .label(label);
        let shell_change = cx.entity().downgrade();
        let shell_content = cx.entity().downgrade();
        div().h_full().flex().items_center().child(
            Popover::new("power-popover")
                .anchor(Anchor::BottomRight)
                .track_focus(&self.power_popover_focus)
                .open(open)
                .on_open_change(move |next_open, window, cx| {
                    let _ = shell_change.update(cx, |this, cx| {
                        if *next_open {
                            this.open_status_popover(StatusBarPopover::Power, window, cx);
                        } else {
                            this.close_power_popover_with_window(window, cx);
                        }
                    });
                })
                .trigger(trigger)
                .content(move |_, _, cx| {
                    shell_content
                        .update(cx, |this, cx| this.render_power_popover_content(cx))
                        .unwrap_or_else(|_| div().into_any_element())
                }),
        )
    }

    pub(crate) fn render_power_popover_content(
        &mut self,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let colors = self.theme.colors;
        let pending = self.pending_settings_save.is_some();
        let current_action = self.settings.platform.action_after_done;
        let keep_awake = self.settings.platform.keep_awake_while_active;

        div()
            .id("power-popover-body")
            .track_focus(&self.power_popover_focus)
            .w(px(240.0))
            .flex()
            .flex_col()
            .gap_2()
            .child(
                div()
                    .text_xs()
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(colors.text_primary)
                    .child(self.t("power-popover-title")),
            )
            .child({
                let mut options_col = div().flex().flex_col().gap_1();
                for action in CompletionPowerActionView::ALL {
                    let is_selected = current_action == action;
                    let shell = cx.entity().downgrade();
                    let key_shell = shell.clone();
                    let action_label = self.t(action.message_key());
                    options_col = options_col.child(
                        div()
                            .id(SharedString::from(format!(
                                "power-popover-action-{}",
                                action.as_str()
                            )))
                            .focusable()
                            .tab_stop(true)
                            .role(Role::Button)
                            .aria_selected(is_selected)
                            .border_1()
                            .border_color(gpui::transparent_black())
                            .focus_visible(|style| style.border_color(colors.focus_ring))
                            .flex()
                            .items_center()
                            .justify_between()
                            .px_2()
                            .py_1()
                            .rounded_md()
                            .text_xs()
                            .text_color(if is_selected {
                                colors.text_primary
                            } else {
                                colors.text_secondary
                            })
                            .hover(|style| style.bg(colors.surface_hover))
                            .active(|style| style.bg(colors.surface_active))
                            .on_key_down(move |event, _, cx| {
                                if !pending && is_activation_key(event) {
                                    cx.stop_propagation();
                                    key_shell
                                        .update(cx, |this, cx| {
                                            this.select_completion_action(action, cx);
                                        })
                                        .ok();
                                }
                            })
                            .on_click(move |_, _, cx| {
                                if !pending {
                                    shell
                                        .update(cx, |this, cx| {
                                            this.select_completion_action(action, cx);
                                        })
                                        .ok();
                                }
                            })
                            .child(action_label)
                            .when(is_selected, |el| {
                                el.child(
                                    Icon::new(IconName::CircleCheck)
                                        .size(IconSize::Small)
                                        .color(colors.accent),
                                )
                            }),
                    );
                }
                options_col
            })
            .child(div().h(px(1.0)).bg(colors.border).my_1())
            .child({
                let shell_toggle = cx.entity().downgrade();
                let keep_awake_supported = self.platform_capabilities.keep_awake_supported;
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .px_2()
                    .py_1()
                    .child(
                        div()
                            .text_xs()
                            .text_color(if keep_awake_supported {
                                colors.text_primary
                            } else {
                                colors.text_muted
                            })
                            .child(self.t("power-keep-awake")),
                    )
                    .child(
                        Switch::new("power-popover-keep-awake")
                            .checked(keep_awake_supported && keep_awake)
                            .accessibility_label(self.t("power-keep-awake"))
                            .disabled(pending || !keep_awake_supported)
                            .on_change(move |_, _, cx| {
                                shell_toggle
                                    .update(cx, |this, cx| {
                                        this.toggle_keep_awake(cx);
                                    })
                                    .ok();
                            }),
                    )
            })
            .into_any_element()
    }
}
