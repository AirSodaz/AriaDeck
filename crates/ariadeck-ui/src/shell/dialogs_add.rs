//! Add-download dialog child entity and `AppShell` coordination.

use ariadeck_i18n::FluentValue;

use super::*;

fn fluent_number(value: usize) -> i64 {
    i64::try_from(value).unwrap_or(i64::MAX)
}

fn metadata_selection_state(preview: &AddDownloadMetadataPreviewView) -> Toggled {
    if preview.selected_file_indices.is_empty() {
        Toggled::False
    } else if preview.selected_file_indices.len() == preview.files.len() {
        Toggled::True
    } else {
        Toggled::Mixed
    }
}

struct MetadataPreviewRowContext<'a> {
    preview_index: usize,
    preview_key: &'a str,
    position: usize,
    active_file_count: usize,
    selected: bool,
    pending: bool,
    colors: crate::ThemeColors,
}

fn filename_hint_for_add(
    sources: &[AddDownloadSourceView],
    metadata_name: Option<&str>,
) -> Option<String> {
    if let Some(name) = metadata_name.map(str::trim).filter(|s| !s.is_empty()) {
        return Some(name.to_owned());
    }
    sources.iter().find_map(|source| match source {
        AddDownloadSourceView::Uri { uri, .. } => ariadeck_domain::filename_hint_from_source(uri),
        AddDownloadSourceView::MetadataFile { path, .. } => path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned()),
    })
}

fn resolve_add_category_id(
    categories: &[crate::DownloadCategoryView],
    manual: Option<&str>,
    filename_hint: Option<&str>,
) -> Option<String> {
    if let Some(id) = manual.map(str::trim).filter(|s| !s.is_empty())
        && categories.iter().any(|c| c.id == id)
    {
        return Some(id.to_owned());
    }
    let pairs: Vec<(String, bool, Vec<String>)> = categories
        .iter()
        .map(|c| {
            let extensions = c
                .extensions
                .split(|ch: char| ch == ',' || ch == ';' || ch == '|' || ch.is_whitespace())
                .filter_map(ariadeck_domain::normalize_extension)
                .collect::<Vec<_>>();
            (c.id.clone(), c.is_fallback, extensions)
        })
        .collect();
    let id = ariadeck_domain::resolve_category_id(
        pairs.iter().map(|(id, is_fallback, extensions)| {
            (id.as_str(), *is_fallback, extensions.as_slice())
        }),
        filename_hint,
    )?;
    Some(id.to_owned())
}

fn category_directory(
    categories: &[crate::DownloadCategoryView],
    category_id: Option<&str>,
    fallback_dir: &str,
) -> Option<String> {
    if let Some(id) = category_id
        && let Some(dir) = categories
            .iter()
            .find(|c| c.id == id)
            .map(|c| c.directory.clone())
            .filter(|d| !d.is_empty())
    {
        return Some(dir);
    }
    categories
        .iter()
        .find(|c| c.is_fallback)
        .map(|c| c.directory.clone())
        .filter(|d| !d.is_empty())
        .or_else(|| (!fallback_dir.is_empty()).then(|| fallback_dir.to_owned()))
}

pub(super) enum AddDownloadDialogEvent {
    LinksInputHeightChanged(f32),
    MetadataPathsChosen(Vec<PathBuf>),
}

pub(super) struct AddDownloadDialogView {
    theme: Theme,
    translator: Translator,
    categories: Vec<crate::DownloadCategoryView>,
    download_directory: String,
    pub(super) add_input: Entity<TextareaState>,
    pub(super) add_inputs: AddInputs,
    pub(super) input_mode: AddDownloadInputModeView,
    pub(super) mode: AddDownloadModeView,
    pub(super) file_conflict: FileConflictPolicyView,
    pub(super) category_id: Option<String>,
    pub(super) advanced_open: bool,
    pub(super) from_bridge: bool,
    pub(super) metadata_files: Vec<AddDownloadMetadataPreviewView>,
    pub(super) active_metadata_file: Option<usize>,
    pub(super) preview_pending: Option<PendingMetadataPreview>,
    pub(super) pending: Option<PendingAddDownload>,
    pub(super) error: Option<OperationErrorView>,
    pub(super) results: Vec<AddDownloadItemResultView>,
    pub(super) links_input_height: f32,
    metadata_file_scroll: UniformListScrollHandle,
    metadata_source_scroll: ScrollHandle,
    results_scroll: ScrollHandle,
    _input_subs: Vec<Subscription>,
}

impl gpui::EventEmitter<AddDownloadDialogEvent> for AddDownloadDialogView {}

impl AddDownloadDialogView {
    pub(super) fn new(
        theme: Theme,
        translator: Translator,
        categories: Vec<crate::DownloadCategoryView>,
        download_directory: String,
        links_input_height: f32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let add_placeholder = translator.t("dialog-add-url-placeholder");
        let add_input = cx.new(|cx| TextareaState::new(window, cx).placeholder(add_placeholder));
        let add_referer_input = make_input("https://example.test/page".into(), false, window, cx);
        let add_user_agent_input = make_input(
            translator.t("dialog-add-user-agent-placeholder").into(),
            false,
            window,
            cx,
        );
        let add_headers_input = make_input(
            translator.t("dialog-add-headers-placeholder").into(),
            false,
            window,
            cx,
        );
        let add_cookie_input = make_input("session=…".into(), true, window, cx);
        let add_http_user_input = make_input(
            translator.t("dialog-add-http-username-placeholder").into(),
            false,
            window,
            cx,
        );
        let add_http_passwd_input = make_input(
            translator.t("dialog-add-http-password-placeholder").into(),
            true,
            window,
            cx,
        );
        let add_checksum_input = make_input("sha-256=…".into(), false, window, cx);

        let mut input_subs = Vec::with_capacity(8);
        input_subs.push(cx.subscribe(
            &add_input,
            |this: &mut Self, _input, event: &InputEvent, cx| {
                if let InputEvent::Change = event
                    && this.pending.is_none()
                {
                    let changed = this.error.take().is_some() || !this.results.is_empty();
                    this.results.clear();
                    if changed {
                        cx.notify();
                    }
                }
            },
        ));
        for input in [
            &add_referer_input,
            &add_user_agent_input,
            &add_headers_input,
            &add_cookie_input,
            &add_http_user_input,
            &add_http_passwd_input,
            &add_checksum_input,
        ] {
            input_subs.push(cx.subscribe(
                input,
                |this: &mut Self, _input, event: &InputEvent, cx| {
                    if let InputEvent::Change = event
                        && this.pending.is_none()
                        && this.error.take().is_some()
                    {
                        cx.notify();
                    }
                },
            ));
        }

        Self {
            theme,
            translator,
            categories,
            download_directory,
            add_input,
            add_inputs: AddInputs {
                referer: add_referer_input,
                user_agent: add_user_agent_input,
                headers: add_headers_input,
                cookie: add_cookie_input,
                http_user: add_http_user_input,
                http_passwd: add_http_passwd_input,
                checksum: add_checksum_input,
            },
            input_mode: AddDownloadInputModeView::Links,
            mode: AddDownloadModeView::SeparateTasks,
            file_conflict: FileConflictPolicyView::AutoRename,
            category_id: None, // None = auto-route by extension (D-042)
            advanced_open: false,
            from_bridge: false,
            metadata_files: Vec::new(),
            active_metadata_file: None,
            preview_pending: None,
            pending: None,
            error: None,
            results: Vec::new(),
            links_input_height,
            metadata_file_scroll: UniformListScrollHandle::new(),
            metadata_source_scroll: ScrollHandle::new(),
            results_scroll: ScrollHandle::new(),
            _input_subs: input_subs,
        }
    }

