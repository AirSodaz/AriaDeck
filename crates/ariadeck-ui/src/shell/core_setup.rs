//! Get-aria2 panel: discovery, verified download, and manual import (B4).
//!
//! [`AppShell::render_core_setup_panel`] is rendered by two callers — the
//! first-run onboarding dialog and Settings → Engine — and they share it
//! deliberately. Onboarding is a *time* at which core setup is offered, not a
//! separate feature, so anything the guide can do stays reachable afterwards
//! without a second implementation drifting out of sync.
//!
//! Nothing here reaches the network on its own. Discovery only reads the local
//! filesystem, and the download is a pinned, checksum-verified artifact fetched
//! solely on a button press.

use super::*;

/// Discovery + download state shared by the panel's two hosts.
#[derive(Default)]
pub(crate) struct CoreSetupState {
    pub(crate) discovered: Vec<DiscoveredCoreView>,
    pub(crate) offer: CoreDownloadOfferView,
    /// A local scan is in flight.
    pub(crate) scanning: bool,
    /// At least one scan has completed, so "nothing found" is a real answer.
    pub(crate) scanned: bool,
    /// The verified download is in flight.
    pub(crate) downloading: bool,
    /// The first-run guide is on screen.
    pub(crate) onboarding_open: bool,
    pub(crate) onboarding_previous_focus: Option<WeakFocusHandle>,
}

impl AppShell {
    /// Pinned download offer for this platform, published once at startup.
    pub fn set_core_download_offer(
        &mut self,
        offer: CoreDownloadOfferView,
        cx: &mut Context<Self>,
    ) {
        self.core_setup.offer = offer;
        cx.notify();
    }

