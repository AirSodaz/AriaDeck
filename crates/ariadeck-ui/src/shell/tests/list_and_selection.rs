use gpui::{TestAppContext, px};

use super::*;

#[gpui::test]
fn task_selection_supports_toggle_range_and_visible_select_all(cx: &mut TestAppContext) {
    let (view, cx) = cx.add_window_view(|window, cx| {
        let mut shell = AppShell::new(Theme::dark(), window, cx);
        shell.snapshot = snapshot(5);
        shell
    });

    view.update_in(cx, |shell, window, cx| {
        shell.select_at_with_modifiers(1, false, false, window, cx);
        shell.select_at_with_modifiers(3, true, false, window, cx);
    });
    view.read_with(cx, |shell, _| {
        let selected = [1, 2, 3]
            .into_iter()
            .map(|index| task(index).identity)
            .collect::<HashSet<_>>();
        assert_eq!(shell.selected_tasks, selected);
        assert_eq!(shell.range_anchor, Some(task(1).identity));
        assert_eq!(shell.selected, Some(task(3).identity));
    });

    view.update_in(cx, |shell, window, cx| {
        shell.select_at_with_modifiers(2, false, true, window, cx);
    });
    view.read_with(cx, |shell, _| {
        assert!(!shell.selected_tasks.contains(&task(2).identity));
        assert_eq!(shell.selected_tasks.len(), 2);
    });

    view.update_in(cx, |shell, window, cx| {
        shell.toggle_select_all(window, cx);
    });
    view.read_with(cx, |shell, _| {
        assert_eq!(shell.selected_tasks.len(), 5);
    });
    view.update_in(cx, |shell, window, cx| {
        shell.toggle_select_all(window, cx);
    });
    view.read_with(cx, |shell, _| {
        assert!(shell.selected_tasks.is_empty());
        assert!(shell.range_anchor.is_none());
    });
}

#[gpui::test]
fn select_all_shortcut_selects_the_current_loaded_query(cx: &mut TestAppContext) {
    cx.update(crate::init);
    let (view, cx) = cx.add_window_view(|window, cx| {
        let mut shell = AppShell::new(Theme::dark(), window, cx);
        shell.snapshot = snapshot(4);
        window.focus(&shell.focus_handle, cx);
        shell
    });

    cx.simulate_keystrokes(&format!("{}-a", secondary_modifier()));
    view.read_with(cx, |shell, _| {
        assert_eq!(shell.visible_selected_task_count(), 4);
        assert_eq!(shell.selected_task_count(), 4);
        assert_eq!(shell.selected, Some(task(0).identity));
    });
}

#[gpui::test]
fn context_menu_opens_from_right_click_and_preserves_multi_selection(cx: &mut TestAppContext) {
    let (view, cx) = cx.add_window_view(|window, cx| {
        let mut shell = AppShell::new(Theme::dark(), window, cx);
        shell.snapshot = snapshot(3);
        shell.selected = Some(task(0).identity);
        shell.selected_tasks = HashSet::from([task(0).identity, task(1).identity]);
        shell
    });

    view.update_in(cx, |shell, window, cx| {
        shell.open_task_context_menu(
            1,
            Point {
                x: px(120.0),
                y: px(80.0),
            },
            window,
            cx,
        );
        assert!(shell.context_menu_target.is_some());
        assert_eq!(shell.selected.as_ref(), Some(&task(1).identity));
        assert_eq!(shell.selected_tasks.len(), 2);
        assert!(shell.selected_tasks.contains(&task(0).identity));
        assert!(shell.selected_tasks.contains(&task(1).identity));

        // Multi-select still targets the right-clicked row for copy.
        let target = shell.context_menu_task_view(cx).expect("menu target");
        shell.copy_task_gid(&target, cx);
        assert_eq!(
            cx.read_from_clipboard().and_then(|item| item.text()),
            Some(task(1).identity.gid.clone()),
        );

        shell.close_task_context_menu(cx);
        assert!(shell.context_menu_target.is_none());
        // Closing the menu does not drop the multi-selection.
        assert_eq!(shell.selected_tasks.len(), 2);
    });
}

