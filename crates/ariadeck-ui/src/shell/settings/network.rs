//! Settings `network` section logic and rendering for `SettingsPanelView`.

use super::*;

impl SettingsPanelView {
    /// Build the current proxy settings draft and password mutation from form state.
    pub(super) fn read_proxy_draft(
        &self,
        cx: &App,
    ) -> (DownloadProxySettingsView, ProxyPasswordUpdateView) {
        let password = self
            .settings_inputs
            .proxy_password
            .read(cx)
            .value()
            .to_owned();
        let password_update = if self.settings_page.clear_proxy_password {
            ProxyPasswordUpdateView::Clear
        } else if password.is_empty() {
            ProxyPasswordUpdateView::Unchanged
        } else {
            ProxyPasswordUpdateView::Set(SecretStringView::new(password))
        };
        let has_password = match &password_update {
            ProxyPasswordUpdateView::Unchanged => self.settings.download_proxy.has_password,
            ProxyPasswordUpdateView::Detach | ProxyPasswordUpdateView::Clear => false,
            ProxyPasswordUpdateView::Set(_) => true,
        };
        let draft = DownloadProxySettingsView {
            mode: self.settings_page.draft_proxy_mode,
            all_proxy: self
                .settings_inputs
                .all_proxy
                .read(cx)
                .value()
                .trim()
                .into(),
            http_proxy: self
                .settings_inputs
                .http_proxy
                .read(cx)
                .value()
                .trim()
                .into(),
            https_proxy: self
                .settings_inputs
                .https_proxy
                .read(cx)
                .value()
                .trim()
                .into(),
            ftp_proxy: self
                .settings_inputs
                .ftp_proxy
                .read(cx)
                .value()
                .trim()
                .into(),
            no_proxy: self
                .settings_inputs
                .no_proxy
                .read(cx)
                .value()
                .split(',')
                .map(str::trim)
                .filter(|entry| !entry.is_empty())
                .map(ToOwned::to_owned)
                .collect(),
            username: self
                .settings_inputs
                .proxy_username
                .read(cx)
                .value()
                .trim()
                .into(),
            has_password,
            check_certificate: self.settings_page.draft_check_certificate,
        };
        (draft, password_update)
    }

    pub(crate) fn select_proxy_mode(&mut self, mode: ProxyModeView, cx: &mut Context<Self>) {
        if self.is_pending() || mode == self.settings_page.draft_proxy_mode {
            return;
        }
        self.settings_page.draft_proxy_mode = mode;
        self.settings_page.error = None;
        cx.notify();
    }

    pub(crate) fn clear_saved_proxy_password(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.is_pending() || !self.settings.download_proxy.has_password {
            return;
        }
        let clear = !self.settings_page.clear_proxy_password;
        if clear {
            self.settings_inputs
                .proxy_password
                .update(cx, |input, cx| input.set_value("", window, cx));
        }
        self.settings_page.clear_proxy_password = clear;
        self.settings_page.error = None;
        cx.notify();
    }

    pub(crate) fn submit_proxy_settings(&mut self, cx: &mut Context<Self>) {
        if self.is_pending() {
            return;
        }
        let (draft, password_update) = self.read_proxy_draft(cx);
        let mut settings = self.settings.clone();
        settings.download_proxy = draft;
        self.request_settings_save(settings, password_update, SettingsSaveSource::Proxy, cx);
    }

    pub(crate) fn toggle_check_certificate(&mut self, cx: &mut Context<Self>) {
        if self.is_pending() {
            return;
        }
        self.settings_page.draft_check_certificate = !self.settings_page.draft_check_certificate;
        cx.notify();
    }

