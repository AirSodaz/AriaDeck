//! Verified aria2 core download (B4).
//!
//! The engine is never fetched on a schedule, on startup, or as a fallback: the
//! only thing that reaches the network here is a button press. What it may fetch
//! is pinned at compile time — URL, byte size, and SHA-256 all come from
//! [`CORE_DOWNLOAD_CATALOG`], so a compromised or rewritten release cannot be
//! installed even if the URL still resolves. Platforms upstream does not publish
//! binaries for simply have no entry, and the UI says so instead of guessing.
//!
//! Once verified, the archive member is extracted to a temp directory and handed
//! to `CoreStore::install_downloaded_executable`, which re-probes `--version` and
//! registers it like any other managed core (D-029).

use std::{
    io::{Cursor, Read as _, Write as _},
    path::{Path, PathBuf},
    time::Duration,
};

use ariadeck_engine::{CoreInstallationView, CoreStore};

use super::*;

const FETCH_TIMEOUT: Duration = Duration::from_secs(300);
const USER_AGENT: &str = concat!("AriaDeck/", env!("CARGO_PKG_VERSION"));
/// Hard ceiling on the extracted executable, independent of the catalog entry.
/// Guards against a zip whose local header lies about the member's size.
const MAX_EXTRACTED_BYTES: u64 = 64 * 1024 * 1024;

/// One pinned, checksum-verified aria2 build.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct CoreDownloadEntry {
    /// Host target string, matching `CoreStore`'s `{os}-{arch}` convention.
    pub(crate) target: &'static str,
    pub(crate) version: &'static str,
    pub(crate) url: &'static str,
    /// Exact archive size. A mismatch fails before hashing.
    pub(crate) size_bytes: u64,
    /// Lowercase hex SHA-256 of the archive as published upstream.
    pub(crate) sha256: &'static str,
    /// Path of the `aria2c` member inside the archive.
    pub(crate) archive_member: &'static str,
    /// True when the pinned build is not native to the target and runs through
    /// OS emulation (x64 on Windows ARM). Surfaced so the offer stays honest.
    pub(crate) emulated: bool,
}

/// Pinned upstream builds.
///
/// aria2 publishes prebuilt binaries for Windows only; macOS and Linux users get
/// aria2 from a package manager, which discovery already finds. Adding a target
/// here means checking in a checksum someone verified by hand — never a hash the
/// app learned from the same server that served the file.
pub(crate) const CORE_DOWNLOAD_CATALOG: &[CoreDownloadEntry] = &[
    CoreDownloadEntry {
        target: "windows-x86_64",
        version: "1.37.0",
        url: "https://github.com/aria2/aria2/releases/download/release-1.37.0/aria2-1.37.0-win-64bit-build1.zip",
        size_bytes: 2_475_379,
        sha256: "67d015301eef0b612191212d564c5bb0a14b5b9c4796b76454276a4d28d9b288",
        archive_member: "aria2-1.37.0-win-64bit-build1/aria2c.exe",
        emulated: false,
    },
    CoreDownloadEntry {
        // Windows on ARM runs the x64 build under emulation; upstream ships no
        // native aarch64 Windows binary.
        target: "windows-aarch64",
        version: "1.37.0",
        url: "https://github.com/aria2/aria2/releases/download/release-1.37.0/aria2-1.37.0-win-64bit-build1.zip",
        size_bytes: 2_475_379,
        sha256: "67d015301eef0b612191212d564c5bb0a14b5b9c4796b76454276a4d28d9b288",
        archive_member: "aria2-1.37.0-win-64bit-build1/aria2c.exe",
        emulated: true,
    },
    CoreDownloadEntry {
        target: "windows-x86",
        version: "1.37.0",
        url: "https://github.com/aria2/aria2/releases/download/release-1.37.0/aria2-1.37.0-win-32bit-build1.zip",
        size_bytes: 2_558_495,
        sha256: "35f6514cc5dd7e98a87b3c4c2d25a0754b9b063dbe59bc0f22d483464f61e5b6",
        archive_member: "aria2-1.37.0-win-32bit-build1/aria2c.exe",
        emulated: false,
    },
];

/// `{os}-{arch}` for this build, matching `CoreStore`'s target naming.
#[must_use]
pub(crate) fn host_target() -> String {
    let arch = std::env::consts::ARCH;
    let os = std::env::consts::OS;
    match (os, arch) {
        ("windows", "x86") => "windows-x86".into(),
        (os, arch) => format!("{os}-{arch}"),
    }
}

/// The pinned build for a target, if upstream publishes one.
#[must_use]
pub(crate) fn catalog_entry_for(target: &str) -> Option<&'static CoreDownloadEntry> {
    CORE_DOWNLOAD_CATALOG
        .iter()
        .find(|entry| entry.target == target)
}