    pub(super) fn sync_context(
        &mut self,
        theme: Theme,
        translator: Translator,
        categories: Vec<crate::DownloadCategoryView>,
        download_directory: String,
        window: Option<&mut Window>,
        cx: &mut Context<Self>,
    ) {
        self.theme = theme;
        self.translator = translator;
        self.categories = categories;
        self.download_directory = download_directory;
        if let Some(category_id) = self.category_id.as_deref()
            && !self.categories.iter().any(|c| c.id == category_id)
        {
            self.category_id = None;
        }
        if let Some(window) = window {
            self.refresh_placeholders(window, cx);
        }
        cx.notify();
    }

    pub(super) fn refresh_placeholders(&self, window: &mut Window, cx: &mut Context<Self>) {
        let add_placeholder = self.t("dialog-add-url-placeholder");
        self.add_input.update(cx, |input, cx| {
            input.set_placeholder(add_placeholder, window, cx);
        });
        for (entity, key) in [
            (
                &self.add_inputs.user_agent,
                "dialog-add-user-agent-placeholder",
            ),
            (&self.add_inputs.headers, "dialog-add-headers-placeholder"),
            (
                &self.add_inputs.http_user,
                "dialog-add-http-username-placeholder",
            ),
            (
                &self.add_inputs.http_passwd,
                "dialog-add-http-password-placeholder",
            ),
        ] {
            let text = self.t(key);
            entity.update(cx, |input, cx| input.set_placeholder(text, window, cx));
        }
    }

    pub(super) fn is_busy(&self) -> bool {
        self.pending.is_some() || self.preview_pending.is_some()
    }

    pub(super) fn dialog_width(&self) -> f32 {
        if self.input_mode == AddDownloadInputModeView::MetadataFiles {
            720.0
        } else {
            600.0
        }
    }

    pub(super) fn collect_advanced_options(&self, cx: &App) -> AddDownloadAdvancedOptionsView {
        let cookie = self.add_inputs.cookie.read(cx).value().trim().to_owned();
        let http_passwd = self.add_inputs.http_passwd.read(cx).value().to_string();
        AddDownloadAdvancedOptionsView {
            referer: self.add_inputs.referer.read(cx).value().trim().to_owned(),
            user_agent: self
                .add_inputs
                .user_agent
                .read(cx)
                .value()
                .trim()
                .to_owned(),
            headers: self.add_inputs.headers.read(cx).value().to_string(),
            cookie: (!cookie.is_empty()).then(|| SecretStringView::new(cookie)),
            http_user: self.add_inputs.http_user.read(cx).value().trim().to_owned(),
            http_passwd: (!http_passwd.is_empty()).then(|| SecretStringView::new(http_passwd)),
            checksum: self.add_inputs.checksum.read(cx).value().trim().to_owned(),
        }
    }

    pub(super) fn set_mode(&mut self, mode: AddDownloadModeView, cx: &mut Context<Self>) {
        if self.is_busy() || self.mode == mode {
            return;
        }
        self.mode = mode;
        self.error = None;
        self.results.clear();
        cx.notify();
    }

    pub(super) fn set_input_mode(
        &mut self,
        mode: AddDownloadInputModeView,
        cx: &mut Context<Self>,
    ) {
        if self.is_busy() || self.input_mode == mode {
            return;
        }
        self.input_mode = mode;
        self.error = None;
        self.results.clear();
        cx.notify();
    }

    pub(super) fn set_file_conflict_policy(
        &mut self,
        policy: FileConflictPolicyView,
        cx: &mut Context<Self>,
    ) {
        if self.pending.is_some() || self.file_conflict == policy {
            return;
        }
        self.file_conflict = policy;
        self.error = None;
        self.results.clear();
        cx.notify();
    }

    pub(super) fn set_category(&mut self, category_id: Option<String>, cx: &mut Context<Self>) {
        self.category_id = category_id;
        cx.notify();
    }

    pub(super) fn toggle_advanced(&mut self, cx: &mut Context<Self>) {
        if self.is_busy() {
            return;
        }
        self.advanced_open = !self.advanced_open;
        cx.notify();
    }

    pub(super) fn set_error(&mut self, summary: String, cx: &mut Context<Self>) {
        self.error = Some(OperationErrorView {
            code: "filesystem.operation_failed".into(),
            summary,
            retryable: true,
        });
        cx.notify();
    }

    pub(super) fn remove_metadata_file(&mut self, index: usize, cx: &mut Context<Self>) {
        if self.is_busy() || index >= self.metadata_files.len() {
            return;
        }
        self.metadata_files.remove(index);
        self.active_metadata_file = if self.metadata_files.is_empty() {
            None
        } else {
            Some(
                self.active_metadata_file
                    .unwrap_or_default()
                    .min(self.metadata_files.len() - 1),
            )
        };
        self.error = None;
        self.results.clear();
        cx.notify();
    }

    pub(super) fn select_metadata_file(&mut self, index: usize, cx: &mut Context<Self>) {
        if !self.is_busy()
            && index < self.metadata_files.len()
            && self.active_metadata_file != Some(index)
        {
            self.active_metadata_file = Some(index);
            cx.notify();
        }
    }

    pub(super) fn toggle_metadata_file_entry(
        &mut self,
        preview_index: usize,
        file_index: u32,
        cx: &mut Context<Self>,
    ) {
        if self.is_busy() {
            return;
        }
        let Some(preview) = self.metadata_files.get_mut(preview_index) else {
            return;
        };
        match preview.selected_file_indices.binary_search(&file_index) {
            Ok(position) => {
                preview.selected_file_indices.remove(position);
            }
            Err(position) if preview.files.iter().any(|file| file.index == file_index) => {
                preview.selected_file_indices.insert(position, file_index);
            }
            Err(_) => return,
        }
        self.error = None;
        self.results.clear();
        cx.notify();
    }

    pub(super) fn toggle_all_metadata_file_entries(
        &mut self,
        preview_index: usize,
        cx: &mut Context<Self>,
    ) {
        if self.is_busy() {
            return;
        }
        let Some(preview) = self.metadata_files.get_mut(preview_index) else {
            return;
        };
        if preview.selected_file_indices.len() == preview.files.len() {
            preview.selected_file_indices.clear();
        } else {
            preview.selected_file_indices = preview.files.iter().map(|file| file.index).collect();
        }
        self.error = None;
        self.results.clear();
        cx.notify();
    }

