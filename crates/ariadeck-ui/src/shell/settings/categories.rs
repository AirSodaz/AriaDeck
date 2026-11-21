//! Settings `categories` section logic and rendering for `SettingsPanelView`.

use super::*;

impl SettingsPanelView {
    pub(super) fn flush_category_editor(&mut self, cx: &mut Context<Self>) {
        let Some(index) = self.settings_page.draft_category_edit_index else {
            return;
        };
        if index >= self.settings_page.draft_categories.len() {
            self.settings_page.draft_category_edit_index = None;
            return;
        }
        let name = self
            .settings_inputs
            .category_name
            .read(cx)
            .value()
            .trim()
            .to_owned();
        let extensions = self
            .settings_inputs
            .category_extensions
            .read(cx)
            .value()
            .to_string();
        let directory = self
            .settings_inputs
            .directory
            .read(cx)
            .value()
            .trim()
            .to_owned();
        let category = &mut self.settings_page.draft_categories[index];
        if !name.is_empty() {
            category.name = name;
        }
        category.extensions = extensions;
        category.directory = directory;
    }

    pub(crate) fn preview_draft_categories(&self, cx: &App) -> Vec<crate::DownloadCategoryView> {
        let mut preview = self.settings_page.draft_categories.clone();
        if let Some(index) = self.settings_page.draft_category_edit_index
            && let Some(category) = preview.get_mut(index)
        {
            let name = self
                .settings_inputs
                .category_name
                .read(cx)
                .value()
                .trim()
                .to_owned();
            if !name.is_empty() {
                category.name = name;
            }
            category.extensions = self
                .settings_inputs
                .category_extensions
                .read(cx)
                .value()
                .to_string();
            category.directory = self
                .settings_inputs
                .directory
                .read(cx)
                .value()
                .trim()
                .to_owned();
        }
        preview
    }

    pub(crate) fn select_category_editor(
        &mut self,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if index >= self.settings_page.draft_categories.len() {
            return;
        }
        self.flush_category_editor(cx);
        self.settings_page.draft_category_edit_index = Some(index);
        let category = self.settings_page.draft_categories[index].clone();
        self.settings_inputs.category_name.update(cx, |input, cx| {
            input.set_value(category.name.clone(), window, cx);
        });
        self.settings_inputs
            .category_extensions
            .update(cx, |input, cx| {
                input.set_value(category.extensions.clone(), window, cx);
            });
        self.settings_inputs.directory.update(cx, |input, cx| {
            input.set_value(category.directory.clone(), window, cx);
        });
        cx.notify();
    }

    pub(crate) fn add_download_category(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.flush_category_editor(cx);
        let n = self.settings_page.draft_categories.len() + 1;
        let name = format!("Category {n}");
        let id = loop {
            self.settings_page.next_draft_category_seq += 1;
            let seq = self.settings_page.next_draft_category_seq;
            let candidate = format!("00000000-0000-4000-8000-{seq:012x}");
            if !self
                .settings_page
                .draft_categories
                .iter()
                .any(|category| category.id == candidate)
            {
                break candidate;
            }
        };
        self.settings_page
            .draft_categories
            .push(crate::DownloadCategoryView {
                id,
                name,
                directory: self.settings.download_directory.clone(),
                extensions: String::new(),
                is_fallback: false,
            });
        let index = self.settings_page.draft_categories.len() - 1;
        self.select_category_editor(index, window, cx);
    }

    pub(crate) fn remove_download_category(&mut self, index: usize, cx: &mut Context<Self>) {
        if index >= self.settings_page.draft_categories.len() {
            return;
        }
        if self.settings_page.draft_categories.len() <= 1 {
            return;
        }
        self.flush_category_editor(cx);
        let removed = &self.settings_page.draft_categories[index];
        // General (fixed fallback) cannot be removed.
        if removed.is_fallback {
            return;
        }
        let _removed = self.settings_page.draft_categories.remove(index);
        if let Some(edit) = self.settings_page.draft_category_edit_index {
            if edit == index {
                self.settings_page.draft_category_edit_index = None;
            } else if edit > index {
                self.settings_page.draft_category_edit_index = Some(edit - 1);
            }
        }
        // Keep exactly one fallback (General).
        if !self
            .settings_page
            .draft_categories
            .iter()
            .any(|c| c.is_fallback)
        {
            let general_idx = self
                .settings_page
                .draft_categories
                .iter()
                .position(|c| c.name.eq_ignore_ascii_case("General"))
                .unwrap_or(0);
            if let Some(general) = self.settings_page.draft_categories.get_mut(general_idx) {
                general.is_fallback = true;
            }
        }
        cx.notify();
    }