/// The pinned build for the machine AriaDeck is running on.
#[must_use]
pub(crate) fn catalog_entry_for_host() -> Option<&'static CoreDownloadEntry> {
    catalog_entry_for(&host_target())
}

#[derive(Clone, Debug)]
pub(crate) struct CoreDownloadRequest {
    pub(crate) request_id: ariadeck_ui::RequestId,
}

#[derive(Debug)]
pub(crate) struct CoreDownloadResult {
    pub(crate) request_id: ariadeck_ui::RequestId,
    pub(crate) installed: Option<CoreInstallationView>,
    pub(crate) result: Result<(), String>,
}

/// Fetch the pinned archive, enforcing the declared size before it is buffered.
async fn fetch_archive(entry: &CoreDownloadEntry) -> Result<Vec<u8>, String> {
    let client = reqwest::Client::builder()
        .timeout(FETCH_TIMEOUT)
        .redirect(reqwest::redirect::Policy::limited(5))
        .user_agent(USER_AGENT)
        .build()
        .map_err(|error| format!("Failed to build HTTP client: {error}"))?;
    let response = client
        .get(entry.url)
        .send()
        .await
        .map_err(|error| format!("aria2 download failed: {error}"))?;
    if !response.status().is_success() {
        return Err(format!(
            "aria2 download failed with HTTP {}.",
            response.status()
        ));
    }
    if let Some(len) = response.content_length()
        && len != entry.size_bytes
    {
        return Err(format!(
            "aria2 download has an unexpected size ({len} bytes, expected {}).",
            entry.size_bytes
        ));
    }
    let bytes = response
        .bytes()
        .await
        .map_err(|error| format!("aria2 download failed: {error}"))?;
    if bytes.len() as u64 != entry.size_bytes {
        return Err(format!(
            "aria2 download has an unexpected size ({} bytes, expected {}).",
            bytes.len(),
            entry.size_bytes
        ));
    }
    Ok(bytes.to_vec())
}

#[must_use]
pub(crate) fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest as _, Sha256};
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// Verify the archive against its pinned digest before anything is written out.
pub(crate) fn verify_archive(entry: &CoreDownloadEntry, bytes: &[u8]) -> Result<(), String> {
    let actual = sha256_hex(bytes);
    if !actual.eq_ignore_ascii_case(entry.sha256) {
        return Err(format!(
            "aria2 download failed checksum verification (expected {}, got {actual}).",
            entry.sha256
        ));
    }
    Ok(())
}

/// Extract the pinned member into `destination_dir`, returning the file path.
///
/// The member is addressed by its exact pinned name, so a hostile archive cannot
/// steer the write anywhere — the destination filename is derived locally and the
/// archive's own path never reaches the filesystem (no zip-slip surface).
pub(crate) fn extract_member(
    entry: &CoreDownloadEntry,
    archive: &[u8],
    destination_dir: &Path,
) -> Result<PathBuf, String> {
    let mut zip = zip::ZipArchive::new(Cursor::new(archive))
        .map_err(|error| format!("aria2 archive could not be opened: {error}"))?;
    let mut member = zip.by_name(entry.archive_member).map_err(|_| {
        format!(
            "aria2 archive does not contain the expected entry {}.",
            entry.archive_member
        )
    })?;
    if !member.is_file() {
        return Err(format!(
            "aria2 archive entry {} is not a file.",
            entry.archive_member
        ));
    }
    if member.size() > MAX_EXTRACTED_BYTES {
        return Err("aria2 archive entry is unexpectedly large.".into());
    }

    let file_name = Path::new(entry.archive_member)
        .file_name()
        .ok_or_else(|| "aria2 archive entry has no file name.".to_owned())?;
    let destination = destination_dir.join(file_name);
    std::fs::create_dir_all(destination_dir)
        .map_err(|error| format!("Failed to prepare the download directory: {error}"))?;

    // Bound the copy independently of the header so a lying size cannot fill the
    // disk mid-extract.
    let mut bounded = member.by_ref().take(MAX_EXTRACTED_BYTES + 1);
    let mut buffer = Vec::new();
    bounded
        .read_to_end(&mut buffer)
        .map_err(|error| format!("aria2 archive could not be read: {error}"))?;
    if buffer.len() as u64 > MAX_EXTRACTED_BYTES {
        return Err("aria2 archive entry is unexpectedly large.".into());
    }

    let mut file = std::fs::File::create(&destination)
        .map_err(|error| format!("Failed to write the aria2 executable: {error}"))?;
    file.write_all(&buffer)
        .map_err(|error| format!("Failed to write the aria2 executable: {error}"))?;
    file.sync_all()
        .map_err(|error| format!("Failed to write the aria2 executable: {error}"))?;
    drop(file);

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        let mut permissions = std::fs::metadata(&destination)
            .map_err(|error| format!("Failed to read the aria2 executable: {error}"))?
            .permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&destination, permissions)
            .map_err(|error| format!("Failed to mark the aria2 executable executable: {error}"))?;
    }

    Ok(destination)
}

