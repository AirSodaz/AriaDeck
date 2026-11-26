//! Settings page child entity, navigation, persistence triggers, and shared helpers.

use super::core_setup::CoreSetupPanelEvent;
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

pub(super) enum SettingsPanelEvent {
    Close,
    SelectColorScheme(ColorSchemeView),
    SelectLanguage(LanguagePreferenceView),
    SaveSettings {
        settings: Box<SettingsView>,
        proxy_password: ProxyPasswordUpdateView,
        source: SettingsSaveSource,
    },
    ExportSettings,
    ImportSettings,
    ExportDiagnostics,
    RefreshTrackerList {
        draft: TrackerListSettingsView,
    },
    SwitchProfile(String),
    SaveProfileCatalog {
        catalog: ProfileCatalogView,
        secret_updates: HashMap<String, ProfileRpcSecretUpdateView>,
    },
    ProfileCatalogChanged(ProfileCatalogView),
    CoreCommand(CoreCommandView),
    CoreDiscovery,
    CoreDownload,
    CheckForUpdates,
    DownloadUpdate,
    InstallUpdate,
    Notice {
        message: String,
        is_error: bool,
    },
}

pub(crate) struct SettingsPanelView {
    pub(crate) theme: Theme,
    pub(crate) translator: Translator,
    pub(crate) settings: SettingsView,
    pub(crate) platform_capabilities: PlatformCapabilitiesView,
    pub(crate) profiles: ProfileCatalogView,
    pub(crate) cores: CoreRegistryView,
    pub(crate) aria2_version: String,
    pub(crate) update_enabled: bool,
    pub(crate) update_portable: bool,
    pub(crate) update_status: crate::UpdateStatusView,
    pub(crate) pending_settings_save_source: Option<SettingsSaveSource>,
    pub(crate) pending_tracker_refresh: bool,
    pub(crate) settings_page: SettingsPage,
    pub(crate) settings_inputs: SettingsInputs,
    pub(crate) settings_scroll: ScrollHandle,
    pub(crate) core_setup_panel: Entity<CoreSetupPanelView>,
    focus_handle: FocusHandle,
    nav_focus_handles: [FocusHandle; 8],
    next_draft_profile_seq: u64,
    _core_setup_sub: Subscription,
    _input_subs: Vec<Subscription>,
}

impl gpui::EventEmitter<SettingsPanelEvent> for SettingsPanelView {}

impl Focusable for SettingsPanelView {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

pub(super) struct SettingsPanelContext<'a> {
    pub(super) theme: Theme,
    pub(super) translator: Translator,
    pub(super) platform_capabilities: PlatformCapabilitiesView,
    pub(super) profiles: ProfileCatalogView,
    pub(super) cores: CoreRegistryView,
    pub(super) core_setup: &'a CoreSetupState,
    pub(super) aria2_version: String,
    pub(super) update_enabled: bool,
    pub(super) update_portable: bool,
    pub(super) update_status: crate::UpdateStatusView,
    pub(super) pending_settings_save_source: Option<SettingsSaveSource>,
    pub(super) pending_tracker_refresh: bool,
}

impl SettingsPanelView {
    pub(super) fn new(
        category: SettingsCategory,
        previous_focus: Option<WeakFocusHandle>,
        settings: SettingsView,
        panel_cx: SettingsPanelContext<'_>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let SettingsPanelContext {
            theme,
            translator,
            platform_capabilities,
            profiles,
            cores,
            core_setup,
            aria2_version,
            update_enabled,
            update_portable,
            update_status,
            pending_settings_save_source,
            pending_tracker_refresh,
        } = panel_cx;
        let directory = make_input("D:\\Downloads".into(), false, window, cx);
        let profile_name = make_input(
            translator.t("settings-profile-name-placeholder").into(),
            false,
            window,
            cx,
        );
        let profile_executable = make_input(
            translator
                .t("settings-profile-executable-placeholder")
                .into(),
            false,
            window,
            cx,
        );
        let profile_endpoint = make_input("wss://host:6800/jsonrpc".into(), false, window, cx);
        let profile_secret = make_input(
            translator.t("settings-profile-secret-placeholder").into(),
            true,
            window,
            cx,
        );
        let all_proxy = make_input("http://proxy.example:8080".into(), false, window, cx);
        let http_proxy = make_input("http://proxy.example:8080".into(), false, window, cx);
        let https_proxy = make_input("http://proxy.example:8080".into(), false, window, cx);
        let ftp_proxy = make_input("http://proxy.example:8080".into(), false, window, cx);
        let no_proxy = make_input("localhost, 127.0.0.1, 10.0.0.0/8".into(), false, window, cx);
        let proxy_username = make_input(
            translator.t("settings-proxy-username-placeholder").into(),
            false,
            window,
            cx,
        );
        let proxy_password = make_input(
            translator.t("settings-proxy-password-placeholder").into(),
            true,
            window,
            cx,
        );
        let download_limit = make_input(
            translator.t("input-download-limit-placeholder").into(),
            false,
            window,
            cx,
        );
        let upload_limit = make_input(
            translator.t("input-upload-limit-placeholder").into(),
            false,
            window,
            cx,
        );
        let max_concurrent = make_input("5".into(), false, window, cx);
        let max_connection = make_input("1-16".into(), false, window, cx);
        let split = make_input("5".into(), false, window, cx);
        let min_split_size = make_input("20M".into(), false, window, cx);
        let tracker_custom_url =
            make_input("https://example.com/trackers.txt".into(), false, window, cx);
        let tracker_list_text =
            make_input("https://tracker.example/announce".into(), false, window, cx);
        let category_name = make_input(
            translator.t("settings-category-name-placeholder").into(),
            false,
            window,
            cx,
        );
        let category_extensions = make_input("mp4, mkv, zip".into(), false, window, cx);