#[gpui::test]
fn copy_task_source_emits_clipboard_source_copied_event(cx: &mut TestAppContext) {
    let (view, cx) = cx.add_window_view(|window, cx| {
        let mut shell = AppShell::new(Theme::dark(), window, cx);
        let mut row = task(0);
        row.primary_source = Some("https://cdn.example.test/archive.zip".into());
        shell.snapshot.tasks = vec![row];
        shell
    });
    let copied = Arc::new(std::sync::Mutex::new(Vec::new()));
    let sink = copied.clone();
    let _subscription = view.update(cx, |_, cx| {
        cx.subscribe(&view, move |_, _, event: &AppShellEvent, _| {
            if let AppShellEvent::ClipboardSourceCopied(source) = event {
                sink.lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .push(source.clone());
            }
        })
    });

    view.update(cx, |shell, cx| {
        let row = shell.snapshot.tasks[0].clone();
        shell.copy_task_source(&row, cx);
    });

    assert_eq!(
        cx.read_from_clipboard().and_then(|item| item.text()),
        Some("https://cdn.example.test/archive.zip".to_owned()),
    );
    let captured = copied
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone();
    assert_eq!(
        captured,
        vec!["https://cdn.example.test/archive.zip".to_owned()]
    );
}

#[gpui::test]
fn visible_selection_counts_and_header_toggle_exclude_hidden_tasks(cx: &mut TestAppContext) {
    let hidden = task(99).identity;
    let (view, cx) = cx.add_window_view(|window, cx| {
        let mut shell = AppShell::new(Theme::dark(), window, cx);
        shell.snapshot = snapshot(2);
        shell.selected = Some(task(0).identity);
        shell.selected_tasks = HashSet::from([task(0).identity, hidden.clone()]);
        shell
    });

    view.read_with(cx, |shell, _| {
        assert_eq!(shell.selected_task_count(), 2);
        assert_eq!(shell.visible_selected_task_count(), 1);
    });
    view.update_in(cx, |shell, window, cx| {
        shell.toggle_select_all(window, cx);
    });
    view.read_with(cx, |shell, _| {
        assert_eq!(shell.visible_selected_task_count(), 2);
        assert_eq!(shell.selected_task_count(), 3);
    });
    view.update_in(cx, |shell, window, cx| {
        shell.toggle_select_all(window, cx);
    });
    view.read_with(cx, |shell, _| {
        assert_eq!(shell.visible_selected_task_count(), 0);
        assert_eq!(shell.selected_tasks, HashSet::from([hidden]));
    });
}

#[gpui::test]
fn query_change_clears_the_query_scoped_task_selection(cx: &mut TestAppContext) {
    let (view, cx) = cx.add_window_view(|window, cx| {
        let mut shell = AppShell::new(Theme::dark(), window, cx);
        shell.snapshot = snapshot(3);
        shell.select_at_with_modifiers(0, false, false, window, cx);
        shell.select_at_with_modifiers(1, false, true, window, cx);
        shell
    });
    view.update_in(cx, |shell, window, cx| {
        shell.set_filter(WorkspaceFilter::Completed, window, cx);
    });
    view.read_with(cx, |shell, _| {
        assert!(shell.selected_tasks.is_empty());
        assert!(shell.range_anchor.is_none());
    });
}

#[gpui::test]
fn hidden_selection_arrows_start_at_the_visible_edges(cx: &mut TestAppContext) {
    let (view, cx) = cx.add_window_view(|window, cx| {
        let mut shell = AppShell::new(Theme::dark(), window, cx);
        shell.snapshot = snapshot(3);
        shell.selected = Some(task(99).identity);
        shell
    });

    view.update_in(cx, |shell, window, cx| {
        shell.select_next(&SelectNextTask, window, cx);
    });
    view.read_with(cx, |shell, _| {
        assert_eq!(shell.selected, Some(task(0).identity));
    });

    view.update(cx, |shell, _| {
        shell.selected = Some(task(99).identity);
    });
    view.update_in(cx, |shell, window, cx| {
        shell.select_previous(&SelectPreviousTask, window, cx);
    });
    view.read_with(cx, |shell, _| {
        assert_eq!(shell.selected, Some(task(2).identity));
    });
}

