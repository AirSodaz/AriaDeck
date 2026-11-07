//! GitHub Release update discovery and installer verification.

use std::{path::Path, time::Duration};

use semver::Version;
use serde::Deserialize;

const RELEASE_API: &str = "https://api.github.com/repos/AirSodaz/AriaDeck/releases/latest";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
const MAX_METADATA_BYTES: usize = 2 * 1024 * 1024;
const MAX_CHECKSUM_BYTES: usize = 1024 * 1024;
const MAX_INSTALLER_BYTES: usize = 512 * 1024 * 1024;
const USER_AGENT: &str = concat!("AriaDeck/", env!("CARGO_PKG_VERSION"));

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
pub(crate) struct ReleaseAsset {
    pub(crate) name: String,
    pub(crate) browser_download_url: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
pub(crate) struct ReleaseInfo {
    pub(crate) tag_name: String,
    #[serde(default)]
    pub(crate) name: String,
    #[serde(default)]
    pub(crate) body: String,
    pub(crate) html_url: String,
    #[serde(default)]
    pub(crate) assets: Vec<ReleaseAsset>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct UpdateOffer {
    pub(crate) version: String,
    pub(crate) title: String,
    pub(crate) notes: String,
    pub(crate) release_url: String,
    pub(crate) installer_url: Option<String>,
    pub(crate) checksums_url: Option<String>,
}

pub(crate) fn supports_updates(os: &str, arch: &str, channel: Option<&str>) -> bool {
    os == "windows" && arch == "x86_64" && channel == Some("release")
}

pub(crate) fn parse_update_offer(
    current_version: &str,
    release: ReleaseInfo,
) -> Result<Option<UpdateOffer>, String> {
    let current = Version::parse(current_version)
        .map_err(|error| format!("Current app version is invalid: {error}"))?;
    let version = release
        .tag_name
        .strip_prefix('v')
        .unwrap_or(&release.tag_name);
    let latest = Version::parse(version)
        .map_err(|_| format!("Release tag is not a valid version: {}", release.tag_name))?;
    let release_url = validated_github_url(&release.html_url)?;
    if latest <= current {
        return Ok(None);
    }
    let installer = release
        .assets
        .iter()
        .find(|asset| asset.name.ends_with("-windows-x64-setup.exe"));
    let checksums = release
        .assets
        .iter()
        .find(|asset| asset.name == "SHA256SUMS.txt");
    Ok(Some(UpdateOffer {
        version: latest.to_string(),
        title: if release.name.trim().is_empty() {
            format!("AriaDeck {latest}")
        } else {
            release.name
        },
        notes: release.body,
        release_url,
        installer_url: installer
            .map(|asset| validated_github_url(&asset.browser_download_url))
            .transpose()?,
        checksums_url: checksums
            .map(|asset| validated_github_url(&asset.browser_download_url))
            .transpose()?,
    }))
}

fn validated_github_url(value: &str) -> Result<String, String> {
    let url = url::Url::parse(value).map_err(|_| "Release contains an invalid URL.".to_owned())?;
    if url.scheme() != "https"
        || !matches!(
            url.host_str(),
            Some(
                "github.com"
                    | "objects.githubusercontent.com"
                    | "release-assets.githubusercontent.com"
            )
        )
    {
        return Err("Release contains a URL outside GitHub's HTTPS domains.".into());
    }
    Ok(url.to_string())
}

pub(crate) async fn check_latest_release(
    current_version: &str,
) -> Result<Option<UpdateOffer>, String> {
    let client = reqwest::Client::builder()
        .timeout(REQUEST_TIMEOUT)
        .redirect(reqwest::redirect::Policy::limited(5))
        .user_agent(USER_AGENT)
        .build()
        .map_err(|error| format!("Could not initialize update check: {error}"))?;
    let mut response = client
        .get(RELEASE_API)
        .header(reqwest::header::ACCEPT, "application/vnd.github+json")
        .send()
        .await
        .map_err(|error| format!("Update check failed: {error}"))?;
    if !response.status().is_success() {
        return Err(format!(
            "Update check failed with HTTP {}.",
            response.status()
        ));
    }
    if response
        .content_length()
        .is_some_and(|size| size > MAX_METADATA_BYTES as u64)
    {
        return Err("Update metadata is too large.".into());
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|error| format!("Could not read update metadata: {error}"))?
    {
        if bytes.len().saturating_add(chunk.len()) > MAX_METADATA_BYTES {
            return Err("Update metadata is too large.".into());
        }
        bytes.extend_from_slice(&chunk);
    }
    let release: ReleaseInfo = serde_json::from_slice(&bytes)
        .map_err(|error| format!("Update metadata is invalid: {error}"))?;
    parse_update_offer(current_version, release)
}

async fn fetch_bounded(url: &str, limit: usize, label: &str) -> Result<Vec<u8>, String> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(300))
        .redirect(reqwest::redirect::Policy::limited(5))
        .user_agent(USER_AGENT)
        .build()
        .map_err(|error| format!("Could not initialize {label} download: {error}"))?;
    let mut response = client
        .get(url)
        .send()
        .await
        .map_err(|error| format!("{label} download failed: {error}"))?;
    if !response.status().is_success() {
        return Err(format!(
            "{label} download failed with HTTP {}.",
            response.status()
        ));
    }
    if response
        .content_length()
        .is_some_and(|size| size > limit as u64)
    {
        return Err(format!("{label} download is too large."));
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|error| format!("{label} download failed: {error}"))?
    {
        if bytes.len().saturating_add(chunk.len()) > limit {
            return Err(format!("{label} download is too large."));
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

pub(crate) async fn download_verified_installer(offer: &UpdateOffer) -> Result<Vec<u8>, String> {
    let installer_url = offer
        .installer_url
        .as_deref()
        .ok_or_else(|| "This release has no Windows x64 installer.".to_owned())?;
    let checksums_url = offer
        .checksums_url
        .as_deref()
        .ok_or_else(|| "This release has no SHA256SUMS.txt.".to_owned())?;
    let installer_name = url::Url::parse(installer_url)
        .ok()
        .and_then(|url| url.path_segments()?.next_back().map(str::to_owned))
        .filter(|name| name.ends_with("-windows-x64-setup.exe"))
        .ok_or_else(|| "The installer asset name is invalid.".to_owned())?;
    let (checksums, installer) = tokio::try_join!(
        fetch_bounded(checksums_url, MAX_CHECKSUM_BYTES, "Checksum file"),
        fetch_bounded(installer_url, MAX_INSTALLER_BYTES, "Installer"),
    )?;
    verify_sha256sum(&checksums, &installer_name, &installer)?;
    Ok(installer)
}

pub(crate) fn verify_sha256sum(
    checksums: &[u8],
    expected_name: &str,
    bytes: &[u8],
) -> Result<(), String> {
    let text = std::str::from_utf8(checksums)
        .map_err(|_| "Checksum file is not valid UTF-8.".to_owned())?;
    let expected = text
        .lines()
        .filter_map(|line| line.split_once("  "))
        .find_map(|(hash, name)| (name.trim_start_matches('*') == expected_name).then_some(hash));
    let expected =
        expected.ok_or_else(|| format!("Checksum file has no entry for {expected_name}."))?;
    if expected.len() != 64 || !expected.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err("Installer checksum entry is invalid.".into());
    }
    use sha2::{Digest as _, Sha256};
    let actual = Sha256::digest(bytes);
    let actual = actual
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    if !actual.eq_ignore_ascii_case(expected) {
        return Err("Installer failed SHA-256 verification.".into());
    }
    Ok(())
}

pub(crate) fn save_installer(
    bytes: &[u8],
    directory: &Path,
    version: &str,
) -> Result<std::path::PathBuf, String> {
    std::fs::create_dir_all(directory)
        .map_err(|error| format!("Could not prepare the update directory: {error}"))?;
    let path = directory.join(format!("AriaDeck-{version}-windows-x64-setup.exe"));
    std::fs::write(&path, bytes)
        .map_err(|error| format!("Could not save the verified installer: {error}"))?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn release(tag: &str) -> ReleaseInfo {
        ReleaseInfo {
            tag_name: tag.into(),
            name: String::new(),
            body: "Release notes".into(),
            html_url: format!("https://github.com/AirSodaz/AriaDeck/releases/tag/{tag}"),
            assets: vec![
                ReleaseAsset {
                    name: format!(
                        "AriaDeck-{}-windows-x64-setup.exe",
                        tag.trim_start_matches('v')
                    ),
                    browser_download_url: format!(
                        "https://github.com/AirSodaz/AriaDeck/releases/download/{tag}/AriaDeck-{}-windows-x64-setup.exe",
                        tag.trim_start_matches('v')
                    ),
                },
                ReleaseAsset {
                    name: "SHA256SUMS.txt".into(),
                    browser_download_url: format!(
                        "https://github.com/AirSodaz/AriaDeck/releases/download/{tag}/SHA256SUMS.txt"
                    ),
                },
            ],
        }
    }

    #[test]
    fn identifies_newer_release_and_windows_assets() {
        let offer = parse_update_offer("0.2.0", release("v0.3.0"))
            .expect("valid release")
            .expect("newer release");
        assert_eq!(offer.version, "0.3.0");
        assert!(
            offer
                .installer_url
                .as_deref()
                .unwrap_or_default()
                .ends_with("setup.exe")
        );
        assert!(offer.checksums_url.is_some());
    }

    #[test]
    fn ignores_same_or_older_releases() {
        assert!(
            parse_update_offer("0.3.0", release("v0.3.0"))
                .expect("same version")
                .is_none()
        );
        assert!(
            parse_update_offer("0.3.0", release("v0.2.9"))
                .expect("older version")
                .is_none()
        );
    }

    #[test]
    fn rejects_invalid_versions_and_non_github_urls() {
        assert!(parse_update_offer("bad", release("v0.3.0")).is_err());
        let mut invalid = release("v0.3.0");
        invalid.html_url = "https://example.com/release".into();
        assert!(parse_update_offer("0.2.0", invalid).is_err());
    }

    #[test]
    fn updates_are_limited_to_windows_x64_release_builds() {
        assert!(supports_updates("windows", "x86_64", Some("release")));
        assert!(!supports_updates("windows", "x86_64", Some("ci")));
        assert!(!supports_updates("windows", "x86_64", Some("dev")));
        assert!(!supports_updates("linux", "x86_64", Some("release")));
        assert!(!supports_updates("windows", "aarch64", Some("release")));
    }

    #[test]
    fn an_offer_without_required_installer_assets_cannot_be_installed() {
        let mut latest = release("v0.3.0");
        latest.assets.clear();
        let offer = parse_update_offer("0.2.0", latest)
            .expect("valid release metadata")
            .expect("newer version");
        assert!(offer.installer_url.is_none());
        assert!(offer.checksums_url.is_none());
    }

    #[test]
    fn requires_matching_sha256_entry() {
        let bytes = b"installer";
        use sha2::{Digest as _, Sha256};
        let hash = Sha256::digest(bytes)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        assert!(
            verify_sha256sum(
                format!("{hash}  setup.exe\n").as_bytes(),
                "setup.exe",
                bytes
            )
            .is_ok()
        );
        assert!(verify_sha256sum(b"bad-hash  setup.exe\n", "setup.exe", bytes).is_err());
        assert!(verify_sha256sum(b"", "setup.exe", bytes).is_err());
    }
}
