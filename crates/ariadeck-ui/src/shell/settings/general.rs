//! Settings `general` section logic and rendering for `SettingsPanelView` and `AppShell`.

use super::*;

impl AppShell {
    pub(crate) fn select_color_scheme(
        &mut self,
        scheme: ColorSchemeView,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.pending_settings_save.is_some() || scheme == self.settings.color_scheme {
            return;
        }
        // Apply chrome immediately so TextFields switch light/dark before save returns.
        self.theme = resolve_theme(scheme, window);
        self.theme.sync_to_kit(Some(window), cx);
        if let Some(panel) = self.settings_panel_entity() {
            let theme = self.theme;
            panel.update(cx, |panel, cx| {
                panel.settings_page.draft_color_scheme = scheme;
                panel.theme = theme;
                cx.notify();
            });
        }
        self.sync_active_modal_context(Some(window), cx);
        self.sync_details_panel_context(cx);
        self.sync_core_setup_panels(cx);
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
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.pending_settings_save.is_some() || language == self.settings.language {
            return;
        }
        // Apply immediately so chrome switches language before save returns.
        self.set_language_runtime(language);
        self.refresh_input_placeholders(window, cx);
        if let Some(panel) = self.settings_panel_entity() {
            let translator = self.translator.clone();
            panel.update(cx, |panel, cx| {
                panel.settings_page.draft_language = language;
                panel.translator = translator;
                panel.refresh_placeholders(window, cx);
                cx.notify();
            });
        }
        self.sync_active_modal_context(Some(window), cx);
        self.sync_details_panel_context(cx);
        self.sync_core_setup_panels(cx);
        let mut settings = self.settings.clone();
        settings.language = language;
        self.request_settings_save(
            settings,
            ProxyPasswordUpdateView::Unchanged,
            SettingsSaveSource::Language,
            cx,
        );
    }
}

impl SettingsPanelView {
    pub(super) fn render_settings_general(&mut self, cx: &mut Context<Self>) -> Div {
        let colors = self.theme.colors;
        let pending = self.is_pending();
        let draft_scheme = self.settings_page.draft_color_scheme;
        let selected_scheme = ColorSchemeView::ALL
            .iter()
            .position(|scheme| *scheme == draft_scheme)
            .unwrap_or(0);
        let panel = cx.entity().downgrade();
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
        .on_select(move |index, _window, cx| {
            let scheme = ColorSchemeView::ALL
                .get(index)
                .copied()
                .unwrap_or(ColorSchemeView::System);
            panel
                .update(cx, |_, cx| {
                    cx.emit(SettingsPanelEvent::SelectColorScheme(scheme));
                })
                .ok();
        });
        let draft_language = self.settings_page.draft_language;
        let selected_language = LanguagePreferenceView::ALL
            .iter()
            .position(|language| *language == draft_language)
            .unwrap_or(0);
        let language_panel = cx.entity().downgrade();
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
            language_panel
                .update(cx, |_, cx| {
                    cx.emit(SettingsPanelEvent::SelectLanguage(language));
                })
                .ok();
        });
        let appearance_title = self.t("settings-appearance");
        let theme_label = self.t("settings-theme");
        let language_label = self.t("settings-language");
        let language_desc = self.t("settings-language-description");
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
            .child(self.render_settings_categories(cx))
    }
}
