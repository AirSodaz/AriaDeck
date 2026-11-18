use gpui::TestAppContext;

use super::*;

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

    cx.simulate_keystrokes(&format!("{}-shift-up", secondary_modifier()));

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
fn add_download_submission_is_single_flight(cx: &mut TestAppContext) {
    let (view, cx) = cx.add_window_view(|window, cx| {
        let mut shell = AppShell::new(Theme::dark(), window, cx);
        shell.snapshot = snapshot(1);
        shell.add_dialog.open = true;
        shell
    });

    view.update_in(cx, |shell, window, cx| {
        shell.add_input.update(cx, |input, cx| {
            input.set_value("https://example.com/archive.bin", window, cx);
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
        shell.set_search_query("archive", window, cx);
    });
    view.read_with(cx, |shell, _| {
        assert!(
            !shell.queue_reordering_available(),
            "a searched projection hides the global queue position"
        );
    });
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
        assert!(shell.open_metadata_paths(
            vec![
                PathBuf::from("sample.TORRENT"),
                PathBuf::from("sample.TORRENT"),
                PathBuf::from("bundle.meta4"),
                PathBuf::from("notes.txt"),
            ],
            window,
            cx,
        ));
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
fn metadata_paths_wait_until_commands_are_available(cx: &mut TestAppContext) {
    let (view, cx) = cx.add_window_view(|window, cx| AppShell::new(Theme::dark(), window, cx));

    view.update_in(cx, |shell, window, cx| {
        assert!(!shell.can_open_metadata_paths());
        assert!(!shell.open_metadata_paths(vec![PathBuf::from("queued.torrent")], window, cx,));
        assert!(!shell.add_dialog.open);

        shell.snapshot = snapshot(1);
        assert!(shell.can_open_metadata_paths());
        assert!(shell.open_metadata_paths(vec![PathBuf::from("queued.torrent")], window, cx,));
        assert!(shell.add_dialog.open);
        assert!(shell.add_dialog.preview_pending.is_some());
    });
}

#[gpui::test]
fn magnet_launch_opens_links_without_submitting_and_does_not_clobber_open_dialog(
    cx: &mut TestAppContext,
) {
    let magnet =
        "magnet:?xt=urn:btih:0123456789abcdef0123456789abcdef01234567&dn=Example".to_owned();
    let (view, cx) = cx.add_window_view(|window, cx| {
        let mut shell = AppShell::new(Theme::dark(), window, cx);
        shell.snapshot = snapshot(1);
        shell
    });

    view.update_in(cx, |shell, window, cx| {
        assert!(shell.can_open_magnet_uris());
        assert!(shell.open_magnet_uris(vec![magnet.clone(), magnet.clone()], window, cx));
        assert!(shell.add_dialog.open);
        assert!(shell.add_dialog.pending.is_none());
        assert_eq!(shell.add_input.read(cx).value().as_ref(), magnet);
        assert!(!shell.can_open_magnet_uris());
        assert!(!shell.open_magnet_uris(vec!["magnet:?xt=urn:btih:bad".into()], window, cx));
        assert_eq!(shell.add_input.read(cx).value().as_ref(), magnet);
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
        assert!(shell.open_metadata_paths(
            vec![PathBuf::from("one.torrent"), PathBuf::from("two.meta4")],
            window,
            cx,
        ));
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
            input.set_value("https://example.test/ref", window, cx);
        });
        shell.add_inputs.user_agent.update(cx, |input, cx| {
            input.set_value("AriaDeck-Test/1.0", window, cx);
        });
        shell.add_inputs.headers.update(cx, |input, cx| {
            input.set_value("X-Token: one\nAccept: */*", window, cx);
        });
        shell.add_inputs.cookie.update(cx, |input, cx| {
            input.set_value("session=secret-cookie", window, cx);
        });
        shell.add_inputs.http_user.update(cx, |input, cx| {
            input.set_value("alice", window, cx);
        });
        shell.add_inputs.http_passwd.update(cx, |input, cx| {
            input.set_value("s3cret", window, cx);
        });
        shell.add_inputs.checksum.update(cx, |input, cx| {
            input.set_value(format!("sha-256={}", "ab".repeat(32)), window, cx);
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
        assert!(shell.add_inputs.cookie.read(cx).presentation().is_masked());
        assert!(
            shell
                .add_inputs
                .http_passwd
                .read(cx)
                .presentation()
                .is_masked()
        );
    });
}

#[gpui::test]
fn add_download_dialog_accepts_keyboard_input(cx: &mut TestAppContext) {
    let (view, mut cx) = open_test_shell(cx, |window, cx| {
        let mut shell = AppShell::new(Theme::dark(), window, cx);
        shell.snapshot = snapshot(1);
        shell
    });

    view.update_in(&mut cx, |shell, window, cx| {
        shell.open_add_download(&OpenAddDownload, window, cx);
    });
    cx.run_until_parked();
    let _ = cx.update(|window, cx| window.draw(cx));
    cx.simulate_input("https://example.com/file");

    view.read_with(&cx, |shell, cx| {
        assert_eq!(shell.add_input.read(cx).text(), "https://example.com/file");
    });
}

#[gpui::test]
fn add_download_dialog_input_can_be_clicked_before_typing(cx: &mut TestAppContext) {
    let (view, mut cx) = open_test_shell(cx, |window, cx| {
        let mut shell = AppShell::new(Theme::dark(), window, cx);
        shell.snapshot = snapshot(1);
        shell
    });

    view.update_in(&mut cx, |shell, window, cx| {
        shell.open_add_download(&OpenAddDownload, window, cx);
    });
    cx.run_until_parked();
    let _ = cx.update(|window, cx| window.draw(cx));
    let input_bounds = view.read_with(&cx, |shell, cx| {
        shell
            .add_input
            .read(cx)
            .text_bounds()
            .expect("add-download input must be painted")
    });
    view.update_in(&mut cx, |shell, window, cx| {
        window.focus(&shell.search_input.focus_handle(cx), cx);
    });
    cx.simulate_click(input_bounds.center(), Default::default());
    cx.simulate_input("https://example.com/file");

    view.read_with(&cx, |shell, cx| {
        assert_eq!(shell.add_input.read(cx).value(), "https://example.com/file");
    });
}

#[gpui::test]
fn add_download_dialog_supports_standard_clipboard_shortcuts(cx: &mut TestAppContext) {
    let (view, mut cx) = open_test_shell(cx, |window, cx| {
        let mut shell = AppShell::new(Theme::dark(), window, cx);
        shell.snapshot = snapshot(1);
        shell
    });

    view.update_in(&mut cx, |shell, window, cx| {
        shell.open_add_download(&OpenAddDownload, window, cx);
        shell.add_input.update(cx, |input, cx| {
            input.set_value("https://example.com/file", window, cx);
        });
    });
    cx.run_until_parked();
    let _ = cx.update(|window, cx| window.draw(cx));
    cx.simulate_keystrokes(&format!(
        "{}-a {}-c",
        secondary_modifier(),
        secondary_modifier()
    ));
    assert_eq!(
        cx.read_from_clipboard().and_then(|item| item.text()),
        Some("https://example.com/file".to_owned())
    );

    cx.write_to_clipboard(ClipboardItem::new_string(
        "magnet:?xt=urn:btih:test".to_owned(),
    ));
    cx.simulate_keystrokes(&format!("{}-v", secondary_modifier()));
    view.read_with(&cx, |shell, cx| {
        assert_eq!(shell.add_input.read(cx).value(), "magnet:?xt=urn:btih:test");
    });

    cx.simulate_keystrokes(&format!(
        "{}-a {}-x",
        secondary_modifier(),
        secondary_modifier()
    ));
    view.read_with(&cx, |shell, cx| {
        assert!(shell.add_input.read(cx).value().is_empty());
    });
    assert_eq!(
        cx.read_from_clipboard().and_then(|item| item.text()),
        Some("magnet:?xt=urn:btih:test".to_owned())
    );
}

#[gpui::test]
fn add_download_input_preserves_pasted_lines_and_shift_enter(cx: &mut TestAppContext) {
    let (view, mut cx) = open_test_shell(cx, |window, cx| {
        let mut shell = AppShell::new(Theme::dark(), window, cx);
        shell.snapshot = snapshot(1);
        shell
    });
    view.update_in(&mut cx, |shell, window, cx| {
        shell.open_add_download(&OpenAddDownload, window, cx);
    });
    cx.run_until_parked();
    let _ = cx.update(|window, cx| window.draw(cx));

    cx.write_to_clipboard(ClipboardItem::new_string(
        "https://example.test/one\r\nhttps://example.test/two".into(),
    ));
    cx.simulate_keystrokes(&format!("{}-v shift-enter", secondary_modifier()));
    cx.simulate_input("magnet:?xt=urn:btih:abc");

    view.read_with(&cx, |shell, cx| {
        assert_eq!(
            shell.add_input.read(cx).value().replace("\r\n", "\n"),
            "https://example.test/one\nhttps://example.test/two\nmagnet:?xt=urn:btih:abc"
        );
        assert_eq!(
            parse_add_download_sources(shell.add_input.read(cx).value().as_ref()).len(),
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
            input.set_value(
                "https://example.test/accepted\nhttps://example.test/retry\nhttps://example.test/unknown",
                window,
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
            input.set_value("https://example.test/existing", window, cx);
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
fn add_download_links_input_height_defaults_and_clamps(cx: &mut TestAppContext) {
    let (view, cx) = cx.add_window_view(|window, cx| {
        let mut shell = AppShell::new(Theme::dark(), window, cx);
        shell.snapshot = snapshot(1);
        shell
    });

    view.read_with(cx, |shell, _| {
        assert_eq!(shell.links_input_height, DEFAULT_LINKS_INPUT_HEIGHT);
        assert_eq!(shell.links_input_height, 120.0);
    });

    view.update_in(cx, |shell, _window, cx| {
        // Test clamping to MIN
        shell.links_input_height = 50.0f32.clamp(MIN_LINKS_INPUT_HEIGHT, MAX_LINKS_INPUT_HEIGHT);
        assert_eq!(shell.links_input_height, 80.0);
        // Test clamping to MAX
        shell.links_input_height = 500.0f32.clamp(MIN_LINKS_INPUT_HEIGHT, MAX_LINKS_INPUT_HEIGHT);
        assert_eq!(shell.links_input_height, 360.0);
        // Test reset to default
        shell.links_input_height = DEFAULT_LINKS_INPUT_HEIGHT;
        assert_eq!(shell.links_input_height, 120.0);
        cx.notify();
    });
}

#[gpui::test]
fn add_download_dialog_renders_scrollable_and_preserves_height_across_open_close(
    cx: &mut TestAppContext,
) {
    let (view, cx) = cx.add_window_view(|window, cx| {
        let mut shell = AppShell::new(Theme::dark(), window, cx);
        shell.snapshot = snapshot(1);
        shell
    });

    view.update_in(cx, |shell, window, cx| {
        shell.open_add_download(&OpenAddDownload, window, cx);
        assert!(shell.add_dialog.open);
        // User adjusts height
        shell.links_input_height = 200.0;
        // Expand advanced options
        shell.toggle_add_advanced(cx);
        assert!(shell.add_dialog.advanced_open);
        // Close dialog
        shell.close_add_download(window, cx);
        assert!(!shell.add_dialog.open);
    });

    // Verify session persistence of adjusted height
    view.read_with(cx, |shell, _| {
        assert_eq!(shell.links_input_height, 200.0);
    });

    // Reopen dialog: height must be preserved
    view.update_in(cx, |shell, window, cx| {
        shell.open_add_download(&OpenAddDownload, window, cx);
        assert!(shell.add_dialog.open);
        assert_eq!(shell.links_input_height, 200.0);
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

    view.update_in(cx, |shell, window, cx| {
        shell.output_name_input.update(cx, |input, cx| {
            input.set_value("folder/archive.iso", window, cx);
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

    view.update_in(cx, |shell, window, cx| {
        shell.output_name_input.update(cx, |input, cx| {
            input.set_value("  archive-renamed.iso  ", window, cx);
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

    view.update_in(cx, |shell, window, cx| {
        shell.output_name_input.update(cx, |input, cx| {
            input.set_value("first.iso", window, cx);
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

    view.update_in(cx, |shell, window, cx| {
        shell.output_name_input.update(cx, |input, cx| {
            input.set_value("second.iso", window, cx);
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
    view.update_in(cx, |shell, window, cx| {
        // "2M" and blank (unlimited) both go through the K/M parser.
        shell
            .settings_inputs
            .download_limit
            .update(cx, |input, cx| {
                input.set_value("2M", window, cx);
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
        assert_eq!(shell.settings_inputs.download_limit.read(cx).value(), "2M");
        assert!(
            shell
                .settings_inputs
                .upload_limit
                .read(cx)
                .value()
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
    view.update_in(cx, |shell, window, cx| {
        shell
            .settings_inputs
            .download_limit
            .update(cx, |input, cx| {
                input.set_value("5MB", window, cx);
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
        assert_eq!(notice.message, shell.t("notice-force-pause-unsupported"));
    });
}