    pub(super) fn render_settings_network(&mut self, cx: &mut Context<Self>) -> Div {
        let colors = self.theme.colors;
        let pending = self.is_pending();
        let (proxy_draft, _) = self.read_proxy_draft(cx);
        let password_cleared = self.settings_page.clear_proxy_password;
        let proxy_has_password = proxy_draft.has_password;
        let manual_proxy = self.settings_page.draft_proxy_mode == ProxyModeView::Manual;
        let system_proxy = self.settings_page.draft_proxy_mode == ProxyModeView::System;
        let draft_check_certificate = self.settings_page.draft_check_certificate;
        let password_button_label = if password_cleared {
            self.t("settings-proxy-password-keep")
        } else {
            self.t("settings-proxy-password-clear")
        };
        let password_button_icon = if password_cleared {
            IconName::RotateCcw
        } else {
            IconName::Trash2
        };
        let proxy_panel = cx.entity().downgrade();
        let proxy_mode_control = SegmentedControl::new(
            "settings-proxy-mode",
            ProxyModeView::all().map(|mode| Segment::new(self.t(mode.message_key()))),
            self.settings_page.draft_proxy_mode.index(),
            self.theme,
        )
        .aria_label(self.t("settings-proxy-mode-aria"))
        .disabled(pending)
        .on_select(move |index, _window, cx| {
            let mode = match index {
                1 => ProxyModeView::System,
                2 => ProxyModeView::Manual,
                _ => ProxyModeView::Disabled,
            };
            proxy_panel
                .update(cx, |panel, cx| panel.select_proxy_mode(mode, cx))
                .ok();
        });
        let cert_panel = cx.entity().downgrade();
        settings_section_owned(self.t("settings-network-proxy"), colors)
            .child(
                div()
                    .py_3()
                    .border_b_1()
                    .border_color(colors.border)
                    .flex()
                    .items_start()
                    .child(proxy_mode_control),
            )
            .child(
                div().max_w(px(760.0)).child(settings_section_row_owned(
                    self.t("settings-proxy-check-certificate"),
                    Some(self.t("settings-proxy-check-certificate-desc")),
                    Switch::new("toggle-check-certificate")
                        .checked(draft_check_certificate)
                        .accessibility_label(self.t("settings-proxy-check-certificate-aria"))
                        .disabled(pending)
                        .on_change(move |_, _, cx| {
                            cert_panel
                                .update(cx, |panel, cx| panel.toggle_check_certificate(cx))
                                .ok();
                        }),
                    colors,
                )),
            )
            .when(system_proxy, |section| {
                section.child(
                    div()
                        .py_3()
                        .max_w(px(760.0))
                        .text_xs()
                        .text_color(colors.text_muted)
                        .child(self.t("settings-proxy-system-hint")),
                )
            })
            .when(manual_proxy, |section| {
                section.child(
                    div()
                        .pt_4()
                        .max_w(px(760.0))
                        .flex()
                        .flex_col()
                        .gap_3()
                        .child(settings_labeled_input(
                            self.t("settings-proxy-all"),
                            &self.settings_inputs.all_proxy,
                            colors,
                        ))
                        .child(
                            div()
                                .flex()
                                .gap_3()
                                .child(
                                    settings_labeled_input(
                                        self.t("settings-proxy-http"),
                                        &self.settings_inputs.http_proxy,
                                        colors,
                                    )
                                    .flex_1()
                                    .min_w_0(),
                                )
                                .child(
                                    settings_labeled_input(
                                        self.t("settings-proxy-https"),
                                        &self.settings_inputs.https_proxy,
                                        colors,
                                    )
                                    .flex_1()
                                    .min_w_0(),
                                ),
                        )
                        .child(settings_labeled_input(
                            self.t("settings-proxy-ftp"),
                            &self.settings_inputs.ftp_proxy,
                            colors,
                        ))
                        .child(settings_labeled_input(
                            self.t("settings-proxy-bypass"),
                            &self.settings_inputs.no_proxy,
                            colors,
                        ))
                        .child(
                            div()
                                .flex()
                                .gap_3()
                                .items_end()
                                .child(
                                    settings_labeled_input(
                                        self.t("settings-proxy-username"),
                                        &self.settings_inputs.proxy_username,
                                        colors,
                                    )
                                    .flex_1()
                                    .min_w_0(),
                                )
                                .child(
                                    settings_labeled_input(
                                        self.t("settings-proxy-password"),
                                        &self.settings_inputs.proxy_password,
                                        colors,
                                    )
                                    .flex_1()
                                    .min_w_0(),
                                )
                                .when(self.settings.download_proxy.has_password, |row| {
                                    row.child(
                                        KitButton::new("clear-proxy-password")
                                            .ghost()
                                            .small()
                                            .compact()
                                            .icon(gpui_kit::component::Icon::new(
                                                password_button_icon,
                                            ))
                                            .accessibility_label(password_button_label.clone())
                                            .disabled(pending)
                                            .tooltip(password_button_label.clone())
                                            .on_click(cx.listener(|this, _, window, cx| {
                                                this.clear_saved_proxy_password(window, cx);
                                            })),
                                    )
                                }),
                        )
                        .when(proxy_has_password, |form| {
                            form.child(
                                div()
                                    .text_xs()
                                    .text_color(colors.text_muted)
                                    .child(self.t("settings-proxy-password-saved")),
                            )
                        }),
                )
            })
            .when(
                !manual_proxy && self.settings.download_proxy.has_password,
                |section| {
                    section.child(
                        div()
                            .py_3()
                            .max_w(px(760.0))
                            .flex()
                            .items_center()
                            .gap_2()
                            .text_xs()
                            .text_color(colors.text_muted)
                            .child(if password_cleared {
                                self.t("settings-proxy-password-remove-hint")
                            } else {
                                self.t("settings-proxy-password-saved")
                            })
                            .child(
                                KitButton::new("clear-disabled-proxy-password")
                                    .ghost()
                                    .small()
                                    .compact()
                                    .icon(gpui_kit::component::Icon::new(password_button_icon))
                                    .accessibility_label(password_button_label.clone())
                                    .disabled(pending)
                                    .tooltip(password_button_label.clone())
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.clear_saved_proxy_password(window, cx);
                                    })),
                            ),
                    )
                },
            )
    }
}
