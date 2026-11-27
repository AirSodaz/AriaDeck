//! Settings `transfers` section logic and rendering for `SettingsPanelView` and `AppShell`.

use super::*;

impl AppShell {
    pub(super) fn request_tracker_list_refresh_with_draft(
        &mut self,
        draft: TrackerListSettingsView,
        cx: &mut Context<Self>,
    ) {
        if self.pending_settings_save.is_some() || self.pending_tracker_refresh.is_some() {
            return;
        }
        // Mirror the form draft into settings so desktop can read URL/source for this fetch.
        self.settings.tracker_list = draft;
        let request_id = self.allocate_request_id();
        self.pending_tracker_refresh = Some(request_id);
        if let Some(panel) = self.settings_panel_entity() {
            panel.update(cx, |panel, cx| {
                panel.pending_tracker_refresh = true;
                panel.settings_page.error = None;
                cx.notify();
            });
        }
        cx.emit(AppShellEvent::TrackerListRefreshRequested { request_id });
        cx.notify();
    }

    pub fn set_tracker_list_refresh_result(
        &mut self,
        request_id: RequestId,
        settings: Option<SettingsView>,
        tracker_count: usize,
        result: Result<(), OperationErrorView>,
        auto: bool,
        cx: &mut Context<Self>,
    ) {
        if self.pending_tracker_refresh != Some(request_id) && !auto {
            return;
        }
        if self.pending_tracker_refresh == Some(request_id) {
            self.pending_tracker_refresh = None;
            if let Some(panel) = self.settings_panel_entity() {
                panel.update(cx, |panel, cx| {
                    panel.pending_tracker_refresh = false;
                    cx.notify();
                });
            }
        }
        match result {
            Ok(()) => {
                if let Some(settings) = settings {
                    self.apply_settings(settings, None, cx);
                }
                if !auto {
                    self.show_notice(
                        self.t_args(
                            "notice-settings-tracker-refreshed",
                            &[("count", FluentValue::from(tracker_count as i64))],
                        ),
                        false,
                        cx,
                    );
                }
            }
            Err(error) => {
                if !auto {
                    if let Some(panel) = self.settings_panel_entity() {
                        panel.update(cx, |panel, cx| {
                            panel.settings_page.error = Some(error);
                            cx.notify();
                        });
                    }
                    cx.notify();
                }
            }
        }
    }
}

