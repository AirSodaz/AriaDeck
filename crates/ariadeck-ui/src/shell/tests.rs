use gpui::{TestAppContext, point, px};

use super::*;
use crate::{
    AddDownloadMetadataFileView, AddDownloadMetadataPreviewItemView, CoreInstallStatusView,
    CoreInstallationView, CoreSourceView, SpeedLimitSettingsView, TaskCountsView,
    TaskNameStateView, TaskSourceKindView, TaskStatusView,
};

fn task(index: usize) -> DownloadRowView {
    DownloadRowView {
        identity: TaskIdentity {
            profile_id: "profile".into(),
            gid: format!("{index:016x}"),
        },
        display_name: format!("archive-{index:05}.bin"),
        name_state: TaskNameStateView::Resolved,
        source_kind: TaskSourceKindView::DirectUri,
        primary_source: Some("https://example.test/file.bin".into()),
        directory: Some("C:/downloads".into()),
        followed_by: Vec::new(),
        belongs_to: None,
        status: TaskStatusView::Complete,
        error: None,
        total_bytes: 1_048_576,
        completed_bytes: 1_048_576,
        uploaded_bytes: 0,
        download_rate: 0,
        upload_rate: 0,
        eta_seconds: Some(0),
        observed_seeding_seconds: None,
        revision: 1,
    }
}

fn snapshot(count: usize) -> WorkspaceSnapshot {
    WorkspaceSnapshot {
        profile_id: "profile".into(),
        session_id: "session".into(),
        generation: 1,
        source_revision: 1,
        connection: ConnectionView::Connected,
        stale: false,
        local_path_actions_available: true,
        download_rate: 0,
        upload_rate: 0,
        speed_history: Vec::new(),
        counts: TaskCountsView {
            all: count,
            completed: count,
            ..TaskCountsView::default()
        },
        stopped_history: crate::StoppedHistoryView {
            loaded: count,
            total: Some(count),
            can_load_more: false,
        },
        tasks: (0..count).map(task).collect(),
        capabilities: crate::EngineCapabilitiesView::unknown(),
    }
}

fn details(file_count: usize) -> TaskDetailsView {
    TaskDetailsView {
        directory: Some("C:/downloads".into()),
        primary_source: Some("https://example.test/file.bin".into()),
        output_path: Some("C:/downloads".into()),
        path_validation: TaskPathValidationView::Valid {
            existing_files: file_count,
            missing_paths: 0,
        },
        info_hash: Some("0123456789abcdef".into()),
        piece_length: Some(1_048_576),
        piece_count: Some(file_count as u32),
        trackers: vec![TaskTrackerView {
            tier: 1,
            uri: "https://tracker.example/announce".into(),
        }],
        uris: vec![TaskUriView {
            uri: "https://example.test/file.bin".into(),
            status: crate::TaskUriStatusView::Used,
        }],
        servers: Vec::new(),
        peers: Vec::new(),
        options: vec![TaskOptionView {
            key: "max-download-limit".into(),
            value: "0".into(),
            redacted: false,
        }],
        files: (0..file_count)
            .map(|index| TaskFileView {
                index: index as u32 + 1,
                path: format!("C:/downloads/file-{index:05}.bin"),
                length: 1_048_576,
                completed_length: 524_288,
                selected: true,
            })
            .collect(),
    }
}

fn metadata_preview(
    path: &str,
    kind: AddDownloadMetadataKindView,
    file_count: u32,
) -> AddDownloadMetadataPreviewView {
    AddDownloadMetadataPreviewView {
        path: PathBuf::from(path),
        kind,
        content_sha256: "digest".into(),
        info_hash: (kind == AddDownloadMetadataKindView::Torrent)
            .then(|| "0123456789abcdef0123456789abcdef01234567".into()),
        files: (1..=file_count)
            .map(|index| AddDownloadMetadataFileView {
                index,
                path: format!("file-{index}.bin"),
                length: Some(u64::from(index) * 100),
            })
            .collect(),
        selected_file_indices: (1..=file_count).collect(),
    }
}

#[test]
fn task_layout_uses_the_remaining_main_pane_width() {
    assert_eq!(task_layout_mode(1_180.0, false), TaskLayoutMode::Wide);
    assert_eq!(task_layout_mode(1_180.0, true), TaskLayoutMode::Compact);
    assert_eq!(task_layout_mode(960.0, false), TaskLayoutMode::Compact);
    assert_eq!(task_layout_mode(1_400.0, true), TaskLayoutMode::Wide);
}

#[test]
fn search_bounds_are_centered_and_ignore_workspace_drawers() {
    for viewport_width in [960.0, 1_180.0, 1_600.0] {
        let (left, right) = centered_search_bounds(viewport_width);
        assert!(((left + right) / 2.0 - viewport_width / 2.0).abs() < f32::EPSILON);
        assert!(right - left <= SEARCH_WIDTH);
    }
}

#[cfg(target_os = "windows")]
#[test]
fn window_controls_map_to_native_areas_and_accessible_labels() {
    let minimize = window_control_config(WindowControlKind::Minimize, false);
    assert_eq!(minimize.area, WindowControlArea::Min);
    assert_eq!(minimize.icon, IconName::WindowMinimize);
    assert_eq!(minimize.label, "Minimize window");
    assert!(!minimize.danger);

    let maximize = window_control_config(WindowControlKind::Maximize, false);
    assert_eq!(maximize.area, WindowControlArea::Max);
    assert_eq!(maximize.icon, IconName::WindowMaximize);
    assert_eq!(maximize.label, "Maximize window");

    let restore = window_control_config(WindowControlKind::Maximize, true);
    assert_eq!(restore.icon, IconName::WindowRestore);
    assert_eq!(restore.label, "Restore window");

    let close = window_control_config(WindowControlKind::Close, false);
    assert_eq!(close.area, WindowControlArea::Close);
    assert_eq!(close.icon, IconName::WindowClose);
    assert_eq!(close.label, "Close window");
    assert!(close.danger);
}

#[test]
fn speed_chart_uses_only_the_latest_bounded_window() {
    let history = (0..=SPEED_CHART_SAMPLES)
        .map(|index| SpeedSampleView {
            download_rate: index as u64,
            upload_rate: 0,
        })
        .collect::<Vec<_>>();

    let visible = speed_chart_window(&history);
    assert_eq!(visible.len(), SPEED_CHART_SAMPLES);
    assert_eq!(visible.first().map(|sample| sample.download_rate), Some(1));
    assert_eq!(
        visible.last().map(|sample| sample.download_rate),
        Some(SPEED_CHART_SAMPLES as u64)
    );
}

#[gpui::test]
fn local_engine_health_surfaces_recovery_and_terminal_failure(cx: &mut TestAppContext) {
    let (view, cx) = cx.add_window_view(|window, cx| AppShell::new(Theme::dark(), window, cx));

    view.update(cx, |shell, cx| {
        shell.set_engine_health(EngineHealthView::Running { restarts: 0 }, cx);
        shell.set_engine_health(EngineHealthView::Restarting { attempt: 1 }, cx);
    });
    view.read_with(cx, |shell, _| {
        assert_eq!(shell.engine_health.label(), "Local engine restarting");
        assert!(
            shell.status_notice.is_none(),
            "persistent restart state belongs in the status bar"
        );
    });

    view.update(cx, |shell, cx| {
        shell.set_engine_health(EngineHealthView::Running { restarts: 1 }, cx);
    });
    view.read_with(cx, |shell, _| {
        let notice = shell.status_notice.as_ref().expect("recovery notice");
        assert!(!notice.is_error);
        assert_eq!(
            notice.message,
            "Local aria2 recovered after 1 restart attempt."
        );
    });

    view.update(cx, |shell, cx| {
        shell.set_engine_health(
            EngineHealthView::Failed {
                summary: "restart budget exhausted".into(),
            },
            cx,
        );
    });
    view.read_with(cx, |shell, _| {
        let notice = shell.status_notice.as_ref().expect("failure notice");
        assert!(notice.is_error);
        assert_eq!(shell.engine_health.label(), "Local engine stopped");
    });
}

#[gpui::test]
fn ten_thousand_tasks_render_only_a_viewport_window(cx: &mut TestAppContext) {
    let (view, cx) = cx.add_window_view(|window, cx| {
        let mut shell = AppShell::new(Theme::dark(), window, cx);
        shell.snapshot = snapshot(10_000);
        shell
    });

    view.read_with(cx, |shell, _| {
        let rendered = shell.rendered_range();
        assert!(!rendered.is_empty());
        assert!(rendered.len() < 64, "rendered {} rows", rendered.len());
        assert_eq!(shell.snapshot.tasks.len(), 10_000);
    });
}

#[gpui::test]
fn selection_survives_filtered_snapshots_for_the_same_profile(cx: &mut TestAppContext) {
    let (view, cx) = cx.add_window_view(|window, cx| {
        let mut shell = AppShell::new(Theme::dark(), window, cx);
        shell.snapshot = snapshot(3);
        shell.selected = Some(shell.snapshot.tasks[1].identity.clone());
        shell
    });
    let selected = view.read_with(cx, |shell, _| shell.selected.clone());

    view.update(cx, |shell, cx| {
        let mut filtered = snapshot(1);
        filtered.tasks[0] = task(2);
        shell.set_snapshot(filtered, cx);
    });
    view.read_with(cx, |shell, _| {
        assert_eq!(shell.selected, selected);
    });
}

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

    cx.simulate_keystrokes("secondary-a");
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
        assert!(shell.context_menu.is_some());
        assert_eq!(shell.selected.as_ref(), Some(&task(1).identity));
        assert_eq!(shell.selected_tasks.len(), 2);
        assert!(shell.selected_tasks.contains(&task(0).identity));
        assert!(shell.selected_tasks.contains(&task(1).identity));

        // Multi-select still targets the right-clicked row for copy.
        let target = shell.context_menu_task_view().expect("menu target");
        shell.copy_task_gid(&target, cx);
        assert_eq!(
            cx.read_from_clipboard().and_then(|item| item.text()),
            Some(task(1).identity.gid.clone()),
        );

        shell.close_task_context_menu(cx);
        assert!(shell.context_menu.is_none());
        // Closing the menu does not drop the multi-selection.
        assert_eq!(shell.selected_tasks.len(), 2);
    });
}

