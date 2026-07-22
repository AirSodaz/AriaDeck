//! Main task list pane for AppShell.

use super::*;

impl AppShell {
    pub(crate) fn render_task_header(
        &mut self,
        layout: TaskLayoutMode,
        cx: &mut Context<Self>,
    ) -> Div {
        let colors = self.theme.colors;
        let selected_count = self.visible_selected_task_count();
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
            .text_color(colors.text_muted)
            .child(
                div()
                    .id("select-all-tasks")
                    .role(Role::CheckBox)
                    .aria_label(match selection_state {
                        Toggled::True => "Clear selection",
                        Toggled::False | Toggled::Mixed => "Select all visible tasks",
                    })
                    .aria_toggled(selection_state)
                    .size(px(20.0))
                    .flex_none()
                    .flex()
                    .items_center()
                    .justify_center()
                    .cursor_pointer()
                    .hover(|style| style.bg(colors.surface_hover))
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
                .child(div().w(px(72.0)).flex_none().child("ETA / seed"))
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

    pub(crate) fn render_main(&mut self, layout: TaskLayoutMode, cx: &mut Context<Self>) -> Div {
        let colors = self.theme.colors;
        let task_count = self.snapshot.tasks.len();
        let selected_count = self.visible_selected_task_count();
        let hidden_selected_count = self.selected_tasks.len().saturating_sub(selected_count);
        let content =
            if task_count == 0 {
                self.render_empty_state(cx)
            } else {
                div()
                    .id("download-task-list")
                    .role(Role::List)
                    .aria_label(format!("Downloads, {task_count} visible tasks"))
                    .size_full()
                    .child(
                        uniform_list(
                            "download-tasks",
                            task_count,
                            cx.processor(move |this, range: Range<usize>, _window, cx| {
                                this.rendered_range = range.clone();
                                range
                                    .filter_map(|index| {
                                        this.snapshot.tasks.get(index).cloned().map(|task| {
                                            this.render_task_row(index, task, layout, cx)
                                        })
                                    })
                                    .collect::<Vec<_>>()
                            }),
                        )
                        .track_scroll(&self.list_scroll)
                        .h_full()
                        .w_full(),
                    )
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
                    .h(px(52.0))
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
                                    .child(self.query.filter.label()),
                            )
                            .child(
                                div()
                                    .font_features(tabular_numbers())
                                    .text_xs()
                                    .text_color(colors.text_muted)
                                    .child(format!("{task_count} visible")),
                            )
                            .when(selected_count > 0 || hidden_selected_count > 0, |element| {
                                element.child(
                                    div()
                                        .font_features(tabular_numbers())
                                        .text_xs()
                                        .text_color(colors.text_secondary)
                                        .child(if hidden_selected_count > 0 {
                                            format!(
                                                "{selected_count} selected, {hidden_selected_count} hidden"
                                            )
                                        } else {
                                            format!("{selected_count} selected")
                                        }),
                                )
                            })
                            .child(self.render_list_controls(cx)),
                    )
                    .child(self.render_task_toolbar(cx)),
            )
            .child(self.render_task_header(layout, cx))
            .child(div().flex_1().min_h_0().child(content));

        div()
            .flex_1()
            .min_w_0()
            .min_h_0()
            .flex()
            .child(center)
            .when(self.details_drawer.is_some(), |element| {
                element.child(self.render_task_details_drawer(cx))
            })
    }

