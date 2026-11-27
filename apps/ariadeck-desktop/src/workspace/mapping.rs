//! Bidirectional view-model mapping between domain/application/settings types
//! and `ariadeck-ui` presentation structs.

use std::{
    collections::{HashMap, HashSet},
    fs,
    path::{Path, PathBuf},
};

use ariadeck_application::{
    ApplicationError, ApplicationErrorCode, CommandItem, CommandOutcome, EngineCapabilities,
    ItemFailure, StoreSnapshot, TaskListQuery,
};
use ariadeck_domain::{
    ConnectionState, DomainHeaderPreset, DownloadSort, DownloadStatus, DownloadTask, EngineSession,
    EngineSessionId, Gid, MirrorMatchKind, MirrorRewriteRule, ProfileId, SessionGeneration,
    TaskConnectionDetails, TaskDetails, TaskEditableState, TaskIdentity as DomainTaskIdentity,
    TaskProgress, TaskUriStatus,
};
use ariadeck_engine::{
    CoreDiscoveryOrigin, CoreInstallStatus, CoreSource, CoreStore, DiscoveredCore,
    LocalEngineHealth, ProfileCatalog, ProfileEntry, ProfileKind,
};
use ariadeck_settings::{
    AppSettings, CloseBehavior, ColorScheme, CompletionPowerAction, DownloadProxyMode,
    DownloadProxySettings, FileAllocationSetting, LanguagePreference, ListFilterPreference,
    ListSortDirectionPreference, ListSortKeyPreference, MirrorRewriteSettings,
    NotificationSettings, NotificationVolume, PlatformSettings, SpeedLimitSettings,
    TransferPolicySettings, UiPreferences,
};
use ariadeck_ui::{
    BatchCommandOutcomeView, BatchTaskFailureView, BrowserBridgeSettingsView, CloseBehaviorView,
    ColorSchemeView, CommandOutcomeView, CompletionPowerActionView, ConnectionView,
    CoreDiscoveryOriginView, CoreDownloadOfferView, CoreInstallStatusView, CoreInstallationView,
    CoreRegistryView, CoreSourceView, DiscoveredCoreView, DomainHeaderPresetView,
    DownloadCategoryView, DownloadProxySettingsView, DownloadRowView, EngineCapabilitiesView,
    EngineHealthView, EngineSessionView, FileAllocationView, LanguagePreferenceView,
    MirrorMatchKindView, MirrorRewriteRuleView, MirrorRewriteSettingsView,
    NotificationSettingsView, NotificationVolumeView, OperationErrorView, PlatformSettingsView,
    ProfileCatalogView, ProfileEntryView, ProfileKindView, ProfileRpcSecretUpdateView,
    ProxyModeView, ProxyPasswordUpdateView, SettingsView, SpeedLimitSettingsView, SpeedSampleView,
    StoppedHistoryView, TaskCountsView, TaskDetailsView, TaskErrorView, TaskFileView, TaskIdentity,
    TaskOptionView, TaskPathValidationView, TaskPeerView, TaskServerView, TaskStatusView,
    TaskTrackerView, TaskUriStatusView, TaskUriView, TrackerListSettingsView,
    TrackerListSourceView, TransferPolicySettingsView, WorkspaceFilter, WorkspaceQuery,
    WorkspaceSnapshot, WorkspaceSortDirection, WorkspaceSortKey, format_speed_limit_field,
};
use secrecy::SecretString;

use super::{
    ProxyPasswordUpdate,
    core_fetch::{catalog_entry_for, host_target},
};

pub(crate) fn map_engine_session(
    session: &EngineSessionView,
) -> Result<EngineSession, ApplicationError> {
    let profile_id = session.profile_id.parse::<ProfileId>().map_err(|error| {
        ApplicationError::new(
            ApplicationErrorCode::Internal,
            format!("Invalid UI profile identity: {error}"),
            false,
        )
    })?;
    let session_id = session
        .session_id
        .parse::<EngineSessionId>()
        .map_err(|error| {
            ApplicationError::new(
                ApplicationErrorCode::Internal,
                format!("Invalid UI engine-session identity: {error}"),
                false,
            )
        })?;
    if session.generation == 0 {
        return Err(ApplicationError::new(
            ApplicationErrorCode::Internal,
            "The UI supplied an invalid zero session generation.",
            false,
        ));
    }
    Ok(EngineSession::new(
        profile_id,
        session_id,
        SessionGeneration::from_u64(session.generation),
    ))
}

pub(crate) fn map_task_identity(
    identity: &TaskIdentity,
) -> Result<DomainTaskIdentity, ApplicationError> {
    let profile_id = identity.profile_id.parse::<ProfileId>().map_err(|error| {
        ApplicationError::new(
            ApplicationErrorCode::Internal,
            format!("Invalid UI task profile identity: {error}"),
            false,
        )
    })?;
    let gid = identity.gid.parse::<Gid>().map_err(|error| {
        ApplicationError::new(
            ApplicationErrorCode::Internal,
            format!("Invalid UI aria2 GID: {error}"),
            false,
        )
    })?;
    Ok(DomainTaskIdentity::new(profile_id, gid))
}

pub(crate) fn map_command_outcome(outcome: CommandOutcome) -> CommandOutcomeView {
    match outcome {
        CommandOutcome::Success { succeeded } => CommandOutcomeView::Success {
            tasks: succeeded.into_iter().map(map_command_item).collect(),
        },
        CommandOutcome::PartialSuccess { succeeded, failed } => {
            if succeeded.is_empty() {
                CommandOutcomeView::Failure(
                    failed
                        .into_iter()
                        .next()
                        .map(|failure| map_application_error(failure.error))
                        .unwrap_or_else(internal_operation_error),
                )
            } else {
                CommandOutcomeView::Success {
                    tasks: succeeded.into_iter().map(map_command_item).collect(),
                }
            }
        }
        CommandOutcome::Failure { failed } => CommandOutcomeView::Failure(
            failed
                .into_iter()
                .next()
                .map(|failure| map_application_error(failure.error))
                .unwrap_or_else(internal_operation_error),
        ),
    }
}