        let mut input_subs = [
            &directory,
            &profile_name,
            &profile_executable,
            &profile_endpoint,
            &profile_secret,
            &all_proxy,
            &http_proxy,
            &https_proxy,
            &ftp_proxy,
            &no_proxy,
            &proxy_username,
            &download_limit,
            &upload_limit,
            &max_concurrent,
            &max_connection,
            &split,
            &min_split_size,
            &tracker_custom_url,
            &tracker_list_text,
            &category_name,
            &category_extensions,
        ]
        .into_iter()
        .map(|input| {
            cx.subscribe(input, |this: &mut Self, _input, event: &InputEvent, cx| {
                if matches!(event, InputEvent::Change)
                    && this.pending_settings_save_source.is_none()
                    && this.settings_page.error.take().is_some()
                {
                    cx.notify();
                }
            })
        })
        .collect::<Vec<_>>();
        input_subs.push(cx.subscribe(
            &proxy_password,
            |this: &mut Self, _input, event: &InputEvent, cx| {
                if let InputEvent::Change = event {
                    let changed = this.settings_page.clear_proxy_password
                        || (this.pending_settings_save_source.is_none()
                            && this.settings_page.error.is_some());
                    this.settings_page.clear_proxy_password = false;
                    this.settings_page.error = None;
                    if changed {
                        cx.notify();
                    }
                }
            },
        ));

        let core_setup_panel = cx.new(|cx| {
            CoreSetupPanelView::new(
                "settings",
                theme,
                translator.clone(),
                core_setup,
                window,
                cx,
            )
        });
        let core_setup_sub = cx.subscribe(
            &core_setup_panel,
            |_this, _panel, event: &CoreSetupPanelEvent, cx| match event {
                CoreSetupPanelEvent::Discover => {
                    cx.emit(SettingsPanelEvent::CoreDiscovery);
                }
                CoreSetupPanelEvent::Download => {
                    cx.emit(SettingsPanelEvent::CoreDownload);
                }
                CoreSetupPanelEvent::Import(path) => {
                    cx.emit(SettingsPanelEvent::CoreCommand(CoreCommandView::Import {
                        path: path.clone(),
                    }));
                }
                CoreSetupPanelEvent::Link(path) => {
                    cx.emit(SettingsPanelEvent::CoreCommand(CoreCommandView::Link {
                        path: path.clone(),
                    }));
                }
                CoreSetupPanelEvent::Notice { message, is_error } => {
                    cx.emit(SettingsPanelEvent::Notice {
                        message: message.clone(),
                        is_error: *is_error,
                    });
                }
            },
        );

