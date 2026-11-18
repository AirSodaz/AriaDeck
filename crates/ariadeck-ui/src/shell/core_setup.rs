//! Get-aria2 panel child entity: discovery, verified download, and manual import (B4).
//!
//! [`CoreSetupPanelView`] is mounted by two hosts — the first-run onboarding
//! dialog and Settings → Engine — and they share it deliberately. Onboarding is
//! a *time* at which core setup is offered, not a separate feature, so anything
//! the guide can do stays reachable afterwards without a second implementation
//! drifting out of sync. Each host owns its own lazily created
//! `Entity<CoreSetupPanelView>` (including its own `core_path: Entity<InputState>`),
//! eliminating any shared global input entity.
//!
//! Nothing here reaches the network on its own. Discovery only reads the local
//! filesystem, and the download is a pinned, checksum-verified artifact fetched
//! solely on a button press.

use super::*;

/// Discovery + download state owned by `AppShell` and mirrored into mounted
/// [`CoreSetupPanelView`] instances.
#[derive(Clone, Default)]
pub(crate) struct CoreSetupState {
    pub(crate) discovered: Vec<DiscoveredCoreView>,
    pub(crate) offer: CoreDownloadOfferView,
    /// A local scan is in flight.
    pub(crate) scanning: bool,
    /// At least one scan has completed, so "nothing found" is a real answer.
    pub(crate) scanned: bool,
    /// The verified download is in flight.
    pub(crate) downloading: bool,
}

pub(crate) enum CoreSetupPanelEvent {
    Discover,
    Download,
    Import(String),
    Link(String),
    Notice { message: String, is_error: bool },
}

pub(crate) struct CoreSetupPanelView {
    id_prefix: &'static str,
    theme: Theme,
    translator: Translator,
    discovered: Vec<DiscoveredCoreView>,
    offer: CoreDownloadOfferView,
    scanning: bool,
    scanned: bool,
    downloading: bool,
    pub(crate) core_path: Entity<InputState>,
    _path_sub: Subscription,
}

impl gpui::EventEmitter<CoreSetupPanelEvent> for CoreSetupPanelView {}

impl CoreSetupPanelView {
    pub(crate) fn new(
        id_prefix: &'static str,
        theme: Theme,
        translator: Translator,
        state: &CoreSetupState,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let placeholder = if cfg!(windows) {
            "C:\\path\\to\\aria2c.exe"
        } else {
            "/usr/bin/aria2c"
        };
        let core_path = cx.new(|cx| InputState::new(window, cx).placeholder(placeholder));
        let path_sub = cx.subscribe(&core_path, |_this, _, _event: &InputEvent, cx| {
            cx.notify();
        });
        Self {
            id_prefix,
            theme,
            translator,
            discovered: state.discovered.clone(),
            offer: state.offer.clone(),
            scanning: state.scanning,
            scanned: state.scanned,
            downloading: state.downloading,
            core_path,
            _path_sub: path_sub,
        }
    }

    pub(crate) fn sync(
        &mut self,
        theme: Theme,
        translator: Translator,
        state: &CoreSetupState,
        cx: &mut Context<Self>,
    ) {
        self.theme = theme;
        self.translator = translator;
        self.discovered = state.discovered.clone();
        self.offer = state.offer.clone();
        self.scanning = state.scanning;
        self.scanned = state.scanned;
        self.downloading = state.downloading;
        cx.notify();
    }

    fn t(&self, id: &str) -> String {
        self.translator.t(id)
    }