    fn choose_metadata_files(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.is_busy() {
            return;
        }
        let selected = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: true,
            prompt: Some(self.t("dialog-add-choose-metadata").into()),
        });
        cx.spawn_in(window, async move |this, cx| {
            let selected = selected.await;
            let _ = this.update_in(cx, |this, _window, cx| match selected {
                Ok(Ok(Some(paths))) => {
                    cx.emit(AddDownloadDialogEvent::MetadataPathsChosen(paths));
                }
                Ok(Ok(None)) => {}
                Ok(Err(error)) => {
                    this.set_error(format!("File picker failed: {error}"), cx);
                }
                Err(error) => {
                    this.set_error(format!("File picker closed unexpectedly: {error}"), cx);
                }
            });
        })
        .detach();
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

    fn t_count(&self, id: &str, n: u64) -> String {
        self.translator.t_count(id, n)
    }

    fn te(&self, error: &OperationErrorView) -> String {
        error.localized_summary(&self.translator)
    }

    fn render_category_picker(
        &self,
        links: &[AddDownloadSourceView],
        cx: &mut Context<Self>,
    ) -> Div {
        let colors = self.theme.colors;
        let pending = self.is_busy();
        if self.categories.is_empty() {
            return div();
        }
        let metadata_name = self.metadata_files.first().and_then(|file| {
            file.path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
        });
        let hint = filename_hint_for_add(links, metadata_name.as_deref());
        let auto_id = resolve_add_category_id(&self.categories, None, hint.as_deref());
        let auto_name = auto_id
            .as_deref()
            .and_then(|id| {
                self.categories
                    .iter()
                    .find(|c| c.id == id)
                    .map(|c| c.name.clone())
            })
            .unwrap_or_else(|| self.t("dialog-add-category-auto"));
        let selected = self.category_id.clone();
        let label = self.t("dialog-add-category");
        let auto_display = category_display_name(&auto_name, &self.translator);
        let auto_label = self.t_args(
            "dialog-add-category-auto-named",
            &[("name", FluentValue::from(auto_display.as_str()))],
        );
        let mut row = div()
            .flex()
            .flex_wrap()
            .items_center()
            .gap_1p5()
            .child(div().text_xs().text_color(colors.text_muted).child(label))
            .child({
                let mut button = KitButton::new("add-category-auto")
                    .small()
                    .compact()
                    .label(auto_label)
                    .disabled(pending)
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.set_category(None, cx);
                    }));
                if selected.is_none() {
                    button = button.primary();
                }
                button
            });
        for category in &self.categories {
            let id = category.id.clone();
            let is_selected = selected.as_deref() == Some(id.as_str());
            let display_name = category_display_name(&category.name, &self.translator);
            row = row.child({
                let mut button = KitButton::new(SharedString::from(format!("add-category-{id}")))
                    .small()
                    .compact()
                    .label(display_name)
                    .disabled(pending)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.set_category(Some(id.clone()), cx);
                    }));
                if is_selected {
                    button = button.primary();
                }
                button
            });
        }
        row
    }

    fn render_advanced_section(
        &self,
        pending: bool,
        colors: crate::ThemeColors,
        cx: &mut Context<Self>,
    ) -> Collapsible {
        let open = self.advanced_open;
        let trigger = div()
            .id("add-download-advanced-toggle")
            .role(Role::Button)
            .aria_label(if open {
                self.t("dialog-add-hide-advanced-aria")
            } else {
                self.t("dialog-add-show-advanced-aria")
            })
            .aria_expanded(open)
            .focusable()
            .tab_stop(true)
            .flex()
            .items_center()
            .justify_between()
            .gap_2()
            .rounded_md()
            .border_1()
            .border_color(colors.border)
            .focus_visible(|style| style.border_color(colors.focus_ring))
            .bg(colors.elevated_surface)
            .px_3()
            .py_2()
            .hover(|style| style.bg(colors.surface_hover))
            .active(|style| style.bg(colors.surface_active))
            .on_key_down(cx.listener(|this, event: &gpui::KeyDownEvent, _, cx| {
                if is_activation_key(event) {
                    cx.stop_propagation();
                    this.toggle_advanced(cx);
                }
            }))
            .on_click(cx.listener(|this, _, _, cx| this.toggle_advanced(cx)))
            .child(
                div()
                    .text_xs()
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(colors.text_secondary)
                    .child(self.t("ui-advanced-options")),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_1()
                    .text_xs()
                    .text_color(colors.text_muted)
                    .child(if open {
                        self.t("dialog-add-hide-advanced")
                    } else {
                        self.t("dialog-add-show-advanced")
                    })
                    .child(
                        Icon::new(if open {
                            IconName::ChevronUp
                        } else {
                            IconName::ChevronDown
                        })
                        .size(IconSize::XSmall),
                    ),
            );
        let body = div()
            .flex()
            .flex_col()
            .gap_2()
            .pt_2()
            .child(
                div()
                    .text_xs()
                    .text_color(colors.text_muted)
                    .child(self.t("dialog-add-advanced-hint")),
            )
            .child(
                div()
                    .flex()
                    .gap_3()
                    .child(
                        settings_labeled_input(
                            self.t("dialog-add-referer"),
                            &self.add_inputs.referer,
                            colors,
                        )
                        .flex_1()
                        .min_w_0(),
                    )
                    .child(
                        settings_labeled_input(
                            self.t("dialog-add-user-agent"),
                            &self.add_inputs.user_agent,
                            colors,
                        )
                        .flex_1()
                        .min_w_0(),
                    ),
            )
            .child(settings_labeled_input(
                self.t("dialog-add-custom-headers"),
                &self.add_inputs.headers,
                colors,
            ))
            .child(settings_labeled_input(
                self.t("dialog-add-cookie"),
                &self.add_inputs.cookie,
                colors,
            ))
            .child(
                div()
                    .flex()
                    .gap_3()
                    .child(
                        settings_labeled_input(
                            self.t("dialog-add-http-username"),
                            &self.add_inputs.http_user,
                            colors,
                        )
                        .flex_1()
                        .min_w_0(),
                    )
                    .child(
                        settings_labeled_input(
                            self.t("dialog-add-http-password"),
                            &self.add_inputs.http_passwd,
                            colors,
                        )
                        .flex_1()
                        .min_w_0(),
                    ),
            )
            .child(settings_labeled_input(
                self.t("dialog-add-checksum"),
                &self.add_inputs.checksum,
                colors,
            ))
            .when(pending, |element| element);

        Collapsible::new()
            .motion_id("add-download-advanced")
            .open(open)
            .child(trigger)
            .content(body)
    }

    fn render_metadata_file_picker(
        &self,
        pending: bool,
        preview_pending: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let colors = self.theme.colors;
        let rows = self
            .metadata_files
            .iter()
            .enumerate()
            .map(|(index, preview)| {
                self.render_metadata_source_row(index, preview, pending, colors, cx)
            })
            .collect::<Vec<_>>();
        let count = self.metadata_files.len();
        let active_index = self.active_metadata_file;
        let active_summary = active_index
            .and_then(|index| self.metadata_files.get(index))
            .map(|preview| self.localized_metadata_selection_summary(preview));
        let active_file_count = active_index
            .and_then(|index| self.metadata_files.get(index))
            .map_or(0, |preview| preview.files.len());
        let active_selection_state = active_index
            .and_then(|index| self.metadata_files.get(index))
            .map_or(Toggled::False, metadata_selection_state);
        let active_selection_icon = match active_selection_state {
            Toggled::False => IconName::Square,
            Toggled::True => IconName::SquareCheckBig,
            Toggled::Mixed => IconName::SquareMinus,
        };
        let file_list = active_index.and_then(|preview_index| {
            self.render_metadata_preview_list(preview_index, active_file_count, pending, colors, cx)
        });

        div()
            .flex()
            .flex_col()
            .gap_2()
            .child(self.render_metadata_drop_target(count, pending, preview_pending, colors, cx))
            .when(!rows.is_empty(), |element| {
                let metadata_source_scroll = self.metadata_source_scroll.clone();
                element.child(
                    div()
                        .relative()
                        .max_h(px(112.0))
                        .overflow_hidden()
                        .border_1()
                        .border_color(colors.border)
                        .rounded_md()
                        .child(
                            div()
                                .id("metadata-file-list")
                                .role(Role::List)
                                .aria_label(self.t("dialog-add-selected-files-aria"))
                                .max_h(px(112.0))
                                .overflow_y_scroll()
                                .track_scroll(&metadata_source_scroll)
                                .children(rows),
                        )
                        .vertical_scrollbar(&metadata_source_scroll),
                )
            })
            .when_some(active_summary, |element, summary| {
                element
                    .child(
                        div()
                            .h(px(36.0))
                            .flex_none()
                            .flex()
                            .items_center()
                            .justify_between()
                            .gap_3()
                            .child(
                                div()
                                    .id("toggle-all-metadata-files")
                                    .role(Role::CheckBox)
                                    .aria_toggled(active_selection_state)
                                    .aria_label(match active_selection_state {
                                        Toggled::True => self.t("dialog-add-clear-file-selection"),
                                        Toggled::False | Toggled::Mixed => self.t("select-all"),
                                    })
                                    .focusable()
                                    .tab_stop(!pending)
                                    .border_1()
                                    .border_color(gpui::transparent_black())
                                    .focus_visible(|style| style.border_color(colors.focus_ring))
                                    .rounded_sm()
                                    .px_1p5()
                                    .py_0p5()
                                    .flex()
                                    .items_center()
                                    .gap_2()
                                    .hover(|style| style.bg(colors.surface_hover))
                                    .active(|style| style.bg(colors.surface_active))
                                    .on_key_down(cx.listener(
                                        move |this, event: &gpui::KeyDownEvent, _, cx| {
                                            if is_activation_key(event)
                                                && let Some(index) = active_index
                                            {
                                                cx.stop_propagation();
                                                this.toggle_all_metadata_file_entries(index, cx);
                                            }
                                        },
                                    ))
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        if let Some(index) = active_index {
                                            this.toggle_all_metadata_file_entries(index, cx);
                                        }
                                    }))
                                    .child(
                                        Icon::new(active_selection_icon)
                                            .size(IconSize::Small)
                                            .color(colors.accent),
                                    )
                                    .child(
                                        div()
                                            .text_xs()
                                            .font_weight(FontWeight::MEDIUM)
                                            .text_color(colors.text_secondary)
                                            .child(self.t("ui-files")),
                                    ),
                            )
                            .child(
                                div()
                                    .font_features(tabular_numbers())
                                    .text_xs()
                                    .text_color(colors.text_muted)
                                    .child(summary),
                            ),
                    )
                    .when_some(file_list, |element, list| element.child(list))
            })
            .into_any_element()
    }

    fn render_metadata_drop_target(
        &self,
        count: usize,
        pending: bool,
        preview_pending: bool,
        colors: crate::ThemeColors,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        div()
            .id("metadata-file-drop-target")
            .role(Role::Group)
            .aria_label(self.t("dialog-add-drop-target-aria"))
            .min_h(px(82.0))
            .flex()
            .items_center()
            .justify_between()
            .gap_3()
            .p_3()
            .border_1()
            .border_color(colors.border)
            .rounded_md()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        Icon::new(IconName::Inbox)
                            .size(IconSize::Medium)
                            .color(colors.text_muted),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .child(self.t("ui-torrent-metalink-files"))
                            .child(div().text_xs().text_color(colors.text_muted).child(
                                if preview_pending {
                                    self.t("dialog-add-reading-metadata")
                                } else {
                                    self.t_count(
                                        "dialog-add-sources-ready",
                                        u64::try_from(count).unwrap_or(u64::MAX),
                                    )
                                },
                            )),
                    ),
            )
            .child(
                KitButton::new("choose-metadata-files")
                    .icon(gpui_kit::component::Icon::new(IconName::FolderDown))
                    .label(self.t("dialog-add-choose-metadata"))
                    .accessibility_label(self.t("dialog-add-choose-files-aria"))
                    .disabled(pending)
                    .loading(preview_pending)
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.choose_metadata_files(window, cx);
                    })),
            )
    }

    fn render_metadata_source_row(
        &self,
        index: usize,
        preview: &AddDownloadMetadataPreviewView,
        pending: bool,
        colors: crate::ThemeColors,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let active = self.active_metadata_file == Some(index);
        let name = preview.path.file_name().map_or_else(
            || preview.path.display().to_string(),
            |name| name.to_string_lossy().into(),
        );
        let full_path = preview.path.display().to_string();
        let kind_label = self.t(preview.kind.message_key());
        let selected = preview.selected_file_indices.len();
        let total = preview.files.len();
        let row_aria = self.t_args(
            "dialog-add-metadata-row-aria",
            &[
                ("kind", FluentValue::from(kind_label.clone())),
                ("name", FluentValue::from(name.clone())),
                ("selected", FluentValue::from(fluent_number(selected))),
                ("total", FluentValue::from(fluent_number(total))),
            ],
        );
        let row_summary = self.t_args(
            "dialog-add-metadata-row-summary",
            &[
                ("kind", FluentValue::from(kind_label.clone())),
                ("selected", FluentValue::from(fluent_number(selected))),
                ("total", FluentValue::from(fluent_number(total))),
                ("path", FluentValue::from(full_path)),
            ],
        );
        let row_key = metadata_path_key(&preview.path);
        let row_id = stable_id("metadata-file", &row_key);
        let remove_id = stable_id("remove-metadata-file", &row_key);
        let remove_aria = self.t_args(
            "dialog-add-remove-kind-aria",
            &[("kind", FluentValue::from(kind_label))],
        );
        div()
            .id(row_id)
            .role(Role::ListItem)
            .aria_label(row_aria)
            .focusable()
            .tab_stop(!pending)
            .border_1()
            .border_color(if active { colors.accent } else { colors.border })
            .focus_visible(|style| style.border_color(colors.focus_ring))
            .h(px(48.0))
            .flex_none()
            .flex()
            .items_center()
            .gap_2()
            .px_2()
            .bg(if active {
                colors.surface_active
            } else {
                colors.surface
            })
            .hover(|style| style.bg(colors.surface_hover))
            .active(|style| style.bg(colors.surface_active))
            .on_key_down(cx.listener(move |this, event: &gpui::KeyDownEvent, _, cx| {
                if is_activation_key(event) {
                    cx.stop_propagation();
                    this.select_metadata_file(index, cx);
                }
            }))
            .on_click(cx.listener(move |this, _, _, cx| {
                this.select_metadata_file(index, cx);
            }))
            .child(
                Icon::new(IconName::Download)
                    .size(IconSize::Small)
                    .color(if active {
                        colors.accent
                    } else {
                        colors.text_muted
                    }),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .child(div().truncate().text_sm().child(name))
                    .child(
                        div()
                            .truncate()
                            .text_xs()
                            .text_color(colors.text_muted)
                            .child(row_summary),
                    ),
            )
            .child(
                KitButton::new(remove_id)
                    .ghost()
                    .small()
                    .compact()
                    .icon(gpui_kit::component::Icon::new(IconName::X))
                    .accessibility_label(remove_aria)
                    .disabled(pending)
                    .tooltip(self.t("dialog-add-remove-file"))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.remove_metadata_file(index, cx);
                    })),
            )
            .into_any_element()
    }

    fn render_metadata_preview_list(
        &self,
        preview_index: usize,
        active_file_count: usize,
        pending: bool,
        colors: crate::ThemeColors,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let preview_key = self
            .metadata_files
            .get(preview_index)
            .map(|preview| metadata_path_key(&preview.path))?;
        let list_id = stable_id("metadata-preview-files", &preview_key);
        Some(
            div()
                .relative()
                .h(px(220.0))
                .min_h_0()
                .min_w_0()
                .overflow_hidden()
                .child(
                    uniform_list(
                        list_id,
                        active_file_count,
                        cx.processor(move |this, range: Range<usize>, _window, cx| {
                            let colors = this.theme.colors;
                            let Some(preview) = this.metadata_files.get(preview_index) else {
                                return Vec::new();
                            };
                            let preview_key = metadata_path_key(&preview.path);
                            range
                                .filter_map(|position| {
                                    let file = preview.files.get(position)?.clone();
                                    let selected = preview
                                        .selected_file_indices
                                        .binary_search(&file.index)
                                        .is_ok();
                                    Some(this.render_metadata_preview_file_row(
                                        file,
                                        MetadataPreviewRowContext {
                                            preview_index,
                                            preview_key: &preview_key,
                                            position,
                                            active_file_count,
                                            selected,
                                            pending,
                                            colors,
                                        },
                                        cx,
                                    ))
                                })
                                .collect::<Vec<_>>()
                        }),
                    )
                    .track_scroll(&self.metadata_file_scroll)
                    .size_full(),
                )
                .vertical_scrollbar(&self.metadata_file_scroll)
                .border_1()
                .border_color(colors.border)
                .rounded_md()
                .into_any_element(),
        )
    }

    fn render_metadata_preview_file_row(
        &self,
        file: crate::AddDownloadMetadataFileView,
        row: MetadataPreviewRowContext<'_>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let MetadataPreviewRowContext {
            preview_index,
            preview_key,
            position,
            active_file_count,
            selected,
            pending,
            colors,
        } = row;
        let file_index = file.index;
        let size_label = file
            .length
            .map_or_else(|| self.t("dialog-add-size-unknown"), format_bytes);
        let row_aria = self.t_args(
            "dialog-add-file-row-aria",
            &[
                ("index", FluentValue::from(i64::from(file_index))),
                ("path", FluentValue::from(file.path.as_str())),
                ("size", FluentValue::from(size_label.as_str())),
            ],
        );
        div()
            .id(stable_id(
                "metadata-preview-file",
                &format!("{preview_key}:{file_index}"),
            ))
            .role(Role::CheckBox)
            .aria_position_in_set(position + 1)
            .aria_size_of_set(active_file_count)
            .aria_toggled(if selected {
                Toggled::True
            } else {
                Toggled::False
            })
            .aria_label(row_aria)
            .focusable()
            .tab_stop(!pending)
            .relative()
            .border_1()
            .border_color(gpui::transparent_black())
            .focus_visible(|style| style.border_color(colors.focus_ring))
            .h(px(40.0))
            .w_full()
            .flex_none()
            .flex()
            .items_center()
            .gap_2()
            .px_3()
            .hover(|style| style.bg(colors.surface_hover))
            .active(|style| style.bg(colors.surface_active))
            .on_key_down(cx.listener(move |this, event: &gpui::KeyDownEvent, _, cx| {
                if is_activation_key(event) {
                    cx.stop_propagation();
                    this.toggle_metadata_file_entry(preview_index, file_index, cx);
                }
            }))
            .on_click(cx.listener(move |this, _, _, cx| {
                this.toggle_metadata_file_entry(preview_index, file_index, cx);
            }))
            .child(
                div()
                    .absolute()
                    .left_0()
                    .right_0()
                    .bottom_0()
                    .h(px(1.0))
                    .bg(colors.border),
            )
            .child(
                div()
                    .flex_none()
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .child(
                        Checkbox::new(stable_id(
                            "metadata-preview-file-check",
                            &format!("{preview_key}:{file_index}"),
                        ))
                        .checked(selected)
                        .disabled(pending)
                        .tab_stop(false)
                        .on_change(cx.listener(move |this, _, _, cx| {
                            this.toggle_metadata_file_entry(preview_index, file_index, cx);
                        })),
                    ),
            )
            .child(
                div()
                    .w(px(34.0))
                    .flex_none()
                    .font_features(tabular_numbers())
                    .text_xs()
                    .text_color(colors.text_muted)
                    .child(file_index.to_string()),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        Icon::new(file_type_icon(&file.path))
                            .size(IconSize::Small)
                            .color(colors.text_muted),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_xs()
                            .text_color(colors.text_secondary)
                            .child(file.path),
                    ),
            )
            .child(
                div()
                    .w(px(84.0))
                    .flex_none()
                    .text_right()
                    .font_features(tabular_numbers())
                    .text_xs()
                    .text_color(colors.text_muted)
                    .child(size_label),
            )
            .into_any_element()
    }

    fn localized_metadata_selection_summary(
        &self,
        preview: &AddDownloadMetadataPreviewView,
    ) -> String {
        let mut known_bytes = 0_u64;
        let mut unknown_sizes = 0_usize;
        for file in &preview.files {
            if preview
                .selected_file_indices
                .binary_search(&file.index)
                .is_ok()
            {
                if let Some(length) = file.length {
                    known_bytes = known_bytes.saturating_add(length);
                } else {
                    unknown_sizes = unknown_sizes.saturating_add(1);
                }
            }
        }

        let selected = fluent_number(preview.selected_file_indices.len());
        let total = fluent_number(preview.files.len());
        let size = format_bytes(known_bytes);
        if unknown_sizes == 0 {
            self.t_args(
                "dialog-add-selection-summary",
                &[
                    ("selected", FluentValue::from(selected)),
                    ("total", FluentValue::from(total)),
                    ("size", FluentValue::from(size)),
                ],
            )
        } else {
            self.t_args(
                "dialog-add-selection-summary-with-unknown",
                &[
                    ("selected", FluentValue::from(selected)),
                    ("total", FluentValue::from(total)),
                    ("size", FluentValue::from(size)),
                    ("unknown", FluentValue::from(fluent_number(unknown_sizes))),
                ],
            )
        }
    }

    fn localized_add_source_label(&self, source: &AddDownloadSourceView) -> String {
        match source {
            AddDownloadSourceView::Uri { line, uri } => self.t_args(
                "dialog-add-source-uri",
                &[
                    ("line", FluentValue::from(fluent_number(*line))),
                    (
                        "source",
                        FluentValue::from(ariadeck_domain::redact_source_uri(uri)),
                    ),
                ],
            ),
            AddDownloadSourceView::MetadataFile { path, kind, .. } => {
                let name = path.file_name().map_or_else(
                    || path.display().to_string(),
                    |name| name.to_string_lossy().into(),
                );
                self.t_args(
                    "dialog-add-source-metadata",
                    &[
                        ("kind", FluentValue::from(self.t(kind.message_key()))),
                        ("name", FluentValue::from(name)),
                    ],
                )
            }
        }
    }

    fn render_result_list(&self, colors: crate::ThemeColors) -> Div {
        let rows = self
            .results
            .iter()
            .enumerate()
            .map(|(index, item)| {
                let source_label = item
                    .sources
                    .iter()
                    .map(|source| self.localized_add_source_label(source))
                    .collect::<Vec<_>>()
                    .join("  |  ");
                let (icon, label, color) = match &item.outcome {
                    CommandOutcomeView::Success { tasks } => (
                        IconName::CircleCheck,
                        match tasks.as_slice() {
                            [] => self.t("dialog-add-result-accepted"),
                            [task] => self.t_args(
                                "dialog-add-result-accepted-gid",
                                &[("gid", FluentValue::from(task.gid.clone()))],
                            ),
                            tasks => self.t_count(
                                "dialog-add-result-accepted-tasks",
                                u64::try_from(tasks.len()).unwrap_or(u64::MAX),
                            ),
                        },
                        colors.success,
                    ),
                    CommandOutcomeView::Failure(error) if error.outcome_unknown() => {
                        (IconName::TriangleAlert, self.te(error), colors.warning)
                    }
                    CommandOutcomeView::Failure(error) => {
                        (IconName::CircleAlert, self.te(error), colors.danger)
                    }
                };
                let result_key = item
                    .sources
                    .iter()
                    .map(|source| match source {
                        AddDownloadSourceView::Uri { line, uri } => format!("uri:{line}:{uri}"),
                        AddDownloadSourceView::MetadataFile {
                            path,
                            content_sha256,
                            ..
                        } => format!("meta:{content_sha256}:{}", metadata_path_key(path)),
                    })
                    .collect::<Vec<_>>()
                    .join("|");
                let result_id = if result_key.is_empty() {
                    SharedString::from(format!("add-result-{index}"))
                } else {
                    stable_id("add-result", &format!("{result_key}:{index}"))
                };
                div()
                    .id(result_id)
                    .role(Role::ListItem)
                    .flex()
                    .items_start()
                    .gap_2()
                    .child(Icon::new(icon).size(IconSize::XSmall).color(color))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .child(div().truncate().text_xs().child(source_label))
                            .child(div().text_xs().text_color(color).child(label)),
                    )
            })
            .collect::<Vec<_>>();
        let results_scroll = self.results_scroll.clone();
        div()
            .relative()
            .max_h(px(220.0))
            .overflow_hidden()
            .border_1()
            .border_color(colors.border)
            .rounded_md()
            .child(
                div()
                    .id("add-download-results")
                    .role(Role::List)
                    .aria_label(self.t("dialog-add-results-aria"))
                    .max_h(px(220.0))
                    .flex()
                    .flex_col()
                    .gap_2()
                    .p_2()
                    .overflow_y_scroll()
                    .track_scroll(&results_scroll)
                    .children(rows),
            )
            .vertical_scrollbar(&results_scroll)
    }
}

