use std::time::Duration;

use ariadeck_i18n::FluentValue;
use gpui::{
    AnyElement, Context, Div, FontWeight, IntoElement, Role, SharedString, Stateful, div,
    prelude::*, px,
};

use super::{AppShell, AppShellEvent, ProxyPasswordUpdateView, SettingsSaveSource};
use crate::{
    Button, ButtonStyle, CompletionPowerActionView, Dialog, Icon, IconName, IconSize, Toggle,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PowerCountdownState {
    pub action: CompletionPowerActionView,
    pub remaining_seconds: u32,
}

impl AppShell {
    pub(crate) fn check_power_action_trigger(&mut self, cx: &mut Context<Self>) {
        if !self.settings.platform.action_after_done.is_armed() {
            return;
        }

        if self.snapshot.counts.active > 0 {
            self.power_had_active_tasks = true;
            // If a countdown was running and a new task started downloading, cancel it.
            if self.power_countdown.is_some() {
                self.power_countdown = None;
                self.power_countdown_generation = self.power_countdown_generation.wrapping_add(1);
                cx.notify();
            }
        } else if self.power_had_active_tasks
            && self.snapshot.counts.active == 0
            && self.snapshot.counts.waiting == 0
            && self.power_countdown.is_none()
        {
            self.start_power_countdown(self.settings.platform.action_after_done, cx);
        }
    }

    pub(crate) fn start_power_countdown(
        &mut self,
        action: CompletionPowerActionView,
        cx: &mut Context<Self>,
    ) {
        self.power_countdown = Some(PowerCountdownState {
            action,
            remaining_seconds: 30,
        });
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
        let Some(countdown) = &mut self.power_countdown else {
            return false;
        };

        if countdown.remaining_seconds > 1 {
            countdown.remaining_seconds -= 1;
            cx.notify();
            true
        } else {
            let action = countdown.action;
            self.power_countdown = None;
            self.power_had_active_tasks = false;
            self.settings.platform.action_after_done = CompletionPowerActionView::None;
            self.settings_page.draft_action_after_done = CompletionPowerActionView::None;
            let settings = self.settings.clone();
            self.request_settings_save(
                settings,
                ProxyPasswordUpdateView::Unchanged,
                SettingsSaveSource::Platform,
                cx,
            );
            cx.emit(AppShellEvent::CompletionPowerActionRequested(action));
            cx.notify();
            false
        }
    }

    pub(crate) fn cancel_power_countdown(&mut self, cx: &mut Context<Self>) {
        self.power_countdown = None;
        self.power_countdown_generation = self.power_countdown_generation.wrapping_add(1);
        self.power_had_active_tasks = false;
        self.settings.platform.action_after_done = CompletionPowerActionView::None;
        self.settings_page.draft_action_after_done = CompletionPowerActionView::None;
        let settings = self.settings.clone();
        self.request_settings_save(
            settings,
            ProxyPasswordUpdateView::Unchanged,
            SettingsSaveSource::Platform,
            cx,
        );
        self.show_notice(self.t("notice-power-action-cancelled"), false, cx);
        cx.notify();
    }

    pub(crate) fn execute_power_action_now(&mut self, cx: &mut Context<Self>) {
        let Some(countdown) = self.power_countdown.take() else {
            return;
        };
        self.power_countdown_generation = self.power_countdown_generation.wrapping_add(1);
        let action = countdown.action;
        self.power_had_active_tasks = false;
        self.settings.platform.action_after_done = CompletionPowerActionView::None;
        self.settings_page.draft_action_after_done = CompletionPowerActionView::None;
        let settings = self.settings.clone();
        self.request_settings_save(
            settings,
            ProxyPasswordUpdateView::Unchanged,
            SettingsSaveSource::Platform,
            cx,
        );
        cx.emit(AppShellEvent::CompletionPowerActionRequested(action));
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
        self.settings_page.draft_action_after_done = action;
        if action.is_armed() {
            if let Some(countdown) = &mut self.power_countdown {
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
            self.power_countdown = None;
            self.power_countdown_generation = self.power_countdown_generation.wrapping_add(1);
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
        self.settings_page.draft_keep_awake_while_active =
            self.settings.platform.keep_awake_while_active;
        let settings = self.settings.clone();
        self.request_settings_save(
            settings,
            ProxyPasswordUpdateView::Unchanged,
            SettingsSaveSource::Platform,
            cx,
        );
        cx.notify();
    }

    pub(crate) fn toggle_power_popover(&mut self, cx: &mut Context<Self>) {
        self.power_popover_open = !self.power_popover_open;
        if self.power_popover_open {
            self.speed_popover_open = false;
            self.sort_popover_open = false;
            self.activity_panel_open = false;
            self.context_menu = None;
        }
        cx.notify();
    }

    pub(crate) fn close_power_popover(&mut self, cx: &mut Context<Self>) {
        if self.power_popover_open {
            self.power_popover_open = false;
            cx.notify();
        }
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

    pub(crate) fn render_power_status_bar_item(&mut self, cx: &mut Context<Self>) -> Stateful<Div> {
        let colors = self.theme.colors;
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
        } else if self.power_popover_open {
            colors.information
        } else {
            colors.text_muted
        };

        div()
            .id("power-status")
            .focusable()
            .tab_stop(true)
            .role(Role::Button)
            .aria_label(aria_label)
            .aria_expanded(self.power_popover_open)
            .ml_auto()
            .h_full()
            .px_2()
            .flex()
            .items_center()
            .gap_1()
            .text_xs()
            .text_color(if armed || self.power_popover_open {
                colors.text_primary
            } else {
                colors.text_muted
            })
            .cursor_pointer()
            .hover(|style| style.bg(colors.surface_hover))
            .focus_visible(|style| style.bg(colors.surface_active))
            .on_click(cx.listener(|this, _, _, cx| {
                this.toggle_power_popover(cx);
            }))
            .child(
                Icon::new(IconName::Moon)
                    .size(IconSize::XSmall)
                    .color(icon_color),
            )
            .child(label)
    }

    pub(crate) fn render_power_popover(&mut self, cx: &mut Context<Self>) -> Stateful<Div> {
        let colors = self.theme.colors;
        let pending = self.pending_settings_save.is_some();
        let current_action = self.settings.platform.action_after_done;
        let keep_awake = self.settings.platform.keep_awake_while_active;

        div()
            .id("power-popover-layer")
            .absolute()
            .inset_0()
            .occlude()
            .on_click(cx.listener(|this, _, _, cx| {
                this.close_power_popover(cx);
            }))
            .child(
                div()
                    .id("power-popover")
                    .absolute()
                    .right(px(160.0))
                    .bottom(px(34.0))
                    .w(px(260.0))
                    .flex()
                    .flex_col()
                    .gap_2()
                    .p_3()
                    .rounded_lg()
                    .border_1()
                    .border_color(colors.border)
                    .bg(colors.elevated_surface)
                    .shadow_lg()
                    .on_click(|_, _, cx| cx.stop_propagation())
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
                                    .cursor_pointer()
                                    .hover(|style| style.bg(colors.surface_hover))
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
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .px_2()
                            .py_1()
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(colors.text_primary)
                                    .child(self.t("power-keep-awake")),
                            )
                            .child(
                                Toggle::new("power-popover-keep-awake", keep_awake)
                                    .aria_label(self.t("power-keep-awake"))
                                    .disabled(pending)
                                    .on_click(move |_, _, cx| {
                                        shell_toggle
                                            .update(cx, |this, cx| {
                                                this.toggle_keep_awake(cx);
                                            })
                                            .ok();
                                    })
                                    .render(colors),
                            )
                    }),
            )
    }

    pub(crate) fn render_power_countdown_dialog(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let colors = self.theme.colors;
        let (action, seconds) = self
            .power_countdown
            .map(|c| (c.action, c.remaining_seconds))
            .unwrap_or((CompletionPowerActionView::None, 0));

        let action_verb = self.action_verb_label(action);
        let action_btn = self.action_btn_label(action);
        let description = self.t_args(
            "dialog-power-countdown-desc",
            &[
                ("action", FluentValue::from(action_verb.as_str())),
                ("seconds", FluentValue::from(seconds.to_string().as_str())),
            ],
        );

        Dialog::new(
            "power-countdown-dialog",
            self.t("dialog-power-countdown-title"),
            self.theme,
        )
        .description(description)
        .key_context("PowerCountdownDialog")
        .track_focus(self.power_countdown_dialog_focus.clone())
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
                        .color(colors.warning),
                )
                .child(format!("{seconds}s")),
        )
        .action(
            Button::new("cancel-power-countdown", self.t("dialog-power-cancel"))
                .aria_label(self.t("dialog-power-cancel"))
                .style(ButtonStyle::Secondary)
                .track_focus(self.power_countdown_cancel_focus.clone())
                .on_click(cx.listener(|this, _, _, cx| {
                    this.cancel_power_countdown(cx);
                }))
                .render(colors),
        )
        .action(
            Button::new("execute-power-now", action_btn.clone())
                .aria_label(action_btn)
                .style(ButtonStyle::Primary)
                .track_focus(self.power_countdown_submit_focus.clone())
                .on_click(cx.listener(|this, _, _, cx| {
                    this.execute_power_action_now(cx);
                }))
                .render(colors),
        )
        .into_any_element()
    }
}