/// Download → verify → extract → register, as one user-initiated action.
pub(crate) async fn run_core_download(
    core_store: CoreStore,
    request: CoreDownloadRequest,
) -> CoreDownloadResult {
    let Some(entry) = catalog_entry_for_host() else {
        return CoreDownloadResult {
            request_id: request.request_id,
            installed: None,
            result: Err(
                "No verified aria2 download is published for this platform. Install aria2 with your package manager, then use Browse."
                    .into(),
            ),
        };
    };

    let archive = match fetch_archive(entry).await {
        Ok(archive) => archive,
        Err(error) => {
            return CoreDownloadResult {
                request_id: request.request_id,
                installed: None,
                result: Err(error),
            };
        }
    };

    // Hashing, unzipping, copying and `--version` probing are all blocking.
    let installed = tokio::task::spawn_blocking(move || {
        verify_archive(entry, &archive)?;
        let staging = tempfile::Builder::new()
            .prefix("ariadeck-core-")
            .tempdir()
            .map_err(|error| format!("Failed to create a staging directory: {error}"))?;
        let executable = extract_member(entry, &archive, staging.path())?;
        core_store
            .install_downloaded_executable(&executable)
            .map_err(|error| error.to_string())
    })
    .await
    .map_err(|error| format!("aria2 install task failed: {error}"))
    .and_then(|result| result);

    match installed {
        Ok(view) => CoreDownloadResult {
            request_id: request.request_id,
            installed: Some(view),
            result: Ok(()),
        },
        Err(error) => CoreDownloadResult {
            request_id: request.request_id,
            installed: None,
            result: Err(error),
        },
    }
}