    fn t_args(&self, id: &str, args: &[(&str, FluentValue<'_>)]) -> String {
        let mut fluent_args = FluentArgs::new();
        for (key, value) in args {
            fluent_args.set(*key, value.clone());
        }
        self.translator.t_args(id, Some(&fluent_args))
    }

    fn browse_core_path(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let prompt = if cfg!(windows) {
            self.t("settings-pick-aria2c-windows")
        } else {
            self.t("settings-pick-aria2c")
        };
        let selected = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some(prompt.into()),
        });
        cx.spawn_in(window, async move |this, cx| {
            let selected = selected.await;
            let _ = this.update_in(cx, |this, window, cx| match selected {
                Ok(Ok(Some(paths))) => {
                    if let Some(path) = paths.into_iter().next() {
                        let value = path.to_string_lossy().into_owned();
                        this.core_path.update(cx, |input, cx| {
                            input.set_value(value, window, cx);
                        });
                        cx.notify();
                    }
                }
                Ok(Ok(None)) => {}
                Ok(Err(_)) => {
                    cx.emit(CoreSetupPanelEvent::Notice {
                        message: this.t("error-settings-path-picker-failed"),
                        is_error: true,
                    });
                }
                Err(_) => {
                    cx.emit(CoreSetupPanelEvent::Notice {
                        message: this.t("error-settings-path-picker-closed"),
                        is_error: true,
                    });
                }
            });
        })
        .detach();
    }

    fn submit_import_from_input(&mut self, cx: &mut Context<Self>) {
        let path = self.core_path.read(cx).value().trim().to_owned();
        if path.is_empty() {
            cx.emit(CoreSetupPanelEvent::Notice {
                message: self.t("notice-profile-enter-aria2c"),
                is_error: true,
            });
            return;
        }
        cx.emit(CoreSetupPanelEvent::Import(path));
    }

    fn submit_link_from_input(&mut self, cx: &mut Context<Self>) {
        let path = self.core_path.read(cx).value().trim().to_owned();
        if path.is_empty() {
            cx.emit(CoreSetupPanelEvent::Notice {
                message: self.t("notice-profile-enter-aria2c"),
                is_error: true,
            });
            return;
        }
        cx.emit(CoreSetupPanelEvent::Link(path));
    }