impl Render for AddDownloadDialogView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = self.theme.colors;
        let add_pending = self.pending.is_some();
        let preview_pending = self.preview_pending.is_some();
        let pending = add_pending || preview_pending;
        let error = self.error.clone();
        let sources = parse_add_download_sources(self.add_input.read(cx).value().as_ref());
        let input_mode = self.input_mode;
        let mode = self.mode;
        let file_conflict = self.file_conflict;
        let dialog = cx.entity().downgrade();
        let input_dialog = dialog.clone();
        let mode_dialog = dialog.clone();
        let conflict_dialog = dialog;
        let input_mode_control = SegmentedControl::new(
            "add-download-input-mode",
            [
                Segment::new(self.t(AddDownloadInputModeView::Links.message_key()))
                    .icon(IconName::Link),
                Segment::new(self.t(AddDownloadInputModeView::MetadataFiles.message_key()))
                    .icon(IconName::FolderArchive),
            ],
            usize::from(input_mode == AddDownloadInputModeView::MetadataFiles),
            self.theme,
        )
        .aria_label(self.t("dialog-add-input-type-aria"))
        .disabled(pending)
        .on_select(move |index, _window, cx| {
            let mode = if index == 0 {
                AddDownloadInputModeView::Links
            } else {
                AddDownloadInputModeView::MetadataFiles
            };
            input_dialog
                .update(cx, |this, cx| this.set_input_mode(mode, cx))
                .ok();
        });
        let mode_control = SegmentedControl::new(
            "add-download-mode",
            [
                Segment::new(self.t(AddDownloadModeView::SeparateTasks.message_key())),
                Segment::new(self.t(AddDownloadModeView::Mirrors.message_key())),
            ],
            usize::from(mode == AddDownloadModeView::Mirrors),
            self.theme,
        )
        .aria_label(self.t("dialog-add-mode-aria"))
        .disabled(pending)
        .on_select(move |index, _window, cx| {
            let mode = if index == 0 {
                AddDownloadModeView::SeparateTasks
            } else {
                AddDownloadModeView::Mirrors
            };
            mode_dialog
                .update(cx, |this, cx| this.set_mode(mode, cx))
                .ok();
        });
        let conflict_control = SegmentedControl::new(
            "add-download-file-conflict",
            [
                Segment::new(self.t(FileConflictPolicyView::AutoRename.message_key())),
                Segment::new(self.t(FileConflictPolicyView::Reject.message_key())),
                Segment::new(self.t(FileConflictPolicyView::Overwrite.message_key())),
            ],
            match file_conflict {
                FileConflictPolicyView::AutoRename => 0,
                FileConflictPolicyView::Reject => 1,
                FileConflictPolicyView::Overwrite => 2,
            },
            self.theme,
        )
        .aria_label(self.t("dialog-add-file-conflict-aria"))
        .disabled(pending)
        .on_select(move |index, _window, cx| {
            let policy = match index {
                0 => FileConflictPolicyView::AutoRename,
                1 => FileConflictPolicyView::Reject,
                _ => FileConflictPolicyView::Overwrite,
            };
            conflict_dialog
                .update(cx, |this, cx| {
                    this.set_file_conflict_policy(policy, cx);
                })
                .ok();
        });
        let category_picker = self.render_category_picker(&sources, cx);
        let links_input_height = self.links_input_height;

        div()
            .flex()
            .flex_col()
            .gap_2()
            .on_drag_move(
                cx.listener(|this, e: &DragMoveEvent<ResizeLinkInput>, _window, cx| {
                    let origin = e.drag(cx);
                    let delta: f32 = (e.event.position.y - origin.origin_y).into();
                    let new_h = (origin.origin_h + delta)
                        .clamp(MIN_LINKS_INPUT_HEIGHT, MAX_LINKS_INPUT_HEIGHT);
                    if (new_h - this.links_input_height).abs() > 0.5 {
                        this.links_input_height = new_h;
                        cx.emit(AddDownloadDialogEvent::LinksInputHeightChanged(new_h));
                        cx.notify();
                    }
                }),
            )
            .child(input_mode_control)
            .child(category_picker)
            .child(match input_mode {
                AddDownloadInputModeView::Links => div()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child(
                        div()
                            .text_xs()
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(colors.text_secondary)
                            .child(self.t("dialog-add-url-or-magnet")),
                    )
                    .child(
                        div()
                            .id("links-input-container")
                            .relative()
                            .w_full()
                            .child(
                                Textarea::new(&self.add_input)
                                    .h(px(links_input_height))
                                    .font_family("monospace"),
                            )
                            .child(
                                resize_handle("links-input-resize-grip", Axis::Vertical)
                                    .inside(HandleEdge::Trailing)
                                    .on_mouse_down(
                                        MouseButton::Left,
                                        cx.listener(
                                            move |this, e: &MouseDownEvent, _window, cx| {
                                                if e.click_count == 2 {
                                                    this.links_input_height =
                                                        DEFAULT_LINKS_INPUT_HEIGHT;
                                                    cx.emit(
                                                        AddDownloadDialogEvent::LinksInputHeightChanged(
                                                            DEFAULT_LINKS_INPUT_HEIGHT,
                                                        ),
                                                    );
                                                    cx.notify();
                                                }
                                            },
                                        ),
                                    )
                                    .on_drag(
                                        ResizeLinkInput {
                                            origin_y: Pixels::ZERO,
                                            origin_h: links_input_height,
                                        },
                                        |drag, _, window, cx| {
                                            let mut state = drag.clone();
                                            state.origin_y = window.mouse_position().y;
                                            cx.new(|_| state)
                                        },
                                    ),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .gap_3()
                            .child(div().text_xs().text_color(colors.text_muted).child(
                                if sources.is_empty() {
                                    self.t("dialog-add-no-sources")
                                } else {
                                    self.t_count(
                                        "dialog-add-sources-detected",
                                        u64::try_from(sources.len()).unwrap_or(u64::MAX),
                                    )
                                },
                            ))
                            .child(mode_control),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .gap_3()
                            .child(
                                div()
                                    .text_xs()
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(colors.text_secondary)
                                    .child(self.t("dialog-add-if-file-exists")),
                            )
                            .child(conflict_control),
                    )
                    .when(
                        file_conflict == FileConflictPolicyView::Overwrite,
                        |element| {
                            element.child(
                                Alert::warning(
                                    "add-download-overwrite-warning",
                                    self.t("ui-overwrite-warning"),
                                )
                                .xsmall(),
                            )
                        },
                    )
                    .child(self.render_advanced_section(pending, colors, cx))
                    .into_any_element(),
                AddDownloadInputModeView::MetadataFiles => {
                    self.render_metadata_file_picker(pending, preview_pending, cx)
                }
            })
            .when(!self.results.is_empty(), |element| {
                element.child(self.render_result_list(colors))
            })
            .when_some(error, |element, error| {
                let message = self.te(&error);
                element.child(Alert::error("add-download-error", message).xsmall())
            })
    }
}