pub(crate) fn spawn_core_download_bridge(
    runtime: tokio::runtime::Handle,
    core_store: CoreStore,
    mut requests: mpsc::UnboundedReceiver<CoreDownloadRequest>,
    results: mpsc::UnboundedSender<CoreDownloadResult>,
) {
    // Needs a reactor for the HTTP fetch and `spawn_blocking`, so it lives on the
    // Tokio runtime rather than the GPUI executor (same reason as tracker_list).
    runtime.spawn(async move {
        while let Some(request) = requests.recv().await {
            let result = run_core_download(core_store.clone(), request).await;
            if results.send(result).is_err() {
                break;
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn windows_entry() -> &'static CoreDownloadEntry {
        catalog_entry_for("windows-x86_64").expect("windows-x86_64 is pinned")
    }

    #[test]
    fn catalog_entries_are_pinned_to_https_with_a_full_sha256() {
        assert!(!CORE_DOWNLOAD_CATALOG.is_empty());
        for entry in CORE_DOWNLOAD_CATALOG {
            assert!(
                entry.url.starts_with("https://"),
                "{} must be fetched over TLS",
                entry.target
            );
            assert_eq!(
                entry.sha256.len(),
                64,
                "{} needs a full SHA-256",
                entry.target
            );
            assert!(
                entry
                    .sha256
                    .chars()
                    .all(|ch| ch.is_ascii_hexdigit() && !ch.is_ascii_uppercase()),
                "{} digest must be lowercase hex",
                entry.target
            );
            assert!(entry.size_bytes > 0, "{} needs a size", entry.target);
            assert!(
                entry.archive_member.ends_with("aria2c.exe")
                    || entry.archive_member.ends_with("aria2c"),
                "{} must point at an aria2c binary",
                entry.target
            );
        }
    }

    /// Platforms upstream does not build for must have no offer at all, rather
    /// than an entry that would download something unverified.
    #[test]
    fn platforms_without_published_binaries_have_no_offer() {
        assert!(catalog_entry_for("linux-x86_64").is_none());
        assert!(catalog_entry_for("macos-aarch64").is_none());
        assert!(catalog_entry_for("windows-x86_64").is_some());
    }

    #[test]
    fn verification_rejects_a_tampered_archive() {
        let entry = windows_entry();
        assert!(verify_archive(entry, b"not the real archive").is_err());
    }

    #[test]
    fn verification_accepts_the_pinned_digest() {
        // A synthetic entry lets this assert the comparison itself without
        // shipping 2.4 MB of fixture.
        let payload = b"pretend archive bytes";
        let entry = CoreDownloadEntry {
            target: "test-target",
            version: "0.0.0",
            url: "https://example.test/aria2.zip",
            size_bytes: payload.len() as u64,
            sha256: Box::leak(sha256_hex(payload).into_boxed_str()),
            archive_member: "aria2/aria2c",
            emulated: false,
        };
        assert!(verify_archive(&entry, payload).is_ok());
    }

    fn zip_with(member: &str, contents: &[u8]) -> Vec<u8> {
        let mut buffer = Vec::new();
        {
            let mut writer = zip::ZipWriter::new(Cursor::new(&mut buffer));
            writer
                .start_file(
                    member,
                    zip::write::SimpleFileOptions::default()
                        .compression_method(zip::CompressionMethod::Deflated),
                )
                .expect("start entry");
            writer.write_all(contents).expect("write entry");
            writer.finish().expect("finish archive");
        }
        buffer
    }

    #[test]
    fn extraction_reads_the_pinned_member_from_a_deflated_archive() {
        let member = "aria2-1.37.0-win-64bit-build1/aria2c.exe";
        let archive = zip_with(member, b"binary-contents");
        let entry = CoreDownloadEntry {
            target: "test-target",
            version: "1.37.0",
            url: "https://example.test/aria2.zip",
            size_bytes: archive.len() as u64,
            sha256: "00",
            archive_member: member,
            emulated: false,
        };
        let staging = tempfile::tempdir().expect("staging directory");
        let extracted = extract_member(&entry, &archive, staging.path()).expect("extract");

        assert_eq!(extracted, staging.path().join("aria2c.exe"));
        assert_eq!(
            std::fs::read(&extracted).expect("read extracted"),
            b"binary-contents"
        );
    }

    /// The member is addressed by its exact pinned name, so an archive that
    /// carries a traversal path simply does not match.
    #[test]
    fn extraction_fails_when_the_pinned_member_is_absent() {
        let archive = zip_with("../../evil.exe", b"payload");
        let entry = CoreDownloadEntry {
            target: "test-target",
            version: "1.37.0",
            url: "https://example.test/aria2.zip",
            size_bytes: archive.len() as u64,
            sha256: "00",
            archive_member: "aria2-1.37.0-win-64bit-build1/aria2c.exe",
            emulated: false,
        };
        let staging = tempfile::tempdir().expect("staging directory");
        let error = extract_member(&entry, &archive, staging.path()).expect_err("must not extract");
        assert!(error.contains("does not contain"), "{error}");
        assert!(
            !staging.path().join("evil.exe").exists(),
            "nothing may be written for a non-matching archive"
        );
    }

    /// Live check for the whole pinned path: fetch the real artifact, verify its
    /// digest, extract, and let `CoreStore` probe `--version` and register it.
    /// Ignored by default so CI and offline builds never reach the network.
    #[test]
    #[ignore = "downloads the pinned aria2 release over the network"]
    fn live_download_installs_the_pinned_core() {
        let Some(entry) = catalog_entry_for_host() else {
            eprintln!(
                "no pinned aria2 build for {} — nothing to check",
                host_target()
            );
            return;
        };
        let root = tempfile::tempdir().expect("temporary data directory");
        let core_store = CoreStore::new(root.path());
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("runtime");
        let result = runtime.block_on(run_core_download(
            core_store.clone(),
            CoreDownloadRequest {
                request_id: ariadeck_ui::RequestId::from_u64(1),
            },
        ));
        result.result.expect("verified download installs");
        let installed = result.installed.expect("installed core");
        assert_eq!(installed.version, entry.version);
        assert_eq!(installed.source, ariadeck_engine::CoreSource::Managed);
        assert!(installed.executable.is_file());

        let listed = core_store.list_installations().expect("list cores");
        assert_eq!(listed.len(), 1);
        assert!(listed[0].is_active, "the first core becomes active");
    }

    /// Even if a pinned member name contained directory components, the write
    /// target is derived from the file name alone.
    #[test]
    fn extraction_writes_only_into_the_staging_directory() {
        let member = "nested/dir/aria2c";
        let archive = zip_with(member, b"payload");
        let entry = CoreDownloadEntry {
            target: "test-target",
            version: "1.37.0",
            url: "https://example.test/aria2.zip",
            size_bytes: archive.len() as u64,
            sha256: "00",
            archive_member: member,
            emulated: false,
        };
        let staging = tempfile::tempdir().expect("staging directory");
        let extracted = extract_member(&entry, &archive, staging.path()).expect("extract");
        assert_eq!(extracted, staging.path().join("aria2c"));
        assert!(!staging.path().join("nested").exists());
    }
}
