//! Profile catalog and managed core registry for AppShell.

use super::*;

impl AppShell {
    pub fn profiles(&self) -> &ProfileCatalogView {
        &self.profiles
    }

    pub fn set_profiles(&mut self, profiles: ProfileCatalogView, cx: &mut Context<Self>) {
        self.profiles = profiles;
        cx.notify();
    }

    pub fn request_switch_profile(&mut self, profile_id: String, cx: &mut Context<Self>) {
        let profile_id = profile_id.trim().to_owned();
        if profile_id.is_empty() {
            self.show_notice("Select a profile to activate.", true, cx);
            return;
        }
        if profile_id == self.profiles.active_profile_id {
            self.show_notice("That profile is already active.", false, cx);
            return;
        }
        if !self
            .profiles
            .profiles
            .iter()
            .any(|profile| profile.profile_id == profile_id)
        {
            self.show_notice(
                "The selected profile is no longer in the catalog.",
                true,
                cx,
            );
            return;
        }
        let request_id = self.allocate_request_id();
        self.show_notice("Switching profile...", false, cx);
        cx.emit(AppShellEvent::SwitchProfileRequested(
            SwitchProfileRequestView {
                request_id,
                profile_id,
            },
        ));
        cx.notify();
    }

    pub fn set_switch_profile_result(
        &mut self,
        result: SwitchProfileResultView,
        cx: &mut Context<Self>,
    ) {
        match result.outcome {
            SwitchProfileOutcomeView::Success => {
                self.profiles = result.catalog;
                self.show_notice(
                    "Active profile updated. Restart AriaDeck to reconnect.",
                    false,
                    cx,
                );
            }
            SwitchProfileOutcomeView::Failure(error) => {
                self.show_notice(error.summary, true, cx);
            }
        }
        cx.notify();
    }

    pub fn request_save_profile_catalog(
        &mut self,
        catalog: ProfileCatalogView,
        cx: &mut Context<Self>,
    ) {
        if catalog.profiles.is_empty() {
            self.show_notice("At least one profile is required.", true, cx);
            return;
        }
        if !catalog
            .profiles
            .iter()
            .any(|profile| profile.profile_id == catalog.active_profile_id)
        {
            self.show_notice("The active profile must exist in the catalog.", true, cx);
            return;
        }
        let request_id = self.allocate_request_id();
        let secret_updates = self.settings_page.profile_secret_updates.clone();
        self.show_notice("Saving profiles...", false, cx);
        cx.emit(AppShellEvent::SaveProfileCatalogRequested(
            SaveProfileCatalogRequestView {
                request_id,
                catalog,
                secret_updates,
            },
        ));
        cx.notify();
    }

    pub fn set_save_profile_catalog_result(
        &mut self,
        result: SaveProfileCatalogResultView,
        cx: &mut Context<Self>,
    ) {
        match result.outcome {
            SaveProfileCatalogOutcomeView::Success => {
                self.profiles = result.catalog;
                self.settings_page.profile_secret_updates.clear();
                self.settings_page.clear_profile_rpc_secret = false;
                self.settings_inputs.profile_secret.update(cx, |input, cx| {
                    input.set_text(String::new(), cx);
                });
                self.show_notice("Profiles saved.", false, cx);
            }
            SaveProfileCatalogOutcomeView::Failure(error) => {
                self.show_notice(error.summary, true, cx);
            }
        }
        cx.notify();
    }

    pub(crate) fn add_draft_local_profile(&mut self, cx: &mut Context<Self>) {
        let id = format!("draft-local-{}", self.allocate_request_id().get());
        let download_dir = self.settings.download_directory.clone();
        let name = format!("Local {}", self.profiles.profiles.len() + 1);
        self.profiles.profiles.push(ProfileEntryView {
            profile_id: id.clone(),
            name,
            kind: ProfileKindView::LocalManaged,
            // Empty = use Settings → Engine active managed core at spawn.
            executable: String::new(),
            download_dir,
            endpoint: String::new(),
            has_secret: false,
        });
        self.open_profile_editor(id, cx);
        self.show_notice(
            "Local profile draft added (uses managed core). Edit fields, Apply, then Save profiles.",
            false,
            cx,
        );
    }