#[gpui::test]
fn magnet_successor_relationship_preserves_selected_task(cx: &mut TestAppContext) {
    let (view, cx) = cx.add_window_view(|window, cx| {
        let mut shell = AppShell::new(Theme::dark(), window, cx);
        let mut previous = snapshot(1);
        previous.tasks[0].followed_by = vec![format!("{:016x}", 1)];
        shell.snapshot = previous;
        shell.selected = Some(shell.snapshot.tasks[0].identity.clone());
        shell
    });

    view.update_in(cx, |shell, _window, cx| {
        let mut next = snapshot(1);
        next.tasks[0] = task(1);
        shell.set_snapshot(next, cx);
    });
    view.read_with(cx, |shell, _| {
        assert_eq!(
            shell.selected.as_ref().map(|task| task.gid.as_str()),
            Some("0000000000000001")
        );
    });
}

#[gpui::test]
fn load_more_stopped_history_is_single_flight_and_gated(cx: &mut TestAppContext) {
    let (view, cx) = cx.add_window_view(|window, cx| {
        let mut shell = AppShell::new(Theme::dark(), window, cx);
        shell.snapshot = snapshot(2);
        shell.snapshot.stopped_history = crate::StoppedHistoryView {
            loaded: 2,
            total: Some(5),
            can_load_more: true,
            local_saved: 0,
        };
        shell
    });
    let events = Arc::new(std::sync::Mutex::new(0usize));
    let sink = events.clone();
    let _subscription = view.update(cx, |_, cx| {
        cx.subscribe(&view, move |_, _, event: &AppShellEvent, _| {
            if matches!(event, AppShellEvent::LoadMoreStoppedRequested) {
                *sink.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) += 1;
            }
        })
    });

    // Stale or disconnected snapshots must not request another page.
    view.update(cx, |shell, cx| {
        shell.snapshot.stale = true;
        shell.request_load_more_stopped(cx);
        assert!(!shell.pending_load_more_stopped);
        shell.snapshot.stale = false;
        shell.snapshot.connection = ConnectionView::Disconnected;
        shell.request_load_more_stopped(cx);
        assert!(!shell.pending_load_more_stopped);
        shell.snapshot.connection = ConnectionView::Connected;
    });

    view.update(cx, |shell, cx| {
        shell.request_load_more_stopped(cx);
        assert!(shell.pending_load_more_stopped);
        // Single-flight: a second click while pending must not re-emit.
        shell.request_load_more_stopped(cx);
    });
    assert_eq!(
        *events
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()),
        1,
        "only one LoadMoreStoppedRequested event while pending"
    );

    view.update(cx, |shell, cx| {
        shell.set_load_more_stopped_result(true, Some("Loaded more history (4 of 5).".into()), cx);
        assert!(!shell.pending_load_more_stopped);
        assert_eq!(
            shell
                .status_notice
                .as_ref()
                .map(|notice| notice.message.as_str()),
            Some("Loaded more history (4 of 5).")
        );
    });
}

#[gpui::test]
fn changing_the_sort_preserves_selection_and_emits_the_query(cx: &mut TestAppContext) {
    let (view, cx) = cx.add_window_view(|window, cx| {
        let mut shell = AppShell::new(Theme::dark(), window, cx);
        shell.snapshot = snapshot(3);
        shell
    });
    let events = Arc::new(std::sync::Mutex::new(Vec::new()));
    let sink = events.clone();
    let _subscription = view.update(cx, |_, cx| {
        cx.subscribe(&view, move |_, _, event: &AppShellEvent, _| {
            if let AppShellEvent::QueryChanged(query) = event {
                sink.lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .push(query.clone());
            }
        })
    });

    let selected = view.update(cx, |shell, _| {
        let identity = shell.snapshot.tasks[1].identity.clone();
        shell.selected = Some(identity.clone());
        shell.selected_tasks.insert(identity.clone());
        identity
    });

    view.update(cx, |shell, cx| {
        shell.set_sort_key(WorkspaceSortKey::Size, cx);
    });

    view.read_with(cx, |shell, _| {
        assert_eq!(shell.query.sort_key, WorkspaceSortKey::Size);
        assert!(
            shell.selected_tasks.contains(&selected),
            "sort changes must preserve identity-based selection (D-014)"
        );
        assert_eq!(shell.selected.as_ref(), Some(&selected));
    });
    let captured = events
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    assert!(
        captured
            .iter()
            .any(|query| query.sort_key == WorkspaceSortKey::Size),
        "changing the sort key must emit a QueryChanged event"
    );
}

