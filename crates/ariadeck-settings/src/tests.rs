use std::{
    fs,
    path::{Path, PathBuf},
};

use super::*;

fn settings(root: &Path) -> AppSettings {
    AppSettings {
        color_scheme: ColorScheme::Light,
        language: LanguagePreference::default(),
        download_directory: root.join("downloads"),
        download_proxy: DownloadProxySettings::default(),
        speed_limits: SpeedLimitSettings::default(),
        transfer_policy: TransferPolicySettings::default(),
        notifications: NotificationSettings::default(),
        platform: PlatformSettings::default(),
        browser_bridge: BrowserBridgeSettings::default(),
        onboarding: OnboardingSettings::default(),
        ui: UiPreferences::default(),
        categories: default_download_categories(root.join("downloads")),
        tracker_list: TrackerListSettings::default(),
        header_presets: Vec::new(),
        mirror_rules: MirrorRewriteSettings::default(),
    }
}

#[test]
fn initializes_and_round_trips_a_versioned_document() {
    let root = tempfile::tempdir().expect("temporary directory");
    let store = JsonSettingsStore::new(root.path().join("settings.json"));
    let expected = settings(root.path());

    let loaded = store
        .load_or_initialize(&expected)
        .expect("initialize settings");
    assert_eq!(loaded.settings, expected);
    assert!(loaded.recovery.is_none());
    assert_eq!(store.load().expect("load settings"), expected);

    let document = fs::read_to_string(store.path()).expect("read settings JSON");
    assert!(document.contains(&format!(
        "\"schema_version\": {CURRENT_SETTINGS_SCHEMA_VERSION}"
    )));
    assert!(document.contains("\"transfer_policy\""));
    assert!(document.contains("\"notifications\""));
    assert!(document.contains("\"platform\""));
    assert!(document.contains("\"os_notifications\""));
    assert!(document.contains("\"ui\""));
    assert!(document.ends_with('\n'));
}

#[test]
fn ui_preferences_and_system_theme_round_trip() {
    let root = tempfile::tempdir().expect("temporary directory");
    let store = JsonSettingsStore::new(root.path().join("settings.json"));
    let mut expected = settings(root.path());
    expected.color_scheme = ColorScheme::System;
    expected.ui = UiPreferences {
        list_filter: ListFilterPreference::Completed,
        list_sort_key: ListSortKeyPreference::Size,
        list_sort_direction: ListSortDirectionPreference::Descending,
        auto_probe_on_add: true,
    };

    store.save(&expected).expect("save ui preferences");
    assert_eq!(store.load().expect("load ui preferences"), expected);
}

#[test]
fn window_geometry_round_trips_and_sanitizes() {
    let root = tempfile::tempdir().expect("temporary directory");
    let store = JsonWindowGeometryStore::new(root.path().join("window.json"));
    assert!(store.load().is_none());

    let geometry = WindowGeometry {
        x: 120.0,
        y: 80.0,
        width: 1400.0,
        height: 900.0,
        maximized: true,
    };
    store.save(geometry).expect("save window geometry");
    assert_eq!(store.load(), Some(geometry));

    let tiny = WindowGeometry {
        x: 0.0,
        y: 0.0,
        width: 10.0,
        height: 10.0,
        maximized: false,
    };
    store.save(tiny).expect("save tiny geometry");
    let restored = store.load().expect("load sanitized geometry");
    assert_eq!(restored.width, WINDOW_MIN_WIDTH);
    assert_eq!(restored.height, WINDOW_MIN_HEIGHT);
}

#[test]
fn platform_settings_default_keep_awake_and_action() {
    let defaults = PlatformSettings::default();
    assert!(defaults.keep_awake_while_active);
    assert!(!defaults.watch_clipboard);
    assert_eq!(defaults.action_after_done, CompletionPowerAction::None);
    assert!(!defaults.action_after_done.is_armed());
    assert_eq!(defaults.action_after_done.as_str(), "none");
    assert!(CompletionPowerAction::Sleep.is_armed());
    assert!(CompletionPowerAction::Hibernate.is_armed());
    assert!(CompletionPowerAction::Shutdown.is_armed());
}

