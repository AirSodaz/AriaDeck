//! Split from workspace.rs — settings_bridge.

use super::*;

pub(crate) struct CredentialMutation {
    pub(crate) credential: Option<ProxyCredentialRef>,
    pub(crate) previous_password: Option<SecretString>,
}

pub(crate) async fn rollback_credential_async(
    store: Arc<dyn ProxyCredentialStore>,
    mutation: CredentialMutation,
) -> Result<(), String> {
    tokio::task::spawn_blocking(move || rollback_credential(store.as_ref(), &mutation))
        .await
        .map_err(|error| format!("credential rollback task failed: {error}"))?
}

pub(crate) fn apply_credential_update(
    store: &dyn ProxyCredentialStore,
    previous: &AppSettings,
    next: &AppSettings,
    update: &ProxyPasswordUpdate,
    previous_password: Option<SecretString>,
) -> Result<(Option<SecretString>, CredentialMutation), String> {
    match update {
        ProxyPasswordUpdate::Unchanged => {
            // Only Manual mode applies keychain credentials to aria2. System/Disabled
            // may still retain a credential ref on disk for when the user switches back.
            if next.download_proxy.mode == DownloadProxyMode::Manual
                && next.download_proxy.credential.is_some()
                && previous_password.is_none()
            {
                return Err("The saved proxy password is missing from the system credential store. Enter it again or clear the saved password.".into());
            }
            Ok((
                previous_password,
                CredentialMutation {
                    credential: None,
                    previous_password: None,
                },
            ))
        }
        ProxyPasswordUpdate::Detach => Ok((
            None,
            CredentialMutation {
                credential: None,
                previous_password: None,
            },
        )),
        ProxyPasswordUpdate::Clear => {
            if let Some(credential) = previous.download_proxy.credential {
                store
                    .delete(credential)
                    .map_err(|error| error.to_string())?;
            }
            Ok((
                None,
                CredentialMutation {
                    credential: previous.download_proxy.credential,
                    previous_password,
                },
            ))
        }
        ProxyPasswordUpdate::Set(password) => {
            let credential = next.download_proxy.credential.ok_or_else(|| {
                "A proxy credential reference was not allocated for the new password.".to_owned()
            })?;
            store
                .save(credential, password)
                .map_err(|error| error.to_string())?;
            Ok((
                Some(password.clone()),
                CredentialMutation {
                    credential: Some(credential),
                    previous_password,
                },
            ))
        }
    }
}

pub(crate) fn rollback_credential(
    store: &dyn ProxyCredentialStore,
    mutation: &CredentialMutation,
) -> Result<(), String> {
    let Some(credential) = mutation.credential else {
        return Ok(());
    };
    if let Some(password) = &mutation.previous_password {
        store
            .save(credential, password)
            .map_err(|error| error.to_string())
    } else {
        store.delete(credential).map_err(|error| error.to_string())
    }
}

pub(crate) fn load_proxy_password(
    store: &dyn ProxyCredentialStore,
    settings: &AppSettings,
) -> Result<Option<SecretString>, String> {
    settings
        .download_proxy
        .credential
        .map_or(Ok(None), |credential| {
            store.load(credential).map_err(|error| error.to_string())
        })
}

/// Best-effort proxy mapping used on rollback paths. Resolution failures fall back to
/// clearing the engine proxy rather than aborting an already-partial recovery.
pub(crate) fn map_download_proxy_config_or_clear(
    settings: &AppSettings,
    password: Option<SecretString>,
) -> DownloadProxyConfig {
    map_download_proxy_config(settings, password).unwrap_or_else(|_| DownloadProxyConfig {
        mode: ApplicationProxyMode::Disabled,
        ..DownloadProxyConfig::default()
    })
}

pub(crate) fn map_download_proxy_config(
    settings: &AppSettings,
    password: Option<SecretString>,
) -> Result<DownloadProxyConfig, String> {
    match settings.download_proxy.mode {
        DownloadProxyMode::Disabled => Ok(DownloadProxyConfig {
            mode: ApplicationProxyMode::Disabled,
            check_certificate: settings.download_proxy.check_certificate,
            ..DownloadProxyConfig::default()
        }),
        DownloadProxyMode::Manual => Ok(DownloadProxyConfig {
            mode: ApplicationProxyMode::Manual,
            all_proxy: settings.download_proxy.all_proxy.clone(),
            http_proxy: settings.download_proxy.http_proxy.clone(),
            https_proxy: settings.download_proxy.https_proxy.clone(),
            ftp_proxy: settings.download_proxy.ftp_proxy.clone(),
            no_proxy: settings.download_proxy.no_proxy.clone(),
            username: settings.download_proxy.username.clone(),
            password,
            check_certificate: settings.download_proxy.check_certificate,
        }),
        DownloadProxyMode::System => {
            let resolved = resolve_system_proxy().map_err(|error| error.to_string())?;
            // Direct / empty system proxy: clear aria2 proxy options (same as Disabled).
            if resolved.is_empty() {
                return Ok(DownloadProxyConfig {
                    mode: ApplicationProxyMode::Disabled,
                    check_certificate: settings.download_proxy.check_certificate,
                    ..DownloadProxyConfig::default()
                });
            }
            Ok(DownloadProxyConfig {
                mode: ApplicationProxyMode::System,
                all_proxy: resolved.all_proxy,
                http_proxy: resolved.http_proxy,
                https_proxy: resolved.https_proxy,
                ftp_proxy: resolved.ftp_proxy,
                no_proxy: resolved.no_proxy,
                // System mode never auto-fills credentials (D-004 / ADR-008).
                username: None,
                password: None,
                check_certificate: settings.download_proxy.check_certificate,
            })
        }
    }
}