/// §6: cookies + auto-submit is the highest-impact configuration, so the first
/// hand-off of the session is confirmed regardless.
#[gpui::test]
fn cookies_plus_auto_submit_still_confirms_once_per_session(cx: &mut TestAppContext) {
    let (view, cx) = cx.add_window_view(|window, cx| {
        let mut shell = AppShell::new(Theme::dark(), window, cx);
        shell.snapshot = snapshot(1);
        shell.settings.browser_bridge.auto_submit = true;
        shell.settings.browser_bridge.allow_cookies = true;
        shell
    });

    view.update_in(cx, |shell, window, cx| {
        assert!(shell.bridge_requires_confirmation());
        let downloads = [bridge_download(
            "https://example.test/first.bin",
            "https://example.test/p",
            Some("session=secret"),
        )];
        assert_eq!(shell.accept_bridge_downloads(&downloads, window, cx), 1);
        let dialog = shell.add_dialog_entity().expect("dialog open").read(cx);
        assert!(dialog.pending.is_none(), "first add is confirmed");

        // Cancelling does not buy the unlock.
        shell.close_add_download(window, cx);
        assert!(shell.bridge_requires_confirmation());
    });

    // Confirming one hand-off unlocks the rest of the session.
    view.update_in(cx, |shell, window, cx| {
        let downloads = [bridge_download(
            "https://example.test/second.bin",
            "https://example.test/p",
            Some("session=secret"),
        )];
        assert_eq!(shell.accept_bridge_downloads(&downloads, window, cx), 1);
        shell.submit_add_download(cx);
        assert!(
            shell
                .add_dialog_entity()
                .expect("dialog open")
                .read(cx)
                .pending
                .is_some()
        );
        assert!(!shell.bridge_requires_confirmation());
    });
}

#[gpui::test]
fn task_removal_requires_the_matching_internal_confirmation(cx: &mut TestAppContext) {
    let (view, cx) = cx.add_window_view(|window, cx| {
        let mut shell = AppShell::new(Theme::dark(), window, cx);
        shell.snapshot = snapshot(1);
        shell.selected = Some(shell.snapshot.tasks[0].identity.clone());
        shell.confirm_remove_selected(window, cx);
        shell
    });
    view.read_with(cx, |shell, _| {
        assert!(shell.remove_confirmation_modal().is_some());
        assert!(
            !shell
                .remove_confirmation_modal()
                .is_some_and(|value| value.delete_files)
        );
        assert!(shell.pending_task_command.is_none());
    });
    view.update(cx, |shell, cx| shell.submit_remove_confirmation(cx));
    view.read_with(cx, |shell, _| {
        assert!(shell.remove_confirmation_modal().is_none());
        assert!(matches!(
            shell
                .pending_task_command
                .as_ref()
                .map(|pending| pending.command.clone()),
            Some(TaskCommandView::RemoveTask)
        ));
    });
}

#[gpui::test]
fn local_removal_can_explicitly_request_recycle_bin_files(cx: &mut TestAppContext) {
    let (view, cx) = cx.add_window_view(|window, cx| {
        let mut shell = AppShell::new(Theme::dark(), window, cx);
        shell.engine_health = EngineHealthView::Running { restarts: 0 };
        shell.snapshot = snapshot(1);
        shell.selected = Some(shell.snapshot.tasks[0].identity.clone());
        shell.confirm_remove_selected(window, cx);
        shell
    });
    view.update(cx, |shell, cx| shell.toggle_remove_files(cx));
    view.read_with(cx, |shell, _| {
        assert!(
            shell
                .remove_confirmation_modal()
                .is_some_and(|value| value.delete_files)
        );
    });
    view.update(cx, |shell, cx| shell.submit_remove_confirmation(cx));
    view.read_with(cx, |shell, _| {
        assert!(matches!(
            shell
                .pending_task_command
                .as_ref()
                .map(|pending| pending.command.clone()),
            Some(TaskCommandView::RemoveTaskAndFiles)
        ));
    });
}

