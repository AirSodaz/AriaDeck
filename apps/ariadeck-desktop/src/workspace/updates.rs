//! Multi-platform update discovery and installer verification.

use std::{collections::HashMap, path::Path, time::Duration};

use semver::Version;
use serde::Deserialize;

const UPDATER_MANIFEST_URL: &str =
    "https://github.com/AirSodaz/AriaDeck/releases/latest/download/updater.json";
const RELEASE_API: &str = "https://api.github.com/repos/AirSodaz/AriaDeck/releases/latest";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
const MAX_METADATA_BYTES: usize = 2 * 1024 * 1024;
const MAX_CHECKSUM_BYTES: usize = 1024 * 1024;
const MAX_SIGNATURE_BYTES: usize = 16 * 1024;
const MAX_INSTALLER_BYTES: usize = 512 * 1024 * 1024;
const USER_AGENT: &str = concat!("AriaDeck/", env!("CARGO_PKG_VERSION"));
const UPDATE_PUBLIC_KEY: Option<&str> = option_env!("ARIADECK_UPDATE_PUBLIC_KEY");

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
pub(crate) struct ManifestPlatform {
    pub(crate) signature: String,
    pub(crate) url: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
pub(crate) struct UpdaterManifest {
    pub(crate) version: String,
    #[serde(default)]
    pub(crate) notes: String,
    #[serde(default)]
    pub(crate) pub_date: Option<String>,
    #[serde(default)]
    pub(crate) platforms: HashMap<String, ManifestPlatform>,
}

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
    pub(crate) package_url: Option<String>,
    pub(crate) signature_data: Option<String>,
    pub(crate) signature_url: Option<String>,
    pub(crate) checksums_url: Option<String>,
}

pub(crate) fn target_platform_key(os: &str, arch: &str) -> Option<&'static str> {
    match (os, arch) {
        ("windows", "x86_64") => Some("windows-x86_64"),
        ("windows", "aarch64") => Some("windows-aarch64"),
        ("macos", "aarch64") => Some("darwin-aarch64"),
        ("macos", "x86_64") => Some("darwin-x86_64"),
        ("linux", "x86_64") => Some("linux-x86_64"),
        _ => None,
    }
}

pub(crate) fn supports_updates(os: &str, arch: &str, channel: Option<&str>) -> bool {
    target_platform_key(os, arch).is_some() && channel == Some("release")
}

pub(crate) fn parse_updater_manifest(
    current_version: &str,
    os: &str,
    arch: &str,
    manifest: UpdaterManifest,
) -> Result<Option<UpdateOffer>, String> {
    let current = Version::parse(current_version)
        .map_err(|error| format!("Current app version is invalid: {error}"))?;
    let version = manifest
        .version
        .strip_prefix('v')
        .unwrap_or(&manifest.version);
    let latest = Version::parse(version)
        .map_err(|_| format!("Release tag is not a valid version: {}", manifest.version))?;
    if latest <= current {
        return Ok(None);
    }
    let release_url = format!("https://github.com/AirSodaz/AriaDeck/releases/tag/v{latest}");
    let release_url = validated_github_url(&release_url)?;

    let platform_key = target_platform_key(os, arch);
    let platform = platform_key.and_then(|key| manifest.platforms.get(key));

    let package_url = platform.map(|p| validated_github_url(&p.url)).transpose()?;
    let signature_data = platform.map(|p| p.signature.clone());

    Ok(Some(UpdateOffer {
        version: latest.to_string(),
        title: format!("AriaDeck {latest}"),
        notes: manifest.notes,
        release_url,
        package_url,
        signature_data,
        signature_url: None,
        checksums_url: None,
    }))
}

