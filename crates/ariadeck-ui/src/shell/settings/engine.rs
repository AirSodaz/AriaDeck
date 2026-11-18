//! Settings `engine` section logic and rendering for `AppShell`.

use super::*;

impl AppShell {
    pub(crate) fn render_settings_engine(&mut self, cx: &mut Context<Self>) -> Div {
        let colors = self.theme.colors;
        let cores = self.cores.clone();
        let can_rollback = cores
            .last_working_id
            .as_ref()
            .is_some_and(|id| cores.active_id.as_ref() != Some(id));
        settings_section_owned(self.t("settings-engine-cores"), colors)
            .child(
                div()
                    .mt_3()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child(div().text_xs().text_color(colors.text_muted).child(
                        if cores.installations.is_empty() {
                            self.t("settings-core-empty")
                        } else {
                            let active = cores.active().map_or_else(
                                || self.t("settings-core-none"),
                                |core| core.version.clone(),
                            );
                            self.t_args(
                                "settings-core-summary",
                                &[
                                    (
                                        "count",
                                        FluentValue::from(
                                            i64::try_from(cores.installations.len())
                                                .unwrap_or(i64::MAX),
                                        ),
                                    ),
                                    ("active", FluentValue::from(active.as_str())),
                                ],
                            )
                        },
                    ))
                    .children(cores.installations.into_iter().map(|core| {
                        let core_id = core.id.clone();
                        let activate_id = core_id.clone();
                        let verify_id = core_id.clone();
                        let remove_id = core_id.clone();
                        let is_active = core.is_active;
                        div()
                            .id(SharedString::from(format!("core-row-{}", core.id)))
                            .flex()
                            .flex_col()
                            .gap_2()
                            .px_3()
                            .py_2()
                            .rounded_md()
                            .border_1()
                            .border_color(if is_active {
                                colors.accent
                            } else {
                                colors.border
                            })
                            .bg(if is_active {
                                with_alpha(colors.accent, 0.08)
                            } else {
                                colors.surface
                            })
                            .child(
                                div().flex().items_center().gap_2().child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .flex()
                                        .flex_col()
                                        .gap_0p5()
                                        .child(
                                            div()
                                                .text_sm()
                                                .font_weight(FontWeight::MEDIUM)
                                                .text_color(colors.text_primary)
                                                .child(self.t_args(
                                                    "settings-core-version-label",
                                                    &[(
                                                        "version",
                                                        FluentValue::from(core.version.as_str()),
                                                    )],
                                                )),
                                        )
                                        .child({
                                            let mut meta = format!(
                                                "{} · {} · {}",
                                                self.t(core.source.message_key()),
                                                core.target,
                                                self.t(core.status.message_key()),
                                            );
                                            if core.is_last_working {
                                                meta.push_str(" · ");
                                                meta.push_str(
                                                    &self.t("settings-core-last-working"),
                                                );
                                            }
                                            div()
                                                .text_xs()
                                                .text_color(colors.text_muted)
                                                .child(meta)
                                        })
                                        .child(
                                            div().text_xs().text_color(colors.text_muted).child(
                                                if core.executable.is_empty() {
                                                    self.t("settings-core-executable-missing")
                                                } else {
                                                    core.executable.clone()
                                                },
                                            ),
                                        ),
                                ),
                            )
                            .child(
                                div()
                                    .flex()
                                    .flex_wrap()
                                    .gap_2()
                                    .child({
                                        let mut button = KitButton::new(SharedString::from(
                                            format!("activate-core-{}", activate_id),
                                        ))
                                        .label(if is_active {
                                            self.t("settings-item-active")
                                        } else {
                                            self.t("settings-item-activate")
                                        })
                                        .accessibility_label(if is_active {
                                            self.t_args(
                                                "settings-core-active-aria",
                                                &[(
                                                    "version",
                                                    FluentValue::from(core.version.as_str()),
                                                )],
                                            )
                                        } else {
                                            self.t_args(
                                                "settings-core-activate-aria",
                                                &[(
                                                    "version",
                                                    FluentValue::from(core.version.as_str()),
                                                )],
                                            )
                                        })
                                        .disabled(is_active)
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            this.request_core_command(
                                                CoreCommandView::Activate {
                                                    core_id: activate_id.clone(),
                                                },
                                                cx,
                                            );
                                        }));
                                        if !is_active {
                                            button = button.primary();
                                        }
                                        button
                                    })
                                    .child(
                                        KitButton::new(SharedString::from(format!(
                                            "verify-core-{}",
                                            verify_id
                                        )))
                                        .label(self.t("settings-core-verify"))
                                        .accessibility_label(self.t_args(
                                            "settings-core-verify-aria",
                                            &[(
                                                "version",
                                                FluentValue::from(core.version.as_str()),
                                            )],
                                        ))
                                        .on_click(
                                            cx.listener(move |this, _, _, cx| {
                                                this.request_core_command(
                                                    CoreCommandView::Verify {
                                                        core_id: verify_id.clone(),
                                                    },
                                                    cx,
                                                );
                                            }),
                                        ),
                                    )
                                    .child(
                                        KitButton::new(SharedString::from(format!(
                                            "remove-core-{}",
                                            remove_id
                                        )))
                                        .label(self.t("settings-core-remove"))
                                        .accessibility_label(self.t_args(
                                            "settings-core-remove-aria",
                                            &[(
                                                "version",
                                                FluentValue::from(core.version.as_str()),
                                            )],
                                        ))
                                        .disabled(is_active)
                                        .on_click(
                                            cx.listener(move |this, _, _, cx| {
                                                this.request_core_command(
                                                    CoreCommandView::Remove {
                                                        core_id: remove_id.clone(),
                                                    },
                                                    cx,
                                                );
                                            }),
                                        ),
                                    ),
                            )
                    })),
            )
            .child(
                div().mt_3().flex().child(
                    KitButton::new("rollback-core")
                        .label(self.t("settings-core-rollback"))
                        .accessibility_label(self.t("settings-core-rollback-aria"))
                        .disabled(!can_rollback)
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.request_core_command(CoreCommandView::Rollback, cx);
                        })),
                ),
            )
            // B4: the same Get-aria2 panel the first-run guide shows. Discovery,
            // verified download, and manual import are not onboarding-only —
            // switching or repairing a core later needs exactly these controls.
            .child(
                div()
                    .mt_4()
                    .pt_4()
                    .border_t_1()
                    .border_color(colors.border)
                    .flex()
                    .flex_col()
                    .gap_3()
                    .child(settings_section_heading(
                        self.t("core-setup-section"),
                        colors,
                    ))
                    .child(self.render_core_setup_panel("settings", cx)),
            )
    }
}
