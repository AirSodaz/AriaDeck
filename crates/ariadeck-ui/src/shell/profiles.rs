//! Profile catalog and managed core registry coordination for `AppShell`.

use super::*;

impl AppShell {
    pub fn profiles(&self) -> &ProfileCatalogView {
        &self.profiles
    }

    pub fn set_profiles(&mut self, profiles: ProfileCatalogView, cx: &mut Context<Self>) {
        self.profiles = profiles;
        self.sync_settings_panel(None, cx);
        cx.notify();
    }

    pub fn request_switch_profile(&mut self, profile_id: String, cx: &mut Context<Self>) {
        let profile_id = profile_id.trim().to_owned();
        if profile_id.is_empty() {
            self.show_notice(self.t("notice-profile-select"), true, cx);
            return;
        }
        if profile_id == self.profiles.active_profile_id {
            self.show_notice(self.t("notice-profile-already-active"), false, cx);
            return;
        }
        if !self
            .profiles
            .profiles
            .iter()
            .any(|profile| profile.profile_id == profile_id)
        {
            self.show_notice(self.t("notice-profile-missing-catalog"), true, cx);
            return;
        }
        let request_id = self.allocate_request_id();
        self.show_notice(self.t("notice-profile-switching"), false, cx);
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
                self.sync_settings_panel(None, cx);
                let name = self
                    .profiles
                    .active()
                    .map(|profile| profile.name.clone())
                    .unwrap_or_else(|| result.profile_id.clone());
                self.show_notice(
                    self.t_args(
                        "notice-profile-activated",
                        &[("name", FluentValue::from(name.as_str()))],
                    ),
                    false,
                    cx,
                );
            }
            SwitchProfileOutcomeView::Failure(error) => {
                self.show_notice(self.te(&error), true, cx);
            }
        }
        cx.notify();
    }

    pub fn request_save_profile_catalog(
        &mut self,
        catalog: ProfileCatalogView,
        cx: &mut Context<Self>,
    ) {
        let secret_updates = self
            .settings_panel
            .as_ref()
            .map(|(panel, _)| panel.read(cx).settings_page.profile_secret_updates.clone())
            .unwrap_or_default();
        self.request_save_profile_catalog_with_secrets(catalog, secret_updates, cx);
    }

    pub(super) fn request_save_profile_catalog_with_secrets(
        &mut self,
        catalog: ProfileCatalogView,
        secret_updates: HashMap<String, ProfileRpcSecretUpdateView>,
        cx: &mut Context<Self>,
    ) {
        if catalog.profiles.is_empty() {
            self.show_notice(self.t("notice-profile-at-least-one"), true, cx);
            return;
        }
        if !catalog
            .profiles
            .iter()
            .any(|profile| profile.profile_id == catalog.active_profile_id)
        {
            self.show_notice(self.t("notice-profile-active-must-exist"), true, cx);
            return;
        }
        let request_id = self.allocate_request_id();
        self.show_notice(self.t("notice-profile-saving"), false, cx);
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
        window: Option<&mut Window>,
        cx: &mut Context<Self>,
    ) {
        match result.outcome {
            SaveProfileCatalogOutcomeView::Success => {
                self.profiles = result.catalog;
                if let Some(panel) = self.settings_panel_entity() {
                    let profiles = self.profiles.clone();
                    panel.update(cx, |panel, cx| {
                        panel.profiles = profiles;
                        panel.settings_page.profile_secret_updates.clear();
                        panel.settings_page.clear_profile_rpc_secret = false;
                        if let Some(window) = window {
                            panel
                                .settings_inputs
                                .profile_secret
                                .update(cx, |input, cx| {
                                    input.set_value(String::new(), window, cx);
                                });
                        }
                        cx.notify();
                    });
                }
                self.show_notice(self.t("notice-profile-saved"), false, cx);
            }
            SaveProfileCatalogOutcomeView::Failure(error) => {
                self.show_notice(self.te(&error), true, cx);
            }
        }
        cx.notify();
    }

    pub fn set_cores(&mut self, cores: CoreRegistryView, cx: &mut Context<Self>) {
        self.cores = cores;
        self.sync_settings_panel(None, cx);
        cx.notify();
    }

    pub fn request_core_command(&mut self, command: CoreCommandView, cx: &mut Context<Self>) {
        let request_id = self.allocate_request_id();
        let notice_key = match &command {
            CoreCommandView::Import { .. } => "notice-core-importing",
            CoreCommandView::Link { .. } => "notice-core-linking",
            CoreCommandView::Verify { .. } => "notice-core-verifying",
            CoreCommandView::Activate { .. } => "notice-core-activating",
            CoreCommandView::Rollback => "notice-core-rolling-back",
            CoreCommandView::Remove { .. } => "notice-core-removing",
        };
        self.show_notice(self.t(notice_key), false, cx);
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
                self.sync_settings_panel(None, cx);
                let message_key = match result.command {
                    CoreCommandView::Import { .. } => "notice-core-imported",
                    CoreCommandView::Link { .. } => "notice-core-linked",
                    CoreCommandView::Verify { .. } => "notice-core-verified",
                    CoreCommandView::Activate { .. } => "notice-core-activated",
                    CoreCommandView::Rollback => "notice-core-rolled-back",
                    CoreCommandView::Remove { .. } => "notice-core-removed",
                };
                self.show_notice(self.t(message_key), false, cx);
            }
            CoreCommandOutcomeView::Failure(error) => {
                self.show_notice(self.te(&error), true, cx);
            }
        }
        cx.notify();
    }
}
