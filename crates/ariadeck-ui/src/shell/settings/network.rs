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

    pub(crate) fn toggle_auto_probe_on_add(&mut self, cx: &mut Context<Self>) {
        if self.is_pending() {
            return;
        }
        self.settings_page.draft_auto_probe_on_add = !self.settings_page.draft_auto_probe_on_add;
        self.settings_page.error = None;
        cx.notify();
    }

    pub(crate) fn toggle_keep_original_as_mirror(&mut self, cx: &mut Context<Self>) {
        if self.is_pending() {
            return;
        }
        self.settings_page
            .draft_mirror_rules
            .keep_original_as_mirror = !self
            .settings_page
            .draft_mirror_rules
            .keep_original_as_mirror;
        self.settings_page.error = None;
        cx.notify();
    }

    fn apply_header_preset_editor_fields(
        &self,
        preset: &mut crate::DomainHeaderPresetView,
        cx: &App,
    ) {
        let name = self
            .settings_inputs
            .header_preset_name
            .read(cx)
            .value()
            .trim()
            .to_owned();
        if !name.is_empty() {
            preset.name = name;
        }
        preset.domains = self
            .settings_inputs
            .header_preset_domains
            .read(cx)
            .value()
            .trim()
            .to_owned();
        preset.referer = self
            .settings_inputs
            .header_preset_referer
            .read(cx)
            .value()
            .trim()
            .to_owned();
        preset.user_agent = self
            .settings_inputs
            .header_preset_user_agent
            .read(cx)
            .value()
            .trim()
            .to_owned();
        preset.headers = self
            .settings_inputs
            .header_preset_headers
            .read(cx)
            .value()
            .to_string();
    }

    fn flush_header_preset_editor(&mut self, cx: &App) {
        let Some(index) = self.settings_page.draft_header_preset_edit_index else {
            return;
        };
        let Some(mut preset) = self.settings_page.draft_header_presets.get(index).cloned() else {
            self.settings_page.draft_header_preset_edit_index = None;
            return;
        };
        self.apply_header_preset_editor_fields(&mut preset, cx);
        self.settings_page.draft_header_presets[index] = preset;
    }

    pub(super) fn preview_draft_header_presets(
        &self,
        cx: &App,
    ) -> Vec<crate::DomainHeaderPresetView> {
        let mut preview = self.settings_page.draft_header_presets.clone();
        if let Some(index) = self.settings_page.draft_header_preset_edit_index
            && let Some(preset) = preview.get_mut(index)
        {
            self.apply_header_preset_editor_fields(preset, cx);
        }
        preview
    }

    pub(crate) fn select_header_preset_editor(
        &mut self,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if index >= self.settings_page.draft_header_presets.len() {
            return;
        }
        self.flush_header_preset_editor(cx);
        if self.settings_page.draft_header_preset_edit_index == Some(index) {
            self.settings_page.draft_header_preset_edit_index = None;
            cx.notify();
            return;
        }
        self.settings_page.draft_header_preset_edit_index = Some(index);
        let preset = self.settings_page.draft_header_presets[index].clone();
        self.settings_inputs
            .header_preset_name
            .update(cx, |input, cx| input.set_value(preset.name, window, cx));
        self.settings_inputs
            .header_preset_domains
            .update(cx, |input, cx| input.set_value(preset.domains, window, cx));
        self.settings_inputs
            .header_preset_referer
            .update(cx, |input, cx| input.set_value(preset.referer, window, cx));
        self.settings_inputs
            .header_preset_user_agent
            .update(cx, |input, cx| {
                input.set_value(preset.user_agent, window, cx)
            });
        self.settings_inputs
            .header_preset_headers
            .update(cx, |input, cx| input.set_value(preset.headers, window, cx));
        cx.notify();
    }

    pub(crate) fn add_header_preset(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.is_pending() {
            return;
        }
        self.flush_header_preset_editor(cx);
        let n = self.settings_page.draft_header_presets.len() + 1;
        let id = loop {
            self.settings_page.next_draft_header_preset_seq += 1;
            let seq = self.settings_page.next_draft_header_preset_seq;
            let candidate = format!("preset-{seq}");
            if !self
                .settings_page
                .draft_header_presets
                .iter()
                .any(|preset| preset.id == candidate)
            {
                break candidate;
            }
        };
        self.settings_page
            .draft_header_presets
            .push(crate::DomainHeaderPresetView {
                id,
                name: format!("Preset {n}"),
                enabled: true,
                domains: String::new(),
                referer: String::new(),
                user_agent: String::new(),
                headers: String::new(),
            });
        let index = self.settings_page.draft_header_presets.len() - 1;
        self.select_header_preset_editor(index, window, cx);
    }

    pub(crate) fn toggle_header_preset(&mut self, index: usize, cx: &mut Context<Self>) {
        if self.is_pending() {
            return;
        }
        if let Some(preset) = self.settings_page.draft_header_presets.get_mut(index) {
            preset.enabled = !preset.enabled;
            self.settings_page.error = None;
            cx.notify();
        }
    }

    pub(crate) fn remove_header_preset(&mut self, index: usize, cx: &mut Context<Self>) {
        if self.is_pending() || index >= self.settings_page.draft_header_presets.len() {
            return;
        }
        self.flush_header_preset_editor(cx);
        self.settings_page.draft_header_presets.remove(index);
        if let Some(edit) = self.settings_page.draft_header_preset_edit_index {
            if edit == index {
                self.settings_page.draft_header_preset_edit_index = None;
            } else if edit > index {
                self.settings_page.draft_header_preset_edit_index = Some(edit - 1);
            }
        }
        self.settings_page.error = None;
        cx.notify();
    }

    fn apply_mirror_rule_editor_fields(&self, rule: &mut crate::MirrorRewriteRuleView, cx: &App) {
        let name = self
            .settings_inputs
            .mirror_rule_name
            .read(cx)
            .value()
            .trim()
            .to_owned();
        if !name.is_empty() {
            rule.name = name;
        }
        rule.pattern = self
            .settings_inputs
            .mirror_rule_pattern
            .read(cx)
            .value()
            .trim()
            .to_owned();
        rule.replacement = self
            .settings_inputs
            .mirror_rule_replacement
            .read(cx)
            .value()
            .trim()
            .to_owned();
    }

    fn flush_mirror_rule_editor(&mut self, cx: &App) {
        let Some(index) = self.settings_page.draft_mirror_rule_edit_index else {
            return;
        };
        let Some(mut rule) = self
            .settings_page
            .draft_mirror_rules
            .rules
            .get(index)
            .cloned()
        else {
            self.settings_page.draft_mirror_rule_edit_index = None;
            return;
        };
        self.apply_mirror_rule_editor_fields(&mut rule, cx);
        self.settings_page.draft_mirror_rules.rules[index] = rule;
    }

    pub(super) fn preview_draft_mirror_rules(&self, cx: &App) -> crate::MirrorRewriteSettingsView {
        let mut preview = self.settings_page.draft_mirror_rules.clone();
        if let Some(index) = self.settings_page.draft_mirror_rule_edit_index
            && let Some(rule) = preview.rules.get_mut(index)
        {
            self.apply_mirror_rule_editor_fields(rule, cx);
        }
        preview
    }

    pub(crate) fn select_mirror_rule_editor(
        &mut self,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if index >= self.settings_page.draft_mirror_rules.rules.len() {
            return;
        }
        self.flush_mirror_rule_editor(cx);
        if self.settings_page.draft_mirror_rule_edit_index == Some(index) {
            self.settings_page.draft_mirror_rule_edit_index = None;
            cx.notify();
            return;
        }
        self.settings_page.draft_mirror_rule_edit_index = Some(index);
        let rule = self.settings_page.draft_mirror_rules.rules[index].clone();
        self.settings_inputs
            .mirror_rule_name
            .update(cx, |input, cx| input.set_value(rule.name, window, cx));
        self.settings_inputs
            .mirror_rule_pattern
            .update(cx, |input, cx| input.set_value(rule.pattern, window, cx));
        self.settings_inputs
            .mirror_rule_replacement
            .update(cx, |input, cx| {
                input.set_value(rule.replacement, window, cx)
            });
        cx.notify();
    }

    pub(crate) fn add_mirror_rule(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.is_pending() {
            return;
        }
        self.flush_mirror_rule_editor(cx);
        let n = self.settings_page.draft_mirror_rules.rules.len() + 1;
        let id = loop {
            self.settings_page.next_draft_mirror_rule_seq += 1;
            let seq = self.settings_page.next_draft_mirror_rule_seq;
            let candidate = format!("mirror-{seq}");
            if !self
                .settings_page
                .draft_mirror_rules
                .rules
                .iter()
                .any(|rule| rule.id == candidate)
            {
                break candidate;
            }
        };
        self.settings_page
            .draft_mirror_rules
            .rules
            .push(crate::MirrorRewriteRuleView {
                id,
                name: format!("Mirror {n}"),
                enabled: true,
                match_kind: crate::MirrorMatchKindView::Prefix,
                pattern: String::new(),
                replacement: String::new(),
            });
        let index = self.settings_page.draft_mirror_rules.rules.len() - 1;
        self.select_mirror_rule_editor(index, window, cx);
    }

    pub(crate) fn add_builtin_mirror_template(
        &mut self,
        template: ariadeck_domain::MirrorRewriteRule,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.is_pending() {
            return;
        }
        self.flush_mirror_rule_editor(cx);
        let mut rule_view = from_domain_mirror_rule(&template);
        rule_view.enabled = true;
        if let Some(existing_idx) =
            self.settings_page
                .draft_mirror_rules
                .rules
                .iter()
                .position(|existing| {
                    existing.id == rule_view.id || existing.pattern == rule_view.pattern
                })
        {
            self.settings_page.draft_mirror_rules.rules[existing_idx].enabled = true;
            self.select_mirror_rule_editor(existing_idx, window, cx);
            return;
        }
        self.settings_page.draft_mirror_rules.rules.push(rule_view);
        let index = self.settings_page.draft_mirror_rules.rules.len() - 1;
        self.select_mirror_rule_editor(index, window, cx);
    }

    pub(crate) fn toggle_mirror_rule(&mut self, index: usize, cx: &mut Context<Self>) {
        if self.is_pending() {
            return;
        }
        if let Some(rule) = self.settings_page.draft_mirror_rules.rules.get_mut(index) {
            rule.enabled = !rule.enabled;
            self.settings_page.error = None;
            cx.notify();
        }
    }

    pub(crate) fn set_mirror_rule_match_kind(
        &mut self,
        index: usize,
        kind: crate::MirrorMatchKindView,
        cx: &mut Context<Self>,
    ) {
        if self.is_pending() {
            return;
        }
        if let Some(rule) = self.settings_page.draft_mirror_rules.rules.get_mut(index) {
            rule.match_kind = kind;
            self.settings_page.error = None;
            cx.notify();
        }
    }

    pub(crate) fn remove_mirror_rule(&mut self, index: usize, cx: &mut Context<Self>) {
        if self.is_pending() || index >= self.settings_page.draft_mirror_rules.rules.len() {
            return;
        }
        self.flush_mirror_rule_editor(cx);
        self.settings_page.draft_mirror_rules.rules.remove(index);
        if let Some(edit) = self.settings_page.draft_mirror_rule_edit_index {
            if edit == index {
                self.settings_page.draft_mirror_rule_edit_index = None;
            } else if edit > index {
                self.settings_page.draft_mirror_rule_edit_index = Some(edit - 1);
            }
        }
        self.settings_page.error = None;
        cx.notify();
    }

    pub(crate) fn submit_proxy_settings(&mut self, cx: &mut Context<Self>) {
        if self.is_pending() {
            return;
        }
        self.flush_header_preset_editor(cx);
        self.flush_mirror_rule_editor(cx);
        let header_presets = self.settings_page.draft_header_presets.clone();
        let domain_presets = to_domain_header_presets(&header_presets);
        if let Err(err) = ariadeck_domain::validate_header_presets(&domain_presets) {
            let (code, key) = match err {
                ariadeck_domain::HeaderPresetValidationError::EmptyId
                | ariadeck_domain::HeaderPresetValidationError::EmptyName
                | ariadeck_domain::HeaderPresetValidationError::DuplicateId(_) => (
                    "settings.invalid_header_preset",
                    "error-settings-header-preset-name",
                ),
                ariadeck_domain::HeaderPresetValidationError::EmptyDomains => (
                    "settings.invalid_header_preset",
                    "error-settings-header-preset-domains",
                ),
                ariadeck_domain::HeaderPresetValidationError::InvalidDomainPattern(_) => (
                    "settings.invalid_header_preset",
                    "error-settings-header-preset-domain-invalid",
                ),
                ariadeck_domain::HeaderPresetValidationError::InvalidControlCharacters(_) => (
                    "settings.invalid_header_preset",
                    "error-settings-header-preset-control",
                ),
                ariadeck_domain::HeaderPresetValidationError::TooManyPresets
                | ariadeck_domain::HeaderPresetValidationError::TooManyDomains
                | ariadeck_domain::HeaderPresetValidationError::TooManyHeaders => (
                    "settings.invalid_header_preset",
                    "error-settings-header-preset-limit",
                ),
                ariadeck_domain::HeaderPresetValidationError::InvalidHeaderLine => (
                    "settings.invalid_header_preset",
                    "error-settings-header-preset-format",
                ),
                ariadeck_domain::HeaderPresetValidationError::ForbiddenSensitiveHeader => (
                    "settings.forbidden_preset_header",
                    "error-settings-header-preset-sensitive",
                ),
            };
            self.settings_page.error = Some(OperationErrorView {
                code: code.into(),
                summary: self.t(key),
                retryable: false,
            });
            cx.notify();
            return;
        }
        let mirror_rules = self.settings_page.draft_mirror_rules.clone();
        for rule in &mirror_rules.rules {
            if rule.pattern.trim().is_empty() || rule.replacement.trim().is_empty() {
                self.settings_page.error = Some(OperationErrorView {
                    code: "settings.invalid_mirror_rule".into(),
                    summary: self.t("error-settings-mirror-rule-empty"),
                    retryable: false,
                });
                cx.notify();
                return;
            }
        }
        let domain_mirrors = to_domain_mirror_rules(&mirror_rules);
        if let Err(err) = ariadeck_domain::validate_mirror_rules(&domain_mirrors) {
            let key = match err {
                ariadeck_domain::MirrorRuleValidationError::EmptyId
                | ariadeck_domain::MirrorRuleValidationError::EmptyName
                | ariadeck_domain::MirrorRuleValidationError::DuplicateId(_) => {
                    "error-settings-mirror-rule-name"
                }
                ariadeck_domain::MirrorRuleValidationError::TooManyRules => {
                    "error-settings-mirror-rule-limit"
                }
                ariadeck_domain::MirrorRuleValidationError::EmptyPattern => {
                    "error-settings-mirror-rule-empty"
                }
                ariadeck_domain::MirrorRuleValidationError::InvalidPattern(_) => {
                    "error-settings-mirror-rule-pattern"
                }
                ariadeck_domain::MirrorRuleValidationError::InvalidReplacement
                | ariadeck_domain::MirrorRuleValidationError::ReplacementContainsCredentials => {
                    "error-settings-mirror-rule-replacement"
                }
            };
            self.settings_page.error = Some(OperationErrorView {
                code: "settings.invalid_mirror_rule".into(),
                summary: self.t(key),
                retryable: false,
            });
            cx.notify();
            return;
        }
        let (proxy_draft, password_update) = self.read_proxy_draft(cx);
        let proxy_changed = proxy_draft != self.settings.download_proxy
            || !matches!(password_update, ProxyPasswordUpdateView::Unchanged);
        let rules_changed = header_presets != self.settings.header_presets
            || mirror_rules != self.settings.mirror_rules
            || self.settings_page.draft_auto_probe_on_add != self.settings.auto_probe_on_add;
        let source = match (proxy_changed, rules_changed) {
            (true, true) => SettingsSaveSource::Network,
            (false, true) => SettingsSaveSource::NetworkRules,
            _ => SettingsSaveSource::Proxy,
        };
        let mut settings = self.settings.clone();
        settings.download_proxy = proxy_draft;
        settings.header_presets = header_presets;
        settings.mirror_rules = mirror_rules;
        settings.auto_probe_on_add = self.settings_page.draft_auto_probe_on_add;
        self.request_settings_save(settings, password_update, source, cx);
    }

    pub(crate) fn toggle_check_certificate(&mut self, cx: &mut Context<Self>) {
        if self.is_pending() {
            return;
        }
        self.settings_page.draft_check_certificate = !self.settings_page.draft_check_certificate;
        cx.notify();
    }

    pub(super) fn render_settings_network_sections(
        &mut self,
        cx: &mut Context<Self>,
    ) -> Vec<(SettingsSectionId, Div)> {
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
        let proxy_section = settings_section_owned(self.t("settings-network-proxy"), colors)
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
            );

        vec![
            (SettingsSectionId::NetworkProxy, proxy_section),
            (
                SettingsSectionId::NetworkHeaderPresets,
                self.render_header_presets_section(cx),
            ),
            (
                SettingsSectionId::NetworkMirrors,
                self.render_mirror_rules_section(cx),
            ),
        ]
    }

    fn render_header_presets_section(&mut self, cx: &mut Context<Self>) -> Div {
        let colors = self.theme.colors;
        let pending = self.is_pending();
        let presets = self.settings_page.draft_header_presets.clone();
        let edit_index = self.settings_page.draft_header_preset_edit_index;
        let edit_label = self.t("settings-category-edit");
        let remove_label = self.t("settings-category-remove");

        let mut list = div().flex().flex_col().gap_2();
        if presets.is_empty() {
            list = list.child(
                div()
                    .text_xs()
                    .text_color(colors.text_muted)
                    .child(self.t("settings-header-presets-empty")),
            );
        }
        for (index, preset) in presets.iter().enumerate() {
            let is_editing = edit_index == Some(index);
            let preset_id = preset.id.clone();
            let toggle_panel = cx.entity().downgrade();
            list = list.child(
                div()
                    .id(SharedString::from(format!("header-preset-row-{preset_id}")))
                    .flex()
                    .flex_col()
                    .gap_2()
                    .px_3()
                    .py_2()
                    .rounded_md()
                    .border_1()
                    .border_color(if is_editing {
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
                                Switch::new(SharedString::from(format!(
                                    "header-preset-toggle-{preset_id}"
                                )))
                                .checked(preset.enabled)
                                .disabled(pending)
                                .on_change(move |_, _, cx| {
                                    toggle_panel
                                        .update(cx, |panel, cx| {
                                            panel.toggle_header_preset(index, cx)
                                        })
                                        .ok();
                                }),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .flex()
                                    .flex_col()
                                    .child(
                                        div()
                                            .text_sm()
                                            .font_weight(FontWeight::MEDIUM)
                                            .text_color(colors.text_primary)
                                            .child(preset.name.clone()),
                                    )
                                    .when(!is_editing, |col| {
                                        col.child(
                                            div()
                                                .truncate()
                                                .text_xs()
                                                .text_color(colors.text_muted)
                                                .child(if preset.domains.trim().is_empty() {
                                                    self.t("settings-header-preset-no-domains")
                                                } else {
                                                    preset.domains.clone()
                                                }),
                                        )
                                    }),
                            )
                            .child({
                                let mut button = KitButton::new(SharedString::from(format!(
                                    "header-preset-edit-{preset_id}"
                                )))
                                .small()
                                .compact()
                                .icon(gpui_kit::component::Icon::new(IconName::Pencil))
                                .label(edit_label.clone())
                                .disabled(pending)
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    this.select_header_preset_editor(index, window, cx);
                                }));
                                if is_editing {
                                    button = button.primary();
                                }
                                button
                            })
                            .child(
                                KitButton::new(SharedString::from(format!(
                                    "header-preset-remove-{preset_id}"
                                )))
                                .ghost()
                                .small()
                                .compact()
                                .icon(gpui_kit::component::Icon::new(IconName::Trash2))
                                .label(remove_label.clone())
                                .disabled(pending)
                                .on_click(cx.listener(
                                    move |this, _, _, cx| {
                                        this.remove_header_preset(index, cx);
                                    },
                                )),
                            ),
                    )
                    .when(is_editing, |row| {
                        row.child(
                            div()
                                .flex()
                                .flex_col()
                                .gap_2()
                                .pt_1()
                                .child(settings_labeled_input(
                                    self.t("settings-header-preset-name"),
                                    &self.settings_inputs.header_preset_name,
                                    colors,
                                ))
                                .child(settings_labeled_input(
                                    self.t("settings-header-preset-domains"),
                                    &self.settings_inputs.header_preset_domains,
                                    colors,
                                ))
                                .child(settings_labeled_input(
                                    self.t("dialog-add-referer"),
                                    &self.settings_inputs.header_preset_referer,
                                    colors,
                                ))
                                .child(settings_labeled_input(
                                    self.t("dialog-add-user-agent"),
                                    &self.settings_inputs.header_preset_user_agent,
                                    colors,
                                ))
                                .child(
                                    div()
                                        .flex()
                                        .flex_col()
                                        .gap_1()
                                        .child(
                                            div()
                                                .text_xs()
                                                .font_weight(FontWeight::MEDIUM)
                                                .text_color(colors.text_secondary)
                                                .child(self.t("dialog-add-custom-headers")),
                                        )
                                        .child(
                                            Textarea::new(
                                                &self.settings_inputs.header_preset_headers,
                                            )
                                            .h(px(64.0)),
                                        ),
                                ),
                        )
                    }),
            );
        }

        settings_section_owned(self.t("settings-network-header-presets"), colors)
            .child(
                div()
                    .text_xs()
                    .text_color(colors.text_muted)
                    .child(self.t("settings-header-presets-desc")),
            )
            .child(div().max_w(px(760.0)).child(list))
            .child(
                div().mt_2().child(
                    KitButton::new("add-header-preset")
                        .icon(gpui_kit::component::Icon::new(IconName::Plus))
                        .label(self.t("settings-header-preset-add"))
                        .disabled(pending || presets.len() >= 64)
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.add_header_preset(window, cx);
                        })),
                ),
            )
    }

    fn render_mirror_rules_section(&mut self, cx: &mut Context<Self>) -> Div {
        let colors = self.theme.colors;
        let pending = self.is_pending();
        let auto_probe = self.settings_page.draft_auto_probe_on_add;
        let keep_original = self
            .settings_page
            .draft_mirror_rules
            .keep_original_as_mirror;
        let rules = self.settings_page.draft_mirror_rules.rules.clone();
        let edit_index = self.settings_page.draft_mirror_rule_edit_index;
        let edit_label = self.t("settings-category-edit");
        let remove_label = self.t("settings-category-remove");

        let probe_panel = cx.entity().downgrade();
        let keep_panel = cx.entity().downgrade();
        let templates = ariadeck_domain::builtin_mirror_templates();

        let mut list = div().flex().flex_col().gap_2();
        if rules.is_empty() {
            list = list.child(
                div()
                    .text_xs()
                    .text_color(colors.text_muted)
                    .child(self.t("settings-mirror-rules-empty")),
            );
        }
        for (index, rule) in rules.iter().enumerate() {
            let is_editing = edit_index == Some(index);
            let rule_id = rule.id.clone();
            let toggle_panel = cx.entity().downgrade();
            let kind_panel = cx.entity().downgrade();
            let kind_control = SegmentedControl::new(
                SharedString::from(format!("mirror-rule-kind-{rule_id}")),
                crate::MirrorMatchKindView::ALL
                    .map(|kind| Segment::new(self.t(kind.message_key()))),
                rule.match_kind.index(),
                self.theme,
            )
            .disabled(pending)
            .on_select(move |selected, _window, cx| {
                let kind = crate::MirrorMatchKindView::ALL
                    .get(selected)
                    .copied()
                    .unwrap_or(crate::MirrorMatchKindView::Prefix);
                kind_panel
                    .update(cx, |panel, cx| {
                        panel.set_mirror_rule_match_kind(index, kind, cx)
                    })
                    .ok();
            });

            list = list.child(
                div()
                    .id(SharedString::from(format!("mirror-rule-row-{rule_id}")))
                    .flex()
                    .flex_col()
                    .gap_2()
                    .px_3()
                    .py_2()
                    .rounded_md()
                    .border_1()
                    .border_color(if is_editing {
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
                                Switch::new(SharedString::from(format!(
                                    "mirror-rule-toggle-{rule_id}"
                                )))
                                .checked(rule.enabled)
                                .disabled(pending)
                                .on_change(move |_, _, cx| {
                                    toggle_panel
                                        .update(cx, |panel, cx| panel.toggle_mirror_rule(index, cx))
                                        .ok();
                                }),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .flex()
                                    .flex_col()
                                    .child(
                                        div()
                                            .flex()
                                            .items_center()
                                            .gap_2()
                                            .child(
                                                div()
                                                    .text_sm()
                                                    .font_weight(FontWeight::MEDIUM)
                                                    .text_color(colors.text_primary)
                                                    .child(rule.name.clone()),
                                            )
                                            .child(
                                                div()
                                                    .text_xs()
                                                    .font_weight(FontWeight::MEDIUM)
                                                    .px_2()
                                                    .py_0p5()
                                                    .rounded_full()
                                                    .bg(with_alpha(colors.accent, 0.12))
                                                    .text_color(colors.accent)
                                                    .child(self.t(rule.match_kind.message_key())),
                                            ),
                                    )
                                    .when(!is_editing, |col| {
                                        col.child(
                                            div()
                                                .truncate()
                                                .text_xs()
                                                .text_color(colors.text_muted)
                                                .child(format!(
                                                    "{} -> {}",
                                                    rule.pattern, rule.replacement
                                                )),
                                        )
                                    }),
                            )
                            .child({
                                let mut button = KitButton::new(SharedString::from(format!(
                                    "mirror-rule-edit-{rule_id}"
                                )))
                                .small()
                                .compact()
                                .icon(gpui_kit::component::Icon::new(IconName::Pencil))
                                .label(edit_label.clone())
                                .disabled(pending)
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    this.select_mirror_rule_editor(index, window, cx);
                                }));
                                if is_editing {
                                    button = button.primary();
                                }
                                button
                            })
                            .child(
                                KitButton::new(SharedString::from(format!(
                                    "mirror-rule-remove-{rule_id}"
                                )))
                                .ghost()
                                .small()
                                .compact()
                                .icon(gpui_kit::component::Icon::new(IconName::Trash2))
                                .label(remove_label.clone())
                                .disabled(pending)
                                .on_click(cx.listener(
                                    move |this, _, _, cx| {
                                        this.remove_mirror_rule(index, cx);
                                    },
                                )),
                            ),
                    )
                    .when(is_editing, |row| {
                        row.child(
                            div()
                                .flex()
                                .flex_col()
                                .gap_2()
                                .pt_1()
                                .child(
                                    div()
                                        .flex()
                                        .items_center()
                                        .gap_2()
                                        .child(
                                            div()
                                                .text_xs()
                                                .text_color(colors.text_muted)
                                                .child(self.t("settings-mirror-rule-match-kind")),
                                        )
                                        .child(kind_control),
                                )
                                .child(settings_labeled_input(
                                    self.t("settings-mirror-rule-name"),
                                    &self.settings_inputs.mirror_rule_name,
                                    colors,
                                ))
                                .child(settings_labeled_input(
                                    self.t("settings-mirror-rule-pattern"),
                                    &self.settings_inputs.mirror_rule_pattern,
                                    colors,
                                ))
                                .child(settings_labeled_input(
                                    self.t("settings-mirror-rule-replacement"),
                                    &self.settings_inputs.mirror_rule_replacement,
                                    colors,
                                )),
                        )
                    }),
            );
        }

        settings_section_owned(self.t("settings-network-mirrors"), colors)
            .child(
                div().max_w(px(760.0)).child(settings_section_row_owned(
                    self.t("settings-auto-probe-on-add"),
                    Some(self.t("settings-auto-probe-on-add-desc")),
                    Switch::new("toggle-auto-probe-on-add")
                        .checked(auto_probe)
                        .disabled(pending)
                        .on_change(move |_, _, cx| {
                            probe_panel
                                .update(cx, |panel, cx| panel.toggle_auto_probe_on_add(cx))
                                .ok();
                        }),
                    colors,
                )),
            )
            .child(
                div().max_w(px(760.0)).child(settings_section_row_owned(
                    self.t("settings-mirror-keep-original"),
                    Some(self.t("settings-mirror-keep-original-desc")),
                    Switch::new("toggle-mirror-keep-original")
                        .checked(keep_original)
                        .disabled(pending)
                        .on_change(move |_, _, cx| {
                            keep_panel
                                .update(cx, |panel, cx| panel.toggle_keep_original_as_mirror(cx))
                                .ok();
                        }),
                    colors,
                )),
            )
            .child(
                div()
                    .max_w(px(760.0))
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap_2()
                    .py_2()
                    .child(
                        div()
                            .text_xs()
                            .text_color(colors.text_muted)
                            .child(self.t("settings-mirror-templates")),
                    )
                    .children(templates.into_iter().map(|template| {
                        let btn_id = SharedString::from(format!("add-mirror-tpl-{}", template.id));
                        let label = template.name.clone();
                        KitButton::new(btn_id)
                            .ghost()
                            .small()
                            .compact()
                            .icon(gpui_kit::component::Icon::new(IconName::Plus))
                            .label(label)
                            .disabled(pending)
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.add_builtin_mirror_template(template.clone(), window, cx);
                            }))
                    })),
            )
            .child(div().max_w(px(760.0)).child(list))
            .child(
                div().mt_2().child(
                    KitButton::new("add-mirror-rule")
                        .icon(gpui_kit::component::Icon::new(IconName::Plus))
                        .label(self.t("settings-mirror-rule-add"))
                        .disabled(pending || rules.len() >= 64)
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.add_mirror_rule(window, cx);
                        })),
                ),
            )
    }
}