#[gpui::test]
fn queue_priority_keyboard_actions_are_wired_on_the_workspace(cx: &mut TestAppContext) {
    cx.update(crate::init);
    let (view, cx) = cx.add_window_view(|window, cx| {
        let mut shell = AppShell::new(Theme::dark(), window, cx);
        shell.snapshot = snapshot(3);
        // Active tasks so queue moves are status-eligible; query stays the
        // authoritative ascending queue (All, no search, Queue/asc).
        for task in &mut shell.snapshot.tasks {
            task.status = TaskStatusView::Active;
        }
        shell.selected = Some(task(1).identity);
        shell.selected_tasks = HashSet::from([task(1).identity]);
        window.focus(&shell.focus_handle, cx);
        shell
    });

    cx.simulate_keystrokes("cmd-shift-up");

    view.read_with(cx, |shell, _| {
        assert!(
            shell.pending_task_command.as_ref().is_some_and(|pending| {
                matches!(pending.command, TaskCommandView::MoveUpInQueue)
                    && pending.identity == task(1).identity
            }),
            "Cmd+Shift+Up should submit MoveUpInQueue for the focused task"
        );
    });
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
fn batch_partial_result_retains_only_failed_source_tasks(cx: &mut TestAppContext) {
    let (view, cx) = cx.add_window_view(|window, cx| {
        let mut shell = AppShell::new(Theme::dark(), window, cx);
        shell.snapshot = snapshot(3);
        for task in &mut shell.snapshot.tasks {
            task.status = TaskStatusView::Active;
        }
        shell.select_at_with_modifiers(0, false, false, window, cx);
        shell.select_at_with_modifiers(1, false, true, window, cx);
        shell.begin_batch_task_command(BatchTaskCommandView::Pause, cx);
        shell
    });
    let result = view.read_with(cx, |shell, _| {
        let pending = shell
            .pending_batch_command
            .as_ref()
            .expect("batch command pending");
        assert_eq!(pending.identities, vec![task(0).identity, task(1).identity]);
        BatchTaskCommandResultView {
            request_id: pending.request_id,
            session: pending.session.clone(),
            identities: pending.identities.clone(),
            command: pending.command,
            outcome: BatchCommandOutcomeView::PartialSuccess {
                succeeded: vec![task(0).identity],
                failed: vec![BatchTaskFailureView {
                    identity: Some(task(1).identity),
                    error: OperationErrorView {
                        code: "rpc.command_rejected".into(),
                        summary: "aria2 rejected pause".into(),
                        retryable: false,
                    },
                }],
            },
        }
    });
    view.update_in(cx, |shell, window, cx| {
        shell.set_batch_task_command_result(result, window, cx);
    });
    view.read_with(cx, |shell, _| {
        assert!(shell.pending_batch_command.is_none());
        assert_eq!(shell.selected_tasks, HashSet::from([task(1).identity]));
        assert_eq!(
            shell
                .batch_failure_details
                .as_ref()
                .map(|details| details.failures.len()),
            Some(1)
        );
        assert!(
            shell
                .status_notice
                .as_ref()
                .is_some_and(|notice| notice.is_error)
        );
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
fn magnet_successor_migrates_nonfocused_selection_anchor_and_details(cx: &mut TestAppContext) {
    let (view, cx) = cx.add_window_view(|window, cx| {
        let mut shell = AppShell::new(Theme::dark(), window, cx);
        let mut previous = snapshot(3);
        previous.tasks[0].followed_by = vec![format!("{:016x}", 3)];
        let parent = previous.tasks[0].clone();
        let focused = previous.tasks[1].identity.clone();
        shell.snapshot = previous;
        shell.selected = Some(focused.clone());
        shell.selected_tasks = HashSet::from([parent.identity.clone(), focused]);
        shell.range_anchor = Some(parent.identity.clone());
        shell.open_details_for(parent, cx);
        shell
    });

    view.update_in(cx, |shell, _window, cx| {
        let mut next = snapshot(3);
        next.tasks[0] = task(3);
        next.tasks[0].belongs_to = Some(format!("{:016x}", 0));
        shell.set_snapshot(next, cx);
    });
    view.read_with(cx, |shell, _| {
        let parent = task(0).identity;
        let successor = task(3).identity;
        assert_eq!(shell.selected, Some(task(1).identity));
        assert_eq!(shell.selected_tasks.len(), 2);
        assert!(!shell.selected_tasks.contains(&parent));
        assert!(shell.selected_tasks.contains(&successor));
        assert_eq!(shell.range_anchor, Some(successor.clone()));
        assert_eq!(
            shell
                .details_drawer
                .as_ref()
                .map(|drawer| drawer.identity.clone()),
            Some(successor)
        );
    });
}

#[gpui::test]
fn add_download_submission_is_single_flight(cx: &mut TestAppContext) {
    let (view, cx) = cx.add_window_view(|window, cx| {
        let mut shell = AppShell::new(Theme::dark(), window, cx);
        shell.snapshot = snapshot(1);
        shell.add_dialog.open = true;
        shell
    });

    view.update(cx, |shell, cx| {
        shell.add_input.update(cx, |input, cx| {
            input.set_text("https://example.com/archive.bin", cx);
        });
        shell.submit_add_download(cx);
        let first = shell
            .add_dialog
            .pending
            .as_ref()
            .expect("first submit must become pending")
            .request_id;
        shell.submit_add_download(cx);
        assert_eq!(
            shell
                .add_dialog
                .pending
                .as_ref()
                .expect("second submit must retain pending request")
                .request_id,
            first
        );
        assert_eq!(shell.next_request_id, first.get() + 1);
    });
}

#[gpui::test]
fn task_command_submission_is_single_flight(cx: &mut TestAppContext) {
    let (view, cx) = cx.add_window_view(|window, cx| {
        let mut shell = AppShell::new(Theme::dark(), window, cx);
        shell.snapshot = snapshot(1);
        shell.snapshot.tasks[0].status = TaskStatusView::Active;
        shell.selected = Some(shell.snapshot.tasks[0].identity.clone());
        shell
    });

    view.update(cx, |shell, cx| {
        shell.begin_task_command(TaskCommandView::Pause, cx);
        let first = shell
            .pending_task_command
            .as_ref()
            .expect("first command must become pending")
            .request_id;
        shell.begin_task_command(TaskCommandView::Pause, cx);
        assert_eq!(
            shell
                .pending_task_command
                .as_ref()
                .expect("duplicate command must retain the first request")
                .request_id,
            first
        );
        assert_eq!(shell.next_request_id, first.get() + 1);
    });
}

#[gpui::test]
fn queue_reordering_is_authoritative_only_for_the_unfiltered_ascending_queue(
    cx: &mut TestAppContext,
) {
    let (view, cx) = cx.add_window_view(|window, cx| AppShell::new(Theme::dark(), window, cx));

    view.read_with(cx, |shell, _| {
        assert!(
            shell.queue_reordering_available(),
            "default query is All / no search / Queue / Ascending"
        );
    });

    view.update(cx, |shell, cx| {
        shell.set_sort_key(WorkspaceSortKey::Progress, cx);
    });
    view.read_with(cx, |shell, _| {
        assert!(
            !shell.queue_reordering_available(),
            "a value sort is not an authoritative queue position"
        );
    });

    view.update(cx, |shell, cx| {
        shell.set_sort_key(WorkspaceSortKey::Queue, cx);
        shell.set_sort_direction(WorkspaceSortDirection::Descending, cx);
    });
    view.read_with(cx, |shell, _| {
        assert!(
            !shell.queue_reordering_available(),
            "a reversed queue is not an authoritative position"
        );
    });

    view.update_in(cx, |shell, window, cx| {
        shell.set_sort_direction(WorkspaceSortDirection::Ascending, cx);
        shell.set_filter(WorkspaceFilter::Active, window, cx);
    });
    view.read_with(cx, |shell, _| {
        assert!(
            !shell.queue_reordering_available(),
            "a filtered projection hides the global queue position"
        );
    });

    view.update_in(cx, |shell, window, cx| {
        shell.set_filter(WorkspaceFilter::All, window, cx);
        shell.search_input.update(cx, |input, cx| {
            input.set_text("archive", cx);
        });
    });
    view.read_with(cx, |shell, _| {
        assert!(
            !shell.queue_reordering_available(),
            "a searched projection hides the global queue position"
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

#[gpui::test]
fn queue_priority_command_is_blocked_outside_the_authoritative_queue(cx: &mut TestAppContext) {
    let (view, cx) = cx.add_window_view(|window, cx| {
        let mut shell = AppShell::new(Theme::dark(), window, cx);
        shell.snapshot = snapshot(2);
        shell.snapshot.tasks[0].status = TaskStatusView::Waiting;
        shell.selected = Some(shell.snapshot.tasks[0].identity.clone());
        shell
    });

    // Reversed queue: priority movement is not authoritative and is rejected.
    view.update(cx, |shell, cx| {
        shell.set_sort_direction(WorkspaceSortDirection::Descending, cx);
        shell.begin_task_command(TaskCommandView::MoveUpInQueue, cx);
    });
    view.read_with(cx, |shell, _| {
        assert!(
            shell.pending_task_command.is_none(),
            "queue movement must not start while the query is reversed"
        );
        assert!(
            shell
                .status_notice
                .as_ref()
                .is_some_and(|notice| notice.is_error)
        );
    });

    // Restore the authoritative queue: the command now becomes pending.
    view.update(cx, |shell, cx| {
        shell.set_sort_direction(WorkspaceSortDirection::Ascending, cx);
        shell.begin_task_command(TaskCommandView::MoveToQueueTop, cx);
    });
    view.read_with(cx, |shell, _| {
        let pending = shell
            .pending_task_command
            .as_ref()
            .expect("queue movement must be pending in the authoritative queue");
        assert_eq!(pending.command, TaskCommandView::MoveToQueueTop);
    });
}

#[gpui::test]
fn global_pause_all_becomes_pending_and_emits_the_engine_wide_command(cx: &mut TestAppContext) {
    let (view, cx) = cx.add_window_view(|window, cx| {
        let mut shell = AppShell::new(Theme::dark(), window, cx);
        shell.snapshot = snapshot(2);
        shell
    });
    let events = Arc::new(std::sync::Mutex::new(Vec::new()));
    let sink = events.clone();
    let _subscription = view.update(cx, |_, cx| {
        cx.subscribe(&view, move |_, _, event: &AppShellEvent, _| {
            if let AppShellEvent::GlobalTaskCommandRequested(request) = event {
                sink.lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .push(request.command);
            }
        })
    });

    view.update(cx, |shell, cx| {
        shell.begin_global_task_command(GlobalTaskCommandView::PauseAll, cx);
    });

    view.read_with(cx, |shell, _| {
        let pending = shell
            .pending_global_task_command
            .as_ref()
            .expect("pause-all must become pending");
        assert_eq!(pending.command, GlobalTaskCommandView::PauseAll);
    });
    let captured = events
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    assert_eq!(captured.as_slice(), &[GlobalTaskCommandView::PauseAll]);
}

#[test]
fn add_download_input_parses_trimmed_non_empty_lines_with_source_positions() {
    let sources =
        parse_add_download_sources("  https://example.test/one  \r\n\r\nmagnet:?xt=urn:btih:abc\n");

    assert_eq!(
        sources,
        vec![
            AddDownloadSourceView::Uri {
                line: 1,
                uri: "https://example.test/one".into(),
            },
            AddDownloadSourceView::Uri {
                line: 3,
                uri: "magnet:?xt=urn:btih:abc".into(),
            },
        ]
    );
}

#[gpui::test]
fn metadata_paths_are_classified_deduplicated_switchable_and_removable(cx: &mut TestAppContext) {
    let (view, cx) = cx.add_window_view(|window, cx| {
        let mut shell = AppShell::new(Theme::dark(), window, cx);
        shell.snapshot = snapshot(1);
        shell
    });

    view.update_in(cx, |shell, window, cx| {
        shell.add_metadata_paths(
            vec![
                PathBuf::from("sample.TORRENT"),
                PathBuf::from("sample.TORRENT"),
                PathBuf::from("bundle.meta4"),
                PathBuf::from("notes.txt"),
            ],
            window,
            cx,
        );
        assert!(shell.add_dialog.open);
        assert_eq!(
            shell.add_dialog.input_mode,
            AddDownloadInputModeView::MetadataFiles
        );
        assert_eq!(shell.add_dialog.mode, AddDownloadModeView::SeparateTasks);
        assert_eq!(
            shell.add_dialog.file_conflict,
            FileConflictPolicyView::Reject
        );
        assert!(shell.add_dialog.metadata_files.is_empty());
        let pending = shell
            .add_dialog
            .preview_pending
            .as_ref()
            .expect("metadata preview must be pending");
        assert_eq!(
            pending.paths,
            vec![
                PathBuf::from("sample.TORRENT"),
                PathBuf::from("bundle.meta4")
            ]
        );
        let request_id = pending.request_id;
        assert!(shell.add_dialog.error.is_some());

        shell.set_add_download_metadata_preview_result(
            AddDownloadMetadataPreviewResultView {
                request_id,
                items: vec![
                    AddDownloadMetadataPreviewItemView {
                        path: PathBuf::from("sample.TORRENT"),
                        outcome: AddDownloadMetadataPreviewOutcomeView::Ready(metadata_preview(
                            "sample.TORRENT",
                            AddDownloadMetadataKindView::Torrent,
                            2,
                        )),
                    },
                    AddDownloadMetadataPreviewItemView {
                        path: PathBuf::from("bundle.meta4"),
                        outcome: AddDownloadMetadataPreviewOutcomeView::Ready(metadata_preview(
                            "bundle.meta4",
                            AddDownloadMetadataKindView::Metalink,
                            1,
                        )),
                    },
                ],
            },
            cx,
        );
        assert_eq!(shell.add_dialog.metadata_files.len(), 2);
        assert_eq!(
            shell.add_dialog.metadata_files[0].selected_file_indices,
            vec![1, 2]
        );
        shell.toggle_metadata_file_entry(0, 2, cx);
        assert_eq!(
            shell.add_dialog.metadata_files[0].selected_file_indices,
            vec![1]
        );
        shell.toggle_all_metadata_file_entries(0, cx);
        assert_eq!(
            shell.add_dialog.metadata_files[0].selected_file_indices,
            vec![1, 2]
        );

        shell.set_add_input_mode(AddDownloadInputModeView::Links, cx);
        assert_eq!(shell.add_dialog.input_mode, AddDownloadInputModeView::Links);
        shell.remove_metadata_file(0, cx);
        assert_eq!(shell.add_dialog.metadata_files.len(), 1);
    });
}

#[gpui::test]
fn metadata_preview_keeps_successes_reports_failures_and_ignores_stale_results(
    cx: &mut TestAppContext,
) {
    let (view, cx) = cx.add_window_view(|window, cx| {
        let mut shell = AppShell::new(Theme::dark(), window, cx);
        shell.snapshot = snapshot(1);
        shell
    });
    let request_id = view.update_in(cx, |shell, window, cx| {
        shell.add_metadata_paths(
            vec![PathBuf::from("one.torrent"), PathBuf::from("two.meta4")],
            window,
            cx,
        );
        shell
            .add_dialog
            .preview_pending
            .as_ref()
            .expect("metadata preview must be pending")
            .request_id
    });

    view.update(cx, |shell, cx| {
        shell.set_add_download_metadata_preview_result(
            AddDownloadMetadataPreviewResultView {
                request_id: RequestId::from_u64(request_id.get() + 1),
                items: vec![
                    AddDownloadMetadataPreviewItemView {
                        path: PathBuf::from("one.torrent"),
                        outcome: AddDownloadMetadataPreviewOutcomeView::Ready(metadata_preview(
                            "one.torrent",
                            AddDownloadMetadataKindView::Torrent,
                            2,
                        )),
                    },
                    AddDownloadMetadataPreviewItemView {
                        path: PathBuf::from("two.meta4"),
                        outcome: AddDownloadMetadataPreviewOutcomeView::Failed(
                            OperationErrorView {
                                code: "validation.invalid_request".into(),
                                summary: "bad metadata".into(),
                                retryable: false,
                            },
                        ),
                    },
                ],
            },
            cx,
        );
    });
    view.read_with(cx, |shell, _| {
        assert!(shell.add_dialog.metadata_files.is_empty());
        assert_eq!(
            shell
                .add_dialog
                .preview_pending
                .as_ref()
                .map(|pending| pending.request_id),
            Some(request_id)
        );
    });

    view.update(cx, |shell, cx| {
        shell.set_add_download_metadata_preview_result(
            AddDownloadMetadataPreviewResultView {
                request_id,
                items: vec![
                    AddDownloadMetadataPreviewItemView {
                        path: PathBuf::from("one.torrent"),
                        outcome: AddDownloadMetadataPreviewOutcomeView::Ready(metadata_preview(
                            "one.torrent",
                            AddDownloadMetadataKindView::Torrent,
                            2,
                        )),
                    },
                    AddDownloadMetadataPreviewItemView {
                        path: PathBuf::from("two.meta4"),
                        outcome: AddDownloadMetadataPreviewOutcomeView::Failed(
                            OperationErrorView {
                                code: "validation.invalid_request".into(),
                                summary: "bad metadata".into(),
                                retryable: false,
                            },
                        ),
                    },
                ],
            },
            cx,
        );
    });
    view.read_with(cx, |shell, _| {
        assert!(shell.add_dialog.preview_pending.is_none());
        assert_eq!(shell.add_dialog.metadata_files.len(), 1);
        assert_eq!(
            shell.add_dialog.metadata_files[0].selected_file_indices,
            vec![1, 2]
        );
        assert!(
            shell
                .add_dialog
                .error
                .as_ref()
                .is_some_and(|error| error.summary.contains("bad metadata"))
        );
    });
}

#[gpui::test]
fn metadata_submit_rejects_zero_selection_and_sums_selected_known_sizes(cx: &mut TestAppContext) {
    let (view, cx) = cx.add_window_view(|window, cx| {
        let mut shell = AppShell::new(Theme::dark(), window, cx);
        shell.snapshot = snapshot(1);
        shell.add_dialog.open = true;
        shell.add_dialog.input_mode = AddDownloadInputModeView::MetadataFiles;
        shell.add_dialog.metadata_files = vec![metadata_preview(
            "one.torrent",
            AddDownloadMetadataKindView::Torrent,
            3,
        )];
        shell.add_dialog.metadata_files[0].files[2].length = None;
        shell
    });

    view.update(cx, |shell, cx| {
        assert_eq!(
            selected_metadata_known_bytes(&shell.add_dialog.metadata_files),
            Some(300)
        );
        shell.toggle_all_metadata_file_entries(0, cx);
        shell.submit_add_download(cx);
    });
    view.read_with(cx, |shell, _| {
        assert!(shell.add_dialog.pending.is_none());
        assert!(
            shell
                .add_dialog
                .error
                .as_ref()
                .is_some_and(|error| error.summary.contains("Select at least one file"))
        );
    });
}

#[test]
fn metadata_drop_is_disabled_while_an_add_request_is_pending() {
    let paths = [PathBuf::from("sample.torrent")];

    assert!(can_accept_metadata_drop(true, &paths));
    assert!(!can_accept_metadata_drop(false, &paths));
}

#[gpui::test]
fn add_download_advanced_options_toggle_and_collect_secrets(cx: &mut TestAppContext) {
    let (view, cx) = cx.add_window_view(|window, cx| {
        let mut shell = AppShell::new(Theme::dark(), window, cx);
        shell.snapshot = snapshot(1);
        shell
    });

    view.update_in(cx, |shell, window, cx| {
        shell.open_add_download(&OpenAddDownload, window, cx);
        assert!(!shell.add_dialog.advanced_open);
        shell.toggle_add_advanced(cx);
        assert!(shell.add_dialog.advanced_open);
        shell.add_inputs.referer.update(cx, |input, cx| {
            input.set_text("https://example.test/ref", cx);
        });
        shell.add_inputs.user_agent.update(cx, |input, cx| {
            input.set_text("AriaDeck-Test/1.0", cx);
        });
        shell.add_inputs.headers.update(cx, |input, cx| {
            input.set_text("X-Token: one\nAccept: */*", cx);
        });
        shell.add_inputs.cookie.update(cx, |input, cx| {
            input.set_text("session=secret-cookie", cx);
        });
        shell.add_inputs.http_user.update(cx, |input, cx| {
            input.set_text("alice", cx);
        });
        shell.add_inputs.http_passwd.update(cx, |input, cx| {
            input.set_text("s3cret", cx);
        });
        shell.add_inputs.checksum.update(cx, |input, cx| {
            input.set_text(format!("sha-256={}", "ab".repeat(32)), cx);
        });
    });

    view.read_with(cx, |shell, cx| {
        let advanced = shell.collect_add_advanced_options(cx);
        assert_eq!(advanced.referer, "https://example.test/ref");
        assert_eq!(advanced.user_agent, "AriaDeck-Test/1.0");
        assert!(advanced.headers.contains("X-Token: one"));
        assert_eq!(
            advanced
                .cookie
                .as_ref()
                .map(|value| value.clone().into_inner()),
            Some("session=secret-cookie".into())
        );
        assert_eq!(advanced.http_user, "alice");
        assert_eq!(
            advanced
                .http_passwd
                .as_ref()
                .map(|value| value.clone().into_inner()),
            Some("s3cret".into())
        );
        assert!(advanced.checksum.starts_with("sha-256="));
        let debug = format!("{advanced:?}");
        assert!(!debug.contains("s3cret"));
        assert!(!debug.contains("secret-cookie"));
        assert!(shell.add_inputs.cookie.read(cx).is_secure());
        assert!(shell.add_inputs.http_passwd.read(cx).is_secure());
    });
}

#[gpui::test]
fn add_download_dialog_accepts_keyboard_input(cx: &mut TestAppContext) {
    let (view, cx) = cx.add_window_view(|window, cx| {
        let mut shell = AppShell::new(Theme::dark(), window, cx);
        shell.snapshot = snapshot(1);
        shell
    });

    view.update_in(cx, |shell, window, cx| {
        shell.open_add_download(&OpenAddDownload, window, cx);
    });
    cx.simulate_input("https://example.com/file");

    view.read_with(cx, |shell, cx| {
        assert_eq!(shell.add_input.read(cx).text(), "https://example.com/file");
    });
}

#[gpui::test]
fn add_download_dialog_input_can_be_clicked_before_typing(cx: &mut TestAppContext) {
    let (view, cx) = cx.add_window_view(|window, cx| {
        let mut shell = AppShell::new(Theme::dark(), window, cx);
        shell.snapshot = snapshot(1);
        shell
    });

    view.update_in(cx, |shell, window, cx| {
        shell.open_add_download(&OpenAddDownload, window, cx);
    });
    let input_bounds = view.read_with(cx, |shell, cx| {
        shell
            .add_input
            .read(cx)
            .text_bounds()
            .expect("add-download input must be painted")
    });
    view.update_in(cx, |shell, window, cx| {
        window.focus(&shell.search_input.focus_handle(cx), cx);
    });
    cx.simulate_click(
        point(input_bounds.left() - px(16.0), input_bounds.center().y),
        Default::default(),
    );
    cx.simulate_input("https://example.com/file");

    view.read_with(cx, |shell, cx| {
        assert_eq!(shell.add_input.read(cx).text(), "https://example.com/file");
    });
}

#[gpui::test]
fn add_download_dialog_supports_standard_clipboard_shortcuts(cx: &mut TestAppContext) {
    cx.update(crate::init);
    let (view, cx) = cx.add_window_view(|window, cx| {
        let mut shell = AppShell::new(Theme::dark(), window, cx);
        shell.snapshot = snapshot(1);
        shell
    });

    view.update_in(cx, |shell, window, cx| {
        shell.open_add_download(&OpenAddDownload, window, cx);
        shell.add_input.update(cx, |input, cx| {
            input.set_text("https://example.com/file", cx);
        });
    });
    cx.simulate_keystrokes("secondary-a secondary-c");
    assert_eq!(
        cx.read_from_clipboard().and_then(|item| item.text()),
        Some("https://example.com/file".to_owned())
    );

    cx.write_to_clipboard(ClipboardItem::new_string(
        "magnet:?xt=urn:btih:test".to_owned(),
    ));
    cx.simulate_keystrokes("secondary-v");
    view.read_with(cx, |shell, cx| {
        assert_eq!(shell.add_input.read(cx).text(), "magnet:?xt=urn:btih:test");
    });

    cx.simulate_keystrokes("secondary-a secondary-x");
    view.read_with(cx, |shell, cx| {
        assert!(shell.add_input.read(cx).text().is_empty());
    });
    assert_eq!(
        cx.read_from_clipboard().and_then(|item| item.text()),
        Some("magnet:?xt=urn:btih:test".to_owned())
    );
}

#[gpui::test]
fn add_download_input_preserves_pasted_lines_and_shift_enter(cx: &mut TestAppContext) {
    cx.update(crate::init);
    let (view, cx) = cx.add_window_view(|window, cx| {
        let mut shell = AppShell::new(Theme::dark(), window, cx);
        shell.snapshot = snapshot(1);
        shell.open_add_download(&OpenAddDownload, window, cx);
        shell
    });

    cx.write_to_clipboard(ClipboardItem::new_string(
        "https://example.test/one\r\nhttps://example.test/two".into(),
    ));
    cx.simulate_keystrokes("secondary-v shift-enter");
    cx.simulate_input("magnet:?xt=urn:btih:abc");

    view.read_with(cx, |shell, cx| {
        assert_eq!(
            shell.add_input.read(cx).text(),
            "https://example.test/one\nhttps://example.test/two\nmagnet:?xt=urn:btih:abc"
        );
        assert_eq!(
            parse_add_download_sources(shell.add_input.read(cx).text()).len(),
            3
        );
    });
}

#[gpui::test]
fn partial_add_result_keeps_only_sources_that_are_safe_to_retry(cx: &mut TestAppContext) {
    let accepted = task(10).identity;
    let accepted_second = task(11).identity;
    let (view, cx) = cx.add_window_view(|window, cx| {
        let mut shell = AppShell::new(Theme::dark(), window, cx);
        shell.snapshot = snapshot(1);
        shell.add_dialog.open = true;
        shell.add_input.update(cx, |input, cx| {
            input.set_text(
                "https://example.test/accepted\nhttps://example.test/retry\nhttps://example.test/unknown",
                cx,
            );
        });
        shell.submit_add_download(cx);
        shell
    });
    let (request_id, session) = view.read_with(cx, |shell, _| {
        let pending = shell.add_dialog.pending.as_ref().expect("add pending");
        (pending.request_id, pending.session.clone())
    });

    view.update_in(cx, |shell, window, cx| {
        shell.set_add_download_result(
            AddDownloadResultView {
                request_id,
                session,
                items: vec![
                    AddDownloadItemResultView {
                        sources: vec![AddDownloadSourceView::Uri {
                            line: 1,
                            uri: "https://example.test/accepted".into(),
                        }],
                        existing_task: None,
                        outcome: CommandOutcomeView::Success {
                            tasks: vec![accepted.clone(), accepted_second.clone()],
                        },
                    },
                    AddDownloadItemResultView {
                        sources: vec![AddDownloadSourceView::Uri {
                            line: 2,
                            uri: "https://example.test/retry".into(),
                        }],
                        existing_task: None,
                        outcome: CommandOutcomeView::Failure(OperationErrorView {
                            code: "rpc.add_not_observed".into(),
                            summary: "Safe to retry".into(),
                            retryable: true,
                        }),
                    },
                    AddDownloadItemResultView {
                        sources: vec![AddDownloadSourceView::Uri {
                            line: 3,
                            uri: "https://example.test/unknown".into(),
                        }],
                        existing_task: None,
                        outcome: CommandOutcomeView::Failure(OperationErrorView {
                            code: "rpc.command_outcome_unknown".into(),
                            summary: "Still unknown".into(),
                            retryable: false,
                        }),
                    },
                ],
            },
            window,
            cx,
        );
    });
    view.read_with(cx, |shell, cx| {
        assert!(shell.add_dialog.open);
        assert_eq!(shell.add_dialog.results.len(), 3);
        assert_eq!(
            shell.add_input.read(cx).text(),
            "https://example.test/retry"
        );
        assert_eq!(
            shell.selected_tasks,
            HashSet::from([accepted.clone(), accepted_second])
        );
        assert_eq!(shell.selected.as_ref(), Some(&accepted));
    });
}

#[gpui::test]
fn duplicate_add_result_focuses_the_existing_task_and_closes_the_dialog(cx: &mut TestAppContext) {
    let existing = task(0).identity;
    let (view, cx) = cx.add_window_view(|window, cx| {
        let mut shell = AppShell::new(Theme::dark(), window, cx);
        shell.snapshot = snapshot(1);
        shell.add_dialog.open = true;
        shell.add_input.update(cx, |input, cx| {
            input.set_text("https://example.test/existing", cx);
        });
        shell.submit_add_download(cx);
        shell
    });
    let (request_id, session) = view.read_with(cx, |shell, _| {
        let pending = shell.add_dialog.pending.as_ref().expect("add pending");
        (pending.request_id, pending.session.clone())
    });

    view.update_in(cx, |shell, window, cx| {
        shell.set_add_download_result(
            AddDownloadResultView {
                request_id,
                session,
                items: vec![AddDownloadItemResultView {
                    sources: vec![AddDownloadSourceView::Uri {
                        line: 1,
                        uri: "https://example.test/existing".into(),
                    }],
                    existing_task: Some(existing.clone()),
                    outcome: CommandOutcomeView::Failure(OperationErrorView {
                        code: "validation.duplicate_task".into(),
                        summary: "Already present".into(),
                        retryable: false,
                    }),
                }],
            },
            window,
            cx,
        );
    });
    view.read_with(cx, |shell, _| {
        assert!(!shell.add_dialog.open);
        assert_eq!(shell.selected.as_ref(), Some(&existing));
        assert_eq!(shell.selected_tasks, HashSet::from([existing.clone()]));
    });
}

#[gpui::test]
fn successful_retry_selects_the_new_task_identity(cx: &mut TestAppContext) {
    let (view, cx) = cx.add_window_view(|window, cx| {
        let mut shell = AppShell::new(Theme::dark(), window, cx);
        shell.snapshot = snapshot(1);
        shell.snapshot.tasks[0].status = TaskStatusView::Failed;
        shell.selected = Some(shell.snapshot.tasks[0].identity.clone());
        shell
    });
    let (request_id, session, old_identity) = view.update(cx, |shell, cx| {
        shell.begin_task_command(TaskCommandView::Retry, cx);
        let pending = shell
            .pending_task_command
            .as_ref()
            .expect("retry must become pending");
        (
            pending.request_id,
            pending.session.clone(),
            pending.identity.clone(),
        )
    });
    let new_identity = TaskIdentity {
        profile_id: old_identity.profile_id.clone(),
        gid: "0000000000000063".into(),
    };

    view.update_in(cx, |shell, window, cx| {
        shell.set_task_command_result(
            TaskCommandResultView {
                request_id,
                session,
                identity: old_identity,
                command: TaskCommandView::Retry,
                outcome: CommandOutcomeView::Success {
                    tasks: vec![new_identity.clone()],
                },
            },
            window,
            cx,
        );
    });
    view.read_with(cx, |shell, _| {
        assert_eq!(shell.selected.as_ref(), Some(&new_identity));
        assert!(shell.pending_task_command.is_none());
        assert!(shell.details_drawer.is_none());
    });
}

#[gpui::test]
fn output_name_dialog_accepts_only_non_terminal_direct_uri_tasks(cx: &mut TestAppContext) {
    let (view, cx) = cx.add_window_view(|window, cx| {
        let mut shell = AppShell::new(Theme::dark(), window, cx);
        shell.snapshot = snapshot(1);
        shell.selected = Some(shell.snapshot.tasks[0].identity.clone());
        shell
    });

    view.update_in(cx, |shell, window, cx| {
        shell.open_task_output_name(window, cx);
    });
    view.read_with(cx, |shell, _| {
        assert!(shell.output_name_dialog.is_none());
    });

    view.update_in(cx, |shell, window, cx| {
        shell.snapshot.tasks[0].status = TaskStatusView::Waiting;
        shell.snapshot.tasks[0].source_kind = TaskSourceKindView::Magnet;
        shell.open_task_output_name(window, cx);
    });
    view.read_with(cx, |shell, _| {
        assert!(shell.output_name_dialog.is_none());
    });

    view.update_in(cx, |shell, window, cx| {
        shell.snapshot.tasks[0].source_kind = TaskSourceKindView::DirectUri;
        shell.open_task_output_name(window, cx);
    });
    view.read_with(cx, |shell, _| {
        assert!(shell.output_name_dialog.is_some());
    });
}

#[gpui::test]
fn output_name_dialog_validates_and_submits_the_exact_filename(cx: &mut TestAppContext) {
    let (view, cx) = cx.add_window_view(|window, cx| {
        let mut shell = AppShell::new(Theme::dark(), window, cx);
        shell.snapshot = snapshot(1);
        shell.snapshot.tasks[0].status = TaskStatusView::Waiting;
        shell.selected = Some(shell.snapshot.tasks[0].identity.clone());
        shell.open_task_output_name(window, cx);
        shell
    });

    view.update(cx, |shell, cx| {
        shell.output_name_input.update(cx, |input, cx| {
            input.set_text("folder/archive.iso", cx);
        });
    });
    view.update(cx, |shell, cx| {
        shell.submit_task_output_name(cx);
    });
    view.read_with(cx, |shell, _| {
        assert!(shell.pending_task_command.is_none());
        assert!(
            shell
                .output_name_dialog
                .as_ref()
                .and_then(|dialog| dialog.error.as_ref())
                .is_some()
        );
    });

    view.update(cx, |shell, cx| {
        shell.output_name_input.update(cx, |input, cx| {
            input.set_text("  archive-renamed.iso  ", cx);
        });
    });
    view.update(cx, |shell, cx| {
        shell.submit_task_output_name(cx);
    });
    view.read_with(cx, |shell, _| {
        assert!(matches!(
            shell
                .pending_task_command
                .as_ref()
                .map(|pending| &pending.command),
            Some(TaskCommandView::SetOutputName { output_name })
                if output_name == "archive-renamed.iso"
        ));
    });
}

#[gpui::test]
fn output_name_result_closes_on_success_and_stays_open_on_failure(cx: &mut TestAppContext) {
    let (view, cx) = cx.add_window_view(|window, cx| {
        let mut shell = AppShell::new(Theme::dark(), window, cx);
        shell.snapshot = snapshot(1);
        shell.snapshot.tasks[0].status = TaskStatusView::Waiting;
        shell.selected = Some(shell.snapshot.tasks[0].identity.clone());
        shell.open_task_output_name(window, cx);
        shell
    });

    view.update(cx, |shell, cx| {
        shell.output_name_input.update(cx, |input, cx| {
            input.set_text("first.iso", cx);
        });
    });
    let first = view.update(cx, |shell, cx| {
        shell.submit_task_output_name(cx);
        let pending = shell
            .pending_task_command
            .as_ref()
            .expect("pending command");
        TaskCommandResultView {
            request_id: pending.request_id,
            session: pending.session.clone(),
            identity: pending.identity.clone(),
            command: pending.command.clone(),
            outcome: CommandOutcomeView::Failure(OperationErrorView {
                code: "rpc.command_rejected".into(),
                summary: "aria2 rejected the output name".into(),
                retryable: false,
            }),
        }
    });
    view.update_in(cx, |shell, window, cx| {
        shell.set_task_command_result(first, window, cx);
    });
    view.read_with(cx, |shell, _| {
        assert!(shell.pending_task_command.is_none());
        assert!(
            shell
                .output_name_dialog
                .as_ref()
                .and_then(|dialog| dialog.error.as_ref())
                .is_some()
        );
    });

    view.update(cx, |shell, cx| {
        shell.output_name_input.update(cx, |input, cx| {
            input.set_text("second.iso", cx);
        });
    });
    let second = view.update(cx, |shell, cx| {
        shell.submit_task_output_name(cx);
        let pending = shell
            .pending_task_command
            .as_ref()
            .expect("pending command");
        TaskCommandResultView {
            request_id: pending.request_id,
            session: pending.session.clone(),
            identity: pending.identity.clone(),
            command: pending.command.clone(),
            outcome: CommandOutcomeView::Success { tasks: Vec::new() },
        }
    });
    view.update_in(cx, |shell, window, cx| {
        shell.set_task_command_result(second, window, cx);
    });
    view.read_with(cx, |shell, _| {
        assert!(shell.pending_task_command.is_none());
        assert!(shell.output_name_dialog.is_none());
    });
}

#[gpui::test]
fn theme_applies_only_after_the_matching_save_succeeds(cx: &mut TestAppContext) {
    let initial = SettingsView {
        color_scheme: ColorSchemeView::Dark,
        download_directory: "C:/Downloads".into(),
        ..SettingsView::default()
    };
    let expected_initial = initial.clone();
    let (view, cx) =
        cx.add_window_view(move |window, cx| AppShell::new_with_settings(initial, window, cx));
    let (request_id, requested) = view.update_in(cx, |shell, window, cx| {
        shell.page = AppPage::Settings;
        shell.select_color_scheme(ColorSchemeView::Light, window, cx);
        let pending = shell
            .pending_settings_save
            .as_ref()
            .expect("settings save must become pending");
        (pending.request_id, pending.settings.clone())
    });

    view.update_in(cx, |shell, window, cx| {
        shell.set_settings_save_result(
            SettingsSaveResultView {
                request_id: RequestId::from_u64(request_id.get() + 1),
                settings: requested.clone(),
                outcome: SettingsSaveOutcomeView::Success,
            },
            window,
            cx,
        );
    });
    view.read_with(cx, |shell, _| {
        assert_eq!(shell.settings, expected_initial);
        assert!(shell.pending_settings_save.is_some());
    });

    view.update_in(cx, |shell, window, cx| {
        shell.set_settings_save_result(
            SettingsSaveResultView {
                request_id,
                settings: requested.clone(),
                outcome: SettingsSaveOutcomeView::Success,
            },
            window,
            cx,
        );
    });
    view.read_with(cx, |shell, _| {
        assert_eq!(shell.settings, requested);
        assert_eq!(shell.theme.mode, ThemeMode::Light);
        assert!(shell.pending_settings_save.is_none());
        assert_eq!(shell.page, AppPage::Settings);
    });
}

#[gpui::test]
fn proxy_settings_build_a_manual_draft_with_a_masked_password(cx: &mut TestAppContext) {
    let initial = SettingsView {
        color_scheme: ColorSchemeView::Dark,
        download_directory: "C:/Downloads".into(),
        download_proxy: DownloadProxySettingsView {
            mode: ProxyModeView::Disabled,
            ..DownloadProxySettingsView::default()
        },
        speed_limits: SpeedLimitSettingsView::default(),
        transfer_policy: TransferPolicySettingsView::default(),
        notifications: NotificationSettingsView::default(),
        platform: PlatformSettingsView::default(),
    };
    let (view, cx) =
        cx.add_window_view(move |window, cx| AppShell::new_with_settings(initial, window, cx));

    view.update_in(cx, |shell, window, cx| {
        shell.open_settings(&OpenSettings, window, cx);
        shell.select_proxy_mode(ProxyModeView::Manual, cx);
        shell.settings_inputs.all_proxy.update(cx, |input, cx| {
            input.set_text("proxy.example:8080", cx);
        });
        shell
            .settings_inputs
            .proxy_username
            .update(cx, |input, cx| input.set_text("proxy-user", cx));
        shell
            .settings_inputs
            .proxy_password
            .update(cx, |input, cx| input.set_text("never-render-this", cx));
        shell.submit_proxy_settings(cx);
    });

    view.read_with(cx, |shell, cx| {
        assert!(shell.settings_inputs.proxy_password.read(cx).is_secure());
        let pending = shell
            .pending_settings_save
            .as_ref()
            .expect("proxy settings save must become pending");
        assert_eq!(pending.source, SettingsSaveSource::Proxy);
        assert_eq!(pending.settings.download_proxy.mode, ProxyModeView::Manual);
        assert_eq!(
            pending.settings.download_proxy.all_proxy,
            "proxy.example:8080"
        );
        assert_eq!(pending.settings.download_proxy.username, "proxy-user");
        assert!(pending.settings.download_proxy.has_password);
        assert_eq!(pending.settings.download_directory, "C:/Downloads");
    });
}

#[gpui::test]
fn stale_details_result_cannot_replace_the_active_request(cx: &mut TestAppContext) {
    let (view, cx) = cx.add_window_view(|window, cx| {
        let mut shell = AppShell::new(Theme::dark(), window, cx);
        shell.snapshot = snapshot(1);
        shell.selected = Some(task(0).identity);
        shell.open_details_for(task(0), cx);
        shell
    });
    let (request_id, session, identity) = view.read_with(cx, |shell, _| {
        let drawer = shell.details_drawer.as_ref().expect("drawer must exist");
        assert!(matches!(drawer.state, TaskDetailsLoadState::Loading));
        (
            drawer
                .pending
                .as_ref()
                .expect("details request must be pending")
                .request_id,
            drawer.session.clone(),
            drawer.identity.clone(),
        )
    });

    view.update(cx, |shell, cx| {
        shell.set_task_details_result(
            TaskDetailsResultView {
                request_id: RequestId::from_u64(request_id.get() + 1),
                session: session.clone(),
                identity: identity.clone(),
                outcome: TaskDetailsOutcomeView::Ready(Box::new(details(1))),
            },
            cx,
        );
    });
    view.read_with(cx, |shell, _| {
        let drawer = shell.details_drawer.as_ref().expect("drawer must exist");
        assert!(matches!(drawer.state, TaskDetailsLoadState::Loading));
        assert_eq!(
            drawer.pending.as_ref().map(|pending| pending.request_id),
            Some(request_id)
        );
    });

    view.update(cx, |shell, cx| {
        shell.set_task_details_result(
            TaskDetailsResultView {
                request_id,
                session,
                identity,
                outcome: TaskDetailsOutcomeView::Ready(Box::new(details(1))),
            },
            cx,
        );
    });
    view.read_with(cx, |shell, _| {
        let drawer = shell.details_drawer.as_ref().expect("drawer must exist");
        assert!(matches!(drawer.state, TaskDetailsLoadState::Ready { .. }));
        assert!(drawer.pending.is_none());
    });
}

#[gpui::test]
fn task_revision_refreshes_visible_file_details_without_loading_flicker(cx: &mut TestAppContext) {
    let (view, cx) = cx.add_window_view(|window, cx| {
        let mut shell = AppShell::new(Theme::dark(), window, cx);
        shell.snapshot = snapshot(1);
        shell.open_details_for(task(0), cx);
        shell
    });
    let (initial_request, session, identity) = view.read_with(cx, |shell, _| {
        let drawer = shell.details_drawer.as_ref().expect("drawer must exist");
        (
            drawer.pending.as_ref().expect("initial request").request_id,
            drawer.session.clone(),
            drawer.identity.clone(),
        )
    });
    let mut first_details = details(1);
    first_details.files[0].completed_length = 100;
    view.update(cx, |shell, cx| {
        shell.set_task_details_result(
            TaskDetailsResultView {
                request_id: initial_request,
                session: session.clone(),
                identity: identity.clone(),
                outcome: TaskDetailsOutcomeView::Ready(Box::new(first_details)),
            },
            cx,
        );
    });

    view.update(cx, |shell, cx| {
        let mut revision_two = snapshot(1);
        revision_two.tasks[0].revision = 2;
        shell.set_snapshot(revision_two, cx);
    });
    let refresh_request = view.read_with(cx, |shell, _| {
        let drawer = shell.details_drawer.as_ref().expect("drawer must exist");
        let TaskDetailsLoadState::Ready { details } = &drawer.state else {
            panic!("existing details must remain visible while refreshing")
        };
        assert_eq!(details.files[0].completed_length, 100);
        let pending = drawer.pending.as_ref().expect("refresh request");
        assert_eq!(pending.source_revision, 2);
        pending.request_id
    });

    view.update(cx, |shell, cx| {
        let mut revision_three = snapshot(1);
        revision_three.tasks[0].revision = 3;
        shell.set_snapshot(revision_three, cx);
    });
    view.read_with(cx, |shell, _| {
        assert_eq!(
            shell
                .details_drawer
                .as_ref()
                .and_then(|drawer| drawer.pending.as_ref())
                .map(|pending| pending.request_id),
            Some(refresh_request),
            "a second refresh must not be started while one is pending"
        );
    });

    let mut second_details = details(1);
    second_details.files[0].completed_length = 200;
    view.update(cx, |shell, cx| {
        shell.set_task_details_result(
            TaskDetailsResultView {
                request_id: refresh_request,
                session: session.clone(),
                identity: identity.clone(),
                outcome: TaskDetailsOutcomeView::Ready(Box::new(second_details)),
            },
            cx,
        );
    });
    let catch_up_request = view.read_with(cx, |shell, _| {
        let drawer = shell.details_drawer.as_ref().expect("drawer must exist");
        let TaskDetailsLoadState::Ready { details } = &drawer.state else {
            panic!("refreshed details must stay visible")
        };
        assert_eq!(details.files[0].completed_length, 200);
        let pending = drawer.pending.as_ref().expect("catch-up request");
        assert_eq!(pending.source_revision, 3);
        assert_ne!(pending.request_id, refresh_request);
        pending.request_id
    });

    let mut stale_details = details(1);
    stale_details.files[0].completed_length = 50;
    view.update(cx, |shell, cx| {
        shell.set_task_details_result(
            TaskDetailsResultView {
                request_id: refresh_request,
                session: session.clone(),
                identity: identity.clone(),
                outcome: TaskDetailsOutcomeView::Ready(Box::new(stale_details)),
            },
            cx,
        );
    });
    view.read_with(cx, |shell, _| {
        let drawer = shell.details_drawer.as_ref().expect("drawer must exist");
        let TaskDetailsLoadState::Ready { details } = &drawer.state else {
            panic!("details must remain ready")
        };
        assert_eq!(details.files[0].completed_length, 200);
        assert_eq!(
            drawer.pending.as_ref().map(|pending| pending.request_id),
            Some(catch_up_request)
        );
    });
}

#[gpui::test]
fn detail_requests_are_task_scoped_and_clear_active_only_network_data(cx: &mut TestAppContext) {
    let (view, cx) = cx.add_window_view(|window, cx| {
        let mut shell = AppShell::new(Theme::dark(), window, cx);
        let mut initial = snapshot(1);
        initial.tasks[0].status = TaskStatusView::Seeding;
        initial.tasks[0].source_kind = TaskSourceKindView::Unknown;
        shell.snapshot = initial;
        shell
    });
    let events = Arc::new(std::sync::Mutex::new(Vec::new()));
    let sink = events.clone();
    let _subscription = view.update(cx, |_, cx| {
        cx.subscribe(&view, move |_, _, event: &AppShellEvent, _| {
            if let AppShellEvent::TaskDetailsRequested(request) = event {
                sink.lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .push(request.clone());
            }
        })
    });

    view.update(cx, |shell, cx| {
        shell.open_details_for(shell.snapshot.tasks[0].clone(), cx);
    });
    let first = events
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())[0]
        .clone();
    assert!(first.active);
    assert!(first.is_bittorrent);

    let mut loaded = details(1);
    loaded.servers.push(TaskServerView {
        file_index: 1,
        uri: "https://origin.example/file".into(),
        current_uri: "https://cdn.example/file".into(),
        download_rate: 1_024,
    });
    loaded.peers.push(TaskPeerView {
        address: "192.0.2.1".into(),
        port: 6_881,
        download_rate: 2_048,
        upload_rate: 512,
        seeder: true,
    });
    view.update(cx, |shell, cx| {
        shell.set_task_details_result(
            TaskDetailsResultView {
                request_id: first.request_id,
                session: first.session.clone(),
                identity: first.identity.clone(),
                outcome: TaskDetailsOutcomeView::Ready(Box::new(loaded)),
            },
            cx,
        );
    });

    view.update(cx, |shell, cx| {
        let mut completed = snapshot(1);
        completed.tasks[0].status = TaskStatusView::Complete;
        completed.tasks[0].source_kind = TaskSourceKindView::BitTorrent;
        completed.tasks[0].revision = 2;
        shell.set_snapshot(completed, cx);
    });
    view.read_with(cx, |shell, _| {
        let drawer = shell.details_drawer.as_ref().expect("drawer remains open");
        let TaskDetailsLoadState::Ready { details } = &drawer.state else {
            panic!("background refresh must keep details visible")
        };
        assert!(details.peers.is_empty());
        assert!(details.servers.is_empty());
    });
    let requests = events
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    assert_eq!(requests.len(), 2);
    assert!(!requests[1].active);
    assert!(requests[1].is_bittorrent);
}

#[gpui::test]
fn details_drawer_survives_filtering_that_hides_its_task(cx: &mut TestAppContext) {
    let (view, cx) = cx.add_window_view(|window, cx| {
        let mut shell = AppShell::new(Theme::dark(), window, cx);
        shell.snapshot = snapshot(3);
        shell.selected = Some(task(1).identity);
        shell.open_details_for(task(1), cx);
        shell
    });
    let selected = view.read_with(cx, |shell, _| shell.selected.clone());

    view.update(cx, |shell, cx| {
        let mut filtered = snapshot(1);
        filtered.tasks[0] = task(2);
        shell.set_snapshot(filtered, cx);
    });
    view.read_with(cx, |shell, _| {
        assert_eq!(shell.selected, selected);
        assert_eq!(
            shell.details_drawer.as_ref().map(|drawer| &drawer.identity),
            selected.as_ref()
        );
    });
}

#[gpui::test]
fn ten_thousand_detail_files_render_only_a_viewport_window(cx: &mut TestAppContext) {
    let (view, cx) = cx.add_window_view(|window, cx| {
        let mut shell = AppShell::new(Theme::dark(), window, cx);
        shell.snapshot = snapshot(1);
        let overview = task(0);
        shell.selected = Some(overview.identity.clone());
        shell.details_drawer = Some(TaskDetailsDrawer {
            identity: overview.identity.clone(),
            overview,
            session: shell.snapshot.engine_session().expect("test session"),
            state: TaskDetailsLoadState::Ready {
                details: Box::new(details(10_000)),
            },
            pending: None,
            open_pending: None,
            tab: TaskDetailsTab::Files,
            file_scroll: UniformListScrollHandle::new(),
            rendered_file_range: 0..0,
        });
        shell
    });

    view.read_with(cx, |shell, _| {
        let drawer = shell.details_drawer.as_ref().expect("drawer must exist");
        assert!(!drawer.rendered_file_range.is_empty());
        assert!(
            drawer.rendered_file_range.len() < 64,
            "rendered {} files",
            drawer.rendered_file_range.len()
        );
        let TaskDetailsLoadState::Ready { details } = &drawer.state else {
            panic!("drawer must be ready")
        };
        assert_eq!(details.files.len(), 10_000);
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
        assert!(shell.remove_confirmation.is_some());
        assert!(
            !shell
                .remove_confirmation
                .as_ref()
                .is_some_and(|value| value.delete_files)
        );
        assert!(shell.pending_task_command.is_none());
    });
    view.update(cx, |shell, cx| shell.submit_remove_confirmation(cx));
    view.read_with(cx, |shell, _| {
        assert!(shell.remove_confirmation.is_none());
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
                .remove_confirmation
                .as_ref()
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
        assert!(shell.add_dialog.open);
        assert_eq!(shell.selected, selected);
    });
}

#[gpui::test]
fn escape_priority_closes_popover_then_settings_then_search(cx: &mut TestAppContext) {
    let (view, cx) = cx.add_window_view(|window, cx| {
        let mut shell = AppShell::new(Theme::dark(), window, cx);
        shell.page = AppPage::Settings;
        shell.speed_popover_open = true;
        shell.search_input.update(cx, |input, cx| {
            input.set_text("archive", cx);
        });
        shell
    });

    view.update_in(cx, |shell, window, cx| {
        shell.clear_search(&ClearSearch, window, cx);
    });
    view.read_with(cx, |shell, cx| {
        assert!(!shell.speed_popover_open);
        assert_eq!(shell.page, AppPage::Settings);
        assert_eq!(shell.search_input.read(cx).text(), "archive");
    });

    view.update_in(cx, |shell, window, cx| {
        shell.clear_search(&ClearSearch, window, cx);
    });
    view.read_with(cx, |shell, cx| {
        assert_eq!(shell.page, AppPage::Downloads);
        assert_eq!(shell.search_input.read(cx).text(), "archive");
    });

    view.update_in(cx, |shell, window, cx| {
        shell.clear_search(&ClearSearch, window, cx);
    });
    view.read_with(cx, |shell, cx| {
        assert!(shell.search_input.read(cx).text().is_empty());
    });
}

#[gpui::test]
fn failed_directory_save_keeps_the_draft(cx: &mut TestAppContext) {
    let initial = SettingsView {
        color_scheme: ColorSchemeView::Dark,
        download_directory: "C:/Downloads".into(),
        ..SettingsView::default()
    };
    let (view, cx) =
        cx.add_window_view(move |window, cx| AppShell::new_with_settings(initial, window, cx));
    let (request_id, requested) = view.update(cx, |shell, cx| {
        shell.page = AppPage::Settings;
        shell.settings_inputs.directory.update(cx, |input, cx| {
            input.set_text("D:/Transfers", cx);
        });
        shell.submit_settings(cx);
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
        assert_eq!(
            shell.settings_inputs.directory.read(cx).text(),
            "D:/Transfers"
        );
        assert_eq!(shell.page, AppPage::Settings);
        assert!(shell.settings_page.error.is_some());
    });
}

#[gpui::test]
fn global_speed_limit_save_emits_parsed_request_and_normalizes_on_success(cx: &mut TestAppContext) {
    let (view, cx) = cx.add_window_view(|window, cx| AppShell::new(Theme::dark(), window, cx));
    let events = Arc::new(std::sync::Mutex::new(Vec::new()));
    let sink = events.clone();
    let _subscription = view.update(cx, |_, cx| {
        cx.subscribe(&view, move |_, _, event: &AppShellEvent, _| {
            if let AppShellEvent::SettingsSaveRequested(request) = event {
                sink.lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .push(request.clone());
            }
        })
    });

    // open_settings hydrates transfer-policy fields so only the speed draft is dirty.
    view.update_in(cx, |shell, window, cx| {
        shell.open_settings(&OpenSettings, window, cx);
    });
    view.update(cx, |shell, cx| {
        // "2M" and blank (unlimited) both go through the K/M parser.
        shell
            .settings_inputs
            .download_limit
            .update(cx, |input, cx| {
                input.set_text("2M", cx);
            });
    });
    let request_id = view.update(cx, |shell, cx| {
        shell.submit_transfers(cx);
        shell
            .pending_settings_save
            .as_ref()
            .expect("speed-limit save must become pending")
            .request_id
    });

    let request = events
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .last()
        .cloned()
        .expect("a settings-save event should have been emitted");
    // The view carries the raw editable text; byte parsing happens in the
    // desktop mapping layer, not here.
    assert_eq!(request.settings.speed_limits.download_limit, "2M");
    assert!(request.settings.speed_limits.upload_limit.is_empty());

    // The desktop persists normalized bytes and echoes back the compact form.
    let mut normalized = request.settings.clone();
    normalized.speed_limits.download_limit = crate::format_speed_limit_field(2 * 1024 * 1024);
    normalized.speed_limits.upload_limit = crate::format_speed_limit_field(0);
    view.update_in(cx, |shell, window, cx| {
        shell.set_settings_save_result(
            SettingsSaveResultView {
                request_id,
                settings: normalized,
                outcome: SettingsSaveOutcomeView::Success,
            },
            window,
            cx,
        );
    });
    view.read_with(cx, |shell, cx| {
        assert!(shell.pending_settings_save.is_none());
        assert_eq!(shell.settings.speed_limits.download_limit, "2M");
        assert_eq!(shell.settings_inputs.download_limit.read(cx).text(), "2M");
        assert!(
            shell
                .settings_inputs
                .upload_limit
                .read(cx)
                .text()
                .is_empty()
        );
    });
}

#[gpui::test]
fn invalid_global_speed_limit_is_rejected_before_a_save_request(cx: &mut TestAppContext) {
    let (view, cx) = cx.add_window_view(|window, cx| AppShell::new(Theme::dark(), window, cx));
    // Set the text in its own cycle so the field's change event (which
    // dismisses stale errors) is flushed before the submit runs, matching
    // the real "type, then click Save" order.
    view.update_in(cx, |shell, window, cx| {
        shell.open_settings(&OpenSettings, window, cx);
    });
    view.update(cx, |shell, cx| {
        shell
            .settings_inputs
            .download_limit
            .update(cx, |input, cx| {
                input.set_text("5MB", cx);
            });
    });
    view.update(cx, |shell, cx| {
        shell.submit_transfers(cx);
    });
    view.read_with(cx, |shell, _cx| {
        assert!(shell.pending_settings_save.is_none());
        let error = shell
            .settings_page
            .error
            .as_ref()
            .expect("invalid speed limit must surface an error");
        assert_eq!(error.code, "settings.invalid_speed_limit");
    });
}

#[gpui::test]
fn transfers_footer_saves_speed_limit_and_policy_together(cx: &mut TestAppContext) {
    let (view, cx) = cx.add_window_view(|window, cx| AppShell::new(Theme::dark(), window, cx));
    let events = Arc::new(std::sync::Mutex::new(Vec::new()));
    let sink = events.clone();
    let _subscription = view.update(cx, |_, cx| {
        cx.subscribe(&view, move |_, _, event: &AppShellEvent, _| {
            if let AppShellEvent::SettingsSaveRequested(request) = event {
                sink.lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .push(request.clone());
            }
        })
    });

    // open_settings hydrates draft fields from SettingsView defaults.
    view.update_in(cx, |shell, window, cx| {
        shell.open_settings(&OpenSettings, window, cx);
    });
    view.update(cx, |shell, cx| {
        shell
            .settings_inputs
            .download_limit
            .update(cx, |input, cx| {
                input.set_text("2M", cx);
            });
        shell
            .settings_inputs
            .max_concurrent
            .update(cx, |input, cx| {
                input.set_text("8", cx);
            });
    });
    view.update(cx, |shell, cx| {
        shell.submit_transfers(cx);
    });

    view.read_with(cx, |shell, _cx| {
        let pending = shell
            .pending_settings_save
            .as_ref()
            .expect("combined transfers save must become pending");
        assert_eq!(pending.source, SettingsSaveSource::Transfers);
        assert_eq!(pending.settings.speed_limits.download_limit, "2M");
        assert_eq!(
            pending.settings.transfer_policy.max_concurrent_downloads,
            "8"
        );
    });
    assert_eq!(
        events
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .len(),
        1,
        "exactly one settings-save event for combined transfers"
    );
}

#[gpui::test]
fn transfers_footer_rejects_invalid_speed_even_when_policy_is_dirty(cx: &mut TestAppContext) {
    let (view, cx) = cx.add_window_view(|window, cx| AppShell::new(Theme::dark(), window, cx));
    view.update_in(cx, |shell, window, cx| {
        shell.open_settings(&OpenSettings, window, cx);
    });
    view.update(cx, |shell, cx| {
        shell
            .settings_inputs
            .download_limit
            .update(cx, |input, cx| {
                input.set_text("5MB", cx);
            });
        shell
            .settings_inputs
            .max_concurrent
            .update(cx, |input, cx| {
                input.set_text("8", cx);
            });
    });
    view.update(cx, |shell, cx| {
        shell.submit_transfers(cx);
    });
    view.read_with(cx, |shell, _cx| {
        assert!(shell.pending_settings_save.is_none());
        let error = shell
            .settings_page
            .error
            .as_ref()
            .expect("invalid speed limit must surface an error");
        assert_eq!(error.code, "settings.invalid_speed_limit");
    });
}

#[gpui::test]
fn speed_popover_toggles_and_restores_previous_focus(cx: &mut TestAppContext) {
    let (view, cx) = cx.add_window_view(|window, cx| {
        let shell = AppShell::new(Theme::dark(), window, cx);
        window.focus(&shell.focus_handle, cx);
        shell
    });

    view.update_in(cx, |shell, window, cx| {
        shell.toggle_speed_popover(window, cx);
        assert!(shell.speed_popover_open);
        shell.close_speed_popover(window, cx);
    });
    view.read_with(cx, |shell, _| {
        assert!(!shell.speed_popover_open);
        assert!(shell.speed_popover_previous_focus.is_none());
    });
}

#[gpui::test]
#[gpui::test]
fn task_status_transitions_group_completions_into_one_notice(cx: &mut TestAppContext) {
    let (view, cx) = cx.add_window_view(|window, cx| {
        let mut shell = AppShell::new(Theme::dark(), window, cx);
        let mut initial = snapshot(3);
        for task in &mut initial.tasks {
            task.status = TaskStatusView::Active;
        }
        // Seed known statuses without treating the first snapshot as transitions.
        shell.set_snapshot(initial, cx);
        shell
    });

    view.update(cx, |shell, cx| {
        let mut next = shell.snapshot.clone();
        next.tasks[0].status = TaskStatusView::Complete;
        next.tasks[1].status = TaskStatusView::Complete;
        next.tasks[2].status = TaskStatusView::Failed;
        next.tasks[2].error = Some(crate::TaskErrorView {
            code: Some(1),
            summary: "Network failed".into(),
            details: None,
        });
        shell.set_snapshot(next, cx);
        assert_eq!(
            shell.activity_log.len(),
            2,
            "one completion group + one failure"
        );
        assert_eq!(shell.activity_log[0].kind, ActivityKindView::Error);
        assert_eq!(shell.activity_log[0].count, 1);
        assert_eq!(shell.activity_log[1].kind, ActivityKindView::Completion);
        assert_eq!(shell.activity_log[1].count, 2);
        assert!(
            shell
                .status_notice
                .as_ref()
                .is_some_and(|notice| notice.is_error && notice.message.contains("failed")),
            "latest automatic notice should be the failure group"
        );
    });
}

#[gpui::test]
fn quiet_volume_suppresses_automatic_toasts_but_keeps_history(cx: &mut TestAppContext) {
    let (view, cx) = cx.add_window_view(|window, cx| {
        let mut shell = AppShell::new(Theme::dark(), window, cx);
        shell.settings.notifications.volume = NotificationVolumeView::Quiet;
        let mut initial = snapshot(1);
        initial.tasks[0].status = TaskStatusView::Active;
        shell.set_snapshot(initial, cx);
        shell
    });

    view.update(cx, |shell, cx| {
        let mut next = shell.snapshot.clone();
        next.tasks[0].status = TaskStatusView::Complete;
        shell.set_snapshot(next, cx);
        assert_eq!(shell.activity_log.len(), 1);
        assert!(
            shell.status_notice.is_none(),
            "Quiet must hide automatic completion toasts"
        );
        // Command feedback still surfaces in Quiet.
        shell.show_notice("Copied.", false, cx);
        assert!(shell.status_notice.is_some());
    });
}

#[gpui::test]
fn silent_volume_suppresses_all_toasts(cx: &mut TestAppContext) {
    let (view, cx) = cx.add_window_view(|window, cx| {
        let mut shell = AppShell::new(Theme::dark(), window, cx);
        shell.settings.notifications.volume = NotificationVolumeView::Silent;
        shell
    });

    view.update(cx, |shell, cx| {
        shell.show_notice("Command feedback.", false, cx);
        assert!(shell.status_notice.is_none());
        shell.record_activity(ActivityKindView::Info, "Still recorded", None, None, 1, cx);
        assert_eq!(shell.activity_log.len(), 1);
    });
}

#[gpui::test]
fn notification_preferences_save_emits_settings_request(cx: &mut TestAppContext) {
    let (view, cx) = cx.add_window_view(|window, cx| AppShell::new(Theme::dark(), window, cx));
    view.update_in(cx, |shell, window, cx| {
        shell.open_settings(&OpenSettings, window, cx);
        shell.select_notification_volume(NotificationVolumeView::Quiet, cx);
        shell.toggle_notify_on_completion(cx);
        shell.submit_notifications(cx);
        let pending = shell
            .pending_settings_save
            .as_ref()
            .expect("notification save pending");
        assert_eq!(pending.source, SettingsSaveSource::Notifications);
        assert_eq!(
            pending.settings.notifications.volume,
            NotificationVolumeView::Quiet
        );
        assert!(!pending.settings.notifications.notify_on_completion);
    });
}

#[gpui::test]
fn system_theme_selection_emits_settings_save(cx: &mut TestAppContext) {
    let initial = SettingsView {
        color_scheme: ColorSchemeView::Dark,
        download_directory: "C:/Downloads".into(),
        ..SettingsView::default()
    };
    let (view, cx) =
        cx.add_window_view(move |window, cx| AppShell::new_with_settings(initial, window, cx));
    view.update_in(cx, |shell, window, cx| {
        shell.page = AppPage::Settings;
        shell.select_color_scheme(ColorSchemeView::System, window, cx);
        let pending = shell
            .pending_settings_save
            .as_ref()
            .expect("system theme save pending");
        assert_eq!(pending.source, SettingsSaveSource::Theme);
        assert_eq!(pending.settings.color_scheme, ColorSchemeView::System);
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
        shell.select_close_behavior(CloseBehaviorView::Quit, cx);
        shell.toggle_start_minimized_to_tray(cx);
        shell.submit_platform(cx);
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

        // Apply saved platform prefs and verify close interception.
        shell.apply_settings(pending.settings.clone(), cx);
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
fn activity_panel_toggles_and_clears(cx: &mut TestAppContext) {
    let (view, cx) = cx.add_window_view(|window, cx| AppShell::new(Theme::dark(), window, cx));
    view.update_in(cx, |shell, window, cx| {
        shell.record_activity(
            ActivityKindView::Completion,
            "One finished.",
            None,
            None,
            1,
            cx,
        );
        shell.toggle_activity_panel(window, cx);
        assert!(shell.activity_panel_open);
        shell.clear_activity_log(cx);
        assert!(shell.activity_log.is_empty());
        shell.close_activity_panel(window, cx);
        assert!(!shell.activity_panel_open);
    });
}

#[gpui::test]
fn force_pause_is_blocked_when_capabilities_omit_the_method(cx: &mut TestAppContext) {
    let (view, cx) = cx.add_window_view(|window, cx| {
        let mut shell = AppShell::new(Theme::dark(), window, cx);
        shell.snapshot = snapshot(1);
        shell.snapshot.tasks[0].status = TaskStatusView::Active;
        shell.selected = Some(task(0).identity);
        shell.selected_tasks = std::collections::HashSet::from([task(0).identity]);
        // Probed methods without forcePause: UI must explain and not submit.
        shell.snapshot.capabilities = crate::EngineCapabilitiesView {
            version: "1.37.0".into(),
            methods_probed: true,
            force_pause: false,
            force_pause_all: false,
            force_remove: false,
            queue_positioning: true,
            change_option: true,
            change_global_option: true,
            get_peers: true,
            get_servers: true,
            multicall: true,
        };
        shell
    });

    view.update(cx, |shell, cx| {
        shell.begin_task_command(TaskCommandView::ForcePause, cx);
        assert!(shell.pending_task_command.is_none());
        let notice = shell.status_notice.as_ref().expect("capability notice");
        assert!(notice.is_error);
        assert!(
            notice.message.contains("force-pause") || notice.message.contains("forcePause"),
            "{}",
            notice.message
        );
    });
}

#[gpui::test]
fn profile_catalog_can_switch_and_add_drafts(cx: &mut TestAppContext) {
    let (view, cx) = cx.add_window_view(|window, cx| {
        let mut shell = AppShell::new(Theme::dark(), window, cx);
        shell.set_profiles(
            ProfileCatalogView {
                active_profile_id: "p1".into(),
                profiles: vec![
                    ProfileEntryView {
                        profile_id: "p1".into(),
                        name: "Local".into(),
                        kind: ProfileKindView::LocalManaged,
                        executable: "aria2c".into(),
                        download_dir: "D:/Downloads".into(),
                        endpoint: String::new(),
                        has_secret: false,
                    },
                    ProfileEntryView {
                        profile_id: "p2".into(),
                        name: "NAS".into(),
                        kind: ProfileKindView::RemoteRpc,
                        executable: String::new(),
                        download_dir: "D:/Downloads".into(),
                        endpoint: "wss://nas.example/jsonrpc".into(),
                        has_secret: false,
                    },
                ],
            },
            cx,
        );
        shell
    });

    view.update(cx, |shell, cx| {
        shell.request_switch_profile("p1".into(), cx);
        assert!(
            shell
                .status_notice
                .as_ref()
                .is_some_and(|notice| notice.message.contains("already active"))
        );
        shell.add_draft_local_profile(cx);
        assert_eq!(shell.profiles.profiles.len(), 3);
        let draft = shell
            .profiles
            .profiles
            .iter()
            .find(|profile| profile.profile_id.starts_with("draft-local-"))
            .expect("local draft");
        assert!(draft.executable.is_empty(), "local draft uses managed core");
        assert_eq!(
            shell.settings_page.editing_profile_id.as_deref(),
            Some(draft.profile_id.as_str())
        );
        shell.settings_inputs.profile_name.update(cx, |input, cx| {
            input.set_text("Home NAS-ready", cx);
        });
        shell.apply_profile_editor(cx);
        assert!(shell.settings_page.editing_profile_id.is_none());
        assert!(
            shell
                .profiles
                .profiles
                .iter()
                .any(|profile| profile.name == "Home NAS-ready")
        );

        shell.add_draft_remote_profile(cx);
        assert_eq!(shell.profiles.profiles.len(), 4);
        assert!(
            shell
                .profiles
                .profiles
                .iter()
                .any(|profile| profile.kind == ProfileKindView::RemoteRpc
                    && profile.profile_id.starts_with("draft-remote-"))
        );

        // Delete requires confirmation, then persists via save request.
        let remove_id = shell.profiles.profiles[3].profile_id.clone();
        shell.request_remove_profile(remove_id.clone(), cx);
        assert!(shell.settings_page.pending_profile_delete.is_some());
        shell.cancel_remove_profile(cx);
        assert!(shell.settings_page.pending_profile_delete.is_none());
        assert_eq!(shell.profiles.profiles.len(), 4);
        shell.request_remove_profile(remove_id, cx);
        shell.confirm_remove_profile(cx);
        assert_eq!(shell.profiles.profiles.len(), 3);

        // Remote secret set is staged until Save profiles.
        shell.add_draft_remote_profile(cx);
        let remote_id = shell
            .profiles
            .profiles
            .iter()
            .find(|profile| profile.profile_id.starts_with("draft-remote-"))
            .map(|profile| profile.profile_id.clone())
            .expect("remote draft");
        shell.open_profile_editor(remote_id.clone(), cx);
        shell
            .settings_inputs
            .profile_secret
            .update(cx, |input, cx| {
                input.set_text("s3cret", cx);
            });
        shell.apply_profile_editor(cx);
        assert!(
            shell
                .settings_page
                .profile_secret_updates
                .get(&remote_id)
                .is_some_and(|update| matches!(update, crate::ProfileRpcSecretUpdateView::Set(_)))
        );
        assert!(
            shell
                .profiles
                .profiles
                .iter()
                .find(|profile| profile.profile_id == remote_id)
                .is_some_and(|profile| profile.has_secret)
        );

        shell.set_switch_profile_result(
            SwitchProfileResultView {
                request_id: RequestId::from_u64(9),
                profile_id: "p2".into(),
                catalog: ProfileCatalogView {
                    active_profile_id: "p2".into(),
                    profiles: shell.profiles.profiles.clone(),
                },
                outcome: SwitchProfileOutcomeView::Success,
            },
            cx,
        );
        assert_eq!(shell.profiles.active_profile_id, "p2");
        assert_eq!(
            shell.profiles.active().map(|profile| profile.name.as_str()),
            Some("NAS")
        );
    });
}

#[gpui::test]
fn core_registry_commands_emit_and_apply_results(cx: &mut TestAppContext) {
    let (view, cx) = cx.add_window_view(|window, cx| {
        let mut shell = AppShell::new(Theme::dark(), window, cx);
        shell.set_cores(
            CoreRegistryView {
                active_id: Some("c1".into()),
                last_working_id: Some("c1".into()),
                installations: vec![CoreInstallationView {
                    id: "c1".into(),
                    version: "1.36.0".into(),
                    target: "windows-x86_64".into(),
                    source: CoreSourceView::Imported,
                    executable: "D:/cores/aria2c.exe".into(),
                    features: vec!["BitTorrent".into()],
                    is_active: true,
                    is_last_working: true,
                    validated_version: Some("1.36.0".into()),
                    status: CoreInstallStatusView::Ready,
                }],
            },
            cx,
        );
        shell
    });

    view.update(cx, |shell, cx| {
        shell.request_core_command(
            CoreCommandView::Verify {
                core_id: "c1".into(),
            },
            cx,
        );
        shell.set_core_command_result(
            CoreCommandResultView {
                request_id: RequestId::from_u64(3),
                command: CoreCommandView::Verify {
                    core_id: "c1".into(),
                },
                registry: CoreRegistryView {
                    active_id: Some("c1".into()),
                    last_working_id: Some("c1".into()),
                    installations: vec![CoreInstallationView {
                        id: "c1".into(),
                        version: "1.36.0".into(),
                        target: "windows-x86_64".into(),
                        source: CoreSourceView::Imported,
                        executable: "D:/cores/aria2c.exe".into(),
                        features: vec!["BitTorrent".into(), "HTTPS".into()],
                        is_active: true,
                        is_last_working: true,
                        validated_version: Some("1.36.0".into()),
                        status: CoreInstallStatusView::Ready,
                    }],
                },
                outcome: CoreCommandOutcomeView::Success,
            },
            cx,
        );
        assert_eq!(shell.cores.installations[0].features.len(), 2);
        assert!(
            shell
                .status_notice
                .as_ref()
                .is_some_and(|notice| notice.message.contains("verified"))
        );
    });
}