impl SettingsPanelView {
    /// Save dirty speed-limit, transfer-policy, and/or tracker-list fields.
    /// Used by the Transfers category footer so groups persist together.
    pub(crate) fn submit_transfers(&mut self, cx: &mut Context<Self>) {
        if self.is_pending() {
            return;
        }
        let speed_limit_draft = self.read_speed_limit_draft(cx);
        let transfer_policy_draft = self.read_transfer_policy_draft(cx);
        let tracker_list_draft = self.read_tracker_list_draft(cx);
        let sl_dirty = speed_limit_draft != self.settings.speed_limits;
        let tp_dirty = transfer_policy_draft != self.settings.transfer_policy;
        let tl_dirty = tracker_list_draft != self.settings.tracker_list;
        if !sl_dirty && !tp_dirty && !tl_dirty {
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
        if tl_dirty {
            settings.tracker_list = tracker_list_draft;
        }
        let source = if tl_dirty || (sl_dirty && tp_dirty) {
            SettingsSaveSource::Transfers
        } else if sl_dirty {
            SettingsSaveSource::SpeedLimit
        } else if tp_dirty {
            SettingsSaveSource::TransferPolicy
        } else {
            return;
        };
        self.request_settings_save(settings, ProxyPasswordUpdateView::Unchanged, source, cx);
    }

    pub(super) fn read_speed_limit_draft(&self, cx: &Context<Self>) -> SpeedLimitSettingsView {
        SpeedLimitSettingsView {
            download_limit: self
                .settings_inputs
                .download_limit
                .read(cx)
                .value()
                .trim()
                .into(),
            upload_limit: self
                .settings_inputs
                .upload_limit
                .read(cx)
                .value()
                .trim()
                .into(),
        }
    }

    pub(super) fn read_transfer_policy_draft(
        &self,
        cx: &Context<Self>,
    ) -> TransferPolicySettingsView {
        TransferPolicySettingsView {
            max_concurrent_downloads: self
                .settings_inputs
                .max_concurrent
                .read(cx)
                .value()
                .trim()
                .into(),
            max_connection_per_server: self
                .settings_inputs
                .max_connection
                .read(cx)
                .value()
                .trim()
                .into(),
            split: self.settings_inputs.split.read(cx).value().trim().into(),
            min_split_size: self
                .settings_inputs
                .min_split_size
                .read(cx)
                .value()
                .trim()
                .into(),
            file_allocation: self.settings_page.draft_file_allocation,
            check_integrity: self.settings_page.draft_check_integrity,
        }
    }

    pub(super) fn read_tracker_list_draft(&self, cx: &Context<Self>) -> TrackerListSettingsView {
        TrackerListSettingsView {
            enabled: self.settings_page.draft_tracker_enabled,
            source: self.settings_page.draft_tracker_source,
            custom_url: self
                .settings_inputs
                .tracker_custom_url
                .read(cx)
                .value()
                .trim()
                .into(),
            auto_refresh: self.settings_page.draft_tracker_auto_refresh,
            // Timestamp is owned by successful network refresh, not the form.
            last_refreshed_at: self.settings.tracker_list.last_refreshed_at,
            list_text: self
                .settings_inputs
                .tracker_list_text
                .read(cx)
                .value()
                .into(),
        }
    }

    pub(super) fn sync_tracker_list_fields_from_settings(
        &mut self,
        window: Option<&mut Window>,
        cx: &mut Context<Self>,
    ) {
        let tracker = self.settings.tracker_list.clone();
        self.settings_page.draft_tracker_enabled = tracker.enabled;
        self.settings_page.draft_tracker_source = tracker.source;
        self.settings_page.draft_tracker_auto_refresh = tracker.auto_refresh;
        if let Some(window) = window {
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
    }

    pub(crate) fn toggle_tracker_list_enabled(&mut self, cx: &mut Context<Self>) {
        if self.is_pending() {
            return;
        }
        self.settings_page.draft_tracker_enabled = !self.settings_page.draft_tracker_enabled;
        self.settings_page.error = None;
        cx.notify();
    }

    pub(crate) fn select_tracker_list_source(
        &mut self,
        source: TrackerListSourceView,
        cx: &mut Context<Self>,
    ) {
        if self.is_pending() {
            return;
        }
        if self.settings_page.draft_tracker_source == source {
            return;
        }
        self.settings_page.draft_tracker_source = source;
        self.settings_page.error = None;
        cx.notify();
    }

    pub(crate) fn toggle_tracker_list_auto_refresh(&mut self, cx: &mut Context<Self>) {
        if self.is_pending() {
            return;
        }
        self.settings_page.draft_tracker_auto_refresh =
            !self.settings_page.draft_tracker_auto_refresh;
        self.settings_page.error = None;
        cx.notify();
    }

    pub(crate) fn request_tracker_list_refresh(&mut self, cx: &mut Context<Self>) {
        if self.is_pending() || self.pending_tracker_refresh {
            return;
        }
        let draft = self.read_tracker_list_draft(cx);
        self.settings.tracker_list = draft.clone();
        self.pending_tracker_refresh = true;
        self.settings_page.error = None;
        cx.emit(SettingsPanelEvent::RefreshTrackerList { draft });
        cx.notify();
    }

    pub(super) fn sync_speed_limit_fields_from_settings(
        &mut self,
        window: Option<&mut Window>,
        cx: &mut Context<Self>,
    ) {
        let speed_limits = self.settings.speed_limits.clone();
        if let Some(window) = window {
            self.settings_inputs.download_limit.update(cx, |input, cx| {
                input.set_value(speed_limits.download_limit.clone(), window, cx);
            });
            self.settings_inputs.upload_limit.update(cx, |input, cx| {
                input.set_value(speed_limits.upload_limit.clone(), window, cx);
            });
        }
    }

    pub(super) fn sync_transfer_policy_fields_from_settings(
        &mut self,
        window: Option<&mut Window>,
        cx: &mut Context<Self>,
    ) {
        let policy = self.settings.transfer_policy.clone();
        if let Some(window) = window {
            self.settings_inputs.max_concurrent.update(cx, |input, cx| {
                input.set_value(policy.max_concurrent_downloads.clone(), window, cx);
            });
            self.settings_inputs.max_connection.update(cx, |input, cx| {
                input.set_value(policy.max_connection_per_server.clone(), window, cx);
            });
            self.settings_inputs.split.update(cx, |input, cx| {
                input.set_value(policy.split.clone(), window, cx);
            });
            self.settings_inputs.min_split_size.update(cx, |input, cx| {
                input.set_value(policy.min_split_size.clone(), window, cx);
            });
        }
    }

    pub(crate) fn select_file_allocation(
        &mut self,
        method: FileAllocationView,
        cx: &mut Context<Self>,
    ) {
        if self.is_pending() {
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
        if self.is_pending() {
            return;
        }
        self.settings_page.draft_check_integrity = !self.settings_page.draft_check_integrity;
        self.settings_page.error = None;
        cx.notify();
    }

    pub(super) fn render_settings_transfers_sections(
        &mut self,
        cx: &mut Context<Self>,
    ) -> Vec<(SettingsSectionId, Div)> {
        let colors = self.theme.colors;
        let pending = self.is_pending();
        let allocation_selected = FileAllocationView::all()
            .iter()
            .position(|method| *method == self.settings_page.draft_file_allocation)
            .unwrap_or(1);
        let allocation_panel = cx.entity().downgrade();
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
            allocation_panel
                .update(cx, |panel, cx| panel.select_file_allocation(method, cx))
                .ok();
        });
        let draft_check_integrity = self.settings_page.draft_check_integrity;
        let integrity_panel = cx.entity().downgrade();
        let speed_section = settings_section_owned(self.t("settings-speed-limits"), colors).child(
            div()
                .py_4()
                .max_w(px(760.0))
                .flex()
                .gap_3()
                .child(
                    settings_labeled_input(
                        self.t("settings-download-limit"),
                        &self.settings_inputs.download_limit,
                        colors,
                    )
                    .flex_1()
                    .min_w_0(),
                )
                .child(
                    settings_labeled_input(
                        self.t("settings-upload-limit"),
                        &self.settings_inputs.upload_limit,
                        colors,
                    )
                    .flex_1()
                    .min_w_0(),
                ),
        );
        let policy_section = settings_section_owned(self.t("settings-transfer-policy"), colors)
            .child(
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
                                    &self.settings_inputs.max_concurrent,
                                    colors,
                                )
                                .flex_1()
                                .min_w_0(),
                            )
                            .child(
                                settings_labeled_input(
                                    self.t("settings-connections-per-server"),
                                    &self.settings_inputs.max_connection,
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
                                    &self.settings_inputs.split,
                                    colors,
                                )
                                .flex_1()
                                .min_w_0(),
                            )
                            .child(
                                settings_labeled_input(
                                    self.t("settings-min-split-size"),
                                    &self.settings_inputs.min_split_size,
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
                        Switch::new("toggle-check-integrity")
                            .checked(draft_check_integrity)
                            .accessibility_label(if draft_check_integrity {
                                self.t("settings-integrity-check-disable")
                            } else {
                                self.t("settings-integrity-check-enable")
                            })
                            .disabled(pending)
                            .on_change(move |_, _, cx| {
                                integrity_panel
                                    .update(cx, |panel, cx| {
                                        panel.toggle_check_integrity(cx);
                                    })
                                    .ok();
                            }),
                        colors,
                    ))
                    .child(
                        div()
                            .text_xs()
                            .text_color(colors.text_muted)
                            .child(self.t("settings-transfer-policy-hint")),
                    ),
            );
        vec![
            (SettingsSectionId::SpeedLimits, speed_section),
            (SettingsSectionId::TransferPolicy, policy_section),
            (
                SettingsSectionId::TrackerList,
                self.render_settings_tracker_list(cx),
            ),
        ]
    }

    fn render_settings_tracker_list(&mut self, cx: &mut Context<Self>) -> Div {
        let colors = self.theme.colors;
        let pending = self.is_pending() || self.pending_tracker_refresh;
        let enabled = self.settings_page.draft_tracker_enabled;
        let auto_refresh = self.settings_page.draft_tracker_auto_refresh;
        let source = self.settings_page.draft_tracker_source;
        let custom = source == TrackerListSourceView::Custom;
        let tracker_count = self
            .settings_inputs
            .tracker_list_text
            .read(cx)
            .value()
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty() && !line.starts_with('#'))
            .count();
        let last_refreshed = self.settings.tracker_list.last_refreshed_at;
        let source_selected = source.index();
        let source_panel = cx.entity().downgrade();
        let source_control = SegmentedControl::new(
            "settings-tracker-source",
            TrackerListSourceView::all().map(|item| Segment::new(self.t(item.message_key()))),
            source_selected,
            self.theme,
        )
        .aria_label(self.t("settings-tracker-source-aria"))
        .disabled(pending)
        .on_select(move |index, _window, cx| {
            let source = match index {
                1 => TrackerListSourceView::Custom,
                _ => TrackerListSourceView::Curated,
            };
            source_panel
                .update(cx, |panel, cx| panel.select_tracker_list_source(source, cx))
                .ok();
        });
        let enable_panel = cx.entity().downgrade();
        let auto_panel = cx.entity().downgrade();
        settings_section_owned(self.t("settings-tracker-list"), colors).child(
            div()
                .pt_4()
                .max_w(px(760.0))
                .flex()
                .flex_col()
                .gap_3()
                .child(
                    div()
                        .text_xs()
                        .text_color(colors.text_muted)
                        .child(self.t("settings-tracker-list-desc")),
                )
                .child(settings_section_row_owned(
                    self.t("settings-tracker-enabled"),
                    Some(self.t("settings-tracker-enabled-desc")),
                    Switch::new("toggle-tracker-enabled")
                        .checked(enabled)
                        .accessibility_label(self.t("settings-tracker-enabled-aria"))
                        .disabled(pending)
                        .on_change(move |_, _, cx| {
                            enable_panel
                                .update(cx, |panel, cx| panel.toggle_tracker_list_enabled(cx))
                                .ok();
                        }),
                    colors,
                ))
                .child(settings_section_row_owned(
                    self.t("settings-tracker-source"),
                    Some(self.t("settings-tracker-source-desc")),
                    source_control,
                    colors,
                ))
                .when(custom, |section| {
                    section.child(settings_labeled_input(
                        self.t("settings-tracker-custom-url"),
                        &self.settings_inputs.tracker_custom_url,
                        colors,
                    ))
                })
                .child(settings_section_row_owned(
                    self.t("settings-tracker-auto-refresh"),
                    Some(self.t("settings-tracker-auto-refresh-desc")),
                    Switch::new("toggle-tracker-auto-refresh")
                        .checked(auto_refresh)
                        .accessibility_label(self.t("settings-tracker-auto-refresh-aria"))
                        .disabled(pending)
                        .on_change(move |_, _, cx| {
                            auto_panel
                                .update(cx, |panel, cx| panel.toggle_tracker_list_auto_refresh(cx))
                                .ok();
                        }),
                    colors,
                ))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_3()
                        .child(
                            KitButton::new("refresh-tracker-list")
                                .label(self.t("settings-tracker-refresh"))
                                .accessibility_label(self.t("settings-tracker-refresh-aria"))
                                .disabled(pending)
                                .loading(self.pending_tracker_refresh)
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.request_tracker_list_refresh(cx);
                                })),
                        )
                        .child(div().text_xs().text_color(colors.text_muted).child(
                            match last_refreshed {
                                Some(ts) => self.t_args(
                                    "settings-tracker-last-refreshed",
                                    &[
                                        ("count", FluentValue::from(tracker_count as i64)),
                                        ("when", FluentValue::from(ts)),
                                    ],
                                ),
                                None => self.t_args(
                                    "settings-tracker-never-refreshed",
                                    &[("count", FluentValue::from(tracker_count as i64))],
                                ),
                            },
                        )),
                )
                .child(settings_labeled_input(
                    self.t("settings-tracker-list-text"),
                    &self.settings_inputs.tracker_list_text,
                    colors,
                ))
                .child(
                    div()
                        .text_xs()
                        .text_color(colors.text_muted)
                        .child(self.t("settings-tracker-list-hint")),
                ),
        )
    }
}