pub(crate) fn map_batch_command_outcome(outcome: CommandOutcome) -> BatchCommandOutcomeView {
    match outcome {
        CommandOutcome::Success { succeeded } => BatchCommandOutcomeView::Success {
            succeeded: succeeded.into_iter().map(map_command_item).collect(),
        },
        CommandOutcome::PartialSuccess { succeeded, failed } => {
            BatchCommandOutcomeView::PartialSuccess {
                succeeded: succeeded.into_iter().map(map_command_item).collect(),
                failed: failed.into_iter().map(map_batch_failure).collect(),
            }
        }
        CommandOutcome::Failure { failed } => BatchCommandOutcomeView::Failure {
            failed: failed.into_iter().map(map_batch_failure).collect(),
        },
    }
}

pub(crate) fn map_batch_failure(failure: ItemFailure) -> BatchTaskFailureView {
    BatchTaskFailureView {
        identity: failure.item.map(|item| match item {
            CommandItem::Task(identity) => TaskIdentity::from_domain(identity),
        }),
        error: map_application_error(failure.error),
    }
}

pub(crate) fn map_command_item(item: CommandItem) -> TaskIdentity {
    let CommandItem::Task(identity) = item;
    TaskIdentity::from_domain(identity)
}

pub(crate) fn map_application_error(error: ApplicationError) -> OperationErrorView {
    OperationErrorView {
        code: error.code.as_str().into(),
        summary: error.summary,
        retryable: error.retryable,
    }
}

pub(crate) fn unavailable_operation_error() -> OperationErrorView {
    OperationErrorView {
        code: "sync.unavailable".into(),
        summary: "The synchronization coordinator is unavailable.".into(),
        retryable: false,
    }
}

pub(crate) fn internal_operation_error() -> OperationErrorView {
    OperationErrorView {
        code: "command.no_result".into(),
        summary: "The command returned no result.".into(),
        retryable: false,
    }
}

pub(crate) fn map_task_details(
    details: TaskDetails,
    connection: TaskConnectionDetails,
    path_validation: TaskPathValidationView,
) -> TaskDetailsView {
    let directory = details.directory.as_ref().map(ToString::to_string);
    let output_path = if details.files.len() == 1 {
        details.files.first().map(|file| file.path.to_string())
    } else {
        directory.clone()
    };
    let primary_source = connection
        .uris
        .first()
        .map(|source| sanitize_source_uri(&source.uri));
    TaskDetailsView {
        directory,
        primary_source,
        output_path,
        path_validation,
        info_hash: details.info_hash,
        piece_length: details.piece_length.map(|length| length.get()),
        piece_count: details.piece_count,
        trackers: details
            .trackers
            .into_iter()
            .map(|tracker| TaskTrackerView {
                tier: tracker.tier,
                uri: ariadeck_domain::redact_tracker_uri(&tracker.uri),
            })
            .collect(),
        uris: connection
            .uris
            .into_iter()
            .map(|uri| TaskUriView {
                uri: sanitize_source_uri(&uri.uri),
                status: match uri.status {
                    TaskUriStatus::Used => TaskUriStatusView::Used,
                    TaskUriStatus::Waiting => TaskUriStatusView::Waiting,
                    TaskUriStatus::Unknown => TaskUriStatusView::Unknown,
                },
            })
            .collect(),
        servers: connection
            .servers
            .into_iter()
            .map(|server| TaskServerView {
                file_index: server.file_index,
                uri: sanitize_source_uri(&server.uri),
                current_uri: sanitize_source_uri(&server.current_uri),
                download_rate: server.download_speed.get(),
            })
            .collect(),
        peers: connection
            .peers
            .into_iter()
            .map(|peer| TaskPeerView {
                address: peer.address,
                port: peer.port,
                download_rate: peer.download_speed.get(),
                upload_rate: peer.upload_speed.get(),
                seeder: peer.seeder,
            })
            .collect(),
        options: connection
            .options
            .into_iter()
            .map(|option| TaskOptionView {
                key: option.key,
                value: option.value,
                redacted: option.redacted,
            })
            .collect(),
        files: details
            .files
            .into_iter()
            .map(|file| TaskFileView {
                index: file.index,
                path: file.path.to_string(),
                length: file.length.get(),
                completed_length: file.completed_length.get(),
                selected: file.selected,
            })
            .collect(),
    }
}

pub(crate) fn map_core_registry(store: &CoreStore) -> CoreRegistryView {
    match store.load_registry_with_installations() {
        Ok((registry, installations)) => CoreRegistryView {
            active_id: registry.active_id.map(|id| id.to_string()),
            last_working_id: registry.last_working_id.map(|id| id.to_string()),
            installations: installations
                .into_iter()
                .map(|core| CoreInstallationView {
                    id: core.id.to_string(),
                    version: core.version,
                    target: core.target,
                    source: match core.source {
                        CoreSource::Imported => CoreSourceView::Imported,
                        CoreSource::Linked => CoreSourceView::Linked,
                        CoreSource::Managed => CoreSourceView::Managed,
                    },
                    executable: core.executable.to_string_lossy().into_owned(),
                    features: core.features,
                    is_active: core.is_active,
                    is_last_working: core.is_last_working,
                    validated_version: core.validated_version,
                    status: match core.status {
                        CoreInstallStatus::Ready => CoreInstallStatusView::Ready,
                        CoreInstallStatus::MissingExecutable => {
                            CoreInstallStatusView::MissingExecutable
                        }
                        CoreInstallStatus::MissingManifest => {
                            CoreInstallStatusView::MissingManifest
                        }
                    },
                })
                .collect(),
        },
        Err(error) => {
            tracing::warn!(%error, "failed to list managed aria2 cores");
            CoreRegistryView::default()
        }
    }
}

pub(crate) fn map_core_discovery_origin(origin: CoreDiscoveryOrigin) -> CoreDiscoveryOriginView {
    match origin {
        CoreDiscoveryOrigin::EnvironmentOverride => CoreDiscoveryOriginView::Environment,
        CoreDiscoveryOrigin::SearchPath => CoreDiscoveryOriginView::SearchPath,
        CoreDiscoveryOrigin::Scoop => CoreDiscoveryOriginView::Scoop,
        CoreDiscoveryOrigin::WinGet => CoreDiscoveryOriginView::WinGet,
        CoreDiscoveryOrigin::Chocolatey => CoreDiscoveryOriginView::Chocolatey,
        CoreDiscoveryOrigin::Homebrew => CoreDiscoveryOriginView::Homebrew,
        CoreDiscoveryOrigin::SystemInstall => CoreDiscoveryOriginView::SystemInstall,
        CoreDiscoveryOrigin::Portable => CoreDiscoveryOriginView::Portable,
    }
}