#[gpui::test]
fn navigation_shortcuts_return_to_downloads_and_preserve_selection(cx: &mut TestAppContext) {
    let (view, cx) = cx.add_window_view(|window, cx| {
        let mut shell = AppShell::new(Theme::dark(), window, cx);
        shell.snapshot = snapshot(2);
        shell.selected = Some(shell.snapshot.tasks[1].identity.clone());
        shell.page = AppPage::Settings;
        shell
    });
    let selected = view.read_with(cx, |shell, _| shell.selected.clone());

    view.update_in(cx, |shell, window, cx| {
        shell.focus_search(&FocusSearch, window, cx);
    });
    view.read_with(cx, |shell, _| {
        assert_eq!(shell.page, AppPage::Downloads);
        assert_eq!(shell.selected, selected);
    });

    view.update_in(cx, |shell, window, cx| {
        shell.page = AppPage::Settings;
        shell.open_add_download(&OpenAddDownload, window, cx);
    });
    view.read_with(cx, |shell, _| {
        assert_eq!(shell.page, AppPage::Downloads);
        assert!(shell.add_dialog_entity().is_some());
        assert_eq!(shell.selected, selected);
    });
}

#[gpui::test]
fn failed_directory_save_keeps_the_draft(cx: &mut TestAppContext) {
    let initial = SettingsView {
        color_scheme: ColorSchemeView::Dark,
        language: LanguagePreferenceView::default(),
        download_directory: "C:/Downloads".into(),
        categories: vec![crate::DownloadCategoryView {
            id: "00000000-0000-4000-8000-000000000001".into(),
            name: "General".into(),
            directory: "C:/Downloads".into(),
            extensions: String::new(),
            is_fallback: true,
        }],
        ..SettingsView::default()
    };
    let (view, cx) =
        cx.add_window_view(move |window, cx| AppShell::new_with_settings(initial, window, cx));
    view.update_in(cx, |shell, window, cx| {
        shell.open_settings(&OpenSettings, window, cx);
        let panel = shell.settings_panel().expect("settings open").clone();
        panel.update(cx, |panel, cx| {
            panel.settings_page.draft_categories[0].directory = "D:/Transfers".into();
            panel.submit_settings(cx);
        });
    });
    let (request_id, requested) = view.read_with(cx, |shell, _| {
        let pending = shell
            .pending_settings_save
            .as_ref()
            .expect("settings save must become pending");
        (pending.request_id, pending.settings.clone())
    });

    view.update_in(cx, |shell, window, cx| {
        shell.set_settings_save_result(
            SettingsSaveResultView {
                request_id,
                settings: requested,
                outcome: SettingsSaveOutcomeView::Failure(OperationErrorView {
                    code: "settings.write_failed".into(),
                    summary: "Could not write settings.".into(),
                    retryable: true,
                }),
            },
            window,
            cx,
        );
    });
    view.read_with(cx, |shell, cx| {
        assert_eq!(shell.settings.download_directory, "C:/Downloads");
        let panel = shell.settings_panel().expect("settings open").read(cx);
        assert_eq!(
            panel.settings_page.draft_categories[0].directory,
            "D:/Transfers"
        );
        assert_eq!(shell.page, AppPage::Settings);
        assert!(panel.settings_page.error.is_some());
    });
}

#[gpui::test]
fn list_preferences_restore_and_emit_without_search(cx: &mut TestAppContext) {
    let (view, cx) = cx.add_window_view(|window, cx| AppShell::new(Theme::dark(), window, cx));
    let events = Arc::new(std::sync::Mutex::new(Vec::new()));
    let sink = events.clone();
    let _subscription = view.update(cx, |_, cx| {
        cx.subscribe(&view, move |_, _, event: &AppShellEvent, _| {
            if let AppShellEvent::UiPreferencesChanged {
                filter,
                sort_key,
                sort_direction,
            } = event
            {
                sink.lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .push((*filter, *sort_key, *sort_direction));
            }
        })
    });

    view.update(cx, |shell, cx| {
        shell.restore_list_preferences(
            WorkspaceQuery {
                filter: WorkspaceFilter::Completed,
                search: "should-not-stick".into(),
                sort_key: WorkspaceSortKey::Size,
                sort_direction: WorkspaceSortDirection::Descending,
                category_id: None,
            },
            cx,
        );
        assert_eq!(shell.query.filter, WorkspaceFilter::Completed);
        assert_eq!(shell.query.sort_key, WorkspaceSortKey::Size);
        assert_eq!(
            shell.query.sort_direction,
            WorkspaceSortDirection::Descending
        );
        assert!(shell.query.search.is_empty());
    });
    assert!(
        events
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .is_empty(),
        "restore must not re-persist preferences"
    );

    view.update_in(cx, |shell, window, cx| {
        shell.set_filter(WorkspaceFilter::Failed, window, cx);
    });
    let captured = events
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone();
    assert_eq!(
        captured.last().copied(),
        Some((
            WorkspaceFilter::Failed,
            WorkspaceSortKey::Size,
            WorkspaceSortDirection::Descending
        ))
    );
}

