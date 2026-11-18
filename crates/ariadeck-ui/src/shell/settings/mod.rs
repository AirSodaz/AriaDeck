//! Settings page coordination, navigation, persistence triggers, and shared helpers.

use super::*;

mod about;
mod categories;
mod engine;
mod general;
mod network;
mod profiles;
mod system;
mod transfers;
mod updates;

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
                self.apply_settings(result.settings, Some(window), cx);
                self.settings_page.error = None;
                let message = match source {
                    SettingsSaveSource::Theme => self.t("notice-settings-appearance"),
                    SettingsSaveSource::Language => self.t("settings-language-saved"),
                    SettingsSaveSource::Categories => self.t("notice-settings-categories"),
                    SettingsSaveSource::Proxy => {
                        self.settings_inputs
                            .proxy_password
                            .update(cx, |input, cx| input.set_value("", window, cx));
                        self.settings_page.clear_proxy_password = false;
                        self.t("notice-settings-proxy")
                    }
                    SettingsSaveSource::SpeedLimit => {
                        self.sync_speed_limit_fields_from_settings(Some(window), cx);
                        self.t("notice-settings-speed")
                    }
                    SettingsSaveSource::TransferPolicy => {
                        self.sync_transfer_policy_fields_from_settings(Some(window), cx);
                        self.t("notice-settings-transfer-policy")
                    }
                    SettingsSaveSource::Transfers => {
                        self.sync_speed_limit_fields_from_settings(Some(window), cx);
                        self.sync_transfer_policy_fields_from_settings(Some(window), cx);
                        self.sync_tracker_list_fields_from_settings(Some(window), cx);
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
    pub fn apply_settings(
        &mut self,
        settings: SettingsView,
        mut window: Option<&mut Window>,
        cx: &mut Context<Self>,
    ) {
        if let Some(w) = window.as_deref() {
            self.theme = resolve_theme(settings.color_scheme, w);
        } else if settings.color_scheme != ColorSchemeView::System {
            self.theme = theme_for_scheme(settings.color_scheme);
        }
        self.theme.sync_to_kit(window.as_deref_mut(), cx);
        self.settings = settings.clone();
        self.settings_page.draft_color_scheme = settings.color_scheme;
        self.settings_page.draft_categories = settings.categories.clone();
        self.settings_page.draft_category_edit_index = None;
        self.settings_page.draft_language = settings.language;
        self.set_language_runtime(settings.language);
        if let Some(window) = window.as_deref_mut() {
            self.refresh_input_placeholders(window, cx);
        }
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
        self.settings_page.draft_keep_awake_while_active =
            settings.platform.keep_awake_while_active;
        self.settings_page.draft_action_after_done = settings.platform.action_after_done;
        self.settings_page.draft_bridge_allow_cookies = settings.browser_bridge.allow_cookies;
        self.settings_page.draft_bridge_auto_submit = settings.browser_bridge.auto_submit;
        self.settings_page.draft_tracker_enabled = settings.tracker_list.enabled;
        self.settings_page.draft_tracker_source = settings.tracker_list.source;
        self.settings_page.draft_tracker_auto_refresh = settings.tracker_list.auto_refresh;
        if let Some(window) = window {
            let download_directory = settings.download_directory.clone();
            self.settings_inputs.directory.update(cx, |input, cx| {
                input.set_value(download_directory, window, cx)
            });
            let proxy = settings.download_proxy.clone();
            self.settings_inputs.all_proxy.update(cx, |input, cx| {
                input.set_value(proxy.all_proxy.clone(), window, cx);
            });
            self.settings_inputs.http_proxy.update(cx, |input, cx| {
                input.set_value(proxy.http_proxy.clone(), window, cx);
            });
            self.settings_inputs.https_proxy.update(cx, |input, cx| {
                input.set_value(proxy.https_proxy.clone(), window, cx);
            });
            self.settings_inputs.ftp_proxy.update(cx, |input, cx| {
                input.set_value(proxy.ftp_proxy.clone(), window, cx);
            });
            self.settings_inputs.no_proxy.update(cx, |input, cx| {
                input.set_value(proxy.no_proxy.join(", "), window, cx);
            });
            self.settings_inputs.proxy_username.update(cx, |input, cx| {
                input.set_value(proxy.username.clone(), window, cx);
            });
            self.settings_inputs
                .proxy_password
                .update(cx, |input, cx| input.set_value("", window, cx));
            let speed_limits = settings.speed_limits.clone();
            self.settings_inputs.download_limit.update(cx, |input, cx| {
                input.set_value(speed_limits.download_limit.clone(), window, cx);
            });
            self.settings_inputs.upload_limit.update(cx, |input, cx| {
                input.set_value(speed_limits.upload_limit.clone(), window, cx);
            });
            let transfer_policy = settings.transfer_policy.clone();
            self.settings_inputs.max_concurrent.update(cx, |input, cx| {
                input.set_value(transfer_policy.max_concurrent_downloads.clone(), window, cx);
            });
            self.settings_inputs.max_connection.update(cx, |input, cx| {
                input.set_value(
                    transfer_policy.max_connection_per_server.clone(),
                    window,
                    cx,
                );
            });
            self.settings_inputs.split.update(cx, |input, cx| {
                input.set_value(transfer_policy.split.clone(), window, cx);
            });
            self.settings_inputs.min_split_size.update(cx, |input, cx| {
                input.set_value(transfer_policy.min_split_size.clone(), window, cx);
            });
            let tracker = settings.tracker_list.clone();
            self.settings_inputs
                .tracker_custom_url
                .update(cx, |input, cx| {
                    input.set_value(tracker.custom_url.clone(), window, cx);
                });
            self.settings_inputs
                .tracker_list_text
                .update(cx, |input, cx| {
                    input.set_value(tracker.list_text.clone(), window, cx);
                });
        }
        cx.notify();
    }

    /// Push the current shell theme into every TextField so light/dark chrome stays in sync.
    // --- open_settings..toggle_start_minimized_to_tray ---
    pub(crate) fn open_settings(
        &mut self,
        _: &OpenSettings,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_settings_page(SettingsCategory::default(), window, cx);
    }

    /// Open Settings on a specific category.
    ///
    /// The core-setup guide uses this to hand the user to Settings → Engine,
    /// where the same panel lives (B4).
    pub(crate) fn open_settings_page(
        &mut self,
        category: SettingsCategory,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.page == AppPage::Settings {
            // Already here: honour the requested category rather than no-op, so a
            // hand-off from elsewhere still lands on the right section.
            self.settings_page.active_category = category;
            window.focus(&self.focus_handle, cx);
            cx.notify();
            return;
        }
        if self.add_dialog.open
            || self.output_name_dialog.is_some()
            || self.remove_confirmation.is_some()
            || self.batch_failure_details.is_some()
            // The core-setup guide and Settings > Engine render the same shared
            // panel — including the same path input entity — so exactly one of
            // them may be mounted at a time. The guide's own button routes here
            // via `open_engine_settings_from_onboarding`, which dismisses first.
            || self.core_setup.onboarding_open
        {
            return;
        }
        let download_directory = self.settings.download_directory.clone();
        self.settings_inputs.directory.update(cx, |input, cx| {
            input.set_value(download_directory, window, cx)
        });
        let proxy = self.settings.download_proxy.clone();
        self.settings_inputs.all_proxy.update(cx, |input, cx| {
            input.set_value(proxy.all_proxy.clone(), window, cx);
        });
        self.settings_inputs.http_proxy.update(cx, |input, cx| {
            input.set_value(proxy.http_proxy.clone(), window, cx);
        });
        self.settings_inputs.https_proxy.update(cx, |input, cx| {
            input.set_value(proxy.https_proxy.clone(), window, cx);
        });
        self.settings_inputs.ftp_proxy.update(cx, |input, cx| {
            input.set_value(proxy.ftp_proxy.clone(), window, cx);
        });
        self.settings_inputs.no_proxy.update(cx, |input, cx| {
            input.set_value(proxy.no_proxy.join(", "), window, cx);
        });
        self.settings_inputs.proxy_username.update(cx, |input, cx| {
            input.set_value(proxy.username.clone(), window, cx);
        });
        self.settings_inputs
            .proxy_password
            .update(cx, |input, cx| input.set_value("", window, cx));
        let speed_limits = self.settings.speed_limits.clone();
        self.settings_inputs.download_limit.update(cx, |input, cx| {
            input.set_value(speed_limits.download_limit.clone(), window, cx);
        });
        self.settings_inputs.upload_limit.update(cx, |input, cx| {
            input.set_value(speed_limits.upload_limit.clone(), window, cx);
        });
        let transfer_policy = self.settings.transfer_policy.clone();
        self.settings_inputs.max_concurrent.update(cx, |input, cx| {
            input.set_value(transfer_policy.max_concurrent_downloads.clone(), window, cx);
        });
        self.settings_inputs.max_connection.update(cx, |input, cx| {
            input.set_value(
                transfer_policy.max_connection_per_server.clone(),
                window,
                cx,
            );
        });
        self.settings_inputs.split.update(cx, |input, cx| {
            input.set_value(transfer_policy.split.clone(), window, cx);
        });
        self.settings_inputs.min_split_size.update(cx, |input, cx| {
            input.set_value(transfer_policy.min_split_size.clone(), window, cx);
        });
        let tracker_list = self.settings.tracker_list.clone();
        self.settings_inputs
            .tracker_custom_url
            .update(cx, |input, cx| {
                input.set_value(tracker_list.custom_url.clone(), window, cx);
            });
        self.settings_inputs
            .tracker_list_text
            .update(cx, |input, cx| {
                input.set_value(tracker_list.list_text.clone(), window, cx);
            });
        self.page = AppPage::Settings;
        self.details_drawer = None;
        self.active_status_popover = None;
        self.settings_page = SettingsPage {
            previous_focus: window.focused(cx).map(|focus| focus.downgrade()),
            active_category: category,
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
            draft_keep_awake_while_active: self.settings.platform.keep_awake_while_active,
            draft_action_after_done: self.settings.platform.action_after_done,
            draft_bridge_allow_cookies: self.settings.browser_bridge.allow_cookies,
            draft_bridge_auto_submit: self.settings.browser_bridge.auto_submit,
            draft_categories: self.settings.categories.clone(),
            draft_category_edit_index: None,
            draft_tracker_enabled: self.settings.tracker_list.enabled,
            draft_tracker_source: self.settings.tracker_list.source,
            draft_tracker_auto_refresh: self.settings.tracker_list.auto_refresh,
            pending_tracker_refresh: None,
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
        self.flush_category_editor(cx);
        let mut categories = self.settings_page.draft_categories.clone();
        if categories.is_empty() {
            self.settings_page.error = Some(OperationErrorView {
                code: "settings.empty_categories".into(),
                summary: self.t("error-settings-empty-categories"),
                retryable: false,
            });
            cx.notify();
            return;
        }
        // General is the fixed fallback category (no UI toggle).
        for category in &mut categories {
            category.is_fallback = category.name.eq_ignore_ascii_case("General");
        }
        if !categories.iter().any(|c| c.is_fallback)
            && let Some(first) = categories.first_mut()
        {
            first.is_fallback = true;
        } else {
            // Exactly one fallback if multiple "General" names somehow exist.
            let mut seen = false;
            for category in &mut categories {
                if category.is_fallback {
                    if seen {
                        category.is_fallback = false;
                    } else {
                        seen = true;
                    }
                }
            }
        }
        let download_directory = categories
            .iter()
            .find(|c| c.is_fallback)
            .map(|c| c.directory.clone())
            .filter(|d| !d.is_empty())
            .unwrap_or_else(|| self.settings.download_directory.clone());
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
        settings.categories = categories;
        self.request_settings_save(
            settings,
            ProxyPasswordUpdateView::Unchanged,
            SettingsSaveSource::Categories,
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
            PathPickTarget::CategoryDirectory { .. } => {
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
                category.directory = display.clone();
            }
            // Keep the edit-panel path field in sync when browsing while editing.
            if self.settings_page.draft_category_edit_index == Some(index) {
                self.settings_inputs.directory.update(cx, |input, cx| {
                    input.set_value(display, window, cx);
                });
            }
            if self.page == AppPage::Settings {
                self.settings_page.error = None;
            }
            cx.notify();
            return;
        }
        let field = match target {
            PathPickTarget::CoreExecutable => self.settings_inputs.core_path.clone(),
            PathPickTarget::ProfileExecutable => self.settings_inputs.profile_executable.clone(),
            PathPickTarget::CategoryDirectory { .. } => unreachable!(),
        };
        field.update(cx, |input, cx| input.set_value(display, window, cx));
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
            .flex_1()
            .min_w_0()
            .min_h_0()
            .h_full()
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
                    .min_w_0()
                    .min_h_0()
                    .flex()
                    .child(self.render_settings_nav(cx))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .min_h_0()
                            .flex()
                            .flex_col()
                            .child(
                                div()
                                    .id("settings-scroll-shell")
                                    .relative()
                                    .flex_1()
                                    .min_w_0()
                                    .min_h_0()
                                    .overflow_hidden()
                                    .child(
                                        div()
                                            .id("settings-scroll")
                                            .size_full()
                                            .min_w_0()
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
                                    .vertical_scrollbar(&self.settings_scroll),
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
            .min_h_0()
            .overflow_y_scroll()
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
                let dirty = self.settings_page.draft_categories != self.settings.categories;
                let saving = self
                    .pending_settings_save
                    .as_ref()
                    .is_some_and(|p| p.source == SettingsSaveSource::Categories);
                (dirty, saving)
            }
            SettingsCategory::Network => {
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
                        .filter(|e| !e.is_empty())
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
                let tracker_list_draft = self.read_tracker_list_draft(cx);
                // Dirty is independent of validity so invalid edits still show the
                // footer; submit_transfers reports validation errors on click.
                let dirty = speed_limit_draft != self.settings.speed_limits
                    || transfer_policy_draft != self.settings.transfer_policy
                    || tracker_list_draft != self.settings.tracker_list;
                let saving = self.pending_settings_save.as_ref().is_some_and(|p| {
                    matches!(
                        p.source,
                        SettingsSaveSource::SpeedLimit
                            | SettingsSaveSource::TransferPolicy
                            | SettingsSaveSource::Transfers
                    )
                }) || self.settings_page.pending_tracker_refresh.is_some();
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
                    keep_awake_while_active: self.settings_page.draft_keep_awake_while_active,
                    action_after_done: self.settings_page.draft_action_after_done,
                };
                let dirty = platform_draft != self.settings.platform
                    || self.browser_bridge_draft() != self.settings.browser_bridge;
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
                KitButton::new("footer-save")
                    .primary()
                    .label(self.t("button-save"))
                    .accessibility_label(if saving {
                        self.t("settings-save-saving")
                    } else {
                        self.t("settings-save")
                    })
                    .disabled(pending || !dirty)
                    .loading(saving)
                    .on_click(cx.listener(move |this, _, _, cx| match active_category {
                        SettingsCategory::General => this.submit_settings(cx),
                        SettingsCategory::Network => this.submit_proxy_settings(cx),
                        SettingsCategory::Transfers => this.submit_transfers(cx),
                        SettingsCategory::Notifications => this.submit_notifications(cx),
                        SettingsCategory::System => this.submit_platform(cx),
                        _ => {}
                    })),
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
