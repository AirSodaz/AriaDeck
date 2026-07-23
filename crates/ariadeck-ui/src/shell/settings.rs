//! Settings page logic and render paths for AppShell.

use super::*;

impl AppShell {
    // --- set_settings_save_result..set_settings_save_result ---
    pub fn set_settings_save_result(
        &mut self,
        result: SettingsSaveResultView,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(pending) = self.pending_settings_save.as_ref() else {
            return;
        };
        if pending.request_id != result.request_id || pending.settings != result.settings {
            return;
        }
        let source = pending.source;
        self.pending_settings_save = None;

        match result.outcome {
            SettingsSaveOutcomeView::Success => {
                self.apply_settings(result.settings, cx);
                self.settings_page.error = None;
                let message = match source {
                    SettingsSaveSource::Theme => self.t("notice-settings-appearance"),
                    SettingsSaveSource::Language => self.t("settings-language-saved"),
                    SettingsSaveSource::Directory => self.t("notice-settings-directory"),
                    SettingsSaveSource::Proxy => {
                        self.settings_inputs
                            .proxy_password
                            .update(cx, |input, cx| input.set_text("", cx));
                        self.settings_page.clear_proxy_password = false;
                        self.t("notice-settings-proxy")
                    }
                    SettingsSaveSource::SpeedLimit => {
                        self.sync_speed_limit_fields_from_settings(cx);
                        self.t("notice-settings-speed")
                    }
                    SettingsSaveSource::TransferPolicy => {
                        self.sync_transfer_policy_fields_from_settings(cx);
                        self.t("notice-settings-transfer-policy")
                    }
                    SettingsSaveSource::Transfers => {
                        self.sync_speed_limit_fields_from_settings(cx);
                        self.sync_transfer_policy_fields_from_settings(cx);
                        self.t("notice-settings-transfers")
                    }
                    SettingsSaveSource::Notifications => self.t("notice-settings-notifications"),
                    SettingsSaveSource::Platform => self.t("notice-settings-platform"),
                    SettingsSaveSource::Import => self.t("notice-settings-imported"),
                };
                self.show_notice(message, false, cx);
            }
            SettingsSaveOutcomeView::Failure(error) => {
                if source == SettingsSaveSource::Import {
                    self.show_notice(self.te(&error), true, cx);
                } else {
                    self.settings_page.error = Some(error);
                    cx.notify();
                }
            }
        }
        let _ = window;
    }

    pub fn set_settings_export_result(
        &mut self,
        result: SettingsExportResultView,
        cx: &mut Context<Self>,
    ) {
        match result.outcome {
            SettingsExportOutcomeView::Success => self.show_notice(
                self.t_args(
                    "notice-settings-exported",
                    &[("path", FluentValue::from(result.path))],
                ),
                false,
                cx,
            ),
            SettingsExportOutcomeView::Failure(error) => {
                self.show_notice(self.te(&error), true, cx);
            }
        }
    }

    pub fn set_settings_import_result(
        &mut self,
        result: SettingsImportResultView,
        cx: &mut Context<Self>,
    ) {
        match result.outcome {
            SettingsImportOutcomeView::Ready(settings) => {
                if self.pending_settings_save.is_some() {
                    let error = OperationErrorView {
                        code: "settings.import_busy".into(),
                        summary: self.t("error-settings-import-busy"),
                        retryable: true,
                    };
                    self.show_notice(self.te(&error), true, cx);
                    return;
                }
                let settings = *settings;
                let preserve_proxy_credential = self.settings.download_proxy.mode
                    == settings.download_proxy.mode
                    && self.settings.download_proxy.all_proxy == settings.download_proxy.all_proxy
                    && self.settings.download_proxy.http_proxy
                        == settings.download_proxy.http_proxy
                    && self.settings.download_proxy.https_proxy
                        == settings.download_proxy.https_proxy
                    && self.settings.download_proxy.ftp_proxy == settings.download_proxy.ftp_proxy
                    && self.settings.download_proxy.no_proxy == settings.download_proxy.no_proxy
                    && self.settings.download_proxy.username == settings.download_proxy.username;
                let credential_update =
                    if self.settings.download_proxy.has_password && !preserve_proxy_credential {
                        ProxyPasswordUpdateView::Detach
                    } else {
                        ProxyPasswordUpdateView::Unchanged
                    };
                self.request_settings_save(
                    settings,
                    credential_update,
                    SettingsSaveSource::Import,
                    cx,
                );
            }
            SettingsImportOutcomeView::Failure(error) => {
                self.show_notice(self.te(&error), true, cx);
            }
        }
    }

    // --- apply_settings..apply_settings ---
    pub(crate) fn apply_settings(&mut self, settings: SettingsView, cx: &mut Context<Self>) {
        // System scheme resolution needs a Window; callers that only have Context
        // keep the previously resolved palette until the next appearance event.
        if settings.color_scheme != ColorSchemeView::System {
            self.theme = theme_for_scheme(settings.color_scheme);
        }
        self.settings = settings.clone();
        self.settings_page.draft_color_scheme = settings.color_scheme;
        self.settings_page.draft_categories = settings.categories.clone();
        self.settings_page.draft_default_category_id = settings.default_category_id.clone();
        self.settings_page.draft_language = settings.language;
        self.set_language_runtime(settings.language);
        self.settings_page.draft_file_allocation = settings.transfer_policy.file_allocation;
        self.settings_page.draft_check_certificate = settings.download_proxy.check_certificate;
        self.settings_page.draft_check_integrity = settings.transfer_policy.check_integrity;
        self.settings_page.draft_notification_volume = settings.notifications.volume;
        self.settings_page.draft_notify_on_completion = settings.notifications.notify_on_completion;
        self.settings_page.draft_notify_on_error = settings.notifications.notify_on_error;
        self.settings_page.draft_notify_on_engine_events =
            settings.notifications.notify_on_engine_events;
        self.settings_page.draft_os_notifications = settings.notifications.os_notifications;
        self.settings_page.draft_notify_on_low_disk = settings.notifications.notify_on_low_disk;
        self.settings_page.draft_close_behavior = settings.platform.close_behavior;
        self.settings_page.draft_show_tray_icon = settings.platform.show_tray_icon;
        self.settings_page.draft_start_minimized_to_tray =
            settings.platform.start_minimized_to_tray;
        self.apply_theme_to_text_fields(cx);
    }

    /// Push the current shell theme into every TextField so light/dark chrome stays in sync.
    // --- open_settings..toggle_start_minimized_to_tray ---
    pub(crate) fn open_settings(
        &mut self,
        _: &OpenSettings,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.page == AppPage::Settings {
            window.focus(&self.settings_inputs.directory.focus_handle(cx), cx);
            return;
        }
        if self.add_dialog.open
            || self.output_name_dialog.is_some()
            || self.remove_confirmation.is_some()
            || self.batch_failure_details.is_some()
        {
            return;
        }
        let download_directory = self.settings.download_directory.clone();
        self.settings_inputs
            .directory
            .update(cx, |input, cx| input.set_text(download_directory, cx));
        let proxy = self.settings.download_proxy.clone();
        self.settings_inputs.all_proxy.update(cx, |input, cx| {
            input.set_text(proxy.all_proxy.clone(), cx);
        });
        self.settings_inputs.http_proxy.update(cx, |input, cx| {
            input.set_text(proxy.http_proxy.clone(), cx);
        });
        self.settings_inputs.https_proxy.update(cx, |input, cx| {
            input.set_text(proxy.https_proxy.clone(), cx);
        });
        self.settings_inputs.ftp_proxy.update(cx, |input, cx| {
            input.set_text(proxy.ftp_proxy.clone(), cx);
        });
        self.settings_inputs.no_proxy.update(cx, |input, cx| {
            input.set_text(proxy.no_proxy.join(", "), cx);
        });
        self.settings_inputs.proxy_username.update(cx, |input, cx| {
            input.set_text(proxy.username.clone(), cx);
        });
        self.settings_inputs
            .proxy_password
            .update(cx, |input, cx| input.set_text("", cx));
        let speed_limits = self.settings.speed_limits.clone();
        self.settings_inputs.download_limit.update(cx, |input, cx| {
            input.set_text(speed_limits.download_limit.clone(), cx);
        });
        self.settings_inputs.upload_limit.update(cx, |input, cx| {
            input.set_text(speed_limits.upload_limit.clone(), cx);
        });
        let transfer_policy = self.settings.transfer_policy.clone();
        self.settings_inputs.max_concurrent.update(cx, |input, cx| {
            input.set_text(transfer_policy.max_concurrent_downloads.clone(), cx);
        });
        self.settings_inputs.max_connection.update(cx, |input, cx| {
            input.set_text(transfer_policy.max_connection_per_server.clone(), cx);
        });
        self.settings_inputs.split.update(cx, |input, cx| {
            input.set_text(transfer_policy.split.clone(), cx);
        });
        self.settings_inputs.min_split_size.update(cx, |input, cx| {
            input.set_text(transfer_policy.min_split_size.clone(), cx);
        });
        self.page = AppPage::Settings;
        self.details_drawer = None;
        self.speed_popover_open = false;
        self.activity_panel_open = false;
        self.settings_page = SettingsPage {
            previous_focus: window.focused(cx).map(|focus| focus.downgrade()),
            active_category: SettingsCategory::default(),
            draft_color_scheme: self.settings.color_scheme,
            draft_language: self.settings.language,
            draft_proxy_mode: proxy.mode,
            draft_check_certificate: proxy.check_certificate,
            draft_file_allocation: transfer_policy.file_allocation,
            draft_check_integrity: transfer_policy.check_integrity,
            draft_notification_volume: self.settings.notifications.volume,
            draft_notify_on_completion: self.settings.notifications.notify_on_completion,
            draft_notify_on_error: self.settings.notifications.notify_on_error,
            draft_notify_on_engine_events: self.settings.notifications.notify_on_engine_events,
            draft_os_notifications: self.settings.notifications.os_notifications,
            draft_notify_on_low_disk: self.settings.notifications.notify_on_low_disk,
            draft_close_behavior: self.settings.platform.close_behavior,
            draft_show_tray_icon: self.settings.platform.show_tray_icon,
            draft_start_minimized_to_tray: self.settings.platform.start_minimized_to_tray,
            draft_categories: self.settings.categories.clone(),
            draft_default_category_id: self.settings.default_category_id.clone(),
            clear_proxy_password: false,
            editing_profile_id: None,
            draft_profile_kind: ProfileKindView::LocalManaged,
            profile_secret_updates: std::collections::HashMap::new(),
            pending_profile_delete: None,
            clear_profile_rpc_secret: false,
            error: None,
        };
        cx.notify();
    }

