//! Settings `about` section logic and rendering for `AppShell`.

use super::*;

impl AppShell {
    pub(crate) fn render_settings_about(&mut self, cx: &mut Context<Self>) -> Div {
        let colors = self.theme.colors;
        let app_title = self.t("settings-about-app");
        let name_label = self.t("settings-about-name");
        let version_label = self.t("settings-about-version");
        let authors_label = self.t("settings-about-authors");
        let description_label = self.t("settings-about-description");
        let description_value = self.t("settings-about-description-value");
        let runtime_title = self.t("settings-about-runtime");
        let platform_label = self.t("settings-about-platform");
        let aria2_label = self.t("settings-about-aria2-version");
        let transfer_title = self.t("settings-transfer-title");
        let settings_export_label = self.t("settings-export");
        let settings_export_description = self.t("settings-export-description");
        let settings_export_aria = self.t("settings-export-aria");
        let settings_import_label = self.t("settings-import");
        let settings_import_description = self.t("settings-import-description");
        let settings_import_aria = self.t("settings-import-aria");
        let diagnostics_title = self.t("settings-diagnostics-title");
        let diagnostics_label = self.t("settings-diagnostics-export");
        let diagnostics_description = self.t("settings-diagnostics-description");
        let diagnostics_aria = self.t("settings-diagnostics-export-aria");
        let aria2_version = {
            let version = self.snapshot.capabilities.version.trim();
            if version.is_empty() {
                self.t("settings-about-aria2-unknown")
            } else {
                version.to_owned()
            }
        };
        let platform = format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH);
        let authors = env!("CARGO_PKG_AUTHORS");
        let authors = if authors.is_empty() {
            "AriaDeck contributors"
        } else {
            authors
        };
        let diagnostic_shell = cx.entity().downgrade();
        let diagnostic_button = KitButton::new("export-diagnostics")
            .icon(gpui_kit::component::Icon::new(IconName::Download))
            .label(diagnostics_label.clone())
            .accessibility_label(diagnostics_aria)
            .on_click(move |_, window, cx| {
                diagnostic_shell
                    .update(cx, |shell, cx| shell.export_diagnostics(window, cx))
                    .ok();
            });
        let transfer_pending = self.pending_settings_save.is_some();
        let export_shell = cx.entity().downgrade();
        let settings_export_button = KitButton::new("export-settings")
            .icon(gpui_kit::component::Icon::new(IconName::Download))
            .label(settings_export_label.clone())
            .accessibility_label(settings_export_aria)
            .disabled(transfer_pending)
            .on_click(move |_, window, cx| {
                export_shell
                    .update(cx, |shell, cx| shell.export_settings_file(window, cx))
                    .ok();
            });
        let import_shell = cx.entity().downgrade();
        let settings_import_button = KitButton::new("import-settings")
            .icon(gpui_kit::component::Icon::new(IconName::ArrowUp))
            .label(settings_import_label.clone())
            .accessibility_label(settings_import_aria)
            .disabled(transfer_pending)
            .on_click(move |_, window, cx| {
                import_shell
                    .update(cx, |shell, cx| shell.import_settings_file(window, cx))
                    .ok();
            });
        let update_check_shell = cx.entity().downgrade();
        let update_check_button = KitButton::new("check-for-updates")
            .icon(gpui_kit::component::Icon::new(IconName::RefreshCw))
            .label(self.t("settings-update-check"))
            .loading(matches!(
                self.update_status,
                crate::UpdateStatusView::Checking
            ))
            .disabled(
                !self.update_enabled
                    || matches!(
                        self.update_status,
                        crate::UpdateStatusView::Checking
                            | crate::UpdateStatusView::Downloading { .. }
                            | crate::UpdateStatusView::Downloaded { .. }
                    ),
            )
            .on_click(move |_, _, cx| {
                update_check_shell
                    .update(cx, |shell, cx| shell.request_update_check(cx))
                    .ok();
            });
        let update_info = match &self.update_status {
            crate::UpdateStatusView::Disabled => self.t("settings-update-disabled"),
            crate::UpdateStatusView::Checking => self.t("settings-update-checking"),
            crate::UpdateStatusView::Current => self.t("settings-update-current"),
            crate::UpdateStatusView::Downloading {
                downloaded_bytes,
                total_bytes: Some(total),
                ..
            } if *total > 0 => {
                let percent =
                    ((*downloaded_bytes as f64 / *total as f64) * 100.0).clamp(0.0, 100.0) as u64;
                self.t_args(
                    "settings-update-download-progress",
                    &[
                        (
                            "downloaded",
                            FluentValue::from(crate::format_bytes(*downloaded_bytes)),
                        ),
                        ("total", FluentValue::from(crate::format_bytes(*total))),
                        ("percent", FluentValue::from(percent.to_string())),
                    ],
                )
            }
            crate::UpdateStatusView::Downloading { .. } => self.t("settings-update-downloading"),
            crate::UpdateStatusView::Downloaded { .. } => self.t("settings-update-downloaded"),
            crate::UpdateStatusView::Failed(_) => self.t("settings-update-failed"),
            crate::UpdateStatusView::Available { version, .. } => self.t_args(
                "settings-update-available",
                &[("version", FluentValue::from(version.as_str()))],
            ),
        };
        let update_actions = match &self.update_status {
            crate::UpdateStatusView::Available {
                release_url,
                installer_available,
                ..
            } => {
                let release_link = Link::new("open-release-page")
                    .href(release_url.clone())
                    .child(self.t("settings-update-open-release"));
                let mut actions = div().flex().flex_row().gap_2().child(release_link);
                if *installer_available && !self.update_portable {
                    let download_shell = cx.entity().downgrade();
                    actions = actions.child(
                        KitButton::new("download-update-installer")
                            .primary()
                            .icon(gpui_kit::component::Icon::new(IconName::Download))
                            .label(self.t("settings-update-download-install"))
                            .on_click(move |_, _, cx| {
                                download_shell
                                    .update(cx, |shell, cx| shell.request_update_download(cx))
                                    .ok();
                            }),
                    );
                }
                actions
            }
            crate::UpdateStatusView::Downloaded { .. } if !self.update_portable => {
                let install_shell = cx.entity().downgrade();
                div().flex().child(
                    KitButton::new("install-update")
                        .primary()
                        .icon(gpui_kit::component::Icon::new(IconName::Check))
                        .label(self.t("settings-update-install"))
                        .on_click(move |_, _, cx| {
                            install_shell
                                .update(cx, |shell, cx| shell.request_update_install(cx))
                                .ok();
                        }),
                )
            }
            _ => div(),
        };
        let update_details = match &self.update_status {
            crate::UpdateStatusView::Downloading {
                downloaded_bytes,
                total_bytes: Some(total),
                ..
            } if *total > 0 => {
                let percent = ((*downloaded_bytes as f32 / *total as f32).clamp(0.0, 1.0)) * 100.0;
                div().w_full().pt_1().child(
                    KitProgress::new("update-download-progress")
                        .value(percent)
                        .xsmall()
                        .color(colors.progress_download),
                )
            }
            crate::UpdateStatusView::Available { title, notes, .. } => div()
                .flex()
                .flex_col()
                .gap_1()
                .child(
                    div()
                        .text_sm()
                        .font_weight(FontWeight::MEDIUM)
                        .child(title.clone()),
                )
                .child(
                    div()
                        .text_sm()
                        .text_color(colors.text_muted)
                        .child(notes.clone()),
                ),
            crate::UpdateStatusView::Failed(error) => div()
                .text_sm()
                .text_color(colors.danger)
                .child(error.clone()),
            _ => div(),
        };

