//! Streaming asynchronous local file checksum calculation (`MD5`, `SHA-1`, `SHA-256`, `SHA-512`).

use std::{fs::File, io::BufReader, path::PathBuf};

use ariadeck_domain::ChecksumAlgorithm;
use tokio::sync::watch;

const READ_BUFFER_BYTES: usize = 64 * 1024;
const PROGRESS_REPORT_STEP_BYTES: u64 = 2 * 1024 * 1024;

/// Stream-hash a locally validated file off the UI thread with progress and cancellation.
pub(crate) async fn compute_file_checksum<F>(
    path: PathBuf,
    algorithm: ChecksumAlgorithm,
    cancel: watch::Receiver<bool>,
    mut on_progress: F,
) -> Result<String, String>
where
    F: FnMut(u64, u64) + Send + 'static,
{
    tokio::task::spawn_blocking(move || {
        compute_file_checksum_blocking(&path, algorithm, &cancel, &mut on_progress)
    })
    .await
    .map_err(|error| format!("Checksum worker failed: {error}"))?
}

fn compute_file_checksum_blocking(
    path: &std::path::Path,
    algorithm: ChecksumAlgorithm,
    cancel: &watch::Receiver<bool>,
    on_progress: &mut impl FnMut(u64, u64),
) -> Result<String, String> {
    let file =
        File::open(path).map_err(|error| format!("Could not open {}: {error}", path.display()))?;
    let total_bytes = file
        .metadata()
        .map_err(|error| format!("Could not inspect {}: {error}", path.display()))?
        .len();
    let reader = BufReader::with_capacity(READ_BUFFER_BYTES, file);
    match algorithm {
        ChecksumAlgorithm::Md5 => hash_reader::<md5::Md5>(reader, total_bytes, cancel, on_progress),
        ChecksumAlgorithm::Sha1 => {
            hash_reader::<sha1::Sha1>(reader, total_bytes, cancel, on_progress)
        }
        ChecksumAlgorithm::Sha256 => {
            hash_reader::<sha2::Sha256>(reader, total_bytes, cancel, on_progress)
        }
        ChecksumAlgorithm::Sha512 => {
            hash_reader::<sha2::Sha512>(reader, total_bytes, cancel, on_progress)
        }
    }
}

fn hash_reader<D: sha2::Digest>(
    mut reader: impl std::io::Read,
    total_bytes: u64,
    cancel: &watch::Receiver<bool>,
    on_progress: &mut impl FnMut(u64, u64),
) -> Result<String, String> {
    let mut hasher = D::new();
    let mut buf = vec![0u8; READ_BUFFER_BYTES];
    let mut processed = 0u64;
    let mut last_reported = 0u64;
    on_progress(0, total_bytes);

    loop {
        if *cancel.borrow() {
            return Err("Checksum calculation was cancelled.".into());
        }
        let read = reader
            .read(&mut buf)
            .map_err(|error| format!("Failed reading file for checksum: {error}"))?;
        if read == 0 {
            break;
        }
        hasher.update(&buf[..read]);
        processed = processed.saturating_add(read as u64);
        if processed.saturating_sub(last_reported) >= PROGRESS_REPORT_STEP_BYTES {
            last_reported = processed;
            on_progress(processed, total_bytes);
        }
    }

    on_progress(processed, total_bytes);
    let digest = hasher.finalize();
    Ok(digest.iter().map(|byte| format!("{byte:02x}")).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write as _;

    #[tokio::test]
    async fn computes_md5_sha1_sha256_and_sha512_for_known_payload() {
        let mut temp = tempfile::NamedTempFile::new().expect("temp file");
        temp.write_all(b"abc").expect("write abc");
        let path = temp.path().to_path_buf();

        let (_tx, cancel) = watch::channel(false);
        let cases = [
            (ChecksumAlgorithm::Md5, "900150983cd24fb0d6963f7d28e17f72"),
            (
                ChecksumAlgorithm::Sha1,
                "a9993e364706816aba3e25717850c26c9cd0d89d",
            ),
            (
                ChecksumAlgorithm::Sha256,
                "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
            ),
            (
                ChecksumAlgorithm::Sha512,
                "ddaf35a193617abacc417349ae20413112e6fa4e89a97ea20a9eeee64b55d39a2192992a274fc1a836ba3c23a3feebbd454d4423643ce80e2a9ac94fa54ca49f",
            ),
        ];
        for (alg, expected) in cases {
            let actual = compute_file_checksum(path.clone(), alg, cancel.clone(), |_, _| {})
                .await
                .expect("hash succeeds");
            assert_eq!(actual, expected, "mismatch for {alg:?}");
        }
    }

    #[tokio::test]
    async fn respects_cancellation_signal() {
        let mut temp = tempfile::NamedTempFile::new().expect("temp file");
        temp.write_all(b"hello").expect("write");
        let (tx, cancel) = watch::channel(true);
        let _keep_tx = tx;
        let err = compute_file_checksum(
            temp.path().to_path_buf(),
            ChecksumAlgorithm::Sha256,
            cancel,
            |_, _| {},
        )
        .await
        .expect_err("cancelled hash must fail");
        assert!(err.contains("cancelled"));
    }
}