    pub(crate) fn close_settings_action(
        &mut self,
        _: &CloseSettings,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.close_settings(window, cx);
    }

    pub(crate) fn close_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.page != AppPage::Settings {
            return;
        }
        let previous_focus = self.settings_page.previous_focus.take();
        self.page = AppPage::Downloads;
        if let Some(focus) = previous_focus.and_then(|focus| focus.upgrade()) {
            window.focus(&focus, cx);
        } else {
            window.focus(&self.focus_handle, cx);
        }
        cx.notify();
    }

    pub(crate) fn save_settings_action(
        &mut self,
        _: &SaveSettings,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.submit_settings(cx);
    }

    pub(crate) fn submit_settings(&mut self, cx: &mut Context<Self>) {
        if self.page != AppPage::Settings || self.pending_settings_save.is_some() {
            return;
        }
        let download_directory = self
            .settings_inputs
            .directory
            .read(cx)
            .text()
            .trim()
            .to_owned();
        if download_directory.is_empty() {
            self.settings_page.error = Some(OperationErrorView {
                code: "settings.invalid_download_directory".into(),
                summary: self.t("error-settings-invalid-download-directory"),
                retryable: false,
            });
            cx.notify();
            return;
        }
        let mut settings = self.settings.clone();
        settings.download_directory = download_directory;
        settings.categories = self.settings_page.draft_categories.clone();
        settings.default_category_id = self.settings_page.draft_default_category_id.clone();
        self.request_settings_save(
            settings,
            ProxyPasswordUpdateView::Unchanged,
            SettingsSaveSource::Directory,
            cx,
        );
    }

    pub(crate) fn select_settings_category(
        &mut self,
        category: SettingsCategory,
        cx: &mut Context<Self>,
    ) {
        self.settings_page.active_category = category;
        cx.notify();
    }

    pub(crate) fn select_color_scheme(
        &mut self,
        scheme: ColorSchemeView,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.pending_settings_save.is_some() || scheme == self.settings.color_scheme {
            return;
        }
        self.settings_page.draft_color_scheme = scheme;
        // Apply chrome immediately so TextFields switch light/dark before save returns.
        self.theme = resolve_theme(scheme, window);
        self.apply_theme_to_text_fields(cx);
        let mut settings = self.settings.clone();
        settings.color_scheme = scheme;
        self.request_settings_save(
            settings,
            ProxyPasswordUpdateView::Unchanged,
            SettingsSaveSource::Theme,
            cx,
        );
    }

    pub(crate) fn select_language(
        &mut self,
        language: LanguagePreferenceView,
        cx: &mut Context<Self>,
    ) {
        if self.pending_settings_save.is_some() || language == self.settings.language {
            return;
        }
        self.settings_page.draft_language = language;
        // Apply immediately so chrome switches language before save returns.
        self.set_language_runtime(language);
        let mut settings = self.settings.clone();
        settings.language = language;
        self.request_settings_save(
            settings,
            ProxyPasswordUpdateView::Unchanged,
            SettingsSaveSource::Language,
            cx,
        );
    }

    pub(crate) fn select_proxy_mode(&mut self, mode: ProxyModeView, cx: &mut Context<Self>) {
        if self.pending_settings_save.is_some() || mode == self.settings_page.draft_proxy_mode {
            return;
        }
        self.settings_page.draft_proxy_mode = mode;
        self.settings_page.error = None;
        cx.notify();
    }

    pub(crate) fn clear_saved_proxy_password(&mut self, cx: &mut Context<Self>) {
        if self.pending_settings_save.is_some() || !self.settings.download_proxy.has_password {
            return;
        }
        let clear = !self.settings_page.clear_proxy_password;
        if clear {
            self.settings_inputs
                .proxy_password
                .update(cx, |input, cx| input.set_text("", cx));
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
            .text()
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
            all_proxy: self.settings_inputs.all_proxy.read(cx).text().trim().into(),
            http_proxy: self
                .settings_inputs
                .http_proxy
                .read(cx)
                .text()
                .trim()
                .into(),
            https_proxy: self
                .settings_inputs
                .https_proxy
                .read(cx)
                .text()
                .trim()
                .into(),
            ftp_proxy: self.settings_inputs.ftp_proxy.read(cx).text().trim().into(),
            no_proxy: self
                .settings_inputs
                .no_proxy
                .read(cx)
                .text()
                .split(',')
                .map(str::trim)
                .filter(|entry| !entry.is_empty())
                .map(ToOwned::to_owned)
                .collect(),
            username: self
                .settings_inputs
                .proxy_username
                .read(cx)
                .text()
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

    /// Save dirty speed-limit and/or transfer-policy fields in a single request.
    /// Used by the Transfers category footer so both groups persist together.
    pub(crate) fn submit_transfers(&mut self, cx: &mut Context<Self>) {
        if self.page != AppPage::Settings || self.pending_settings_save.is_some() {
            return;
        }
        let speed_limit_draft = self.read_speed_limit_draft(cx);
        let transfer_policy_draft = self.read_transfer_policy_draft(cx);
        let sl_dirty = speed_limit_draft != self.settings.speed_limits;
        let tp_dirty = transfer_policy_draft != self.settings.transfer_policy;
        if !sl_dirty && !tp_dirty {
            return;
        }
        if sl_dirty && !speed_limit_draft.is_valid() {
            self.settings_page.error = Some(OperationErrorView {
                code: "settings.invalid_speed_limit".into(),
                summary: self.t("error-settings-invalid-speed-limit"),
                retryable: false,
            });
            cx.notify();
            return;
        }
        if tp_dirty && !transfer_policy_draft.is_valid() {
            self.settings_page.error = Some(OperationErrorView {
                code: "settings.invalid_transfer_policy".into(),
                summary: self.t("error-settings-invalid-transfer-policy"),
                retryable: false,
            });
            cx.notify();
            return;
        }
        let mut settings = self.settings.clone();
        if sl_dirty {
            settings.speed_limits = speed_limit_draft;
        }
        if tp_dirty {
            settings.transfer_policy = transfer_policy_draft;
        }
        let source = match (sl_dirty, tp_dirty) {
            (true, true) => SettingsSaveSource::Transfers,
            (true, false) => SettingsSaveSource::SpeedLimit,
            (false, true) => SettingsSaveSource::TransferPolicy,
            (false, false) => return,
        };
        self.request_settings_save(settings, ProxyPasswordUpdateView::Unchanged, source, cx);
    }

    pub(crate) fn read_speed_limit_draft(&self, cx: &Context<Self>) -> SpeedLimitSettingsView {
        SpeedLimitSettingsView {
            download_limit: self
                .settings_inputs
                .download_limit
                .read(cx)
                .text()
                .trim()
                .into(),
            upload_limit: self
                .settings_inputs
                .upload_limit
                .read(cx)
                .text()
                .trim()
                .into(),
        }
    }

    pub(crate) fn read_transfer_policy_draft(
        &self,
        cx: &Context<Self>,
    ) -> TransferPolicySettingsView {
        TransferPolicySettingsView {
            max_concurrent_downloads: self
                .settings_inputs
                .max_concurrent
                .read(cx)
                .text()
                .trim()
                .into(),
            max_connection_per_server: self
                .settings_inputs
                .max_connection
                .read(cx)
                .text()
                .trim()
                .into(),
            split: self.settings_inputs.split.read(cx).text().trim().into(),
            min_split_size: self
                .settings_inputs
                .min_split_size
                .read(cx)
                .text()
                .trim()
                .into(),
            file_allocation: self.settings_page.draft_file_allocation,
            check_integrity: self.settings_page.draft_check_integrity,
        }
    }

    /// Reflect the normalized (compact) speed-limit form back into the fields
    /// so a saved "2097152" re-renders as "2M".
    pub(crate) fn sync_speed_limit_fields_from_settings(&mut self, cx: &mut Context<Self>) {
        let speed_limits = self.settings.speed_limits.clone();
        self.settings_inputs.download_limit.update(cx, |input, cx| {
            input.set_text(speed_limits.download_limit.clone(), cx);
        });
        self.settings_inputs.upload_limit.update(cx, |input, cx| {
            input.set_text(speed_limits.upload_limit.clone(), cx);
        });
    }

    pub(crate) fn sync_transfer_policy_fields_from_settings(&mut self, cx: &mut Context<Self>) {
        let policy = self.settings.transfer_policy.clone();
        self.settings_inputs.max_concurrent.update(cx, |input, cx| {
            input.set_text(policy.max_concurrent_downloads.clone(), cx);
        });
        self.settings_inputs.max_connection.update(cx, |input, cx| {
            input.set_text(policy.max_connection_per_server.clone(), cx);
        });
        self.settings_inputs.split.update(cx, |input, cx| {
            input.set_text(policy.split.clone(), cx);
        });
        self.settings_inputs.min_split_size.update(cx, |input, cx| {
            input.set_text(policy.min_split_size.clone(), cx);
        });
    }

    pub(crate) fn select_file_allocation(
        &mut self,
        method: FileAllocationView,
        cx: &mut Context<Self>,
    ) {
        if self.page != AppPage::Settings || self.pending_settings_save.is_some() {
            return;
        }
        if self.settings_page.draft_file_allocation == method {
            return;
        }
        self.settings_page.draft_file_allocation = method;
        self.settings_page.error = None;
        cx.notify();
    }

    pub(crate) fn toggle_check_integrity(&mut self, cx: &mut Context<Self>) {
        if self.page != AppPage::Settings || self.pending_settings_save.is_some() {
            return;
        }
        self.settings_page.draft_check_integrity = !self.settings_page.draft_check_integrity;
        self.settings_page.error = None;
        cx.notify();
    }

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

    pub(crate) fn submit_platform(&mut self, cx: &mut Context<Self>) {
        if self.page != AppPage::Settings || self.pending_settings_save.is_some() {
            return;
        }
        let draft = PlatformSettingsView {
            close_behavior: self.settings_page.draft_close_behavior,
            show_tray_icon: self.settings_page.draft_show_tray_icon,
            start_minimized_to_tray: self.settings_page.draft_start_minimized_to_tray,
        };
        if draft == self.settings.platform {
            return;
        }
        let mut settings = self.settings.clone();
        settings.platform = draft;
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

    /// Intercept the window close control: hide to tray when configured, else quit.
    // --- request_settings_save..request_settings_save ---
    pub(crate) fn request_settings_save(
        &mut self,
        settings: SettingsView,
        proxy_password: ProxyPasswordUpdateView,
        source: SettingsSaveSource,
        cx: &mut Context<Self>,
    ) {
        if self.pending_settings_save.is_some() {
            return;
        }
        let request_id = self.allocate_request_id();
        self.pending_settings_save = Some(PendingSettingsSave {
            request_id,
            settings: settings.clone(),
            source,
        });
        self.settings_page.error = None;
        cx.emit(AppShellEvent::SettingsSaveRequested(
            SettingsSaveRequestView {
                request_id,
                settings,
                proxy_password,
            },
        ));
        cx.notify();
    }

    pub(crate) fn export_diagnostics(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let initial_directory = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        let selected = cx.prompt_for_new_path(&initial_directory, Some("ariadeck-diagnostics.zip"));
        cx.spawn_in(window, async move |this, cx| {
            let selected = selected.await;
            let _ = this.update_in(cx, |this, _window, cx| match selected {
                Ok(Ok(Some(path))) => {
                    cx.emit(AppShellEvent::DiagnosticExportRequested(
                        DiagnosticExportRequestView {
                            path: path.to_string_lossy().into_owned(),
                        },
                    ));
                }
                Ok(Ok(None)) => {}
                Ok(Err(_)) => {
                    let error = OperationErrorView {
                        code: "settings.path_picker_failed".into(),
                        summary: this.t("error-settings-path-picker-failed"),
                        retryable: true,
                    };
                    let message = this.te(&error);
                    this.show_notice(message, true, cx);
                }
                Err(_) => {
                    let error = OperationErrorView {
                        code: "settings.path_picker_closed".into(),
                        summary: this.t("error-settings-path-picker-closed"),
                        retryable: true,
                    };
                    let message = this.te(&error);
                    this.show_notice(message, true, cx);
                }
            });
        })
        .detach();
    }

    pub(crate) fn export_settings_file(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.pending_settings_save.is_some() {
            return;
        }
        let initial_directory = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        let selected = cx.prompt_for_new_path(&initial_directory, Some("ariadeck-settings.json"));
        cx.spawn_in(window, async move |this, cx| {
            let selected = selected.await;
            let _ = this.update_in(cx, |this, _window, cx| match selected {
                Ok(Ok(Some(path))) => cx.emit(AppShellEvent::SettingsExportRequested(
                    SettingsExportRequestView {
                        path: path.to_string_lossy().into_owned(),
                    },
                )),
                Ok(Ok(None)) => {}
                Ok(Err(_)) => {
                    let error = OperationErrorView {
                        code: "settings.path_picker_failed".into(),
                        summary: this.t("error-settings-path-picker-failed"),
                        retryable: true,
                    };
                    let message = this.te(&error);
                    this.show_notice(message, true, cx);
                }
                Err(_) => {
                    let error = OperationErrorView {
                        code: "settings.path_picker_closed".into(),
                        summary: this.t("error-settings-path-picker-closed"),
                        retryable: true,
                    };
                    let message = this.te(&error);
                    this.show_notice(message, true, cx);
                }
            });
        })
        .detach();
    }

    pub(crate) fn import_settings_file(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.pending_settings_save.is_some() {
            return;
        }
        let selected = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some(self.t("settings-import-picker-prompt").into()),
        });
        cx.spawn_in(window, async move |this, cx| {
            let selected = selected.await;
            let _ = this.update_in(cx, |this, _window, cx| match selected {
                Ok(Ok(Some(paths))) => {
                    if let Some(path) = paths.into_iter().next() {
                        cx.emit(AppShellEvent::SettingsImportRequested(
                            SettingsImportRequestView {
                                path: path.to_string_lossy().into_owned(),
                            },
                        ));
                    }
                }
                Ok(Ok(None)) => {}
                Ok(Err(_)) => {
                    let error = OperationErrorView {
                        code: "settings.path_picker_failed".into(),
                        summary: this.t("error-settings-path-picker-failed"),
                        retryable: true,
                    };
                    let message = this.te(&error);
                    this.show_notice(message, true, cx);
                }
                Err(_) => {
                    let error = OperationErrorView {
                        code: "settings.path_picker_closed".into(),
                        summary: this.t("error-settings-path-picker-closed"),
                        retryable: true,
                    };
                    let message = this.te(&error);
                    this.show_notice(message, true, cx);
                }
            });
        })
        .detach();
    }

    // --- pick_path_for_field..apply_picked_path ---
    pub(crate) fn pick_path_for_field(
        &mut self,
        target: PathPickTarget,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (files, directories, prompt) = match target {
            PathPickTarget::DownloadDirectory
            | PathPickTarget::ProfileDownloadDirectory
            | PathPickTarget::CategoryDirectory { .. } => {
                (false, true, self.t("settings-pick-download-directory"))
            }
            PathPickTarget::CoreExecutable | PathPickTarget::ProfileExecutable => (
                true,
                false,
                if cfg!(windows) {
                    self.t("settings-pick-aria2c-windows")
                } else {
                    self.t("settings-pick-aria2c")
                },
            ),
        };
        let selected = cx.prompt_for_paths(PathPromptOptions {
            files,
            directories,
            multiple: false,
            prompt: Some(prompt.into()),
        });
        cx.spawn_in(window, async move |this, cx| {
            let selected = selected.await;
            let _ = this.update_in(cx, |this, window, cx| match selected {
                Ok(Ok(Some(paths))) => {
                    if let Some(path) = paths.into_iter().next() {
                        this.apply_picked_path(target, path, window, cx);
                    }
                }
                Ok(Ok(None)) => {}
                Ok(Err(_)) => {
                    this.settings_page.error = Some(OperationErrorView {
                        code: "settings.path_picker_failed".into(),
                        summary: this.t("error-settings-path-picker-failed"),
                        retryable: true,
                    });
                    cx.notify();
                }
                Err(_) => {
                    this.settings_page.error = Some(OperationErrorView {
                        code: "settings.path_picker_closed".into(),
                        summary: this.t("error-settings-path-picker-closed"),
                        retryable: true,
                    });
                    cx.notify();
                }
            });
        })
        .detach();
    }

    pub(crate) fn apply_picked_path(
        &mut self,
        target: PathPickTarget,
        path: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let display = path.to_string_lossy().into_owned();
        if let PathPickTarget::CategoryDirectory { index } = target {
            if let Some(category) = self.settings_page.draft_categories.get_mut(index) {
                category.directory = display;
            }
            cx.notify();
            return;
        }
        let field = match target {
            PathPickTarget::DownloadDirectory => self.settings_inputs.directory.clone(),
            PathPickTarget::CoreExecutable => self.settings_inputs.core_path.clone(),
            PathPickTarget::ProfileExecutable => self.settings_inputs.profile_executable.clone(),
            PathPickTarget::ProfileDownloadDirectory => {
                self.settings_inputs.profile_download.clone()
            }
            PathPickTarget::CategoryDirectory { .. } => unreachable!(),
        };
        field.update(cx, |input, cx| input.set_text(display, cx));
        window.focus(&field.focus_handle(cx), cx);
        // Clear stale settings form error once the user picks a path.
        if self.page == AppPage::Settings {
            self.settings_page.error = None;
        }
        cx.notify();
    }

    // --- render_settings_page..render_settings_system ---
    pub(crate) fn render_settings_page(&mut self, cx: &mut Context<Self>) -> Stateful<Div> {
        let colors = self.theme.colors;
        let active_category = self.settings_page.active_category;
        div()
            .id("settings-page")
            .key_context("SettingsPage")
            .role(Role::Main)
            .aria_label(self.t("settings-title"))
            .size_full()
            .flex()
            .flex_col()
            .bg(colors.background)
            .child(
                div()
                    .h(px(48.0))
                    .flex_none()
                    .flex()
                    .items_center()
                    .px_6()
                    .border_b_1()
                    .border_color(colors.border)
                    .bg(colors.toolbar_surface)
                    .child(
                        div()
                            .text_base()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(self.t("ui-settings-title")),
                    ),
            )
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .child(self.render_settings_nav(cx))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .child(
                                div()
                                    .id("settings-scroll-shell")
                                    .flex_1()
                                    .min_h_0()
                                    .flex()
                                    .child(
                                        div()
                                            .id("settings-scroll")
                                            .flex_1()
                                            .min_h_0()
                                            .px_6()
                                            .py_6()
                                            .overflow_y_scroll()
                                            .track_scroll(&self.settings_scroll)
                                            .child(
                                                div()
                                                    .w_full()
                                                    .max_w(px(760.0))
                                                    .flex()
                                                    .flex_col()
                                                    .gap_6()
                                                    .child(match active_category {
                                                        SettingsCategory::General => self
                                                            .render_settings_general(cx)
                                                            .into_any_element(),
                                                        SettingsCategory::Profiles => self
                                                            .render_settings_profiles(cx)
                                                            .into_any_element(),
                                                        SettingsCategory::Engine => self
                                                            .render_settings_engine(cx)
                                                            .into_any_element(),
                                                        SettingsCategory::Network => self
                                                            .render_settings_network(cx)
                                                            .into_any_element(),
                                                        SettingsCategory::Transfers => self
                                                            .render_settings_transfers(cx)
                                                            .into_any_element(),
                                                        SettingsCategory::Notifications => self
                                                            .render_settings_notifications(cx)
                                                            .into_any_element(),
                                                        SettingsCategory::System => self
                                                            .render_settings_system(cx)
                                                            .into_any_element(),
                                                        SettingsCategory::About => self
                                                            .render_settings_about(cx)
                                                            .into_any_element(),
                                                    }),
                                            ),
                                    )
                                    .child(render_vertical_scrollbar(
                                        &self.settings_scroll,
                                        colors,
                                    )),
                            )
                            .child(self.render_settings_footer(cx)),
                    ),
            )
    }

    pub(crate) fn render_settings_nav(&mut self, cx: &mut Context<Self>) -> Stateful<Div> {
        let colors = self.theme.colors;
        let active = self.settings_page.active_category;
        let mut nav = div()
            .id("settings-category-list")
            .w(px(168.0))
            .flex_none()
            .flex()
            .flex_col()
            .border_r_1()
            .border_color(colors.border)
            .bg(colors.background)
            .py_4()
            .role(Role::TabList)
            .aria_label(self.t("settings-nav-aria"));
        for category in SettingsCategory::ALL {
            let is_active = category == active;
            let item = div()
                .id(SharedString::from(format!("nav-{}", category.label())))
                .role(Role::Tab)
                .aria_label(self.t(category.message_key()))
                .aria_selected(is_active)
                .focusable()
                .tab_stop(true)
                .focus_visible(|style| style.border_1().border_color(colors.focus_ring))
                .flex()
                .items_center()
                .gap_3()
                .px_4()
                .py_2()
                .text_sm()
                .cursor_pointer()
                .text_color(if is_active {
                    colors.text_primary
                } else {
                    colors.text_secondary
                })
                .when(is_active, |el| {
                    el.bg(with_alpha(colors.accent, 0.08))
                        .border_l_2()
                        .border_color(colors.accent)
                })
                .child(
                    Icon::new(category.icon())
                        .size(IconSize::Small)
                        .color(if is_active {
                            colors.accent
                        } else {
                            colors.text_secondary
                        }),
                )
                .child(div().child(self.t(category.message_key())))
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.select_settings_category(category, cx);
                }));
            nav = nav.child(item);
        }
        nav
    }

    pub(crate) fn render_settings_footer(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let colors = self.theme.colors;
        let pending = self.pending_settings_save.is_some();
        let active_category = self.settings_page.active_category;
        let error = self.settings_page.error.clone();

        if matches!(
            active_category,
            SettingsCategory::Profiles | SettingsCategory::Engine | SettingsCategory::About
        ) {
            return div().into_any_element();
        }

        let (dirty, saving) = match active_category {
            SettingsCategory::General => {
                let dir_dirty = self.settings_inputs.directory.read(cx).text().trim()
                    != self.settings.download_directory;
                let categories_dirty = self.settings_page.draft_categories
                    != self.settings.categories
                    || self.settings_page.draft_default_category_id
                        != self.settings.default_category_id;
                let dirty = dir_dirty || categories_dirty;
                let saving = self
                    .pending_settings_save
                    .as_ref()
                    .is_some_and(|p| p.source == SettingsSaveSource::Directory);
                (dirty, saving)
            }
            SettingsCategory::Network => {
                let password_changed = !self
                    .settings_inputs
                    .proxy_password
                    .read(cx)
                    .text()
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
                    all_proxy: self.settings_inputs.all_proxy.read(cx).text().trim().into(),
                    http_proxy: self
                        .settings_inputs
                        .http_proxy
                        .read(cx)
                        .text()
                        .trim()
                        .into(),
                    https_proxy: self
                        .settings_inputs
                        .https_proxy
                        .read(cx)
                        .text()
                        .trim()
                        .into(),
                    ftp_proxy: self.settings_inputs.ftp_proxy.read(cx).text().trim().into(),
                    no_proxy: self
                        .settings_inputs
                        .no_proxy
                        .read(cx)
                        .text()
                        .split(',')
                        .map(str::trim)
                        .filter(|e| !e.is_empty())
                        .map(ToOwned::to_owned)
                        .collect(),
                    username: self
                        .settings_inputs
                        .proxy_username
                        .read(cx)
                        .text()
                        .trim()
                        .into(),
                    has_password: proxy_has_password,
                    check_certificate: self.settings_page.draft_check_certificate,
                };
                let dirty = proxy_draft != self.settings.download_proxy
                    || password_changed
                    || password_cleared;
                let saving = self
                    .pending_settings_save
                    .as_ref()
                    .is_some_and(|p| p.source == SettingsSaveSource::Proxy);
                (dirty, saving)
            }
            SettingsCategory::Transfers => {
                let speed_limit_draft = self.read_speed_limit_draft(cx);
                let transfer_policy_draft = self.read_transfer_policy_draft(cx);
                // Dirty is independent of validity so invalid edits still show the
                // footer; submit_transfers reports validation errors on click.
                let dirty = speed_limit_draft != self.settings.speed_limits
                    || transfer_policy_draft != self.settings.transfer_policy;
                let saving = self.pending_settings_save.as_ref().is_some_and(|p| {
                    matches!(
                        p.source,
                        SettingsSaveSource::SpeedLimit
                            | SettingsSaveSource::TransferPolicy
                            | SettingsSaveSource::Transfers
                    )
                });
                (dirty, saving)
            }
            SettingsCategory::Notifications => {
                let notifications_draft = NotificationSettingsView {
                    volume: self.settings_page.draft_notification_volume,
                    notify_on_completion: self.settings_page.draft_notify_on_completion,
                    notify_on_error: self.settings_page.draft_notify_on_error,
                    notify_on_engine_events: self.settings_page.draft_notify_on_engine_events,
                    os_notifications: self.settings_page.draft_os_notifications,
                    notify_on_low_disk: self.settings_page.draft_notify_on_low_disk,
                    low_disk_threshold_bytes: self.settings.notifications.low_disk_threshold_bytes,
                };
                let dirty = notifications_draft != self.settings.notifications;
                let saving = self
                    .pending_settings_save
                    .as_ref()
                    .is_some_and(|p| p.source == SettingsSaveSource::Notifications);
                (dirty, saving)
            }
            SettingsCategory::System => {
                let platform_draft = PlatformSettingsView {
                    close_behavior: self.settings_page.draft_close_behavior,
                    show_tray_icon: self.settings_page.draft_show_tray_icon,
                    start_minimized_to_tray: self.settings_page.draft_start_minimized_to_tray,
                };
                let dirty = platform_draft != self.settings.platform;
                let saving = self
                    .pending_settings_save
                    .as_ref()
                    .is_some_and(|p| p.source == SettingsSaveSource::Platform);
                (dirty, saving)
            }
            SettingsCategory::Profiles | SettingsCategory::Engine | SettingsCategory::About => {
                unreachable!()
            }
        };

        // Keep the footer out of the way until there is something to save or report.
        if !dirty && !saving && error.is_none() {
            return div().into_any_element();
        }

        div()
            .flex_none()
            .border_t_1()
            .border_color(colors.border)
            .bg(colors.toolbar_surface)
            .h(px(48.0))
            .flex()
            .items_center()
            .px_6()
            .gap_3()
            .child(
                Button::new("footer-save", self.t("button-save"))
                    .aria_label(if saving {
                        self.t("settings-save-saving")
                    } else {
                        self.t("settings-save")
                    })
                    .style(ButtonStyle::Primary)
                    .disabled(pending || !dirty)
                    .loading(saving)
                    .on_click(cx.listener(move |this, _, _, cx| match active_category {
                        SettingsCategory::General => this.submit_settings(cx),
                        SettingsCategory::Network => this.submit_proxy_settings(cx),
                        SettingsCategory::Transfers => this.submit_transfers(cx),
                        SettingsCategory::Notifications => this.submit_notifications(cx),
                        SettingsCategory::System => this.submit_platform(cx),
                        _ => {}
                    }))
                    .render(colors),
            )
            .when_some(error, |footer, error| {
                let message = self.te(&error);
                footer.child(
                    div()
                        .id("settings-error")
                        .role(Role::Alert)
                        .aria_label(message.clone())
                        .flex()
                        .items_center()
                        .gap_1()
                        .text_xs()
                        .text_color(colors.danger)
                        .child(
                            Icon::new(IconName::CircleAlert)
                                .size(IconSize::XSmall)
                                .color(colors.danger),
                        )
                        .child(message),
                )
            })
            .into_any_element()
    }

    pub(crate) fn add_download_category(&mut self, cx: &mut Context<Self>) {
        let n = self.settings_page.draft_categories.len() + 1;
        let name = format!("Category {n}");
        self.settings_page
            .draft_categories
            .push(crate::DownloadCategoryView {
                id: format!(
                    "{:x}",
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.as_nanos())
                        .unwrap_or(0)
                ),
                name,
                directory: self.settings.download_directory.clone(),
            });
        cx.notify();
    }

    pub(crate) fn remove_download_category(&mut self, index: usize, cx: &mut Context<Self>) {
        if index >= self.settings_page.draft_categories.len() {
            return;
        }
        let removed = self.settings_page.draft_categories.remove(index);
        if self.settings_page.draft_default_category_id.as_deref() == Some(removed.id.as_str()) {
            self.settings_page.draft_default_category_id = None;
        }
        if self.add_dialog.category_id.as_deref() == Some(removed.id.as_str()) {
            self.add_dialog.category_id = self.settings_page.draft_default_category_id.clone();
        }
        cx.notify();
    }

    pub(crate) fn set_default_download_category(
        &mut self,
        category_id: Option<String>,
        cx: &mut Context<Self>,
    ) {
        self.settings_page.draft_default_category_id = category_id;
        cx.notify();
    }

    pub(crate) fn render_settings_general(&mut self, cx: &mut Context<Self>) -> Div {
        let colors = self.theme.colors;
        let pending = self.pending_settings_save.is_some();
        let draft_scheme = self.settings_page.draft_color_scheme;
        let selected_scheme = ColorSchemeView::ALL
            .iter()
            .position(|scheme| *scheme == draft_scheme)
            .unwrap_or(0);
        let shell = cx.entity().downgrade();
        let scheme_control = SegmentedControl::new(
            "settings-theme",
            [
                Segment::new(self.t("settings-theme-system")).icon(IconName::Settings),
                Segment::new(self.t("settings-theme-light")).icon(IconName::Sun),
                Segment::new(self.t("settings-theme-dark")).icon(IconName::Moon),
            ],
            selected_scheme,
            self.theme,
        )
        .aria_label(self.t("settings-theme-aria"))
        .disabled(pending)
        .on_select(move |index, window, cx| {
            let scheme = ColorSchemeView::ALL
                .get(index)
                .copied()
                .unwrap_or(ColorSchemeView::System);
            shell
                .update(cx, |shell, cx| {
                    shell.select_color_scheme(scheme, window, cx)
                })
                .ok();
        });
        let draft_language = self.settings_page.draft_language;
        let selected_language = LanguagePreferenceView::ALL
            .iter()
            .position(|language| *language == draft_language)
            .unwrap_or(0);
        let language_shell = cx.entity().downgrade();
        let language_control = SegmentedControl::new(
            "settings-language",
            LanguagePreferenceView::ALL
                .map(|language| Segment::new(self.t(language.message_key()))),
            selected_language,
            self.theme,
        )
        .aria_label(self.t("settings-language-aria"))
        .disabled(pending)
        .on_select(move |index, _window, cx| {
            let language = LanguagePreferenceView::ALL
                .get(index)
                .copied()
                .unwrap_or_default();
            language_shell
                .update(cx, |shell, cx| shell.select_language(language, cx))
                .ok();
        });
        let appearance_title = self.t("settings-appearance");
        let theme_label = self.t("settings-theme");
        let language_label = self.t("settings-language");
        let language_desc = self.t("settings-language-description");
        let downloads_title = self.t("settings-downloads");
        let default_dir_label = self.t("settings-default-directory");
        let default_dir_desc = self.t("settings-default-directory-desc");
        let browse_label = self.t("button-browse");
        let browse_aria = self.t("settings-default-directory-browse-aria");
        div()
            .flex()
            .flex_col()
            .gap_6()
            .child(
                settings_section_owned(appearance_title, colors)
                    .child(settings_section_row_owned(
                        theme_label,
                        None::<SharedString>,
                        scheme_control,
                        colors,
                    ))
                    .child(settings_section_row_owned(
                        language_label,
                        Some(language_desc),
                        language_control,
                        colors,
                    )),
            )
            .child(settings_section_owned(downloads_title, colors).child(
                settings_section_row_owned(
                    default_dir_label,
                    Some(default_dir_desc),
                    settings_path_field_row(
                        self.settings_inputs.directory.clone(),
                        "browse-download-directory",
                        browse_label,
                        browse_aria,
                        PathPickTarget::DownloadDirectory,
                        colors,
                        cx,
                    ),
                    colors,
                ),
            ))
            .child(self.render_settings_categories(cx))
    }

    pub(crate) fn render_settings_categories(&mut self, cx: &mut Context<Self>) -> Div {
        let colors = self.theme.colors;
        let pending = self.pending_settings_save.is_some();
        let categories = self.settings_page.draft_categories.clone();
        let default_id = self.settings_page.draft_default_category_id.clone();
        let title = self.t("settings-categories");
        let desc = self.t("settings-categories-desc");
        let add_label = self.t("settings-category-add");
        let browse_label = self.t("button-browse");
        let default_label = self.t("settings-category-default");
        let is_default_label = self.t("settings-category-is-default");
        let remove_label = self.t("settings-category-remove");
        let empty_label = self.t("settings-categories-empty");
        let no_dir = self.t("settings-category-no-directory");

        let mut list = div().flex().flex_col().gap_2();
        if categories.is_empty() {
            list = list.child(
                div()
                    .text_xs()
                    .text_color(colors.text_muted)
                    .child(empty_label),
            );
        }
        for (index, category) in categories.iter().enumerate() {
            let is_default = default_id.as_deref() == Some(category.id.as_str());
            let cat_id = category.id.clone();
            let name = category.name.clone();
            let directory = category.directory.clone();
            list = list.child(
                div()
                    .id(SharedString::from(format!("category-row-{index}")))
                    .flex()
                    .flex_col()
                    .gap_1()
                    .px_3()
                    .py_2()
                    .rounded_md()
                    .border_1()
                    .border_color(if is_default {
                        colors.accent
                    } else {
                        colors.border
                    })
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .text_sm()
                                    .font_weight(gpui::FontWeight::MEDIUM)
                                    .text_color(colors.text_primary)
                                    .child(name),
                            )
                            .child(
                                Button::new(
                                    SharedString::from(format!("category-default-{index}")),
                                    if is_default {
                                        is_default_label.clone()
                                    } else {
                                        default_label.clone()
                                    },
                                )
                                .style(ButtonStyle::Secondary)
                                .disabled(pending || is_default)
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.set_default_download_category(Some(cat_id.clone()), cx);
                                }))
                                .render(colors),
                            )
                            .child(
                                Button::new(
                                    SharedString::from(format!("category-remove-{index}")),
                                    remove_label.clone(),
                                )
                                .style(ButtonStyle::Secondary)
                                .disabled(pending)
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.remove_download_category(index, cx);
                                }))
                                .render(colors),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .text_xs()
                                    .text_color(colors.text_muted)
                                    .child(if directory.is_empty() {
                                        no_dir.clone()
                                    } else {
                                        directory
                                    }),
                            )
                            .child(
                                Button::new(
                                    SharedString::from(format!("category-browse-{index}")),
                                    browse_label.clone(),
                                )
                                .style(ButtonStyle::Secondary)
                                .disabled(pending)
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    this.pick_path_for_field(
                                        PathPickTarget::CategoryDirectory { index },
                                        window,
                                        cx,
                                    );
                                }))
                                .render(colors),
                            ),
                    ),
            );
        }

        settings_section_owned(title, colors)
            .child(div().text_xs().text_color(colors.text_muted).child(desc))
            .child(list)
            .child(
                div().mt_2().child(
                    Button::new("add-download-category", add_label)
                        .style(ButtonStyle::Secondary)
                        .disabled(pending || self.settings_page.draft_categories.len() >= 32)
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.add_download_category(cx);
                        }))
                        .render(colors),
                ),
            )
    }

    pub(crate) fn render_settings_profiles(&mut self, cx: &mut Context<Self>) -> Div {
        let colors = self.theme.colors;
        let profiles = self.profiles.clone();
        let active_id = profiles.active_profile_id.clone();
        let profiles_count = profiles.profiles.len();
        settings_section_owned(self.t("settings-profiles"), colors)
            .child(div().mt_3().flex().flex_col().gap_2().children(
                profiles.profiles.into_iter().map(|profile| {
                    let is_active = profile.profile_id == active_id;
                    let profile_id = profile.profile_id.clone();
                    let switch_id = profile_id.clone();
                    let edit_id = profile_id.clone();
                    let remove_id = profile_id.clone();
                    let can_remove = profiles_count > 1;
                    let summary = match profile.kind {
                        ProfileKindView::LocalManaged => {
                            if profile.executable.is_empty() {
                                self.t("settings-profile-summary-local-managed")
                            } else {
                                self.t_args(
                                    "settings-profile-summary-local-pinned",
                                    &[("path", FluentValue::from(profile.executable.as_str()))],
                                )
                            }
                        }
                        ProfileKindView::RemoteRpc => {
                            let endpoint = if profile.endpoint.is_empty() {
                                self.t("settings-profile-no-endpoint")
                            } else {
                                profile.endpoint.clone()
                            };
                            if profile.has_secret {
                                self.t_args(
                                    "settings-profile-summary-remote-secret",
                                    &[("endpoint", FluentValue::from(endpoint.as_str()))],
                                )
                            } else {
                                self.t_args(
                                    "settings-profile-summary-remote",
                                    &[("endpoint", FluentValue::from(endpoint.as_str()))],
                                )
                            }
                        }
                    };
                    div()
                        .id(SharedString::from(format!(
                            "profile-row-{}",
                            profile.profile_id
                        )))
                        .flex()
                        .items_center()
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
                                        .child(profile.name.clone()),
                                )
                                .child(
                                    div().text_xs().text_color(colors.text_muted).child(summary),
                                ),
                        )
                        .child(
                            Button::new(
                                SharedString::from(format!("activate-profile-{}", profile_id)),
                                if is_active {
                                    self.t("settings-item-active")
                                } else {
                                    self.t("settings-item-activate")
                                },
                            )
                            .aria_label(if is_active {
                                self.t_args(
                                    "settings-profile-active-aria",
                                    &[("name", FluentValue::from(profile.name.as_str()))],
                                )
                            } else {
                                self.t_args(
                                    "settings-profile-activate-aria",
                                    &[("name", FluentValue::from(profile.name.as_str()))],
                                )
                            })
                            .style(if is_active {
                                ButtonStyle::Secondary
                            } else {
                                ButtonStyle::Primary
                            })
                            .disabled(is_active)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.request_switch_profile(switch_id.clone(), cx);
                            }))
                            .render(colors),
                        )
                        .child(
                            Button::new(
                                SharedString::from(format!("edit-profile-{}", edit_id)),
                                self.t("settings-item-edit"),
                            )
                            .aria_label(self.t_args(
                                "settings-profile-edit-aria",
                                &[("name", FluentValue::from(profile.name.as_str()))],
                            ))
                            .style(ButtonStyle::Secondary)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.open_profile_editor(edit_id.clone(), cx);
                            }))
                            .render(colors),
                        )
                        .child(
                            Button::new(
                                SharedString::from(format!("remove-profile-{}", remove_id)),
                                self.t("button-delete"),
                            )
                            .aria_label(self.t_args(
                                "settings-profile-delete-aria",
                                &[("name", FluentValue::from(profile.name.as_str()))],
                            ))
                            .style(ButtonStyle::Secondary)
                            .disabled(!can_remove)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.request_remove_profile(remove_id.clone(), cx);
                            }))
                            .render(colors),
                        )
                }),
            ))
            .when_some(
                self.settings_page.editing_profile_id.clone(),
                |section, editing_id| {
                    let kind = self.settings_page.draft_profile_kind;
                    let is_local = kind == ProfileKindView::LocalManaged;
                    let kind_shell = cx.entity().downgrade();
                    let kind_control = SegmentedControl::new(
                        "settings-profile-kind",
                        [
                            Segment::new(self.t("settings-profile-kind-local")),
                            Segment::new(self.t("settings-profile-kind-remote")),
                        ],
                        usize::from(!is_local),
                        self.theme,
                    )
                    .aria_label(self.t("settings-profile-kind-aria"))
                    .on_select(move |index, _window, cx| {
                        let kind = if index == 0 {
                            ProfileKindView::LocalManaged
                        } else {
                            ProfileKindView::RemoteRpc
                        };
                        kind_shell
                            .update(cx, |shell, cx| {
                                shell.select_profile_editor_kind(kind, cx);
                            })
                            .ok();
                    });
                    section.child(
                        div()
                            .mt_3()
                            .flex()
                            .flex_col()
                            .gap_3()
                            .px_3()
                            .py_3()
                            .rounded_md()
                            .border_1()
                            .border_color(colors.border)
                            .bg(colors.surface)
                            .child(
                                div()
                                    .text_sm()
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(colors.text_primary)
                                    .child(self.t_args(
                                        "settings-profile-editor-title",
                                        &[("id", FluentValue::from(editing_id.as_str()))],
                                    )),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(colors.text_muted)
                                    .child(self.t("settings-profile-editor-hint")),
                            )
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .gap_1()
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(colors.text_muted)
                                            .child(self.t("ui-profile-name")),
                                    )
                                    .child(self.settings_inputs.profile_name.clone()),
                            )
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .gap_1()
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(colors.text_muted)
                                            .child(self.t("ui-profile-kind")),
                                    )
                                    .child(kind_control),
                            )
                            .child(if is_local {
                                div()
                                    .flex()
                                    .flex_col()
                                    .gap_1()
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(colors.text_muted)
                                            .child(self.t("settings-profile-executable-label")),
                                    )
                                    .child(settings_path_field_row(
                                        self.settings_inputs.profile_executable.clone(),
                                        "browse-profile-executable",
                                        self.t("button-browse"),
                                        self.t("settings-profile-executable-browse-aria"),
                                        PathPickTarget::ProfileExecutable,
                                        colors,
                                        cx,
                                    ))
                                    .into_any_element()
                            } else {
                                let has_secret = self
                                    .profiles
                                    .profiles
                                    .iter()
                                    .find(|profile| profile.profile_id == editing_id)
                                    .is_some_and(|profile| profile.has_secret);
                                let secret_cleared = self.settings_page.clear_profile_rpc_secret;
                                div()
                                    .flex()
                                    .flex_col()
                                    .gap_2()
                                    .child(
                                        div()
                                            .flex()
                                            .flex_col()
                                            .gap_1()
                                            .child(
                                                div()
                                                    .text_xs()
                                                    .text_color(colors.text_muted)
                                                    .child(self.t("ui-profile-endpoint")),
                                            )
                                            .child(self.settings_inputs.profile_endpoint.clone()),
                                    )
                                    .child(
                                        div()
                                            .flex()
                                            .flex_col()
                                            .gap_1()
                                            .child(
                                                div()
                                                    .text_xs()
                                                    .text_color(colors.text_muted)
                                                    .child(if secret_cleared {
                                                        self.t("settings-profile-secret-clear")
                                                    } else if has_secret {
                                                        self.t("settings-profile-secret-saved")
                                                    } else {
                                                        self.t("settings-profile-secret-optional")
                                                    }),
                                            )
                                            .child(self.settings_inputs.profile_secret.clone()),
                                    )
                                    .child(
                                        Button::new(
                                            "toggle-clear-profile-secret",
                                            if secret_cleared {
                                                self.t("settings-profile-secret-keep")
                                            } else if has_secret {
                                                self.t("settings-profile-secret-remove")
                                            } else {
                                                self.t("settings-profile-secret-none")
                                            },
                                        )
                                        .aria_label(self.t("settings-profile-secret-toggle-aria"))
                                        .style(ButtonStyle::Secondary)
                                        .disabled(!has_secret && !secret_cleared)
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.toggle_clear_profile_rpc_secret(cx);
                                        }))
                                        .render(colors),
                                    )
                                    .into_any_element()
                            })
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .gap_1()
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(colors.text_muted)
                                            .child(self.t("ui-profile-download-dir")),
                                    )
                                    .child(settings_path_field_row(
                                        self.settings_inputs.profile_download.clone(),
                                        "browse-profile-download",
                                        self.t("button-browse"),
                                        self.t("settings-profile-download-browse-aria"),
                                        PathPickTarget::ProfileDownloadDirectory,
                                        colors,
                                        cx,
                                    )),
                            )
                            .child(
                                div()
                                    .flex()
                                    .flex_wrap()
                                    .gap_2()
                                    .child(
                                        Button::new(
                                            "apply-profile-editor",
                                            self.t("settings-profile-apply"),
                                        )
                                        .aria_label(self.t("settings-profile-apply-aria"))
                                        .style(ButtonStyle::Primary)
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.apply_profile_editor(cx);
                                        }))
                                        .render(colors),
                                    )
                                    .child(
                                        Button::new(
                                            "cancel-profile-editor",
                                            self.t("button-cancel"),
                                        )
                                        .aria_label(self.t("settings-profile-cancel-aria"))
                                        .style(ButtonStyle::Secondary)
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.close_profile_editor(cx);
                                        }))
                                        .render(colors),
                                    ),
                            ),
                    )
                },
            )
            .child(
                div()
                    .mt_3()
                    .flex()
                    .flex_wrap()
                    .gap_2()
                    .child(
                        Button::new("add-local-profile", self.t("settings-profile-add-local"))
                            .aria_label(self.t("settings-profile-add-local-aria"))
                            .style(ButtonStyle::Secondary)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.add_draft_local_profile(cx);
                            }))
                            .render(colors),
                    )
                    .child(
                        Button::new("add-remote-profile", self.t("settings-profile-add-remote"))
                            .aria_label(self.t("settings-profile-add-remote-aria"))
                            .style(ButtonStyle::Secondary)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.add_draft_remote_profile(cx);
                            }))
                            .render(colors),
                    )
                    .child(
                        Button::new("save-profile-catalog", self.t("settings-profile-save"))
                            .aria_label(self.t("settings-profile-save-aria"))
                            .style(ButtonStyle::Primary)
                            .on_click(cx.listener(|this, _, _, cx| {
                                let catalog = this.profiles.clone();
                                this.request_save_profile_catalog(catalog, cx);
                            }))
                            .render(colors),
                    ),
            )
            .when_some(
                self.settings_page
                    .pending_profile_delete
                    .as_ref()
                    .map(|pending| (pending.profile_id.clone(), pending.name.clone())),
                |section, (delete_id, delete_name)| {
                    let is_active = delete_id == self.profiles.active_profile_id;
                    section.child(
                        div()
                            .mt_3()
                            .flex()
                            .flex_col()
                            .gap_2()
                            .px_3()
                            .py_3()
                            .rounded_md()
                            .border_1()
                            .border_color(colors.danger)
                            .bg(with_alpha(colors.danger, 0.08))
                            .child(
                                div()
                                    .text_sm()
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(colors.text_primary)
                                    .child(self.t_args(
                                        "settings-profile-delete-title",
                                        &[("name", FluentValue::from(delete_name.as_str()))],
                                    )),
                            )
                            .child(div().text_xs().text_color(colors.text_muted).child(
                                if is_active {
                                    self.t("settings-profile-delete-active-hint")
                                } else {
                                    self.t("settings-profile-delete-hint")
                                },
                            ))
                            .child(
                                div()
                                    .flex()
                                    .flex_wrap()
                                    .gap_2()
                                    .child(
                                        Button::new(
                                            "confirm-delete-profile",
                                            self.t("settings-profile-delete-confirm"),
                                        )
                                        .aria_label(self.t_args(
                                            "settings-profile-delete-confirm-aria",
                                            &[("name", FluentValue::from(delete_name.as_str()))],
                                        ))
                                        .style(ButtonStyle::Primary)
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.confirm_remove_profile(cx);
                                        }))
                                        .render(colors),
                                    )
                                    .child(
                                        Button::new(
                                            "cancel-delete-profile",
                                            self.t("button-cancel"),
                                        )
                                        .aria_label(self.t("settings-profile-delete-cancel-aria"))
                                        .style(ButtonStyle::Secondary)
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.cancel_remove_profile(cx);
                                        }))
                                        .render(colors),
                                    ),
                            ),
                    )
                },
            )
    }

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
                                    .child(
                                        Button::new(
                                            SharedString::from(format!(
                                                "activate-core-{}",
                                                activate_id
                                            )),
                                            if is_active {
                                                self.t("settings-item-active")
                                            } else {
                                                self.t("settings-item-activate")
                                            },
                                        )
                                        .aria_label(if is_active {
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
                                        .style(if is_active {
                                            ButtonStyle::Secondary
                                        } else {
                                            ButtonStyle::Primary
                                        })
                                        .disabled(is_active)
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            this.request_core_command(
                                                CoreCommandView::Activate {
                                                    core_id: activate_id.clone(),
                                                },
                                                cx,
                                            );
                                        }))
                                        .render(colors),
                                    )
                                    .child(
                                        Button::new(
                                            SharedString::from(format!(
                                                "verify-core-{}",
                                                verify_id
                                            )),
                                            self.t("settings-core-verify"),
                                        )
                                        .aria_label(self.t_args(
                                            "settings-core-verify-aria",
                                            &[(
                                                "version",
                                                FluentValue::from(core.version.as_str()),
                                            )],
                                        ))
                                        .style(ButtonStyle::Secondary)
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            this.request_core_command(
                                                CoreCommandView::Verify {
                                                    core_id: verify_id.clone(),
                                                },
                                                cx,
                                            );
                                        }))
                                        .render(colors),
                                    )
                                    .child(
                                        Button::new(
                                            SharedString::from(format!(
                                                "remove-core-{}",
                                                remove_id
                                            )),
                                            self.t("settings-core-remove"),
                                        )
                                        .aria_label(self.t_args(
                                            "settings-core-remove-aria",
                                            &[(
                                                "version",
                                                FluentValue::from(core.version.as_str()),
                                            )],
                                        ))
                                        .style(ButtonStyle::Secondary)
                                        .disabled(is_active)
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            this.request_core_command(
                                                CoreCommandView::Remove {
                                                    core_id: remove_id.clone(),
                                                },
                                                cx,
                                            );
                                        }))
                                        .render(colors),
                                    ),
                            )
                    })),
            )
            .child(
                div()
                    .mt_3()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child(
                        div()
                            .text_xs()
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(colors.text_primary)
                            .child(self.t("ui-import-aria2c")),
                    )
                    .child(settings_path_field_row(
                        self.settings_inputs.core_path.clone(),
                        "browse-core-path",
                        self.t("button-browse"),
                        self.t("settings-core-browse-aria"),
                        PathPickTarget::CoreExecutable,
                        colors,
                        cx,
                    ))
                    .child(
                        div()
                            .flex()
                            .flex_wrap()
                            .gap_2()
                            .child(
                                Button::new("import-core", self.t("settings-core-import"))
                                    .aria_label(self.t("settings-core-import-aria"))
                                    .style(ButtonStyle::Primary)
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.request_import_core_from_input(cx);
                                    }))
                                    .render(colors),
                            )
                            .child(
                                Button::new("link-core", self.t("settings-core-link"))
                                    .aria_label(self.t("settings-core-link-aria"))
                                    .style(ButtonStyle::Secondary)
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.request_link_core_from_input(cx);
                                    }))
                                    .render(colors),
                            )
                            .child(
                                Button::new("rollback-core", self.t("settings-core-rollback"))
                                    .aria_label(self.t("settings-core-rollback-aria"))
                                    .style(ButtonStyle::Secondary)
                                    .disabled(!can_rollback)
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.request_core_command(CoreCommandView::Rollback, cx);
                                    }))
                                    .render(colors),
                            ),
                    ),
            )
    }

    pub(crate) fn render_settings_network(&mut self, cx: &mut Context<Self>) -> Div {
        let colors = self.theme.colors;
        let pending = self.pending_settings_save.is_some();
        let password_changed = !self
            .settings_inputs
            .proxy_password
            .read(cx)
            .text()
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
            all_proxy: self.settings_inputs.all_proxy.read(cx).text().trim().into(),
            http_proxy: self
                .settings_inputs
                .http_proxy
                .read(cx)
                .text()
                .trim()
                .into(),
            https_proxy: self
                .settings_inputs
                .https_proxy
                .read(cx)
                .text()
                .trim()
                .into(),
            ftp_proxy: self.settings_inputs.ftp_proxy.read(cx).text().trim().into(),
            no_proxy: self
                .settings_inputs
                .no_proxy
                .read(cx)
                .text()
                .split(',')
                .map(str::trim)
                .filter(|entry| !entry.is_empty())
                .map(ToOwned::to_owned)
                .collect(),
            username: self
                .settings_inputs
                .proxy_username
                .read(cx)
                .text()
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
                    Toggle::new("toggle-check-certificate", draft_check_certificate)
                        .aria_label(self.t("settings-proxy-check-certificate-aria"))
                        .disabled(pending)
                        .on_click(move |_, _, cx| {
                            cert_shell
                                .update(cx, |shell, cx| shell.toggle_check_certificate(cx))
                                .ok();
                        })
                        .render(colors),
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
                            self.settings_inputs.all_proxy.clone(),
                            colors,
                        ))
                        .child(
                            div()
                                .flex()
                                .gap_3()
                                .child(
                                    settings_labeled_input(
                                        self.t("settings-proxy-http"),
                                        self.settings_inputs.http_proxy.clone(),
                                        colors,
                                    )
                                    .flex_1()
                                    .min_w_0(),
                                )
                                .child(
                                    settings_labeled_input(
                                        self.t("settings-proxy-https"),
                                        self.settings_inputs.https_proxy.clone(),
                                        colors,
                                    )
                                    .flex_1()
                                    .min_w_0(),
                                ),
                        )
                        .child(settings_labeled_input(
                            self.t("settings-proxy-ftp"),
                            self.settings_inputs.ftp_proxy.clone(),
                            colors,
                        ))
                        .child(settings_labeled_input(
                            self.t("settings-proxy-bypass"),
                            self.settings_inputs.no_proxy.clone(),
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
                                        self.settings_inputs.proxy_username.clone(),
                                        colors,
                                    )
                                    .flex_1()
                                    .min_w_0(),
                                )
                                .child(
                                    settings_labeled_input(
                                        self.t("settings-proxy-password"),
                                        self.settings_inputs.proxy_password.clone(),
                                        colors,
                                    )
                                    .flex_1()
                                    .min_w_0(),
                                )
                                .when(self.settings.download_proxy.has_password, |row| {
                                    row.child(
                                        IconButton::new(
                                            "clear-proxy-password",
                                            password_button_icon,
                                        )
                                        .aria_label(password_button_label.clone())
                                        .disabled(pending)
                                        .tooltip(Tooltip::new(password_button_label.clone()))
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.clear_saved_proxy_password(cx);
                                        }))
                                        .render(colors),
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
                                IconButton::new(
                                    "clear-disabled-proxy-password",
                                    password_button_icon,
                                )
                                .aria_label(password_button_label.clone())
                                .disabled(pending)
                                .tooltip(Tooltip::new(password_button_label.clone()))
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.clear_saved_proxy_password(cx);
                                }))
                                .render(colors),
                            ),
                    )
                },
            )
    }

    pub(crate) fn render_settings_transfers(&mut self, cx: &mut Context<Self>) -> Div {
        let colors = self.theme.colors;
        let pending = self.pending_settings_save.is_some();
        let allocation_selected = FileAllocationView::all()
            .iter()
            .position(|method| *method == self.settings_page.draft_file_allocation)
            .unwrap_or(1);
        let allocation_shell = cx.entity().downgrade();
        let allocation_control = SegmentedControl::new(
            "settings-file-allocation",
            FileAllocationView::all().map(|method| {
                Segment::new(self.t(match method {
                    FileAllocationView::None => "settings-file-allocation-none",
                    FileAllocationView::Prealloc => "settings-file-allocation-prealloc",
                    FileAllocationView::Trunc => "settings-file-allocation-trunc",
                    FileAllocationView::Falloc => "settings-file-allocation-falloc",
                }))
            }),
            allocation_selected,
            self.theme,
        )
        .aria_label(self.t("settings-file-allocation-aria"))
        .disabled(pending)
        .on_select(move |index, _window, cx| {
            let method = FileAllocationView::all()
                .get(index)
                .copied()
                .unwrap_or_default();
            allocation_shell
                .update(cx, |shell, cx| shell.select_file_allocation(method, cx))
                .ok();
        });
        let draft_check_integrity = self.settings_page.draft_check_integrity;
        let integrity_shell = cx.entity().downgrade();
        div()
            .flex()
            .flex_col()
            .gap_6()
            .child(
                settings_section_owned(self.t("settings-speed-limits"), colors).child(
                    div()
                        .py_4()
                        .max_w(px(760.0))
                        .flex()
                        .gap_3()
                        .child(
                            settings_labeled_input(
                                self.t("settings-download-limit"),
                                self.settings_inputs.download_limit.clone(),
                                colors,
                            )
                            .flex_1()
                            .min_w_0(),
                        )
                        .child(
                            settings_labeled_input(
                                self.t("settings-upload-limit"),
                                self.settings_inputs.upload_limit.clone(),
                                colors,
                            )
                            .flex_1()
                            .min_w_0(),
                        ),
                ),
            )
            .child(
                settings_section_owned(self.t("settings-transfer-policy"), colors).child(
                    div()
                        .pt_4()
                        .max_w(px(760.0))
                        .flex()
                        .flex_col()
                        .gap_3()
                        .child(
                            div()
                                .flex()
                                .gap_3()
                                .child(
                                    settings_labeled_input(
                                        self.t("settings-max-concurrent"),
                                        self.settings_inputs.max_concurrent.clone(),
                                        colors,
                                    )
                                    .flex_1()
                                    .min_w_0(),
                                )
                                .child(
                                    settings_labeled_input(
                                        self.t("settings-connections-per-server"),
                                        self.settings_inputs.max_connection.clone(),
                                        colors,
                                    )
                                    .flex_1()
                                    .min_w_0(),
                                ),
                        )
                        .child(
                            div()
                                .flex()
                                .gap_3()
                                .child(
                                    settings_labeled_input(
                                        self.t("settings-split"),
                                        self.settings_inputs.split.clone(),
                                        colors,
                                    )
                                    .flex_1()
                                    .min_w_0(),
                                )
                                .child(
                                    settings_labeled_input(
                                        self.t("settings-min-split-size"),
                                        self.settings_inputs.min_split_size.clone(),
                                        colors,
                                    )
                                    .flex_1()
                                    .min_w_0(),
                                ),
                        )
                        .child(settings_section_row_owned(
                            self.t("settings-file-allocation"),
                            Some(self.t("settings-file-allocation-desc")),
                            allocation_control,
                            colors,
                        ))
                        .child(settings_section_row_owned(
                            self.t("settings-integrity-check"),
                            Some(self.t("settings-integrity-check-desc")),
                            Toggle::new("toggle-check-integrity", draft_check_integrity)
                                .aria_label(if draft_check_integrity {
                                    self.t("settings-integrity-check-disable")
                                } else {
                                    self.t("settings-integrity-check-enable")
                                })
                                .disabled(pending)
                                .on_click(move |_, _, cx| {
                                    integrity_shell
                                        .update(cx, |shell, cx| {
                                            shell.toggle_check_integrity(cx);
                                        })
                                        .ok();
                                })
                                .render(colors),
                            colors,
                        ))
                        .child(
                            div()
                                .text_xs()
                                .text_color(colors.text_muted)
                                .child(self.t("settings-transfer-policy-hint")),
                        ),
                ),
            )
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
                Toggle::new("toggle-notify-completion", draft_completion)
                    .aria_label(self.t("settings-notify-completion"))
                    .disabled(pending)
                    .on_click(move |_, _, cx| {
                        s1.update(cx, |shell, cx| shell.toggle_notify_on_completion(cx))
                            .ok();
                    })
                    .render(colors),
                colors,
            ))
            .child(settings_section_row_owned(
                self.t("settings-notify-error"),
                Some(self.t("settings-notify-error-desc")),
                Toggle::new("toggle-notify-error", draft_error)
                    .aria_label(self.t("settings-notify-error"))
                    .disabled(pending)
                    .on_click(move |_, _, cx| {
                        s2.update(cx, |shell, cx| shell.toggle_notify_on_error(cx))
                            .ok();
                    })
                    .render(colors),
                colors,
            ))
            .child(settings_section_row_owned(
                self.t("settings-notify-engine"),
                Some(self.t("settings-notify-engine-desc")),
                Toggle::new("toggle-notify-engine", draft_engine)
                    .aria_label(self.t("settings-notify-engine"))
                    .disabled(pending)
                    .on_click(move |_, _, cx| {
                        s3.update(cx, |shell, cx| shell.toggle_notify_on_engine_events(cx))
                            .ok();
                    })
                    .render(colors),
                colors,
            ))
            .child(settings_section_row_owned(
                self.t("settings-notify-os"),
                Some(self.t("settings-notify-os-desc")),
                Toggle::new("toggle-os-notifications", draft_os)
                    .aria_label(self.t("settings-notify-os"))
                    .disabled(pending)
                    .on_click(move |_, _, cx| {
                        s4.update(cx, |shell, cx| shell.toggle_os_notifications(cx))
                            .ok();
                    })
                    .render(colors),
                colors,
            ))
            .child(settings_section_row_owned(
                self.t("settings-notify-low-disk"),
                Some(self.t("settings-notify-low-disk-desc")),
                Toggle::new("toggle-notify-low-disk", draft_low_disk)
                    .aria_label(self.t("settings-notify-low-disk"))
                    .disabled(pending)
                    .on_click(move |_, _, cx| {
                        s5.update(cx, |shell, cx| shell.toggle_notify_on_low_disk(cx))
                            .ok();
                    })
                    .render(colors),
                colors,
            ))
    }

    pub(crate) fn render_settings_system(&mut self, cx: &mut Context<Self>) -> Div {
        let colors = self.theme.colors;
        let pending = self.pending_settings_save.is_some();
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
        settings_section_owned(self.t("settings-window-tray"), colors)
            .child(settings_section_row_owned(
                self.t("settings-tray-icon"),
                Some(self.t("settings-tray-icon-desc")),
                Toggle::new("toggle-show-tray", draft_show_tray)
                    .aria_label(self.t("settings-tray-icon"))
                    .disabled(pending)
                    .on_click(move |_, _, cx| {
                        s1.update(cx, |shell, cx| shell.toggle_show_tray_icon(cx))
                            .ok();
                    })
                    .render(colors),
                colors,
            ))
            .child(settings_section_row_owned(
                self.t("settings-start-minimized"),
                Some(self.t("settings-start-minimized-desc")),
                Toggle::new("toggle-start-minimized", draft_start_minimized)
                    .aria_label(self.t("settings-start-minimized"))
                    .disabled(pending || !draft_show_tray)
                    .on_click(move |_, _, cx| {
                        s2.update(cx, |shell, cx| shell.toggle_start_minimized_to_tray(cx))
                            .ok();
                    })
                    .render(colors),
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
            )
    }

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
        let diagnostic_button = Button::new("export-diagnostics", diagnostics_label.clone())
            .icon(IconName::Download)
            .aria_label(diagnostics_aria)
            .on_click(move |_, window, cx| {
                diagnostic_shell
                    .update(cx, |shell, cx| shell.export_diagnostics(window, cx))
                    .ok();
            });
        let transfer_pending = self.pending_settings_save.is_some();
        let export_shell = cx.entity().downgrade();
        let settings_export_button = Button::new("export-settings", settings_export_label.clone())
            .icon(IconName::Download)
            .aria_label(settings_export_aria)
            .disabled(transfer_pending)
            .on_click(move |_, window, cx| {
                export_shell
                    .update(cx, |shell, cx| shell.export_settings_file(window, cx))
                    .ok();
            });
        let import_shell = cx.entity().downgrade();
        let settings_import_button = Button::new("import-settings", settings_import_label.clone())
            .icon(IconName::ArrowUp)
            .aria_label(settings_import_aria)
            .disabled(transfer_pending)
            .on_click(move |_, window, cx| {
                import_shell
                    .update(cx, |shell, cx| shell.import_settings_file(window, cx))
                    .ok();
            });

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
                        env!("CARGO_PKG_VERSION"),
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
            .child(
                settings_section_owned(transfer_title, colors)
                    .child(settings_section_row_owned(
                        settings_export_label,
                        Some(settings_export_description),
                        settings_export_button.render(colors),
                        colors,
                    ))
                    .child(settings_section_row_owned(
                        settings_import_label,
                        Some(settings_import_description),
                        settings_import_button.render(colors),
                        colors,
                    )),
            )
            .child(settings_section_owned(diagnostics_title, colors).child(
                settings_section_row_owned(
                    diagnostics_label,
                    Some(diagnostics_description),
                    diagnostic_button.render(colors),
                    colors,
                ),
            ))
    }
}