    fn render_discovered(&self, cx: &mut Context<Self>) -> Div {
        let colors = self.theme.colors;
        let id_prefix = self.id_prefix;
        let scanning = self.scanning;
        let scanned = self.scanned;
        let discovered = self.discovered.clone();
        let summary = if scanning {
            self.t("core-setup-scanning")
        } else if discovered.is_empty() {
            if scanned {
                self.t("core-setup-none-found")
            } else {
                self.t("core-setup-not-scanned")
            }
        } else {
            self.t_args(
                "core-setup-found-summary",
                &[(
                    "count",
                    FluentValue::from(i64::try_from(discovered.len()).unwrap_or(i64::MAX)),
                )],
            )
        };
        div()
            .flex()
            .flex_col()
            .gap_2()
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
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(colors.text_primary)
                            .child(self.t("core-setup-found-title")),
                    )
                    .child(
                        KitButton::new(SharedString::from(format!("{id_prefix}-rescan-cores")))
                            .icon(gpui_kit::component::Icon::new(IconName::ScanSearch))
                            .label(self.t("core-setup-rescan"))
                            .accessibility_label(self.t("core-setup-rescan-aria"))
                            .disabled(scanning)
                            .on_click(cx.listener(|_this, _, _, cx| {
                                cx.emit(CoreSetupPanelEvent::Discover);
                            })),
                    ),
            )
            .child(div().text_xs().text_color(colors.text_muted).child(summary))
            .children(discovered.into_iter().map(|core| {
                let import_path = core.path.clone();
                let link_path = core.path.clone();
                let already_registered = core.already_registered;
                let row_id = stable_id(&format!("{id_prefix}-discovered-core"), &core.path);
                let import_id = stable_id(&format!("{id_prefix}-import-discovered"), &core.path);
                let link_id = stable_id(&format!("{id_prefix}-link-discovered"), &core.path);
                div()
                    .id(row_id)
                    .flex()
                    .flex_col()
                    .gap_2()
                    .px_3()
                    .py_2()
                    .rounded_md()
                    .border_1()
                    .border_color(colors.border)
                    .bg(colors.surface)
                    .child(
                        div()
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
                                        &[("version", FluentValue::from(core.version.as_str()))],
                                    )),
                            )
                            .child({
                                let mut meta = self.t(core.origin.message_key());
                                if already_registered {
                                    meta.push_str(" · ");
                                    meta.push_str(&self.t("core-setup-already-registered"));
                                }
                                div().text_xs().text_color(colors.text_muted).child(meta)
                            })
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(colors.text_muted)
                                    .child(core.path.clone()),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_wrap()
                            .gap_2()
                            .child(
                                KitButton::new(import_id)
                                    .primary()
                                    .label(self.t("settings-core-import"))
                                    .accessibility_label(self.t_args(
                                        "core-setup-import-aria",
                                        &[("path", FluentValue::from(core.path.as_str()))],
                                    ))
                                    .disabled(already_registered)
                                    .on_click(cx.listener(move |_this, _, _, cx| {
                                        cx.emit(CoreSetupPanelEvent::Import(import_path.clone()));
                                    })),
                            )
                            .child(
                                KitButton::new(link_id)
                                    .label(self.t("settings-core-link"))
                                    .accessibility_label(self.t_args(
                                        "core-setup-link-aria",
                                        &[("path", FluentValue::from(core.path.as_str()))],
                                    ))
                                    .disabled(already_registered)
                                    .on_click(cx.listener(move |_this, _, _, cx| {
                                        cx.emit(CoreSetupPanelEvent::Link(link_path.clone()));
                                    })),
                            ),
                    )
            }))
    }

    fn render_download(&self, cx: &mut Context<Self>) -> Div {
        let colors = self.theme.colors;
        let id_prefix = self.id_prefix;
        let offer = self.offer.clone();
        let downloading = self.downloading;
        let body = if offer.available {
            let mut lines = vec![
                self.t_args(
                    "core-setup-download-description",
                    &[
                        ("version", FluentValue::from(offer.version.as_str())),
                        ("target", FluentValue::from(offer.target.as_str())),
                    ],
                ),
                offer.url.clone(),
                self.t("core-setup-download-verified"),
                self.t("core-setup-download-consent"),
            ];
            if offer.emulated {
                lines.push(self.t("core-setup-download-emulated"));
            }
            lines
        } else {
            vec![self.t("core-setup-download-unavailable")]
        };
        div()
            .flex()
            .flex_col()
            .gap_2()
            .pt_3()
            .border_t_1()
            .border_color(colors.border)
            .child(
                div()
                    .text_xs()
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(colors.text_primary)
                    .child(self.t("core-setup-download-title")),
            )
            .children(
                body.into_iter()
                    .map(|line| div().text_xs().text_color(colors.text_muted).child(line)),
            )
            .when(offer.available, |element| {
                element.child(
                    div().flex().child(
                        KitButton::new(SharedString::from(format!("{id_prefix}-download-core")))
                            .primary()
                            .icon(gpui_kit::component::Icon::new(IconName::FolderDown))
                            .label(if downloading {
                                self.t("core-setup-download-running")
                            } else {
                                self.t("core-setup-download-action")
                            })
                            .accessibility_label(self.t_args(
                                "core-setup-download-aria",
                                &[("version", FluentValue::from(offer.version.as_str()))],
                            ))
                            .disabled(downloading)
                            .on_click(cx.listener(|_this, _, _, cx| {
                                cx.emit(CoreSetupPanelEvent::Download);
                            })),
                    ),
                )
            })
    }
}