impl AppShell {
    pub(super) fn add_dialog_view(&self) -> Option<&Entity<AddDownloadDialogView>> {
        match &self.active_modal {
            Some(ActiveModal::AddDownload { view, .. }) => Some(view),
            _ => None,
        }
    }

    pub(super) fn set_add_dialog_busy(&mut self, cx: &App) {
        if let Some(ActiveModal::AddDownload { view, busy, .. }) = &mut self.active_modal {
            *busy = view.read(cx).is_busy();
        }
    }

    pub(crate) fn open_add_download(
        &mut self,
        _: &OpenAddDownload,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.leave_settings_page();
        self.active_status_popover = None;
        if let Some(view) = self.add_dialog_view().cloned() {
            let focus = view.read(cx).add_input.focus_handle(cx);
            window.focus(&focus, cx);
            return;
        }
        if self.active_modal.is_some()
            || self.pending_task_command.is_some()
            || self.pending_batch_command.is_some()
        {
            return;
        }
        if !self.snapshot.commands_available() {
            self.show_notice(self.t("notice-connect-before-add"), true, cx);
            return;
        }

        let theme = self.theme;
        let translator = self.translator.clone();
        let categories = self.settings.categories.clone();
        let download_directory = self.settings.download_directory.clone();
        let links_input_height = self.links_input_height;
        let view = cx.new(|cx| {
            AddDownloadDialogView::new(
                theme,
                translator,
                categories,
                download_directory,
                links_input_height,
                window,
                cx,
            )
        });
        let sub = cx.subscribe_in(
            &view,
            window,
            |this, _view, event, window, cx| match event {
                AddDownloadDialogEvent::LinksInputHeightChanged(height) => {
                    this.links_input_height = *height;
                }
                AddDownloadDialogEvent::MetadataPathsChosen(paths) => {
                    let _ = this.open_metadata_paths(paths.clone(), window, cx);
                }
            },
        );
        self.active_modal = Some(ActiveModal::AddDownload {
            view: view.clone(),
            busy: false,
            _sub: sub,
        });
        self.sync_kit_modals(window, cx);
        cx.notify();
        if !has_kit_root(window) {
            cx.defer_in(window, move |this, window, cx| {
                if let Some(active) = this.add_dialog_view()
                    && active == &view
                    && active.read(cx).input_mode == AddDownloadInputModeView::Links
                {
                    let focus = active.read(cx).add_input.focus_handle(cx);
                    window.focus(&focus, cx);
                }
            });
        }
    }