#[gpui::test]
fn platform_preferences_save_and_close_policy(cx: &mut TestAppContext) {
    let (view, cx) = cx.add_window_view(|window, cx| AppShell::new(Theme::dark(), window, cx));
    let events = Arc::new(std::sync::Mutex::new(Vec::new()));
    let sink = events.clone();
    let _subscription = view.update(cx, |_, cx| {
        cx.subscribe(&view, move |_, _, event: &AppShellEvent, _| match event {
            AppShellEvent::HideToTrayRequested => sink
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .push("hide"),
            AppShellEvent::QuitRequested => sink
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .push("quit"),
            _ => {}
        })
    });

    view.update_in(cx, |shell, window, cx| {
        shell.open_settings(&OpenSettings, window, cx);
        let panel = shell.settings_panel().expect("settings open").clone();
        panel.update(cx, |panel, cx| {
            panel.select_close_behavior(CloseBehaviorView::Quit, cx);
            panel.toggle_start_minimized_to_tray(cx);
            panel.toggle_watch_clipboard(cx);
            panel.submit_platform(cx);
        });
    });
    view.update_in(cx, |shell, window, cx| {
        let pending = shell
            .pending_settings_save
            .as_ref()
            .expect("platform save pending");
        assert_eq!(pending.source, SettingsSaveSource::Platform);
        assert_eq!(
            pending.settings.platform.close_behavior,
            CloseBehaviorView::Quit
        );
        assert!(pending.settings.platform.start_minimized_to_tray);
        assert!(pending.settings.platform.watch_clipboard);

        // Apply saved platform prefs and verify close interception.
        shell.apply_settings(pending.settings.clone(), Some(window), cx);
        shell.pending_settings_save = None;
        assert!(shell.handle_window_close_request(cx));

        shell.settings.platform.close_behavior = CloseBehaviorView::MinimizeToTray;
        shell.settings.platform.show_tray_icon = true;
        assert!(!shell.handle_window_close_request(cx));
    });

    let captured = events
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone();
    assert!(
        captured.contains(&"hide"),
        "minimize-to-tray close must emit HideToTrayRequested"
    );
    assert!(
        captured.contains(&"quit"),
        "quit close policy must emit QuitRequested"
    );
}

#[gpui::test]
fn low_disk_report_is_deduplicated_until_recovery(cx: &mut TestAppContext) {
    let (view, cx) = cx.add_window_view(|window, cx| AppShell::new(Theme::dark(), window, cx));
    view.update(cx, |shell, cx| {
        shell.settings.notifications.notify_on_low_disk = true;
        shell.settings.notifications.low_disk_threshold_bytes = 1_000;
        shell.report_disk_space(Some(100), cx);
        assert!(shell.low_disk_active);
        let first_len = shell.activity_log.len();
        shell.report_disk_space(Some(50), cx);
        assert_eq!(shell.activity_log.len(), first_len);
        shell.report_disk_space(Some(5_000), cx);
        assert!(!shell.low_disk_active);
        shell.report_disk_space(Some(10), cx);
        assert!(shell.low_disk_active);
        assert!(shell.activity_log.len() > first_len);
    });
}

#[gpui::test]
fn task_context_menu_populates_items_for_selected_task(cx: &mut TestAppContext) {
    let (shell, mut cx) = open_test_shell(cx, |window, cx| {
        let mut shell = AppShell::new(Theme::dark(), window, cx);
        shell.snapshot = snapshot(2);
        shell
    });

    shell.update_in(&mut cx, |shell, window, cx| {
        let target = shell
            .prepare_task_context_menu(&task(0).identity, window, cx)
            .expect("target task");
        let weak = cx.entity().downgrade();
        let menu = PopupMenu::build(window, cx, |menu, _, _| {
            shell.populate_task_context_menu(menu, &target, weak)
        });
        assert!(!menu.read(cx).is_empty());
    });
}