impl Render for CoreSetupPanelView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = self.theme.colors;
        let id_prefix = self.id_prefix;
        div()
            .flex()
            .flex_col()
            .gap_3()
            .child(self.render_discovered(cx))
            .child(self.render_download(cx))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child(
                        div()
                            .text_xs()
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(colors.text_primary)
                            .child(self.t("core-setup-manual-title")),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(colors.text_muted)
                            .child(self.t("core-setup-manual-description")),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(div().flex_1().min_w_0().child(Input::new(&self.core_path)))
                            .child(
                                KitButton::new(SharedString::from(format!(
                                    "{id_prefix}-browse-core-path"
                                )))
                                .icon(gpui_kit::component::Icon::new(IconName::FolderDown))
                                .label(self.t("button-browse"))
                                .accessibility_label(self.t("settings-core-browse-aria"))
                                .on_click(cx.listener(
                                    |this, _, window, cx| {
                                        this.browse_core_path(window, cx);
                                    },
                                )),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_wrap()
                            .gap_2()
                            .child(
                                KitButton::new(SharedString::from(format!(
                                    "{id_prefix}-import-core"
                                )))
                                .primary()
                                .label(self.t("settings-core-import"))
                                .accessibility_label(self.t("settings-core-import-aria"))
                                .on_click(cx.listener(
                                    |this, _, _, cx| {
                                        this.submit_import_from_input(cx);
                                    },
                                )),
                            )
                            .child(
                                KitButton::new(SharedString::from(format!(
                                    "{id_prefix}-link-core"
                                )))
                                .label(self.t("settings-core-link"))
                                .accessibility_label(self.t("settings-core-link-aria"))
                                .on_click(cx.listener(
                                    |this, _, _, cx| {
                                        this.submit_link_from_input(cx);
                                    },
                                )),
                            ),
                    ),
            )
    }
}

impl AppShell {
    pub(crate) fn create_core_setup_panel(
        &self,
        id_prefix: &'static str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> (Entity<CoreSetupPanelView>, Subscription) {
        let theme = self.theme;
        let translator = self.translator.clone();
        let state = self.core_setup.clone();
        let panel =
            cx.new(|cx| CoreSetupPanelView::new(id_prefix, theme, translator, &state, window, cx));
        let sub = cx.subscribe(&panel, |this, _, event, cx| match event {
            CoreSetupPanelEvent::Discover => this.request_core_discovery(cx),
            CoreSetupPanelEvent::Download => this.request_core_download(cx),
            CoreSetupPanelEvent::Import(path) => {
                this.request_core_command(CoreCommandView::Import { path: path.clone() }, cx);
            }
            CoreSetupPanelEvent::Link(path) => {
                this.request_core_command(CoreCommandView::Link { path: path.clone() }, cx);
            }
            CoreSetupPanelEvent::Notice { message, is_error } => {
                this.show_notice(message.clone(), *is_error, cx);
            }
        });
        (panel, sub)
    }

    pub(crate) fn sync_core_setup_panels(&self, cx: &mut Context<Self>) {
        let theme = self.theme;
        let translator = self.translator.clone();
        let state = self.core_setup.clone();
        if let Some(ActiveModal::CoreSetupOnboarding { panel, .. }) = &self.active_modal {
            panel.update(cx, |panel, cx| {
                panel.sync(theme, translator.clone(), &state, cx);
            });
        }
        if let Some((panel, _)) = &self.settings_panel {
            panel.update(cx, |panel, cx| {
                panel.sync_core_setup(theme, translator, &state, cx);
            });
        }
    }

    /// Pinned download offer for this platform, published once at startup.
    pub fn set_core_download_offer(
        &mut self,
        offer: CoreDownloadOfferView,
        cx: &mut Context<Self>,
    ) {
        self.core_setup.offer = offer;
        self.sync_core_setup_panels(cx);
        cx.notify();
    }

    /// Scan this machine for existing `aria2c` binaries.
    pub fn request_core_discovery(&mut self, cx: &mut Context<Self>) {
        if self.core_setup.scanning {
            return;
        }
        let request_id = self.allocate_request_id();
        self.core_setup.scanning = true;
        self.sync_core_setup_panels(cx);
        cx.emit(AppShellEvent::CoreDiscoveryRequested { request_id });
        cx.notify();
    }

