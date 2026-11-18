use super::{commands::OperationErrorView, settings::ProfileRpcSecretUpdateView, task::RequestId};

/// Local managed vs remote RPC profile (PROFILE-001).
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub enum ProfileKindView {
    #[default]
    LocalManaged,
    RemoteRpc,
}

impl ProfileKindView {
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::LocalManaged => "Local managed",
            Self::RemoteRpc => "Remote RPC",
        }
    }

    #[must_use]
    pub const fn all() -> [Self; 2] {
        [Self::LocalManaged, Self::RemoteRpc]
    }
}

/// One profile catalog entry for settings/switch UI.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProfileEntryView {
    pub profile_id: String,
    pub name: String,
    pub kind: ProfileKindView,
    pub executable: String,
    pub download_dir: String,
    pub endpoint: String,
    pub has_secret: bool,
}

impl Default for ProfileEntryView {
    fn default() -> Self {
        Self {
            profile_id: String::new(),
            name: "Local aria2".into(),
            kind: ProfileKindView::LocalManaged,
            executable: String::new(),
            download_dir: String::new(),
            endpoint: String::new(),
            has_secret: false,
        }
    }
}

/// Multi-profile catalog presented to the settings page.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ProfileCatalogView {
    pub active_profile_id: String,
    pub profiles: Vec<ProfileEntryView>,
}

impl ProfileCatalogView {
    #[must_use]
    pub fn active(&self) -> Option<&ProfileEntryView> {
        self.profiles
            .iter()
            .find(|profile| profile.profile_id == self.active_profile_id)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SwitchProfileRequestView {
    pub request_id: RequestId,
    pub profile_id: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SwitchProfileOutcomeView {
    Success,
    Failure(OperationErrorView),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SwitchProfileResultView {
    pub request_id: RequestId,
    pub profile_id: String,
    pub catalog: ProfileCatalogView,
    pub outcome: SwitchProfileOutcomeView,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SaveProfileCatalogRequestView {
    pub request_id: RequestId,
    pub catalog: ProfileCatalogView,
    /// Per-profile secret mutations (keyed by profile_id string from the draft catalog).
    pub secret_updates: std::collections::HashMap<String, ProfileRpcSecretUpdateView>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SaveProfileCatalogOutcomeView {
    Success,
    Failure(OperationErrorView),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SaveProfileCatalogResultView {
    pub request_id: RequestId,
    pub catalog: ProfileCatalogView,
    pub outcome: SaveProfileCatalogOutcomeView,
}
