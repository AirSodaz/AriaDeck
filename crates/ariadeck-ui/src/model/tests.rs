use super::*;

#[test]
fn settings_request_debug_output_redacts_proxy_password() {
    let request = SettingsSaveRequestView {
        request_id: RequestId::from_u64(1),
        settings: SettingsView::default(),
        proxy_password: ProxyPasswordUpdateView::Set(SecretStringView::new("never-log-this")),
    };

    let debug = format!("{request:?}");
    assert!(debug.contains("[REDACTED]"));
    assert!(!debug.contains("never-log-this"));
}

#[test]
fn compact_transfer_formatting_is_stable_at_unit_boundaries() {
    set_active_format_options(FormatOptions::en());
    assert_eq!(format_bytes(0), "0 B");
    assert_eq!(format_bytes(1_536), "1.5 KiB");
    assert_eq!(format_rate(1_048_576), "1.0 MiB/s");
    assert_eq!(format_eta(Some(3_661)), "1h 1m");
    assert_eq!(format_percent(Some(5_050)), "50.5%");
    assert_eq!(format_relative_time(0), "just now");
    assert_eq!(format_relative_time(120), "2 minutes ago");
}

#[test]
fn format_options_support_comma_decimal_locales() {
    let opts = FormatOptions::comma_decimal();
    assert_eq!(format_bytes_with(1_536, opts), "1,5 KiB");
    assert_eq!(format_rate_with(1_048_576, opts), "1,0 MiB/s");
    assert_eq!(format_percent_with(Some(5_050), opts), "50,5%");
    assert_eq!(format_share_ratio_with(Some(1_250), opts), "1,25");
}

#[test]
fn file_conflict_policy_defaults_to_keep_both() {
    assert_eq!(
        FileConflictPolicyView::default(),
        FileConflictPolicyView::AutoRename
    );
    assert_eq!(FileConflictPolicyView::AutoRename.label(), "Keep both");
    assert_eq!(FileConflictPolicyView::Reject.label(), "Reject");
    assert_eq!(FileConflictPolicyView::Overwrite.label(), "Overwrite");
}

#[test]
fn add_dialog_choices_have_stable_message_keys() {
    assert_eq!(
        AddDownloadInputModeView::Links.message_key(),
        "dialog-add-input-links"
    );
    assert_eq!(
        AddDownloadInputModeView::MetadataFiles.message_key(),
        "dialog-add-input-metadata"
    );
    assert_eq!(
        AddDownloadMetadataKindView::Torrent.message_key(),
        "dialog-add-kind-torrent"
    );
    assert_eq!(
        AddDownloadMetadataKindView::Metalink.message_key(),
        "dialog-add-kind-metalink"
    );
    assert_eq!(
        AddDownloadModeView::SeparateTasks.message_key(),
        "dialog-add-mode-separate"
    );
    assert_eq!(
        AddDownloadModeView::Mirrors.message_key(),
        "dialog-add-mode-mirrors"
    );
    assert_eq!(
        FileConflictPolicyView::AutoRename.message_key(),
        "dialog-add-conflict-keep-both"
    );
    assert_eq!(
        FileConflictPolicyView::Reject.message_key(),
        "dialog-add-conflict-reject"
    );
    assert_eq!(
        FileConflictPolicyView::Overwrite.message_key(),
        "dialog-add-conflict-overwrite"
    );
}

#[test]
fn progress_clamps_overreported_completion() {
    let mut row = DownloadRowView {
        identity: TaskIdentity {
            profile_id: "profile".into(),
            gid: "gid".into(),
        },
        display_name: "archive".into(),
        name_state: TaskNameStateView::Resolved,
        source_kind: TaskSourceKindView::DirectUri,
        primary_source: None,
        directory: None,
        category_id: None,
        followed_by: Vec::new(),
        belongs_to: None,
        status: TaskStatusView::Active,
        error: None,
        total_bytes: 100,
        completed_bytes: 120,
        uploaded_bytes: 250,
        download_rate: 0,
        upload_rate: 0,
        eta_seconds: None,
        observed_seeding_seconds: None,
        revision: 1,
    };

    assert_eq!(row.progress_basis_points(), Some(10_000));
    assert_eq!(row.share_ratio_milli(), Some(2_500));
    assert_eq!(format_share_ratio(row.share_ratio_milli()), "2.50");
    assert!(row.can_set_output_name());

    row.source_kind = TaskSourceKindView::Magnet;
    assert!(!row.can_set_output_name());
    row.source_kind = TaskSourceKindView::DirectUri;
    row.status = TaskStatusView::Complete;
    assert!(!row.can_set_output_name());
}

#[test]
fn retry_is_available_only_for_failed_tasks() {
    assert!(TaskStatusView::Failed.can_retry());
    assert!(!TaskStatusView::Paused.can_retry());
    assert!(!TaskStatusView::Complete.can_retry());
}

#[test]
fn seeding_keeps_live_task_controls_and_active_connections() {
    assert!(TaskStatusView::Seeding.can_pause());
    assert!(TaskStatusView::Seeding.can_move_in_queue());
    assert!(TaskStatusView::Seeding.can_set_speed_limit());
    assert!(TaskStatusView::Seeding.uses_active_connections());
    assert!(!TaskStatusView::Seeding.is_terminal());
}

