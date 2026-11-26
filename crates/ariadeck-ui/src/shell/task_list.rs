//! Main task list pane for AppShell.

use super::*;

impl AppShell {
    pub(crate) fn render_task_header(
        &mut self,
        layout: TaskLayoutMode,
        selected_count: usize,
        cx: &mut Context<Self>,
    ) -> Div {
        let colors = self.theme.colors;
        let selection_state = if selected_count == 0 {
            Toggled::False
        } else if selected_count == self.snapshot.tasks.len() {
            Toggled::True
        } else {
            Toggled::Mixed
        };
        let selection_icon = match selection_state {
            Toggled::False => IconName::Square,
            Toggled::True => IconName::SquareCheckBig,
            Toggled::Mixed => IconName::SquareMinus,
        };
        let header = div()
            .h(px(36.0))
            .flex_none()
            .flex()
            .items_center()
            .gap_3()
            .px_3()
            .border_b_1()
            .border_color(colors.border)
            .bg(colors.toolbar_surface)
            .text_xs()
            .font_weight(FontWeight::MEDIUM)
            .text_color(colors.text_secondary)
            .child(
                div()
                    .id("select-all-tasks")
                    .role(Role::CheckBox)
                    .aria_label(match selection_state {
                        Toggled::True => self.t("action-clear-selection"),
                        Toggled::False | Toggled::Mixed => self.t("action-select-all-visible"),
                    })
                    .aria_toggled(selection_state)
                    .focusable()
                    .tab_stop(true)
                    .border_1()
                    .border_color(gpui::transparent_black())
                    .focus_visible(|style| style.border_color(colors.focus_ring))
                    .size(px(20.0))
                    .rounded_sm()
                    .flex_none()
                    .flex()
                    .items_center()
                    .justify_center()
                    .hover(|style| style.bg(colors.surface_hover))
                    .active(|style| style.bg(colors.surface_active))
                    .on_key_down(cx.listener(|this, event: &gpui::KeyDownEvent, window, cx| {
                        if is_activation_key(event) {
                            cx.stop_propagation();
                            this.toggle_select_all(window, cx);
                        }
                    }))
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.toggle_select_all(window, cx);
                    }))
                    .child(Icon::new(selection_icon).size(IconSize::Small).color(
                        if selected_count == 0 {
                            colors.text_muted
                        } else {
                            colors.accent
                        },
                    )),
            )
            .child(div().w(px(32.0)).flex_none());

        match layout {
            TaskLayoutMode::Wide => header
                .child(div().flex_1().min_w_0().child(self.t("ui-col-name")))
                .child(
                    div()
                        .w(px(132.0))
                        .flex_none()
                        .child(self.t("ui-col-progress-ratio")),
                )
                .child(
                    div()
                        .w(px(88.0))
                        .flex_none()
                        .child(self.t("ui-col-down-up")),
                )
                .child(div().w(px(124.0)).flex_none().child(self.t("ui-col-size")))
                .child(
                    div()
                        .w(px(72.0))
                        .flex_none()
                        .child(self.t("ui-col-eta-seed")),
                )
                .child(
                    div()
                        .w(px(86.0))
                        .flex_none()
                        .text_center()
                        .child(self.t("ui-col-status")),
                ),
            TaskLayoutMode::Compact => header
                .child(div().flex_1().min_w_0().child(self.t("ui-col-task")))
                .child(
                    div()
                        .w(px(112.0))
                        .flex_none()
                        .child(self.t("ui-col-progress")),
                )
                .child(
                    div()
                        .w(px(78.0))
                        .flex_none()
                        .text_center()
                        .child(self.t("ui-col-status")),
                ),
        }
    }

    pub(crate) fn render_main(
        &mut self,
        layout: TaskLayoutMode,
        details_presentation: DetailsPresentation,
        cx: &mut Context<Self>,
    ) -> Div {
        let colors = self.theme.colors;
        let task_count = self.snapshot.tasks.len();
        let selected_count = self.visible_selected_task_count();
        let hidden_selected_count = self.selected_tasks.len().saturating_sub(selected_count);
        let content = if task_count == 0 {
            self.render_empty_state(cx)
        } else {
            div()
                .id("download-task-list")
                .role(Role::List)
                .aria_label(self.t_count(
                    "download-list-aria",
                    u64::try_from(task_count).unwrap_or(u64::MAX),
                ))
                .relative()
                .size_full()
                .overflow_hidden()
                .child(
                    uniform_list(
                        "download-tasks",
                        task_count,
                        cx.processor(move |this, range: Range<usize>, _window, cx| {
                            this.rendered_range = range.clone();
                            range
                                .filter_map(|index| {
                                    this.snapshot
                                        .tasks
                                        .get(index)
                                        .map(|task| this.render_task_row(index, task, layout, cx))
                                })
                                .collect::<Vec<_>>()
                        }),
                    )
                    .track_scroll(&self.list_scroll)
                    .h_full()
                    .w_full(),
                )
                .vertical_scrollbar(&self.list_scroll)
                .into_any_element()
        };

        let center = div()
            .flex_1()
            .min_w_0()
            .min_h_0()
            .flex()
            .flex_col()
            .bg(colors.background)
            .child(
                div()
                    .h(px(48.0))
                    .flex_none()
                    .flex()
                    .items_center()
                    .justify_between()
                    .px_4()
                    .bg(colors.toolbar_surface)
                    .child(
                        div()
                            .flex()
                            .items_baseline()
                            .gap_2()
                            .child(
                                div()
                                    .text_base()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(self.t(self.query.filter.message_key())),
                            )
                            .child(
                                div()
                                    .font_features(tabular_numbers())
                                    .text_xs()
                                    .text_color(colors.text_muted)
                                    .child(self.t_count(
                                        "visible-task-count",
                                        u64::try_from(task_count).unwrap_or(u64::MAX),
                                    )),
                            )
                            .when(selected_count > 0 || hidden_selected_count > 0, |element| {
                                element.child(
                                    div()
                                        .font_features(tabular_numbers())
                                        .text_xs()
                                        .text_color(colors.text_secondary)
                                        .child(if hidden_selected_count > 0 {
                                            self.t_args(
                                                "selected-task-count-with-hidden",
                                                &[
                                                    (
                                                        "selected",
                                                        FluentValue::from(
                                                            i64::try_from(selected_count)
                                                                .unwrap_or(i64::MAX),
                                                        ),
                                                    ),
                                                    (
                                                        "hidden",
                                                        FluentValue::from(
                                                            i64::try_from(hidden_selected_count)
                                                                .unwrap_or(i64::MAX),
                                                        ),
                                                    ),
                                                ],
                                            )
                                        } else {
                                            self.t_count(
                                                "selected-task-count",
                                                u64::try_from(selected_count).unwrap_or(u64::MAX),
                                            )
                                        }),
                                )
                            })
                            .child(self.render_list_controls(cx)),
                    )
                    .child(self.render_task_toolbar(selected_count, cx)),
            )
            .child(self.render_task_header(layout, selected_count, cx))
            .child(div().flex_1().min_w_0().min_h_0().child(content));

        let pane = div()
            .relative()
            .flex_1()
            .min_w_0()
            .min_h_0()
            .flex()
            .child(center);

        match (self.details_panel.as_ref(), details_presentation) {
            (Some(panel), DetailsPresentation::Inline) => pane.child(panel.clone()),
            // Narrow Overlay presentation is rendered via `WindowExt::open_sheet` in Root.
            (Some(_), DetailsPresentation::Overlay) | (None, _) => pane,
        }
    }

    pub(crate) fn populate_task_context_menu(
        &self,
        menu: PopupMenu,
        identity: &TaskIdentity,
        shell: gpui::WeakEntity<Self>,
    ) -> PopupMenu {
        let Some(task) = self
            .snapshot
            .tasks
            .iter()
            .find(|task| &task.identity == identity)
        else {
            return menu;
        };
        let idle = self.pending_task_command.is_none()
            && self.pending_batch_command.is_none()
            && self.active_modal.is_none();
        let commands_available = self.snapshot.commands_available() && idle;
        let path_actions = commands_available && self.snapshot.local_path_actions_available;
        let can_pause = commands_available && task.status.can_pause();
        let can_force_pause = can_pause && self.snapshot.capabilities.force_pause;
        let can_resume = commands_available && task.status.can_resume();
        let can_retry = commands_available && task.status.can_retry();
        let can_remove = commands_available && task.status.can_remove();
        let can_queue = commands_available
            && task.status.can_move_in_queue()
            && self.queue_reordering_available();
        let can_output = commands_available && task.can_set_output_name();
        let can_speed = commands_available
            && task.status.can_set_speed_limit()
            && self.snapshot.capabilities.change_option;
        let has_source = task
            .primary_source
            .as_deref()
            .is_some_and(|value| !value.is_empty());

        let make_item = |action: ContextMenuAction,
                         label: String,
                         shortcut_action: Option<Box<dyn gpui::Action>>,
                         enabled: bool| {
            let shell = shell.clone();
            let mut item =
                PopupMenuItem::new(label)
                    .disabled(!enabled)
                    .on_click(move |_, window, cx| {
                        let _ = shell.update(cx, |this, cx| {
                            this.activate_context_menu_action(action, window, cx);
                        });
                    });
            if let Some(shortcut_action) = shortcut_action {
                item = item.action(shortcut_action);
            }
            item
        };

        let mut menu = menu
            .action_context(self.focus_handle.clone())
            .min_w(px(220.0))
            .item(make_item(
                ContextMenuAction::Details,
                self.t("action-details"),
                Some(Box::new(OpenTaskDetails)),
                true,
            ))
            .item(make_item(
                ContextMenuAction::OpenDownload,
                self.t("action-open-file"),
                None,
                path_actions,
            ))
            .item(make_item(
                ContextMenuAction::OpenFolder,
                self.t("action-open-folder"),
                None,
                path_actions,
            ))
            .separator()
            .item(make_item(
                ContextMenuAction::CopySource,
                self.t("action-copy-source"),
                None,
                has_source,
            ))
            .item(make_item(
                ContextMenuAction::CopyGid,
                self.t("action-copy-gid"),
                None,
                true,
            ));

        let has_control =
            task.status.can_pause() || task.status.can_resume() || task.status.can_retry();
        if has_control {
            menu = menu.separator();
            if task.status.can_pause() {
                menu = menu
                    .item(make_item(
                        ContextMenuAction::Pause,
                        self.t("action-pause"),
                        Some(Box::new(PauseSelectedTask)),
                        can_pause,
                    ))
                    .item(make_item(
                        ContextMenuAction::ForcePause,
                        self.t("action-force-pause"),
                        None,
                        can_force_pause,
                    ));
            }
            if task.status.can_resume() {
                menu = menu.item(make_item(
                    ContextMenuAction::Resume,
                    self.t("action-resume"),
                    Some(Box::new(ResumeSelectedTask)),
                    can_resume,
                ));
            }
            if task.status.can_retry() {
                menu = menu.item(make_item(
                    ContextMenuAction::Retry,
                    self.t("action-retry"),
                    Some(Box::new(RetrySelectedTask)),
                    can_retry,
                ));
            }
        }

        if task.status.can_move_in_queue() {
            menu = menu
                .separator()
                .item(make_item(
                    ContextMenuAction::MoveTop,
                    self.t("action-move-queue-top"),
                    Some(Box::new(MoveTaskToQueueTop)),
                    can_queue,
                ))
                .item(make_item(
                    ContextMenuAction::MoveUp,
                    self.t("action-move-queue-up"),
                    Some(Box::new(MoveTaskUpInQueue)),
                    can_queue,
                ))
                .item(make_item(
                    ContextMenuAction::MoveDown,
                    self.t("action-move-queue-down"),
                    Some(Box::new(MoveTaskDownInQueue)),
                    can_queue,
                ))
                .item(make_item(
                    ContextMenuAction::MoveBottom,
                    self.t("action-move-queue-bottom"),
                    Some(Box::new(MoveTaskToQueueBottom)),
                    can_queue,
                ));
        }

        if task.can_set_output_name() || task.status.can_set_speed_limit() {
            menu = menu.separator();
            if task.can_set_output_name() {
                menu = menu.item(make_item(
                    ContextMenuAction::OutputName,
                    self.t("action-change-output-name"),
                    Some(Box::new(OpenTaskOutputName)),
                    can_output,
                ));
            }
            if task.status.can_set_speed_limit() {
                menu = menu
                    .item(make_item(
                        ContextMenuAction::SpeedLimit,
                        self.t("action-set-speed-limits"),
                        None,
                        can_speed,
                    ))
                    .item(make_item(
                        ContextMenuAction::TaskOptions,
                        self.t("action-edit-task-options"),
                        None,
                        can_speed,
                    ));
            }
        }

        let remove_label = self.t("action-remove");
        let danger = self.theme.colors.danger;
        let muted = self.theme.colors.text_muted;
        let remove_shell = shell.clone();
        menu.separator().item(
            PopupMenuItem::element(move |_, _| {
                div()
                    .text_color(if can_remove { danger } else { muted })
                    .child(remove_label.clone())
            })
            .action(Box::new(RemoveSelectedTask))
            .disabled(!can_remove)
            .on_click(move |_, window, cx| {
                let _ = remove_shell.update(cx, |this, cx| {
                    this.activate_context_menu_action(ContextMenuAction::Remove, window, cx);
                });
            }),
        )
    }

    pub(crate) fn activate_context_menu_action(
        &mut self,
        action: ContextMenuAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Capture the right-clicked task before closing so multi-select still
        // has a single authoritative target for details/copy/open.
        let menu_task = self.context_menu_task_view(cx);
        self.close_task_context_menu(cx);
        match action {
            ContextMenuAction::Details => {
                if let Some(task) = menu_task.or_else(|| self.selected_task_view(cx)) {
                    self.open_details_for(task, cx);
                }
            }
            ContextMenuAction::OpenDownload => {
                self.request_task_open_for_selection(TaskOpenTargetView::Download, cx);
            }
            ContextMenuAction::OpenFolder => {
                self.request_task_open_for_selection(TaskOpenTargetView::Folder, cx);
            }
            ContextMenuAction::CopySource => {
                if let Some(task) = menu_task.or_else(|| self.selected_task_view(cx)) {
                    self.copy_task_source(&task, cx);
                }
            }
            ContextMenuAction::CopyGid => {
                if let Some(task) = menu_task.or_else(|| self.selected_task_view(cx)) {
                    self.copy_task_gid(&task, cx);
                }
            }
            ContextMenuAction::Pause => {
                self.begin_task_command(TaskCommandView::Pause, cx);
            }
            ContextMenuAction::ForcePause => {
                self.begin_task_command(TaskCommandView::ForcePause, cx);
            }
            ContextMenuAction::Resume => {
                self.begin_task_command(TaskCommandView::Resume, cx);
            }
            ContextMenuAction::Retry => {
                self.begin_task_command(TaskCommandView::Retry, cx);
            }
            ContextMenuAction::MoveTop => {
                self.begin_task_command(TaskCommandView::MoveToQueueTop, cx);
            }
            ContextMenuAction::MoveUp => {
                self.begin_task_command(TaskCommandView::MoveUpInQueue, cx);
            }
            ContextMenuAction::MoveDown => {
                self.begin_task_command(TaskCommandView::MoveDownInQueue, cx);
            }
            ContextMenuAction::MoveBottom => {
                self.begin_task_command(TaskCommandView::MoveToQueueBottom, cx);
            }
            ContextMenuAction::OutputName => self.open_task_output_name(window, cx),
            ContextMenuAction::SpeedLimit => self.open_task_speed_limit(window, cx),
            ContextMenuAction::TaskOptions => self.open_task_options(window, cx),
            ContextMenuAction::Remove => self.confirm_remove_selected(window, cx),
        }
    }

    /// Sort menu and engine-wide pause-all/resume-all controls (D-014).
    pub(crate) fn render_list_controls(&mut self, cx: &mut Context<Self>) -> Div {
        let idle = self.pending_task_command.is_none()
            && self.pending_global_task_command.is_none()
            && self.pending_batch_command.is_none()
            && !matches!(self.active_modal, Some(ActiveModal::RemoveConfirmation(_)));
        let commands_available = self.snapshot.commands_available() && idle;
        let pending_global = self
            .pending_global_task_command
            .as_ref()
            .map(|pending| pending.command);
        let sort_label = self.t(sort_key_message_key(self.query.sort_key));

        div()
            .ml_2()
            .flex()
            .items_center()
            .gap_1()
            .child(
                KitButton::new("pause-all-action")
                    .ghost()
                    .small()
                    .compact()
                    .icon(gpui_kit::component::Icon::new(IconName::Pause))
                    .accessibility_label(self.t("action-pause-all-aria"))
                    .disabled(!commands_available)
                    .loading(pending_global == Some(GlobalTaskCommandView::PauseAll))
                    .tooltip(self.t("action-pause-all"))
                    .when(commands_available, |button| {
                        button.on_click(cx.listener(|this, _, _, cx| {
                            this.begin_global_task_command(GlobalTaskCommandView::PauseAll, cx);
                        }))
                    }),
            )
            .child(
                KitButton::new("force-pause-all-action")
                    .ghost()
                    .small()
                    .compact()
                    .icon(gpui_kit::component::Icon::new(IconName::Square))
                    .accessibility_label(self.t("action-force-pause-all-aria"))
                    .disabled(!commands_available || !self.snapshot.capabilities.force_pause_all)
                    .loading(pending_global == Some(GlobalTaskCommandView::ForcePauseAll))
                    .tooltip(if self.snapshot.capabilities.force_pause_all {
                        self.t("action-force-pause-all")
                    } else {
                        self.t("notice-capability-force-pause-all-unsupported")
                    })
                    .when(
                        commands_available && self.snapshot.capabilities.force_pause_all,
                        |button| {
                            button.on_click(cx.listener(|this, _, _, cx| {
                                this.begin_global_task_command(
                                    GlobalTaskCommandView::ForcePauseAll,
                                    cx,
                                );
                            }))
                        },
                    ),
            )
            .child(
                KitButton::new("resume-all-action")
                    .ghost()
                    .small()
                    .compact()
                    .icon(gpui_kit::component::Icon::new(IconName::Play))
                    .accessibility_label(self.t("action-resume-all-aria"))
                    .disabled(!commands_available)
                    .loading(pending_global == Some(GlobalTaskCommandView::ResumeAll))
                    .tooltip(self.t("action-resume-all"))
                    .when(commands_available, |button| {
                        button.on_click(cx.listener(|this, _, _, cx| {
                            this.begin_global_task_command(GlobalTaskCommandView::ResumeAll, cx);
                        }))
                    }),
            )
            .child({
                let shell = cx.entity().downgrade();
                let sort_aria = self.t_args(
                    "sort-by-aria",
                    &[("key", FluentValue::from(sort_label.as_str()))],
                );
                KitButton::new("sort-menu-trigger")
                    .ghost()
                    .small()
                    .compact()
                    .accessibility_label(sort_aria)
                    .icon(gpui_kit::component::Icon::new(IconName::ArrowUpDown))
                    .label(sort_label)
                    .dropdown_menu(move |menu, _, cx| {
                        let Some(entity) = shell.upgrade() else {
                            return menu;
                        };
                        entity.read(cx).populate_sort_menu(menu, shell.clone())
                    })
            })
    }

    pub(crate) fn populate_sort_menu(
        &self,
        menu: PopupMenu,
        shell: gpui::WeakEntity<Self>,
    ) -> PopupMenu {
        let current_key = self.query.sort_key;
        let current_direction = self.query.sort_direction;

        let mut menu = menu.min_w(px(200.0)).label(self.t("ui-sort-by"));
        for key in WorkspaceSortKey::ALL {
            let selected = key == current_key;
            let key_label = self.t(sort_key_message_key(key));
            let shell = shell.clone();
            menu = menu.item(PopupMenuItem::new(key_label).checked(selected).on_click(
                move |_, _, cx| {
                    let _ = shell.update(cx, |this, cx| this.set_sort_key(key, cx));
                },
            ));
        }

        menu = menu.separator();
        for direction in [
            WorkspaceSortDirection::Ascending,
            WorkspaceSortDirection::Descending,
        ] {
            let selected = direction == current_direction;
            let direction_label = self.t(sort_direction_message_key(direction));
            let shell = shell.clone();
            menu = menu.item(
                PopupMenuItem::new(direction_label)
                    .checked(selected)
                    .on_click(move |_, _, cx| {
                        let _ = shell.update(cx, |this, cx| this.set_sort_direction(direction, cx));
                    }),
            );
        }
        menu
    }

    pub(crate) fn render_task_toolbar(
        &mut self,
        visible_selected_count: usize,
        cx: &mut Context<Self>,
    ) -> Div {
        if visible_selected_count > 1 {
            return self.render_batch_task_toolbar(cx);
        }
        if !self.selected_tasks.is_empty() && self.selected_tasks.len() > visible_selected_count {
            return self.render_hidden_selection_toolbar(visible_selected_count, cx);
        }
        let colors = self.theme.colors;
        let Some(task) = self.selected_task_view(cx) else {
            return div();
        };
        let idle = self.pending_task_command.is_none()
            && self.pending_batch_command.is_none()
            && !matches!(
                self.active_modal,
                Some(ActiveModal::RemoveConfirmation(_) | ActiveModal::OutputName { .. })
            );
        let pending_command = self
            .pending_task_command
            .as_ref()
            .map(|pending| pending.command.clone());
        let commands_available = self.snapshot.commands_available() && idle;
        let details_enabled = self.snapshot.commands_available();
        let pause_enabled = commands_available && task.status.can_pause();
        let force_pause_enabled = pause_enabled && self.snapshot.capabilities.force_pause;
        let resume_enabled = commands_available && task.status.can_resume();
        let retry_enabled = commands_available && task.status.can_retry();
        let remove_enabled = commands_available && task.status.can_remove();

        div()
            .flex()
            .items_center()
            .gap_1()
            .child(
                toolbar_icon_button(
                    "task-details-action",
                    IconName::PanelRight,
                    self.t("action-details"),
                    ToolbarButtonState::from_flags(details_enabled, false),
                    false,
                    Some(&OpenTaskDetails),
                    colors,
                    &self.translator,
                )
                .when(details_enabled, |button| {
                    button.on_click(cx.listener(|this, _, _window, cx| {
                        if let Some(task) = this.selected_task_view(cx) {
                            this.open_details_for(task, cx);
                        }
                    }))
                }),
            )
            .when(task.status.can_pause(), |element| {
                element
                    .child(
                        toolbar_icon_button(
                            "pause-task-action",
                            IconName::Pause,
                            self.t("action-pause"),
                            ToolbarButtonState::from_flags(
                                pause_enabled,
                                pending_command == Some(TaskCommandView::Pause),
                            ),
                            false,
                            Some(&PauseSelectedTask),
                            colors,
                            &self.translator,
                        )
                        .when(pause_enabled, |button| {
                            button.on_click(cx.listener(|this, _, _window, cx| {
                                this.begin_task_command(TaskCommandView::Pause, cx);
                            }))
                        }),
                    )
                    .child(
                        toolbar_icon_button(
                            "force-pause-task-action",
                            IconName::Square,
                            if self.snapshot.capabilities.force_pause {
                                self.t("action-force-pause")
                            } else {
                                self.t("notice-force-pause-unsupported")
                            },
                            ToolbarButtonState::from_flags(
                                force_pause_enabled,
                                pending_command == Some(TaskCommandView::ForcePause),
                            ),
                            false,
                            None,
                            colors,
                            &self.translator,
                        )
                        .when(force_pause_enabled, |button| {
                            button.on_click(cx.listener(|this, _, _window, cx| {
                                this.begin_task_command(TaskCommandView::ForcePause, cx);
                            }))
                        }),
                    )
            })
            .when(task.status.can_resume(), |element| {
                element.child(
                    toolbar_icon_button(
                        "resume-task-action",
                        IconName::Play,
                        self.t("action-resume"),
                        ToolbarButtonState::from_flags(
                            resume_enabled,
                            pending_command == Some(TaskCommandView::Resume),
                        ),
                        false,
                        Some(&ResumeSelectedTask),
                        colors,
                        &self.translator,
                    )
                    .when(resume_enabled, |button| {
                        button.on_click(cx.listener(|this, _, _window, cx| {
                            this.begin_task_command(TaskCommandView::Resume, cx);
                        }))
                    }),
                )
            })
            .when(
                task.status.can_move_in_queue() && self.queue_reordering_available(),
                |element| {
                    let queue_enabled = commands_available;
                    element.children([
                        queue_move_button(
                            "queue-move-top-action",
                            IconName::ChevronsUp,
                            self.t("action-move-queue-top"),
                            TaskCommandView::MoveToQueueTop,
                            queue_enabled,
                            pending_command.as_ref(),
                            Some(&MoveTaskToQueueTop),
                            colors,
                            &self.translator,
                            cx,
                        ),
                        queue_move_button(
                            "queue-move-up-action",
                            IconName::ChevronUp,
                            self.t("action-move-queue-up"),
                            TaskCommandView::MoveUpInQueue,
                            queue_enabled,
                            pending_command.as_ref(),
                            Some(&MoveTaskUpInQueue),
                            colors,
                            &self.translator,
                            cx,
                        ),
                        queue_move_button(
                            "queue-move-down-action",
                            IconName::ChevronDown,
                            self.t("action-move-queue-down"),
                            TaskCommandView::MoveDownInQueue,
                            queue_enabled,
                            pending_command.as_ref(),
                            Some(&MoveTaskDownInQueue),
                            colors,
                            &self.translator,
                            cx,
                        ),
                        queue_move_button(
                            "queue-move-bottom-action",
                            IconName::ChevronsDown,
                            self.t("action-move-queue-bottom"),
                            TaskCommandView::MoveToQueueBottom,
                            queue_enabled,
                            pending_command.as_ref(),
                            Some(&MoveTaskToQueueBottom),
                            colors,
                            &self.translator,
                            cx,
                        ),
                    ])
                },
            )
            .when(task.status.can_retry(), |element| {
                element.child(
                    toolbar_icon_button(
                        "retry-task-action",
                        IconName::RotateCcw,
                        self.t("action-retry"),
                        ToolbarButtonState::from_flags(
                            retry_enabled,
                            pending_command == Some(TaskCommandView::Retry),
                        ),
                        false,
                        Some(&RetrySelectedTask),
                        colors,
                        &self.translator,
                    )
                    .when(retry_enabled, |button| {
                        button.on_click(cx.listener(|this, _, _window, cx| {
                            this.begin_task_command(TaskCommandView::Retry, cx);
                        }))
                    }),
                )
            })
            .when(task.can_set_output_name(), |element| {
                element.child(
                    toolbar_icon_button(
                        "task-output-name-action",
                        IconName::Pencil,
                        self.t("action-change-output-name"),
                        ToolbarButtonState::from_flags(commands_available, false),
                        false,
                        Some(&OpenTaskOutputName),
                        colors,
                        &self.translator,
                    )
                    .when(commands_available, |button| {
                        button.on_click(cx.listener(|this, _, window, cx| {
                            this.open_task_output_name(window, cx);
                        }))
                    }),
                )
            })
            .when(task.status.can_set_speed_limit(), |element| {
                element
                    .child(
                        toolbar_icon_button(
                            "task-speed-limit-action",
                            IconName::ArrowUpDown,
                            self.t("action-set-speed-limits"),
                            ToolbarButtonState::from_flags(commands_available, false),
                            false,
                            None,
                            colors,
                            &self.translator,
                        )
                        .when(commands_available, |button| {
                            button.on_click(cx.listener(|this, _, window, cx| {
                                this.open_task_speed_limit(window, cx);
                            }))
                        }),
                    )
                    .child(
                        toolbar_icon_button(
                            "task-options-action",
                            IconName::Settings,
                            self.t("action-edit-task-options"),
                            ToolbarButtonState::from_flags(commands_available, false),
                            false,
                            None,
                            colors,
                            &self.translator,
                        )
                        .when(commands_available, |button| {
                            button.on_click(cx.listener(|this, _, window, cx| {
                                this.open_task_options(window, cx);
                            }))
                        }),
                    )
            })
            .child(
                toolbar_icon_button(
                    "remove-task-action",
                    IconName::Trash2,
                    self.t("action-remove"),
                    ToolbarButtonState::from_flags(
                        remove_enabled,
                        matches!(
                            pending_command,
                            Some(
                                TaskCommandView::RemoveTask
                                    | TaskCommandView::ForceRemoveTask
                                    | TaskCommandView::RemoveTaskAndFiles
                            )
                        ),
                    ),
                    true,
                    Some(&RemoveSelectedTask),
                    colors,
                    &self.translator,
                )
                .when(remove_enabled, |button| {
                    button.on_click(cx.listener(|this, _, window, cx| {
                        this.confirm_remove_selected(window, cx);
                    }))
                }),
            )
    }

    pub(crate) fn render_batch_task_toolbar(&mut self, cx: &mut Context<Self>) -> Div {
        let colors = self.theme.colors;
        let selected = self
            .snapshot
            .tasks
            .iter()
            .filter(|task| self.selected_tasks.contains(&task.identity))
            .collect::<Vec<_>>();
        let idle = self.pending_task_command.is_none()
            && self.pending_batch_command.is_none()
            && !matches!(self.active_modal, Some(ActiveModal::RemoveConfirmation(_)));
        let commands_available = self.snapshot.commands_available() && idle;
        let can_pause = selected.iter().any(|task| task.status.can_pause());
        let can_force_pause = can_pause && self.snapshot.capabilities.force_pause;
        let can_resume = selected.iter().any(|task| task.status.can_resume());
        let can_retry = selected.iter().any(|task| task.status.can_retry());
        let can_remove = selected.iter().any(|task| task.status.can_remove());
        let pending = self
            .pending_batch_command
            .as_ref()
            .map(|pending| pending.command);

        div()
            .flex()
            .items_center()
            .gap_1()
            .child(
                div()
                    .mr_2()
                    .font_features(tabular_numbers())
                    .text_xs()
                    .text_color(colors.text_secondary)
                    .child(self.t_count(
                        "selected-task-count",
                        u64::try_from(selected.len()).unwrap_or(u64::MAX),
                    )),
            )
            .child(
                toolbar_icon_button(
                    "batch-pause-action",
                    IconName::Pause,
                    self.t("action-pause-selected"),
                    ToolbarButtonState::from_flags(
                        commands_available && can_pause,
                        pending == Some(BatchTaskCommandView::Pause),
                    ),
                    false,
                    Some(&PauseSelectedTask),
                    colors,
                    &self.translator,
                )
                .when(commands_available && can_pause, |button| {
                    button.on_click(cx.listener(|this, _, _, cx| {
                        this.begin_batch_task_command(BatchTaskCommandView::Pause, cx);
                    }))
                }),
            )
            .child(
                toolbar_icon_button(
                    "batch-force-pause-action",
                    IconName::Square,
                    if self.snapshot.capabilities.force_pause {
                        self.t("action-force-pause-selected")
                    } else {
                        self.t("notice-force-pause-unsupported")
                    },
                    ToolbarButtonState::from_flags(
                        commands_available && can_force_pause,
                        pending == Some(BatchTaskCommandView::ForcePause),
                    ),
                    false,
                    None,
                    colors,
                    &self.translator,
                )
                .when(commands_available && can_force_pause, |button| {
                    button.on_click(cx.listener(|this, _, _, cx| {
                        this.begin_batch_task_command(BatchTaskCommandView::ForcePause, cx);
                    }))
                }),
            )
            .child(
                toolbar_icon_button(
                    "batch-resume-action",
                    IconName::Play,
                    self.t("action-resume-selected"),
                    ToolbarButtonState::from_flags(
                        commands_available && can_resume,
                        pending == Some(BatchTaskCommandView::Resume),
                    ),
                    false,
                    Some(&ResumeSelectedTask),
                    colors,
                    &self.translator,
                )
                .when(commands_available && can_resume, |button| {
                    button.on_click(cx.listener(|this, _, _, cx| {
                        this.begin_batch_task_command(BatchTaskCommandView::Resume, cx);
                    }))
                }),
            )
            .child(
                toolbar_icon_button(
                    "batch-retry-action",
                    IconName::RotateCcw,
                    self.t("action-retry-selected"),
                    ToolbarButtonState::from_flags(
                        commands_available && can_retry,
                        pending == Some(BatchTaskCommandView::Retry),
                    ),
                    false,
                    Some(&RetrySelectedTask),
                    colors,
                    &self.translator,
                )
                .when(commands_available && can_retry, |button| {
                    button.on_click(cx.listener(|this, _, _, cx| {
                        this.begin_batch_task_command(BatchTaskCommandView::Retry, cx);
                    }))
                }),
            )
            .child(
                toolbar_icon_button(
                    "batch-remove-action",
                    IconName::Trash2,
                    self.t("action-remove-selected"),
                    ToolbarButtonState::from_flags(
                        commands_available && can_remove,
                        matches!(
                            pending,
                            Some(
                                BatchTaskCommandView::RemoveTask
                                    | BatchTaskCommandView::ForceRemoveTask
                                    | BatchTaskCommandView::RemoveTaskAndFiles
                            )
                        ),
                    ),
                    true,
                    Some(&RemoveSelectedTask),
                    colors,
                    &self.translator,
                )
                .when(commands_available && can_remove, |button| {
                    button.on_click(cx.listener(|this, _, window, cx| {
                        this.confirm_remove_selected(window, cx);
                    }))
                }),
            )
            .child(
                toolbar_icon_button(
                    "clear-task-selection",
                    IconName::X,
                    self.t("action-clear-selection"),
                    ToolbarButtonState::from_flags(idle, false),
                    false,
                    Some(&ClearSearch),
                    colors,
                    &self.translator,
                )
                .when(idle, |button| {
                    button.on_click(cx.listener(|this, _, _, cx| {
                        this.clear_task_selection();
                        cx.notify();
                    }))
                }),
            )
    }

    pub(crate) fn render_hidden_selection_toolbar(
        &mut self,
        visible: usize,
        cx: &mut Context<Self>,
    ) -> Div {
        let colors = self.theme.colors;
        let hidden = self.selected_tasks.len().saturating_sub(visible);
        div()
            .flex()
            .items_center()
            .gap_2()
            .child(
                div()
                    .font_features(tabular_numbers())
                    .text_xs()
                    .text_color(colors.text_secondary)
                    .child(self.t_args(
                        "hidden-selection-counts",
                        &[
                            (
                                "visible",
                                FluentValue::from(i64::try_from(visible).unwrap_or(i64::MAX)),
                            ),
                            (
                                "hidden",
                                FluentValue::from(i64::try_from(hidden).unwrap_or(i64::MAX)),
                            ),
                        ],
                    )),
            )
            .child(
                toolbar_icon_button(
                    "clear-hidden-task-selection",
                    IconName::X,
                    self.t("action-clear-selection"),
                    ToolbarButtonState::Enabled,
                    false,
                    Some(&ClearSearch),
                    colors,
                    &self.translator,
                )
                .on_click(cx.listener(|this, _, _, cx| {
                    this.clear_task_selection();
                    cx.notify();
                })),
            )
    }

    pub(crate) fn render_task_row(
        &self,
        index: usize,
        task: &DownloadRowView,
        layout: TaskLayoutMode,
        cx: &Context<Self>,
    ) -> AnyElement {
        let colors = self.theme.colors;
        let focused = self.selected.as_ref() == Some(&task.identity);
        let selected = self.selected_tasks.contains(&task.identity);
        let task_count = self.snapshot.tasks.len();
        let mut display = self.build_task_row_display(task, layout, colors);
        let aria_label = std::mem::take(&mut display.aria_label);
        let task_key = task.identity.element_key();
        let checkbox_aria = if selected {
            self.t_args(
                "deselect-task-aria",
                &[("name", FluentValue::from(display.display_name.as_str()))],
            )
        } else {
            self.t_args(
                "select-task-aria",
                &[("name", FluentValue::from(display.display_name.as_str()))],
            )
        };
        let task_for_double_click = task.clone();
        let row = div()
            .id(("task-row", task_key))
            .role(Role::ListItem)
            .aria_label(aria_label)
            .aria_selected(selected)
            .aria_position_in_set(index + 1)
            .aria_size_of_set(task_count)
            .when(focused, |row| row.aria_active_descendant())
            .relative()
            .h(px(TASK_ROW_HEIGHT))
            .w_full()
            .flex_none()
            .flex()
            .items_center()
            .gap_3()
            .px_3()
            .border_1()
            .border_color(if focused {
                with_alpha(colors.accent, 0.72)
            } else {
                gpui::transparent_black()
            })
            .bg(if selected {
                with_alpha(colors.accent, 0.07)
            } else {
                colors.background
            })
            .hover(|style| style.bg(colors.surface_hover))
            .active(|style| style.bg(colors.surface_active))
            .when(!focused, |row| {
                row.child(
                    div()
                        .absolute()
                        .left_0()
                        .right_0()
                        .bottom_0()
                        .h(px(1.0))
                        .bg(colors.border),
                )
            })
            .on_click(cx.listener(move |this, event: &ClickEvent, window, cx| {
                if !event.standard_click() {
                    return;
                }
                let modifiers = event.modifiers();
                this.select_at_with_modifiers(
                    index,
                    modifiers.shift,
                    modifiers.secondary(cx),
                    window,
                    cx,
                );
                if event.click_count() == 2 && !modifiers.shift && !modifiers.secondary(cx) {
                    this.open_details_for(task_for_double_click.clone(), cx);
                }
            }))
            .child(
                div()
                    .flex_none()
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .child(
                        Checkbox::new(("task-select", task_key))
                            .checked(selected)
                            .tab_stop(false)
                            .accessibility_label(checkbox_aria)
                            .on_change(cx.listener(move |this, _, window, cx| {
                                this.select_at_with_modifiers(index, false, true, window, cx);
                            })),
                    ),
            )
            .child(
                div()
                    .size(px(32.0))
                    .flex_none()
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded_md()
                    .border_1()
                    .border_color(colors.border)
                    .bg(colors.elevated_surface)
                    .child(
                        Icon::new(file_type_icon(&display.display_name))
                            .size(IconSize::Small)
                            .color(display.status_color),
                    ),
            );

        let status_badge =
            task_status_badge(task.status, self.t(task.status.message_key()), colors);
        let row = match layout {
            TaskLayoutMode::Wide => {
                render_task_row_wide(row, task_key, task.status, display, status_badge, colors)
            }
            TaskLayoutMode::Compact => {
                render_task_row_compact(row, task_key, task.status, display, status_badge, colors)
            }
        };
        let shell = cx.entity().downgrade();
        let identity = task.identity.clone();
        row.context_menu(move |menu, window, cx| {
            let menu_entity = cx.entity();
            let dismiss_shell = shell.clone();
            window
                .subscribe(
                    &menu_entity,
                    cx,
                    move |_, _: &gpui::DismissEvent, window, cx| {
                        let shell = dismiss_shell.clone();
                        window.defer(cx, move |_, cx| {
                            let _ = shell.update(cx, |this, cx| this.close_task_context_menu(cx));
                        });
                    },
                )
                .detach();
            let target = shell
                .update(cx, |this, cx| {
                    this.prepare_task_context_menu(&identity, window, cx)
                })
                .ok()
                .flatten();
            let Some(target) = target else {
                return menu;
            };
            let Some(entity) = shell.upgrade() else {
                return menu;
            };
            entity
                .read(cx)
                .populate_task_context_menu(menu, &target, shell.clone())
        })
        .into_any_element()
    }

    fn build_task_row_display(
        &self,
        task: &DownloadRowView,
        layout: TaskLayoutMode,
        colors: crate::ThemeColors,
    ) -> TaskRowDisplay {
        let basis_points = task.progress_basis_points();
        let progress = f32::from(basis_points.unwrap_or(0)) / 10_000.0;
        let seeding = task.status == TaskStatusView::Seeding;
        let status_color = task_status_color(task.status, colors);
        let display_name = task_display_name(task, &self.translator);
        let size_label = if task.total_bytes == 0 {
            format_bytes(task.completed_bytes)
        } else {
            format!(
                "{} / {}",
                format_bytes(task.completed_bytes),
                format_bytes(task.total_bytes)
            )
        };
        let task_error_label = task.error.as_ref().map(|error| {
            error.code.map_or_else(
                || error.summary.clone(),
                |code| {
                    self.t_args(
                        "task-error-with-code",
                        &[
                            ("code", FluentValue::from(i64::from(code))),
                            ("summary", FluentValue::from(error.summary.as_str())),
                        ],
                    )
                },
            )
        });
        let (progress_label, rate_label, eta_label, uploaded_label, share_ratio) = if seeding {
            let share_ratio = format_share_ratio(task.share_ratio_milli());
            let observed_seeding = format_eta(task.observed_seeding_seconds);
            let upload_rate = format_rate(task.upload_rate);
            let uploaded = format_bytes(task.uploaded_bytes);
            let progress_label = self.t_args(
                "task-ratio-label",
                &[("ratio", FluentValue::from(share_ratio.as_str()))],
            );
            let rate_label = self.t_args(
                "task-upload-rate-label",
                &[("rate", FluentValue::from(upload_rate.as_str()))],
            );
            (
                progress_label,
                rate_label,
                observed_seeding,
                Some((uploaded, upload_rate)),
                Some(share_ratio),
            )
        } else {
            (
                format_percent(basis_points),
                format_rate(task.download_rate),
                format_eta(task.eta_seconds),
                None,
                None,
            )
        };

        let base_aria = if let (Some((uploaded, upload_rate)), Some(ratio)) =
            (uploaded_label.as_ref(), share_ratio.as_ref())
        {
            self.t_args(
                "task-seeding-aria",
                &[
                    ("name", FluentValue::from(display_name.as_str())),
                    ("status", FluentValue::from(self.t("task-status-seeding"))),
                    ("ratio", FluentValue::from(ratio.as_str())),
                    ("uploaded", FluentValue::from(uploaded.as_str())),
                    ("rate", FluentValue::from(upload_rate.as_str())),
                    ("time", FluentValue::from(eta_label.as_str())),
                ],
            )
        } else {
            self.t_args(
                "task-row-aria",
                &[
                    ("name", FluentValue::from(display_name.as_str())),
                    (
                        "status",
                        FluentValue::from(self.t(task.status.message_key())),
                    ),
                    ("progress", FluentValue::from(progress_label.as_str())),
                    ("rate", FluentValue::from(rate_label.as_str())),
                    ("eta", FluentValue::from(eta_label.as_str())),
                ],
            )
        };
        let aria_label = match task_error_label.as_deref() {
            Some(error) => self.t_args(
                "task-row-with-error-aria",
                &[
                    ("task", FluentValue::from(base_aria.as_str())),
                    ("error", FluentValue::from(error)),
                ],
            ),
            None => base_aria,
        };
        let secondary_color = if task_error_label.is_some() {
            colors.danger
        } else {
            colors.text_muted
        };
        let secondary_label = match (layout, task_error_label) {
            (_, Some(error)) => error,
            (TaskLayoutMode::Wide, None) => self.t_args(
                "task-gid-label",
                &[("gid", FluentValue::from(task.identity.gid.as_str()))],
            ),
            (TaskLayoutMode::Compact, None) => {
                if let Some((uploaded, upload_rate)) = uploaded_label.as_ref() {
                    self.t_args(
                        "task-seeding-secondary",
                        &[
                            ("uploaded", FluentValue::from(uploaded.as_str())),
                            ("rate", FluentValue::from(upload_rate.as_str())),
                            ("time", FluentValue::from(eta_label.as_str())),
                        ],
                    )
                } else {
                    self.t_args(
                        "task-download-secondary",
                        &[
                            ("size", FluentValue::from(size_label.as_str())),
                            ("rate", FluentValue::from(rate_label.as_str())),
                            ("eta", FluentValue::from(eta_label.as_str())),
                        ],
                    )
                }
            }
        };

        TaskRowDisplay {
            display_name,
            secondary_label,
            secondary_color,
            status_color,
            progress,
            progress_label,
            rate_label,
            size_label,
            eta_label,
            aria_label,
        }
    }
}

