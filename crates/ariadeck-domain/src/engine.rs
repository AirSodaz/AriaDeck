use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use url::Url;

use crate::{CoreInstallationId, CredentialId, EngineSessionId, ProfileId, SessionGeneration};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum EngineSource {
    Managed {
        core_id: CoreInstallationId,
    },
    External {
        executable: PathBuf,
    },
    Remote {
        endpoint: Url,
        credential_id: Option<CredentialId>,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProcessOwnership {
    SpawnedByApplication,
    ExistingLocalProcess,
    Remote,
}

/// Identity attached to every asynchronous engine response.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
pub struct EngineSession {
    pub profile_id: ProfileId,
    pub session_id: EngineSessionId,
    pub generation: SessionGeneration,
}

impl EngineSession {
    #[must_use]
    pub const fn new(
        profile_id: ProfileId,
        session_id: EngineSessionId,
        generation: SessionGeneration,
    ) -> Self {
        Self {
            profile_id,
            session_id,
            generation,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ConnectionFailure {
    pub code: String,
    pub summary: String,
    pub retryable: bool,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum ConnectionState {
    #[default]
    Disconnected,
    Connecting,
    Authenticating,
    Synchronizing,
    Connected,
    Reconnecting {
        attempt: u32,
    },
    Failed {
        reason: ConnectionFailure,
    },
}
