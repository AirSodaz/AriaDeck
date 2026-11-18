//! Settings `network` section logic and rendering for `AppShell`.

use super::*;

impl AppShell {
    pub(crate) fn select_proxy_mode(&mut self, mode: ProxyModeView, cx: &mut Context<Self>) {
        if self.pending_settings_save.is_some() || mode == self.settings_page.draft_proxy_mode {
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
        if self.pending_settings_save.is_some() || !self.settings.download_proxy.has_password {
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
        if self.page != AppPage::Settings || self.pending_settings_save.is_some() {
            return;
        }
        let mut settings = self.settings.clone();
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
        settings.download_proxy = DownloadProxySettingsView {
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
            has_password: match &password_update {
                ProxyPasswordUpdateView::Unchanged => self.settings.download_proxy.has_password,
                ProxyPasswordUpdateView::Detach => false,
                ProxyPasswordUpdateView::Clear => false,
                ProxyPasswordUpdateView::Set(_) => true,
            },
            check_certificate: self.settings_page.draft_check_certificate,
        };
        self.request_settings_save(settings, password_update, SettingsSaveSource::Proxy, cx);
    }

    pub(crate) fn toggle_check_certificate(&mut self, cx: &mut Context<Self>) {
        if self.pending_settings_save.is_some() {
            return;
        }
        self.settings_page.draft_check_certificate = !self.settings_page.draft_check_certificate;
        cx.notify();
    }

    pub(crate) fn render_settings_network(&mut self, cx: &mut Context<Self>) -> Div {
        let colors = self.theme.colors;
        let pending = self.pending_settings_save.is_some();
        let password_changed = !self
            .settings_inputs
            .proxy_password
            .read(cx)
            .value()
            .is_empty();
        let password_cleared = self.settings_page.clear_proxy_password;
        let proxy_has_password = if password_changed {
            true
        } else if password_cleared {
            false
        } else {
            self.settings.download_proxy.has_password
        };
        let proxy_draft = DownloadProxySettingsView {
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
            has_password: proxy_has_password,
            check_certificate: self.settings_page.draft_check_certificate,
        };
        let manual_proxy = proxy_draft.mode == ProxyModeView::Manual;
        let system_proxy = proxy_draft.mode == ProxyModeView::System;
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
        let proxy_shell = cx.entity().downgrade();
        let proxy_mode_control = SegmentedControl::new(
            "settings-proxy-mode",
            ProxyModeView::all().map(|mode| Segment::new(self.t(mode.message_key()))),
            proxy_draft.mode.index(),
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
            proxy_shell
                .update(cx, |shell, cx| shell.select_proxy_mode(mode, cx))
                .ok();
        });
        let cert_shell = cx.entity().downgrade();
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
                            cert_shell
                                .update(cx, |shell, cx| shell.toggle_check_certificate(cx))
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
