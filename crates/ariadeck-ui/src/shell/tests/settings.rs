use gpui::{TestAppContext, px};

use super::*;

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
fn update_checks_are_single_flight_and_portable_offers_never_download(cx: &mut TestAppContext) {
    let (view, cx) = cx.add_window_view(|window, cx| AppShell::new(Theme::dark(), window, cx));
    let events = Arc::new(std::sync::Mutex::new(Vec::new()));
    let sink = events.clone();
    let _subscription = view.update(cx, |_, cx| {
        cx.subscribe(&view, move |_, _, event: &AppShellEvent, _| {
            sink.lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .push(event.clone());
        })
    });

    view.update(cx, |shell, cx| {
        shell.configure_updates(true, true, cx);
        shell.request_update_check(cx);
        shell.request_update_check(cx);
        let request_id = shell.pending_update_check.expect("check is pending");
        shell.set_update_check_result(
            request_id,
            Ok(Some(crate::UpdateOfferView {
                version: "0.3.0".into(),
                title: "AriaDeck 0.3.0".into(),
                notes: "Release notes".into(),
                release_url: "https://github.com/AirSodaz/AriaDeck/releases/tag/v0.3.0".into(),
                installer_available: true,
            })),
            cx,
        );
        shell.request_update_download(cx);
        assert!(matches!(
            shell.update_status,
            crate::UpdateStatusView::Available { .. }
        ));
    });

    let events = events
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    assert_eq!(events.len(), 1, "duplicate checks are merged");
    assert!(matches!(
        events[0],
        AppShellEvent::CheckForUpdatesRequested { .. }
    ));
}

#[gpui::test]
fn download_progress_and_completion_update_view_state(cx: &mut TestAppContext) {
    let (view, cx) = cx.add_window_view(|window, cx| AppShell::new(Theme::dark(), window, cx));
    view.update(cx, |shell, cx| {
        shell.configure_updates(true, false, cx);
        let check_id = shell.allocate_request_id();
        shell.pending_update_check = Some(check_id);
        shell.set_update_check_result(
            check_id,
            Ok(Some(crate::UpdateOfferView {
                version: "0.3.0".into(),
                title: "AriaDeck 0.3.0".into(),
                notes: "Release notes".into(),
                release_url: "https://github.com/AirSodaz/AriaDeck/releases/tag/v0.3.0".into(),
                installer_available: true,
            })),
            cx,
        );
        shell.request_update_download(cx);
        let request_id = shell.pending_update_download.expect("download is pending");
        assert!(matches!(
            shell.update_status,
            crate::UpdateStatusView::Downloading {
                downloaded_bytes: 0,
                total_bytes: None,
                ..
            }
        ));
        shell.set_update_download_progress(request_id, 500, Some(1000), cx);
        assert_eq!(
            shell.update_status,
            crate::UpdateStatusView::Downloading {
                version: "0.3.0".into(),
                downloaded_bytes: 500,
                total_bytes: Some(1000),
            }
        );
        shell.set_update_download_result(request_id, Ok("0.3.0".into()), cx);
        assert_eq!(
            shell.update_status,
            crate::UpdateStatusView::Downloaded {
                version: "0.3.0".into()
            }
        );
    });
}

#[gpui::test]
fn bridge_downloads_fill_the_dialog_without_submitting(cx: &mut TestAppContext) {
    let (view, cx) = cx.add_window_view(|window, cx| {
        let mut shell = AppShell::new(Theme::dark(), window, cx);
        shell.snapshot = snapshot(1);
        shell
    });

    view.update_in(cx, |shell, window, cx| {
        assert!(shell.can_open_bridge_downloads());
        let downloads = [bridge_download(
            "https://example.test/file.bin",
            "https://example.test/page",
            Some("session=bridge-secret"),
        )];
        assert_eq!(shell.open_bridge_downloads(&downloads, window, cx), 1);

        let dialog = shell.add_dialog_entity().expect("dialog open").read(cx);
        // D-045 §6: fill, never submit.
        assert!(dialog.pending.is_none());
        assert_eq!(
            dialog.add_input.read(cx).text(),
            "https://example.test/file.bin"
        );
        assert_eq!(
            dialog.add_inputs.referer.read(cx).text(),
            "https://example.test/page"
        );
        // A prefilled cookie must be visible to whoever confirms the add.
        assert!(dialog.advanced_open);
        assert!(dialog.add_inputs.cookie.read(cx).presentation().is_masked());
    });

    view.read_with(cx, |shell, cx| {
        let dialog = shell.add_dialog_entity().expect("dialog open").read(cx);
        let advanced = dialog.collect_advanced_options(cx);
        assert_eq!(
            advanced
                .cookie
                .as_ref()
                .map(|value| value.clone().into_inner()),
            Some("session=bridge-secret".into())
        );
        // The bridge never supplies an output path or HTTP auth (D-001 / D-045 §5).
        assert!(advanced.http_user.is_empty());
        assert!(advanced.http_passwd.is_none());
        assert!(advanced.checksum.is_empty());
        assert!(advanced.headers.is_empty());
        assert!(!format!("{advanced:?}").contains("bridge-secret"));
    });
}

#[gpui::test]
fn bridge_confirms_by_default_and_auto_submits_only_when_opted_in(cx: &mut TestAppContext) {
    let (view, cx) = cx.add_window_view(|window, cx| {
        let mut shell = AppShell::new(Theme::dark(), window, cx);
        shell.snapshot = snapshot(1);
        shell
    });

    view.update_in(cx, |shell, window, cx| {
        // Default: confirm. The dialog fills and waits.
        assert!(shell.bridge_requires_confirmation());
        let downloads = [bridge_download("https://example.test/a.bin", "", None)];
        assert_eq!(shell.accept_bridge_downloads(&downloads, window, cx), 1);
        let dialog = shell.add_dialog_entity().expect("dialog open").read(cx);
        assert!(
            dialog.pending.is_none(),
            "the default path must not submit for the user"
        );
    });

    view.update_in(cx, |shell, window, cx| {
        shell.close_add_download(window, cx);
        shell.settings.browser_bridge.auto_submit = true;
        assert!(!shell.bridge_requires_confirmation());
        let downloads = [bridge_download("https://example.test/b.bin", "", None)];
        assert_eq!(shell.accept_bridge_downloads(&downloads, window, cx), 1);
        let dialog = shell.add_dialog_entity().expect("dialog open").read(cx);
        assert!(
            dialog.pending.is_some(),
            "auto-submit must reach the engine without a confirmation"
        );
    });
}

#[gpui::test]
fn bridge_downloads_consume_only_the_leading_header_group(cx: &mut TestAppContext) {
    let (view, cx) = cx.add_window_view(|window, cx| {
        let mut shell = AppShell::new(Theme::dark(), window, cx);
        shell.snapshot = snapshot(1);
        shell
    });

    view.update_in(cx, |shell, window, cx| {
        let downloads = [
            // Two items agree on headers, and the second repeats the first URL.
            bridge_download("https://example.test/a.bin", "https://example.test/p", None),
            bridge_download("https://example.test/a.bin", "https://example.test/p", None),
            bridge_download("https://example.test/b.bin", "https://example.test/p", None),
            // Different referer: must not inherit the group above.
            bridge_download("https://other.test/c.bin", "https://other.test/p", None),
        ];
        assert_eq!(shell.open_bridge_downloads(&downloads, window, cx), 3);
        let dialog = shell.add_dialog_entity().expect("dialog open").read(cx);
        assert_eq!(
            dialog.add_input.read(cx).text(),
            "https://example.test/a.bin\nhttps://example.test/b.bin"
        );
        assert_eq!(
            dialog.add_inputs.referer.read(cx).text(),
            "https://example.test/p"
        );

        // The dialog is now busy, so the remainder stays queued for the caller.
        assert!(!shell.can_open_bridge_downloads());
        assert_eq!(shell.open_bridge_downloads(&downloads[3..], window, cx), 0);
    });
}