#[test]
fn platform_settings_round_trip() {
    let root = tempfile::tempdir().expect("temporary directory");
    let store = JsonSettingsStore::new(root.path().join("settings.json"));
    let mut expected = settings(root.path());
    expected.platform = PlatformSettings {
        close_behavior: CloseBehavior::Quit,
        show_tray_icon: false,
        start_minimized_to_tray: true,
        keep_awake_while_active: false,
        action_after_done: CompletionPowerAction::Shutdown,
        watch_clipboard: true,
    };
    expected.notifications.os_notifications = false;
    expected.notifications.notify_on_low_disk = false;
    expected.notifications.low_disk_threshold_bytes = 512 * 1024 * 1024;

    store.save(&expected).expect("save platform settings");
    assert_eq!(store.load().expect("load platform settings"), expected);
}

#[test]
fn platform_settings_backward_compatibility_when_fields_missing() {
    let root = tempfile::tempdir().expect("temporary directory");
    let store = JsonSettingsStore::new(root.path().join("settings.json"));
    let legacy_json = concat!(
        r#"{"schema_version":1,"color_scheme":"dark","language":"system","download_directory":"downloads","#,
        r#""download_proxy":{"mode":"disabled","all_proxy":null,"http_proxy":null,"https_proxy":null,"ftp_proxy":null,"no_proxy":[],"username":null,"credential":null,"check_certificate":true},"#,
        r#""speed_limits":{"download_limit":0,"upload_limit":0},"#,
        r#""transfer_policy":{"max_concurrent_downloads":5,"max_connection_per_server":1,"split":5,"min_split_size":20971520,"file_allocation":"prealloc","check_integrity":false},"#,
        r#""notifications":{"volume":"normal","notify_on_completion":true,"notify_on_error":true,"notify_on_engine_events":true,"os_notifications":false,"notify_on_low_disk":true,"low_disk_threshold_bytes":1073741824},"#,
        r#""platform":{"close_behavior":"minimize_to_tray","show_tray_icon":true,"start_minimized_to_tray":false},"#,
        r#""ui":{"list_filter":"all","list_sort_key":"queue","list_sort_direction":"ascending"},"#,
        r#""categories":[{"id":"00000000-0000-4000-8000-000000000001","name":"General","directory":"downloads","extensions":[],"is_fallback":true}],"#,
        r#""tracker_list":{"enabled":false,"source":"curated","custom_url":null,"auto_refresh":false,"last_refreshed_at":null,"list_text":""}}"#
    );
    std::fs::write(store.path(), legacy_json).expect("seed legacy settings");
    let loaded = store.load().expect("load legacy settings");
    assert!(loaded.platform.keep_awake_while_active);
    assert!(!loaded.platform.watch_clipboard);
    assert_eq!(
        loaded.platform.action_after_done,
        CompletionPowerAction::None
    );
}