#[test]
fn remove_is_available_for_live_tasks_and_stopped_results() {
    assert!(TaskStatusView::Active.can_remove());
    assert!(TaskStatusView::Complete.can_remove());
    assert!(TaskStatusView::Failed.can_remove());
    assert!(TaskStatusView::Removed.can_remove());
    assert!(!TaskStatusView::Unknown.can_remove());
}

#[test]
fn speed_limit_field_parses_bytes_and_ki_mi_gi_suffixes() {
    assert_eq!(parse_speed_limit_field(""), Some(0));
    assert_eq!(parse_speed_limit_field("   "), Some(0));
    assert_eq!(parse_speed_limit_field("0"), Some(0));
    assert_eq!(parse_speed_limit_field("1048576"), Some(1_048_576));
    assert_eq!(parse_speed_limit_field("512K"), Some(512 * 1024));
    assert_eq!(parse_speed_limit_field("512k"), Some(512 * 1024));
    assert_eq!(parse_speed_limit_field("2M"), Some(2 * 1024 * 1024));
    assert_eq!(
        parse_speed_limit_field(" 3g "),
        Some(3 * 1024 * 1024 * 1024)
    );
    assert_eq!(parse_speed_limit_field("10 M"), Some(10 * 1024 * 1024));
}

#[test]
fn speed_limit_field_rejects_malformed_and_overflowing_values() {
    assert_eq!(parse_speed_limit_field("abc"), None);
    assert_eq!(parse_speed_limit_field("K"), None);
    assert_eq!(parse_speed_limit_field("1.5M"), None);
    assert_eq!(parse_speed_limit_field("-5"), None);
    assert_eq!(parse_speed_limit_field("5MB"), None);
    // 18 EiB * 1024 overflows u64.
    assert_eq!(parse_speed_limit_field("18446744073709551615K"), None);
}

#[test]
fn speed_limit_field_round_trips_through_the_compact_formatter() {
    assert_eq!(format_speed_limit_field(0), "");
    assert_eq!(format_speed_limit_field(2 * 1024 * 1024), "2M");
    assert_eq!(format_speed_limit_field(512 * 1024), "512K");
    assert_eq!(format_speed_limit_field(3 * 1024 * 1024 * 1024), "3G");
    // Not a clean unit multiple stays in raw bytes.
    assert_eq!(format_speed_limit_field(1_500_000), "1500000");
    for value in [0, 100, 1024, 2 * 1024 * 1024, 5 * 1024 * 1024 * 1024] {
        assert_eq!(
            parse_speed_limit_field(&format_speed_limit_field(value)),
            Some(value)
        );
    }
}

#[test]
fn transfer_policy_view_parses_counts_and_size_with_aria2_ranges() {
    let mut view = TransferPolicySettingsView::default();
    assert!(view.is_valid());
    assert_eq!(view.parse_max_concurrent_downloads(), Some(5));
    assert_eq!(view.parse_max_connection_per_server(), Some(1));
    assert_eq!(view.parse_split(), Some(5));
    assert_eq!(view.parse_min_split_size(), Some(20 * 1024 * 1024));

    view.max_connection_per_server = "16".into();
    assert_eq!(view.parse_max_connection_per_server(), Some(16));
    view.max_connection_per_server = "17".into();
    assert_eq!(view.parse_max_connection_per_server(), None);
    view.max_connection_per_server = "0".into();
    assert_eq!(view.parse_max_connection_per_server(), None);

    view.min_split_size = "1M".into();
    assert_eq!(view.parse_min_split_size(), Some(1024 * 1024));
    view.min_split_size = "".into();
    assert_eq!(view.parse_min_split_size(), None);
    view.min_split_size = "0".into();
    assert_eq!(view.parse_min_split_size(), None);
}

#[test]
fn operation_error_codes_localize_in_chinese_without_english_fallback() {
    let translator = crate::Translator::new(crate::LocaleId::ZhCn);
    let fallback = "FALLBACK_ENGLISH_SUMMARY";
    for code in [
        "validation.invalid_request",
        "validation.duplicate_task",
        "validation.unsupported_metadata_file",
        "validation.invalid_metadata",
        "validation.invalid_output_name",
        "validation.invalid_speed_limit",
        "validation.invalid_seed_ratio",
        "validation.invalid_seed_time",
        "validation.empty_task_options",
        "command.wrong_profile",
        "command.stale_session",
        "command.task_changed",
        "command.seed_rules_unsupported",
        "rpc.disconnected",
        "rpc.command_outcome_unknown",
        "rpc.add_not_observed",
        "rpc.retry_not_observed",
        "rpc.remove_not_observed",
        "rpc.authentication_failed",
        "rpc.timeout",
        "rpc.command_rejected",
        "command.unsupported",
        "filesystem.unsafe_path",
        "filesystem.operation_failed",
        "application.internal",
        "sync.unavailable",
        "command.no_result",
        "settings.invalid_download_directory",
        "settings.invalid_speed_limit",
        "settings.invalid_transfer_policy",
        "settings.path_picker_failed",
        "settings.path_picker_closed",
        "settings.save_failed",
        "settings.export_failed",
        "settings.import_failed",
        "settings.import_busy",
        "profile.switch_failed",
        "profile.save_failed",
        "core.command_failed",
        "diagnostics.export_failed",
    ] {
        let error = OperationErrorView {
            code: code.into(),
            summary: fallback.into(),
            retryable: false,
        };
        let localized = error.localized_summary(&translator);
        assert_ne!(
            localized,
            fallback,
            "error code {code} (key {}) fell back to English summary",
            error.message_key()
        );
    }
}