    pub(crate) fn close_add_download_action(
        &mut self,
        _: &CloseAddDownload,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.close_add_download(window, cx);
    }

    pub(crate) fn close_add_download(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(view) = self.add_dialog_view() else {
            return;
        };
        if view.read(cx).is_busy() {
            return;
        }
        self.links_input_height = view.read(cx).links_input_height;
        self.active_modal = None;
        self.sync_kit_modals(window, cx);
        if !has_kit_root(window) {
            window.focus(&self.focus_handle, cx);
        }
        cx.notify();
    }

    pub(crate) fn submit_add_download_action(
        &mut self,
        _: &SubmitAddDownload,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.submit_add_download(cx);
    }

    pub(crate) fn submit_add_download(&mut self, cx: &mut Context<Self>) {
        let Some(view) = self.add_dialog_view().cloned() else {
            return;
        };
        if view.read(cx).is_busy() {
            return;
        }
        let (
            input_mode,
            mode,
            file_conflict,
            manual_category_id,
            from_bridge,
            sources,
            metadata_name,
            required_bytes,
            advanced,
        ) = {
            let dialog = view.read(cx);
            let sources = match dialog.input_mode {
                AddDownloadInputModeView::Links => {
                    parse_add_download_sources(dialog.add_input.read(cx).value().as_ref())
                }
                AddDownloadInputModeView::MetadataFiles => dialog
                    .metadata_files
                    .iter()
                    .map(|preview| AddDownloadSourceView::MetadataFile {
                        path: preview.path.clone(),
                        kind: preview.kind,
                        content_sha256: preview.content_sha256.clone(),
                        info_hash: preview.info_hash.clone(),
                        selected_file_indices: preview.selected_file_indices.clone(),
                    })
                    .collect(),
            };
            if sources.is_empty() {
                let summary = match dialog.input_mode {
                    AddDownloadInputModeView::Links => self.t("notice-enter-url"),
                    AddDownloadInputModeView::MetadataFiles => self.t("notice-choose-metadata"),
                };
                view.update(cx, |dialog, cx| {
                    dialog.error = Some(OperationErrorView {
                        code: "validation.invalid_request".into(),
                        summary,
                        retryable: false,
                    });
                    cx.notify();
                });
                cx.notify();
                return;
            }
            if let Some(preview) = dialog
                .metadata_files
                .iter()
                .find(|preview| preview.selected_file_indices.is_empty())
            {
                let summary = format!("Select at least one file from {}.", preview.path.display());
                view.update(cx, |dialog, cx| {
                    dialog.error = Some(OperationErrorView {
                        code: "validation.invalid_request".into(),
                        summary,
                        retryable: false,
                    });
                    cx.notify();
                });
                cx.notify();
                return;
            }
            let required_bytes = if dialog.input_mode == AddDownloadInputModeView::MetadataFiles {
                match selected_metadata_known_bytes(&dialog.metadata_files) {
                    Some(bytes) => Some(bytes),
                    None => {
                        view.update(cx, |dialog, cx| {
                            dialog.error = Some(OperationErrorView {
                                code: "validation.invalid_request".into(),
                                summary: "Selected metadata file sizes exceed the supported range."
                                    .into(),
                                retryable: false,
                            });
                            cx.notify();
                        });
                        cx.notify();
                        return;
                    }
                }
            } else {
                None
            };
            let metadata_name = dialog.metadata_files.first().and_then(|file| {
                file.path
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
            });
            let advanced = if dialog.input_mode == AddDownloadInputModeView::Links {
                dialog.collect_advanced_options(cx)
            } else {
                AddDownloadAdvancedOptionsView::default()
            };
            (
                dialog.input_mode,
                dialog.mode,
                dialog.file_conflict,
                dialog.category_id.clone(),
                dialog.from_bridge,
                sources,
                metadata_name,
                required_bytes,
                advanced,
            )
        };

        let Some(session) = self
            .snapshot
            .commands_available()
            .then(|| self.snapshot.engine_session())
            .flatten()
        else {
            view.update(cx, |dialog, cx| {
                dialog.error = Some(stale_session_error());
                cx.notify();
            });
            cx.notify();
            return;
        };

        let request_id = self.allocate_request_id();
        view.update(cx, |dialog, cx| {
            dialog.pending = Some(PendingAddDownload {
                request_id,
                session: session.clone(),
            });
            dialog.error = None;
            cx.notify();
        });
        self.set_add_dialog_busy(cx);
        if from_bridge {
            // One confirmed hand-off unlocks auto-submit for the rest of the
            // session (D-045 §6). A dialog the user cancels does not.
            self.bridge_auto_submit_unlocked = true;
        }
        // Parse and resolve category routing once per submission.
        let hint = filename_hint_for_add(&sources, metadata_name.as_deref());
        let category_id = resolve_add_category_id(
            &self.settings.categories,
            manual_category_id.as_deref(),
            hint.as_deref(),
        );
        let destination = category_directory(
            &self.settings.categories,
            category_id.as_deref(),
            &self.settings.download_directory,
        );
        cx.emit(AppShellEvent::AddDownloadRequested(
            AddDownloadRequestView {
                request_id,
                session,
                sources,
                mode: if input_mode == AddDownloadInputModeView::Links {
                    mode
                } else {
                    AddDownloadModeView::SeparateTasks
                },
                destination,
                category_id,
                required_bytes,
                file_conflict: if input_mode == AddDownloadInputModeView::Links {
                    file_conflict
                } else {
                    FileConflictPolicyView::Reject
                },
                advanced,
            },
        ));
        cx.notify();
    }