    pub(crate) fn add_draft_remote_profile(&mut self, cx: &mut Context<Self>) {
        let id = format!("draft-remote-{}", self.allocate_request_id().get());
        let download_dir = self.settings.download_directory.clone();
        let name = format!("Remote {}", self.profiles.profiles.len() + 1);
        self.profiles.profiles.push(ProfileEntryView {
            profile_id: id.clone(),
            name,
            kind: ProfileKindView::RemoteRpc,
            executable: String::new(),
            download_dir,
            endpoint: "wss://127.0.0.1:6800/jsonrpc".into(),
            has_secret: false,
        });
        self.open_profile_editor(id, cx);
        self.show_notice(
            "Remote profile draft added. Set the endpoint, Apply, then Save profiles.",
            false,
            cx,
        );
    }

    pub(crate) fn open_profile_editor(&mut self, profile_id: String, cx: &mut Context<Self>) {
        let Some(profile) = self
            .profiles
            .profiles
            .iter()
            .find(|profile| profile.profile_id == profile_id)
            .cloned()
        else {
            self.show_notice("That profile is no longer in the catalog.", true, cx);
            return;
        };
        self.settings_page.editing_profile_id = Some(profile.profile_id);
        self.settings_page.draft_profile_kind = profile.kind;
        self.settings_inputs.profile_name.update(cx, |input, cx| {
            input.set_text(profile.name, cx);
        });
        self.settings_inputs
            .profile_executable
            .update(cx, |input, cx| {
                input.set_text(profile.executable, cx);
            });
        self.settings_inputs
            .profile_endpoint
            .update(cx, |input, cx| {
                input.set_text(profile.endpoint, cx);
            });
        self.settings_inputs
            .profile_download
            .update(cx, |input, cx| {
                input.set_text(profile.download_dir, cx);
            });
        self.settings_inputs.profile_secret.update(cx, |input, cx| {
            input.set_text(String::new(), cx);
        });
        self.settings_page.clear_profile_rpc_secret = false;
        cx.notify();
    }

    pub(crate) fn close_profile_editor(&mut self, cx: &mut Context<Self>) {
        self.settings_page.editing_profile_id = None;
        self.settings_page.clear_profile_rpc_secret = false;
        self.settings_inputs.profile_secret.update(cx, |input, cx| {
            input.set_text(String::new(), cx);
        });
        cx.notify();
    }