        div()
            .flex()
            .flex_col()
            .gap_4()
            .child(
                settings_section_owned(app_title, colors)
                    .child(settings_section_info_row_owned(
                        name_label, "AriaDeck", colors,
                    ))
                    .child(settings_section_info_row_owned(
                        version_label,
                        crate::app_version(),
                        colors,
                    ))
                    .child(settings_section_info_row_owned(
                        description_label,
                        description_value,
                        colors,
                    ))
                    .child(settings_section_info_row_owned(
                        authors_label,
                        authors,
                        colors,
                    )),
            )
            .child(
                settings_section_owned(runtime_title, colors)
                    .child(settings_section_info_row_owned(
                        platform_label,
                        platform,
                        colors,
                    ))
                    .child(settings_section_info_row_owned(
                        aria2_label,
                        aria2_version,
                        colors,
                    )),
            )
            .when(self.update_enabled, |content| {
                content.child(
                    settings_section_owned(self.t("settings-update-title"), colors).child(
                        settings_section_row_owned(
                            self.t("settings-update-status"),
                            Some(update_info),
                            div()
                                .flex()
                                .flex_col()
                                .gap_2()
                                .child(update_check_button)
                                .child(update_actions)
                                .child(update_details),
                            colors,
                        ),
                    ),
                )
            })
            .child(
                settings_section_owned(transfer_title, colors)
                    .child(settings_section_row_owned(
                        settings_export_label,
                        Some(settings_export_description),
                        settings_export_button,
                        colors,
                    ))
                    .child(settings_section_row_owned(
                        settings_import_label,
                        Some(settings_import_description),
                        settings_import_button,
                        colors,
                    )),
            )
            .child(settings_section_owned(diagnostics_title, colors).child(
                settings_section_row_owned(
                    diagnostics_label,
                    Some(diagnostics_description),
                    diagnostic_button,
                    colors,
                ),
            ))
    }
}
