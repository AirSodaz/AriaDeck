//! Shell chrome (header, sidebar, status) for AppShell.

use super::*;

impl AppShell {
    pub(crate) fn render_header(&mut self, _window: &Window, cx: &mut Context<Self>) -> Div {
        let colors = self.theme.colors;
        let (search_left, search_right) =
            centered_search_bounds(f32::from(_window.viewport_size().width));
        let search_width = search_right - search_left;
        let brand = div()
            .w(px(TITLEBAR_SIDE_WIDTH))
            .flex_none()
            .flex()
            .items_center()
            .h_full()
            .gap_2()
            .pl(px(TITLEBAR_BRAND_INSET))
            .window_control_area(WindowControlArea::Drag)
            .child(
                Icon::new(IconName::Download)
                    .size(IconSize::Medium)
                    .color(colors.accent),
            )
            .child(
                div()
                    .text_sm()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(self.t("ui-app-name")),
            );
        // On Windows the caption strip must reach the physical right edge, so
        // horizontal padding is applied only on the left (and non-Windows keeps
        // symmetric padding for the Add action cluster).
        let header = div()
            .h(px(TITLEBAR_HEIGHT))
            .flex_none()
            .flex()
            .items_center()
            .pl_3()
            .border_b_1()
            .border_color(colors.border)
            .bg(colors.toolbar_surface);
        #[cfg(not(target_os = "windows"))]
        let header = header.pr_3();
        header
            .child(brand)
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .flex()
                    .items_center()
                    .child(titlebar_drag_region())
                    .child(
                        div()
                            .w(px(search_width))
                            .flex_none()
                            .child(self.search_input.clone()),
                    )
                    .child(titlebar_drag_region()),
            )
            .child({
                // Keep chrome actions (Add) padded; Windows caption buttons are
                // rendered outside this inset so Close can sit flush to the edge.
                let actions = div()
                    .flex_none()
                    .flex()
                    .items_center()
                    .justify_end()
                    .gap_2()
                    .when(cfg!(target_os = "windows"), |element| element.pl_2().pr_2())
                    .when(!cfg!(target_os = "windows"), |element| {
                        element.w(px(TITLEBAR_SIDE_WIDTH))
                    })
                    .child(self.render_add_button(cx));
                #[cfg(target_os = "windows")]
                {
                    div()
                        .flex_none()
                        .flex()
                        .items_center()
                        .justify_end()
                        .child(actions)
                        .child(self.render_window_controls(_window))
                }
                #[cfg(not(target_os = "windows"))]
                {
                    actions
                }
            })
    }

    #[cfg(target_os = "windows")]
    pub(crate) fn render_window_controls(&self, window: &Window) -> Div {
        let colors = self.theme.colors;
        let maximized = window.is_maximized();
        div()
            .h(px(TITLEBAR_HEIGHT))
            .flex_none()
            .flex()
            .items_center()
            .children(
                [
                    WindowControlKind::Minimize,
                    WindowControlKind::Maximize,
                    WindowControlKind::Close,
                ]
                .map(|kind| {
                    let control = window_control_config(kind, maximized);
                    window_control_button(
                        control.id,
                        control.icon,
                        control.label,
                        control.area,
                        colors,
                        control.danger,
                    )
                }),
            )
    }

    pub(crate) fn render_add_button(&self, cx: &mut Context<Self>) -> Stateful<Div> {
        let colors = self.theme.colors;
        let enabled = self.snapshot.commands_available() && !self.add_dialog.open;
        Button::new("open-add-download", self.t("action-add-download"))
            .icon(IconName::Plus)
            .aria_label(if enabled {
                self.t("action-add-download-aria")
            } else {
                self.t("add-download-unavailable")
            })
            .tooltip(Tooltip::new(self.t("action-add-download")).meta("Ctrl/Cmd+N"))
            .style(ButtonStyle::Primary)
            .disabled(!enabled)
            .on_click(cx.listener(|this, _, window, cx| {
                this.open_add_download(&OpenAddDownload, window, cx);
            }))
            .render(colors)
    }

    pub(crate) fn render_sidebar(&mut self, cx: &mut Context<Self>) -> Div {
        let colors = self.theme.colors;
        let mut filters = Vec::with_capacity(WorkspaceFilter::ALL.len());
        for filter in WorkspaceFilter::ALL {
            let count = filter.count(self.snapshot.counts);
            let selected = self.page == AppPage::Downloads && self.query.filter == filter;
            let icon = filter_icon(filter);
            filters.push(
                div()
                    .id(SharedString::from(format!(
                        "sidebar-filter-{}",
                        filter.key()
                    )))
                    .focusable()
                    .tab_stop(true)
                    .role(Role::Button)
                    .aria_label(format!("{}, {count}", self.t(filter.message_key())))
                    .h(px(38.0))
                    .w_full()
                    .px_3()
                    .flex()
                    .items_center()
                    .gap_3()
                    .rounded_md()
                    .text_xs()
                    .text_color(if selected {
                        colors.accent
                    } else {
                        colors.text_secondary
                    })
                    .when(selected, |element| {
                        element.bg(with_alpha(colors.accent, 0.09))
                    })
                    .when(!selected, |element| {
                        element.hover(|style| style.bg(colors.surface_hover))
                    })
                    .focus_visible(|style| style.border_1().border_color(colors.focus_ring))
                    .cursor_pointer()
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.set_filter(filter, window, cx);
                    }))
                    .child(Icon::new(icon).size(IconSize::Small).color(if selected {
                        colors.accent
                    } else {
                        colors.text_muted
                    }))
                    .child(div().flex_1().child(filter.short_label()))
                    .child(
                        div()
                            .h(px(22.0))
                            .min_w(px(22.0))
                            .px_1()
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded_full()
                            .bg(if selected {
                                with_alpha(colors.accent, 0.12)
                            } else {
                                colors.surface_active
                            })
                            .font_features(tabular_numbers())
                            .text_xs()
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(if selected {
                                colors.accent
                            } else {
                                colors.text_muted
                            })
                            .child(count.to_string()),
                    ),
            );
        }

        let active_profile_name = self
            .profiles
            .active()
            .map(|profile| profile.name.clone())
            .unwrap_or_else(|| self.t("no-profile"));
        let active_profile_kind = self
            .profiles
            .active()
            .map(|profile| profile.kind.label())
            .unwrap_or("—");
        let profile_count = self.profiles.profiles.len();

        div()
            .w(px(SIDEBAR_WIDTH))
            .flex_none()
            .flex()
            .flex_col()
            .justify_between()
            .border_r_1()
            .border_color(colors.border)
            .bg(colors.surface)
            .p_3()
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child(
                        div()
                            .id("active-profile-banner")
                            .role(Role::Status)
                            .aria_label(format!(
                                "Active profile {active_profile_name}, {active_profile_kind}"
                            ))
                            .px_3()
                            .py_2()
                            .rounded_md()
                            .bg(colors.surface_active)
                            .child(
                                div()
                                    .text_xs()
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(colors.text_primary)
                                    .child(active_profile_name),
                            )
                            .child(div().text_xs().text_color(colors.text_muted).child(format!(
                                "{active_profile_kind} · {profile_count} profile{}",
                                if profile_count == 1 { "" } else { "s" }
                            ))),
                    )
                    .child(div().flex().flex_col().gap_1().children(filters)),
            )
            .child(
                div()
                    .id("open-settings")
                    .focusable()
                    .tab_stop(true)
                    .role(Role::Button)
                    .aria_label(self.t("action-open-settings"))
                    .h(px(38.0))
                    .w_full()
                    .px_3()
                    .flex()
                    .items_center()
                    .gap_3()
                    .rounded_md()
                    .text_xs()
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(if self.page == AppPage::Settings {
                        colors.accent
                    } else {
                        colors.text_secondary
                    })
                    .when(self.page == AppPage::Settings, |element| {
                        element.bg(with_alpha(colors.accent, 0.09))
                    })
                    .cursor_pointer()
                    .hover(|style| style.bg(colors.surface_hover))
                    .focus_visible(|style| style.border_1().border_color(colors.focus_ring))
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.open_settings(&OpenSettings, window, cx);
                    }))
                    .child(Icon::new(IconName::Settings).size(IconSize::Small).color(
                        if self.page == AppPage::Settings {
                            colors.accent
                        } else {
                            colors.text_muted
                        },
                    ))
                    .child(self.t("settings-label")),
            )
    }

    pub(crate) fn render_speed_chart(&self) -> Stateful<Div> {
        let colors = self.theme.colors;
        let visible = speed_chart_window(&self.snapshot.speed_history);
        let max_rate = visible
            .iter()
            .map(|sample| sample.download_rate.max(sample.upload_rate))
            .max()
            .unwrap_or(0);
        let scale = max_rate.max(1) as f32;
        let mut columns = Vec::with_capacity(SPEED_CHART_SAMPLES);
        columns.extend(
            (visible.len()..SPEED_CHART_SAMPLES).map(|_| speed_chart_column(0.0, 0.0, colors)),
        );
        columns.extend(visible.iter().map(|sample| {
            speed_chart_column(
                sample.download_rate as f32 / scale,
                sample.upload_rate as f32 / scale,
                colors,
            )
        }));

        div()
            .id("speed-history-chart")
            .role(Role::Group)
            .aria_label(format!(
                "Transfer speed for the last minute, current download {}, current upload {}, peak {}",
                format_rate(self.snapshot.download_rate),
                format_rate(self.snapshot.upload_rate),
                format_rate(max_rate)
            ))
            .h(px(144.0))
            .w(px(280.0))
            .flex_none()
            .flex()
            .flex_col()
                    .gap_2()
                    .p_3()
                    .rounded_md()
            .border_1()
            .border_color(colors.border_strong)
            .bg(colors.elevated_surface)
            .child(
                div()
                    .flex()
                    .items_baseline()
                    .justify_between()
                    .child(
                        div()
                            .text_xs()
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(colors.text_secondary)
                            .child(self.t("speed-last-minute")),
                    )
                    .child(
                        div()
                            .font_features(tabular_numbers())
                            .text_xs()
                            .text_color(colors.text_muted)
                            .child(format_rate(max_rate)),
                    ),
            )
            .child(
                div()
                    .h(px(58.0))
                    .w_full()
                    .flex_none()
                    .flex()
                    .items_end()
                    .children(columns),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_3()
                    .text_xs()
                    .text_color(colors.text_muted)
                    .child(speed_chart_legend("Down", colors.progress_download, colors))
                    .child(speed_chart_legend("Up", colors.progress_upload, colors)),
            )
    }

    pub(crate) fn render_status_bar(&mut self, cx: &mut Context<Self>) -> Div {
        let colors = self.theme.colors;
        let connection_color = connection_color(&self.snapshot.connection, colors);
        let connection_label = match &self.snapshot.connection {
            ConnectionView::Reconnecting { attempt } => {
                format!("{} · {attempt}", self.t("connection-reconnecting"))
            }
            connection => self.t(connection.message_key()),
        };
        let status_button = div()
            .id("connection-status")
            .role(if self.snapshot.connection.can_retry() {
                Role::Button
            } else {
                Role::Status
            })
            .aria_label(if self.snapshot.connection.can_retry() {
                self.t("connection-retry")
            } else {
                connection_label.clone()
            })
            .h_full()
            .px_2()
            .flex()
            .items_center()
            .gap_1()
            .text_xs()
            .text_color(colors.text_muted)
            .child(StatusIndicator::new(connection_color).icon(
                if self.snapshot.connection.is_connected() {
                    IconName::Wifi
                } else {
                    IconName::WifiOff
                },
            ))
            .child(connection_label)
            .when(self.snapshot.connection.can_retry(), |element| {
                element
                    .focusable()
                    .tab_stop(true)
                    .cursor_pointer()
                    .hover(|style| style.bg(colors.surface_hover))
                    .on_click(cx.listener(|this, _, _, cx| this.request_retry(cx)))
            });

        div()
            .h(px(28.0))
            .flex_none()
            .flex()
            .items_center()
            .border_t_1()
            .border_color(colors.border)
            .bg(colors.toolbar_surface)
            .child(status_button)
            .child(
                div()
                    .id("engine-status")
                    .role(Role::Status)
                    .aria_label(self.t(self.engine_health.message_key()))
                    .h_full()
                    .px_2()
                    .flex()
                    .items_center()
                    .gap_1()
                    .text_xs()
                    .text_color(colors.text_muted)
                    .child(
                        StatusIndicator::new(engine_health_color(&self.engine_health, colors))
                            .icon(match &self.engine_health {
                                EngineHealthView::Failed { .. } => IconName::CircleAlert,
                                EngineHealthView::Restarting { .. } => IconName::RefreshCw,
                                EngineHealthView::Running { restarts } if *restarts > 0 => {
                                    IconName::TriangleAlert
                                }
                                EngineHealthView::External | EngineHealthView::Running { .. } => {
                                    IconName::Activity
                                }
                            }),
                    )
                    .child(self.t(self.engine_health.message_key())),
            )
            .when(self.snapshot.stale, |element| {
                element.child(
                    div()
                        .id("stale-status")
                        .role(Role::Status)
                        .h_full()
                        .px_2()
                        .flex()
                        .items_center()
                        .gap_1()
                        .text_xs()
                        .text_color(colors.warning)
                        .child(
                            Icon::new(IconName::TriangleAlert)
                                .size(IconSize::XSmall)
                                .color(colors.warning),
                        )
                        .child(self.t("stale-data")),
                )
            })
            .when_some(
                self.snapshot.stopped_history.summary_label(),
                |element, label| {
                    let can_load = self.snapshot.stopped_history.can_load_more
                        && self.snapshot.connection.is_connected()
                        && !self.snapshot.stale;
                    let pending = self.pending_load_more_stopped;
                    element.child(
                        div()
                            .id("stopped-history-status")
                            .role(if can_load { Role::Button } else { Role::Status })
                            .aria_label(if can_load {
                                format!("{label}. Load more stopped results.")
                            } else {
                                label.clone()
                            })
                            .h_full()
                            .px_2()
                            .flex()
                            .items_center()
                            .gap_1()
                            .text_xs()
                            .text_color(colors.text_muted)
                            .child(label)
                            .when(can_load, |element| {
                                element
                                    .focusable()
                                    .tab_stop(true)
                                    .cursor_pointer()
                                    .hover(|style| style.bg(colors.surface_hover))
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.request_load_more_stopped(cx);
                                    }))
                                    .child(div().text_color(colors.information).child(if pending {
                                        "…".to_owned()
                                    } else {
                                        self.t("load-more")
                                    }))
                            }),
                    )
                },
            )
            .child({
                let activity_count = self.activity_log.len();
                let activity_label = if activity_count == 0 {
                    self.t("activity-history")
                } else {
                    format!("{}, {activity_count}", self.t("activity-history"))
                };
                div()
                    .id("activity-status")
                    .focusable()
                    .tab_stop(true)
                    .role(Role::Button)
                    .aria_label(activity_label)
                    .aria_expanded(self.activity_panel_open)
                    .ml_auto()
                    .h_full()
                    .px_2()
                    .flex()
                    .items_center()
                    .gap_1()
                    .text_xs()
                    .text_color(if self.activity_panel_open {
                        colors.text_primary
                    } else {
                        colors.text_muted
                    })
                    .cursor_pointer()
                    .hover(|style| style.bg(colors.surface_hover))
                    .focus_visible(|style| style.bg(colors.surface_active))
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.toggle_activity_panel(window, cx);
                    }))
                    .child(Icon::new(IconName::Activity).size(IconSize::XSmall).color(
                        if self.activity_panel_open {
                            colors.information
                        } else {
                            colors.text_muted
                        },
                    ))
                    .child(if activity_count == 0 {
                        "Activity".to_owned()
                    } else {
                        format!("Activity · {activity_count}")
                    })
            })
            .child(
                div()
                    .id("transfer-status")
                    .focusable()
                    .tab_stop(true)
                    .role(Role::Button)
                    .aria_label(format!(
                        "Transfer speed, download {}, upload {}; show last minute chart",
                        format_rate(self.snapshot.download_rate),
                        format_rate(self.snapshot.upload_rate)
                    ))
                    .aria_expanded(self.speed_popover_open)
                    .h_full()
                    .px_2()
                    .flex()
                    .items_center()
                    .gap_2()
                    .font_features(tabular_numbers())
                    .text_xs()
                    .text_color(colors.text_muted)
                    .cursor_pointer()
                    .hover(|style| style.bg(colors.surface_hover))
                    .focus_visible(|style| style.bg(colors.surface_active))
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.toggle_speed_popover(window, cx);
                    }))
                    .child(
                        Icon::new(IconName::ArrowDown)
                            .size(IconSize::XSmall)
                            .color(colors.progress_download),
                    )
                    .child(format_rate(self.snapshot.download_rate))
                    .child(
                        Icon::new(IconName::ArrowUp)
                            .size(IconSize::XSmall)
                            .color(colors.progress_upload),
                    )
                    .child(format_rate(self.snapshot.upload_rate)),
            )
    }

    pub(crate) fn render_speed_popover(&self, cx: &mut Context<Self>) -> Stateful<Div> {
        let colors = self.theme.colors;
        div()
            .id("speed-popover-layer")
            .absolute()
            .inset_0()
            .occlude()
            .on_click(cx.listener(|this, _, window, cx| {
                this.close_speed_popover(window, cx);
            }))
            .child(
                div()
                    .id("speed-popover")
                    .absolute()
                    .right(px(8.0))
                    .bottom(px(32.0))
                    .on_click(|_, _, cx| cx.stop_propagation())
                    .bg(colors.elevated_surface)
                    .child(self.render_speed_chart()),
            )
    }

    pub(crate) fn render_text_field_context_menu(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let colors = self.theme.colors;
        let Some(menu) = self.text_field_context_menu.as_ref() else {
            return div().into_any_element();
        };
        let field = menu.field.clone();
        let position = menu.position;
        let field_state = field.read(cx);
        let has_selection = field_state.has_selection();
        let secure = field_state.is_secure_field();
        let is_empty = field_state.is_empty();
        let can_copy = has_selection && !secure;
        let can_cut = can_copy;
        let can_paste = true;
        let can_select_all = !is_empty;
        let left = f32::from(position.x).max(8.0);
        let top = f32::from(position.y).max(8.0);

        let item = |action: TextFieldContextAction, label: &'static str, enabled: bool| {
            let id = match action {
                TextFieldContextAction::Cut => "shell-text-ctx-cut",
                TextFieldContextAction::Copy => "shell-text-ctx-copy",
                TextFieldContextAction::Paste => "shell-text-ctx-paste",
                TextFieldContextAction::SelectAll => "shell-text-ctx-select-all",
            };
            div()
                .id(id)
                .role(Role::MenuItem)
                .aria_label(label)
                .focusable()
                .tab_stop(enabled)
                .focus_visible(|style| style.border_1().border_color(colors.focus_ring))
                .w_full()
                .px_3()
                .py_1p5()
                .rounded_sm()
                .text_sm()
                .text_color(if enabled {
                    colors.text_primary
                } else {
                    colors.text_muted
                })
                .when(enabled, |element| {
                    element
                        .cursor_pointer()
                        .hover(|style| style.bg(colors.surface_active))
                        .on_mouse_down(MouseButton::Left, {
                            cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                                cx.stop_propagation();
                                window.prevent_default();
                                let _ = event;
                                this.activate_text_field_context_action(action, window, cx);
                            })
                        })
                        .on_click(cx.listener(move |this, event: &ClickEvent, window, cx| {
                            if event.is_keyboard() {
                                cx.stop_propagation();
                                window.prevent_default();
                                this.activate_text_field_context_action(action, window, cx);
                            }
                        }))
                })
                .child(label)
        };

        div()
            .id("text-field-context-menu-layer")
            .absolute()
            .inset_0()
            .occlude()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| this.close_text_field_context_menu(cx)),
            )
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(|this, _, _, cx| this.close_text_field_context_menu(cx)),
            )
            .child(
                div()
                    .id("text-field-context-menu")
                    .role(Role::Menu)
                    .aria_label(self.t("text-field-menu"))
                    .absolute()
                    .left(px(left))
                    .top(px(top))
                    .min_w(px(168.0))
                    .py_1()
                    .px_1()
                    .rounded_md()
                    .border_1()
                    .border_color(colors.border_strong)
                    .bg(colors.elevated_surface)
                    .shadow_md()
                    .flex()
                    .flex_col()
                    .gap_0p5()
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .on_mouse_down(MouseButton::Right, |_, _, cx| cx.stop_propagation())
                    .child(item(TextFieldContextAction::Cut, "Cut", can_cut))
                    .child(item(TextFieldContextAction::Copy, "Copy", can_copy))
                    .child(item(TextFieldContextAction::Paste, "Paste", can_paste))
                    .child(item(
                        TextFieldContextAction::SelectAll,
                        "Select all",
                        can_select_all,
                    )),
            )
            .into_any_element()
    }

    pub(crate) fn render_activity_panel(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let colors = self.theme.colors;
        let entries = self.activity_log.clone();
        div()
            .id("activity-panel-layer")
            .absolute()
            .inset_0()
            .occlude()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, window, cx| {
                    this.close_activity_panel(window, cx);
                }),
            )
            .child(
                div()
                    .id("activity-panel")
                    .absolute()
                    .right(px(8.0))
                    .bottom(px(36.0))
                    .w(px(ACTIVITY_PANEL_WIDTH))
                    .max_h(px(420.0))
                    .flex()
                    .flex_col()
                    .gap_2()
                    .p_3()
                    .rounded_lg()
                    .border_1()
                    .border_color(colors.border)
                    .bg(colors.elevated_surface)
                    .shadow_lg()
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .gap_2()
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap_2()
                                    .child(
                                        Icon::new(IconName::Activity)
                                            .size(IconSize::Small)
                                            .color(colors.information),
                                    )
                                    .child(
                                        div()
                                            .text_sm()
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .text_color(colors.text_primary)
                                            .child(self.t("activity-title")),
                                    ),
                            )
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap_1()
                                    .child(
                                        Button::new("clear-activity-log", "Clear")
                                            .aria_label(self.t("action-clear-activity"))
                                            .style(ButtonStyle::Secondary)
                                            .disabled(entries.is_empty())
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.clear_activity_log(cx);
                                            }))
                                            .render(colors),
                                    )
                                    .child(
                                        IconButton::new("close-activity-panel", IconName::X)
                                            .aria_label(self.t("action-close-activity"))
                                            .on_click(cx.listener(|this, _, window, cx| {
                                                this.close_activity_panel(window, cx);
                                            }))
                                            .render(colors),
                                    ),
                            ),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(colors.text_muted)
                            .child(
                                "Recent completion, error, and engine events for this session. Grouped when many finish together.",
                            ),
                    )
                    .child(if entries.is_empty() {
                        div()
                            .flex_1()
                            .py_6()
                            .flex()
                            .items_center()
                            .justify_center()
                            .text_xs()
                            .text_color(colors.text_muted)
                            .child(self.t("ui-no-activity"))
                            .into_any_element()
                    } else {
                        div()
                            .id("activity-panel-scroll")
                            .flex_1()
                            .min_h_0()
                            .max_h(px(320.0))
                            .overflow_y_scroll()
                            .flex()
                            .flex_col()
                            .gap_2()
                            .children(entries.into_iter().map(|entry| {
                                let kind_color = match entry.kind {
                                    ActivityKindView::Completion => colors.success,
                                    ActivityKindView::Error => colors.danger,
                                    ActivityKindView::Engine => colors.warning,
                                    ActivityKindView::Command => colors.information,
                                    ActivityKindView::Info => colors.text_muted,
                                };
                                div()
                                    .id(SharedString::from(format!(
                                        "activity-entry-{}",
                                        entry.id
                                    )))
                                    .flex()
                                    .flex_col()
                                    .gap_1()
                                    .p_2()
                                    .rounded_md()
                                    .bg(colors.surface)
                                    .border_1()
                                    .border_color(colors.border)
                                    .child(
                                        div()
                                            .flex()
                                            .items_center()
                                            .gap_2()
                                            .child(
                                                StatusIndicator::new(kind_color).icon(
                                                    match entry.kind {
                                                        ActivityKindView::Completion => {
                                                            IconName::CircleCheck
                                                        }
                                                        ActivityKindView::Error => IconName::CircleX,
                                                        ActivityKindView::Engine => {
                                                            IconName::TriangleAlert
                                                        }
                                                        ActivityKindView::Command => {
                                                            IconName::Activity
                                                        }
                                                        ActivityKindView::Info => IconName::Info,
                                                    },
                                                ),
                                            )
                                            .child(
                                                div()
                                                    .text_xs()
                                                    .font_weight(FontWeight::MEDIUM)
                                                    .text_color(colors.text_muted)
                                                    .child(entry.kind.label()),
                                            )
                                            .when(entry.count > 1, |element| {
                                                element.child(
                                                    div()
                                                        .text_xs()
                                                        .text_color(colors.text_muted)
                                                        .child(format!("x{}", entry.count)),
                                                )
                                            }),
                                    )
                                    .child(
                                        div()
                                            .text_sm()
                                            .text_color(colors.text_primary)
                                            .child(entry.summary.clone()),
                                    )
                                    .when_some(entry.detail.clone(), |element, detail| {
                                        element.child(
                                            div()
                                                .text_xs()
                                                .text_color(colors.text_muted)
                                                .child(detail),
                                        )
                                    })
                            }))
                            .into_any_element()
                    }),
            )
            .into_any_element()
    }

    pub(crate) fn render_toast(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let Some(notice) = self.status_notice.as_ref() else {
            return div().into_any_element();
        };
        let kind = if notice.is_error {
            ToastKind::Error
        } else {
            ToastKind::Success
        };
        div()
            .absolute()
            .right(px(16.0))
            .bottom(px(44.0))
            .child(
                Toast::new("operation-toast", notice.message.clone(), kind, self.theme)
                    .on_close(cx.listener(|this, _, _, cx| this.dismiss_notice(cx))),
            )
            .into_any_element()
    }

    pub(crate) fn render_empty_state(&self, cx: &mut Context<Self>) -> AnyElement {
        let colors = self.theme.colors;
        let (icon, title, show_clear) = match &self.snapshot.connection {
            ConnectionView::Connecting
            | ConnectionView::Authenticating
            | ConnectionView::Synchronizing
            | ConnectionView::Reconnecting { .. }
                if self.snapshot.tasks.is_empty() =>
            {
                (
                    IconName::LoaderCircle,
                    self.t("connection-connecting"),
                    false,
                )
            }
            ConnectionView::Failed { .. } => {
                (IconName::CloudOff, self.t("connection-failed-title"), false)
            }
            ConnectionView::Disconnected if self.snapshot.tasks.is_empty() => {
                (IconName::CloudOff, self.t("aria2-unavailable"), false)
            }
            _ if !self.query.search.trim().is_empty() => {
                (IconName::SearchX, self.t("no-matching-downloads"), true)
            }
            _ if self.query.filter != WorkspaceFilter::All => (
                IconName::Inbox,
                format!(
                    "{} — {}",
                    self.t("no-downloads"),
                    self.t(self.query.filter.message_key())
                ),
                true,
            ),
            _ => (IconName::Inbox, self.t("no-downloads"), false),
        };
        let show_add = self.query.filter == WorkspaceFilter::All
            && self.query.search.trim().is_empty()
            && self.snapshot.commands_available()
            && !self.add_dialog.open;

        div()
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .child(
                div()
                    .max_w(px(420.0))
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap_3()
                    .text_center()
                    .child(
                        div()
                            .size(px(48.0))
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded_md()
                            .border_1()
                            .border_color(colors.border)
                            .bg(colors.elevated_surface)
                            .child(
                                Icon::new(icon)
                                    .size(IconSize::Large)
                                    .color(colors.text_muted),
                            ),
                    )
                    .child(
                        div()
                            .text_base()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(title),
                    )
                    .when(show_clear, |element| {
                        element.child(
                            Button::new("clear-empty-filter", self.t("clear-filter"))
                                .aria_label(self.t("action-clear-search"))
                                .style(ButtonStyle::Secondary)
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.query.filter = WorkspaceFilter::All;
                                    this.search_input
                                        .update(cx, |input, cx| input.set_text("", cx));
                                    window.focus(&this.focus_handle, cx);
                                    this.emit_query(cx);
                                }))
                                .render(colors),
                        )
                    })
                    .when(show_add, |element| {
                        element.child(
                            Button::new("add-download-empty-state", self.t("action-add-download"))
                                .icon(IconName::Plus)
                                .aria_label(self.t("action-add-download-aria"))
                                .style(ButtonStyle::Primary)
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.open_add_download(&OpenAddDownload, window, cx);
                                }))
                                .render(colors),
                        )
                    })
                    .when(self.snapshot.connection.can_retry(), |element| {
                        element.child(
                            Button::new("retry-connection", self.t("retry-label"))
                                .aria_label(self.t("connection-retry-now"))
                                .style(ButtonStyle::Primary)
                                .on_click(cx.listener(|this, _, _, cx| this.request_retry(cx)))
                                .render(colors),
                        )
                    }),
            )
            .into_any_element()
    }
}