    pub(crate) fn apply_profile_editor(&mut self, cx: &mut Context<Self>) {
        let Some(profile_id) = self.settings_page.editing_profile_id.clone() else {
            self.show_notice("No profile is open for editing.", true, cx);
            return;
        };
        let name = self
            .settings_inputs
            .profile_name
            .read(cx)
            .text()
            .trim()
            .to_owned();
        if name.is_empty() {
            self.show_notice("Profile name cannot be empty.", true, cx);
            return;
        }
        let kind = self.settings_page.draft_profile_kind;
        let executable = self
            .settings_inputs
            .profile_executable
            .read(cx)
            .text()
            .trim()
            .to_owned();
        let endpoint = self
            .settings_inputs
            .profile_endpoint
            .read(cx)
            .text()
            .trim()
            .to_owned();
        let download_dir = self
            .settings_inputs
            .profile_download
            .read(cx)
            .text()
            .trim()
            .to_owned();
        if kind == ProfileKindView::RemoteRpc && endpoint.is_empty() {
            self.show_notice("Remote profiles need a ws/wss endpoint.", true, cx);
            return;
        }
        let Some(profile) = self
            .profiles
            .profiles
            .iter_mut()
            .find(|profile| profile.profile_id == profile_id)
        else {
            self.show_notice("That profile is no longer in the catalog.", true, cx);
            self.settings_page.editing_profile_id = None;
            cx.notify();
            return;
        };
        profile.name = name;
        profile.kind = kind;
        profile.executable = if kind == ProfileKindView::LocalManaged {
            executable
        } else {
            String::new()
        };
        profile.endpoint = if kind == ProfileKindView::RemoteRpc {
            endpoint
        } else {
            String::new()
        };
        profile.download_dir = if download_dir.is_empty() {
            self.settings.download_directory.clone()
        } else {
            download_dir
        };
        if kind == ProfileKindView::RemoteRpc {
            let secret_text = self
                .settings_inputs
                .profile_secret
                .read(cx)
                .text()
                .trim()
                .to_owned();
            let update = if self.settings_page.clear_profile_rpc_secret {
                ProfileRpcSecretUpdateView::Clear
            } else if !secret_text.is_empty() {
                ProfileRpcSecretUpdateView::Set(SecretStringView::new(secret_text))
            } else {
                ProfileRpcSecretUpdateView::Unchanged
            };
            match &update {
                ProfileRpcSecretUpdateView::Unchanged => {}
                ProfileRpcSecretUpdateView::Clear => {
                    profile.has_secret = false;
                    self.settings_page
                        .profile_secret_updates
                        .insert(profile_id.clone(), update);
                }
                ProfileRpcSecretUpdateView::Set(_) => {
                    profile.has_secret = true;
                    self.settings_page
                        .profile_secret_updates
                        .insert(profile_id.clone(), update);
                }
            }
        } else {
            profile.has_secret = false;
            self.settings_page
                .profile_secret_updates
                .remove(&profile_id);
        }
        self.settings_page.editing_profile_id = None;
        self.settings_page.clear_profile_rpc_secret = false;
        self.settings_inputs.profile_secret.update(cx, |input, cx| {
            input.set_text(String::new(), cx);
        });
        self.show_notice(
            "Profile updated in the draft catalog. Click Save profiles to persist.",
            false,
            cx,
        );
        cx.notify();
    }

    pub(crate) fn request_remove_profile(&mut self, profile_id: String, cx: &mut Context<Self>) {
        if self.profiles.profiles.len() <= 1 {
            self.show_notice("At least one profile must remain.", true, cx);
            return;
        }
        let Some(profile) = self
            .profiles
            .profiles
            .iter()
            .find(|profile| profile.profile_id == profile_id)
        else {
            self.show_notice("That profile is no longer in the catalog.", true, cx);
            return;
        };
        self.settings_page.pending_profile_delete = Some(PendingProfileDelete {
            profile_id: profile.profile_id.clone(),
            name: profile.name.clone(),
        });
        cx.notify();
    }

    pub(crate) fn cancel_remove_profile(&mut self, cx: &mut Context<Self>) {
        self.settings_page.pending_profile_delete = None;
        cx.notify();
    }

    pub(crate) fn confirm_remove_profile(&mut self, cx: &mut Context<Self>) {
        let Some(pending) = self.settings_page.pending_profile_delete.take() else {
            return;
        };
        self.remove_profile(pending.profile_id, cx);
    }

    pub(crate) fn remove_profile(&mut self, profile_id: String, cx: &mut Context<Self>) {
        if self.profiles.profiles.len() <= 1 {
            self.show_notice("At least one profile must remain.", true, cx);
            return;
        }
        let Some(index) = self
            .profiles
            .profiles
            .iter()
            .position(|profile| profile.profile_id == profile_id)
        else {
            self.show_notice("That profile is no longer in the catalog.", true, cx);
            return;
        };
        let removed = self.profiles.profiles.remove(index);
        self.settings_page
            .profile_secret_updates
            .remove(&profile_id);
        if self.settings_page.editing_profile_id.as_deref() == Some(profile_id.as_str()) {
            self.settings_page.editing_profile_id = None;
        }
        if self.profiles.active_profile_id == profile_id {
            self.profiles.active_profile_id = self
                .profiles
                .profiles
                .first()
                .map(|profile| profile.profile_id.clone())
                .unwrap_or_default();
        }
        let catalog = self.profiles.clone();
        self.show_notice(
            format!("Removed “{}”. Saving catalog…", removed.name),
            false,
            cx,
        );
        self.request_save_profile_catalog(catalog, cx);
    }

