use std::{fmt, str::FromStr};

use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error as _};
use thiserror::Error;
use uuid::Uuid;

macro_rules! uuid_id {
    ($name:ident, $description:literal) => {
        #[doc = $description]
        #[derive(
            Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize,
        )]
        #[serde(transparent)]
        pub struct $name(Uuid);

        impl $name {
            #[must_use]
            pub fn new() -> Self {
                Self(Uuid::new_v4())
            }

            #[must_use]
            pub const fn from_uuid(value: Uuid) -> Self {
                Self(value)
            }

            #[must_use]
            pub const fn as_uuid(self) -> Uuid {
                self.0
            }
        }

        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                self.0.fmt(formatter)
            }
        }

        impl FromStr for $name {
            type Err = uuid::Error;

            fn from_str(value: &str) -> Result<Self, Self::Err> {
                Uuid::parse_str(value).map(Self)
            }
        }
    };
}

uuid_id!(ProfileId, "Stable identifier for an engine profile.");
uuid_id!(
    EngineSessionId,
    "Identifier for one engine process or remote connection session."
);
uuid_id!(
    CoreInstallationId,
    "Identifier for a managed aria2 installation."
);
uuid_id!(
    CredentialId,
    "Opaque reference to a credential-store entry."
);

/// Monotonic generation used to reject responses from an obsolete session.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SessionGeneration(u64);

impl SessionGeneration {
    #[must_use]
    pub const fn initial() -> Self {
        Self(1)
    }

    #[must_use]
    pub const fn from_u64(value: u64) -> Self {
        Self(value)
    }

    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }

    #[must_use]
    pub const fn next(self) -> Self {
        Self(self.0.saturating_add(1))
    }
}

impl Default for SessionGeneration {
    fn default() -> Self {
        Self::initial()
    }
}

impl fmt::Display for SessionGeneration {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

/// aria2 task identifier represented without heap allocation.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Gid(u64);

impl Gid {
    pub const HEX_LENGTH: usize = 16;

    #[must_use]
    pub const fn from_u64(value: u64) -> Self {
        Self(value)
    }

    #[must_use]
    pub const fn as_u64(self) -> u64 {
        self.0
    }
}

impl fmt::Display for Gid {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{:016x}", self.0)
    }
}

impl FromStr for Gid {
    type Err = GidParseError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        if value.len() != Self::HEX_LENGTH {
            return Err(GidParseError::InvalidLength {
                actual: value.len(),
            });
        }
        if !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err(GidParseError::InvalidHex);
        }

        u64::from_str_radix(value, 16)
            .map(Self)
            .map_err(|_| GidParseError::InvalidHex)
    }
}

impl Serialize for Gid {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for Gid {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        String::deserialize(deserializer)?
            .parse()
            .map_err(D::Error::custom)
    }
}

#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum GidParseError {
    #[error("an aria2 GID must contain exactly 16 hexadecimal characters, got {actual}")]
    InvalidLength { actual: usize },
    #[error("an aria2 GID contains a non-hexadecimal character")]
    InvalidHex,
}

/// Stable identity for a task across simultaneous engine profiles.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
pub struct TaskIdentity {
    pub profile_id: ProfileId,
    pub gid: Gid,
}

impl TaskIdentity {
    #[must_use]
    pub const fn new(profile_id: ProfileId, gid: Gid) -> Self {
        Self { profile_id, gid }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gid_round_trips_as_fixed_lowercase_hex() {
        let parsed = match "00ABCDEF12345678".parse::<Gid>() {
            Ok(gid) => gid,
            Err(error) => panic!("valid GID rejected: {error}"),
        };

        assert_eq!(parsed.to_string(), "00abcdef12345678");
    }

    #[test]
    fn gid_rejects_wrong_length_and_non_hex_values() {
        assert_eq!(
            "abc".parse::<Gid>(),
            Err(GidParseError::InvalidLength { actual: 3 })
        );
        assert_eq!(
            "zzzzzzzzzzzzzzzz".parse::<Gid>(),
            Err(GidParseError::InvalidHex)
        );
    }
}