pub(crate) fn parse_github_release_offer(
    current_version: &str,
    os: &str,
    arch: &str,
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

    let target_key = target_platform_key(os, arch);
    let package_asset = target_key.and_then(|key| match key {
        "windows-x86_64" => release
            .assets
            .iter()
            .find(|asset| asset.name.ends_with("x64-setup.exe")),
        "windows-aarch64" => release
            .assets
            .iter()
            .find(|asset| asset.name.ends_with("arm64-setup.exe")),
        "darwin-aarch64" => release
            .assets
            .iter()
            .find(|asset| asset.name.ends_with("aarch64.app.tar.gz")),
        "darwin-x86_64" => release
            .assets
            .iter()
            .find(|asset| asset.name.ends_with("x64.app.tar.gz")),
        "linux-x86_64" => release
            .assets
            .iter()
            .find(|asset| asset.name.ends_with("amd64.AppImage")),
        _ => None,
    });

    let signature_asset = package_asset.and_then(|package| {
        let sig_name = format!("{}.sig", package.name);
        let minisig_name = format!("{}.minisig", package.name);
        release
            .assets
            .iter()
            .find(|asset| asset.name == sig_name || asset.name == minisig_name)
    });

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
        package_url: package_asset
            .map(|asset| validated_github_url(&asset.browser_download_url))
            .transpose()?,
        signature_data: None,
        signature_url: signature_asset
            .map(|asset| validated_github_url(&asset.browser_download_url))
            .transpose()?,
        checksums_url: checksums
            .map(|asset| validated_github_url(&asset.browser_download_url))
            .transpose()?,
    }))
}

#[cfg(test)]
pub(crate) fn parse_update_offer(
    current_version: &str,
    release: ReleaseInfo,
) -> Result<Option<UpdateOffer>, String> {
    parse_github_release_offer(current_version, "windows", "x86_64", release)
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
    os: &str,
    arch: &str,
) -> Result<Option<UpdateOffer>, String> {
    match fetch_manifest_bytes().await {
        Ok(bytes) => {
            let manifest: UpdaterManifest = serde_json::from_slice(&bytes)
                .map_err(|error| format!("Update manifest is invalid: {error}"))?;
            parse_updater_manifest(current_version, os, arch, manifest)
        }
        Err(manifest_err) => {
            tracing::info!(
                %manifest_err,
                "could not fetch updater manifest from CDN, falling back to GitHub releases API"
            );
            let bytes = fetch_release_api_bytes().await?;
            let release: ReleaseInfo = serde_json::from_slice(&bytes)
                .map_err(|error| format!("Update metadata is invalid: {error}"))?;
            parse_github_release_offer(current_version, os, arch, release)
        }
    }
}

async fn fetch_manifest_bytes() -> Result<Vec<u8>, String> {
    fetch_bounded(
        UPDATER_MANIFEST_URL,
        MAX_METADATA_BYTES,
        "Update manifest",
        &[("Accept", "application/json")],
    )
    .await
}

async fn fetch_release_api_bytes() -> Result<Vec<u8>, String> {
    fetch_bounded(
        RELEASE_API,
        MAX_METADATA_BYTES,
        "Update metadata",
        &[("Accept", "application/vnd.github+json")],
    )
    .await
}