struct TaskRowDisplay {
    display_name: String,
    secondary_label: String,
    secondary_color: Hsla,
    status_color: Hsla,
    progress: f32,
    progress_label: String,
    rate_label: String,
    size_label: String,
    eta_label: String,
    aria_label: String,
}

fn render_task_row_wide(
    row: Stateful<Div>,
    task_key: u64,
    status: TaskStatusView,
    display: TaskRowDisplay,
    status_badge: impl IntoElement,
    colors: crate::ThemeColors,
) -> Stateful<Div> {
    row.child(
        div()
            .flex_1()
            .min_w_0()
            .flex()
            .flex_col()
            .gap_1()
            .child(
                div()
                    .truncate()
                    .text_sm()
                    .font_weight(FontWeight::MEDIUM)
                    .child(display.display_name),
            )
            .child(
                div()
                    .truncate()
                    .font_features(tabular_numbers())
                    .text_xs()
                    .text_color(display.secondary_color)
                    .child(display.secondary_label),
            ),
    )
    .child(
        div()
            .w(px(132.0))
            .flex_none()
            .flex()
            .flex_col()
            .gap_2()
            .font_features(tabular_numbers())
            .text_xs()
            .text_color(colors.text_secondary)
            .child(display.progress_label)
            .child(task_progress_bar_with_id(
                ("task-progress", task_key),
                display.progress,
                status,
                colors,
            )),
    )
    .child(task_table_value(88.0, display.rate_label, colors))
    .child(task_table_value(124.0, display.size_label, colors))
    .child(task_table_value(72.0, display.eta_label, colors))
    .child(
        div()
            .w(px(86.0))
            .flex_none()
            .flex()
            .justify_center()
            .child(status_badge),
    )
}

