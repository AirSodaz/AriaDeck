use gpui::{TestAppContext, px};

use super::*;

#[test]
fn task_layout_follows_viewport_shell_breakpoint() {
    assert_eq!(task_layout_mode(1_180.0), TaskLayoutMode::Compact);
    assert_eq!(task_layout_mode(960.0), TaskLayoutMode::Compact);
    assert_eq!(task_layout_mode(1_400.0), TaskLayoutMode::Wide);
}

#[gpui::test]
fn title_bar_renders_centered_search_and_add_action(cx: &mut TestAppContext) {
    let (shell, mut visual_cx) =
        open_test_shell_sized(cx, gpui::size(px(1_280.0), px(720.0)), |window, cx| {
            let mut shell = AppShell::new(Theme::dark(), window, cx);
            shell.snapshot = snapshot(2);
            shell
        });

    visual_cx.run_until_parked();
    let _ = visual_cx.update(|window, cx| window.draw(cx));

    shell.read_with(&visual_cx, |shell, cx| {
        let search_bounds = shell
            .search_input
            .read(cx)
            .text_bounds()
            .expect("titlebar search input must be painted");
        assert!(
            search_bounds.bottom() <= px(TITLEBAR_HEIGHT + 2.0),
            "search input must sit inside the {TITLEBAR_HEIGHT}px titlebar (bottom={:?})",
            search_bounds.bottom()
        );
        assert!(
            search_bounds.size.width > px(0.0),
            "titlebar search input must have positive width"
        );
    });

    let add_bounds = visual_cx
        .debug_bounds("titlebar-add-download")
        .expect("titlebar add download button must be painted");
    let center = add_bounds.center();
    let down_result = visual_cx.update(|window, cx| {
        window.dispatch_event(
            gpui::PlatformInput::MouseDown(MouseDownEvent {
                button: MouseButton::Left,
                position: center,
                modifiers: gpui::Modifiers::none(),
                click_count: 1,
                first_mouse: false,
            }),
            cx,
        )
    });
    assert!(
        !down_result.propagate,
        "mouse down on titlebar add button must stop propagation so Windows HTCAPTION does not hijack the click"
    );
    visual_cx.simulate_mouse_up(center, MouseButton::Left, gpui::Modifiers::none());
    visual_cx.run_until_parked();
    let _ = visual_cx.update(|window, cx| window.draw(cx));

    shell.read_with(&visual_cx, |shell, _| {
        assert!(
            shell.add_dialog_entity().is_some(),
            "clicking the titlebar add download button must open the Add Download dialog"
        );
    });
    assert!(
        visual_cx.update(|window, cx| window.has_active_dialog(cx)),
        "Add Download dialog must mount in Root after clicking the titlebar button"
    );
}

#[gpui::test]
fn speed_chart_area_chart_renders_empty_single_and_bounded_histories(cx: &mut TestAppContext) {
    let (shell, mut visual_cx) = open_test_shell(cx, |window, cx| {
        let mut shell = AppShell::new(Theme::dark(), window, cx);
        shell.snapshot = snapshot(1);
        shell.active_status_popover = Some(StatusBarPopover::Speed);
        shell
    });

    for sample_count in [0, 1, SPEED_CHART_SAMPLES + 10] {
        shell.update(&mut visual_cx, |shell, cx| {
            shell.snapshot.speed_history = (0..sample_count)
                .map(|index| SpeedSampleView {
                    download_rate: (index as u64 + 1) * 1024,
                    upload_rate: (index as u64) * 256,
                })
                .collect();
            shell.snapshot.download_rate = shell
                .snapshot
                .speed_history
                .last()
                .map_or(0, |sample| sample.download_rate);
            shell.snapshot.upload_rate = shell
                .snapshot
                .speed_history
                .last()
                .map_or(0, |sample| sample.upload_rate);
            cx.notify();
        });
        visual_cx.run_until_parked();
        let _ = visual_cx.update(|window, cx| window.draw(cx));
    }

    shell.read_with(&visual_cx, |shell, _| {
        assert_eq!(
            speed_chart_window(&shell.snapshot.speed_history).len(),
            SPEED_CHART_SAMPLES
        );
    });
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
        assert_eq!(notice.message, shell.t_count("notice-engine-recovered", 1));
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
fn speed_popover_toggles_and_restores_previous_focus(cx: &mut TestAppContext) {
    let (view, cx) = cx.add_window_view(|window, cx| {
        let shell = AppShell::new(Theme::dark(), window, cx);
        window.focus(&shell.focus_handle, cx);
        shell
    });

    view.update_in(cx, |shell, window, cx| {
        shell.toggle_speed_popover(window, cx);
        assert_eq!(shell.active_status_popover, Some(StatusBarPopover::Speed));
        shell.close_speed_popover(window, cx);
    });
    view.read_with(cx, |shell, _| {
        assert_eq!(shell.active_status_popover, None);
        assert!(shell.speed_popover_previous_focus.is_none());
    });
}

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
        let expected_failure = shell.t_args(
            "notice-task-failed-single",
            &[(
                "name",
                FluentValue::from(task_display_name(
                    &shell.snapshot.tasks[2],
                    shell.translator(),
                )),
            )],
        );
        assert!(
            shell
                .status_notice
                .as_ref()
                .is_some_and(|notice| notice.is_error && notice.message == expected_failure),
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
        assert_eq!(
            shell.active_status_popover,
            Some(StatusBarPopover::Activity)
        );
        shell.clear_activity_log(cx);
        assert!(shell.activity_log.is_empty());
        shell.close_activity_panel(window, cx);
        assert_eq!(shell.active_status_popover, None);
    });
}

