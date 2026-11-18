//! Settings `profiles` section logic and rendering for `SettingsPanelView`.

use super::*;

impl SettingsPanelView {
    pub(crate) fn add_draft_local_profile(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let id = format!("draft-local-{}", self.allocate_draft_profile_seq());
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
        cx.emit(SettingsPanelEvent::ProfileCatalogChanged(
            self.profiles.clone(),
        ));
        self.open_profile_editor(id, window, cx);
        cx.emit(SettingsPanelEvent::Notice {
            message: self.t("notice-profile-draft-local-added"),
            is_error: false,
        });
    }

    pub(crate) fn add_draft_remote_profile(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let id = format!("draft-remote-{}", self.allocate_draft_profile_seq());
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
        cx.emit(SettingsPanelEvent::ProfileCatalogChanged(
            self.profiles.clone(),
        ));
        self.open_profile_editor(id, window, cx);
        cx.emit(SettingsPanelEvent::Notice {
            message: self.t("notice-profile-draft-remote-added"),
            is_error: false,
        });
    }

    pub(crate) fn open_profile_editor(
        &mut self,
        profile_id: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(profile) = self
            .profiles
            .profiles
            .iter()
            .find(|profile| profile.profile_id == profile_id)
            .cloned()
        else {
            cx.emit(SettingsPanelEvent::Notice {
                message: self.t("notice-profile-gone"),
                is_error: true,
            });
            return;
        };
        self.settings_page.editing_profile_id = Some(profile.profile_id);
        self.settings_page.draft_profile_kind = profile.kind;
        self.settings_inputs.profile_name.update(cx, |input, cx| {
            input.set_value(profile.name, window, cx);
        });
        self.settings_inputs
            .profile_executable
            .update(cx, |input, cx| {
                input.set_value(profile.executable, window, cx);
            });
        self.settings_inputs
            .profile_endpoint
            .update(cx, |input, cx| {
                input.set_value(profile.endpoint, window, cx);
            });
        self.settings_inputs.profile_secret.update(cx, |input, cx| {
            input.set_value(String::new(), window, cx);
        });
        self.settings_page.clear_profile_rpc_secret = false;
        cx.notify();
    }

    pub(crate) fn close_profile_editor(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.settings_page.editing_profile_id = None;
        self.settings_page.clear_profile_rpc_secret = false;
        self.settings_inputs.profile_secret.update(cx, |input, cx| {
            input.set_value(String::new(), window, cx);
        });
        cx.notify();
    }

    pub(crate) fn apply_profile_editor(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(profile_id) = self.settings_page.editing_profile_id.clone() else {
            cx.emit(SettingsPanelEvent::Notice {
                message: self.t("notice-profile-none-editing"),
                is_error: true,
            });
            return;
        };
        let name = self
            .settings_inputs
            .profile_name
            .read(cx)
            .value()
            .trim()
            .to_owned();
        if name.is_empty() {
            cx.emit(SettingsPanelEvent::Notice {
                message: self.t("notice-profile-name-empty"),
                is_error: true,
            });
            return;
        }
        let kind = self.settings_page.draft_profile_kind;
        let executable = self
            .settings_inputs
            .profile_executable
            .read(cx)
            .value()
            .trim()
            .to_owned();
        let endpoint = self
            .settings_inputs
            .profile_endpoint
            .read(cx)
            .value()
            .trim()
            .to_owned();
        if kind == ProfileKindView::RemoteRpc && endpoint.is_empty() {
            cx.emit(SettingsPanelEvent::Notice {
                message: self.t("notice-profile-remote-endpoint"),
                is_error: true,
            });
            return;
        }
        let Some(profile) = self
            .profiles
            .profiles
            .iter_mut()
            .find(|profile| profile.profile_id == profile_id)
        else {
            cx.emit(SettingsPanelEvent::Notice {
                message: self.t("notice-profile-gone"),
                is_error: true,
            });
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
        profile.download_dir = self.settings.download_directory.clone();
        if kind == ProfileKindView::RemoteRpc {
            let secret_text = self
                .settings_inputs
                .profile_secret
                .read(cx)
                .value()
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
            input.set_value(String::new(), window, cx);
        });
        cx.emit(SettingsPanelEvent::ProfileCatalogChanged(
            self.profiles.clone(),
        ));
        cx.emit(SettingsPanelEvent::Notice {
            message: self.t("notice-profile-draft-updated"),
            is_error: false,
        });
        cx.notify();
    }

