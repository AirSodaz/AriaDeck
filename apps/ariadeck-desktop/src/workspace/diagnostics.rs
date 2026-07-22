use std::{fs, io::Write as _, path::Path};

use ariadeck_domain::{DiagnosticSnapshot, redact_endpoint_url};
use thiserror::Error;
use zip::{ZipWriter, result::ZipError, write::SimpleFileOptions};

const PRIVACY_NOTICE: &str = "AriaDeck diagnostic export\n\
\n\
This archive contains a small, redacted runtime snapshot for support.\n\
It does not include download URLs, task names, local paths, settings files,\n\
credentials, proxy passwords, RPC secrets, cookies, headers, or log files.\n";

#[derive(Debug, Error)]
pub(crate) enum DiagnosticExportError {
    #[error("diagnostic data could not be serialized: {0}")]
    Serialize(#[from] serde_json::Error),
    #[error("diagnostic archive could not be created: {0}")]
    Zip(#[from] ZipError),
    #[error("diagnostic archive could not be saved: {0}")]
    Io(#[from] std::io::Error),
}

pub(crate) fn export_diagnostic_zip(
    path: &Path,
    mut snapshot: DiagnosticSnapshot,
) -> Result<(), DiagnosticExportError> {
    snapshot.redacted_rpc_endpoint = snapshot
        .redacted_rpc_endpoint
        .as_deref()
        .map(redact_endpoint_url);
    let diagnostics = serde_json::to_vec_pretty(&snapshot)?;

    let mut archive = ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let options = SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Stored)
        .unix_permissions(0o600);
    archive.start_file("diagnostics.json", options)?;
    archive.write_all(&diagnostics)?;
    archive.start_file("README.txt", options)?;
    archive.write_all(PRIVACY_NOTICE.as_bytes())?;
    let bytes = archive.finish()?.into_inner();
    fs::write(path, bytes)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::io::Read as _;

    use super::*;

    #[test]
    fn diagnostic_zip_contains_only_redacted_snapshot_and_privacy_notice() {
        let root = tempfile::tempdir().expect("temporary directory");
        let path = root.path().join("diagnostics.zip");
        let snapshot = DiagnosticSnapshot {
            app_version: "0.1.0".into(),
            platform: "windows-x86_64".into(),
            engine_version: Some("1.37.0".into()),
            settings_schema_version: Some(9),
            connection_state: "Connected".into(),
            redacted_rpc_endpoint: Some(
                "wss://user:never-export@rpc.example:6800/jsonrpc?token=private".into(),
            ),
            profile_kind: Some("Remote RPC".into()),
            task_count: Some(3),
            capability_count: Some(9),
        };

        export_diagnostic_zip(&path, snapshot).expect("diagnostic export");

        let file = fs::File::open(path).expect("open export");
        let mut archive = zip::ZipArchive::new(file).expect("read export");
        assert_eq!(archive.len(), 2);
        let names = archive
            .file_names()
            .map(ToOwned::to_owned)
            .collect::<Vec<_>>();
        assert_eq!(names, ["diagnostics.json", "README.txt"]);

        let mut diagnostics = String::new();
        archive
            .by_name("diagnostics.json")
            .expect("diagnostics entry")
            .read_to_string(&mut diagnostics)
            .expect("diagnostics content");
        let document: serde_json::Value =
            serde_json::from_str(&diagnostics).expect("diagnostics json");
        assert_eq!(
            document["redacted_rpc_endpoint"],
            "wss://rpc.example:6800/jsonrpc"
        );
        assert!(!diagnostics.contains("never-export"));
        assert!(!diagnostics.contains("token=private"));
    }
}