async fn fetch_bounded(
    url: &str,
    limit: usize,
    label: &str,
    headers: &[(&str, &str)],
) -> Result<Vec<u8>, String> {
    let client = reqwest::Client::builder()
        .timeout(REQUEST_TIMEOUT)
        .redirect(reqwest::redirect::Policy::limited(5))
        .user_agent(USER_AGENT)
        .build()
        .map_err(|error| format!("Could not initialize {label} download: {error}"))?;
    let mut request = client.get(url);
    for (name, value) in headers {
        request = request.header(*name, *value);
    }
    let mut response = request
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

pub(crate) async fn download_verified_package<F>(
    offer: &UpdateOffer,
    mut on_progress: F,
) -> Result<Vec<u8>, String>
where
    F: FnMut(u64, Option<u64>),
{
    let package_url = offer
        .package_url
        .as_deref()
        .ok_or_else(|| "This release has no download package for your platform.".to_owned())?;

    let signature_bytes = if let Some(sig_data) = &offer.signature_data {
        sig_data.as_bytes().to_vec()
    } else if let Some(sig_url) = &offer.signature_url {
        fetch_bounded(sig_url, MAX_SIGNATURE_BYTES, "Package signature", &[]).await?
    } else {
        return Err("This release has no package signature.".to_owned());
    };

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(300))
        .redirect(reqwest::redirect::Policy::limited(5))
        .user_agent(USER_AGENT)
        .build()
        .map_err(|error| format!("Could not initialize package download: {error}"))?;

    let mut response = client
        .get(package_url)
        .send()
        .await
        .map_err(|error| format!("Package download failed: {error}"))?;

    if !response.status().is_success() {
        return Err(format!(
            "Package download failed with HTTP {}.",
            response.status()
        ));
    }

    let total_bytes = response.content_length();
    if total_bytes.is_some_and(|size| size > MAX_INSTALLER_BYTES as u64) {
        return Err("Package download is too large.".into());
    }

    let mut bytes = Vec::new();
    let mut last_progress_report = 0u64;
    on_progress(0, total_bytes);

    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|error| format!("Package download failed: {error}"))?
    {
        if bytes.len().saturating_add(chunk.len()) > MAX_INSTALLER_BYTES {
            return Err("Package download is too large.".into());
        }
        bytes.extend_from_slice(&chunk);
        let current_len = bytes.len() as u64;
        if current_len.saturating_sub(last_progress_report) >= 256 * 1024
            || total_bytes == Some(current_len)
        {
            last_progress_report = current_len;
            on_progress(current_len, total_bytes);
        }
    }

    verify_minisign(&signature_bytes, &bytes)?;

    if let Some(checksums_url) = &offer.checksums_url {
        let package_name = url::Url::parse(package_url)
            .ok()
            .and_then(|url| url.path_segments()?.next_back().map(str::to_owned));
        let checksums_bytes =
            fetch_bounded(checksums_url, MAX_CHECKSUM_BYTES, "Checksum file", &[]).await;
        if let (Some(name), Ok(checksums_bytes)) = (package_name, checksums_bytes) {
            verify_sha256sum(&checksums_bytes, &name, &bytes)?;
        }
    }

    Ok(bytes)
}

fn verify_minisign(signature: &[u8], bytes: &[u8]) -> Result<(), String> {
    let public_key = UPDATE_PUBLIC_KEY
        .ok_or_else(|| "This build has no trusted update signing key.".to_owned())?;
    verify_minisign_with_key(public_key, signature, bytes)
}

fn verify_minisign_with_key(
    public_key: &str,
    signature: &[u8],
    bytes: &[u8],
) -> Result<(), String> {
    let encoded_key = public_key
        .lines()
        .filter(|line| !line.starts_with("untrusted comment:"))
        .map(str::trim)
        .find(|line| !line.is_empty())
        .ok_or_else(|| "The trusted update signing key is invalid.".to_owned())?;
    let public_key = minisign_verify::PublicKey::from_base64(encoded_key)
        .map_err(|_| "The trusted update signing key is invalid.".to_owned())?;
    let signature_str = std::str::from_utf8(signature)
        .map_err(|_| "Package signature is not valid UTF-8.".to_owned())?;

    let decoded_bytes;
    let effective_sig_str = if !signature_str.starts_with("untrusted comment:") {
        use data_encoding::BASE64;
        let clean: String = signature_str
            .chars()
            .filter(|c| !c.is_whitespace())
            .collect();
        if let Ok(bytes) = BASE64.decode(clean.as_bytes()) {
            decoded_bytes = bytes;
            std::str::from_utf8(&decoded_bytes)
                .map_err(|_| "Package signature Base64 payload is not valid UTF-8.".to_owned())?
        } else {
            signature_str
        }
    } else {
        signature_str
    };

    let signature = minisign_verify::Signature::decode(effective_sig_str)
        .map_err(|_| "Package signature is invalid.".to_owned())?;
    public_key
        .verify(bytes, &signature, true)
        .map_err(|_| "Package signature verification failed.".to_owned())
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
        return Err("Package checksum entry is invalid.".into());
    }
    use sha2::{Digest as _, Sha256};
    let actual = Sha256::digest(bytes);
    let actual = actual
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    if !actual.eq_ignore_ascii_case(expected) {
        return Err("Package failed SHA-256 verification.".into());
    }
    Ok(())
}