    #[cfg(test)]
    pub(crate) fn set_add_input_mode(
        &mut self,
        mode: AddDownloadInputModeView,
        cx: &mut Context<Self>,
    ) {
        if let Some(view) = self.add_dialog_view().cloned() {
            view.update(cx, |dialog, cx| dialog.set_input_mode(mode, cx));
            cx.notify();
        }
    }

    #[must_use]
    pub fn can_open_metadata_paths(&self) -> bool {
        if !self.snapshot.commands_available()
            || self.pending_task_command.is_some()
            || self.pending_batch_command.is_some()
        {
            return false;
        }
        match &self.active_modal {
            None => true,
            Some(ActiveModal::AddDownload { busy, .. }) => !*busy,
            Some(_) => false,
        }
    }

    #[must_use]
    pub fn can_open_magnet_uris(&self) -> bool {
        self.snapshot.commands_available()
            && self.active_modal.is_none()
            && self.pending_task_command.is_none()
            && self.pending_batch_command.is_none()
    }

    /// Open the add-download dialog with external magnet links awaiting confirmation.
    #[must_use]
    pub fn open_magnet_uris(
        &mut self,
        mut uris: Vec<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if !self.can_open_magnet_uris()
            || uris.is_empty()
            || uris
                .iter()
                .any(|uri| ariadeck_domain::magnet_info_hash(uri).is_none())
        {
            return false;
        }
        let mut known = HashSet::new();
        uris.retain(|uri| known.insert(uri.clone()));
        self.open_add_download(&OpenAddDownload, window, cx);
        let Some(view) = self.add_dialog_view().cloned() else {
            return false;
        };
        let joined = uris.join("\n");
        view.update(cx, |dialog, cx| {
            dialog
                .add_input
                .update(cx, |input, cx| input.set_value(joined, window, cx));
            cx.notify();
        });
        cx.notify();
        true
    }