pub(crate) fn map_speed_limit_config(settings: &AppSettings) -> SpeedLimitConfig {
    SpeedLimitConfig {
        download_limit: ByteRate::new(settings.speed_limits.download_limit),
        upload_limit: ByteRate::new(settings.speed_limits.upload_limit),
    }
}

pub(crate) fn map_transfer_policy_config(settings: &AppSettings) -> TransferPolicyConfig {
    TransferPolicyConfig {
        max_concurrent_downloads: settings.transfer_policy.max_concurrent_downloads,
        max_connection_per_server: settings.transfer_policy.max_connection_per_server,
        split: settings.transfer_policy.split,
        min_split_size: settings.transfer_policy.min_split_size,
        file_allocation: match settings.transfer_policy.file_allocation {
            FileAllocationSetting::None => ariadeck_domain::FileAllocationMethod::None,
            FileAllocationSetting::Prealloc => ariadeck_domain::FileAllocationMethod::Prealloc,
            FileAllocationSetting::Trunc => ariadeck_domain::FileAllocationMethod::Trunc,
            FileAllocationSetting::Falloc => ariadeck_domain::FileAllocationMethod::Falloc,
        },
        check_integrity: settings.transfer_policy.check_integrity,
    }
}

#[cfg(test)]
pub(crate) fn persist_settings(
    store: &JsonSettingsStore,
    settings: &AppSettings,
    destination_gateway: Option<&dyn DownloadDestinationGateway>,
) -> Result<(), String> {
    preflight_settings(settings, destination_gateway)?;
    store.save(settings).map_err(|error| error.to_string())
}

pub(crate) fn preflight_settings(
    settings: &AppSettings,
    destination_gateway: Option<&dyn DownloadDestinationGateway>,
) -> Result<(), String> {
    settings.validate().map_err(|error| error.to_string())?;
    if let Some(gateway) = destination_gateway {
        if !settings.download_directory.is_absolute() {
            return Err(format!(
                "Local download directory must be absolute: {}",
                settings.download_directory.display()
            ));
        }
        fs::create_dir_all(&settings.download_directory).map_err(|error| {
            format!(
                "Failed to create download directory {}: {error}",
                settings.download_directory.display()
            )
        })?;
        gateway
            .preflight(&DownloadDestinationRequest {
                directory: EnginePath::new(settings.download_directory.to_string_lossy()),
                required_bytes: None,
                files: Vec::new(),
            })
            .map_err(|error| error.message)?;
    }
    Ok(())
}

pub(crate) fn spawn_settings_result_bridge(
    mut results: mpsc::UnboundedReceiver<SettingsPersistenceResult>,
    window: &Window,
    cx: &mut Context<DesktopRoot>,
) {
    cx.spawn_in(window, async move |this, cx| {
        while let Some(result) = results.recv().await {
            if this
                .update_in(cx, |this, window, cx| {
                    if result.result.is_ok() {
                        this.settings = result.settings.clone();
                        this.sync_tray_with_settings(cx);
                    }
                    let outcome = result.result.map_or_else(
                        |summary| {
                            SettingsSaveOutcomeView::Failure(OperationErrorView {
                                code: "settings.save_failed".into(),
                                summary,
                                retryable: true,
                            })
                        },
                        |()| SettingsSaveOutcomeView::Success,
                    );
                    this.workspace.update(cx, |workspace, cx| {
                        workspace.set_settings_save_result(
                            SettingsSaveResultView {
                                request_id: result.request_id,
                                settings: map_settings(&result.settings),
                                outcome,
                            },
                            window,
                            cx,
                        );
                    });
                })
                .is_err()
            {
                break;
            }
        }
    })
    .detach();
}