#[gpui::test]
fn rapid_ten_thousand_snapshots_keep_viewport_window(cx: &mut TestAppContext) {
    let (view, cx) = cx.add_window_view(|window, cx| {
        let mut shell = AppShell::new(Theme::dark(), window, cx);
        shell.snapshot = snapshot(10_000);
        shell
    });

    for revision in 2..=20u64 {
        view.update(cx, |shell, cx| {
            let mut next = snapshot(10_000);
            next.source_revision = revision;
            next.download_rate = revision;
            shell.set_snapshot(next, cx);
        });
    }

    view.read_with(cx, |shell, _| {
        let rendered = shell.rendered_range();
        assert!(!rendered.is_empty());
        assert!(
            rendered.len() < 64,
            "rapid full-list snapshots must stay virtualized ({})",
            rendered.len()
        );
        assert_eq!(shell.snapshot.tasks.len(), 10_000);
        assert_eq!(shell.snapshot.source_revision, 20);
    });
}

#[gpui::test]
fn power_action_armed_with_zero_tasks_does_not_trigger(cx: &mut TestAppContext) {
    let (view, cx) = cx.add_window_view(|window, cx| AppShell::new(Theme::dark(), window, cx));

    view.update(cx, |shell, cx| {
        shell.select_completion_action(CompletionPowerActionView::Shutdown, cx);
        let mut snap = snapshot(0);
        snap.counts.active = 0;
        snap.counts.waiting = 0;
        shell.set_snapshot(snap, cx);
        assert!(shell.power_countdown().is_none());
        assert_eq!(
            shell.settings.platform.action_after_done,
            CompletionPowerActionView::Shutdown
        );
    });
}

#[gpui::test]
fn power_action_triggers_when_active_tasks_complete(cx: &mut TestAppContext) {
    let (view, cx) = cx.add_window_view(|window, cx| AppShell::new(Theme::dark(), window, cx));

    view.update(cx, |shell, cx| {
        shell.select_completion_action(CompletionPowerActionView::Shutdown, cx);
        let mut snap = snapshot(1);
        snap.counts.active = 1;
        shell.set_snapshot(snap, cx);
        assert!(shell.power_countdown().is_none());
        assert!(shell.power_had_active_tasks);

        let mut done_snap = snapshot(1);
        done_snap.source_revision = 2;
        done_snap.counts.active = 0;
        done_snap.counts.waiting = 0;
        shell.set_snapshot(done_snap, cx);
        assert!(shell.power_countdown().is_some());
        let cd = shell.power_countdown().expect("countdown active");
        assert_eq!(cd.action, CompletionPowerActionView::Shutdown);
        assert_eq!(cd.remaining_seconds, 30);
    });
}

#[gpui::test]
fn power_action_countdown_cancelled_by_user(cx: &mut TestAppContext) {
    let (view, cx) = cx.add_window_view(|window, cx| AppShell::new(Theme::dark(), window, cx));

    view.update(cx, |shell, cx| {
        shell.select_completion_action(CompletionPowerActionView::Shutdown, cx);
        let mut snap = snapshot(1);
        snap.counts.active = 1;
        shell.set_snapshot(snap, cx);

        let mut done_snap = snapshot(1);
        done_snap.source_revision = 2;
        done_snap.counts.active = 0;
        done_snap.counts.waiting = 0;
        shell.set_snapshot(done_snap, cx);
        assert!(shell.power_countdown().is_some());

        shell.cancel_power_countdown(cx);
        assert!(shell.power_countdown().is_none());
        assert_eq!(
            shell.settings.platform.action_after_done,
            CompletionPowerActionView::None
        );
    });
}