#[gpui::test]
fn task_list_keyboard_navigation_supports_home_end_page_and_shift_range(cx: &mut TestAppContext) {
    cx.update(crate::init);
    let (view, cx) = cx.add_window_view(|window, cx| {
        let mut shell = AppShell::new(Theme::dark(), window, cx);
        shell.snapshot = snapshot(25);
        window.focus(&shell.focus_handle, cx);
        shell
    });

    cx.simulate_keystrokes("end");
    view.read_with(cx, |shell, _| {
        assert_eq!(shell.selected, Some(task(24).identity));
    });

    cx.simulate_keystrokes("home");
    view.read_with(cx, |shell, _| {
        assert_eq!(shell.selected, Some(task(0).identity));
    });

    cx.simulate_keystrokes("pagedown");
    view.read_with(cx, |shell, _| {
        let idx = shell
            .selected_index()
            .expect("task selected after pagedown");
        assert!(idx > 0 && idx <= 24);
    });

    cx.simulate_keystrokes("pageup");
    view.read_with(cx, |shell, _| {
        assert_eq!(shell.selected, Some(task(0).identity));
    });

    cx.simulate_keystrokes("shift-down");
    cx.simulate_keystrokes("shift-down");
    view.read_with(cx, |shell, _| {
        assert_eq!(shell.selected, Some(task(2).identity));
        assert_eq!(
            shell.selected_tasks,
            HashSet::from([task(0).identity, task(1).identity, task(2).identity])
        );
    });

    cx.simulate_keystrokes("shift-up");
    view.read_with(cx, |shell, _| {
        assert_eq!(shell.selected, Some(task(1).identity));
        assert_eq!(
            shell.selected_tasks,
            HashSet::from([task(0).identity, task(1).identity])
        );
    });
}

#[test]
fn band_selector_geometry_and_modifiers_work_as_expected() {
    use super::super::band_selector::{
        band_row_range, compute_band_selection, edge_autoscroll_delta, marquee_overlay_geometry,
    };
    use gpui::{Bounds, size};

    assert_eq!(band_row_range(10.0, 140.0, 64.0, 10), Some(0..=2));
    // Dragging upward reverses coordinates cleanly.
    assert_eq!(band_row_range(200.0, 70.0, 64.0, 10), Some(1..=3));
    // Below all rows selects nothing.
    assert_eq!(band_row_range(700.0, 800.0, 64.0, 10), None);
    // Exact boundary at bottom of row 0 (64.0) does not leak into row 1.
    assert_eq!(band_row_range(0.0, 64.0, 64.0, 10), Some(0..=0));

    // Edge auto-scroll triggers near top and bottom, zero in center.
    assert!(edge_autoscroll_delta(105.0, 100.0, 500.0) < 0.0);
    assert_eq!(edge_autoscroll_delta(300.0, 100.0, 500.0), 0.0);
    assert!(edge_autoscroll_delta(490.0, 100.0, 500.0) > 0.0);

    // Overlay rect clamps to viewport bounds when scrolled.
    let viewport = Bounds::new(Point::new(px(200.0), px(100.0)), size(px(600.0), px(400.0)));
    let overlay = marquee_overlay_geometry(
        Point::new(px(40.0), px(300.0)),
        Point::new(px(350.0), px(550.0)),
        viewport,
        Point::new(px(0.0), px(-120.0)),
    )
    .expect("overlay");
    assert_eq!(overlay.left, 40.0);
    assert_eq!(overlay.top, 180.0);
    assert_eq!(overlay.width, 110.0);
    assert_eq!(overlay.height, 220.0);

    // Modifier combinations: replace, Shift union, Ctrl symmetric toggle.
    let rows = snapshot(5).tasks;
    let base = HashSet::from([task(0).identity, task(2).identity]);
    assert_eq!(
        compute_band_selection(&rows, &base, Some(1..=2), false, false),
        HashSet::from([task(1).identity, task(2).identity])
    );
    assert_eq!(
        compute_band_selection(&rows, &base, Some(1..=2), false, true),
        HashSet::from([task(0).identity, task(1).identity, task(2).identity])
    );
    assert_eq!(
        compute_band_selection(&rows, &base, Some(1..=2), true, false),
        HashSet::from([task(0).identity, task(1).identity])
    );
}