    pub fn set_core_discovery_result(
        &mut self,
        result: CoreDiscoveryResultView,
        cx: &mut Context<Self>,
    ) {
        self.core_setup.scanning = false;
        self.core_setup.scanned = true;
        self.core_setup.discovered = result.discovered;
        self.sync_core_setup_panels(cx);
        cx.notify();
    }

    /// Download the pinned, checksum-verified aria2 build for this platform.
    pub fn request_core_download(&mut self, cx: &mut Context<Self>) {
        if self.core_setup.downloading {
            return;
        }
        if !self.core_setup.offer.available {
            self.show_notice(self.t("notice-core-download-unavailable"), true, cx);
            return;
        }
        let request_id = self.allocate_request_id();
        self.core_setup.downloading = true;
        self.sync_core_setup_panels(cx);
        self.show_notice(self.t("notice-core-downloading"), false, cx);
        cx.emit(AppShellEvent::CoreDownloadRequested { request_id });
        cx.notify();
    }

    pub fn set_core_download_result(
        &mut self,
        result: CoreDownloadResultView,
        cx: &mut Context<Self>,
    ) {
        self.core_setup.downloading = false;
        self.sync_core_setup_panels(cx);
        match result.outcome {
            CoreCommandOutcomeView::Success => {
                self.cores = result.registry;
                self.sync_settings_panel(None, cx);
                let version = result.installed_version.unwrap_or_default();
                self.show_notice(
                    self.t_args(
                        "notice-core-download-installed",
                        &[("version", FluentValue::from(version.as_str()))],
                    ),
                    false,
                    cx,
                );
            }
            CoreCommandOutcomeView::Failure(error) => {
                self.show_notice(self.te(&error), true, cx);
            }
        }
        cx.notify();
    }

    // --- first-run guide ---

    #[must_use]
    pub fn core_setup_onboarding_open(&self) -> bool {
        matches!(
            self.active_modal,
            Some(ActiveModal::CoreSetupOnboarding { .. })
        )
    }

    /// Open the first-run guide, unless the user is already somewhere better.
    ///
    /// On the Settings page the guide is redundant: switching to Engine puts
    /// the user in front of the same controls. Another open modal wins
    /// outright; the dismissal flag stays unset, so an unshown guide is offered
    /// again later.
    pub fn open_core_setup_onboarding(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.core_setup_onboarding_open() {
            return;
        }
        if self.page == AppPage::Settings {
            if let Some(panel) = self.settings_panel_entity() {
                panel.update(cx, |panel, cx| {
                    panel.select_settings_category(SettingsCategory::Engine, cx);
                });
            }
            cx.notify();
            return;
        }
        if self.active_modal.is_some() {
            return;
        }
        let (panel, sub) = self.create_core_setup_panel("onboarding", window, cx);
        self.active_modal = Some(ActiveModal::CoreSetupOnboarding { panel, _sub: sub });
        // Re-scan on open so the list reflects an aria2 installed since launch.
        self.request_core_discovery(cx);
        self.sync_kit_modals(window, cx);
        cx.notify();
    }

    /// Close the guide and remember that the user has been asked (B4).
    pub fn dismiss_core_setup_onboarding(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.core_setup_onboarding_open() {
            return;
        }
        self.active_modal = None;
        self.sync_kit_modals(window, cx);
        if !has_kit_root(window) {
            window.focus(&self.focus_handle, cx);
        }
        cx.emit(AppShellEvent::CoreSetupOnboardingDismissed);
        cx.notify();
    }

    pub(crate) fn close_core_setup_onboarding_action(
        &mut self,
        _: &CloseCoreSetup,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.dismiss_core_setup_onboarding(window, cx);
    }

    /// Jump from the guide to Settings → Engine, where the same panel lives.
    pub(crate) fn open_engine_settings_from_onboarding(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.dismiss_core_setup_onboarding(window, cx);
        self.open_settings_page(SettingsCategory::Engine, window, cx);
    }
}
