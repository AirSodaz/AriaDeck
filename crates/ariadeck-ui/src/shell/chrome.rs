//! Shell chrome (header, sidebar, status) for AppShell.

use super::*;

#[derive(Clone)]
struct SpeedChartPoint {
    label: SharedString,
    download_rate: f64,
    upload_rate: f64,
}

const WINDOW_CONTROL_BUTTON_WIDTH: f32 = 46.0;

impl AppShell {
    pub(crate) fn render_header(
        &mut self,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let colors = self.theme.colors;
        let is_linux = cfg!(target_os = "linux");
        let is_macos = cfg!(target_os = "macos");
        let is_windows = cfg!(target_os = "windows");
        let is_windows_or_linux = is_windows || is_linux;

        let (search_left, search_right) =
            centered_search_bounds(f32::from(window.viewport_size().width));
        let search_width = search_right - search_left;
        let brand = div()
            .flex_none()
            .flex()
            .items_center()
            .h_full()
            .gap_2()
            .child(img("icon.svg").size(px(20.0)).flex_none())
            .child(
                div()
                    .text_sm()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(self.t("ui-app-name")),
            );
        let actions = div()
            .flex_none()
            .flex()
            .items_center()
            .justify_end()
            .when(!is_windows_or_linux, |element| element.pr_4())
            .child(
                div()
                    .debug_selector(|| "titlebar-add-download".to_string())
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .on_mouse_up(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .child(self.render_add_button(cx)),
            )
            .when(is_windows_or_linux, |element| {
                element.child(
                    div()
                        .flex_none()
                        .px_2()
                        .child(Separator::vertical().h(px(16.0))),
                )
            });

        let window_controls = if is_windows_or_linux {
            let is_maximized = window.is_maximized();
            div()
                .id("window-controls")
                .flex()
                .flex_row()
                .items_center()
                .flex_shrink_0()
                .h_full()
                .child(
                    div()
                        .id("minimize")
                        .flex()
                        .w(px(WINDOW_CONTROL_BUTTON_WIDTH))
                        .h_full()
                        .flex_shrink_0()
                        .justify_center()
                        .items_center()
                        .text_color(colors.text_primary)
                        .hover(|style| {
                            style
                                .bg(colors.surface_hover)
                                .text_color(colors.text_primary)
                        })
                        .active(|style| {
                            style
                                .bg(colors.surface_active)
                                .text_color(colors.text_primary)
                        })
                        .when(is_windows, |this| {
                            this.window_control_area(WindowControlArea::Min)
                        })
                        .when(is_linux, |this| {
                            this.on_mouse_down(MouseButton::Left, |_, window, cx| {
                                window.prevent_default();
                                cx.stop_propagation();
                            })
                            .on_click(|_, window, cx| {
                                cx.stop_propagation();
                                window.minimize_window();
                            })
                        })
                        .child(Icon::new(IconName::WindowMinimize).size(IconSize::Small)),
                )
                .child(
                    div()
                        .id(if is_maximized { "restore" } else { "maximize" })
                        .flex()
                        .w(px(WINDOW_CONTROL_BUTTON_WIDTH))
                        .h_full()
                        .flex_shrink_0()
                        .justify_center()
                        .items_center()
                        .text_color(colors.text_primary)
                        .hover(|style| {
                            style
                                .bg(colors.surface_hover)
                                .text_color(colors.text_primary)
                        })
                        .active(|style| {
                            style
                                .bg(colors.surface_active)
                                .text_color(colors.text_primary)
                        })
                        .when(is_windows, |this| {
                            this.window_control_area(WindowControlArea::Max)
                        })
                        .when(is_linux, |this| {
                            this.on_mouse_down(MouseButton::Left, |_, window, cx| {
                                window.prevent_default();
                                cx.stop_propagation();
                            })
                            .on_click(|_, window, cx| {
                                cx.stop_propagation();
                                window.zoom_window();
                            })
                        })
                        .child(
                            Icon::new(if is_maximized {
                                IconName::WindowRestore
                            } else {
                                IconName::WindowMaximize
                            })
                            .size(IconSize::Small),
                        ),
                )
                .child(
                    div()
                        .id("close")
                        .flex()
                        .w(px(WINDOW_CONTROL_BUTTON_WIDTH))
                        .h_full()
                        .flex_shrink_0()
                        .justify_center()
                        .items_center()
                        .text_color(colors.text_primary)
                        .hover(|style| style.bg(colors.danger).text_color(colors.text_inverse))
                        .active(|style| style.bg(colors.danger).text_color(colors.text_inverse))
                        .when(is_windows, |this| {
                            this.window_control_area(WindowControlArea::Close)
                        })
                        .when(is_linux, |this| {
                            this.on_mouse_down(MouseButton::Left, |_, window, cx| {
                                window.prevent_default();
                                cx.stop_propagation();
                            })
                            .on_click(cx.listener(
                                |this, _, window, cx| {
                                    cx.stop_propagation();
                                    if this.handle_window_close_request(cx) {
                                        window.remove_window();
                                    }
                                },
                            ))
                        })
                        .child(Icon::new(IconName::WindowClose).size(IconSize::Small)),
                )
        } else {
            div().id("window-controls")
        };

        let titlebar_left_padding = if is_macos { px(80.0) } else { px(12.0) };

        div()
            .id("title-bar")
            .flex_shrink_0()
            .flex()
            .flex_row()
            .items_center()
            .justify_between()
            .h(px(TITLEBAR_HEIGHT))
            .pl(titlebar_left_padding)
            .border_b_1()
            .border_color(colors.border)
            .bg(colors.toolbar_surface)
            .when(is_linux, |this| {
                this.on_double_click(|_, window, _| window.zoom_window())
            })
            .when(is_macos, |this| {
                this.on_double_click(|_, window, _| window.titlebar_double_click())
            })
            .when(!is_windows, |this| {
                this.on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _, _, _| {
                        this.titlebar_drag_pending = true;
                    }),
                )
                .on_mouse_up(
                    MouseButton::Left,
                    cx.listener(|this, _, _, _| {
                        this.titlebar_drag_pending = false;
                    }),
                )
                .on_mouse_move(cx.listener(|this, _, window, _| {
                    if this.titlebar_drag_pending {
                        this.titlebar_drag_pending = false;
                        window.start_window_move();
                    }
                }))
            })
            .child(
                div()
                    .id("bar")
                    .h_full()
                    .flex_1()
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .when(is_windows, |this| {
                        this.window_control_area(WindowControlArea::Drag)
                    })
                    .child(brand)
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .h_full()
                            .px_3()
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(
                                div()
                                    .w(px(search_width))
                                    .flex_none()
                                    .on_mouse_down(MouseButton::Left, |_, _, cx| {
                                        cx.stop_propagation()
                                    })
                                    .on_mouse_up(MouseButton::Left, |_, _, cx| {
                                        cx.stop_propagation()
                                    })
                                    .on_mouse_down(MouseButton::Right, |_, _, cx| {
                                        cx.stop_propagation()
                                    })
                                    .on_mouse_up(MouseButton::Right, |_, _, cx| {
                                        cx.stop_propagation()
                                    })
                                    .child(
                                        Input::new(&self.search_input)
                                            .cleanable(true)
                                            .prefix(Icon::new(IconName::Search)),
                                    ),
                            ),
                    )
                    .child(actions),
            )
            .child(window_controls)
    }

    pub(crate) fn render_add_button(&self, cx: &mut Context<Self>) -> KitButton {
        let enabled = self.snapshot.commands_available() && self.add_dialog_view().is_none();
        KitButton::new("open-add-download")
            .primary()
            .icon(gpui_kit::component::Icon::new(IconName::Plus))
            .label(self.t("action-add-download"))
            .accessibility_label(if enabled {
                self.t("action-add-download-aria")
            } else {
                self.t("add-download-unavailable")
            })
            .tooltip_with_action(
                self.t("action-add-download"),
                &OpenAddDownload,
                Some("DownloadWorkspace"),
            )
            .disabled(!enabled)
            .on_click(cx.listener(|this, _, window, cx| {
                this.open_add_download(&OpenAddDownload, window, cx);
            }))
    }

    pub(crate) fn render_sidebar(
        &mut self,
        sidebar_width: f32,
        cx: &mut Context<Self>,
    ) -> Sidebar<SidebarGroup<SidebarMenu>> {
        let colors = self.theme.colors;
        let mut filter_items = Vec::with_capacity(WorkspaceFilter::ALL.len());
        for filter in WorkspaceFilter::ALL {
            let count = filter.count(self.snapshot.counts);
            let selected = self.page == AppPage::Downloads && self.query.filter == filter;
            let label = self.t(filter.message_key());
            let aria_label = format!("{label}, {count}");
            let icon = filter_icon(filter);
            filter_items.push(
                SidebarMenuItem::new(label)
                    .icon(gpui_kit::component::Icon::new(icon))
                    .accessibility_label(aria_label)
                    .active(selected)
                    .suffix(move |_, _| {
                        div()
                            .h(px(20.0))
                            .min_w(px(22.0))
                            .px_1()
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded_full()
                            .bg(if selected {
                                with_alpha(colors.accent, 0.14)
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
                            .child(count.to_string())
                    })
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.set_filter(filter, window, cx);
                    })),
            );
        }

        let active_profile_name = self
            .profiles
            .active()
            .map(|profile| profile.name.clone())
            .unwrap_or_else(|| self.t("no-profile"));
        let active_profile_kind = self.profiles.active().map_or_else(
            || "—".to_owned(),
            |profile| {
                self.t(match profile.kind {
                    ProfileKindView::LocalManaged => "profile-kind-local-managed",
                    ProfileKindView::RemoteRpc => "profile-kind-remote-rpc",
                })
            },
        );
        let profile_count = self.profiles.profiles.len();
        let active_profile_aria = self.t_args(
            "active-profile-aria",
            &[
                ("name", FluentValue::from(active_profile_name.as_str())),
                ("kind", FluentValue::from(active_profile_kind.as_str())),
            ],
        );
        let profile_summary = self.t_args(
            "active-profile-summary",
            &[
                ("kind", FluentValue::from(active_profile_kind.as_str())),
                (
                    "count",
                    FluentValue::from(i64::try_from(profile_count).unwrap_or(i64::MAX)),
                ),
            ],
        );

        let profile_banner = div()
            .id("active-profile-banner")
            .debug_selector(|| "sidebar-profile-header".to_string())
            .role(Role::Status)
            .aria_label(active_profile_aria)
            .w_full()
            .px_3()
            .py_2()
            .rounded_md()
            .border_1()
            .border_color(colors.border)
            .bg(colors.toolbar_surface)
            .shadow_xs()
            .flex()
            .items_center()
            .gap_2p5()
            .child(
                Icon::new(IconName::Activity)
                    .size(IconSize::Small)
                    .color(colors.accent),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .child(
                        div()
                            .truncate()
                            .text_xs()
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(colors.text_primary)
                            .child(active_profile_name),
                    )
                    .child(
                        div()
                            .truncate()
                            .text_xs()
                            .text_color(colors.text_muted)
                            .child(profile_summary),
                    ),
            );

        let settings_footer = div()
            .w_full()
            .pt_2()
            .border_t_1()
            .border_color(colors.border)
            .debug_selector(|| "sidebar-settings-footer".to_string())
            .child(
                KitButton::new("open-settings")
                    .ghost()
                    .small()
                    .w_full()
                    .justify_start()
                    .selected(self.page == AppPage::Settings)
                    .icon(gpui_kit::component::Icon::new(IconName::Settings))
                    .label(self.t("settings-label"))
                    .accessibility_label(self.t("action-open-settings"))
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.open_settings(&OpenSettings, window, cx);
                    })),
            );

        let transfers_group = SidebarGroup::new(self.t("settings-nav-transfers"))
            .child(SidebarMenu::new().children(filter_items));

        Sidebar::new("app-sidebar")
            .collapsible(SidebarCollapsible::None)
            .w(px(sidebar_width))
            .header(profile_banner)
            .child(transfers_group)
            .when_some(self.render_sidebar_categories(cx), |sidebar, group| {
                sidebar.child(group)
            })
            .footer(settings_footer)
    }

    fn render_sidebar_categories(
        &mut self,
        cx: &mut Context<Self>,
    ) -> Option<SidebarGroup<SidebarMenu>> {
        let categories = self.settings.categories.clone();
        if categories.is_empty() {
            return None;
        }
        let selected = self.query.category_id.clone();
        let all_selected = selected.is_none();
        let mut items = Vec::with_capacity(categories.len() + 1);
        let all_label = self.t("filter-category-all");
        items.push(
            SidebarMenuItem::new(all_label.clone())
                .icon(gpui_kit::component::Icon::new(IconName::FolderDown))
                .accessibility_label(all_label)
                .active(all_selected)
                .on_click(cx.listener(|this, _, window, cx| {
                    this.set_category_filter(None, window, cx);
                })),
        );
        for category in categories {
            let id = category.id.clone();
            let is_selected = selected.as_deref() == Some(id.as_str());
            let name = category.name.clone();
            let display_name = category_display_name(&name, &self.translator);
            let icon = category_icon(&name);
            items.push(
                SidebarMenuItem::new(display_name.clone())
                    .icon(gpui_kit::component::Icon::new(icon))
                    .accessibility_label(display_name)
                    .active(is_selected)
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.set_category_filter(Some(id.clone()), window, cx);
                    })),
            );
        }
        Some(
            SidebarGroup::new(self.t("settings-categories"))
                .child(SidebarMenu::new().children(items)),
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
        let pad_len = SPEED_CHART_SAMPLES.saturating_sub(visible.len());
        let mut points = Vec::with_capacity(SPEED_CHART_SAMPLES);
        for index in 0..SPEED_CHART_SAMPLES {
            let seconds_ago = SPEED_CHART_SAMPLES - 1 - index;
            let label = if seconds_ago == 0 {
                SharedString::from("Now")
            } else {
                SharedString::from(format!("-{seconds_ago}s"))
            };
            let sample = index.checked_sub(pad_len).and_then(|i| visible.get(i));
            points.push(SpeedChartPoint {
                label,
                download_rate: sample.map_or(0.0, |s| s.download_rate as f64),
                upload_rate: sample.map_or(0.0, |s| s.upload_rate as f64),
            });
        }

        let area_chart = AreaChart::new(points)
            .id("speed-history-area-chart")
            .x(|point| point.label.clone())
            .y(|point| point.download_rate)
            .stroke(colors.progress_download)
            .fill(with_alpha(colors.progress_download, 0.22))
            .name(self.t("speed-download"))
            .linear()
            .y(|point| point.upload_rate)
            .stroke(colors.progress_upload)
            .fill(with_alpha(colors.progress_upload, 0.18))
            .name(self.t("speed-upload"))
            .linear()
            .x_axis(false)
            .y_axis(false)
            .grid(true)
            .y_tick_count(3)
            .y_padding(4.0, 0.0)
            .y_domain(0.0, max_rate.max(1) as f64)
            .appear(false)
            .tooltip_value(|_, _, value| format_rate(value.max(0.0).round() as u64).into());

        div()
            .id("speed-history-chart")
            .role(Role::Group)
            .aria_label(self.t_args(
                "speed-chart-aria",
                &[
                    (
                        "download",
                        FluentValue::from(format_rate(self.snapshot.download_rate)),
                    ),
                    (
                        "upload",
                        FluentValue::from(format_rate(self.snapshot.upload_rate)),
                    ),
                    ("peak", FluentValue::from(format_rate(max_rate))),
                ],
            ))
            .h(px(156.0))
            .w(px(300.0))
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
            .child(div().h(px(76.0)).w_full().flex_none().child(area_chart))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_3()
                    .text_xs()
                    .text_color(colors.text_muted)
                    .child(speed_chart_legend(
                        self.t("speed-download"),
                        colors.progress_download,
                        colors,
                    ))
                    .child(speed_chart_legend(
                        self.t("speed-upload"),
                        colors.progress_upload,
                        colors,
                    )),
            )
    }

    pub(crate) fn render_status_bar(&mut self, cx: &mut Context<Self>) -> StatusBar {
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
            .px_1p5()
            .flex()
            .items_center()
            .gap_1()
            .text_xs()
            .text_color(colors.text_secondary)
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

        let engine_status = div()
            .id("engine-status")
            .role(Role::Status)
            .aria_label(self.t(self.engine_health.message_key()))
            .h_full()
            .px_1p5()
            .flex()
            .items_center()
            .gap_1()
            .text_xs()
            .text_color(colors.text_secondary)
            .child(
                StatusIndicator::new(engine_health_color(&self.engine_health, colors)).icon(
                    match &self.engine_health {
                        EngineHealthView::Failed { .. } => IconName::CircleAlert,
                        EngineHealthView::Restarting { .. } => IconName::RefreshCw,
                        EngineHealthView::Running { restarts } if *restarts > 0 => {
                            IconName::TriangleAlert
                        }
                        EngineHealthView::External | EngineHealthView::Running { .. } => {
                            IconName::Activity
                        }
                    },
                ),
            )
            .child(self.t(self.engine_health.message_key()));

        let stale_status = self.snapshot.stale.then(|| {
            div()
                .id("stale-status")
                .role(Role::Status)
                .h_full()
                .px_1p5()
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
                .child(self.t("stale-data"))
        });

        let stopped_status = self.snapshot.stopped_history.summary_label().map(|label| {
            let can_load = self.snapshot.stopped_history.can_load_more
                && self.snapshot.connection.is_connected()
                && !self.snapshot.stale;
            let pending = self.pending_load_more_stopped;
            div()
                .id("stopped-history-status")
                .role(if can_load { Role::Button } else { Role::Status })
                .aria_label(if can_load {
                    self.t_args(
                        "stopped-history-load-more-aria",
                        &[("summary", FluentValue::from(label.as_str()))],
                    )
                } else {
                    label.clone()
                })
                .h_full()
                .px_1p5()
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
                })
        });

        let activity_popover = {
            let activity_open = self.active_status_popover == Some(StatusBarPopover::Activity);
            let activity_count = self.activity_log.len();
            let activity_label = if activity_count == 0 {
                self.t("activity-history")
            } else {
                format!("{}, {activity_count}", self.t("activity-history"))
            };
            let trigger = KitButton::new("activity-status")
                .ghost()
                .xsmall()
                .compact()
                .accessibility_label(activity_label)
                .icon(
                    gpui_kit::component::Icon::new(IconName::Activity).text_color(
                        if activity_open {
                            colors.information
                        } else {
                            colors.text_muted
                        },
                    ),
                )
                .label(if activity_count == 0 {
                    self.t("activity-title")
                } else {
                    self.t_count("activity-count", activity_count as u64)
                });
            let shell_change = cx.entity().downgrade();
            let shell_content = cx.entity().downgrade();
            Popover::new("activity-panel")
                .anchor(Anchor::BottomRight)
                .open(activity_open)
                .on_open_change(move |next_open, window, cx| {
                    let _ = shell_change.update(cx, |this, cx| {
                        if *next_open {
                            this.context_menu_target = None;
                            this.active_status_popover = Some(StatusBarPopover::Activity);
                            cx.notify();
                        } else {
                            this.close_activity_panel(window, cx);
                        }
                    });
                })
                .trigger(trigger)
                .content(move |_, _, cx| {
                    shell_content
                        .update(cx, |this, cx| this.render_activity_panel_content(cx))
                        .unwrap_or_else(|_| div().into_any_element())
                })
        };

        let speed_popover = {
            let speed_open = self.active_status_popover == Some(StatusBarPopover::Speed);
            let speed_aria = self.t_args(
                "transfer-speed-aria",
                &[
                    (
                        "download",
                        FluentValue::from(format_rate(self.snapshot.download_rate)),
                    ),
                    (
                        "upload",
                        FluentValue::from(format_rate(self.snapshot.upload_rate)),
                    ),
                ],
            );
            let trigger = KitButton::new("transfer-status")
                .ghost()
                .xsmall()
                .compact()
                .accessibility_label(speed_aria)
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .font_features(tabular_numbers())
                        .text_xs()
                        .text_color(colors.text_muted)
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
                );
            let shell_change = cx.entity().downgrade();
            let shell_content = cx.entity().downgrade();
            Popover::new("speed-popover")
                .anchor(Anchor::BottomRight)
                .appearance(false)
                .open(speed_open)
                .on_open_change(move |next_open, window, cx| {
                    let _ = shell_change.update(cx, |this, cx| {
                        if *next_open {
                            this.speed_popover_previous_focus =
                                window.focused(cx).map(|focus| focus.downgrade());
                            this.context_menu_target = None;
                            this.active_status_popover = Some(StatusBarPopover::Speed);
                            cx.notify();
                        } else {
                            this.close_speed_popover(window, cx);
                        }
                    });
                })
                .trigger(trigger)
                .content(move |_, _, cx| {
                    shell_content
                        .read_with(cx, |this, _| this.render_speed_chart().into_any_element())
                        .unwrap_or_else(|_| div().into_any_element())
                })
        };

        StatusBar::new()
            .h(px(28.0))
            .flex_none()
            .py_0()
            .left(status_button)
            .left(Separator::vertical().h(px(14.0)))
            .left(engine_status)
            .when_some(stale_status, |bar, item| {
                bar.left(Separator::vertical().h(px(14.0))).left(item)
            })
            .when_some(stopped_status, |bar, item| {
                bar.left(Separator::vertical().h(px(14.0))).left(item)
            })
            .right(self.render_power_status_bar_item(cx))
            .right(Separator::vertical().h(px(14.0)))
            .right(activity_popover)
            .right(Separator::vertical().h(px(14.0)))
            .right(speed_popover)
    }

    pub(crate) fn render_activity_panel_content(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let colors = self.theme.colors;
        let entries = self.activity_log.clone();
        div()
            .id("activity-panel-body")
            .w(px(ACTIVITY_PANEL_WIDTH))
            .max_h(px(420.0))
            .flex()
            .flex_col()
            .gap_2()
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
                                KitButton::new("clear-activity-log")
                                    .small()
                                    .label(self.t("button-clear"))
                                    .accessibility_label(self.t("action-clear-activity"))
                                    .disabled(entries.is_empty())
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.clear_activity_log(cx);
                                    })),
                            )
                            .child(
                                KitButton::new("close-activity-panel")
                                    .ghost()
                                    .small()
                                    .compact()
                                    .icon(gpui_kit::component::Icon::new(IconName::X))
                                    .accessibility_label(self.t("action-close-activity"))
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.close_activity_panel(window, cx);
                                    })),
                            ),
                    ),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(colors.text_muted)
                    .child(self.t("activity-description")),
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
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .min_w_0()
                    .max_h(px(320.0))
                    .flex()
                    .flex_col()
                    .overflow_hidden()
                    .child(
                        div()
                            .id("activity-panel-scroll")
                            .flex_1()
                            .min_h_0()
                            .min_w_0()
                            .pr_3()
                            .overflow_y_scroll()
                            .track_scroll(&self.activity_scroll)
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
                                    .id(SharedString::from(format!("activity-entry-{}", entry.id)))
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
                                            .child(StatusIndicator::new(kind_color).icon(
                                                match entry.kind {
                                                    ActivityKindView::Completion => {
                                                        IconName::CircleCheck
                                                    }
                                                    ActivityKindView::Error => IconName::CircleX,
                                                    ActivityKindView::Engine => {
                                                        IconName::TriangleAlert
                                                    }
                                                    ActivityKindView::Command => IconName::Activity,
                                                    ActivityKindView::Info => IconName::Info,
                                                },
                                            ))
                                            .child(
                                                div()
                                                    .text_xs()
                                                    .font_weight(FontWeight::MEDIUM)
                                                    .text_color(colors.text_muted)
                                                    .child(self.t(match entry.kind {
                                                        ActivityKindView::Completion => {
                                                            "activity-kind-completion"
                                                        }
                                                        ActivityKindView::Error => {
                                                            "activity-kind-error"
                                                        }
                                                        ActivityKindView::Engine => {
                                                            "activity-kind-engine"
                                                        }
                                                        ActivityKindView::Command => {
                                                            "activity-kind-command"
                                                        }
                                                        ActivityKindView::Info => {
                                                            "activity-kind-info"
                                                        }
                                                    })),
                                            )
                                            .when(entry.count > 1, |element| {
                                                element.child(
                                                    div()
                                                        .text_xs()
                                                        .text_color(colors.text_muted)
                                                        .child(self.t_count(
                                                            "activity-repeated-count",
                                                            u64::from(entry.count),
                                                        )),
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
                            })),
                    )
                    .vertical_scrollbar(&self.activity_scroll)
                    .into_any_element()
            })
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
            && self.add_dialog_view().is_none();

        let mut header = EmptyHeader::new()
            .media(
                EmptyMedia::new()
                    .with_variant(EmptyMediaVariant::Icon)
                    .size(px(56.0))
                    .rounded_xl()
                    .border_1()
                    .border_color(colors.border)
                    .bg(colors.surface)
                    .shadow_xs()
                    .child(Icon::new(icon).size(IconSize::Large).color(colors.accent)),
            )
            .title(EmptyTitle::new().child(title));
        if show_add {
            header =
                header.description(EmptyDescription::new().child(self.t("empty-state-subtitle")));
        }

        let has_actions = show_clear || show_add || self.snapshot.connection.can_retry();
        let mut empty = Empty::new().size_full().header(header);
        if has_actions {
            let mut actions = EmptyContent::new()
                .flex_row()
                .flex_wrap()
                .justify_center()
                .gap_2();
            if show_clear {
                actions = actions.child(
                    KitButton::new("clear-empty-filter")
                        .label(self.t("clear-filter"))
                        .accessibility_label(self.t("action-clear-search"))
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.query.filter = WorkspaceFilter::All;
                            this.set_search_query("", window, cx);
                            window.focus(&this.focus_handle, cx);
                        })),
                );
            }
            if show_add {
                actions = actions.child(
                    KitButton::new("add-download-empty-state")
                        .primary()
                        .icon(gpui_kit::component::Icon::new(IconName::Plus))
                        .label(self.t("action-add-download"))
                        .accessibility_label(self.t("action-add-download-aria"))
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.open_add_download(&OpenAddDownload, window, cx);
                        })),
                );
            }
            if self.snapshot.connection.can_retry() {
                actions = actions.child(
                    KitButton::new("retry-connection")
                        .primary()
                        .label(self.t("retry-label"))
                        .accessibility_label(self.t("connection-retry-now"))
                        .on_click(cx.listener(|this, _, _, cx| this.request_retry(cx))),
                );
            }
            empty = empty.content(actions);
        }

        empty.into_any_element()
    }
}