    /// Scan this machine for existing `aria2c` binaries.
    pub fn request_core_discovery(&mut self, cx: &mut Context<Self>) {
        if self.core_setup.scanning {
            return;
        }
        let request_id = self.allocate_request_id();
        self.core_setup.scanning = true;
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
        match result.outcome {
            CoreCommandOutcomeView::Success => {
                self.cores = result.registry;
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
        self.core_setup.onboarding_open
    }

    /// Open the first-run guide, unless the user is already somewhere better.
    ///
    /// The guide and Settings → Engine render the same panel — the same path
    /// input entity included — so only one may be mounted per frame. On the
    /// Settings page the guide is redundant anyway: switching to Engine puts the
    /// user in front of the same controls. Another open modal wins outright; the
    /// dismissal flag stays unset, so an unshown guide is offered again later.
    pub fn open_core_setup_onboarding(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.core_setup.onboarding_open {
            return;
        }
        if self.page == AppPage::Settings {
            self.settings_page.active_category = SettingsCategory::Engine;
            cx.notify();
            return;
        }
        if self.add_dialog.open
            || self.output_name_dialog.is_some()
            || self.task_speed_limit_dialog.is_some()
            || self.task_options_dialog.is_some()
            || self.remove_confirmation.is_some()
            || self.batch_failure_details.is_some()
        {
            return;
        }
        self.core_setup.onboarding_open = true;
        self.core_setup.onboarding_previous_focus =
            window.focused(cx).map(|focus| focus.downgrade());
        // Re-scan on open so the list reflects an aria2 installed since launch.
        self.request_core_discovery(cx);
        window.focus(&self.core_setup_dialog_focus, cx);
        cx.notify();
    }

    /// Close the guide and remember that the user has been asked (B4).
    pub fn dismiss_core_setup_onboarding(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.core_setup.onboarding_open {
            return;
        }
        self.core_setup.onboarding_open = false;
        let previous_focus = self
            .core_setup
            .onboarding_previous_focus
            .take()
            .and_then(|focus| focus.upgrade());
        if let Some(focus) = previous_focus {
            window.focus(&focus, cx);
        } else {
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

    // --- rendering ---

    /// The shared panel. `id_prefix` keeps element ids unique between the two
    /// hosts so both can be mounted without colliding.
    pub(crate) fn render_core_setup_panel(
        &mut self,
        id_prefix: &'static str,
        cx: &mut Context<Self>,
    ) -> Div {
        let colors = self.theme.colors;
        div()
            .flex()
            .flex_col()
            .gap_3()
            .child(self.render_core_setup_discovered(id_prefix, cx))
            .child(self.render_core_setup_download(id_prefix, cx))
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
                    .child(settings_path_field_row(
                        self.settings_inputs.core_path.clone(),
                        SharedString::from(format!("{id_prefix}-browse-core-path")),
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
                                Button::new(
                                    SharedString::from(format!("{id_prefix}-import-core")),
                                    self.t("settings-core-import"),
                                )
                                .aria_label(self.t("settings-core-import-aria"))
                                .style(ButtonStyle::Primary)
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.request_import_core_from_input(cx);
                                }))
                                .render(colors),
                            )
                            .child(
                                Button::new(
                                    SharedString::from(format!("{id_prefix}-link-core")),
                                    self.t("settings-core-link"),
                                )
                                .aria_label(self.t("settings-core-link-aria"))
                                .style(ButtonStyle::Secondary)
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.request_link_core_from_input(cx);
                                }))
                                .render(colors),
                            ),
                    ),
            )
    }

    fn render_core_setup_discovered(
        &mut self,
        id_prefix: &'static str,
        cx: &mut Context<Self>,
    ) -> Div {
        let colors = self.theme.colors;
        let scanning = self.core_setup.scanning;
        let scanned = self.core_setup.scanned;
        let discovered = self.core_setup.discovered.clone();
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
                        Button::new(
                            SharedString::from(format!("{id_prefix}-rescan-cores")),
                            self.t("core-setup-rescan"),
                        )
                        .icon(IconName::ScanSearch)
                        .aria_label(self.t("core-setup-rescan-aria"))
                        .style(ButtonStyle::Secondary)
                        .disabled(scanning)
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.request_core_discovery(cx);
                        }))
                        .render(colors),
                    ),
            )
            .child(div().text_xs().text_color(colors.text_muted).child(summary))
            .children(discovered.into_iter().enumerate().map(|(index, core)| {
                let import_path = core.path.clone();
                let link_path = core.path.clone();
                let already_registered = core.already_registered;
                div()
                    .id(SharedString::from(format!(
                        "{id_prefix}-discovered-core-{index}"
                    )))
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
                                Button::new(
                                    SharedString::from(format!(
                                        "{id_prefix}-import-discovered-{index}"
                                    )),
                                    self.t("settings-core-import"),
                                )
                                .aria_label(self.t_args(
                                    "core-setup-import-aria",
                                    &[("path", FluentValue::from(core.path.as_str()))],
                                ))
                                .style(ButtonStyle::Primary)
                                .disabled(already_registered)
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.request_core_command(
                                        CoreCommandView::Import {
                                            path: import_path.clone(),
                                        },
                                        cx,
                                    );
                                }))
                                .render(colors),
                            )
                            .child(
                                Button::new(
                                    SharedString::from(format!(
                                        "{id_prefix}-link-discovered-{index}"
                                    )),
                                    self.t("settings-core-link"),
                                )
                                .aria_label(self.t_args(
                                    "core-setup-link-aria",
                                    &[("path", FluentValue::from(core.path.as_str()))],
                                ))
                                .style(ButtonStyle::Secondary)
                                .disabled(already_registered)
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.request_core_command(
                                        CoreCommandView::Link {
                                            path: link_path.clone(),
                                        },
                                        cx,
                                    );
                                }))
                                .render(colors),
                            ),
                    )
            }))
    }

    fn render_core_setup_download(
        &mut self,
        id_prefix: &'static str,
        cx: &mut Context<Self>,
    ) -> Div {
        let colors = self.theme.colors;
        let offer = self.core_setup.offer.clone();
        let downloading = self.core_setup.downloading;
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
                        Button::new(
                            SharedString::from(format!("{id_prefix}-download-core")),
                            if downloading {
                                self.t("core-setup-download-running")
                            } else {
                                self.t("core-setup-download-action")
                            },
                        )
                        .icon(IconName::FolderDown)
                        .aria_label(self.t_args(
                            "core-setup-download-aria",
                            &[("version", FluentValue::from(offer.version.as_str()))],
                        ))
                        .style(ButtonStyle::Primary)
                        .disabled(downloading)
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.request_core_download(cx);
                        }))
                        .render(colors),
                    ),
                )
            })
    }

    pub(crate) fn render_core_setup_onboarding(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let colors = self.theme.colors;
        let has_core = !self.cores.installations.is_empty();
        Dialog::new(
            "core-setup-dialog",
            self.t("dialog-core-setup-title"),
            self.theme,
        )
        .description(self.t("dialog-core-setup-description"))
        .width(560.0)
        .key_context("CoreSetupDialog")
        .track_focus(self.core_setup_dialog_focus.clone())
        .child(
            div()
                .flex()
                .items_start()
                .gap_2()
                .text_xs()
                .text_color(colors.text_secondary)
                .child(Icon::new(IconName::Info).size(IconSize::Small))
                .child(self.t("dialog-core-setup-remote-hint")),
        )
        .child(self.render_core_setup_panel("onboarding", cx))
        .when(has_core, |dialog| {
            dialog.child(
                div()
                    .flex()
                    .items_start()
                    .gap_2()
                    .text_xs()
                    .text_color(colors.text_secondary)
                    .child(
                        Icon::new(IconName::CircleCheck)
                            .size(IconSize::Small)
                            .color(colors.success),
                    )
                    .child(self.t("dialog-core-setup-restart-hint")),
            )
        })
        .action(
            Button::new(
                "core-setup-open-settings",
                self.t("dialog-core-setup-open-settings"),
            )
            .aria_label(self.t("dialog-core-setup-open-settings-aria"))
            .style(ButtonStyle::Secondary)
            .track_focus(self.core_setup_settings_focus.clone())
            .on_click(cx.listener(|this, _, window, cx| {
                this.open_engine_settings_from_onboarding(window, cx);
            }))
            .render(colors),
        )
        .action(
            Button::new("core-setup-dismiss", self.t("dialog-core-setup-dismiss"))
                .aria_label(self.t("dialog-core-setup-dismiss-aria"))
                .style(ButtonStyle::Primary)
                .track_focus(self.core_setup_dismiss_focus.clone())
                .on_click(cx.listener(|this, _, window, cx| {
                    this.dismiss_core_setup_onboarding(window, cx);
                }))
                .render(colors),
        )
        .into_any_element()
    }
}