    /// Whether the shell can accept browser-bridge downloads right now.
    ///
    /// Same gate as magnet links, so a forwarded download never clobbers input
    /// the user is already editing.
    #[must_use]
    pub fn can_open_bridge_downloads(&self) -> bool {
        self.can_open_magnet_uris()
    }

    /// Whether a forwarded download will stop for confirmation (D-045 §6).
    ///
    /// Auto-submit is opt-in. With cookies also enabled, the first hand-off of
    /// the session is confirmed anyway: that combination is the highest-impact
    /// configuration and does not get a fully silent path.
    #[must_use]
    pub fn bridge_requires_confirmation(&self) -> bool {
        !self.settings.browser_bridge.auto_submit
            || (self.settings.browser_bridge.allow_cookies && !self.bridge_auto_submit_unlocked)
    }

    /// Take browser-bridge downloads and either fill the dialog for confirmation
    /// or submit them, per [`Self::bridge_requires_confirmation`].
    #[must_use]
    pub fn accept_bridge_downloads(
        &mut self,
        downloads: &[BridgeDownloadView],
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> usize {
        let consumed = self.open_bridge_downloads(downloads, window, cx);
        if consumed > 0 && !self.bridge_requires_confirmation() {
            self.submit_add_download(cx);
        }
        consumed
    }

    /// Fill the add-download dialog from browser-bridge downloads **without
    /// submitting** (D-045 confirm policy).
    #[must_use]
    pub fn open_bridge_downloads(
        &mut self,
        downloads: &[BridgeDownloadView],
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> usize {
        let Some(first) = downloads.first() else {
            return 0;
        };
        if !self.can_open_bridge_downloads() {
            return 0;
        }
        let group_len = downloads
            .iter()
            .take_while(|download| download.shares_headers_with(first))
            .count();

        let mut known = HashSet::new();
        let urls = downloads[..group_len]
            .iter()
            .map(|download| download.url.clone())
            .filter(|url| known.insert(url.clone()))
            .collect::<Vec<_>>();
        if urls.is_empty() {
            return 0;
        }

        self.open_add_download(&OpenAddDownload, window, cx);
        let Some(view) = self.add_dialog_view().cloned() else {
            return 0;
        };
        let joined = urls.join("\n");
        let has_headers =
            !first.referer.is_empty() || !first.user_agent.is_empty() || first.cookie.is_some();
        let referer = first.referer.clone();
        let user_agent = first.user_agent.clone();
        let cookie = first
            .cookie
            .clone()
            .map(SecretStringView::into_inner)
            .unwrap_or_default();
        view.update(cx, |dialog, cx| {
            dialog.from_bridge = true;
            dialog
                .add_input
                .update(cx, |input, cx| input.set_value(joined, window, cx));
            if has_headers {
                dialog
                    .add_inputs
                    .referer
                    .update(cx, |input, cx| input.set_value(referer, window, cx));
                dialog
                    .add_inputs
                    .user_agent
                    .update(cx, |input, cx| input.set_value(user_agent, window, cx));
                dialog
                    .add_inputs
                    .cookie
                    .update(cx, |input, cx| input.set_value(cookie, window, cx));
                // Reveal what was prefilled. Carrying a cookie silently would hide
                // the most sensitive part of the request from whoever confirms it.
                dialog.advanced_open = true;
            }
            cx.notify();
        });
        cx.notify();
        group_len
    }

    /// Open the add-download dialog and enqueue metadata files for preview.
    #[must_use]
    pub fn open_metadata_paths(
        &mut self,
        paths: Vec<PathBuf>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if !self.can_open_metadata_paths() {
            return false;
        }
        if self.add_dialog_view().is_none() {
            self.open_add_download(&OpenAddDownload, window, cx);
        }
        let Some(view) = self.add_dialog_view().cloned() else {
            return false;
        };
        if view.read(cx).is_busy() {
            return false;
        }

        let mut known = view
            .read(cx)
            .metadata_files
            .iter()
            .map(|preview| metadata_path_key(&preview.path))
            .collect::<HashSet<_>>();
        let mut invalid = Vec::new();
        let mut accepted = Vec::new();
        for path in paths {
            if metadata_kind_from_path(&path).is_none() {
                invalid.push(path);
                continue;
            }
            if known.insert(metadata_path_key(&path)) {
                accepted.push(path);
            }
        }
        let error = if invalid.is_empty() {
            None
        } else {
            Some(OperationErrorView {
                code: "validation.unsupported_metadata_file".into(),
                summary: format!(
                    "Skipped {} file{}; supported extensions are .torrent, .metalink, and .meta4.",
                    invalid.len(),
                    if invalid.len() == 1 { "" } else { "s" }
                ),
                retryable: false,
            })
        };
        let preview_request = if accepted.is_empty() {
            None
        } else {
            let request_id = self.allocate_request_id();
            Some((request_id, accepted))
        };
        view.update(cx, |dialog, cx| {
            dialog.input_mode = AddDownloadInputModeView::MetadataFiles;
            dialog.mode = AddDownloadModeView::SeparateTasks;
            dialog.file_conflict = FileConflictPolicyView::Reject;
            dialog.results.clear();
            dialog.error = error;
            if let Some((request_id, ref paths)) = preview_request {
                dialog.preview_pending = Some(PendingMetadataPreview {
                    request_id,
                    paths: paths.clone(),
                });
            }
            cx.notify();
        });
        if let Some((request_id, paths)) = preview_request {
            self.set_add_dialog_busy(cx);
            cx.emit(AppShellEvent::AddDownloadMetadataPreviewRequested(
                AddDownloadMetadataPreviewRequestView { request_id, paths },
            ));
        }
        cx.notify();
        true
    }

    #[cfg(test)]
    pub(crate) fn remove_metadata_file(&mut self, index: usize, cx: &mut Context<Self>) {
        if let Some(view) = self.add_dialog_view().cloned() {
            view.update(cx, |dialog, cx| dialog.remove_metadata_file(index, cx));
            cx.notify();
        }
    }

    #[cfg(test)]
    pub(crate) fn toggle_metadata_file_entry(
        &mut self,
        preview_index: usize,
        file_index: u32,
        cx: &mut Context<Self>,
    ) {
        if let Some(view) = self.add_dialog_view().cloned() {
            view.update(cx, |dialog, cx| {
                dialog.toggle_metadata_file_entry(preview_index, file_index, cx)
            });
            cx.notify();
        }
    }

    #[cfg(test)]
    pub(crate) fn toggle_all_metadata_file_entries(
        &mut self,
        preview_index: usize,
        cx: &mut Context<Self>,
    ) {
        if let Some(view) = self.add_dialog_view().cloned() {
            view.update(cx, |dialog, cx| {
                dialog.toggle_all_metadata_file_entries(preview_index, cx)
            });
            cx.notify();
        }
    }

    #[cfg(test)]
    pub(crate) fn toggle_add_advanced(&mut self, cx: &mut Context<Self>) {
        if let Some(view) = self.add_dialog_view().cloned() {
            view.update(cx, |dialog, cx| dialog.toggle_advanced(cx));
            cx.notify();
        }
    }
}