        let settings_page = SettingsPage {
            previous_focus,
            active_category: category,
            draft_color_scheme: settings.color_scheme,
            draft_language: settings.language,
            draft_proxy_mode: settings.download_proxy.mode,
            draft_check_certificate: settings.download_proxy.check_certificate,
            draft_file_allocation: settings.transfer_policy.file_allocation,
            draft_check_integrity: settings.transfer_policy.check_integrity,
            draft_notifications: settings.notifications,
            draft_platform: settings.platform,
            draft_browser_bridge: settings.browser_bridge,
            draft_categories: settings.categories.clone(),
            draft_category_edit_index: None,
            next_draft_category_seq: 0,
            draft_tracker_enabled: settings.tracker_list.enabled,
            draft_tracker_source: settings.tracker_list.source,
            draft_tracker_auto_refresh: settings.tracker_list.auto_refresh,
            clear_proxy_password: false,
            editing_profile_id: None,
            draft_profile_kind: ProfileKindView::LocalManaged,
            profile_secret_updates: HashMap::new(),
            pending_profile_delete: None,
            clear_profile_rpc_secret: false,
            error: None,
        };

        let mut view = Self {
            theme,
            translator,
            settings,
            platform_capabilities,
            profiles,
            cores,
            aria2_version,
            update_enabled,
            update_portable,
            update_status,
            pending_settings_save_source,
            pending_tracker_refresh,
            settings_page,
            settings_inputs: SettingsInputs {
                directory,
                profile_name,
                profile_executable,
                profile_endpoint,
                profile_secret,
                all_proxy,
                http_proxy,
                https_proxy,
                ftp_proxy,
                no_proxy,
                proxy_username,
                proxy_password,
                download_limit,
                upload_limit,
                max_concurrent,
                max_connection,
                split,
                min_split_size,
                tracker_custom_url,
                tracker_list_text,
                category_name,
                category_extensions,
            },
            settings_scroll: ScrollHandle::new(),
            core_setup_panel,
            focus_handle: cx.focus_handle(),
            nav_focus_handles: std::array::from_fn(|_| cx.focus_handle()),
            next_draft_profile_seq: 1,
            _core_setup_sub: core_setup_sub,
            _input_subs: input_subs,
        };
        view.sync_inputs_from_settings(window, cx);
        view
    }

    pub(crate) fn t(&self, id: &str) -> String {
        self.translator.t(id)
    }

    pub(super) fn t_args(&self, id: &str, pairs: &[(&str, FluentValue<'_>)]) -> String {
        let mut args = FluentArgs::new();
        for (key, value) in pairs {
            args.set(*key, value.clone());
        }
        self.translator.t_args(id, Some(&args))
    }

    pub(super) fn te(&self, error: &OperationErrorView) -> String {
        error.localized_summary(&self.translator)
    }

    pub(super) fn is_pending(&self) -> bool {
        self.pending_settings_save_source.is_some()
    }

    pub(super) fn allocate_draft_profile_seq(&mut self) -> u64 {
        let seq = self.next_draft_profile_seq;
        self.next_draft_profile_seq = self.next_draft_profile_seq.saturating_add(1);
        seq
    }

    pub(super) fn sync_inputs_from_settings(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let download_directory = self.settings.download_directory.clone();
        self.settings_inputs.directory.update(cx, |input, cx| {
            input.set_value(download_directory, window, cx);
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
        self.sync_speed_limit_fields_from_settings(Some(window), cx);
        self.sync_transfer_policy_fields_from_settings(Some(window), cx);
        self.sync_tracker_list_fields_from_settings(Some(window), cx);
    }

    pub(super) fn refresh_placeholders(&self, window: &mut Window, cx: &mut Context<Self>) {
        let localized_inputs: [(&Entity<InputState>, &'static str); 8] = [
            (
                &self.settings_inputs.profile_name,
                "settings-profile-name-placeholder",
            ),
            (
                &self.settings_inputs.profile_executable,
                "settings-profile-executable-placeholder",
            ),
            (
                &self.settings_inputs.profile_secret,
                "settings-profile-secret-placeholder",
            ),
            (
                &self.settings_inputs.proxy_username,
                "settings-proxy-username-placeholder",
            ),
            (
                &self.settings_inputs.proxy_password,
                "settings-proxy-password-placeholder",
            ),
            (
                &self.settings_inputs.download_limit,
                "input-download-limit-placeholder",
            ),
            (
                &self.settings_inputs.upload_limit,
                "input-upload-limit-placeholder",
            ),
            (
                &self.settings_inputs.category_name,
                "settings-category-name-placeholder",
            ),
        ];
        for (entity, key) in localized_inputs {
            let placeholder = self.t(key);
            entity.update(cx, |input, cx| {
                input.set_placeholder(placeholder, window, cx);
            });
        }
    }

    pub(super) fn sync_context(
        &mut self,
        panel_cx: SettingsPanelContext<'_>,
        window: Option<&mut Window>,
        cx: &mut Context<Self>,
    ) {
        let SettingsPanelContext {
            theme,
            translator,
            platform_capabilities,
            profiles,
            cores,
            core_setup,
            aria2_version,
            update_enabled,
            update_portable,
            update_status,
            pending_settings_save_source,
            pending_tracker_refresh,
        } = panel_cx;
        self.theme = theme;
        self.translator = translator.clone();
        self.platform_capabilities = platform_capabilities;
        self.profiles = profiles;
        self.cores = cores;
        self.aria2_version = aria2_version;
        self.update_enabled = update_enabled;
        self.update_portable = update_portable;
        self.update_status = update_status;
        self.pending_settings_save_source = pending_settings_save_source;
        self.pending_tracker_refresh = pending_tracker_refresh;
        self.core_setup_panel.update(cx, |panel, cx| {
            panel.sync(theme, translator, core_setup, cx);
        });
        if let Some(window) = window {
            self.refresh_placeholders(window, cx);
        }
        cx.notify();
    }

    pub(super) fn sync_core_setup(
        &mut self,
        theme: Theme,
        translator: Translator,
        core_setup: &CoreSetupState,
        cx: &mut Context<Self>,
    ) {
        self.theme = theme;
        self.translator = translator.clone();
        self.core_setup_panel.update(cx, |panel, cx| {
            panel.sync(theme, translator, core_setup, cx);
        });
        cx.notify();
    }

    pub(super) fn sync_power_draft(
        &mut self,
        action_after_done: CompletionPowerActionView,
        keep_awake_while_active: bool,
        cx: &mut Context<Self>,
    ) {
        self.settings.platform.action_after_done = action_after_done;
        self.settings.platform.keep_awake_while_active = keep_awake_while_active;
        self.settings_page.draft_platform.action_after_done = action_after_done;
        self.settings_page.draft_platform.keep_awake_while_active = keep_awake_while_active;
        cx.notify();
    }

    pub(super) fn apply_settings(
        &mut self,
        settings: SettingsView,
        theme: Theme,
        translator: Translator,
        window: Option<&mut Window>,
        cx: &mut Context<Self>,
    ) {
        self.theme = theme;
        self.translator = translator;
        self.settings = settings.clone();
        self.settings_page.draft_color_scheme = settings.color_scheme;
        self.settings_page.draft_categories = settings.categories.clone();
        self.settings_page.draft_category_edit_index = None;
        self.settings_page.draft_language = settings.language;
        self.settings_page.draft_proxy_mode = settings.download_proxy.mode;
        self.settings_page.draft_file_allocation = settings.transfer_policy.file_allocation;
        self.settings_page.draft_check_certificate = settings.download_proxy.check_certificate;
        self.settings_page.draft_check_integrity = settings.transfer_policy.check_integrity;
        self.settings_page.draft_notifications = settings.notifications;
        self.settings_page.draft_platform = settings.platform;
        self.settings_page.draft_browser_bridge = settings.browser_bridge;
        self.settings_page.draft_tracker_enabled = settings.tracker_list.enabled;
        self.settings_page.draft_tracker_source = settings.tracker_list.source;
        self.settings_page.draft_tracker_auto_refresh = settings.tracker_list.auto_refresh;
        if let Some(window) = window {
            self.refresh_placeholders(window, cx);
            self.sync_inputs_from_settings(window, cx);
        }
        cx.notify();
    }

    pub(super) fn request_settings_save(
        &mut self,
        settings: SettingsView,
        proxy_password: ProxyPasswordUpdateView,
        source: SettingsSaveSource,
        cx: &mut Context<Self>,
    ) {
        if self.is_pending() {
            return;
        }
        self.settings_page.error = None;
        self.pending_settings_save_source = Some(source);
        cx.emit(SettingsPanelEvent::SaveSettings {
            settings: Box::new(settings),
            proxy_password,
            source,
        });
        cx.notify();
    }

    pub(crate) fn select_settings_category(
        &mut self,
        category: SettingsCategory,
        cx: &mut Context<Self>,
    ) {
        if self.settings_page.active_category != category {
            self.settings_page.active_category = category;
            self.settings_scroll.set_offset(gpui::Point::default());
            cx.notify();
        }
    }

    pub(crate) fn submit_active_settings_category(&mut self, cx: &mut Context<Self>) {
        if self.is_pending() {
            return;
        }
        match self.settings_page.active_category {
            SettingsCategory::General => self.submit_settings(cx),
            SettingsCategory::Network => self.submit_proxy_settings(cx),
            SettingsCategory::Transfers => self.submit_transfers(cx),
            SettingsCategory::Notifications => self.submit_notifications(cx),
            SettingsCategory::System => self.submit_platform(cx),
            SettingsCategory::Profiles | SettingsCategory::Engine | SettingsCategory::About => {}
        }
    }

    pub(crate) fn submit_settings(&mut self, cx: &mut Context<Self>) {
        if self.is_pending() {
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

    pub(crate) fn pick_path_for_field(
        &mut self,
        target: PathPickTarget,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (files, directories, prompt) = match &target {
            PathPickTarget::CategoryDirectory { .. } => {
                (false, true, self.t("settings-pick-download-directory"))
            }
            PathPickTarget::ProfileExecutable => (
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
        match target {
            PathPickTarget::CategoryDirectory { category_id } => {
                if let Some(index) = self
                    .settings_page
                    .draft_categories
                    .iter()
                    .position(|category| category.id == category_id)
                {
                    if let Some(category) = self.settings_page.draft_categories.get_mut(index) {
                        category.directory = display.clone();
                    }
                    if self.settings_page.draft_category_edit_index == Some(index) {
                        self.settings_inputs.directory.update(cx, |input, cx| {
                            input.set_value(display, window, cx);
                        });
                    }
                }
            }
            PathPickTarget::ProfileExecutable => {
                let field = self.settings_inputs.profile_executable.clone();
                field.update(cx, |input, cx| input.set_value(display, window, cx));
                window.focus(&field.focus_handle(cx), cx);
            }
        }
        self.settings_page.error = None;
        cx.notify();
    }

    pub(super) fn settings_path_field_row(
        input: &Entity<InputState>,
        browse_id: impl Into<ElementId>,
        browse_label: impl Into<SharedString>,
        browse_aria: impl Into<SharedString>,
        target: PathPickTarget,
        cx: &mut Context<Self>,
    ) -> Div {
        let browse_label = browse_label.into();
        let browse_aria = browse_aria.into();
        div()
            .flex()
            .items_center()
            .gap_2()
            .child(div().flex_1().min_w_0().child(Input::new(input)))
            .child(
                KitButton::new(browse_id)
                    .icon(gpui_kit::component::Icon::new(IconName::FolderDown))
                    .label(browse_label)
                    .accessibility_label(browse_aria)
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.pick_path_for_field(target.clone(), window, cx);
                    })),
            )
    }

    fn render_settings_nav(&mut self, cx: &mut Context<Self>) -> Stateful<Div> {
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
        for (index, category) in SettingsCategory::ALL.into_iter().enumerate() {
            let is_active = category == active;
            let prev_index =
                (index + SettingsCategory::ALL.len() - 1) % SettingsCategory::ALL.len();
            let next_index = (index + 1) % SettingsCategory::ALL.len();
            let item = div()
                .id(SharedString::from(format!("nav-{}", category.label())))
                .role(Role::Tab)
                .aria_label(self.t(category.message_key()))
                .aria_selected(is_active)
                .track_focus(&self.nav_focus_handles[index])
                .tab_stop(is_active)
                .border_1()
                .border_color(gpui::transparent_black())
                .focus_visible(|style| style.border_color(colors.focus_ring))
                .flex()
                .items_center()
                .gap_3()
                .mx_2()
                .px_2p5()
                .py_2()
                .rounded_md()
                .text_sm()
                .text_color(if is_active {
                    colors.text_primary
                } else {
                    colors.text_secondary
                })
                .when(is_active, |el| {
                    el.font_weight(FontWeight::MEDIUM)
                        .bg(with_alpha(colors.accent, 0.12))
                })
                .when(!is_active, |el| {
                    el.hover(|style| style.bg(colors.surface_hover))
                        .active(|style| style.bg(colors.surface_active))
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
                .on_key_down(
                    cx.listener(move |this, event: &gpui::KeyDownEvent, window, cx| {
                        if is_activation_key(event) {
                            cx.stop_propagation();
                            this.select_settings_category(category, cx);
                        } else {
                            let target_index = match event.keystroke.key.as_str() {
                                "up" => Some(prev_index),
                                "down" => Some(next_index),
                                "home" => Some(0),
                                "end" => Some(SettingsCategory::ALL.len() - 1),
                                _ => None,
                            };
                            if let Some(target_index) = target_index {
                                cx.stop_propagation();
                                this.select_settings_category(
                                    SettingsCategory::ALL[target_index],
                                    cx,
                                );
                                window.focus(&this.nav_focus_handles[target_index], cx);
                            }
                        }
                    }),
                )
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.select_settings_category(category, cx);
                    window.focus(&this.nav_focus_handles[index], cx);
                }));
            nav = nav.child(item);
        }
        nav
    }

    pub(crate) fn render_settings_footer(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let colors = self.theme.colors;
        let pending = self.is_pending();
        let active_category = self.settings_page.active_category;
        let error = self.settings_page.error.clone();

        let (dirty, saving) = match active_category {
            SettingsCategory::Profiles | SettingsCategory::Engine | SettingsCategory::About => {
                return div().into_any_element();
            }
            SettingsCategory::General => {
                let dirty = self.preview_draft_categories(cx) != self.settings.categories;
                let saving =
                    self.pending_settings_save_source == Some(SettingsSaveSource::Categories);
                (dirty, saving)
            }
            SettingsCategory::Network => {
                let (proxy_draft, password_update) = self.read_proxy_draft(cx);
                let dirty = proxy_draft != self.settings.download_proxy
                    || !matches!(password_update, ProxyPasswordUpdateView::Unchanged);
                let saving = self.pending_settings_save_source == Some(SettingsSaveSource::Proxy);
                (dirty, saving)
            }
            SettingsCategory::Transfers => {
                let speed_limit_draft = self.read_speed_limit_draft(cx);
                let transfer_policy_draft = self.read_transfer_policy_draft(cx);
                let tracker_list_draft = self.read_tracker_list_draft(cx);
                let dirty = speed_limit_draft != self.settings.speed_limits
                    || transfer_policy_draft != self.settings.transfer_policy
                    || tracker_list_draft != self.settings.tracker_list;
                let saving = matches!(
                    self.pending_settings_save_source,
                    Some(
                        SettingsSaveSource::SpeedLimit
                            | SettingsSaveSource::TransferPolicy
                            | SettingsSaveSource::Transfers
                    )
                ) || self.pending_tracker_refresh;
                (dirty, saving)
            }
            SettingsCategory::Notifications => {
                let dirty = self.settings_page.draft_notifications != self.settings.notifications;
                let saving =
                    self.pending_settings_save_source == Some(SettingsSaveSource::Notifications);
                (dirty, saving)
            }
            SettingsCategory::System => {
                let dirty = self.settings_page.draft_platform != self.settings.platform
                    || self.settings_page.draft_browser_bridge != self.settings.browser_bridge;
                let saving =
                    self.pending_settings_save_source == Some(SettingsSaveSource::Platform);
                (dirty, saving)
            }
        };

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
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.submit_active_settings_category(cx);
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

impl Render for SettingsPanelView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = self.theme.colors;
        let active_category = self.settings_page.active_category;
        let reduced_motion = crate::accessibility::is_reduced_motion(cx);
        let category_body = animate_fade_settle(
            div()
                .id(SharedString::from(format!(
                    "settings-category-{}",
                    active_category.as_str()
                )))
                .w_full()
                .max_w(px(760.0))
                .flex()
                .flex_col()
                .gap_6()
                .child(match active_category {
                    SettingsCategory::General => {
                        self.render_settings_general(cx).into_any_element()
                    }
                    SettingsCategory::Profiles => {
                        self.render_settings_profiles(cx).into_any_element()
                    }
                    SettingsCategory::Engine => self.render_settings_engine(cx).into_any_element(),
                    SettingsCategory::Network => {
                        self.render_settings_network(cx).into_any_element()
                    }
                    SettingsCategory::Transfers => {
                        self.render_settings_transfers(cx).into_any_element()
                    }
                    SettingsCategory::Notifications => {
                        self.render_settings_notifications(cx).into_any_element()
                    }
                    SettingsCategory::System => self.render_settings_system(cx).into_any_element(),
                    SettingsCategory::About => self.render_settings_about(cx).into_any_element(),
                }),
            SharedString::from(format!(
                "settings-category-enter:{}",
                active_category.as_str()
            )),
            px(6.0),
            150,
            reduced_motion,
        );
        div()
            .id("settings-page")
            .key_context("SettingsPage")
            .track_focus(&self.focus_handle)
            .on_action(cx.listener(|_this, _: &CloseSettings, _window, cx| {
                cx.emit(SettingsPanelEvent::Close);
            }))
            .on_action(cx.listener(|this, _: &SaveSettings, _window, cx| {
                this.submit_active_settings_category(cx);
            }))
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
            .child(animate_fade_settle(
                div()
                    .id("settings-page-body")
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
                                            .child(category_body),
                                    )
                                    .vertical_scrollbar(&self.settings_scroll),
                            )
                            .child(self.render_settings_footer(cx)),
                    ),
                "settings-page-enter",
                px(4.0),
                150,
                reduced_motion,
            ))
    }
}

impl AppShell {
    pub(super) fn settings_panel_entity(&self) -> Option<Entity<SettingsPanelView>> {
        self.settings_panel.as_ref().map(|(panel, _)| panel.clone())
    }

    #[cfg(test)]
    pub(super) fn settings_panel(&self) -> Option<&Entity<SettingsPanelView>> {
        self.settings_panel.as_ref().map(|(panel, _)| panel)
    }

    pub(super) fn leave_settings_page(&mut self) {
        self.page = AppPage::Downloads;
        self.settings_panel = None;
    }

    fn settings_panel_context<'a>(
        &self,
        core_setup: &'a CoreSetupState,
    ) -> SettingsPanelContext<'a> {
        SettingsPanelContext {
            theme: self.theme,
            translator: self.translator.clone(),
            platform_capabilities: self.platform_capabilities,
            profiles: self.profiles.clone(),
            cores: self.cores.clone(),
            core_setup,
            aria2_version: self.snapshot.capabilities.version.clone(),
            update_enabled: self.update_enabled,
            update_portable: self.update_portable,
            update_status: self.update_status.clone(),
            pending_settings_save_source: self.pending_settings_save.as_ref().map(|p| p.source),
            pending_tracker_refresh: self.pending_tracker_refresh.is_some(),
        }
    }

    pub(super) fn sync_settings_panel(&self, window: Option<&mut Window>, cx: &mut Context<Self>) {
        let Some((panel, _)) = &self.settings_panel else {
            return;
        };
        let core_setup = self.core_setup.clone();
        let panel_cx = self.settings_panel_context(&core_setup);
        panel.update(cx, |panel, cx| {
            panel.sync_context(panel_cx, window, cx);
        });
    }

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
                if let Some(panel) = self.settings_panel_entity() {
                    panel.update(cx, |panel, cx| {
                        panel.settings_page.error = None;
                        panel.pending_settings_save_source = None;
                        if source == SettingsSaveSource::Proxy {
                            panel
                                .settings_inputs
                                .proxy_password
                                .update(cx, |input, cx| input.set_value("", window, cx));
                            panel.settings_page.clear_proxy_password = false;
                        }
                        cx.notify();
                    });
                }
                let message = match source {
                    SettingsSaveSource::Theme => self.t("notice-settings-appearance"),
                    SettingsSaveSource::Language => self.t("settings-language-saved"),
                    SettingsSaveSource::Categories => self.t("notice-settings-categories"),
                    SettingsSaveSource::Proxy => self.t("notice-settings-proxy"),
                    SettingsSaveSource::SpeedLimit => self.t("notice-settings-speed"),
                    SettingsSaveSource::TransferPolicy => self.t("notice-settings-transfer-policy"),
                    SettingsSaveSource::Transfers => self.t("notice-settings-transfers"),
                    SettingsSaveSource::Notifications => self.t("notice-settings-notifications"),
                    SettingsSaveSource::Platform => self.t("notice-settings-platform"),
                    SettingsSaveSource::Import => self.t("notice-settings-imported"),
                };
                self.show_notice(message, false, cx);
            }
            SettingsSaveOutcomeView::Failure(error) => {
                if source == SettingsSaveSource::Import {
                    if let Some(panel) = self.settings_panel_entity() {
                        panel.update(cx, |panel, cx| {
                            panel.pending_settings_save_source = None;
                            cx.notify();
                        });
                    }
                    self.show_notice(self.te(&error), true, cx);
                } else {
                    if let Some(panel) = self.settings_panel_entity() {
                        panel.update(cx, |panel, cx| {
                            panel.pending_settings_save_source = None;
                            panel.settings_page.error = Some(error);
                            cx.notify();
                        });
                    }
                    cx.notify();
                }
            }
        }
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
        self.set_language_runtime(settings.language);
        if let Some(window) = window.as_deref_mut() {
            self.refresh_input_placeholders(window, cx);
        }
        self.sync_active_modal_context(window.as_deref_mut(), cx);
        if let Some(panel) = self.settings_panel_entity() {
            let theme = self.theme;
            let translator = self.translator.clone();
            panel.update(cx, |panel, cx| {
                panel.apply_settings(settings, theme, translator, window, cx);
            });
        }
        self.sync_details_panel_context(cx);
        self.sync_core_setup_panels(cx);
        cx.notify();
    }

    pub(crate) fn toggle_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.page == AppPage::Settings {
            self.close_settings(window, cx);
        } else {
            self.open_settings(&OpenSettings, window, cx);
        }
    }

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
            if let Some(panel) = self.settings_panel_entity() {
                let focus = panel.update(cx, |panel, cx| {
                    panel.select_settings_category(category, cx);
                    panel.focus_handle(cx)
                });
                window.focus(&focus, cx);
            }
            cx.notify();
            return;
        }
        if self.active_modal.is_some() {
            return;
        }
        let previous_focus = window.focused(cx).map(|focus| focus.downgrade());
        let settings = self.settings.clone();
        let core_setup = self.core_setup.clone();
        let panel_cx = self.settings_panel_context(&core_setup);

        let panel = cx.new(|cx| {
            SettingsPanelView::new(category, previous_focus, settings, panel_cx, window, cx)
        });
        let sub = cx.subscribe_in(
            &panel,
            window,
            |this, _panel, event: &SettingsPanelEvent, window, cx| match event {
                SettingsPanelEvent::Close => {
                    this.close_settings(window, cx);
                }
                SettingsPanelEvent::SelectColorScheme(scheme) => {
                    this.select_color_scheme(*scheme, window, cx);
                }
                SettingsPanelEvent::SelectLanguage(language) => {
                    this.select_language(*language, window, cx);
                }
                SettingsPanelEvent::SaveSettings {
                    settings,
                    proxy_password,
                    source,
                } => {
                    this.request_settings_save(
                        (**settings).clone(),
                        proxy_password.clone(),
                        *source,
                        cx,
                    );
                }
                SettingsPanelEvent::ExportSettings => this.export_settings_file(window, cx),
                SettingsPanelEvent::ImportSettings => this.import_settings_file(window, cx),
                SettingsPanelEvent::ExportDiagnostics => this.export_diagnostics(window, cx),
                SettingsPanelEvent::RefreshTrackerList { draft } => {
                    this.request_tracker_list_refresh_with_draft(draft.clone(), cx);
                }
                SettingsPanelEvent::SwitchProfile(profile_id) => {
                    this.request_switch_profile(profile_id.clone(), cx);
                }
                SettingsPanelEvent::SaveProfileCatalog {
                    catalog,
                    secret_updates,
                } => {
                    this.request_save_profile_catalog_with_secrets(
                        catalog.clone(),
                        secret_updates.clone(),
                        cx,
                    );
                }
                SettingsPanelEvent::ProfileCatalogChanged(catalog) => {
                    this.profiles = catalog.clone();
                    cx.notify();
                }
                SettingsPanelEvent::CoreCommand(command) => {
                    this.request_core_command(command.clone(), cx);
                }
                SettingsPanelEvent::CoreDiscovery => this.request_core_discovery(cx),
                SettingsPanelEvent::CoreDownload => this.request_core_download(cx),
                SettingsPanelEvent::CheckForUpdates => this.request_update_check(cx),
                SettingsPanelEvent::DownloadUpdate => this.request_update_download(cx),
                SettingsPanelEvent::InstallUpdate => this.request_update_install(cx),
                SettingsPanelEvent::Notice { message, is_error } => {
                    this.show_notice(message.clone(), *is_error, cx);
                }
            },
        );
        let panel_focus = panel.read(cx).focus_handle(cx);
        self.page = AppPage::Settings;
        self.settings_panel = Some((panel, sub));
        self.clear_details_panel();
        self.sync_details_sheet(window, cx);
        self.active_status_popover = None;
        window.focus(&panel_focus, cx);
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
        let previous_focus = self.settings_panel.as_ref().and_then(|(panel, _)| {
            panel.update(cx, |panel, _| panel.settings_page.previous_focus.take())
        });
        self.leave_settings_page();
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
        if self.page != AppPage::Settings || self.pending_settings_save.is_some() {
            return;
        }
        if let Some(panel) = self.settings_panel_entity() {
            panel.update(cx, |panel, cx| {
                panel.submit_active_settings_category(cx);
            });
        }
    }

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
        if let Some(panel) = self.settings_panel_entity() {
            panel.update(cx, |panel, cx| {
                panel.pending_settings_save_source = Some(source);
                panel.settings_page.error = None;
                cx.notify();
            });
        }
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
