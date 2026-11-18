//! Shared validation and fallback-directory synchronization helpers for
//! per-profile engine environment fields (shared by `AppSettings` and
//! `ProfileEnvironment` without Serde `flatten` so `deny_unknown_fields` stays intact).

use std::path::{Path, PathBuf};

use crate::{
    DownloadCategory, DownloadProxySettings, SettingsError, TrackerListSettings,
    TransferPolicySettings, validate_categories,
};

pub(crate) fn validate_engine_environment(
    download_directory: &Path,
    download_proxy: &DownloadProxySettings,
    transfer_policy: &TransferPolicySettings,
    categories: &[DownloadCategory],
    tracker_list: &TrackerListSettings,
) -> Result<(), SettingsError> {
    if download_directory.as_os_str().is_empty() {
        return Err(SettingsError::EmptyDownloadDirectory);
    }
    download_proxy.validate()?;
    transfer_policy.validate()?;
    validate_categories(categories)?;
    tracker_list.validate()?;
    Ok(())
}

pub(crate) fn sync_fallback_directory(
    download_directory: &mut PathBuf,
    categories: &[DownloadCategory],
) {
    if let Some(fallback) = categories.iter().find(|category| category.is_fallback) {
        *download_directory = fallback.directory.clone();
    } else if let Some(first) = categories.first() {
        *download_directory = first.directory.clone();
    }
}