#[gpui::test]
fn marquee_drag_updates_selection_and_empty_click_clears_selection(cx: &mut TestAppContext) {
    let (view, cx) = cx.add_window_view(|window, cx| {
        let mut shell = AppShell::new(Theme::dark(), window, cx);
        shell.snapshot = snapshot(6);
        shell
    });
    cx.run_until_parked();
    let _ = cx.update(|window, cx| window.draw(cx));

    let viewport = view.read_with(cx, |shell, _| shell.list_viewport_bounds());
    assert!(viewport.size.width > px(0.0) && viewport.size.height > px(0.0));

    // Real GPUI mouse drag from row 1 into row 3 (> 5px drag threshold).
    let start = Point::new(viewport.origin.x + px(120.0), viewport.origin.y + px(80.0));
    let end = Point::new(viewport.origin.x + px(180.0), viewport.origin.y + px(210.0));
    cx.simulate_mouse_down(start, gpui::MouseButton::Left, Default::default());
    cx.simulate_mouse_move(end, Some(gpui::MouseButton::Left), Default::default());
    cx.simulate_mouse_up(end, gpui::MouseButton::Left, Default::default());
    cx.run_until_parked();

    view.read_with(cx, |shell, _| {
        assert_eq!(
            shell.selected_tasks,
            HashSet::from([task(1).identity, task(2).identity, task(3).identity])
        );
        assert_eq!(shell.range_anchor, Some(task(1).identity));
        assert_eq!(shell.selected, Some(task(3).identity));
        assert!(!shell.marquee_consumed_click);
    });

    // Upward drag from row 4 to row 2 sets anchor at start (row 4) and focus at end (row 2).
    let up_start = Point::new(viewport.origin.x + px(120.0), viewport.origin.y + px(270.0));
    let up_end = Point::new(viewport.origin.x + px(180.0), viewport.origin.y + px(140.0));
    cx.simulate_mouse_down(up_start, gpui::MouseButton::Left, Default::default());
    cx.simulate_mouse_move(up_end, Some(gpui::MouseButton::Left), Default::default());
    cx.simulate_mouse_up(up_end, gpui::MouseButton::Left, Default::default());
    cx.run_until_parked();
    view.read_with(cx, |shell, _| {
        assert_eq!(
            shell.selected_tasks,
            HashSet::from([task(2).identity, task(3).identity, task(4).identity])
        );
        assert_eq!(shell.range_anchor, Some(task(4).identity));
        assert_eq!(shell.selected, Some(task(2).identity));
    });

    // Dragging exclusively across empty space below all rows clears both selected_tasks and selected.
    let empty_a = Point::new(
        viewport.origin.x + px(120.0),
        viewport.origin.y + px(6.0 * super::super::TASK_ROW_HEIGHT + 16.0),
    );
    let empty_b = Point::new(
        viewport.origin.x + px(180.0),
        viewport.origin.y + px(6.0 * super::super::TASK_ROW_HEIGHT + 44.0),
    );
    cx.simulate_mouse_down(empty_a, gpui::MouseButton::Left, Default::default());
    cx.simulate_mouse_move(empty_b, Some(gpui::MouseButton::Left), Default::default());
    cx.simulate_mouse_up(empty_b, gpui::MouseButton::Left, Default::default());
    cx.run_until_parked();
    view.read_with(cx, |shell, _| {
        assert!(shell.selected_tasks.is_empty());
        assert!(shell.selected.is_none());
        assert!(shell.range_anchor.is_none());
    });

    // Short click (< 5px) on row 0 still triggers normal row click selection.
    let row0 = Point::new(viewport.origin.x + px(120.0), viewport.origin.y + px(20.0));
    cx.simulate_click(row0, Default::default());
    cx.run_until_parked();
    view.read_with(cx, |shell, _| {
        assert_eq!(shell.selected_tasks, HashSet::from([task(0).identity]));
        assert_eq!(shell.selected, Some(task(0).identity));
    });

    // Short click in empty space below row 5 clears selection.
    cx.simulate_click(empty_a, Default::default());
    cx.run_until_parked();
    view.read_with(cx, |shell, _| {
        assert!(shell.selected_tasks.is_empty());
        assert!(shell.selected.is_none());
    });
}