    pub(crate) fn render_task_context_menu(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let colors = self.theme.colors;
        let Some(menu) = self.context_menu.as_ref() else {
            return div().into_any_element();
        };
        let identity = menu.identity.clone();
        let position = menu.position;
        let Some(task) = self
            .snapshot
            .tasks
            .iter()
            .find(|task| task.identity == identity)
            .cloned()
        else {
            return div().into_any_element();
        };
        let idle = self.pending_task_command.is_none()
            && self.pending_batch_command.is_none()
            && self.remove_confirmation.is_none()
            && self.output_name_dialog.is_none()
            && self.task_speed_limit_dialog.is_none()
            && self.task_options_dialog.is_none();
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

        let mut entries: Vec<(
            ContextMenuAction,
            &'static str,
            Option<&'static str>,
            bool,
            bool,
        )> = vec![
            (
                ContextMenuAction::Details,
                "Details",
                Some("Enter"),
                true,
                false,
            ),
            (
                ContextMenuAction::OpenDownload,
                "Open download",
                None,
                path_actions,
                false,
            ),
            (
                ContextMenuAction::OpenFolder,
                "Open folder",
                None,
                path_actions,
                false,
            ),
            (
                ContextMenuAction::CopySource,
                "Copy source",
                None,
                has_source,
                false,
            ),
            (ContextMenuAction::CopyGid, "Copy GID", None, true, false),
        ];
        if task.status.can_pause() {
            entries.push((
                ContextMenuAction::Pause,
                "Pause",
                Some("Cmd+Shift+P"),
                can_pause,
                false,
            ));
            entries.push((
                ContextMenuAction::ForcePause,
                "Force pause",
                None,
                can_force_pause,
                false,
            ));
        }
        if task.status.can_resume() {
            entries.push((
                ContextMenuAction::Resume,
                "Resume",
                Some("Cmd+Shift+R"),
                can_resume,
                false,
            ));
        }
        if task.status.can_retry() {
            entries.push((
                ContextMenuAction::Retry,
                "Retry",
                Some("Cmd+Alt+R"),
                can_retry,
                false,
            ));
        }
        if task.status.can_move_in_queue() {
            entries.push((
                ContextMenuAction::MoveTop,
                "Move to top",
                Some("Cmd+Shift+Home"),
                can_queue,
                false,
            ));
            entries.push((
                ContextMenuAction::MoveUp,
                "Move up",
                Some("Cmd+Shift+Up"),
                can_queue,
                false,
            ));
            entries.push((
                ContextMenuAction::MoveDown,
                "Move down",
                Some("Cmd+Shift+Down"),
                can_queue,
                false,
            ));
            entries.push((
                ContextMenuAction::MoveBottom,
                "Move to bottom",
                Some("Cmd+Shift+End"),
                can_queue,
                false,
            ));
        }
        if task.can_set_output_name() {
            entries.push((
                ContextMenuAction::OutputName,
                "Change output name",
                Some("F2"),
                can_output,
                false,
            ));
        }
        if task.status.can_set_speed_limit() {
            entries.push((
                ContextMenuAction::SpeedLimit,
                "Set speed limits",
                None,
                can_speed,
                false,
            ));
            entries.push((
                ContextMenuAction::TaskOptions,
                "Edit task options",
                None,
                can_speed,
                false,
            ));
        }
        entries.push((
            ContextMenuAction::Remove,
            "Remove",
            Some("Delete"),
            can_remove,
            true,
        ));

        let left = f32::from(position.x).max(8.0);
        let top = f32::from(position.y).max(8.0);
        let display_name = task_display_name(&task);

        div()
            .id("task-context-menu-layer")
            .absolute()
            .inset_0()
            .occlude()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| this.close_task_context_menu(cx)),
            )
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(|this, _, _, cx| this.close_task_context_menu(cx)),
            )
            .child(
                div()
                    .id("task-context-menu")
                    .absolute()
                    .left(px(left))
                    .top(px(top))
                    .w(px(260.0))
                    .py_1()
                    .px_1()
                    .rounded_md()
                    .border_1()
                    .border_color(colors.border)
                    .bg(colors.elevated_surface)
                    .shadow_md()
                    .role(Role::Menu)
                    .aria_label(format!("Actions for {display_name}"))
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .on_mouse_down(MouseButton::Right, |_, _, cx| cx.stop_propagation())
                    .flex()
                    .flex_col()
                    .children(entries.into_iter().map(
                        |(action, label, shortcut, enabled, destructive)| {
                            context_menu_item(
                                action,
                                label,
                                shortcut,
                                enabled,
                                destructive,
                                colors,
                                cx,
                            )
                        },
                    )),
            )
            .into_any_element()
    }

    pub(crate) fn activate_context_menu_action(
        &mut self,
        action: ContextMenuAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Capture the right-clicked task before closing so multi-select still
        // has a single authoritative target for details/copy/open.
        let menu_task = self.context_menu_task_view();
        self.close_task_context_menu(cx);
        match action {
            ContextMenuAction::Details => {
                if let Some(task) = menu_task.or_else(|| self.selected_task_view()) {
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
                if let Some(task) = menu_task.or_else(|| self.selected_task_view()) {
                    self.copy_task_source(&task, cx);
                }
            }
            ContextMenuAction::CopyGid => {
                if let Some(task) = menu_task.or_else(|| self.selected_task_view()) {
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
        let colors = self.theme.colors;
        let idle = self.pending_task_command.is_none()
            && self.pending_global_task_command.is_none()
            && self.pending_batch_command.is_none()
            && self.remove_confirmation.is_none();
        let commands_available = self.snapshot.commands_available() && idle;
        let pending_global = self
            .pending_global_task_command
            .as_ref()
            .map(|pending| pending.command);
        let sort_label = self.query.sort_key.label();

        div()
            .ml_2()
            .flex()
            .items_center()
            .gap_1()
            .child(
                IconButton::new("pause-all-action", IconName::Pause)
                    .aria_label("Pause all tasks")
                    .style(ButtonStyle::Ghost)
                    .disabled(!commands_available)
                    .loading(pending_global == Some(GlobalTaskCommandView::PauseAll))
                    .tooltip(Tooltip::new("Pause all"))
                    .render(colors)
                    .when(commands_available, |button| {
                        button.on_click(cx.listener(|this, _, _, cx| {
                            this.begin_global_task_command(GlobalTaskCommandView::PauseAll, cx);
                        }))
                    }),
            )
            .child(
                IconButton::new("force-pause-all-action", IconName::Square)
                    .aria_label("Force pause all tasks")
                    .style(ButtonStyle::Ghost)
                    .disabled(!commands_available || !self.snapshot.capabilities.force_pause_all)
                    .loading(pending_global == Some(GlobalTaskCommandView::ForcePauseAll))
                    .tooltip(Tooltip::new(
                        if self.snapshot.capabilities.force_pause_all {
                            "Force pause all"
                        } else {
                            self.snapshot
                                .capabilities
                                .unsupported_force_pause_all_message()
                        },
                    ))
                    .render(colors)
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
                IconButton::new("resume-all-action", IconName::Play)
                    .aria_label("Resume all tasks")
                    .style(ButtonStyle::Ghost)
                    .disabled(!commands_available)
                    .loading(pending_global == Some(GlobalTaskCommandView::ResumeAll))
                    .tooltip(Tooltip::new("Resume all"))
                    .render(colors)
                    .when(commands_available, |button| {
                        button.on_click(cx.listener(|this, _, _, cx| {
                            this.begin_global_task_command(GlobalTaskCommandView::ResumeAll, cx);
                        }))
                    }),
            )
            .child(
                div()
                    .id("sort-menu-trigger")
                    .focusable()
                    .tab_stop(true)
                    .role(Role::Button)
                    .aria_label(format!("Sort by {sort_label}"))
                    .aria_expanded(self.sort_popover_open)
                    .h(px(28.0))
                    .px_2()
                    .flex()
                    .items_center()
                    .gap_1()
                    .rounded_md()
                    .text_xs()
                    .text_color(colors.text_secondary)
                    .cursor_pointer()
                    .hover(|style| style.bg(colors.surface_hover))
                    .focus_visible(|style| style.border_1().border_color(colors.focus_ring))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.toggle_sort_popover(cx);
                    }))
                    .child(
                        Icon::new(IconName::ArrowUpDown)
                            .size(IconSize::Small)
                            .color(colors.text_muted),
                    )
                    .child(sort_label),
            )
    }

    pub(crate) fn render_sort_popover(&mut self, cx: &mut Context<Self>) -> Stateful<Div> {
        let colors = self.theme.colors;
        let current_key = self.query.sort_key;
        let current_direction = self.query.sort_direction;

        let mut menu = div()
            .id("sort-menu")
            .absolute()
            .right(px(12.0))
            .top(px(96.0))
            .w(px(220.0))
            .on_click(|_, _, cx| cx.stop_propagation())
            .bg(colors.elevated_surface)
            .border_1()
            .border_color(colors.border)
            .rounded_lg()
            .p_1()
            .flex()
            .flex_col()
            .gap_px()
            .child(
                div()
                    .px_2()
                    .py_1()
                    .text_xs()
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(colors.text_muted)
                    .child(self.t("ui-sort-by")),
            );

        for key in WorkspaceSortKey::ALL {
            let selected = key == current_key;
            menu = menu.child(
                div()
                    .id(SharedString::from(format!("sort-key-{}", key.key())))
                    .role(Role::Button)
                    .aria_label(format!("Sort by {}", key.label()))
                    .h(px(32.0))
                    .px_2()
                    .flex()
                    .items_center()
                    .gap_2()
                    .rounded_md()
                    .text_xs()
                    .text_color(if selected {
                        colors.accent
                    } else {
                        colors.text_secondary
                    })
                    .cursor_pointer()
                    .hover(|style| style.bg(colors.surface_hover))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.set_sort_key(key, cx);
                    }))
                    .child(div().w(px(16.0)).flex_none().when(selected, |element| {
                        element.child(
                            Icon::new(IconName::Check)
                                .size(IconSize::Small)
                                .color(colors.accent),
                        )
                    }))
                    .child(div().flex_1().child(key.label())),
            );
        }

        menu = menu.child(
            div()
                .mt_1()
                .pt_1()
                .border_t_1()
                .border_color(colors.border)
                .flex()
                .flex_col()
                .gap_px(),
        );
        for direction in [
            WorkspaceSortDirection::Ascending,
            WorkspaceSortDirection::Descending,
        ] {
            let selected = direction == current_direction;
            let icon = match direction {
                WorkspaceSortDirection::Ascending => IconName::ArrowUp,
                WorkspaceSortDirection::Descending => IconName::ArrowDown,
            };
            menu = menu.child(
                div()
                    .id(SharedString::from(match direction {
                        WorkspaceSortDirection::Ascending => "sort-direction-ascending",
                        WorkspaceSortDirection::Descending => "sort-direction-descending",
                    }))
                    .role(Role::Button)
                    .aria_label(format!("{} order", direction.label()))
                    .aria_toggled(if selected {
                        Toggled::True
                    } else {
                        Toggled::False
                    })
                    .h(px(32.0))
                    .px_2()
                    .flex()
                    .items_center()
                    .gap_2()
                    .rounded_md()
                    .text_xs()
                    .text_color(if selected {
                        colors.accent
                    } else {
                        colors.text_secondary
                    })
                    .cursor_pointer()
                    .hover(|style| style.bg(colors.surface_hover))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.set_sort_direction(direction, cx);
                    }))
                    .child(div().w(px(16.0)).flex_none().child(
                        Icon::new(icon).size(IconSize::Small).color(if selected {
                            colors.accent
                        } else {
                            colors.text_muted
                        }),
                    ))
                    .child(div().flex_1().child(direction.label())),
            );
        }

        div()
            .id("sort-popover-layer")
            .absolute()
            .inset_0()
            .occlude()
            .on_click(cx.listener(|this, _, _, cx| {
                this.close_sort_popover(cx);
            }))
            .child(menu)
    }

    pub(crate) fn render_task_toolbar(&mut self, cx: &mut Context<Self>) -> Div {
        let visible_selected_count = self.visible_selected_task_count();
        if visible_selected_count > 1 {
            return self.render_batch_task_toolbar(cx);
        }
        if !self.selected_tasks.is_empty() && self.selected_tasks.len() > visible_selected_count {
            return self.render_hidden_selection_toolbar(cx);
        }
        let colors = self.theme.colors;
        let Some(task) = self.selected_task_view() else {
            return div();
        };
        let idle = self.pending_task_command.is_none()
            && self.pending_batch_command.is_none()
            && self.remove_confirmation.is_none()
            && self.output_name_dialog.is_none();
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
                    "Details",
                    ToolbarButtonState::from_flags(details_enabled, false),
                    false,
                    Some("Enter"),
                    colors,
                )
                .when(details_enabled, |button| {
                    button.on_click(cx.listener(|this, _, _window, cx| {
                        if let Some(task) = this.selected_task_view() {
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
                            "Pause",
                            ToolbarButtonState::from_flags(
                                pause_enabled,
                                pending_command == Some(TaskCommandView::Pause),
                            ),
                            false,
                            Some("Cmd+Shift+P"),
                            colors,
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
                                "Force pause"
                            } else {
                                self.snapshot.capabilities.unsupported_force_pause_message()
                            },
                            ToolbarButtonState::from_flags(
                                force_pause_enabled,
                                pending_command == Some(TaskCommandView::ForcePause),
                            ),
                            false,
                            None,
                            colors,
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
                        "Resume",
                        ToolbarButtonState::from_flags(
                            resume_enabled,
                            pending_command == Some(TaskCommandView::Resume),
                        ),
                        false,
                        Some("Cmd+Shift+R"),
                        colors,
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
                            "Move to top",
                            TaskCommandView::MoveToQueueTop,
                            queue_enabled,
                            pending_command.as_ref(),
                            Some("Cmd+Shift+Home"),
                            colors,
                            cx,
                        ),
                        queue_move_button(
                            "queue-move-up-action",
                            IconName::ChevronUp,
                            "Move up",
                            TaskCommandView::MoveUpInQueue,
                            queue_enabled,
                            pending_command.as_ref(),
                            Some("Cmd+Shift+Up"),
                            colors,
                            cx,
                        ),
                        queue_move_button(
                            "queue-move-down-action",
                            IconName::ChevronDown,
                            "Move down",
                            TaskCommandView::MoveDownInQueue,
                            queue_enabled,
                            pending_command.as_ref(),
                            Some("Cmd+Shift+Down"),
                            colors,
                            cx,
                        ),
                        queue_move_button(
                            "queue-move-bottom-action",
                            IconName::ChevronsDown,
                            "Move to bottom",
                            TaskCommandView::MoveToQueueBottom,
                            queue_enabled,
                            pending_command.as_ref(),
                            Some("Cmd+Shift+End"),
                            colors,
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
                        "Retry",
                        ToolbarButtonState::from_flags(
                            retry_enabled,
                            pending_command == Some(TaskCommandView::Retry),
                        ),
                        false,
                        Some("Cmd+Alt+R"),
                        colors,
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
                        "Change output name",
                        ToolbarButtonState::from_flags(commands_available, false),
                        false,
                        Some("F2"),
                        colors,
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
                            "Set speed limits",
                            ToolbarButtonState::from_flags(commands_available, false),
                            false,
                            None,
                            colors,
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
                            "Edit task options",
                            ToolbarButtonState::from_flags(commands_available, false),
                            false,
                            None,
                            colors,
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
                    "Remove",
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
                    Some("Delete"),
                    colors,
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
            && self.remove_confirmation.is_none();
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
                    .child(format!("{} selected", selected.len())),
            )
            .child(
                toolbar_icon_button(
                    "batch-pause-action",
                    IconName::Pause,
                    "Pause selected",
                    ToolbarButtonState::from_flags(
                        commands_available && can_pause,
                        pending == Some(BatchTaskCommandView::Pause),
                    ),
                    false,
                    Some("Cmd+Shift+P"),
                    colors,
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
                        "Force pause selected"
                    } else {
                        self.snapshot.capabilities.unsupported_force_pause_message()
                    },
                    ToolbarButtonState::from_flags(
                        commands_available && can_force_pause,
                        pending == Some(BatchTaskCommandView::ForcePause),
                    ),
                    false,
                    None,
                    colors,
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
                    "Resume selected",
                    ToolbarButtonState::from_flags(
                        commands_available && can_resume,
                        pending == Some(BatchTaskCommandView::Resume),
                    ),
                    false,
                    Some("Cmd+Shift+R"),
                    colors,
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
                    "Retry selected",
                    ToolbarButtonState::from_flags(
                        commands_available && can_retry,
                        pending == Some(BatchTaskCommandView::Retry),
                    ),
                    false,
                    Some("Cmd+Alt+R"),
                    colors,
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
                    "Remove selected",
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
                    Some("Delete"),
                    colors,
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
                    "Clear selection",
                    ToolbarButtonState::from_flags(idle, false),
                    false,
                    Some("Escape"),
                    colors,
                )
                .when(idle, |button| {
                    button.on_click(cx.listener(|this, _, _, cx| {
                        this.clear_task_selection();
                        cx.notify();
                    }))
                }),
            )
    }

    pub(crate) fn render_hidden_selection_toolbar(&mut self, cx: &mut Context<Self>) -> Div {
        let colors = self.theme.colors;
        let visible = self.visible_selected_task_count();
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
                    .child(format!("{visible} visible, {hidden} hidden selected")),
            )
            .child(
                toolbar_icon_button(
                    "clear-hidden-task-selection",
                    IconName::X,
                    "Clear selection",
                    ToolbarButtonState::Enabled,
                    false,
                    Some("Escape"),
                    colors,
                )
                .on_click(cx.listener(|this, _, _, cx| {
                    this.clear_task_selection();
                    cx.notify();
                })),
            )
    }

    pub(crate) fn render_task_row(
        &mut self,
        index: usize,
        task: DownloadRowView,
        layout: TaskLayoutMode,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let colors = self.theme.colors;
        let focused = self.selected.as_ref() == Some(&task.identity);
        let selected = self.selected_tasks.contains(&task.identity);
        let stable_id = SharedString::from(format!(
            "task-row:{}:{}",
            task.identity.profile_id, task.identity.gid
        ));
        let task_count = self.snapshot.tasks.len();
        let basis_points = task.progress_basis_points();
        let progress = f32::from(basis_points.unwrap_or(0)) / 10_000.0;
        let seeding = task.status == TaskStatusView::Seeding;
        let share_ratio = format_share_ratio(task.share_ratio_milli());
        let observed_seeding = format_eta(task.observed_seeding_seconds);
        let status_color = task_status_color(task.status, colors);
        let display_name = task_display_name(&task);
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
                |code| format!("Error {code}: {}", error.summary),
            )
        });
        let mut aria_label = if seeding {
            format!(
                "{}, Seeding, share ratio {}, uploaded {}, upload speed {}, observed seeding time {} in this session",
                display_name.as_str(),
                share_ratio,
                format_bytes(task.uploaded_bytes),
                format_rate(task.upload_rate),
                observed_seeding
            )
        } else {
            format!(
                "{}, {}, {}, download speed {}, ETA {}",
                display_name.as_str(),
                task.status.label(),
                format_percent(basis_points),
                format_rate(task.download_rate),
                format_eta(task.eta_seconds)
            )
        };
        if let Some(error) = &task_error_label {
            aria_label.push_str(", ");
            aria_label.push_str(error);
        }
        let wide_secondary_label = task_error_label
            .clone()
            .unwrap_or_else(|| format!("GID {}", task.identity.gid));
        let compact_secondary_label = task_error_label.clone().unwrap_or_else(|| {
            if seeding {
                format!(
                    "Uploaded {} · Up {} · {}",
                    format_bytes(task.uploaded_bytes),
                    format_rate(task.upload_rate),
                    observed_seeding
                )
            } else {
                format!(
                    "{size_label} · {} · {}",
                    format_rate(task.download_rate),
                    format_eta(task.eta_seconds)
                )
            }
        });
        let secondary_color = if task_error_label.is_some() {
            colors.danger
        } else {
            colors.text_muted
        };
        let progress_label = if seeding {
            format!("Ratio {share_ratio}")
        } else {
            format_percent(basis_points)
        };
        let rate_label = if seeding {
            format!("Up {}", format_rate(task.upload_rate))
        } else {
            format_rate(task.download_rate)
        };
        let eta_label = if seeding {
            observed_seeding
        } else {
            format_eta(task.eta_seconds)
        };
        let status_badge =
            task_status_badge(task.status, self.t(task.status.message_key()), colors);
        let row = div()
            .id(stable_id)
            .role(Role::ListItem)
            .aria_label(aria_label)
            .aria_selected(selected)
            .aria_position_in_set(index + 1)
            .aria_size_of_set(task_count)
            .when(focused, |row| row.aria_active_descendant())
            .h(px(TASK_ROW_HEIGHT))
            .w_full()
            .flex_none()
            .flex()
            .items_center()
            .gap_3()
            .px_3()
            .border_b_1()
            .border_color(colors.border)
            .bg(if selected {
                with_alpha(colors.accent, 0.07)
            } else {
                colors.background
            })
            .when(focused, |row| {
                row.border_1().border_color(with_alpha(colors.accent, 0.72))
            })
            .hover(|style| style.bg(colors.surface_hover))
            .cursor_pointer()
            .on_click(cx.listener(move |this, event: &ClickEvent, window, cx| {
                if event.is_right_click() {
                    this.open_task_context_menu(index, event.position(), window, cx);
                    return;
                }
                if !event.standard_click() {
                    return;
                }
                let modifiers = event.modifiers();
                this.select_at_with_modifiers(
                    index,
                    modifiers.shift,
                    modifiers.secondary(),
                    window,
                    cx,
                );
            }))
            .child(
                div()
                    .id(SharedString::from(format!(
                        "task-select:{}:{}",
                        task.identity.profile_id, task.identity.gid
                    )))
                    .role(Role::CheckBox)
                    .aria_label(if selected {
                        format!("Deselect {display_name}")
                    } else {
                        format!("Select {display_name}")
                    })
                    .aria_toggled(if selected {
                        Toggled::True
                    } else {
                        Toggled::False
                    })
                    .size(px(20.0))
                    .flex_none()
                    .flex()
                    .items_center()
                    .justify_center()
                    .cursor_pointer()
                    .hover(|style| style.bg(colors.surface_hover))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        cx.stop_propagation();
                        this.select_at_with_modifiers(index, false, true, window, cx);
                    }))
                    .child(
                        Icon::new(if selected {
                            IconName::SquareCheckBig
                        } else {
                            IconName::Square
                        })
                        .size(IconSize::Small)
                        .color(if selected {
                            colors.accent
                        } else {
                            colors.text_muted
                        }),
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
                        Icon::new(task_status_icon(task.status))
                            .size(IconSize::Small)
                            .color(status_color),
                    ),
            );

        match layout {
            TaskLayoutMode::Wide => row
                .child(
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
                                .child(display_name),
                        )
                        .child(
                            div()
                                .truncate()
                                .font_features(tabular_numbers())
                                .text_xs()
                                .text_color(secondary_color)
                                .child(wide_secondary_label),
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
                        .child(progress_label)
                        .child(task_progress_bar(progress, task.status, colors)),
                )
                .child(task_table_value(88.0, rate_label, colors))
                .child(task_table_value(124.0, size_label, colors))
                .child(task_table_value(72.0, eta_label, colors))
                .child(
                    div()
                        .w(px(86.0))
                        .flex_none()
                        .flex()
                        .justify_center()
                        .child(status_badge),
                ),
            TaskLayoutMode::Compact => row
                .child(
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
                                .child(display_name),
                        )
                        .child(
                            div()
                                .truncate()
                                .font_features(tabular_numbers())
                                .text_xs()
                                .text_color(secondary_color)
                                .child(compact_secondary_label),
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
                        .child(progress_label)
                        .child(task_progress_bar(progress, task.status, colors)),
                )
                .child(
                    div()
                        .w(px(78.0))
                        .flex_none()
                        .flex()
                        .justify_center()
                        .child(status_badge),
                ),
        }
    }
}