    pub(crate) fn toggle_clear_profile_rpc_secret(&mut self, cx: &mut Context<Self>) {
        let clear = !self.settings_page.clear_profile_rpc_secret;
        if clear {
            self.settings_inputs.profile_secret.update(cx, |input, cx| {
                input.set_text(String::new(), cx);
            });
        }
        self.settings_page.clear_profile_rpc_secret = clear;
        cx.notify();
    }

    pub(crate) fn select_profile_editor_kind(
        &mut self,
        kind: ProfileKindView,
        cx: &mut Context<Self>,
    ) {
        self.settings_page.draft_profile_kind = kind;
        if kind == ProfileKindView::LocalManaged {
            self.settings_page.clear_profile_rpc_secret = false;
            self.settings_inputs.profile_secret.update(cx, |input, cx| {
                input.set_text(String::new(), cx);
            });
        }
        cx.notify();
    }

    pub fn set_cores(&mut self, cores: CoreRegistryView, cx: &mut Context<Self>) {
        self.cores = cores;
        cx.notify();
    }

    pub fn request_core_command(&mut self, command: CoreCommandView, cx: &mut Context<Self>) {
        let request_id = self.allocate_request_id();
        let notice = match &command {
            CoreCommandView::Import { .. } => "Importing aria2 core...",
            CoreCommandView::Link { .. } => "Linking aria2 core...",
            CoreCommandView::Verify { .. } => "Verifying aria2 core...",
            CoreCommandView::Activate { .. } => "Activating aria2 core...",
            CoreCommandView::Rollback => "Rolling back to last working core...",
            CoreCommandView::Remove { .. } => "Removing aria2 core...",
        };
        self.show_notice(notice, false, cx);
        cx.emit(AppShellEvent::CoreCommandRequested(
            CoreCommandRequestView {
                request_id,
                command,
            },
        ));
        cx.notify();
    }

    pub fn set_core_command_result(
        &mut self,
        result: CoreCommandResultView,
        cx: &mut Context<Self>,
    ) {
        match result.outcome {
            CoreCommandOutcomeView::Success => {
                self.cores = result.registry;
                let message = match result.command {
                    CoreCommandView::Import { .. } => {
                        "aria2 core imported. Activate it, then restart AriaDeck to use it."
                    }
                    CoreCommandView::Link { .. } => {
                        "aria2 core linked. Activate it, then restart AriaDeck to use it."
                    }
                    CoreCommandView::Verify { .. } => "aria2 core verified.",
                    CoreCommandView::Activate { .. } => {
                        "Active aria2 core updated. Restart AriaDeck to start that version."
                    }
                    CoreCommandView::Rollback => {
                        "Rolled back to the last working core. Restart AriaDeck to apply."
                    }
                    CoreCommandView::Remove { .. } => "aria2 core removed.",
                };
                self.show_notice(message, false, cx);
            }
            CoreCommandOutcomeView::Failure(error) => {
                self.show_notice(error.summary, true, cx);
            }
        }
        cx.notify();
    }

    pub(crate) fn request_import_core_from_input(&mut self, cx: &mut Context<Self>) {
        let path = self
            .settings_inputs
            .core_path
            .read(cx)
            .text()
            .trim()
            .to_owned();
        if path.is_empty() {
            self.show_notice("Enter a path to an aria2c executable first.", true, cx);
            return;
        }
        self.request_core_command(CoreCommandView::Import { path }, cx);
    }

    pub(crate) fn request_link_core_from_input(&mut self, cx: &mut Context<Self>) {
        let path = self
            .settings_inputs
            .core_path
            .read(cx)
            .text()
            .trim()
            .to_owned();
        if path.is_empty() {
            self.show_notice("Enter a path to an aria2c executable first.", true, cx);
            return;
        }
        self.request_core_command(CoreCommandView::Link { path }, cx);
    }
}