    pub(super) fn render_settings_categories(&mut self, cx: &mut Context<Self>) -> Div {
        let colors = self.theme.colors;
        let pending = self.is_pending();
        let categories = self.settings_page.draft_categories.clone();
        let edit_index = self.settings_page.draft_category_edit_index;
        let title = self.t("settings-categories");
        let desc = self.t("settings-categories-desc");
        let add_label = self.t("settings-category-add");
        let browse_label = self.t("button-browse");
        let remove_label = self.t("settings-category-remove");
        let empty_label = self.t("settings-categories-empty");
        let no_dir = self.t("settings-category-no-directory");
        let edit_label = self.t("settings-category-edit");
        let extensions_hint = self.t("settings-category-extensions-hint");
        let extensions_prefix = self.t("settings-category-extensions");
        let fallback_badge = self.t("settings-category-is-fallback");

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
            let is_fallback = category.is_fallback;
            let is_editing = edit_index == Some(index);
            let category_id = category.id.clone();
            let name = category.name.clone();
            let directory = category.directory.clone();
            let extensions = category.extensions.clone();
            let ext_line = if extensions.trim().is_empty() {
                extensions_hint.clone()
            } else {
                format!("{extensions_prefix}: {extensions}")
            };
            list = list.child(
                div()
                    .id(SharedString::from(format!("category-row-{category_id}")))
                    .flex()
                    .flex_col()
                    .gap_1()
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
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .flex()
                                    .items_center()
                                    .gap_2()
                                    .child(
                                        Icon::new(category_icon(&name))
                                            .size(IconSize::Small)
                                            .color(if is_editing {
                                                colors.accent
                                            } else {
                                                colors.text_muted
                                            }),
                                    )
                                    .child(
                                        div()
                                            .text_sm()
                                            .font_weight(gpui::FontWeight::MEDIUM)
                                            .text_color(colors.text_primary)
                                            .child(category_display_name(&name, &self.translator)),
                                    ),
                            )
                            .when(is_fallback, |row| {
                                row.child(
                                    div()
                                        .text_xs()
                                        .font_weight(gpui::FontWeight::MEDIUM)
                                        .px_2()
                                        .py_0p5()
                                        .rounded_full()
                                        .bg(with_alpha(colors.accent, 0.12))
                                        .text_color(colors.accent)
                                        .child(fallback_badge.clone()),
                                )
                            })
                            .child({
                                let mut button = KitButton::new(SharedString::from(format!(
                                    "category-edit-{category_id}"
                                )))
                                .small()
                                .compact()
                                .icon(gpui_kit::component::Icon::new(IconName::Pencil))
                                .label(edit_label.clone())
                                .disabled(pending)
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    this.select_category_editor(index, window, cx);
                                }));
                                if is_editing {
                                    button = button.primary();
                                }
                                button
                            })
                            .when(!is_fallback, |row| {
                                row.child(
                                    KitButton::new(SharedString::from(format!(
                                        "category-remove-{category_id}"
                                    )))
                                    .ghost()
                                    .small()
                                    .compact()
                                    .icon(gpui_kit::component::Icon::new(IconName::Trash2))
                                    .label(remove_label.clone())
                                    .disabled(pending || categories.len() <= 1)
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.remove_download_category(index, cx);
                                    })),
                                )
                            }),
                    )
                    .when(!is_editing, |row| {
                        row.child(div().text_xs().text_color(colors.text_muted).child(
                            if directory.is_empty() {
                                no_dir.clone()
                            } else {
                                directory
                            },
                        ))
                        .child(
                            div()
                                .text_xs()
                                .text_color(colors.text_muted)
                                .child(ext_line),
                        )
                    })
                    .when(is_editing, |row| {
                        row.child(
                            div()
                                .flex()
                                .flex_col()
                                .gap_2()
                                .pt_1()
                                .child(settings_labeled_input(
                                    self.t("settings-category-name"),
                                    &self.settings_inputs.category_name,
                                    colors,
                                ))
                                .child(settings_labeled_input(
                                    self.t("settings-category-extensions"),
                                    &self.settings_inputs.category_extensions,
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
                                                .text_color(colors.text_muted)
                                                .child(self.t("settings-category-directory")),
                                        )
                                        .child(Self::settings_path_field_row(
                                            &self.settings_inputs.directory,
                                            "browse-category-directory",
                                            browse_label.clone(),
                                            self.t("settings-category-directory-browse-aria"),
                                            PathPickTarget::CategoryDirectory {
                                                category_id: category_id.clone(),
                                            },
                                            cx,
                                        )),
                                ),
                        )
                    }),
            );
        }

        settings_section_owned(title, colors)
            .child(div().text_xs().text_color(colors.text_muted).child(desc))
            .child(list)
            .child(
                div().mt_2().child(
                    KitButton::new("add-download-category")
                        .icon(gpui_kit::component::Icon::new(IconName::Plus))
                        .label(add_label)
                        .disabled(pending || self.settings_page.draft_categories.len() >= 32)
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.add_download_category(window, cx);
                        })),
                ),
            )
    }
}