#[gpui::test]
fn power_action_countdown_cancelled_by_new_task(cx: &mut TestAppContext) {
    let (view, cx) = cx.add_window_view(|window, cx| AppShell::new(Theme::dark(), window, cx));

    view.update(cx, |shell, cx| {
        shell.select_completion_action(CompletionPowerActionView::Shutdown, cx);
        let mut snap = snapshot(1);
        snap.counts.active = 1;
        shell.set_snapshot(snap, cx);

        let mut done_snap = snapshot(1);
        done_snap.source_revision = 2;
        done_snap.counts.active = 0;
        done_snap.counts.waiting = 0;
        shell.set_snapshot(done_snap, cx);
        assert!(shell.power_countdown().is_some());

        let mut new_snap = snapshot(2);
        new_snap.source_revision = 3;
        new_snap.counts.active = 1;
        shell.set_snapshot(new_snap, cx);
        assert!(shell.power_countdown().is_none());
        assert!(shell.power_had_active_tasks);
    });
}

#[gpui::test]
fn power_action_countdown_ticks_and_emits_on_zero(cx: &mut TestAppContext) {
    let (view, cx) = cx.add_window_view(|window, cx| AppShell::new(Theme::dark(), window, cx));

    view.update(cx, |shell, cx| {
        shell.active_modal = Some(ActiveModal::PowerCountdown(PowerCountdownState {
            action: CompletionPowerActionView::Hibernate,
            remaining_seconds: 2,
        }));
        assert!(shell.tick_power_countdown(cx));
        assert_eq!(
            shell
                .power_countdown()
                .expect("countdown active")
                .remaining_seconds,
            1
        );

        assert!(!shell.tick_power_countdown(cx));
        assert!(shell.power_countdown().is_none());
        assert_eq!(
            shell.settings.platform.action_after_done,
            CompletionPowerActionView::None
        );
    });
}

#[gpui::test]
fn power_popover_toggles(cx: &mut TestAppContext) {
    let (view, cx) = cx.add_window_view(|window, cx| AppShell::new(Theme::dark(), window, cx));

    view.update(cx, |shell, cx| {
        assert_eq!(shell.active_status_popover, None);
        shell.toggle_power_popover(cx);
        assert_eq!(shell.active_status_popover, Some(StatusBarPopover::Power));
        shell.close_power_popover(cx);
        assert_eq!(shell.active_status_popover, None);
    });
}

#[gpui::test]
fn power_countdown_cancelled_by_escape(cx: &mut TestAppContext) {
    let (view, cx) = cx.add_window_view(|window, cx| AppShell::new(Theme::dark(), window, cx));

    view.update_in(cx, |shell, window, cx| {
        shell.active_modal = Some(ActiveModal::PowerCountdown(PowerCountdownState {
            action: CompletionPowerActionView::Shutdown,
            remaining_seconds: 30,
        }));
        shell.clear_search(&ClearSearch, window, cx);
        assert!(shell.power_countdown().is_none());
        assert_eq!(
            shell.settings.platform.action_after_done,
            CompletionPowerActionView::None
        );
    });
}

#[gpui::test]
fn status_bar_popovers_are_mutually_exclusive(cx: &mut TestAppContext) {
    let (shell, mut cx) =
        open_test_shell(cx, |window, cx| AppShell::new(Theme::dark(), window, cx));

    shell.update_in(&mut cx, |shell, window, cx| {
        shell.toggle_speed_popover(window, cx);
        assert_eq!(shell.active_status_popover, Some(StatusBarPopover::Speed));

        shell.toggle_power_popover(cx);
        assert_eq!(shell.active_status_popover, Some(StatusBarPopover::Power));

        shell.toggle_activity_panel(window, cx);
        assert_eq!(
            shell.active_status_popover,
            Some(StatusBarPopover::Activity)
        );

        shell.toggle_activity_panel(window, cx);
        assert_eq!(shell.active_status_popover, None);
    });
}

#[gpui::test]
fn notices_push_into_root_notifications_and_stack(cx: &mut TestAppContext) {
    let (shell, mut cx) =
        open_test_shell(cx, |window, cx| AppShell::new(Theme::dark(), window, cx));

    shell.update(&mut cx, |shell, cx| {
        shell.show_notice("Saved successfully.", false, cx);
        shell.show_notice("Connection refused.", true, cx);
    });
    cx.run_until_parked();

    cx.update(|window, cx| {
        let notifications = window.notifications(cx);
        assert_eq!(notifications.len(), 2);
    });
}