#[test]
fn platform_settings_export_sanitizes_armed_action() {
    let root = tempfile::tempdir().expect("temporary directory");
    let mut source = settings(root.path());
    source.platform.action_after_done = CompletionPowerAction::Shutdown;
    source.platform.watch_clipboard = true;
    let exported = export_settings_json(&source).expect("export settings");
    assert!(
        exported.contains(r#""action_after_done": "none""#),
        "{exported}"
    );
    let imported =
        import_settings_json(&exported, &settings(root.path())).expect("import settings");
    assert!(imported.platform.watch_clipboard);

    let mut legacy_export: serde_json::Value =
        serde_json::from_str(&exported).expect("parse exported settings");
    legacy_export["platform"]
        .as_object_mut()
        .expect("platform obj")
        .remove("watch_clipboard");
    let legacy_imported = import_settings_json(
        &serde_json::to_string(&legacy_export).expect("serialize legacy export"),
        &settings(root.path()),
    )
    .expect("legacy export without watch_clipboard imports");
    assert!(!legacy_imported.platform.watch_clipboard);
}
#[test]
fn browser_bridge_settings_default_off_and_round_trip() {
    assert_eq!(
        BrowserBridgeSettings::default(),
        BrowserBridgeSettings {
            allow_cookies: false,
            auto_submit: false,
        },
        "the bridge's safe configuration must be the no-decision default"
    );

    let root = tempfile::tempdir().expect("temporary directory");
    let store = JsonSettingsStore::new(root.path().join("settings.json"));
    let mut expected = settings(root.path());
    expected.browser_bridge = BrowserBridgeSettings {
        allow_cookies: true,
        auto_submit: true,
    };

    store.save(&expected).expect("save bridge settings");
    assert_eq!(store.load().expect("load bridge settings"), expected);
    // The host reads this key directly out of settings.json, so its name and
    // shape are part of the D-045 contract, not an internal detail.
    let document = fs::read_to_string(store.path()).expect("read settings JSON");
    assert!(
        document.contains(r#""browser_bridge""#) && document.contains(r#""allow_cookies""#),
        "{document}"
    );
}

/// A settings file written before D-045 has no `browser_bridge` key. It must
/// load with the all-off default, not trip the corrupt-document recovery
/// path, which would reset every other preference the user had set.
#[test]
fn settings_written_before_the_bridge_existed_still_load() {
    let root = tempfile::tempdir().expect("temporary directory");
    let store = JsonSettingsStore::new(root.path().join("settings.json"));
    let mut expected = settings(root.path());
    expected.platform.close_behavior = CloseBehavior::Quit;
    store.save(&expected).expect("save current settings");

    let document = fs::read_to_string(store.path()).expect("read settings JSON");
    let mut value: serde_json::Value =
        serde_json::from_str(&document).expect("settings JSON parses");
    value
        .as_object_mut()
        .expect("settings document is an object")
        .remove("browser_bridge")
        .expect("browser_bridge was written");
    fs::write(store.path(), value.to_string()).expect("seed pre-D-045 settings");

    let loaded = store
        .load_or_initialize(&expected)
        .expect("pre-D-045 settings load");
    assert!(
        loaded.recovery.is_none(),
        "an additive field must not look like corruption"
    );
    assert_eq!(
        loaded.settings.browser_bridge,
        BrowserBridgeSettings::default()
    );
    assert_eq!(loaded.settings.platform.close_behavior, CloseBehavior::Quit);
}

/// 搂5: importing a settings document is an explicit action, but it is not an
/// explicit decision about forwarding cookies. `auto_submit` travels;
/// `allow_cookies` is re-decided per machine.
#[test]
fn exported_settings_carry_auto_submit_but_never_the_cookie_opt_in() {
    let root = tempfile::tempdir().expect("temporary directory");
    let mut source = settings(root.path());
    source.browser_bridge = BrowserBridgeSettings {
        allow_cookies: true,
        auto_submit: true,
    };

    let document = export_settings_json(&source).expect("export settings");
    assert!(!document.contains("allow_cookies"), "{document}");
    assert!(document.contains("auto_submit"), "{document}");

    let current = settings(root.path());
    let imported = import_settings_json(&document, &current).expect("import settings");
    assert!(imported.browser_bridge.auto_submit);
    assert!(
        !imported.browser_bridge.allow_cookies,
        "an import must never turn cookie forwarding on"
    );
}

#[test]
fn onboarding_defaults_to_not_yet_dismissed_and_round_trips() {
    assert_eq!(
        OnboardingSettings::default(),
        OnboardingSettings {
            core_setup_dismissed: false,
        },
        "a fresh install has not been through the core-setup guide"
    );

    let root = tempfile::tempdir().expect("temporary directory");
    let store = JsonSettingsStore::new(root.path().join("settings.json"));
    let mut expected = settings(root.path());
    expected.onboarding.core_setup_dismissed = true;

    store.save(&expected).expect("save onboarding settings");
    assert_eq!(store.load().expect("load onboarding settings"), expected);
}

/// Same additive contract as `browser_bridge`: a settings file written before
/// B4 has no `onboarding` key, and must load rather than trip recovery.
#[test]
fn settings_written_before_onboarding_existed_still_load() {
    let root = tempfile::tempdir().expect("temporary directory");
    let store = JsonSettingsStore::new(root.path().join("settings.json"));
    let mut expected = settings(root.path());
    expected.platform.close_behavior = CloseBehavior::Quit;
    store.save(&expected).expect("save current settings");

    let document = fs::read_to_string(store.path()).expect("read settings JSON");
    let mut value: serde_json::Value =
        serde_json::from_str(&document).expect("settings JSON parses");
    value
        .as_object_mut()
        .expect("settings document is an object")
        .remove("onboarding")
        .expect("onboarding was written");
    fs::write(store.path(), value.to_string()).expect("seed pre-B4 settings");

    let loaded = store
        .load_or_initialize(&expected)
        .expect("pre-B4 settings load");
    assert!(
        loaded.recovery.is_none(),
        "an additive field must not look like corruption"
    );
    assert_eq!(loaded.settings.onboarding, OnboardingSettings::default());
    assert_eq!(loaded.settings.platform.close_behavior, CloseBehavior::Quit);
}

/// Importing someone else's settings says nothing about whether *this*
/// installation has been guided through core setup, so the flag stays local.
#[test]
fn imported_settings_never_carry_onboarding_state() {
    let root = tempfile::tempdir().expect("temporary directory");
    let mut source = settings(root.path());
    source.onboarding.core_setup_dismissed = true;

    let document = export_settings_json(&source).expect("export settings");
    assert!(!document.contains("onboarding"), "{document}");

    let current = settings(root.path());
    let imported = import_settings_json(&document, &current).expect("import settings");
    assert!(
        !imported.onboarding.core_setup_dismissed,
        "an import must not claim the local first run already happened"
    );

    let mut already_dismissed = settings(root.path());
    already_dismissed.onboarding.core_setup_dismissed = true;
    let imported = import_settings_json(&document, &already_dismissed).expect("import settings");
    assert!(
        imported.onboarding.core_setup_dismissed,
        "an import must not re-arm a guide the user already finished"
    );
}

#[test]
fn manual_proxy_round_trips_without_a_password_field() {
    let root = tempfile::tempdir().expect("temporary directory");
    let store = JsonSettingsStore::new(root.path().join("settings.json"));
    let mut expected = settings(root.path());
    expected.download_proxy = DownloadProxySettings {
        mode: DownloadProxyMode::Manual,
        all_proxy: Some("http://proxy.example:8080".into()),
        http_proxy: None,
        https_proxy: Some("secure-proxy.example:8443".into()),
        ftp_proxy: None,
        no_proxy: vec!["localhost".into(), "10.0.0.0/8".into()],
        username: Some("proxy-user".into()),
        credential: Some(ProxyCredentialRef::new()),
        check_certificate: true,
    };

    store.save(&expected).expect("save proxy settings");

    assert_eq!(store.load().expect("load proxy settings"), expected);
    let document = fs::read_to_string(store.path()).expect("read proxy settings JSON");
    assert!(!document.to_ascii_lowercase().contains("password"));
    assert!(document.contains("proxy.example:8080"));
    assert!(document.contains("proxy-user"));
}

#[test]
fn proxy_validation_rejects_ambiguous_or_secret_bearing_values() {
    let mut settings = AppSettings::new("downloads");
    settings.download_proxy.mode = DownloadProxyMode::Manual;
    assert!(matches!(
        settings.validate(),
        Err(SettingsError::MissingManualProxyEndpoint)
    ));

    settings.download_proxy.all_proxy = Some("http://user:secret@proxy.example:8080".into());
    assert!(matches!(
        settings.validate(),
        Err(SettingsError::InvalidProxyEndpoint { .. })
    ));

    settings.download_proxy.all_proxy = Some("proxy.example:8080".into());
    settings.download_proxy.no_proxy = vec!["https://localhost".into()];
    assert!(matches!(
        settings.validate(),
        Err(SettingsError::InvalidNoProxyEntry { .. })
    ));
}

#[test]
fn malformed_document_is_preserved_before_defaults_are_restored() {
    let root = tempfile::tempdir().expect("temporary directory");
    let store = JsonSettingsStore::new(root.path().join("settings.json"));
    fs::write(store.path(), b"{not-json").expect("seed malformed settings");
    let defaults = settings(root.path());

    let loaded = store
        .load_or_initialize(&defaults)
        .expect("recover settings");
    let recovery = loaded.recovery.expect("recovery metadata");
    assert_eq!(
        fs::read(recovery.backup_path).expect("read backup"),
        b"{not-json"
    );
    assert_eq!(store.load().expect("load restored defaults"), defaults);
}

#[test]
fn invalid_proxy_document_is_preserved_before_defaults_are_restored() {
    let root = tempfile::tempdir().expect("temporary directory");
    let store = JsonSettingsStore::new(root.path().join("settings.json"));
    // Current schema, but manual proxy with no endpoints 鈫?validation recovery.
    let invalid = concat!(
        r#"{"schema_version":1,"color_scheme":"dark","language":"system","download_directory":"downloads","#,
        r#""download_proxy":{"mode":"manual","all_proxy":null,"http_proxy":null,"https_proxy":null,"ftp_proxy":null,"no_proxy":[],"username":null,"credential":null,"check_certificate":true},"#,
        r#""speed_limits":{"download_limit":0,"upload_limit":0},"#,
        r#""transfer_policy":{"max_concurrent_downloads":5,"max_connection_per_server":1,"split":5,"min_split_size":20971520,"file_allocation":"prealloc","check_integrity":false},"#,
        r#""notifications":{"volume":"normal","notify_on_completion":true,"notify_on_error":true,"notify_on_engine_events":true,"os_notifications":false,"notify_on_low_disk":true,"low_disk_threshold_bytes":1073741824},"#,
        r#""platform":{"close_behavior":"minimize_to_tray","show_tray_icon":true,"start_minimized_to_tray":false},"#,
        r#""ui":{"list_filter":"all","list_sort_key":"queue","list_sort_direction":"ascending"},"#,
        r#""categories":[{"id":"00000000-0000-4000-8000-000000000001","name":"General","directory":"downloads","extensions":[],"is_fallback":true}],"#,
        r#""tracker_list":{"enabled":false,"source":"curated","custom_url":null,"auto_refresh":false,"last_refreshed_at":null,"list_text":""}}"#
    );
    fs::write(store.path(), invalid).expect("seed invalid proxy settings");
    let defaults = settings(root.path());

    let loaded = store
        .load_or_initialize(&defaults)
        .expect("recover invalid proxy settings");
    let recovery = loaded.recovery.expect("recovery metadata");
    assert_eq!(
        fs::read_to_string(recovery.backup_path).expect("read backup"),
        invalid
    );
    assert_eq!(loaded.settings, defaults);
    assert_eq!(store.load().expect("load restored defaults"), defaults);
}

#[test]
fn future_schema_is_rejected_without_replacing_the_document() {
    let root = tempfile::tempdir().expect("temporary directory");
    let store = JsonSettingsStore::new(root.path().join("settings.json"));
    let future = format!(
        "{{\"schema_version\":{},\"color_scheme\":\"dark\",\"download_directory\":\"downloads\"}}",
        CURRENT_SETTINGS_SCHEMA_VERSION + 1
    );
    fs::write(store.path(), &future).expect("seed future settings");

    assert!(matches!(
        store.load_or_initialize(&settings(root.path())),
        Err(SettingsError::UnsupportedSchemaVersion { .. })
    ));
    assert_eq!(
        fs::read_to_string(store.path()).expect("read future JSON"),
        future
    );
}

#[test]
fn download_categories_validate_and_round_trip() {
    let root = tempfile::tempdir().expect("temporary directory");
    let mut expected = settings(root.path());
    let movies = DownloadCategory::new("Movies", root.path().join("movies"))
        .with_extensions(["mp4", "mkv"])
        .fallback();
    let music = DownloadCategory::new("Music", root.path().join("music")).with_extensions(["mp3"]);
    expected.categories = vec![movies.clone(), music];
    expected.download_directory = movies.directory.clone();
    let store = JsonSettingsStore::new(root.path().join("settings.json"));
    store.save(&expected).expect("save");
    let loaded = store.load().expect("load");
    assert_eq!(loaded.categories.len(), 2);
    assert!(loaded.categories[0].is_fallback);
    assert_eq!(loaded.categories[0].name, "Movies");
    assert_eq!(loaded.categories[0].extensions, vec!["mp4", "mkv"]);
    assert_eq!(
        resolve_download_category(&loaded.categories, Some("clip.mp4")).map(|c| c.name.as_str()),
        Some("Movies")
    );

    expected.categories[0].name = " ".into();
    assert!(matches!(
        expected.validate(),
        Err(SettingsError::EmptyCategoryName)
    ));
}

#[test]
fn portable_settings_round_trip_omits_credentials_and_preserves_current_keychain_ref() {
    let root = tempfile::tempdir().expect("temporary directory");
    let mut expected = settings(root.path());
    let exported_credential = ProxyCredentialRef::new();
    expected.download_proxy = DownloadProxySettings {
        mode: DownloadProxyMode::Manual,
        all_proxy: Some("http://proxy.example:8080".into()),
        username: Some("proxy-user".into()),
        credential: Some(exported_credential),
        ..DownloadProxySettings::default()
    };
    let payload = export_settings_json(&expected).expect("export settings");
    assert!(!payload.contains("credential"));
    assert!(!payload.contains(&exported_credential.as_uuid().to_string()));
    assert!(payload.contains("proxy-user"));

    let current_credential = ProxyCredentialRef::new();
    let mut current = expected.clone();
    current.download_proxy.credential = Some(current_credential);
    let imported = import_settings_json(&payload, &current).expect("import settings");
    assert_eq!(imported.download_directory, expected.download_directory);
    assert_eq!(
        imported.download_proxy.all_proxy,
        expected.download_proxy.all_proxy
    );
    assert_eq!(
        imported.download_proxy.username,
        expected.download_proxy.username
    );
    assert_eq!(imported.download_proxy.credential, Some(current_credential));

    current.download_proxy.all_proxy = Some("http://different.example:8080".into());
    let imported = import_settings_json(&payload, &current).expect("import changed proxy");
    assert!(imported.download_proxy.credential.is_none());
}

#[test]
fn portable_settings_reject_unknown_fields_and_future_export_versions() {
    let root = tempfile::tempdir().expect("temporary directory");
    let current = settings(root.path());
    let payload = export_settings_json(&current).expect("export settings");
    let mut document: serde_json::Value = serde_json::from_str(&payload).expect("parse export");
    document["unexpected"] = serde_json::json!(true);
    let unknown = serde_json::to_string(&document).expect("serialize unknown field");
    assert!(matches!(
        import_settings_json(&unknown, &current),
        Err(SettingsError::MalformedExport { .. })
    ));

    let mut document: serde_json::Value = serde_json::from_str(&payload).expect("parse export");
    document["export_version"] = serde_json::json!(SETTINGS_EXPORT_FORMAT_VERSION + 1);
    let future = serde_json::to_string(&document).expect("serialize future export");
    assert!(matches!(
        import_settings_json(&future, &current),
        Err(SettingsError::UnsupportedExportVersion { .. })
    ));
}

#[test]
fn tracker_list_settings_validate() {
    let mut invalid = TrackerListSettings {
        source: TrackerListSource::Custom,
        ..TrackerListSettings::default()
    };
    assert!(matches!(
        invalid.validate(),
        Err(SettingsError::MissingCustomTrackerListUrl)
    ));
    invalid.custom_url = Some("http://insecure.example/list.txt".into());
    assert!(matches!(
        invalid.validate(),
        Err(SettingsError::InvalidTrackerListUrl { .. })
    ));
    invalid.custom_url = Some("https://example.com/list.txt".into());
    invalid.validate().expect("https custom url ok");
}

#[test]
fn empty_download_directory_is_rejected() {
    let store = JsonSettingsStore::new("settings.json");
    let settings = AppSettings::new(PathBuf::new());
    assert!(matches!(
        store.save(&settings),
        Err(SettingsError::EmptyDownloadDirectory)
    ));
}

#[test]
fn load_or_initialize_recovers_corrupt_categories_and_tracker_settings() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("settings.json");
    let store = JsonSettingsStore::new(&path);
    let defaults = AppSettings::new(dir.path().join("Downloads"));
    store.save(&defaults).expect("save defaults");

    // Corrupt categories so no fallback remains (previously failed fatally).
    let mut doc: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&path).expect("read")).expect("json");
    doc["categories"] = serde_json::json!([]);
    fs::write(&path, serde_json::to_vec_pretty(&doc).expect("ser")).expect("write corrupt");

    let loaded = store
        .load_or_initialize(&defaults)
        .expect("should recover corrupt categories");
    let recovery = loaded.recovery.expect("recovery metadata expected");
    assert!(recovery.backup_path.exists());
    assert_eq!(loaded.settings, defaults);
}