fn render_task_row_compact(
    row: Stateful<Div>,
    task_key: u64,
    status: TaskStatusView,
    display: TaskRowDisplay,
    status_badge: impl IntoElement,
    colors: crate::ThemeColors,
) -> Stateful<Div> {
    row.child(
        div()
            .flex_1()
            .min_w_0()
            .flex()
            .flex_col()
            .gap_1()
            .child(
                div()
                    .truncate()
                    .text_sm()
                    .font_weight(FontWeight::MEDIUM)
                    .child(display.display_name),
            )
            .child(
                div()
                    .truncate()
                    .font_features(tabular_numbers())
                    .text_xs()
                    .text_color(display.secondary_color)
                    .child(display.secondary_label),
            ),
    )
    .child(
        div()
            .w(px(112.0))
            .flex_none()
            .flex()
            .flex_col()
            .gap_2()
            .font_features(tabular_numbers())
            .text_xs()
            .text_color(colors.text_secondary)
            .child(display.progress_label)
            .child(task_progress_bar_with_id(
                ("task-progress", task_key),
                display.progress,
                status,
                colors,
            )),
    )
    .child(
        div()
            .w(px(78.0))
            .flex_none()
            .flex()
            .justify_center()
            .child(status_badge),
    )
}

fn sort_key_message_key(key: WorkspaceSortKey) -> &'static str {
    match key {
        WorkspaceSortKey::Queue => "sort-queue",
        WorkspaceSortKey::Name => "sort-name",
        WorkspaceSortKey::Status => "sort-status",
        WorkspaceSortKey::Progress => "sort-progress",
        WorkspaceSortKey::DownloadSpeed => "sort-download-speed",
        WorkspaceSortKey::Size => "sort-size",
    }
}

fn sort_direction_message_key(direction: WorkspaceSortDirection) -> &'static str {
    match direction {
        WorkspaceSortDirection::Ascending => "sort-ascending",
        WorkspaceSortDirection::Descending => "sort-descending",
    }
}