    pub(crate) fn request_remove_profile(&mut self, profile_id: String, cx: &mut Context<Self>) {
        if self.profiles.profiles.len() <= 1 {
            cx.emit(SettingsPanelEvent::Notice {
                message: self.t("notice-profile-at-least-remain"),
                is_error: true,
            });
            return;
        }
        let Some(profile) = self
            .profiles
            .profiles
            .iter()
            .find(|profile| profile.profile_id == profile_id)
        else {
            cx.emit(SettingsPanelEvent::Notice {
                message: self.t("notice-profile-gone"),
                is_error: true,
            });
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
            cx.emit(SettingsPanelEvent::Notice {
                message: self.t("notice-profile-at-least-remain"),
                is_error: true,
            });
            return;
        }
        let Some(index) = self
            .profiles
            .profiles
            .iter()
            .position(|profile| profile.profile_id == profile_id)
        else {
            cx.emit(SettingsPanelEvent::Notice {
                message: self.t("notice-profile-gone"),
                is_error: true,
            });
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
        let secret_updates = self.settings_page.profile_secret_updates.clone();
        cx.emit(SettingsPanelEvent::ProfileCatalogChanged(catalog.clone()));
        cx.emit(SettingsPanelEvent::Notice {
            message: self.t_args(
                "notice-profile-removed-saving",
                &[("name", FluentValue::from(removed.name.as_str()))],
            ),
            is_error: false,
        });
        cx.emit(SettingsPanelEvent::SaveProfileCatalog {
            catalog,
            secret_updates,
        });
        cx.notify();
    }

    pub(crate) fn toggle_clear_profile_rpc_secret(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let clear = !self.settings_page.clear_profile_rpc_secret;
        if clear {
            self.settings_inputs.profile_secret.update(cx, |input, cx| {
                input.set_value(String::new(), window, cx);
            });
        }
        self.settings_page.clear_profile_rpc_secret = clear;
        cx.notify();
    }

    pub(crate) fn select_profile_editor_kind(
        &mut self,
        kind: ProfileKindView,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.settings_page.draft_profile_kind = kind;
        if kind == ProfileKindView::LocalManaged {
            self.settings_page.clear_profile_rpc_secret = false;
            self.settings_inputs.profile_secret.update(cx, |input, cx| {
                input.set_value(String::new(), window, cx);
            });
        }
        cx.notify();
    }

    pub(super) fn render_settings_profiles(&mut self, cx: &mut Context<Self>) -> Div {
        let colors = self.theme.colors;
        let profiles = self.profiles.clone();
        let active_id = profiles.active_profile_id.clone();
        let profiles_count = profiles.profiles.len();
        settings_section_owned(self.t("settings-profiles"), colors)
            .child(
                div()
                    .mt_3()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(
                        div()
                            .text_sm()
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(colors.text_primary)
                            .child(self.t("settings-profile-env-scope")),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(colors.text_muted)
                            .child(self.t("settings-profile-env-scope-desc")),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(colors.text_muted)
                            .child(self.t("settings-profile-env-local-note")),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(colors.text_muted)
                            .child(self.t("settings-profile-env-remote-note")),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(colors.text_muted)
                            .child(self.t("settings-profile-env-switch-hint")),
                    ),
            )
            .child(div().mt_3().flex().flex_col().gap_2().children(
                profiles.profiles.into_iter().map(|profile| {
                    let is_active = profile.profile_id == active_id;
                    let profile_id = profile.profile_id.clone();
                    let switch_id = profile_id.clone();
                    let edit_id = profile_id.clone();
                    let remove_id = profile_id.clone();
                    let can_remove = profiles_count > 1;
                    let summary = match profile.kind {
                        ProfileKindView::LocalManaged => {
                            if profile.executable.is_empty() {
                                self.t("settings-profile-summary-local-managed")
                            } else {
                                self.t_args(
                                    "settings-profile-summary-local-pinned",
                                    &[("path", FluentValue::from(profile.executable.as_str()))],
                                )
                            }
                        }
                        ProfileKindView::RemoteRpc => {
                            let endpoint = if profile.endpoint.is_empty() {
                                self.t("settings-profile-no-endpoint")
                            } else {
                                profile.endpoint.clone()
                            };
                            if profile.has_secret {
                                self.t_args(
                                    "settings-profile-summary-remote-secret",
                                    &[("endpoint", FluentValue::from(endpoint.as_str()))],
                                )
                            } else {
                                self.t_args(
                                    "settings-profile-summary-remote",
                                    &[("endpoint", FluentValue::from(endpoint.as_str()))],
                                )
                            }
                        }
                    };
                    div()
                        .id(SharedString::from(format!(
                            "profile-row-{}",
                            profile.profile_id
                        )))
                        .flex()
                        .items_center()
                        .gap_2()
                        .px_3()
                        .py_2()
                        .rounded_md()
                        .border_1()
                        .border_color(if is_active {
                            colors.accent
                        } else {
                            colors.border
                        })
                        .bg(if is_active {
                            with_alpha(colors.accent, 0.08)
                        } else {
                            colors.surface
                        })
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .flex()
                                .flex_col()
                                .gap_0p5()
                                .child(
                                    div()
                                        .text_sm()
                                        .font_weight(FontWeight::MEDIUM)
                                        .text_color(colors.text_primary)
                                        .child(profile.name.clone()),
                                )
                                .child(
                                    div().text_xs().text_color(colors.text_muted).child(summary),
                                ),
                        )
                        .child({
                            let mut button = KitButton::new(SharedString::from(format!(
                                "activate-profile-{}",
                                profile_id
                            )))
                            .label(if is_active {
                                self.t("settings-item-active")
                            } else {
                                self.t("settings-item-activate")
                            })
                            .accessibility_label(if is_active {
                                self.t_args(
                                    "settings-profile-active-aria",
                                    &[("name", FluentValue::from(profile.name.as_str()))],
                                )
                            } else {
                                self.t_args(
                                    "settings-profile-activate-aria",
                                    &[("name", FluentValue::from(profile.name.as_str()))],
                                )
                            })
                            .disabled(is_active)
                            .on_click(cx.listener(
                                move |_this, _, _, cx| {
                                    cx.emit(SettingsPanelEvent::SwitchProfile(switch_id.clone()));
                                },
                            ));
                            if !is_active {
                                button = button.primary();
                            }
                            button
                        })
                        .child(
                            KitButton::new(SharedString::from(format!("edit-profile-{}", edit_id)))
                                .label(self.t("settings-item-edit"))
                                .accessibility_label(self.t_args(
                                    "settings-profile-edit-aria",
                                    &[("name", FluentValue::from(profile.name.as_str()))],
                                ))
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    this.open_profile_editor(edit_id.clone(), window, cx);
                                })),
                        )
                        .child(
                            KitButton::new(SharedString::from(format!(
                                "remove-profile-{}",
                                remove_id
                            )))
                            .label(self.t("button-delete"))
                            .accessibility_label(self.t_args(
                                "settings-profile-delete-aria",
                                &[("name", FluentValue::from(profile.name.as_str()))],
                            ))
                            .disabled(!can_remove)
                            .on_click(cx.listener(
                                move |this, _, _, cx| {
                                    this.request_remove_profile(remove_id.clone(), cx);
                                },
                            )),
                        )
                }),
            ))
            .when_some(
                self.settings_page.editing_profile_id.clone(),
                |section, editing_id| {
                    let kind = self.settings_page.draft_profile_kind;
                    let is_local = kind == ProfileKindView::LocalManaged;
                    let kind_panel = cx.entity().downgrade();
                    let kind_control = SegmentedControl::new(
                        "settings-profile-kind",
                        [
                            Segment::new(self.t("settings-profile-kind-local")),
                            Segment::new(self.t("settings-profile-kind-remote")),
                        ],
                        usize::from(!is_local),
                        self.theme,
                    )
                    .aria_label(self.t("settings-profile-kind-aria"))
                    .on_select(move |index, window, cx| {
                        let kind = if index == 0 {
                            ProfileKindView::LocalManaged
                        } else {
                            ProfileKindView::RemoteRpc
                        };
                        kind_panel
                            .update(cx, |panel, cx| {
                                panel.select_profile_editor_kind(kind, window, cx);
                            })
                            .ok();
                    });
                    section.child(
                        div()
                            .mt_3()
                            .flex()
                            .flex_col()
                            .gap_3()
                            .px_3()
                            .py_3()
                            .rounded_md()
                            .border_1()
                            .border_color(colors.border)
                            .bg(colors.surface)
                            .child(
                                div()
                                    .text_sm()
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(colors.text_primary)
                                    .child(self.t_args(
                                        "settings-profile-editor-title",
                                        &[("id", FluentValue::from(editing_id.as_str()))],
                                    )),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(colors.text_muted)
                                    .child(self.t("settings-profile-editor-hint")),
                            )
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .gap_1()
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(colors.text_muted)
                                            .child(self.t("ui-profile-name")),
                                    )
                                    .child(Input::new(&self.settings_inputs.profile_name)),
                            )
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .gap_1()
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(colors.text_muted)
                                            .child(self.t("ui-profile-kind")),
                                    )
                                    .child(kind_control),
                            )
                            .child(if is_local {
                                div()
                                    .flex()
                                    .flex_col()
                                    .gap_1()
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(colors.text_muted)
                                            .child(self.t("settings-profile-executable-label")),
                                    )
                                    .child(Self::settings_path_field_row(
                                        &self.settings_inputs.profile_executable,
                                        "browse-profile-executable",
                                        self.t("button-browse"),
                                        self.t("settings-profile-executable-browse-aria"),
                                        PathPickTarget::ProfileExecutable,
                                        cx,
                                    ))
                                    .into_any_element()
                            } else {
                                let has_secret = self
                                    .profiles
                                    .profiles
                                    .iter()
                                    .find(|profile| profile.profile_id == editing_id)
                                    .is_some_and(|profile| profile.has_secret);
                                let secret_cleared = self.settings_page.clear_profile_rpc_secret;
                                div()
                                    .flex()
                                    .flex_col()
                                    .gap_2()
                                    .child(
                                        div()
                                            .flex()
                                            .flex_col()
                                            .gap_1()
                                            .child(
                                                div()
                                                    .text_xs()
                                                    .text_color(colors.text_muted)
                                                    .child(self.t("ui-profile-endpoint")),
                                            )
                                            .child(Input::new(
                                                &self.settings_inputs.profile_endpoint,
                                            )),
                                    )
                                    .child(
                                        div()
                                            .flex()
                                            .flex_col()
                                            .gap_1()
                                            .child(
                                                div()
                                                    .text_xs()
                                                    .text_color(colors.text_muted)
                                                    .child(if secret_cleared {
                                                        self.t("settings-profile-secret-clear")
                                                    } else if has_secret {
                                                        self.t("settings-profile-secret-saved")
                                                    } else {
                                                        self.t("settings-profile-secret-optional")
                                                    }),
                                            )
                                            .child(Input::new(
                                                &self.settings_inputs.profile_secret,
                                            )),
                                    )
                                    .child(
                                        KitButton::new("toggle-clear-profile-secret")
                                            .label(if secret_cleared {
                                                self.t("settings-profile-secret-keep")
                                            } else if has_secret {
                                                self.t("settings-profile-secret-remove")
                                            } else {
                                                self.t("settings-profile-secret-none")
                                            })
                                            .accessibility_label(
                                                self.t("settings-profile-secret-toggle-aria"),
                                            )
                                            .disabled(!has_secret && !secret_cleared)
                                            .on_click(cx.listener(|this, _, window, cx| {
                                                this.toggle_clear_profile_rpc_secret(window, cx);
                                            })),
                                    )
                                    .into_any_element()
                            })
                            .child(
                                div()
                                    .flex()
                                    .flex_wrap()
                                    .gap_2()
                                    .child(
                                        KitButton::new("apply-profile-editor")
                                            .primary()
                                            .label(self.t("settings-profile-apply"))
                                            .accessibility_label(
                                                self.t("settings-profile-apply-aria"),
                                            )
                                            .on_click(cx.listener(|this, _, window, cx| {
                                                this.apply_profile_editor(window, cx);
                                            })),
                                    )
                                    .child(
                                        KitButton::new("cancel-profile-editor")
                                            .label(self.t("button-cancel"))
                                            .accessibility_label(
                                                self.t("settings-profile-cancel-aria"),
                                            )
                                            .on_click(cx.listener(|this, _, window, cx| {
                                                this.close_profile_editor(window, cx);
                                            })),
                                    ),
                            ),
                    )
                },
            )
            .child(
                div()
                    .mt_3()
                    .flex()
                    .flex_wrap()
                    .gap_2()
                    .child(
                        KitButton::new("add-local-profile")
                            .label(self.t("settings-profile-add-local"))
                            .accessibility_label(self.t("settings-profile-add-local-aria"))
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.add_draft_local_profile(window, cx);
                            })),
                    )
                    .child(
                        KitButton::new("add-remote-profile")
                            .label(self.t("settings-profile-add-remote"))
                            .accessibility_label(self.t("settings-profile-add-remote-aria"))
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.add_draft_remote_profile(window, cx);
                            })),
                    )
                    .child(
                        KitButton::new("save-profile-catalog")
                            .primary()
                            .label(self.t("settings-profile-save"))
                            .accessibility_label(self.t("settings-profile-save-aria"))
                            .on_click(cx.listener(|this, _, _, cx| {
                                cx.emit(SettingsPanelEvent::SaveProfileCatalog {
                                    catalog: this.profiles.clone(),
                                    secret_updates: this
                                        .settings_page
                                        .profile_secret_updates
                                        .clone(),
                                });
                            })),
                    ),
            )
            .when_some(
                self.settings_page
                    .pending_profile_delete
                    .as_ref()
                    .map(|pending| (pending.profile_id.clone(), pending.name.clone())),
                |section, (delete_id, delete_name)| {
                    let is_active = delete_id == self.profiles.active_profile_id;
                    section.child(
                        div()
                            .mt_3()
                            .flex()
                            .flex_col()
                            .gap_2()
                            .px_3()
                            .py_3()
                            .rounded_md()
                            .border_1()
                            .border_color(colors.danger)
                            .bg(with_alpha(colors.danger, 0.08))
                            .child(
                                div()
                                    .text_sm()
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(colors.text_primary)
                                    .child(self.t_args(
                                        "settings-profile-delete-title",
                                        &[("name", FluentValue::from(delete_name.as_str()))],
                                    )),
                            )
                            .child(div().text_xs().text_color(colors.text_muted).child(
                                if is_active {
                                    self.t("settings-profile-delete-active-hint")
                                } else {
                                    self.t("settings-profile-delete-hint")
                                },
                            ))
                            .child(
                                div()
                                    .flex()
                                    .flex_wrap()
                                    .gap_2()
                                    .child(
                                        KitButton::new("confirm-delete-profile")
                                            .danger()
                                            .label(self.t("settings-profile-delete-confirm"))
                                            .accessibility_label(self.t_args(
                                                "settings-profile-delete-confirm-aria",
                                                &[(
                                                    "name",
                                                    FluentValue::from(delete_name.as_str()),
                                                )],
                                            ))
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.confirm_remove_profile(cx);
                                            })),
                                    )
                                    .child(
                                        KitButton::new("cancel-delete-profile")
                                            .label(self.t("button-cancel"))
                                            .accessibility_label(
                                                self.t("settings-profile-delete-cancel-aria"),
                                            )
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.cancel_remove_profile(cx);
                                            })),
                                    ),
                            ),
                    )
                },
            )
    }
}