#[test]
fn import_settings_rejects_future_export_with_unknown_fields_as_unsupported_version() {
    let current = AppSettings::new(PathBuf::from("C:/Downloads"));
    let payload = export_settings_json(&current).expect("export");
    let mut document: serde_json::Value = serde_json::from_str(&payload).expect("parse export");
    document["export_version"] = serde_json::json!(SETTINGS_EXPORT_FORMAT_VERSION + 1);
    document["future_unknown_field"] = serde_json::json!(true);
    let future = serde_json::to_string(&document).expect("serialize future export");
    assert!(matches!(
        import_settings_json(&future, &current),
        Err(SettingsError::UnsupportedExportVersion { found, .. })
            if found == SETTINGS_EXPORT_FORMAT_VERSION + 1
    ));
}

#[test]
fn duplicate_category_id_returns_dedicated_error() {
    let id = Uuid::new_v4();
    let categories = vec![
        DownloadCategory {
            id,
            name: "General".into(),
            directory: PathBuf::from("C:/Downloads"),
            extensions: Vec::new(),
            is_fallback: true,
        },
        DownloadCategory {
            id,
            name: "Video".into(),
            directory: PathBuf::from("C:/Downloads/Video"),
            extensions: vec!["mp4".into()],
            is_fallback: false,
        },
    ];
    assert!(matches!(
        validate_categories(&categories),
        Err(SettingsError::DuplicateCategoryId { id: found }) if found == id
    ));
}