#[gpui::test]
fn theme_applies_only_after_the_matching_save_succeeds(cx: &mut TestAppContext) {
    let initial = SettingsView {
        color_scheme: ColorSchemeView::Dark,
        language: LanguagePreferenceView::default(),
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
        language: LanguagePreferenceView::default(),
        download_directory: "C:/Downloads".into(),
        download_proxy: DownloadProxySettingsView {
            mode: ProxyModeView::Disabled,
            ..DownloadProxySettingsView::default()
        },
        speed_limits: SpeedLimitSettingsView::default(),
        transfer_policy: TransferPolicySettingsView::default(),
        notifications: NotificationSettingsView::default(),
        platform: PlatformSettingsView::default(),
        browser_bridge: BrowserBridgeSettingsView::default(),
        categories: vec![crate::DownloadCategoryView {
            id: "00000000-0000-4000-8000-000000000001".into(),
            name: "General".into(),
            directory: "C:/Downloads".into(),
            extensions: String::new(),
            is_fallback: true,
        }],
        tracker_list: Default::default(),
    };
    let (view, cx) =
        cx.add_window_view(move |window, cx| AppShell::new_with_settings(initial, window, cx));

    view.update_in(cx, |shell, window, cx| {
        shell.open_settings(&OpenSettings, window, cx);
        let panel = shell.settings_panel().expect("settings open").clone();
        panel.update(cx, |panel, cx| {
            panel.select_proxy_mode(ProxyModeView::Manual, cx);
            panel.settings_inputs.all_proxy.update(cx, |input, cx| {
                input.set_value("proxy.example:8080", window, cx);
            });
            panel
                .settings_inputs
                .proxy_username
                .update(cx, |input, cx| input.set_value("proxy-user", window, cx));
            panel
                .settings_inputs
                .proxy_password
                .update(cx, |input, cx| {
                    input.set_value("never-render-this", window, cx)
                });
            panel.submit_proxy_settings(cx);
        });
    });

    view.read_with(cx, |shell, cx| {
        let panel = shell.settings_panel().expect("settings open").read(cx);
        assert!(
            panel
                .settings_inputs
                .proxy_password
                .read(cx)
                .presentation()
                .is_masked()
        );
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
fn escape_priority_closes_popover_then_settings_then_search(cx: &mut TestAppContext) {
    let (view, cx) = cx.add_window_view(|window, cx| {
        let mut shell = AppShell::new(Theme::dark(), window, cx);
        shell.page = AppPage::Settings;
        shell.active_status_popover = Some(StatusBarPopover::Speed);
        shell.search_input.update(cx, |input, cx| {
            input.set_value("archive", window, cx);
        });
        shell
    });

    view.update_in(cx, |shell, window, cx| {
        shell.clear_search(&ClearSearch, window, cx);
    });
    view.read_with(cx, |shell, cx| {
        assert_eq!(shell.active_status_popover, None);
        assert_eq!(shell.page, AppPage::Settings);
        assert_eq!(shell.search_input.read(cx).value(), "archive");
    });

    view.update_in(cx, |shell, window, cx| {
        shell.clear_search(&ClearSearch, window, cx);
    });
    view.read_with(cx, |shell, cx| {
        assert_eq!(shell.page, AppPage::Downloads);
        assert_eq!(shell.search_input.read(cx).value(), "archive");
    });

    view.update_in(cx, |shell, window, cx| {
        shell.clear_search(&ClearSearch, window, cx);
    });
    view.read_with(cx, |shell, cx| {
        assert!(shell.search_input.read(cx).value().is_empty());
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
    view.update_in(cx, |shell, window, cx| {
        let panel = shell.settings_panel().expect("settings open").clone();
        panel.update(cx, |panel, cx| {
            panel
                .settings_inputs
                .download_limit
                .update(cx, |input, cx| {
                    input.set_value("2M", window, cx);
                });
            panel
                .settings_inputs
                .max_concurrent
                .update(cx, |input, cx| {
                    input.set_value("8", window, cx);
                });
        });
    });
    view.update(cx, |shell, cx| {
        let panel = shell.settings_panel().expect("settings open").clone();
        panel.update(cx, |panel, cx| panel.submit_transfers(cx));
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
    view.update_in(cx, |shell, window, cx| {
        let panel = shell.settings_panel().expect("settings open").clone();
        panel.update(cx, |panel, cx| {
            panel
                .settings_inputs
                .download_limit
                .update(cx, |input, cx| {
                    input.set_value("5MB", window, cx);
                });
            panel
                .settings_inputs
                .max_concurrent
                .update(cx, |input, cx| {
                    input.set_value("8", window, cx);
                });
        });
    });
    view.update(cx, |shell, cx| {
        let panel = shell.settings_panel().expect("settings open").clone();
        panel.update(cx, |panel, cx| panel.submit_transfers(cx));
    });
    view.read_with(cx, |shell, cx| {
        assert!(shell.pending_settings_save.is_none());
        let panel = shell.settings_panel().expect("settings open").read(cx);
        let error = panel
            .settings_page
            .error
            .as_ref()
            .expect("invalid speed limit must surface an error");
        assert_eq!(error.code, "settings.invalid_speed_limit");
    });
}

#[gpui::test]
fn task_settings_dialogs_keep_tab_focus_inside_the_modal(cx: &mut TestAppContext) {
    let (view, mut cx) = open_test_shell(cx, |window, cx| {
        let mut shell = AppShell::new(Theme::dark(), window, cx);
        let mut workspace = snapshot(1);
        workspace.tasks[0].status = TaskStatusView::Active;
        shell.selected = Some(workspace.tasks[0].identity.clone());
        shell.snapshot = workspace;
        shell
    });

    // Open speed limit dialog and verify focus starts in download_limit and Tab stays trapped inside.
    view.update_in(&mut cx, |shell, window, cx| {
        shell.open_task_speed_limit(window, cx);
    });
    cx.run_until_parked();
    let _ = cx.update(|window, cx| window.draw(cx));
    assert!(cx.update(|window, cx| window.has_active_dialog(cx)));
    view.update_in(&mut cx, |shell, window, cx| {
        let (_, download_limit, _) = shell
            .task_speed_limit_modal()
            .expect("speed limit dialog open");
        assert!(
            download_limit.focus_handle(cx).is_focused(window),
            "speed limit dialog should focus download_limit"
        );
    });

    // Cycle Tab through the dialog controls; focus must never escape to the workspace behind it.
    for _ in 0..8 {
        cx.simulate_keystrokes("tab");
        cx.run_until_parked();
        view.update_in(&mut cx, |shell, window, _| {
            assert!(
                !shell.focus_handle.is_focused(window),
                "Tab must not escape active speed limit dialog"
            );
        });
    }

    // Escape closes speed limit dialog.
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    view.read_with(&cx, |shell, _| {
        assert!(shell.task_speed_limit_modal().is_none());
    });
    assert!(!cx.update(|window, cx| window.has_active_dialog(cx)));

    // Open task options dialog and verify initial focus & Escape dismissal.
    view.update_in(&mut cx, |shell, window, cx| {
        shell.open_task_options(window, cx);
    });
    cx.run_until_parked();
    let _ = cx.update(|window, cx| window.draw(cx));
    assert!(cx.update(|window, cx| window.has_active_dialog(cx)));
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    view.read_with(&cx, |shell, _| {
        assert!(shell.task_options_modal().is_none());
    });
    assert!(!cx.update(|window, cx| window.has_active_dialog(cx)));

    // Verify core setup onboarding & power countdown also open as kit dialogs and dismiss on Escape.
    view.update_in(&mut cx, |shell, window, cx| {
        shell.open_core_setup_onboarding(window, cx);
    });
    cx.run_until_parked();
    let _ = cx.update(|window, cx| window.draw(cx));
    assert!(cx.update(|window, cx| window.has_active_dialog(cx)));
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    view.read_with(&cx, |shell, _| {
        assert!(!shell.core_setup_onboarding_open());
    });
    assert!(!cx.update(|window, cx| window.has_active_dialog(cx)));

    view.update(&mut cx, |shell, cx| {
        shell.start_power_countdown(CompletionPowerActionView::Sleep, cx);
    });
    cx.run_until_parked();
    let _ = cx.update(|window, cx| window.draw(cx));
    assert!(cx.update(|window, cx| window.has_active_dialog(cx)));
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    view.read_with(&cx, |shell, _| {
        assert!(shell.power_countdown().is_none());
    });
    assert!(!cx.update(|window, cx| window.has_active_dialog(cx)));
}

#[gpui::test]
fn transfer_policy_integrity_toggle_updates_draft(cx: &mut TestAppContext) {
    let (view, cx) = cx.add_window_view(|window, cx| AppShell::new(Theme::dark(), window, cx));
    view.update_in(cx, |shell, window, cx| {
        shell.open_settings(&OpenSettings, window, cx);
        let panel = shell.settings_panel().expect("settings open").clone();
        panel.update(cx, |panel, cx| {
            let before = panel.settings_page.draft_check_integrity;
            panel.toggle_check_integrity(cx);
            assert_ne!(panel.settings_page.draft_check_integrity, before);
            panel.toggle_check_integrity(cx);
            assert_eq!(panel.settings_page.draft_check_integrity, before);
        });
    });
}

#[gpui::test]
fn about_settings_category_has_no_footer_save(cx: &mut TestAppContext) {
    let initial = SettingsView {
        language: LanguagePreferenceView::English,
        ..SettingsView::default()
    };
    let (view, cx) =
        cx.add_window_view(move |window, cx| AppShell::new_with_settings(initial, window, cx));
    view.update_in(cx, |shell, window, cx| {
        shell.open_settings(&OpenSettings, window, cx);
        shell.snapshot.capabilities.version = "1.37.0".into();
        shell.sync_settings_panel(None, cx);
        let panel = shell.settings_panel().expect("settings open").clone();
        panel.update(cx, |panel, cx| {
            panel.select_settings_category(SettingsCategory::About, cx);
            assert_eq!(panel.settings_page.active_category, SettingsCategory::About);
            assert_eq!(panel.t("settings-nav-about"), "About");
            assert_eq!(panel.t("settings-about-version"), "Version");
            // About actions are self-contained: the category has no draft save footer.
            let _ = panel.render_settings_about(cx);
            let footer = panel.render_settings_footer(cx);
            drop(footer);
        });
        assert!(shell.pending_settings_save.is_none());
    });
}

#[gpui::test]
fn settings_import_routes_through_the_normal_save_without_a_password_update(
    cx: &mut TestAppContext,
) {
    let initial = SettingsView {
        language: LanguagePreferenceView::English,
        download_directory: "C:/Downloads".into(),
        ..SettingsView::default()
    };
    let (view, cx) =
        cx.add_window_view(move |window, cx| AppShell::new_with_settings(initial, window, cx));
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
    let imported = SettingsView {
        language: LanguagePreferenceView::ChineseSimplified,
        download_directory: "D:/Imported".into(),
        ..SettingsView::default()
    };

    let (request_id, requested) = view.update(cx, |shell, cx| {
        shell.set_settings_import_result(
            SettingsImportResultView {
                path: "D:/ariadeck-settings.json".into(),
                outcome: SettingsImportOutcomeView::Ready(Box::new(imported.clone())),
            },
            cx,
        );
        let pending = shell
            .pending_settings_save
            .as_ref()
            .expect("import must use settings persistence");
        assert_eq!(pending.source, SettingsSaveSource::Import);
        (pending.request_id, pending.settings.clone())
    });
    let requests = events
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].settings, imported);
    assert_eq!(
        requests[0].proxy_password,
        ProxyPasswordUpdateView::Unchanged
    );
    drop(requests);

    view.update_in(cx, |shell, window, cx| {
        shell.set_settings_save_result(
            SettingsSaveResultView {
                request_id,
                settings: requested,
                outcome: SettingsSaveOutcomeView::Success,
            },
            window,
            cx,
        );
        assert_eq!(shell.settings.download_directory, "D:/Imported");
        assert!(shell.status_notice.as_ref().is_some_and(|notice| {
            notice.message.contains("设置已导入") && !notice.is_error
        }));
    });
}

#[gpui::test]
fn diagnostic_snapshot_redacts_the_active_remote_endpoint(cx: &mut TestAppContext) {
    let (view, cx) = cx.add_window_view(|window, cx| {
        let mut shell = AppShell::new(Theme::dark(), window, cx);
        shell.set_profiles(
            ProfileCatalogView {
                active_profile_id: "remote".into(),
                profiles: vec![ProfileEntryView {
                    profile_id: "remote".into(),
                    name: "Private profile name".into(),
                    kind: ProfileKindView::RemoteRpc,
                    executable: String::new(),
                    download_dir: "C:/private/downloads".into(),
                    endpoint:
                        "wss://user:never-export@rpc.example:6800/jsonrpc?token=private#fragment"
                            .into(),
                    has_secret: true,
                }],
            },
            cx,
        );
        shell.set_snapshot(snapshot(3), cx);
        shell
    });

    view.read_with(cx, |shell, _cx| {
        let diagnostics = shell.diagnostic_snapshot(9);
        assert_eq!(diagnostics.app_version, crate::app_version());
        assert_eq!(
            diagnostics.redacted_rpc_endpoint.as_deref(),
            Some("wss://rpc.example:6800/jsonrpc")
        );
        assert_eq!(diagnostics.profile_kind.as_deref(), Some("Remote RPC"));
        assert_eq!(diagnostics.task_count, Some(3));
        let summary = diagnostics.summary_line();
        assert!(!summary.contains("never-export"));
        assert!(!summary.contains("Private profile name"));
        assert!(!summary.contains("C:/private"));
        assert!(!summary.contains("token=private"));
    });
}

#[gpui::test]
fn settings_import_detaches_a_saved_password_when_proxy_identity_changes(cx: &mut TestAppContext) {
    let initial = SettingsView {
        download_directory: "C:/Downloads".into(),
        download_proxy: DownloadProxySettingsView {
            mode: ProxyModeView::Manual,
            all_proxy: "http://old-proxy.example:8080".into(),
            username: "proxy-user".into(),
            has_password: true,
            ..DownloadProxySettingsView::default()
        },
        ..SettingsView::default()
    };
    let (view, cx) =
        cx.add_window_view(move |window, cx| AppShell::new_with_settings(initial, window, cx));
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
    let imported = SettingsView {
        download_directory: "C:/Downloads".into(),
        download_proxy: DownloadProxySettingsView {
            mode: ProxyModeView::Manual,
            all_proxy: "http://new-proxy.example:8080".into(),
            username: "proxy-user".into(),
            has_password: false,
            ..DownloadProxySettingsView::default()
        },
        ..SettingsView::default()
    };

    view.update(cx, |shell, cx| {
        shell.set_settings_import_result(
            SettingsImportResultView {
                path: "D:/ariadeck-settings.json".into(),
                outcome: SettingsImportOutcomeView::Ready(Box::new(imported)),
            },
            cx,
        );
    });
    let requests = events
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].proxy_password, ProxyPasswordUpdateView::Detach);
}

#[gpui::test]
fn notification_preferences_save_emits_settings_request(cx: &mut TestAppContext) {
    let (view, cx) = cx.add_window_view(|window, cx| AppShell::new(Theme::dark(), window, cx));
    view.update_in(cx, |shell, window, cx| {
        shell.open_settings(&OpenSettings, window, cx);
        let panel = shell.settings_panel().expect("settings open").clone();
        panel.update(cx, |panel, cx| {
            panel.select_notification_volume(NotificationVolumeView::Quiet, cx);
            panel.toggle_notify_on_completion(cx);
            panel.submit_notifications(cx);
        });
    });
    view.read_with(cx, |shell, _| {
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
        language: LanguagePreferenceView::default(),
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
fn language_preference_selection_emits_settings_save_and_switches_catalog(cx: &mut TestAppContext) {
    let initial = SettingsView {
        color_scheme: ColorSchemeView::Dark,
        language: LanguagePreferenceView::English,
        download_directory: "C:/Downloads".into(),
        ..SettingsView::default()
    };
    let (view, cx) =
        cx.add_window_view(move |window, cx| AppShell::new_with_settings(initial, window, cx));
    view.update_in(cx, |shell, window, cx| {
        assert_eq!(shell.t("filter-active"), "Active");
        shell.select_language(LanguagePreferenceView::ChineseSimplified, window, cx);
        let pending = shell
            .pending_settings_save
            .as_ref()
            .expect("language save pending");
        assert_eq!(pending.source, SettingsSaveSource::Language);
        assert_eq!(
            pending.settings.language,
            LanguagePreferenceView::ChineseSimplified
        );
        // Runtime catalog switches immediately.
        assert_eq!(shell.t("filter-active"), "下载中");
        assert_eq!(shell.t("settings-nav-general"), "通用");
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

    view.update_in(cx, |shell, window, cx| {
        shell.request_switch_profile("p1".into(), cx);
        assert!(shell.status_notice.as_ref().is_some_and(|notice| {
            notice.message.contains("already active") || notice.message.contains("已是当前配置")
        }));
        shell.open_settings_page(SettingsCategory::Profiles, window, cx);
        let panel = shell.settings_panel().expect("settings open").clone();
        panel.update(cx, |panel, cx| {
            panel.add_draft_local_profile(window, cx);
        });
    });
    view.update_in(cx, |shell, window, cx| {
        let panel = shell.settings_panel().expect("settings open").clone();
        assert_eq!(shell.profiles.profiles.len(), 3);
        let draft = shell
            .profiles
            .profiles
            .iter()
            .find(|profile| profile.profile_id.starts_with("draft-local-"))
            .expect("local draft")
            .clone();
        assert!(draft.executable.is_empty(), "local draft uses managed core");
        assert_eq!(
            panel.read(cx).settings_page.editing_profile_id.as_deref(),
            Some(draft.profile_id.as_str())
        );
        panel.update(cx, |panel, cx| {
            panel.settings_inputs.profile_name.update(cx, |input, cx| {
                input.set_value("Home NAS-ready", window, cx);
            });
            panel.apply_profile_editor(window, cx);
        });
    });
    view.update_in(cx, |shell, window, cx| {
        let panel = shell.settings_panel().expect("settings open").clone();
        assert!(panel.read(cx).settings_page.editing_profile_id.is_none());
        assert!(
            shell
                .profiles
                .profiles
                .iter()
                .any(|profile| profile.name == "Home NAS-ready")
        );

        panel.update(cx, |panel, cx| {
            panel.add_draft_remote_profile(window, cx);
        });
    });
    view.update_in(cx, |shell, _window, cx| {
        let panel = shell.settings_panel().expect("settings open").clone();
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
        panel.update(cx, |panel, cx| {
            panel.request_remove_profile(remove_id.clone(), cx);
            assert!(panel.settings_page.pending_profile_delete.is_some());
            panel.cancel_remove_profile(cx);
            assert!(panel.settings_page.pending_profile_delete.is_none());
        });
        assert_eq!(shell.profiles.profiles.len(), 4);
        panel.update(cx, |panel, cx| {
            panel.request_remove_profile(remove_id, cx);
            panel.confirm_remove_profile(cx);
        });
    });
    view.update_in(cx, |shell, window, cx| {
        let panel = shell.settings_panel().expect("settings open").clone();
        assert_eq!(shell.profiles.profiles.len(), 3);

        // Remote secret set is staged until Save profiles.
        panel.update(cx, |panel, cx| {
            panel.add_draft_remote_profile(window, cx);
        });
    });
    let remote_id = view.update_in(cx, |shell, window, cx| {
        let panel = shell.settings_panel().expect("settings open").clone();
        let remote_id = shell
            .profiles
            .profiles
            .iter()
            .find(|profile| profile.profile_id.starts_with("draft-remote-"))
            .map(|profile| profile.profile_id.clone())
            .expect("remote draft");
        panel.update(cx, |panel, cx| {
            panel.open_profile_editor(remote_id.clone(), window, cx);
            panel
                .settings_inputs
                .profile_secret
                .update(cx, |input, cx| {
                    input.set_value("s3cret", window, cx);
                });
            panel.apply_profile_editor(window, cx);
        });
        remote_id
    });
    view.update_in(cx, |shell, _window, cx| {
        let panel = shell.settings_panel().expect("settings open").clone();
        assert!(
            panel
                .read(cx)
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
                .is_some_and(|notice| notice.message == shell.t("notice-core-verified"))
        );
    });
}

/// B4: the guide asks the desktop layer to scan, renders what came back, and
/// dismissing it reports that the user was asked so it stops reopening.
#[gpui::test]
fn core_setup_guide_scans_on_open_and_reports_dismissal(cx: &mut TestAppContext) {
    let (view, cx) = cx.add_window_view(|window, cx| AppShell::new(Theme::dark(), window, cx));
    let events = Arc::new(std::sync::Mutex::new(Vec::new()));
    let sink = events.clone();
    let _subscription = view.update(cx, |_, cx| {
        cx.subscribe(&view, move |_, _, event: &AppShellEvent, _| match event {
            AppShellEvent::CoreDiscoveryRequested { .. } => sink
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .push("discovery"),
            AppShellEvent::CoreSetupOnboardingDismissed => sink
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .push("dismissed"),
            _ => {}
        })
    });

    view.update_in(cx, |shell, window, cx| {
        shell.open_core_setup_onboarding(window, cx);
        assert!(shell.core_setup_onboarding_open());
        assert!(shell.core_setup.scanning, "opening must trigger a scan");
    });
    assert_eq!(
        *events
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()),
        vec!["discovery"]
    );

    view.update(cx, |shell, cx| {
        shell.set_core_discovery_result(
            CoreDiscoveryResultView {
                request_id: RequestId::from_u64(1),
                discovered: vec![DiscoveredCoreView {
                    path: "C:/tools/aria2c.exe".into(),
                    version: "1.37.0".into(),
                    origin: CoreDiscoveryOriginView::SearchPath,
                    features: vec!["BitTorrent".into()],
                    already_registered: false,
                }],
            },
            cx,
        );
        assert!(!shell.core_setup.scanning);
        assert!(shell.core_setup.scanned);
        assert_eq!(shell.core_setup.discovered.len(), 1);
    });

    view.update_in(cx, |shell, window, cx| {
        shell.dismiss_core_setup_onboarding(window, cx);
        assert!(!shell.core_setup_onboarding_open());
    });
    assert_eq!(
        *events
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()),
        vec!["discovery", "dismissed"],
        "dismissing must report exactly once so the guide stops reopening"
    );
}

/// The download is user-initiated by construction: no offer means no request,
/// and a request in flight does not queue a second one.
#[gpui::test]
fn core_download_requires_an_offer_and_never_runs_twice(cx: &mut TestAppContext) {
    let (view, cx) = cx.add_window_view(|window, cx| AppShell::new(Theme::dark(), window, cx));
    let requests = Arc::new(std::sync::Mutex::new(0usize));
    let sink = requests.clone();
    let _subscription = view.update(cx, |_, cx| {
        cx.subscribe(&view, move |_, _, event: &AppShellEvent, _| {
            if matches!(event, AppShellEvent::CoreDownloadRequested { .. }) {
                *sink.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) += 1;
            }
        })
    });

    // Platform with nothing published upstream: the button must not fire.
    view.update(cx, |shell, cx| {
        shell.set_core_download_offer(
            CoreDownloadOfferView {
                available: false,
                target: "linux-x86_64".into(),
                ..CoreDownloadOfferView::default()
            },
            cx,
        );
        shell.request_core_download(cx);
        assert!(!shell.core_setup.downloading);
    });
    assert_eq!(
        *requests
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()),
        0
    );

    view.update(cx, |shell, cx| {
        shell.set_core_download_offer(
            CoreDownloadOfferView {
                available: true,
                version: "1.37.0".into(),
                target: "windows-x86_64".into(),
                url: "https://github.example/aria2.zip".into(),
                emulated: false,
            },
            cx,
        );
        shell.request_core_download(cx);
        shell.request_core_download(cx);
        assert!(shell.core_setup.downloading);
    });
    assert_eq!(
        *requests
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()),
        1,
        "a download already in flight must not be queued again"
    );

    view.update(cx, |shell, cx| {
        shell.set_core_download_result(
            CoreDownloadResultView {
                request_id: RequestId::from_u64(1),
                registry: CoreRegistryView {
                    active_id: None,
                    last_working_id: None,
                    installations: vec![CoreInstallationView {
                        id: "c1".into(),
                        version: "1.37.0".into(),
                        target: "windows-x86_64".into(),
                        source: CoreSourceView::Managed,
                        executable: "D:/cores/aria2c.exe".into(),
                        features: Vec::new(),
                        is_active: false,
                        is_last_working: false,
                        validated_version: Some("1.37.0".into()),
                        status: CoreInstallStatusView::Ready,
                    }],
                },
                installed_version: Some("1.37.0".into()),
                outcome: CoreCommandOutcomeView::Success,
            },
            cx,
        );
        assert!(!shell.core_setup.downloading);
        assert_eq!(shell.cores.installations.len(), 1);
        assert!(
            shell
                .status_notice
                .as_ref()
                .is_some_and(|notice| notice.message.contains("1.37.0")),
            "a completed download must say which version landed"
        );
    });
}

/// The guide hands off to Settings → Engine, where the same panel lives, and
/// that hand-off still counts as having been asked.
#[gpui::test]
fn core_setup_guide_hands_off_to_engine_settings(cx: &mut TestAppContext) {
    let (view, cx) = cx.add_window_view(|window, cx| AppShell::new(Theme::dark(), window, cx));

    view.update_in(cx, |shell, window, cx| {
        shell.open_core_setup_onboarding(window, cx);
        shell.open_engine_settings_from_onboarding(window, cx);
        assert!(!shell.core_setup_onboarding_open());
        assert_eq!(shell.page, AppPage::Settings);
        assert_eq!(
            shell
                .settings_panel()
                .expect("settings open")
                .read(cx)
                .settings_page
                .active_category,
            SettingsCategory::Engine
        );
    });
}

/// The guide and Settings → Engine share one panel — including one path input
/// entity — so they must never be mounted in the same frame. Whichever the user
/// is already looking at wins.
#[gpui::test]
fn core_setup_panel_is_never_mounted_in_two_places_at_once(cx: &mut TestAppContext) {
    let (view, cx) = cx.add_window_view(|window, cx| AppShell::new(Theme::dark(), window, cx));

    // Already on Settings: the guide redirects to Engine instead of stacking.
    view.update_in(cx, |shell, window, cx| {
        shell.open_settings(&OpenSettings, window, cx);
        shell.open_core_setup_onboarding(window, cx);
        assert!(!shell.core_setup_onboarding_open());
        assert_eq!(
            shell
                .settings_panel()
                .expect("settings open")
                .read(cx)
                .settings_page
                .active_category,
            SettingsCategory::Engine
        );
        shell.close_settings(window, cx);
    });

    // Guide up: opening Settings is refused until it is dismissed.
    view.update_in(cx, |shell, window, cx| {
        shell.open_core_setup_onboarding(window, cx);
        assert!(shell.core_setup_onboarding_open());
        shell.open_settings(&OpenSettings, window, cx);
        assert_eq!(shell.page, AppPage::Downloads);

        shell.dismiss_core_setup_onboarding(window, cx);
        shell.open_settings(&OpenSettings, window, cx);
        assert_eq!(shell.page, AppPage::Settings);
    });

    // Another modal already owns the screen: the guide waits rather than stacks.
    view.update_in(cx, |shell, window, cx| {
        shell.close_settings(window, cx);
        shell.snapshot = snapshot(1);
        shell.open_add_download(&OpenAddDownload, window, cx);
        shell.open_core_setup_onboarding(window, cx);
        assert!(!shell.core_setup_onboarding_open());
    });
}

#[gpui::test]
fn power_action_change_during_countdown_updates_action(cx: &mut TestAppContext) {
    let (view, cx) = cx.add_window_view(|window, cx| AppShell::new(Theme::dark(), window, cx));

    view.update(cx, |shell, cx| {
        shell.active_modal = Some(ActiveModal::PowerCountdown(PowerCountdownState {
            action: CompletionPowerActionView::Sleep,
            remaining_seconds: 30,
        }));
        shell.select_completion_action(CompletionPowerActionView::Shutdown, cx);
        assert_eq!(
            shell.power_countdown().expect("countdown active").action,
            CompletionPowerActionView::Shutdown
        );
    });
}

#[gpui::test]
fn sort_dropdown_menu_builds_and_updates_query(cx: &mut TestAppContext) {
    let (shell, mut cx) =
        open_test_shell(cx, |window, cx| AppShell::new(Theme::dark(), window, cx));

    shell.update_in(&mut cx, |shell, window, cx| {
        let weak = cx.entity().downgrade();
        let menu = PopupMenu::build(window, cx, |menu, _, _| {
            shell.populate_sort_menu(menu, weak)
        });
        assert!(!menu.read(cx).is_empty());

        shell.set_sort_key(WorkspaceSortKey::Name, cx);
        assert_eq!(shell.query.sort_key, WorkspaceSortKey::Name);

        shell.set_sort_direction(WorkspaceSortDirection::Descending, cx);
        assert_eq!(
            shell.query.sort_direction,
            WorkspaceSortDirection::Descending
        );
    });
}

#[gpui::test]
fn input_placeholders_sync_on_language_switch_and_apply_settings(cx: &mut TestAppContext) {
    let (view, cx) = cx.add_window_view(|window, cx| {
        let settings = SettingsView {
            language: LanguagePreferenceView::English,
            ..Default::default()
        };
        let mut shell = AppShell::new_with_settings(settings, window, cx);
        shell.snapshot = snapshot(1);
        shell.snapshot.tasks[0].status = TaskStatusView::Active;
        shell.selected = Some(shell.snapshot.tasks[0].identity.clone());
        shell
    });

    let en = Translator::new(LocaleId::En);
    let zh = Translator::new(LocaleId::ZhCn);

    view.update_in(cx, |shell, window, cx| {
        shell.open_add_download(&OpenAddDownload, window, cx);
    });
    view.read_with(cx, |shell, cx| {
        let dialog = shell.add_dialog_entity().expect("dialog open").read(cx);
        assert_eq!(
            dialog
                .add_input
                .read(cx)
                .presentation()
                .placeholder()
                .as_ref(),
            en.t("dialog-add-url-placeholder")
        );
        assert_eq!(
            dialog
                .add_inputs
                .user_agent
                .read(cx)
                .presentation()
                .placeholder()
                .as_ref(),
            en.t("dialog-add-user-agent-placeholder")
        );
        assert_eq!(
            dialog
                .add_inputs
                .headers
                .read(cx)
                .presentation()
                .placeholder()
                .as_ref(),
            en.t("dialog-add-headers-placeholder")
        );
        assert_eq!(
            dialog
                .add_inputs
                .http_user
                .read(cx)
                .presentation()
                .placeholder()
                .as_ref(),
            en.t("dialog-add-http-username-placeholder")
        );
        assert_eq!(
            dialog
                .add_inputs
                .http_passwd
                .read(cx)
                .presentation()
                .placeholder()
                .as_ref(),
            en.t("dialog-add-http-password-placeholder")
        );
    });

    // Switch language directly via Settings -> open modal placeholders switch to Chinese.
    view.update_in(cx, |shell, window, cx| {
        shell.select_language(LanguagePreferenceView::ChineseSimplified, window, cx);
    });
    view.read_with(cx, |shell, cx| {
        let dialog = shell.add_dialog_entity().expect("dialog open").read(cx);
        assert_eq!(
            dialog
                .add_input
                .read(cx)
                .presentation()
                .placeholder()
                .as_ref(),
            zh.t("dialog-add-url-placeholder")
        );
        assert_eq!(
            dialog
                .add_inputs
                .user_agent
                .read(cx)
                .presentation()
                .placeholder()
                .as_ref(),
            zh.t("dialog-add-user-agent-placeholder")
        );
        assert_eq!(
            dialog
                .add_inputs
                .headers
                .read(cx)
                .presentation()
                .placeholder()
                .as_ref(),
            zh.t("dialog-add-headers-placeholder")
        );
        assert_eq!(
            dialog
                .add_inputs
                .http_user
                .read(cx)
                .presentation()
                .placeholder()
                .as_ref(),
            zh.t("dialog-add-http-username-placeholder")
        );
        assert_eq!(
            dialog
                .add_inputs
                .http_passwd
                .read(cx)
                .presentation()
                .placeholder()
                .as_ref(),
            zh.t("dialog-add-http-password-placeholder")
        );
    });

    // Opening Settings after switching to Chinese initializes its lazy inputs in Chinese,
    // and switching back to English refreshes open Settings placeholders live.
    view.update_in(cx, |shell, window, cx| {
        shell.close_add_download(window, cx);
        shell.open_settings(&OpenSettings, window, cx);
    });
    view.read_with(cx, |shell, cx| {
        let panel = shell.settings_panel().expect("settings open").read(cx);
        assert_eq!(
            panel
                .settings_inputs
                .profile_name
                .read(cx)
                .presentation()
                .placeholder()
                .as_ref(),
            zh.t("settings-profile-name-placeholder")
        );
    });

    // Lazily opened speed limit modal also inherits the active Chinese locale.
    view.update_in(cx, |shell, window, cx| {
        shell.close_settings(window, cx);
        shell.open_task_speed_limit(window, cx);
    });
    view.read_with(cx, |shell, cx| {
        let (_, download_limit, _) = shell
            .task_speed_limit_modal()
            .expect("speed limit dialog open");
        assert_eq!(
            download_limit
                .read(cx)
                .presentation()
                .placeholder()
                .as_ref(),
            zh.t("input-download-limit-placeholder")
        );
    });

    // Apply settings without a window handle (startup edge case), then open Add Download:
    // opening the dialog must still synchronize placeholders to the new language.
    view.update_in(cx, |shell, window, cx| {
        shell.close_task_speed_limit(window, cx);
        shell.pending_settings_save = None;
        let mut settings = shell.settings.clone();
        settings.language = LanguagePreferenceView::English;
        shell.apply_settings(settings, None, cx);
        shell.open_add_download(&OpenAddDownload, window, cx);
    });
    view.read_with(cx, |shell, cx| {
        let dialog = shell.add_dialog_entity().expect("dialog open").read(cx);
        assert_eq!(
            dialog
                .add_input
                .read(cx)
                .presentation()
                .placeholder()
                .as_ref(),
            en.t("dialog-add-url-placeholder")
        );
        assert_eq!(
            dialog
                .add_inputs
                .http_user
                .read(cx)
                .presentation()
                .placeholder()
                .as_ref(),
            en.t("dialog-add-http-username-placeholder")
        );
    });
}

#[gpui::test]
fn sidebar_scrolls_upper_region_while_keeping_settings_pinned_on_short_window(
    cx: &mut TestAppContext,
) {
    let (_shell, mut visual_cx) =
        open_test_shell_sized(cx, gpui::size(px(900.0), px(360.0)), |window, cx| {
            let settings = SettingsView {
                categories: (0..12)
                    .map(|index| crate::DownloadCategoryView {
                        id: format!("cat-{index}"),
                        name: format!("Category {index}"),
                        directory: format!("D:\\Downloads\\cat-{index}"),
                        extensions: format!("ext{index}"),
                        is_fallback: false,
                    })
                    .collect(),
                ..Default::default()
            };
            let mut shell = AppShell::new_with_settings(settings, window, cx);
            shell.snapshot = snapshot(1);
            shell
        });

    visual_cx.run_until_parked();
    let _ = visual_cx.update(|window, cx| window.draw(cx));

    let header_bounds = visual_cx
        .debug_bounds("sidebar-profile-header")
        .expect("sidebar profile header must be painted");
    let footer_bounds = visual_cx
        .debug_bounds("sidebar-settings-footer")
        .expect("sidebar settings footer must be painted");
    assert!(
        header_bounds.size.height > px(0.0),
        "sidebar header must have positive height"
    );
    assert!(
        footer_bounds.size.height > px(0.0),
        "sidebar footer must have positive height"
    );
    assert!(
        footer_bounds.bottom() <= px(360.0),
        "sidebar settings footer must stay pinned inside the 360px window (bottom={:?})",
        footer_bounds.bottom()
    );
    assert!(
        footer_bounds.top() > header_bounds.bottom(),
        "sidebar scrollable content region must sit between header ({:?}) and pinned footer ({:?})",
        header_bounds.bottom(),
        footer_bounds.top()
    );
}

#[gpui::test]
fn settings_page_and_add_download_dialog_keep_scrollbars_inside_narrow_viewport(
    cx: &mut TestAppContext,
) {
    let viewport_width = px(640.0);
    let viewport_height = px(440.0);
    let (shell, mut visual_cx) = open_test_shell_sized(
        cx,
        gpui::size(viewport_width, viewport_height),
        |window, cx| {
            let mut shell = AppShell::new(Theme::dark(), window, cx);
            shell.snapshot = snapshot(5);
            shell.open_settings(&OpenSettings, window, cx);
            shell
        },
    );

    visual_cx.run_until_parked();
    let _ = visual_cx.update(|window, cx| window.draw(cx));

    shell.read_with(&visual_cx, |shell, cx| {
        let bounds = shell
            .settings_panel()
            .expect("settings open")
            .read(cx)
            .settings_scroll
            .bounds();
        assert!(
            bounds.size.width > px(0.0),
            "settings scroll viewport must have positive width"
        );
        assert!(
            bounds.right() <= viewport_width + px(0.5),
            "settings scroll viewport (and its overlay scrollbar) must stay within narrow viewport (right={:?}, viewport={:?})",
            bounds.right(),
            viewport_width
        );
    });

    // Now open Add Download with advanced options expanded so the dialog fits & scrolls in narrow viewport.
    shell.update_in(&mut visual_cx, |shell, window, cx| {
        shell.open_add_download(&OpenAddDownload, window, cx);
        shell.toggle_add_advanced(cx);
    });
    visual_cx.run_until_parked();
    let _ = visual_cx.update(|window, cx| window.draw(cx));

    assert!(visual_cx.update(|window, cx| window.has_active_dialog(cx)));
    shell.read_with(&visual_cx, |shell, cx| {
        let bounds = shell
            .add_dialog_entity()
            .expect("dialog open")
            .read(cx)
            .add_input
            .read(cx)
            .text_bounds()
            .expect("add download input painted inside kit dialog");
        assert!(
            bounds.size.width > px(0.0) && bounds.size.height > px(0.0),
            "add download input must have positive bounds"
        );
        assert!(
            bounds.right() <= viewport_width + px(0.5),
            "add download dialog must fit inside narrow viewport (right={:?}, viewport={:?})",
            bounds.right(),
            viewport_width
        );
    });
}

#[gpui::test]
fn save_settings_shortcut_submits_active_category_and_general_detects_inline_edits(
    cx: &mut gpui::TestAppContext,
) {
    let (shell, visual_cx) =
        cx.add_window_view(|window, cx| AppShell::new(Theme::dark(), window, cx));
    let mut visual_cx = visual_cx.clone();
    let events: std::rc::Rc<std::cell::RefCell<Vec<AppShellEvent>>> =
        std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    let captured = events.clone();
    visual_cx.update(|_, cx| {
        cx.subscribe(&shell, move |_, event, _| {
            captured.borrow_mut().push(event.clone());
        })
        .detach();
    });

    shell.update_in(&mut visual_cx, |shell, window, cx| {
        let settings = SettingsView {
            categories: vec![crate::DownloadCategoryView {
                id: "general".into(),
                name: "General".into(),
                directory: "C:/Downloads".into(),
                extensions: String::new(),
                is_fallback: true,
            }],
            ..SettingsView::default()
        };
        shell.apply_settings(settings, Some(window), cx);
        shell.open_settings_page(SettingsCategory::General, window, cx);
        // Edit the active General category directory input directly without flushing.
        let panel = shell.settings_panel().expect("settings open").clone();
        panel.update(cx, |panel, cx| {
            panel.select_category_editor(0, window, cx);
            panel.settings_inputs.directory.update(cx, |input, cx| {
                input.set_value("D:/CustomDownloads", window, cx);
            });
            assert_ne!(
                panel.preview_draft_categories(cx),
                panel.settings.categories,
                "inline category edit must mark General page dirty before flush"
            );

            // Switch to Network tab, toggle proxy mode, and press Ctrl/Cmd+Enter (`SaveSettings`).
            panel.settings_page.active_category = SettingsCategory::Network;
            panel.settings_page.draft_proxy_mode = ProxyModeView::System;
        });
        shell.save_settings_action(&SaveSettings, window, cx);
    });

    let emitted = events.borrow();
    assert!(
        emitted.iter().any(|event| matches!(
            event,
            AppShellEvent::SettingsSaveRequested(req)
                if req.settings.download_proxy.mode == ProxyModeView::System
        )),
        "SaveSettings shortcut on Network tab must submit proxy settings, not General categories"
    );
}

#[gpui::test]
fn child_entities_are_created_lazily_on_open_and_dropped_on_close(cx: &mut TestAppContext) {
    let (view, cx) = cx.add_window_view(|window, cx| {
        let mut shell = AppShell::new(Theme::dark(), window, cx);
        shell.snapshot = snapshot(2);
        shell
    });

    // Startup: no settings panel, no add dialog, no task details panel.
    view.read_with(cx, |shell, _| {
        assert!(shell.settings_panel().is_none());
        assert!(shell.add_dialog_entity().is_none());
        assert!(shell.details_panel.is_none());
    });

    // SettingsPanelView: created on open, destroyed on close and on leaving via filter.
    view.update_in(cx, |shell, window, cx| {
        shell.open_settings(&OpenSettings, window, cx);
        assert!(shell.settings_panel().is_some());
        shell.close_settings(window, cx);
        assert!(shell.settings_panel().is_none());

        shell.open_settings(&OpenSettings, window, cx);
        assert!(shell.settings_panel().is_some());
        shell.set_filter(WorkspaceFilter::Active, window, cx);
        assert!(shell.settings_panel().is_none());
    });

    // AddDownloadDialogView: created on open, destroyed on close.
    view.update_in(cx, |shell, window, cx| {
        shell.open_add_download(&OpenAddDownload, window, cx);
        assert!(shell.add_dialog_entity().is_some());
        shell.close_add_download(window, cx);
        assert!(shell.add_dialog_entity().is_none());
    });

    // TaskDetailsPanelView: created on open, destroyed on close.
    view.update_in(cx, |shell, window, cx| {
        let row = shell.snapshot.tasks[0].clone();
        shell.open_details_for(row, cx);
        assert!(shell.details_panel.is_some());
        shell.close_task_details(window, cx);
        assert!(shell.details_panel.is_none());
    });
}

#[gpui::test]
fn newly_localized_notices_and_task_overviews_render_in_chinese(cx: &mut TestAppContext) {
    let settings = SettingsView {
        language: LanguagePreferenceView::ChineseSimplified,
        ..SettingsView::default()
    };
    let (view, cx) =
        cx.add_window_view(move |window, cx| AppShell::new_with_settings(settings, window, cx));

    view.update(cx, |shell, cx| {
        // Profile already-active notice localizes to Chinese.
        shell.set_profiles(
            ProfileCatalogView {
                active_profile_id: "p1".into(),
                profiles: vec![ProfileEntryView {
                    profile_id: "p1".into(),
                    name: "Local".into(),
                    kind: ProfileKindView::LocalManaged,
                    executable: String::new(),
                    download_dir: "D:/Downloads".into(),
                    endpoint: String::new(),
                    has_secret: false,
                }],
            },
            cx,
        );
        shell.request_switch_profile("p1".into(), cx);
        let notice = shell
            .status_notice
            .as_ref()
            .expect("profile notice emitted");
        assert_eq!(notice.message, shell.t("notice-profile-already-active"));
        assert!(
            notice.message.contains("已是当前配置"),
            "expected Chinese profile notice, got: {}",
            notice.message
        );

        // Task display name fallback, built-in category label, and task overview strings localize in Chinese.
        let mut pending_task = task(0);
        pending_task.display_name = "Metadata pending".into();
        pending_task.name_state = TaskNameStateView::Resolving;
        assert_eq!(
            task_display_name(&pending_task, &shell.translator),
            shell.t("task-name-resolving")
        );
        assert_eq!(
            category_display_name("General", &shell.translator),
            shell.t("category-general")
        );
        let remaining = shell.t_args(
            "task-overview-remaining",
            &[("duration", FluentValue::from("5m"))],
        );
        assert!(
            remaining != "task-overview-remaining" && !remaining.contains("remaining"),
            "expected localized Chinese task-overview-remaining, got: {remaining}"
        );
        let _overview_div =
            task_overview_summary(&pending_task, &shell.translator, shell.theme.colors);
    });
}

#[gpui::test]
fn unsupported_platform_capabilities_block_keep_awake_and_bridge_toggles(
    cx: &mut gpui::TestAppContext,
) {
    let (view, cx) = cx.add_window_view(|window, cx| AppShell::new(Theme::dark(), window, cx));
    view.update_in(cx, |shell, window, cx| {
        shell.set_platform_capabilities(
            PlatformCapabilitiesView {
                keep_awake_supported: false,
                browser_bridge_supported: false,
            },
            cx,
        );
        shell.open_settings(&OpenSettings, window, cx);
        let panel = shell.settings_panel().expect("settings panel open").clone();
        panel.update(cx, |panel, cx| {
            panel.select_settings_category(SettingsCategory::System, cx);
            let initial_keep_awake = panel.settings_page.draft_platform.keep_awake_while_active;
            let initial_auto_submit = panel.settings_page.draft_browser_bridge.auto_submit;
            let initial_allow_cookies = panel.settings_page.draft_browser_bridge.allow_cookies;
            panel.toggle_draft_keep_awake(cx);
            panel.toggle_bridge_auto_submit(cx);
            panel.toggle_bridge_allow_cookies(cx);
            assert_eq!(
                panel.settings_page.draft_platform.keep_awake_while_active,
                initial_keep_awake
            );
            assert_eq!(
                panel.settings_page.draft_browser_bridge.auto_submit,
                initial_auto_submit
            );
            assert_eq!(
                panel.settings_page.draft_browser_bridge.allow_cookies,
                initial_allow_cookies
            );
        });

        shell.report_power_action_failed("systemctl exited 1".into(), cx);
        let notice = shell
            .status_notice
            .as_ref()
            .expect("power failure notice emitted");
        assert!(notice.is_error);
        assert!(notice.message.contains("systemctl exited 1"));
    });
}

#[gpui::test]
fn draft_category_ids_stay_unique_after_delete_and_path_pick_tracks_category_id(
    cx: &mut gpui::TestAppContext,
) {
    let initial = SettingsView {
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
        let panel = shell.settings_panel().expect("settings panel open").clone();
        panel.update(cx, |panel, cx| {
            panel.add_download_category(window, cx);
            panel.add_download_category(window, cx);
            assert_eq!(panel.settings_page.draft_categories.len(), 3);
            let first_added_id = panel.settings_page.draft_categories[1].id.clone();
            let second_added_id = panel.settings_page.draft_categories[2].id.clone();
            assert_ne!(first_added_id, second_added_id);

            // Delete the first added category and add another: IDs must remain strictly unique.
            panel.remove_download_category(1, cx);
            panel.add_download_category(window, cx);
            let third_added_id = panel.settings_page.draft_categories[2].id.clone();
            assert_ne!(third_added_id, first_added_id);
            assert_ne!(third_added_id, second_added_id);

            // Simulate the folder picker returning for `third_added_id` even after deleting
            // an earlier category (`index 1` = `second_added_id`), shifting `third_added_id` down.
            panel.select_category_editor(2, window, cx);
            panel.remove_download_category(1, cx);
            assert_eq!(panel.settings_page.draft_categories.len(), 2);
            assert_eq!(panel.settings_page.draft_categories[1].id, third_added_id);
            panel.select_category_editor(1, window, cx);
            panel.apply_picked_path(
                PathPickTarget::CategoryDirectory {
                    category_id: third_added_id.clone(),
                },
                std::path::PathBuf::from("D:/CustomPicked"),
                window,
                cx,
            );
            assert_eq!(
                panel.settings_page.draft_categories[1].directory,
                "D:/CustomPicked"
            );
            assert_eq!(
                panel.settings_inputs.directory.read(cx).value(),
                "D:/CustomPicked"
            );

            // General (fallback) must remain untouched.
            assert_eq!(
                panel.settings_page.draft_categories[0].directory,
                "C:/Downloads"
            );
        });
    });
}

#[gpui::test]
fn settings_category_switch_resets_scroll_offset(cx: &mut TestAppContext) {
    let (shell, mut visual_cx) =
        open_test_shell(cx, |window, cx| AppShell::new(Theme::dark(), window, cx));

    shell.update_in(&mut visual_cx, |shell, window, cx| {
        shell.open_settings(&OpenSettings, window, cx);
    });
    visual_cx.run_until_parked();
    let _ = visual_cx.update(|window, cx| window.draw(cx));

    shell.update(&mut visual_cx, |shell, cx| {
        let panel = shell.settings_panel().expect("settings open").clone();
        panel.update(cx, |panel, cx| {
            panel
                .settings_scroll
                .set_offset(gpui::Point::new(px(0.0), px(-120.0)));
            assert_ne!(panel.settings_scroll.offset().y, px(0.0));
            panel.select_settings_category(SettingsCategory::Network, cx);
            assert_eq!(
                panel.settings_scroll.offset().y,
                px(0.0),
                "switching settings category must reset scroll position to top"
            );
        });
    });
}

#[gpui::test]
fn sidebar_settings_button_toggles_settings_page(cx: &mut TestAppContext) {
    let (shell, mut visual_cx) =
        open_test_shell(cx, |window, cx| AppShell::new(Theme::dark(), window, cx));

    visual_cx.run_until_parked();
    let _ = visual_cx.update(|window, cx| window.draw(cx));

    let settings_item_bounds = visual_cx
        .debug_bounds("sidebar-settings-item")
        .expect("sidebar settings item must be painted");
    assert_eq!(
        settings_item_bounds.size.height,
        px(28.0),
        "sidebar settings item must match SidebarMenuItem height (28px)"
    );
    assert_eq!(
        settings_item_bounds.origin.x,
        px(12.0),
        "sidebar settings item must align with sidebar inner menu items (12px inset)"
    );
    let settings_btn_center = settings_item_bounds.center();

    // First click opens Settings and highlights with sidebar_accent.
    visual_cx.simulate_click(settings_btn_center, gpui::Modifiers::none());
    visual_cx.run_until_parked();
    let _ = visual_cx.update(|window, cx| window.draw(cx));
    shell.read_with(&visual_cx, |shell, _| {
        assert_eq!(shell.page, AppPage::Settings);
        assert!(shell.settings_panel().is_some());
    });
    let accent = shell.read_with(&visual_cx, |shell, _| shell.theme.colors.accent);
    let scale = visual_cx.update(|window, _| window.scale_factor());
    let expected_left = f32::from(settings_item_bounds.origin.x) * scale;
    let expected_top = f32::from(settings_item_bounds.origin.y) * scale;
    let expected_width = f32::from(settings_item_bounds.size.width) * scale;
    let expected_height = f32::from(settings_item_bounds.size.height) * scale;
    let has_sidebar_accent_quad = visual_cx.update(|window, _| {
        window.painted_quads().into_iter().any(|quad| {
            (quad.bounds.origin.x.0 - expected_left).abs() < 1.5
                && (quad.bounds.origin.y.0 - expected_top).abs() < 1.5
                && (quad.bounds.size.width.0 - expected_width).abs() < 1.5
                && (quad.bounds.size.height.0 - expected_height).abs() < 1.5
                && quad.background.as_solid().is_some_and(|bg| {
                    (bg.h - accent.h).abs() < 0.02
                        && (bg.s - accent.s).abs() < 0.02
                        && (bg.l - accent.l).abs() < 0.02
                        && (bg.a - 0.10).abs() < 0.02
                })
        })
    });
    assert!(
        has_sidebar_accent_quad,
        "active sidebar settings item must use sidebar_accent background color"
    );

    // Second click closes Settings and returns to Downloads.
    visual_cx.simulate_click(settings_btn_center, gpui::Modifiers::none());
    visual_cx.run_until_parked();
    let _ = visual_cx.update(|window, cx| window.draw(cx));
    shell.read_with(&visual_cx, |shell, _| {
        assert_eq!(shell.page, AppPage::Downloads);
        assert!(shell.settings_panel().is_none());
    });
}