/// An unframed settings group. The heading divider provides hierarchy without
/// turning each section into a decorative card.
fn settings_section_owned(title: impl Into<SharedString>, colors: crate::ThemeColors) -> Div {
    div()
        .flex()
        .flex_col()
        .child(settings_section_heading(title, colors))
}

fn settings_section_heading(title: impl Into<SharedString>, colors: crate::ThemeColors) -> Div {
    div()
        .min_h(px(40.0))
        .pb_3()
        .flex()
        .items_end()
        .border_b_1()
        .border_color(colors.border)
        .text_sm()
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(colors.text_primary)
        .child(title.into())
}

fn settings_section_row_owned(
    label: impl Into<SharedString>,
    description: Option<impl Into<SharedString>>,
    control: impl IntoElement,
    colors: crate::ThemeColors,
) -> Div {
    div()
        .min_h(px(64.0))
        .py_3()
        .border_b_1()
        .border_color(colors.border)
        .child(settings_row_owned(label, description, control, colors))
}

fn settings_section_info_row_owned(
    label: impl Into<SharedString>,
    value: impl Into<SharedString>,
    colors: crate::ThemeColors,
) -> Div {
    div()
        .min_h(px(48.0))
        .py_3()
        .border_b_1()
        .border_color(colors.border)
        .child(settings_info_row_owned(label, value, colors))
}