#[test]
fn import_settings_defaults_omitted_check_certificate_and_syncs_download_directory() {
    let root = tempfile::tempdir().expect("tempdir");
    let mut current = settings(root.path());
    let fallback_dir = root.path().join("custom-fallback");
    current.categories[0].directory = fallback_dir.clone();
    let payload = export_settings_json(&current).expect("export");

    let mut doc: serde_json::Value = serde_json::from_str(&payload).expect("parse export");
    doc["download_directory"] = serde_json::json!(root.path().join("stale-root"));
    if let Some(proxy) = doc
        .get_mut("download_proxy")
        .and_then(serde_json::Value::as_object_mut)
    {
        proxy.remove("check_certificate");
    }
    let mutated = serde_json::to_string(&doc).expect("serialize");

    let imported = import_settings_json(&mutated, &current).expect("import settings");
    assert!(imported.download_proxy.check_certificate);
    assert_eq!(imported.download_directory, fallback_dir);
}

#[test]
fn settings_document_allows_omitted_bridge_and_onboarding_flags() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("settings.json");
    let store = JsonSettingsStore::new(&path);
    let defaults = AppSettings::new(dir.path().join("Downloads"));
    store.save(&defaults).expect("save defaults");

    let mut doc: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&path).expect("read")).expect("parse");
    doc["browser_bridge"] = serde_json::json!({});
    doc["onboarding"] = serde_json::json!({});
    fs::write(&path, serde_json::to_vec(&doc).expect("serialize")).expect("write");

    let parsed = store.load().expect("load settings with omitted flags");
    assert!(!parsed.browser_bridge.allow_cookies);
    assert!(!parsed.browser_bridge.auto_submit);
    assert!(!parsed.onboarding.core_setup_dismissed);
}

