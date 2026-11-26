//! Supported local file checksum algorithms and expected-digest parsing.

/// Hash algorithms supported for post-download local file verification.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub enum ChecksumAlgorithm {
    Md5,
    Sha1,
    #[default]
    Sha256,
    Sha512,
}

impl ChecksumAlgorithm {
    pub const ALL: [Self; 4] = [Self::Md5, Self::Sha1, Self::Sha256, Self::Sha512];

    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Md5 => "MD5",
            Self::Sha1 => "SHA-1",
            Self::Sha256 => "SHA-256",
            Self::Sha512 => "SHA-512",
        }
    }

    #[must_use]
    pub const fn aria2_prefix(self) -> &'static str {
        match self {
            Self::Md5 => "md5",
            Self::Sha1 => "sha-1",
            Self::Sha256 => "sha-256",
            Self::Sha512 => "sha-512",
        }
    }

    #[must_use]
    pub const fn hex_len(self) -> usize {
        match self {
            Self::Md5 => 32,
            Self::Sha1 => 40,
            Self::Sha256 => 64,
            Self::Sha512 => 128,
        }
    }

    #[must_use]
    pub fn from_aria2_prefix(prefix: &str) -> Option<Self> {
        match prefix.trim().to_ascii_lowercase().as_str() {
            "md5" => Some(Self::Md5),
            "sha-1" | "sha1" => Some(Self::Sha1),
            "sha-256" | "sha256" => Some(Self::Sha256),
            "sha-512" | "sha512" => Some(Self::Sha512),
            _ => None,
        }
    }

    #[must_use]
    pub const fn from_hex_len(len: usize) -> Option<Self> {
        match len {
            32 => Some(Self::Md5),
            40 => Some(Self::Sha1),
            64 => Some(Self::Sha256),
            128 => Some(Self::Sha512),
            _ => None,
        }
    }
}

/// Normalized expected checksum parsed from raw hex or aria2 `type=digest` form.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ParsedExpectedChecksum {
    pub algorithm: Option<ChecksumAlgorithm>,
    pub hex_digest: String,
}

/// Result of comparing a locally computed digest with user-supplied expected input.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ChecksumComparison {
    Empty,
    InvalidExpected,
    AlgorithmMismatch {
        expected: ChecksumAlgorithm,
        actual: ChecksumAlgorithm,
    },
    Match,
    Mismatch,
}

/// Parse either `type=digest` (e.g. `sha-256=...`) or bare hex digest text.
pub fn parse_expected_checksum(raw: &str) -> Result<ParsedExpectedChecksum, &'static str> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err("Expected checksum is empty.");
    }
    let (explicit_alg, digest_str) = if let Some((prefix, digest)) = trimmed.split_once('=') {
        let alg = ChecksumAlgorithm::from_aria2_prefix(prefix)
            .ok_or("Unsupported checksum algorithm prefix.")?;
        (Some(alg), digest.trim())
    } else if let Some((prefix, digest)) = trimmed.split_once(':') {
        let alg = ChecksumAlgorithm::from_aria2_prefix(prefix)
            .ok_or("Unsupported checksum algorithm prefix.")?;
        (Some(alg), digest.trim())
    } else {
        (None, trimmed)
    };

    if digest_str.is_empty()
        || !digest_str.bytes().all(|byte| byte.is_ascii_hexdigit())
        || digest_str.len() % 2 == 1
    {
        return Err("Checksum must be an even-length hexadecimal digest.");
    }
    if let Some(alg) = explicit_alg
        && digest_str.len() != alg.hex_len()
    {
        return Err("Checksum length does not match the specified algorithm.");
    }
    let inferred = explicit_alg.or_else(|| ChecksumAlgorithm::from_hex_len(digest_str.len()));
    Ok(ParsedExpectedChecksum {
        algorithm: inferred,
        hex_digest: digest_str.to_ascii_lowercase(),
    })
}

/// Compare a computed digest against user-supplied expected checksum text.
#[must_use]
pub fn compare_checksum(
    computed_hex: &str,
    expected_raw: &str,
    actual_algorithm: ChecksumAlgorithm,
) -> ChecksumComparison {
    let trimmed = expected_raw.trim();
    if trimmed.is_empty() {
        return ChecksumComparison::Empty;
    }
    let Ok(parsed) = parse_expected_checksum(trimmed) else {
        return ChecksumComparison::InvalidExpected;
    };
    let Some(expected_alg) = parsed.algorithm else {
        return ChecksumComparison::InvalidExpected;
    };
    if expected_alg != actual_algorithm {
        return ChecksumComparison::AlgorithmMismatch {
            expected: expected_alg,
            actual: actual_algorithm,
        };
    }
    if computed_hex.trim().eq_ignore_ascii_case(&parsed.hex_digest) {
        ChecksumComparison::Match
    } else {
        ChecksumComparison::Mismatch
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_prefixed_and_bare_digests_case_insensitively() {
        let sha = "A".repeat(64);
        let parsed = parse_expected_checksum(&format!("SHA-256={sha}")).expect("valid");
        assert_eq!(parsed.algorithm, Some(ChecksumAlgorithm::Sha256));
        assert_eq!(
            compare_checksum(
                &"a".repeat(64),
                &format!("sha-256:{sha}"),
                ChecksumAlgorithm::Sha256
            ),
            ChecksumComparison::Match
        );
        assert_eq!(
            compare_checksum(&"b".repeat(64), &sha, ChecksumAlgorithm::Sha256),
            ChecksumComparison::Mismatch
        );
        assert_eq!(
            compare_checksum(&"a".repeat(32), &sha, ChecksumAlgorithm::Md5),
            ChecksumComparison::AlgorithmMismatch {
                expected: ChecksumAlgorithm::Sha256,
                actual: ChecksumAlgorithm::Md5,
            }
        );
        assert_eq!(
            compare_checksum(&"a".repeat(64), "deadbeef", ChecksumAlgorithm::Sha256),
            ChecksumComparison::InvalidExpected
        );
    }
}
