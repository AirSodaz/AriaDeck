use gpui::{TestAppContext, px};

use super::*;

#[test]
fn shell_breakpoints_select_sidebar_and_details_presentation() {
    let compact = shell_layout(1_279.0);
    assert_eq!(compact.sidebar_width, COMPACT_SIDEBAR_WIDTH);
    assert_eq!(compact.details_presentation, DetailsPresentation::Overlay);
    assert_eq!(compact.task_layout, TaskLayoutMode::Compact);

    let wide = shell_layout(1_280.0);
    assert_eq!(wide.sidebar_width, SIDEBAR_WIDTH);
    assert_eq!(wide.details_presentation, DetailsPresentation::Inline);
    assert_eq!(wide.task_layout, TaskLayoutMode::Wide);
}

#[test]
fn search_bounds_are_centered_and_ignore_workspace_drawers() {
    for viewport_width in [960.0, 1_180.0, 1_600.0] {
        let (left, right) = centered_search_bounds(viewport_width);
        assert!(((left + right) / 2.0 - viewport_width / 2.0).abs() < f32::EPSILON);
        assert!(right - left <= SEARCH_WIDTH);
    }
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
    view.read_with(cx, |shell, cx| {
        let parent = task(0).identity;
        let successor = task(3).identity;
        assert_eq!(shell.selected, Some(task(1).identity));
        assert_eq!(shell.selected_tasks.len(), 2);
        assert!(!shell.selected_tasks.contains(&parent));
        assert!(shell.selected_tasks.contains(&successor));
        assert_eq!(shell.range_anchor, Some(successor.clone()));
        assert_eq!(
            shell
                .details_panel()
                .map(|panel| panel.read(cx).identity.clone()),
            Some(successor)
        );
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
    let (request_id, session, identity) = view.read_with(cx, |shell, cx| {
        let drawer = shell.details_panel().expect("drawer must exist").read(cx);
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
    view.read_with(cx, |shell, cx| {
        let drawer = shell.details_panel().expect("drawer must exist").read(cx);
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
    view.read_with(cx, |shell, cx| {
        let drawer = shell.details_panel().expect("drawer must exist").read(cx);
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
    let (initial_request, session, identity) = view.read_with(cx, |shell, cx| {
        let drawer = shell.details_panel().expect("drawer must exist").read(cx);
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
        revision_two.source_revision = 2;
        revision_two.tasks[0].revision = 2;
        shell.set_snapshot(revision_two, cx);
    });
    let refresh_request = view.read_with(cx, |shell, cx| {
        let drawer = shell.details_panel().expect("drawer must exist").read(cx);
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
        revision_three.source_revision = 3;
        revision_three.tasks[0].revision = 3;
        shell.set_snapshot(revision_three, cx);
    });
    view.read_with(cx, |shell, cx| {
        assert_eq!(
            shell
                .details_panel()
                .and_then(|panel| panel.read(cx).pending.as_ref())
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
    let catch_up_request = view.read_with(cx, |shell, cx| {
        let drawer = shell.details_panel().expect("drawer must exist").read(cx);
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
    view.read_with(cx, |shell, cx| {
        let drawer = shell.details_panel().expect("drawer must exist").read(cx);
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
    view.read_with(cx, |shell, cx| {
        let drawer = shell.details_panel().expect("drawer remains open").read(cx);
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
    view.read_with(cx, |shell, cx| {
        assert_eq!(shell.selected, selected);
        assert_eq!(
            shell
                .details_panel()
                .map(|panel| panel.read(cx).identity.clone()),
            selected
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
        shell.open_details_for(overview, cx);
        if let Some(panel) = shell.details_panel().cloned() {
            panel.update(cx, |drawer, cx| {
                drawer.pending = None;
                drawer.state = TaskDetailsLoadState::Ready {
                    details: Arc::new(details(10_000)),
                };
                drawer.tab = TaskDetailsTab::Files;
                cx.notify();
            });
        }
        shell
    });

    view.read_with(cx, |shell, cx| {
        let drawer = shell.details_panel().expect("drawer must exist").read(cx);
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
fn identical_snapshot_is_a_no_op_for_selection_and_details(cx: &mut TestAppContext) {
    let (view, cx) = cx.add_window_view(|window, cx| {
        let mut shell = AppShell::new(Theme::dark(), window, cx);
        shell.snapshot = snapshot(3);
        shell.selected = Some(shell.snapshot.tasks[1].identity.clone());
        shell
    });
    let selected = view.read_with(cx, |shell, _| shell.selected.clone());

    view.update(cx, |shell, cx| {
        let same = shell.snapshot.clone();
        shell.set_snapshot(same, cx);
    });
    view.read_with(cx, |shell, _| {
        assert_eq!(shell.selected, selected);
        assert_eq!(shell.snapshot.source_revision, 1);
    });
}

#[gpui::test]
fn details_drawer_tabs_render_with_scrollbars_inside_viewport(cx: &mut TestAppContext) {
    let viewport_width = px(720.0);
    let viewport_height = px(420.0);
    let (shell, mut visual_cx) = open_test_shell_sized(
        cx,
        gpui::size(viewport_width, viewport_height),
        |window, cx| {
            let mut shell = AppShell::new(Theme::dark(), window, cx);
            shell.snapshot = snapshot(2);
            shell.selected = Some(shell.snapshot.tasks[0].identity.clone());
            shell
        },
    );

    let (request_id, session, identity) = shell.update_in(&mut visual_cx, |shell, window, cx| {
        shell.open_task_details_action(&OpenTaskDetails, window, cx);
        let drawer = shell.details_panel().expect("drawer opened").read(cx);
        let pending = drawer.pending.as_ref().expect("details request pending");
        (
            pending.request_id,
            drawer.session.clone(),
            drawer.identity.clone(),
        )
    });

    shell.update(&mut visual_cx, |shell, cx| {
        shell.set_task_details_result(
            TaskDetailsResultView {
                request_id,
                session,
                identity,
                outcome: TaskDetailsOutcomeView::Ready(Box::new(details(12))),
            },
            cx,
        );
    });
    visual_cx.run_until_parked();
    assert!(visual_cx.update(|window, cx| window.has_active_sheet(cx)));
    // Allow Sheet's 150ms slide-in animation to settle at right(0px).
    visual_cx
        .background_executor
        .advance_clock(Duration::from_millis(200));
    visual_cx.run_until_parked();

    for tab in [
        TaskDetailsTab::Info,
        TaskDetailsTab::Files,
        TaskDetailsTab::Network,
        TaskDetailsTab::Options,
    ] {
        shell.update(&mut visual_cx, |shell, cx| {
            if let Some(panel) = shell.details_panel().cloned() {
                panel.update(cx, |drawer, cx| {
                    drawer.tab = tab;
                    cx.notify();
                });
            }
            cx.notify();
        });
        visual_cx.run_until_parked();
        let _ = visual_cx.update(|window, cx| window.draw(cx));

        shell.read_with(&visual_cx, |shell, cx| {
            let drawer = shell.details_panel().expect("drawer present").read(cx);
            let bounds = match tab {
                TaskDetailsTab::Info => drawer.info_scroll.bounds(),
                TaskDetailsTab::Files => drawer.file_scroll.0.borrow().base_handle.bounds(),
                TaskDetailsTab::Network => drawer.network_scroll.bounds(),
                TaskDetailsTab::Options => drawer.options_scroll.bounds(),
            };
            assert!(
                bounds.size.width > px(0.0) && bounds.size.height > px(0.0),
                "details tab {tab:?} must have positive scroll bounds"
            );
            assert!(
                bounds.right() <= viewport_width + px(0.5),
                "details tab {tab:?} scroll bounds must remain inside viewport (right={:?}, viewport={:?})",
                bounds.right(),
                viewport_width
            );
        });
    }
}

#[gpui::test]
fn details_drawer_transitions_between_sheet_and_inline_across_breakpoint(cx: &mut TestAppContext) {
    let (shell, mut visual_cx) =
        open_test_shell_sized(cx, gpui::size(px(900.0), px(700.0)), |window, cx| {
            let mut shell = AppShell::new(Theme::dark(), window, cx);
            shell.snapshot = snapshot(2);
            shell.selected = Some(shell.snapshot.tasks[0].identity.clone());
            shell
        });

    shell.update_in(&mut visual_cx, |shell, window, cx| {
        shell.open_task_details_action(&OpenTaskDetails, window, cx);
    });
    visual_cx.run_until_parked();
    assert!(visual_cx.update(|window, cx| window.has_active_sheet(cx)));
    // Allow Sheet's 150ms slide-in animation to settle at right(0px).
    visual_cx
        .background_executor
        .advance_clock(Duration::from_millis(200));
    visual_cx.run_until_parked();

    // Grow across the 1280px breakpoint: Sheet closes, inline drawer stays open.
    visual_cx.simulate_resize(gpui::size(px(1400.0), px(700.0)));
    visual_cx.run_until_parked();
    assert!(!visual_cx.update(|window, cx| window.has_active_sheet(cx)));
    shell.read_with(&visual_cx, |shell, _| {
        assert!(shell.details_panel().is_some());
    });

    // Shrink back below 1280px: Sheet re-opens automatically.
    visual_cx.simulate_resize(gpui::size(px(900.0), px(700.0)));
    visual_cx.run_until_parked();
    assert!(visual_cx.update(|window, cx| window.has_active_sheet(cx)));
    // Allow Sheet's 150ms slide-in animation to settle at right(0px).
    visual_cx
        .background_executor
        .advance_clock(Duration::from_millis(200));
    visual_cx.run_until_parked();
}

#[gpui::test]
fn details_drawer_preserves_active_tab_when_switching_selected_task(cx: &mut TestAppContext) {
    let (view, cx) = cx.add_window_view(|window, cx| {
        let mut shell = AppShell::new(Theme::dark(), window, cx);
        shell.snapshot = snapshot(3);
        shell.select_at_with_modifiers(0, false, false, window, cx);
        shell.open_details_for(task(0), cx);
        shell
    });

    view.update(cx, |shell, cx| {
        let panel = shell.details_panel().expect("details open").clone();
        panel.update(cx, |panel, _| {
            panel.tab = TaskDetailsTab::Network;
        });
    });

    view.update_in(cx, |shell, window, cx| {
        shell.select_at_with_modifiers(1, false, false, window, cx);
    });

    view.read_with(cx, |shell, cx| {
        let drawer = shell.details_panel().expect("details still open").read(cx);
        assert_eq!(drawer.identity, task(1).identity);
        assert_eq!(
            drawer.tab,
            TaskDetailsTab::Network,
            "active details tab must stay selected when browsing tasks"
        );
    });
}