#[test]
fn settings_and_export_round_trip_header_presets_and_mirror_rules() {
    use ariadeck_domain::{DomainHeaderPreset, MirrorMatchKind, MirrorRewriteRule};

    let dir = tempfile::tempdir().expect("tempdir");
    let store = JsonSettingsStore::new(dir.path().join("settings.json"));
    let mut current = settings(dir.path());
    current.header_presets = vec![DomainHeaderPreset {
        id: "preset-1".into(),
        name: "CDN".into(),
        enabled: true,
        domains: vec!["*.cdn.example".into()],
        referer: Some("https://cdn.example/".into()),
        user_agent: Some("AriaDeckTest/1.0".into()),
        headers: vec!["Accept: */*".into()],
    }];
    current.mirror_rules = MirrorRewriteSettings {
        keep_original_as_mirror: true,
        rules: vec![MirrorRewriteRule {
            id: "mirror-1".into(),
            name: "HF".into(),
            enabled: true,
            match_kind: MirrorMatchKind::Host,
            pattern: "huggingface.co".into(),
            replacement: "https://hf-mirror.com".into(),
        }],
    };
    current.ui.auto_probe_on_add = true;

    store.save(&current).expect("save settings with rules");
    let loaded = store.load().expect("load settings with rules");
    assert_eq!(loaded, current);

    let exported = export_settings_json(&current).expect("export");
    let imported = import_settings_json(&exported, &settings(dir.path())).expect("import");
    assert_eq!(imported.header_presets, current.header_presets);
    assert_eq!(imported.mirror_rules, current.mirror_rules);
    assert!(imported.ui.auto_probe_on_add);

    // Omitting new fields from an older schema v1 settings file still loads cleanly.
    let mut legacy_doc: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(store.path()).expect("read")).expect("parse");
    let obj = legacy_doc.as_object_mut().expect("object");
    obj.remove("header_presets");
    obj.remove("mirror_rules");
    obj["ui"]
        .as_object_mut()
        .expect("ui obj")
        .remove("auto_probe_on_add");
    fs::write(store.path(), serde_json::to_vec(&legacy_doc).expect("ser")).expect("write");

    let legacy_loaded = store.load().expect("legacy schema v1 loads");
    assert!(legacy_loaded.header_presets.is_empty());
    assert!(legacy_loaded.mirror_rules.rules.is_empty());
    assert!(legacy_loaded.mirror_rules.keep_original_as_mirror);
    assert!(!legacy_loaded.ui.auto_probe_on_add);
}