/// Project discovery results, marking the ones the core registry already holds.
///
/// "Already managed" is decided by comparing canonicalized paths against every
/// registered installation, so a scoop shim found through PATH is not offered as
/// a fresh import of a core the user already added.
pub(crate) fn map_discovered_cores(
    discovered: Vec<DiscoveredCore>,
    registry: &CoreRegistryView,
) -> Vec<DiscoveredCoreView> {
    let registered: HashSet<PathBuf> = registry
        .installations
        .iter()
        .filter(|core| !core.executable.is_empty())
        .map(|core| canonical_or_original(Path::new(&core.executable)))
        .collect();
    discovered
        .into_iter()
        .map(|core| {
            let identity = canonical_or_original(&core.path);
            DiscoveredCoreView {
                path: core.path.to_string_lossy().into_owned(),
                version: core.version,
                origin: map_core_discovery_origin(core.origin),
                features: core.features,
                already_registered: registered.contains(&identity),
            }
        })
        .collect()
}

fn canonical_or_original(path: &Path) -> PathBuf {
    fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

/// The pinned download offer for this platform, or an explicit "not published".
pub(crate) fn map_core_download_offer() -> CoreDownloadOfferView {
    let target = host_target();
    catalog_entry_for(&target).map_or(
        CoreDownloadOfferView {
            available: false,
            target,
            ..CoreDownloadOfferView::default()
        },
        |entry| CoreDownloadOfferView {
            available: true,
            version: entry.version.to_owned(),
            target: entry.target.to_owned(),
            url: entry.url.to_owned(),
            emulated: entry.emulated,
        },
    )
}

pub(crate) fn map_profile_catalog(catalog: &ProfileCatalog) -> ProfileCatalogView {
    ProfileCatalogView {
        active_profile_id: catalog.active_profile_id.to_string(),
        profiles: catalog.profiles.iter().map(map_profile_entry).collect(),
    }
}

pub(crate) fn map_profile_entry(entry: &ProfileEntry) -> ProfileEntryView {
    ProfileEntryView {
        profile_id: entry.profile_id.to_string(),
        name: entry.name.clone(),
        kind: match entry.kind {
            ProfileKind::LocalManaged => ProfileKindView::LocalManaged,
            ProfileKind::RemoteRpc => ProfileKindView::RemoteRpc,
        },
        executable: entry
            .executable
            .as_ref()
            .map(|path| path.to_string_lossy().into_owned())
            .unwrap_or_default(),
        download_dir: entry.download_dir.to_string_lossy().into_owned(),
        endpoint: entry.endpoint.clone().unwrap_or_default(),
        has_secret: entry.has_secret,
    }
}

pub(crate) struct MappedProfileCatalog {
    pub(crate) catalog: ProfileCatalog,
    pub(crate) secret_updates: HashMap<ProfileId, ProfileRpcSecretUpdateView>,
}

pub(crate) fn map_profile_catalog_request(
    view: &ProfileCatalogView,
    secret_updates: &HashMap<String, ProfileRpcSecretUpdateView>,
    existing: &ProfileCatalog,
    data_dir: &Path,
    settings: &AppSettings,
) -> Result<MappedProfileCatalog, String> {
    if view.profiles.is_empty() {
        return Err("At least one profile is required.".into());
    }
    let mut profiles = Vec::with_capacity(view.profiles.len());
    let mut mapped_secret_updates: HashMap<ProfileId, ProfileRpcSecretUpdateView> = HashMap::new();
    let mut resolved_active: Option<ProfileId> = None;
    for entry in &view.profiles {
        // Draft rows use non-UUID ids ("draft-local-N"); mint a stable id on save.
        let profile_id = entry
            .profile_id
            .parse::<ProfileId>()
            .unwrap_or_else(|_| ProfileId::new());
        if entry.profile_id == view.active_profile_id {
            resolved_active = Some(profile_id);
        }
        let name = entry.name.trim();
        if name.is_empty() {
            return Err("Profile name cannot be empty.".into());
        }
        // Preserve secret_ref from the existing catalog entry when ids match.
        let previous = entry
            .profile_id
            .parse::<ProfileId>()
            .ok()
            .and_then(|id| existing.get(id).cloned())
            .or_else(|| existing.get(profile_id).cloned());
        let mapped = match entry.kind {
            ProfileKindView::LocalManaged => {
                // Empty executable = use active managed core / discovery at spawn.
                let executable = entry.executable.trim();
                let download_dir = entry.download_dir.trim();
                let download_dir = if download_dir.is_empty() {
                    settings.download_directory.clone()
                } else {
                    PathBuf::from(download_dir)
                };
                let mut profile = ProfileEntry::local_managed(
                    profile_id,
                    name,
                    if executable.is_empty() {
                        PathBuf::new()
                    } else {
                        PathBuf::from(executable)
                    },
                    data_dir.to_path_buf(),
                    download_dir,
                );
                if executable.is_empty() {
                    profile.executable = None;
                }
                profile.has_secret = false;
                profile.secret_ref = None;
                if previous.as_ref().and_then(|p| p.secret_ref).is_some() {
                    mapped_secret_updates.insert(profile_id, ProfileRpcSecretUpdateView::Clear);
                }
                profile
            }
            ProfileKindView::RemoteRpc => {
                let endpoint = entry.endpoint.trim();
                if endpoint.is_empty() {
                    return Err(format!("Remote profile {name} needs a ws/wss endpoint."));
                }
                let download_dir = entry.download_dir.trim();
                let download_dir = if download_dir.is_empty() {
                    settings.download_directory.clone()
                } else {
                    PathBuf::from(download_dir)
                };
                let secret_update = secret_updates
                    .get(&entry.profile_id)
                    .cloned()
                    .unwrap_or(ProfileRpcSecretUpdateView::Unchanged);
                let secret_ref = match &secret_update {
                    ProfileRpcSecretUpdateView::Clear => {
                        mapped_secret_updates.insert(profile_id, ProfileRpcSecretUpdateView::Clear);
                        None
                    }
                    ProfileRpcSecretUpdateView::Set(_) => {
                        mapped_secret_updates.insert(profile_id, secret_update.clone());
                        // Keep previous ref if present so we overwrite the same keyring entry;
                        // otherwise mint a new ref (applied in apply_profile_secret_updates).
                        Some(
                            previous
                                .as_ref()
                                .and_then(|profile| profile.secret_ref)
                                .unwrap_or_else(ariadeck_engine::RpcSecretRef::new),
                        )
                    }
                    ProfileRpcSecretUpdateView::Unchanged => {
                        previous.as_ref().and_then(|profile| profile.secret_ref)
                    }
                };
                ProfileEntry::remote_rpc(profile_id, name, endpoint, download_dir, secret_ref)
                    .map_err(|error| error.to_string())?
            }
        };
        profiles.push(mapped);
    }
    let active_profile_id = resolved_active
        .or_else(|| {
            view.active_profile_id
                .parse::<ProfileId>()
                .ok()
                .filter(|id| profiles.iter().any(|profile| profile.profile_id == *id))
        })
        .or_else(|| profiles.first().map(|profile| profile.profile_id))
        .ok_or_else(|| "At least one profile is required.".to_owned())?;
    let catalog = ProfileCatalog {
        schema_version: ariadeck_engine::PROFILE_CATALOG_SCHEMA_VERSION,
        active_profile_id,
        profiles,
    };
    catalog.validate().map_err(|error| error.to_string())?;
    Ok(MappedProfileCatalog {
        catalog,
        secret_updates: mapped_secret_updates,
    })
}

pub(crate) fn map_completion_power_action_to_view(
    action: CompletionPowerAction,
) -> CompletionPowerActionView {
    match action {
        CompletionPowerAction::None => CompletionPowerActionView::None,
        CompletionPowerAction::Sleep => CompletionPowerActionView::Sleep,
        CompletionPowerAction::Hibernate => CompletionPowerActionView::Hibernate,
        CompletionPowerAction::Shutdown => CompletionPowerActionView::Shutdown,
    }
}

pub(crate) fn map_view_completion_power_action_to_app(
    action: CompletionPowerActionView,
) -> CompletionPowerAction {
    match action {
        CompletionPowerActionView::None => CompletionPowerAction::None,
        CompletionPowerActionView::Sleep => CompletionPowerAction::Sleep,
        CompletionPowerActionView::Hibernate => CompletionPowerAction::Hibernate,
        CompletionPowerActionView::Shutdown => CompletionPowerAction::Shutdown,
    }
}

pub(crate) fn map_settings(settings: &AppSettings) -> SettingsView {
    SettingsView {
        color_scheme: match settings.color_scheme {
            ColorScheme::System => ColorSchemeView::System,
            ColorScheme::Light => ColorSchemeView::Light,
            ColorScheme::Dark => ColorSchemeView::Dark,
        },
        language: match settings.language {
            LanguagePreference::System => LanguagePreferenceView::System,
            LanguagePreference::En => LanguagePreferenceView::English,
            LanguagePreference::ZhCn => LanguagePreferenceView::ChineseSimplified,
        },
        download_directory: settings.download_directory.to_string_lossy().into_owned(),
        download_proxy: DownloadProxySettingsView {
            mode: match settings.download_proxy.mode {
                DownloadProxyMode::Disabled => ProxyModeView::Disabled,
                DownloadProxyMode::System => ProxyModeView::System,
                DownloadProxyMode::Manual => ProxyModeView::Manual,
            },
            all_proxy: settings
                .download_proxy
                .all_proxy
                .clone()
                .unwrap_or_default(),
            http_proxy: settings
                .download_proxy
                .http_proxy
                .clone()
                .unwrap_or_default(),
            https_proxy: settings
                .download_proxy
                .https_proxy
                .clone()
                .unwrap_or_default(),
            ftp_proxy: settings
                .download_proxy
                .ftp_proxy
                .clone()
                .unwrap_or_default(),
            no_proxy: settings.download_proxy.no_proxy.clone(),
            username: settings.download_proxy.username.clone().unwrap_or_default(),
            has_password: settings.download_proxy.credential.is_some(),
            check_certificate: settings.download_proxy.check_certificate,
        },
        speed_limits: SpeedLimitSettingsView {
            download_limit: format_speed_limit_field(settings.speed_limits.download_limit),
            upload_limit: format_speed_limit_field(settings.speed_limits.upload_limit),
        },
        transfer_policy: TransferPolicySettingsView {
            max_concurrent_downloads: settings
                .transfer_policy
                .max_concurrent_downloads
                .to_string(),
            max_connection_per_server: settings
                .transfer_policy
                .max_connection_per_server
                .to_string(),
            split: settings.transfer_policy.split.to_string(),
            min_split_size: format_speed_limit_field(settings.transfer_policy.min_split_size),
            file_allocation: match settings.transfer_policy.file_allocation {
                FileAllocationSetting::None => FileAllocationView::None,
                FileAllocationSetting::Prealloc => FileAllocationView::Prealloc,
                FileAllocationSetting::Trunc => FileAllocationView::Trunc,
                FileAllocationSetting::Falloc => FileAllocationView::Falloc,
            },
            check_integrity: settings.transfer_policy.check_integrity,
        },
        notifications: NotificationSettingsView {
            volume: match settings.notifications.volume {
                NotificationVolume::Normal => NotificationVolumeView::Normal,
                NotificationVolume::Quiet => NotificationVolumeView::Quiet,
                NotificationVolume::Silent => NotificationVolumeView::Silent,
            },
            notify_on_completion: settings.notifications.notify_on_completion,
            notify_on_error: settings.notifications.notify_on_error,
            notify_on_engine_events: settings.notifications.notify_on_engine_events,
            os_notifications: settings.notifications.os_notifications,
            notify_on_low_disk: settings.notifications.notify_on_low_disk,
            low_disk_threshold_bytes: settings.notifications.low_disk_threshold_bytes,
        },
        platform: PlatformSettingsView {
            close_behavior: match settings.platform.close_behavior {
                CloseBehavior::MinimizeToTray => CloseBehaviorView::MinimizeToTray,
                CloseBehavior::Quit => CloseBehaviorView::Quit,
            },
            show_tray_icon: settings.platform.show_tray_icon,
            start_minimized_to_tray: settings.platform.start_minimized_to_tray,
            keep_awake_while_active: settings.platform.keep_awake_while_active,
            action_after_done: map_completion_power_action_to_view(
                settings.platform.action_after_done,
            ),
            watch_clipboard: settings.platform.watch_clipboard,
        },
        browser_bridge: BrowserBridgeSettingsView {
            allow_cookies: settings.browser_bridge.allow_cookies,
            auto_submit: settings.browser_bridge.auto_submit,
        },
        categories: settings
            .categories
            .iter()
            .map(|category| DownloadCategoryView {
                id: category.id.to_string(),
                name: category.name.clone(),
                directory: category.directory.to_string_lossy().into_owned(),
                extensions: ariadeck_settings::format_category_extensions(&category.extensions),
                is_fallback: category.is_fallback,
            })
            .collect(),
        tracker_list: TrackerListSettingsView {
            enabled: settings.tracker_list.enabled,
            source: match settings.tracker_list.source {
                ariadeck_settings::TrackerListSource::Curated => TrackerListSourceView::Curated,
                ariadeck_settings::TrackerListSource::Custom => TrackerListSourceView::Custom,
            },
            custom_url: settings.tracker_list.custom_url.clone().unwrap_or_default(),
            auto_refresh: settings.tracker_list.auto_refresh,
            last_refreshed_at: settings.tracker_list.last_refreshed_at,
            list_text: settings.tracker_list.list_text.clone(),
        },
        header_presets: settings
            .header_presets
            .iter()
            .map(|preset| DomainHeaderPresetView {
                id: preset.id.clone(),
                name: preset.name.clone(),
                enabled: preset.enabled,
                domains: preset.domains.join(", "),
                referer: preset.referer.clone().unwrap_or_default(),
                user_agent: preset.user_agent.clone().unwrap_or_default(),
                headers: preset.headers.join("\n"),
            })
            .collect(),
        mirror_rules: MirrorRewriteSettingsView {
            keep_original_as_mirror: settings.mirror_rules.keep_original_as_mirror,
            rules: settings
                .mirror_rules
                .rules
                .iter()
                .map(|rule| MirrorRewriteRuleView {
                    id: rule.id.clone(),
                    name: rule.name.clone(),
                    enabled: rule.enabled,
                    match_kind: match rule.match_kind {
                        MirrorMatchKind::Prefix => MirrorMatchKindView::Prefix,
                        MirrorMatchKind::Host => MirrorMatchKindView::Host,
                    },
                    pattern: rule.pattern.clone(),
                    replacement: rule.replacement.clone(),
                })
                .collect(),
        },
        auto_probe_on_add: settings.ui.auto_probe_on_add,
    }
}

pub(crate) fn map_settings_request(
    settings: &SettingsView,
    current: &AppSettings,
    password: ProxyPasswordUpdateView,
) -> Result<(AppSettings, ProxyPasswordUpdate), String> {
    let password = match password {
        ProxyPasswordUpdateView::Unchanged => ProxyPasswordUpdate::Unchanged,
        ProxyPasswordUpdateView::Detach => ProxyPasswordUpdate::Detach,
        ProxyPasswordUpdateView::Clear => ProxyPasswordUpdate::Clear,
        ProxyPasswordUpdateView::Set(password) => {
            let password = password.into_inner();
            if password.is_empty() {
                return Err("Proxy password must not be empty.".into());
            }
            ProxyPasswordUpdate::Set(SecretString::from(password))
        }
    };
    let credential = match &password {
        ProxyPasswordUpdate::Unchanged => current.download_proxy.credential,
        ProxyPasswordUpdate::Detach => None,
        ProxyPasswordUpdate::Clear => None,
        ProxyPasswordUpdate::Set(_) => Some(current.download_proxy.credential.unwrap_or_default()),
    };
    let mut mapped = AppSettings {
        color_scheme: match settings.color_scheme {
            ColorSchemeView::System => ColorScheme::System,
            ColorSchemeView::Light => ColorScheme::Light,
            ColorSchemeView::Dark => ColorScheme::Dark,
        },
        language: match settings.language {
            LanguagePreferenceView::System => LanguagePreference::System,
            LanguagePreferenceView::English => LanguagePreference::En,
            LanguagePreferenceView::ChineseSimplified => LanguagePreference::ZhCn,
        },
        download_directory: PathBuf::from(settings.download_directory.trim()),
        download_proxy: DownloadProxySettings {
            mode: match settings.download_proxy.mode {
                ProxyModeView::Disabled => DownloadProxyMode::Disabled,
                ProxyModeView::System => DownloadProxyMode::System,
                ProxyModeView::Manual => DownloadProxyMode::Manual,
            },
            all_proxy: trimmed_value(&settings.download_proxy.all_proxy),
            http_proxy: trimmed_value(&settings.download_proxy.http_proxy),
            https_proxy: trimmed_value(&settings.download_proxy.https_proxy),
            ftp_proxy: trimmed_value(&settings.download_proxy.ftp_proxy),
            no_proxy: settings
                .download_proxy
                .no_proxy
                .iter()
                .map(|entry| entry.trim())
                .filter(|entry| !entry.is_empty())
                .map(ToOwned::to_owned)
                .collect(),
            username: trimmed_value(&settings.download_proxy.username),
            credential,
            check_certificate: settings.download_proxy.check_certificate,
        },
        speed_limits: SpeedLimitSettings {
            download_limit: settings
                .speed_limits
                .parse_download_limit()
                .ok_or_else(|| "Download speed limit must be bytes/second or a K/M/G value (or empty for unlimited).".to_owned())?,
            upload_limit: settings
                .speed_limits
                .parse_upload_limit()
                .ok_or_else(|| "Upload speed limit must be bytes/second or a K/M/G value (or empty for unlimited).".to_owned())?,
        },
        transfer_policy: TransferPolicySettings {
            max_concurrent_downloads: settings
                .transfer_policy
                .parse_max_concurrent_downloads()
                .ok_or_else(|| "Maximum concurrent downloads must be a positive integer.".to_owned())?,
            max_connection_per_server: settings
                .transfer_policy
                .parse_max_connection_per_server()
                .ok_or_else(|| {
                    "Maximum connections per server must be an integer from 1 to 16.".to_owned()
                })?,
            split: settings
                .transfer_policy
                .parse_split()
                .ok_or_else(|| "Split count must be a positive integer.".to_owned())?,
            min_split_size: settings
                .transfer_policy
                .parse_min_split_size()
                .ok_or_else(|| {
                    "Minimum split size must be a positive byte count or K/M/G value.".to_owned()
                })?,
            file_allocation: match settings.transfer_policy.file_allocation {
                FileAllocationView::None => FileAllocationSetting::None,
                FileAllocationView::Prealloc => FileAllocationSetting::Prealloc,
                FileAllocationView::Trunc => FileAllocationSetting::Trunc,
                FileAllocationView::Falloc => FileAllocationSetting::Falloc,
            },
            check_integrity: settings.transfer_policy.check_integrity,
        },
        notifications: NotificationSettings {
            volume: match settings.notifications.volume {
                NotificationVolumeView::Normal => NotificationVolume::Normal,
                NotificationVolumeView::Quiet => NotificationVolume::Quiet,
                NotificationVolumeView::Silent => NotificationVolume::Silent,
            },
            notify_on_completion: settings.notifications.notify_on_completion,
            notify_on_error: settings.notifications.notify_on_error,
            notify_on_engine_events: settings.notifications.notify_on_engine_events,
            os_notifications: settings.notifications.os_notifications,
            notify_on_low_disk: settings.notifications.notify_on_low_disk,
            low_disk_threshold_bytes: settings.notifications.low_disk_threshold_bytes,
        },
        platform: PlatformSettings {
            close_behavior: match settings.platform.close_behavior {
                CloseBehaviorView::MinimizeToTray => CloseBehavior::MinimizeToTray,
                CloseBehaviorView::Quit => CloseBehavior::Quit,
            },
            show_tray_icon: settings.platform.show_tray_icon,
            start_minimized_to_tray: settings.platform.start_minimized_to_tray,
            keep_awake_while_active: settings.platform.keep_awake_while_active,
            action_after_done: map_view_completion_power_action_to_app(settings.platform.action_after_done),
            watch_clipboard: settings.platform.watch_clipboard,
        },
        browser_bridge: ariadeck_settings::BrowserBridgeSettings {
            allow_cookies: settings.browser_bridge.allow_cookies,
            auto_submit: settings.browser_bridge.auto_submit,
        },
        // First-run state is owned by the core-setup guide (B4), not the settings
        // form; saving Settings must never re-arm or dismiss it.
        onboarding: current.onboarding,
        // List preferences are owned by the shell query path (UI-001); preserve
        // them while updating `auto_probe_on_add` from the settings panel.
        ui: UiPreferences {
            auto_probe_on_add: settings.auto_probe_on_add,
            ..current.ui
        },
        categories: Vec::new(),
        tracker_list: ariadeck_settings::TrackerListSettings {
            enabled: settings.tracker_list.enabled,
            source: match settings.tracker_list.source {
                TrackerListSourceView::Curated => ariadeck_settings::TrackerListSource::Curated,
                TrackerListSourceView::Custom => ariadeck_settings::TrackerListSource::Custom,
            },
            custom_url: trimmed_value(&settings.tracker_list.custom_url),
            auto_refresh: settings.tracker_list.auto_refresh,
            last_refreshed_at: settings.tracker_list.last_refreshed_at,
            list_text: settings.tracker_list.list_text.clone(),
        },
        header_presets: settings
            .header_presets
            .iter()
            .map(|preset| DomainHeaderPreset {
                id: preset.id.trim().to_owned(),
                name: preset.name.trim().to_owned(),
                enabled: preset.enabled,
                domains: preset
                    .domains
                    .split([',', ';', '\n'])
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(ToOwned::to_owned)
                    .collect(),
                referer: trimmed_value(&preset.referer),
                user_agent: trimmed_value(&preset.user_agent),
                headers: preset
                    .headers
                    .lines()
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(ToOwned::to_owned)
                    .collect(),
            })
            .collect(),
        mirror_rules: MirrorRewriteSettings {
            keep_original_as_mirror: settings.mirror_rules.keep_original_as_mirror,
            rules: settings
                .mirror_rules
                .rules
                .iter()
                .map(|rule| MirrorRewriteRule {
                    id: rule.id.trim().to_owned(),
                    name: rule.name.trim().to_owned(),
                    enabled: rule.enabled,
                    match_kind: match rule.match_kind {
                        MirrorMatchKindView::Prefix => MirrorMatchKind::Prefix,
                        MirrorMatchKindView::Host => MirrorMatchKind::Host,
                    },
                    pattern: rule.pattern.trim().to_owned(),
                    replacement: rule.replacement.trim().to_owned(),
                })
                .collect(),
        },
    };
    let mut categories = Vec::with_capacity(settings.categories.len());
    for category in &settings.categories {
        let id = category
            .id
            .parse()
            .map_err(|_| "Invalid download category id.".to_owned())?;
        categories.push(ariadeck_settings::DownloadCategory {
            id,
            name: category.name.trim().to_owned(),
            directory: std::path::PathBuf::from(category.directory.trim()),
            extensions: ariadeck_settings::parse_category_extensions_text(&category.extensions),
            is_fallback: category.is_fallback,
        });
    }
    mapped.categories = categories;
    // General is the fixed fallback category (D-042); name match wins.
    for category in &mut mapped.categories {
        category.is_fallback = category.name.eq_ignore_ascii_case("General");
    }
    if !mapped.categories.iter().any(|c| c.is_fallback) {
        if let Some(first) = mapped.categories.first_mut() {
            first.is_fallback = true;
        }
    } else {
        let mut seen = false;
        for category in &mut mapped.categories {
            if category.is_fallback {
                if seen {
                    category.is_fallback = false;
                } else {
                    seen = true;
                }
            }
        }
    }
    ariadeck_settings::sync_download_directory_from_fallback(&mut mapped);
    mapped.validate().map_err(|error| error.to_string())?;
    Ok((mapped, password))
}

pub(crate) fn trimmed_value(value: &str) -> Option<String> {
    let value = value.trim();
    (!value.is_empty()).then(|| value.to_owned())
}

pub(crate) fn map_ui_preferences_to_query(ui: &UiPreferences) -> WorkspaceQuery {
    WorkspaceQuery {
        filter: match ui.list_filter {
            ListFilterPreference::All => WorkspaceFilter::All,
            ListFilterPreference::Active => WorkspaceFilter::Active,
            ListFilterPreference::Waiting => WorkspaceFilter::Waiting,
            ListFilterPreference::Paused => WorkspaceFilter::Paused,
            ListFilterPreference::Completed => WorkspaceFilter::Completed,
            ListFilterPreference::Failed => WorkspaceFilter::Failed,
        },
        search: String::new(),
        sort_key: match ui.list_sort_key {
            ListSortKeyPreference::Queue => WorkspaceSortKey::Queue,
            ListSortKeyPreference::Name => WorkspaceSortKey::Name,
            ListSortKeyPreference::Status => WorkspaceSortKey::Status,
            ListSortKeyPreference::Progress => WorkspaceSortKey::Progress,
            ListSortKeyPreference::DownloadSpeed => WorkspaceSortKey::DownloadSpeed,
            ListSortKeyPreference::Size => WorkspaceSortKey::Size,
        },
        sort_direction: match ui.list_sort_direction {
            ListSortDirectionPreference::Ascending => WorkspaceSortDirection::Ascending,
            ListSortDirectionPreference::Descending => WorkspaceSortDirection::Descending,
        },
        category_id: None,
    }
}

pub(crate) fn map_query(query: &WorkspaceQuery) -> TaskListQuery {
    TaskListQuery {
        filter: query.filter,
        search: query.search.clone(),
        sort: DownloadSort {
            key: query.sort_key,
            direction: query.sort_direction,
        },
        category_id: query.category_id.clone(),
    }
}

pub(crate) fn map_snapshot(
    snapshot: StoreSnapshot,
    local_path_actions_available: bool,
) -> WorkspaceSnapshot {
    let profile_id = snapshot.session.profile_id.to_string();
    let observed_seeding_seconds = snapshot.observed_seeding_seconds;
    let category_by_gid = snapshot.category_by_gid;
    WorkspaceSnapshot {
        profile_id: profile_id.clone(),
        session_id: snapshot.session.session_id.to_string(),
        generation: snapshot.session.generation.get(),
        source_revision: snapshot.view.source_revision,
        connection: map_connection(snapshot.connection_state),
        stale: snapshot.stale,
        local_path_actions_available,
        download_rate: snapshot.global_stat.download_speed.get(),
        upload_rate: snapshot.global_stat.upload_speed.get(),
        speed_history: snapshot
            .speed_history
            .samples()
            .iter()
            .map(|sample| SpeedSampleView {
                download_rate: sample.download.get(),
                upload_rate: sample.upload.get(),
            })
            .collect(),
        counts: TaskCountsView {
            all: snapshot.counts.all,
            active: snapshot.counts.active,
            waiting: snapshot.counts.waiting,
            paused: snapshot.counts.paused,
            completed: snapshot.counts.completed,
            failed: snapshot.counts.failed,
        },
        stopped_history: StoppedHistoryView {
            loaded: snapshot.stopped_history.loaded,
            total: snapshot.stopped_history.total,
            can_load_more: snapshot.stopped_history.can_load_more,
            local_saved: snapshot.stopped_history.local_saved,
        },
        tasks: snapshot
            .tasks
            .into_iter()
            .map(|task| {
                let observed = observed_seeding_seconds.get(&task.gid).copied();
                {
                    let category_id = category_by_gid.get(&task.gid).cloned();
                    map_task(&profile_id, task, observed, category_id)
                }
            })
            .collect(),
        capabilities: map_capabilities(&snapshot.capabilities),
    }
}

pub(crate) fn map_capabilities(capabilities: &EngineCapabilities) -> EngineCapabilitiesView {
    EngineCapabilitiesView {
        version: capabilities.version.clone(),
        methods_probed: capabilities.methods_probed(),
        force_pause: capabilities.supports_force_pause(),
        force_pause_all: capabilities.supports_force_pause_all(),
        force_remove: capabilities.supports_force_remove(),
        queue_positioning: capabilities.supports_queue_positioning(),
        change_option: capabilities.supports_change_option(),
        change_uri: capabilities.supports_change_uri(),
        change_global_option: capabilities.supports_change_global_option(),
        get_peers: capabilities.supports_get_peers(),
        get_servers: capabilities.supports_get_servers(),
        multicall: capabilities.supports_multicall(),
    }
}

pub(crate) fn map_task_editable_state(
    state: TaskEditableState,
) -> ariadeck_ui::TaskEditableStateView {
    ariadeck_ui::TaskEditableStateView {
        uris: state.uris,
        referer: state.referer.unwrap_or_default(),
        user_agent: state.user_agent.unwrap_or_default(),
        headers: state.headers.join("\n"),
        proxy: state.proxy.unwrap_or_default(),
        max_download_limit: state.max_download_limit.unwrap_or_default(),
        max_connection_per_server: state
            .max_connection_per_server
            .map_or_else(String::new, |value| value.to_string()),
        split: state
            .split
            .map_or_else(String::new, |value| value.to_string()),
        out: state.out.unwrap_or_default(),
        dir: state
            .dir
            .map_or_else(String::new, |dir| dir.as_str().to_owned()),
    }
}

pub(crate) fn map_connection(state: ConnectionState) -> ConnectionView {
    match state {
        ConnectionState::Disconnected => ConnectionView::Disconnected,
        ConnectionState::Connecting => ConnectionView::Connecting,
        ConnectionState::Authenticating => ConnectionView::Authenticating,
        ConnectionState::Synchronizing => ConnectionView::Synchronizing,
        ConnectionState::Connected => ConnectionView::Connected,
        ConnectionState::Reconnecting { attempt } => ConnectionView::Reconnecting { attempt },
        ConnectionState::Failed { reason } => ConnectionView::Failed {
            summary: reason.summary,
            retryable: reason.retryable,
        },
    }
}

pub(crate) fn map_local_engine_health(health: LocalEngineHealth) -> EngineHealthView {
    match health {
        LocalEngineHealth::Running { restarts } => EngineHealthView::Running { restarts },
        LocalEngineHealth::Restarting { attempt } => EngineHealthView::Restarting { attempt },
        LocalEngineHealth::Failed { reason, .. } => EngineHealthView::Failed { summary: reason },
    }
}

pub(crate) fn map_task(
    profile_id: &str,
    task: DownloadTask,
    observed_seeding_seconds: Option<u64>,
    category_id: Option<String>,
) -> DownloadRowView {
    let eta_seconds = (task.status != DownloadStatus::Seeding)
        .then(|| {
            TaskProgress::new(task.completed_length, task.total_length)
                .eta(task.download_speed)
                .map(|duration| duration.as_secs())
        })
        .flatten();
    let primary_source = task
        .metadata
        .primary_uri
        .as_deref()
        .map(ariadeck_domain::redact_source_uri);
    let directory = task.metadata.directory.as_ref().map(ToString::to_string);
    DownloadRowView {
        identity: TaskIdentity {
            profile_id: profile_id.into(),
            gid: task.gid.to_string(),
        },
        display_name: task.display_name,
        name_state: task.name_state,
        source_kind: task.metadata.source_kind,
        primary_source,
        directory,
        category_id,
        followed_by: task
            .metadata
            .followed_by
            .into_iter()
            .map(|gid| gid.to_string())
            .collect(),
        belongs_to: task.metadata.belongs_to.map(|gid| gid.to_string()),
        status: match task.status {
            DownloadStatus::Active => TaskStatusView::Active,
            DownloadStatus::Seeding => TaskStatusView::Seeding,
            DownloadStatus::Waiting => TaskStatusView::Waiting,
            DownloadStatus::Paused => TaskStatusView::Paused,
            DownloadStatus::Complete => TaskStatusView::Complete,
            DownloadStatus::Error => TaskStatusView::Failed,
            DownloadStatus::Removed => TaskStatusView::Removed,
            DownloadStatus::Verifying => TaskStatusView::Verifying,
            DownloadStatus::Unknown => TaskStatusView::Unknown,
        },
        error: task.error.map(classify_task_error),
        total_bytes: task.total_length.get(),
        completed_bytes: task.completed_length.get(),
        uploaded_bytes: task.upload_length.get(),
        download_rate: task.download_speed.get(),
        upload_rate: task.upload_speed.get(),
        eta_seconds,
        observed_seeding_seconds,
        revision: task.revision,
    }
}

fn redact_error_message(message: &str) -> String {
    const SCHEMES: &[&str] = &[
        "https://", "http://", "ftps://", "ftp://", "sftp://", "wss://", "ws://", "magnet:",
    ];

    let lower = message.to_ascii_lowercase();
    let mut out = String::with_capacity(message.len());
    let mut cursor = 0usize;

    while cursor < message.len() {
        let next = SCHEMES
            .iter()
            .filter_map(|scheme| lower[cursor..].find(scheme).map(|offset| cursor + offset))
            .min();
        let Some(start) = next else {
            out.push_str(&message[cursor..]);
            break;
        };
        out.push_str(&message[cursor..start]);

        let rest = &message[start..];
        let mut end = rest
            .find(|ch: char| {
                ch.is_whitespace() || matches!(ch, '"' | '\'' | ')' | ']' | '}' | '>' | ',' | ';')
            })
            .unwrap_or(rest.len());
        if end > 0 && rest.as_bytes()[end - 1] == b'.' {
            end -= 1;
        }
        let uri_slice = &rest[..end];
        out.push_str(&ariadeck_domain::redact_source_uri(uri_slice));
        cursor = start + end;
    }

    out
}

pub(crate) fn classify_task_error(error: ariadeck_domain::TaskError) -> TaskErrorView {
    let redacted = redact_error_message(error.message.trim());
    let message = redacted.as_str();
    let normalized = message.to_ascii_lowercase();
    let summary = if normalized.contains("permission denied")
        || normalized.contains("access is denied")
        || normalized.contains("access denied")
    {
        "Permission denied. Check the download directory and file permissions.".into()
    } else if normalized.contains("file name too long")
        || normalized.contains("filename too long")
        || normalized.contains("path too long")
        || normalized.contains("error 206")
    {
        "The output path is too long. Choose a shorter directory or filename.".into()
    } else {
        match error.code {
            Some(9) => "Not enough disk space in the download directory.".into(),
            Some(10) => "The downloaded piece length does not match the expected metadata.".into(),
            Some(11) => "Output conflict: another task is downloading the same file.".into(),
            Some(12) => "Output conflict: the same Torrent is already downloading.".into(),
            Some(13) => "Output conflict: the destination file already exists.".into(),
            Some(14) => {
                "aria2 could not rename the output file. Check the path and permissions.".into()
            }
            Some(15) => {
                "aria2 could not open the output file. Check that it exists and is accessible."
                    .into()
            }
            Some(16) => {
                "aria2 could not create the output file. Check the directory and permissions."
                    .into()
            }
            Some(17) => "A filesystem input/output error interrupted the download.".into(),
            Some(18) => "aria2 could not create the download directory.".into(),
            _ if !message.is_empty() => message.into(),
            Some(code) => format!("aria2 reported error code {code}."),
            None => "aria2 reported an unspecified download error.".into(),
        }
    };
    TaskErrorView {
        code: error.code,
        summary,
        details: (!message.is_empty()).then(|| message.to_owned()),
    }
}

pub(crate) fn sanitize_source_uri(uri: &str) -> String {
    ariadeck_domain::redact_source_uri(uri)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_task_error_redacts_embedded_urls_and_magnets_without_hiding_plain_text() {
        let plain = classify_task_error(ariadeck_domain::TaskError {
            code: Some(3),
            message: "The response status is not successful. status=404".into(),
        });
        assert_eq!(
            plain.summary,
            "The response status is not successful. status=404"
        );
        assert_eq!(
            plain.details.as_deref(),
            Some("The response status is not successful. status=404")
        );

        let url_err = classify_task_error(ariadeck_domain::TaskError {
            code: Some(22),
            message: "Failed fetching https://alice:s3cr3t@cdn.example.com/archive.zip?token=xyz."
                .into(),
        });
        assert!(!url_err.summary.contains("s3cr3t"));
        assert!(!url_err.summary.contains("token=xyz"));
        assert!(
            url_err
                .summary
                .contains("https://cdn.example.com/archive.zip")
        );
        assert_eq!(url_err.details.as_deref(), Some(url_err.summary.as_str()));

        let magnet_err = classify_task_error(ariadeck_domain::TaskError {
            code: None,
            message: "Rejected magnet:?xt=urn:btih:0123456789abcdef0123456789abcdef01234567&tr=https://t.example/announce?passkey=secret".into(),
        });
        assert!(!magnet_err.summary.contains("passkey=secret"));
        assert!(
            magnet_err
                .summary
                .contains("magnet:?xt=urn:btih:0123456789abcdef0123456789abcdef01234567")
        );
    }
}