pub(crate) fn save_package(
    bytes: &[u8],
    directory: &Path,
    version: &str,
    os: &str,
    arch: &str,
) -> Result<std::path::PathBuf, String> {
    std::fs::create_dir_all(directory)
        .map_err(|error| format!("Could not prepare the update directory: {error}"))?;
    let target = target_platform_key(os, arch).unwrap_or("unknown");
    let ext = match os {
        "windows" => "exe",
        "macos" => "app.tar.gz",
        "linux" => "AppImage",
        _ => "bin",
    };
    let path = directory.join(format!("AriaDeck_{version}_{target}.{ext}"));
    std::fs::write(&path, bytes)
        .map_err(|error| format!("Could not save the verified package: {error}"))?;
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
                    name: format!(
                        "AriaDeck-{}-windows-x64-setup.exe.minisig",
                        tag.trim_start_matches('v')
                    ),
                    browser_download_url: format!(
                        "https://github.com/AirSodaz/AriaDeck/releases/download/{tag}/AriaDeck-{}-windows-x64-setup.exe.minisig",
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
                .package_url
                .as_deref()
                .unwrap_or_default()
                .ends_with("setup.exe")
        );
        assert!(offer.checksums_url.is_some());
        assert!(offer.signature_url.is_some());
    }

    #[test]
    fn parses_manifest_for_multiple_platforms() {
        let mut platforms = HashMap::new();
        platforms.insert(
            "darwin-aarch64".into(),
            ManifestPlatform {
                signature: "sig-mac-arm".into(),
                url: "https://github.com/AirSodaz/AriaDeck/releases/download/v0.3.0/AriaDeck_0.3.0_aarch64.app.tar.gz".into(),
            },
        );
        platforms.insert(
            "windows-x86_64".into(),
            ManifestPlatform {
                signature: "sig-win-x64".into(),
                url: "https://github.com/AirSodaz/AriaDeck/releases/download/v0.3.0/AriaDeck_0.3.0_x64-setup.exe".into(),
            },
        );
        platforms.insert(
            "linux-x86_64".into(),
            ManifestPlatform {
                signature: "sig-linux-x64".into(),
                url: "https://github.com/AirSodaz/AriaDeck/releases/download/v0.3.0/AriaDeck_0.3.0_amd64.AppImage".into(),
            },
        );

        let manifest = UpdaterManifest {
            version: "0.3.0".into(),
            notes: "Notes".into(),
            pub_date: None,
            platforms,
        };

        let mac_offer = parse_updater_manifest("0.2.0", "macos", "aarch64", manifest.clone())
            .expect("valid")
            .expect("offer");
        assert_eq!(mac_offer.version, "0.3.0");
        assert_eq!(
            mac_offer.package_url.as_deref(),
            Some(
                "https://github.com/AirSodaz/AriaDeck/releases/download/v0.3.0/AriaDeck_0.3.0_aarch64.app.tar.gz"
            )
        );
        assert_eq!(mac_offer.signature_data.as_deref(), Some("sig-mac-arm"));

        let win_offer = parse_updater_manifest("0.2.0", "windows", "x86_64", manifest.clone())
            .expect("valid")
            .expect("offer");
        assert_eq!(
            win_offer.package_url.as_deref(),
            Some(
                "https://github.com/AirSodaz/AriaDeck/releases/download/v0.3.0/AriaDeck_0.3.0_x64-setup.exe"
            )
        );

        let linux_offer = parse_updater_manifest("0.2.0", "linux", "x86_64", manifest)
            .expect("valid")
            .expect("offer");
        assert_eq!(
            linux_offer.package_url.as_deref(),
            Some(
                "https://github.com/AirSodaz/AriaDeck/releases/download/v0.3.0/AriaDeck_0.3.0_amd64.AppImage"
            )
        );
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
    fn target_platform_supports_expanded_platforms() {
        assert!(supports_updates("windows", "x86_64", Some("release")));
        assert!(supports_updates("windows", "aarch64", Some("release")));
        assert!(supports_updates("macos", "aarch64", Some("release")));
        assert!(supports_updates("macos", "x86_64", Some("release")));
        assert!(supports_updates("linux", "x86_64", Some("release")));

        assert!(!supports_updates("windows", "x86_64", Some("ci")));
        assert!(!supports_updates("windows", "x86_64", Some("dev")));
        assert!(!supports_updates("linux", "aarch64", Some("release")));
        assert!(!supports_updates("freebsd", "x86_64", Some("release")));
    }

    #[test]
    fn an_offer_without_required_installer_assets_cannot_be_installed() {
        let mut latest = release("v0.3.0");
        latest.assets.clear();
        let offer = parse_update_offer("0.2.0", latest)
            .expect("valid release metadata")
            .expect("newer version");
        assert!(offer.package_url.is_none());
        assert!(offer.signature_url.is_none());
        assert!(offer.signature_data.is_none());
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

    #[test]
    fn rejects_invalid_minisign_inputs() {
        assert!(verify_minisign(b"not a signature", b"installer").is_err());
    }

    #[test]
    fn verifies_minisign_signature_and_rejects_modified_payload() {
        let public_key = "untrusted comment: minisign public key E7620F1842B4E81F\nRWQf6LRCGA9i53mlYecO4IzT51TGPpvWucNSCh1CBM0QTaLn73Y7GFO3";
        let signature = br#"untrusted comment: signature from minisign secret key
RWQf6LRCGA9i59SLOFxz6NxvASXDJeRtuZykwQepbDEGt87ig1BNpWaVWuNrm73YiIiJbq71Wi+dP9eKL8OC351vwIasSSbXxwA=
trusted comment: timestamp:1555779966	file:test
QtKMXWyYcwdpZAlPF7tE2ENJkRd1ujvKjlj1m9RtHTBnZPa5WKU5uWRs5GoP5M/VqE81QFuMKI5k/SfNQUaOAA=="#;
        assert!(verify_minisign_with_key(public_key, signature, b"test").is_ok());
        assert!(verify_minisign_with_key(public_key, signature, b"tampered").is_err());
    }

    #[test]
    fn verifies_base64_encoded_signature_and_rejects_modified_payload() {
        let public_key = "untrusted comment: minisign public key E7620F1842B4E81F\nRWQf6LRCGA9i53mlYecO4IzT51TGPpvWucNSCh1CBM0QTaLn73Y7GFO3";
        let signature = br#"untrusted comment: signature from minisign secret key
RWQf6LRCGA9i59SLOFxz6NxvASXDJeRtuZykwQepbDEGt87ig1BNpWaVWuNrm73YiIiJbq71Wi+dP9eKL8OC351vwIasSSbXxwA=
trusted comment: timestamp:1555779966	file:test
QtKMXWyYcwdpZAlPF7tE2ENJkRd1ujvKjlj1m9RtHTBnZPa5WKU5uWRs5GoP5M/VqE81QFuMKI5k/SfNQUaOAA=="#;
        let base64_sig = data_encoding::BASE64.encode(signature);
        assert!(verify_minisign_with_key(public_key, base64_sig.as_bytes(), b"test").is_ok());
        assert!(verify_minisign_with_key(public_key, base64_sig.as_bytes(), b"tampered").is_err());
    }
}