pub(crate) fn spawn_proxy_reapply_bridge(
    runtime: tokio::runtime::Handle,
    handle: SyncHandle,
    store: JsonSettingsStore,
    credential_store: Arc<dyn ProxyCredentialStore>,
    cx: &mut Context<DesktopRoot>,
) {
    let mut events = handle.subscribe();
    cx.spawn(async move |this, cx| {
        let mut attempted_session = None;
        loop {
            let Some(snapshot) = handle.snapshot(TaskListQuery::default()).await else {
                break;
            };
            if matches!(snapshot.connection_state, ConnectionState::Connected)
                && attempted_session.as_ref() != Some(&snapshot.session)
            {
                attempted_session = Some(snapshot.session);
                let settings_store = store.clone();
                let credentials = credential_store.clone();
                let loaded = spawn_proxy_settings_load(&runtime, settings_store, credentials)
                    .await
                    .map_err(|error| format!("proxy configuration task failed: {error}"))
                    .and_then(|result| result);
                let result = match loaded {
                    Ok((settings, password)) => {
                        let proxy_result = match map_download_proxy_config(&settings, password) {
                            Ok(proxy) => handle
                                .apply_download_proxy(snapshot.session, proxy)
                                .await
                                .map_err(|error| {
                                    format!(
                                        "Download proxy settings were not applied: {}",
                                        error.summary
                                    )
                                }),
                            Err(error) => {
                                Err(format!("Download proxy settings were not applied: {error}"))
                            }
                        };
                        // Reapply persisted speed limits and transfer policy on
                        // the fresh session so a reconnect restores the user's
                        // throttle and connection defaults.
                        let speed_result = handle
                            .apply_speed_limit(snapshot.session, map_speed_limit_config(&settings))
                            .await
                            .map_err(|error| {
                                format!("Speed limits were not applied: {}", error.summary)
                            });
                        let policy_result = handle
                            .apply_transfer_policy(
                                snapshot.session,
                                map_transfer_policy_config(&settings),
                            )
                            .await
                            .map_err(|error| {
                                format!("Transfer policy was not applied: {}", error.summary)
                            });
                        proxy_result.and(speed_result).and(policy_result)
                    }
                    Err(error) => Err(error),
                };
                if let Err(error) = result
                    && this
                        .update(cx, |this, cx| {
                            this.workspace.update(cx, |workspace, cx| {
                                workspace.set_startup_notice(error, true, cx);
                            });
                        })
                        .is_err()
                {
                    break;
                }
            }

            match events.recv().await {
                Ok(_) | Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            }
        }
    })
    .detach();
}

pub(crate) fn spawn_proxy_settings_load(
    runtime: &tokio::runtime::Handle,
    store: JsonSettingsStore,
    credential_store: Arc<dyn ProxyCredentialStore>,
) -> JoinHandle<Result<(AppSettings, Option<SecretString>), String>> {
    runtime.spawn_blocking(move || {
        let settings = store.load().map_err(|error| error.to_string())?;
        // System / Disabled modes never send keychain credentials to aria2.
        if !matches!(settings.download_proxy.mode, DownloadProxyMode::Manual) {
            return Ok((settings, None));
        }
        let password = load_proxy_password(credential_store.as_ref(), &settings)?;
        if settings.download_proxy.credential.is_some() && password.is_none() {
            return Err(
                "The saved proxy password is missing from the system credential store. Enter it again or clear the saved password."
                    .into(),
            );
        }
        Ok((settings, password))
    })
}

pub(crate) fn spawn_local_engine_health_bridge(
    health_handle: LocalEngineHealthHandle,
    cx: &mut Context<DesktopRoot>,
) {
    let executor = cx.background_executor().clone();
    cx.spawn(async move |this, cx| {
        let mut previous = None;
        while let Some(health) = health_handle.health() {
            if previous.as_ref() != Some(&health) {
                let view = map_local_engine_health(health.clone());
                previous = Some(health);
                if this
                    .update(cx, |this, cx| {
                        this.workspace.update(cx, |workspace, cx| {
                            workspace.set_engine_health(view, cx);
                        });
                    })
                    .is_err()
                {
                    break;
                }
            }
            executor.timer(Duration::from_millis(250)).await;
        }
    })
    .detach();
}

pub(crate) fn spawn_snapshot_bridge(
    handle: SyncHandle,
    mut query_receiver: watch::Receiver<TaskListQuery>,
    local_path_actions_available: bool,
    cx: &mut Context<DesktopRoot>,
) {
    let mut events = handle.subscribe();
    cx.spawn(async move |this, cx| {
        loop {
            let query = query_receiver.borrow().clone();
            let Some(snapshot) = handle.snapshot(query.clone()).await else {
                break;
            };
            if *query_receiver.borrow() == query {
                let snapshot = map_snapshot(snapshot, local_path_actions_available);
                if this
                    .update(cx, |this, cx| {
                        this.workspace.update(cx, |workspace, cx| {
                            workspace.set_snapshot(snapshot, cx);
                        });
                    })
                    .is_err()
                {
                    break;
                }
            }

            tokio::select! {
                event = events.recv() => match event {
                    Ok(_) | Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                },
                changed = query_receiver.changed() => {
                    if changed.is_err() {
                        break;
                    }
                }
            }
        }
    })
    .detach();
}
