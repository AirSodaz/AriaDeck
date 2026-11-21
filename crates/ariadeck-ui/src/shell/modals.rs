//! Modal dialogs rendered via `gpui_kit::component::WindowExt::open_dialog`
//! and `open_alert_dialog`, backed by the single mutually exclusive
//! [`ActiveModal`] state slot on [`AppShell`].
//!
//! `WindowExt::open_dialog` builds the dialog inside `Root`'s overlay layer,
//! which does not observe `AppShell`. Each open modal therefore mounts a pair of
//! lightweight observing views (`ShellModalBody` and `ShellModalFooter`) created
//! once on open that subscribe via `cx.observe(&shell, ...)`, ensuring live
//! updates (e.g. countdown ticks, validation errors, pending spinners, checkbox
//! toggles) re-render without rebuilding stateful entities.

use std::cell::Cell;

use ariadeck_i18n::FluentValue;
use gpui::{
    AnyElement, Context, Entity, IntoElement, Render, Role, SharedString, Subscription, Window,
    div, px,
};
use gpui_kit::component::{button::Button as KitButton, dialog::DialogFooter};

use super::*;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ModalKind {
    AddDownload,
    TaskOutputName,
    TaskSpeedLimit,
    TaskOptions,
    RemoveConfirmation,
    PowerCountdown,
    BatchFailureDetails,
    CoreSetupOnboarding,
}

/// Single mutually exclusive modal slot owned by [`AppShell`].
///
/// Dialog-specific `InputState` / child-view entities and their subscriptions
/// are created lazily when a modal opens and dropped when it closes.
pub(super) enum ActiveModal {
    AddDownload {
        view: Entity<AddDownloadDialogView>,
        busy: bool,
        _sub: Subscription,
    },
    OutputName {
        dialog: TaskOutputNameDialog,
        input: Entity<InputState>,
        _sub: Subscription,
    },
    TaskSpeedLimit {
        dialog: TaskSpeedLimitDialog,
        download_input: Entity<InputState>,
        upload_input: Entity<InputState>,
        _subs: [Subscription; 2],
    },
    TaskOptions {
        dialog: TaskOptionsDialog,
        seed_ratio_input: Entity<InputState>,
        seed_time_input: Entity<InputState>,
        _subs: [Subscription; 2],
    },
    RemoveConfirmation(RemoveConfirmation),
    BatchFailureDetails(BatchFailureDetails),
    PowerCountdown(PowerCountdownState),
    CoreSetupOnboarding {
        panel: Entity<CoreSetupPanelView>,
        _sub: Subscription,
    },
}

impl ActiveModal {
    pub(super) const fn kind(&self) -> ModalKind {
        match self {
            Self::AddDownload { .. } => ModalKind::AddDownload,
            Self::OutputName { .. } => ModalKind::TaskOutputName,
            Self::TaskSpeedLimit { .. } => ModalKind::TaskSpeedLimit,
            Self::TaskOptions { .. } => ModalKind::TaskOptions,
            Self::RemoveConfirmation(_) => ModalKind::RemoveConfirmation,
            Self::BatchFailureDetails(_) => ModalKind::BatchFailureDetails,
            Self::PowerCountdown(_) => ModalKind::PowerCountdown,
            Self::CoreSetupOnboarding { .. } => ModalKind::CoreSetupOnboarding,
        }
    }
}

struct ShellModalBody {
    shell: Entity<AppShell>,
    kind: ModalKind,
    _observer: Subscription,
}

impl ShellModalBody {
    fn new(shell: Entity<AppShell>, kind: ModalKind, cx: &mut Context<Self>) -> Self {
        let observer = cx.observe(&shell, |_, _, cx| cx.notify());
        Self {
            shell,
            kind,
            _observer: observer,
        }
    }
}

impl Render for ShellModalBody {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let shell = self.shell.clone();
        let kind = self.kind;
        let content = self
            .shell
            .update(cx, |this, cx| this.render_modal_body(kind, cx));
        let container = div().w_full().child(content);
        match kind {
            ModalKind::AddDownload => container
                .id("add-download-dialog")
                .key_context("AddDownloadDialog")
                .on_action(move |action: &CloseAddDownload, window, cx| {
                    shell.update(cx, |this, cx| {
                        this.close_add_download_action(action, window, cx);
                    });
                })
                .on_action({
                    let shell = self.shell.clone();
                    move |action: &SubmitAddDownload, window, cx| {
                        shell.update(cx, |this, cx| {
                            this.submit_add_download_action(action, window, cx);
                        });
                    }
                })
                .into_any_element(),
            ModalKind::TaskOutputName => container
                .id("task-output-name-dialog")
                .key_context("TaskOutputNameDialog")
                .on_action(move |action: &CloseTaskOutputName, window, cx| {
                    shell.update(cx, |this, cx| {
                        this.close_task_output_name_action(action, window, cx);
                    });
                })
                .on_action({
                    let shell = self.shell.clone();
                    move |action: &SubmitTaskOutputName, window, cx| {
                        shell.update(cx, |this, cx| {
                            this.submit_task_output_name_action(action, window, cx);
                        });
                    }
                })
                .into_any_element(),
            ModalKind::TaskSpeedLimit => container
                .id("task-speed-limit-dialog")
                .key_context("TaskSpeedLimitDialog")
                .on_action(move |action: &CloseTaskSpeedLimit, window, cx| {
                    shell.update(cx, |this, cx| {
                        this.close_task_speed_limit_action(action, window, cx);
                    });
                })
                .on_action({
                    let shell = self.shell.clone();
                    move |action: &SubmitTaskSpeedLimit, window, cx| {
                        shell.update(cx, |this, cx| {
                            this.submit_task_speed_limit_action(action, window, cx);
                        });
                    }
                })
                .into_any_element(),
            ModalKind::TaskOptions => container
                .id("task-options-dialog")
                .key_context("TaskOptionsDialog")
                .on_action(move |action: &CloseTaskOptions, window, cx| {
                    shell.update(cx, |this, cx| {
                        this.close_task_options_action(action, window, cx);
                    });
                })
                .on_action({
                    let shell = self.shell.clone();
                    move |action: &SubmitTaskOptions, window, cx| {
                        shell.update(cx, |this, cx| {
                            this.submit_task_options_action(action, window, cx);
                        });
                    }
                })
                .into_any_element(),
            ModalKind::RemoveConfirmation => container
                .id("remove-task-dialog")
                .key_context("RemoveTaskDialog")
                .into_any_element(),
            ModalKind::PowerCountdown => container
                .id("power-countdown-dialog")
                .key_context("PowerCountdownDialog")
                .into_any_element(),
            ModalKind::BatchFailureDetails => container
                .id("batch-failure-dialog")
                .key_context("BatchFailureDialog")
                .on_action(move |action: &CloseBatchFailures, window, cx| {
                    shell.update(cx, |this, cx| {
                        this.close_batch_failure_details_action(action, window, cx);
                    });
                })
                .into_any_element(),
            ModalKind::CoreSetupOnboarding => container
                .id("core-setup-dialog")
                .key_context("CoreSetupDialog")
                .on_action(move |action: &CloseCoreSetup, window, cx| {
                    shell.update(cx, |this, cx| {
                        this.close_core_setup_onboarding_action(action, window, cx);
                    });
                })
                .into_any_element(),
        }
    }
}

struct ShellModalFooter {
    shell: Entity<AppShell>,
    kind: ModalKind,
    _observer: Subscription,
}

impl ShellModalFooter {
    fn new(shell: Entity<AppShell>, kind: ModalKind, cx: &mut Context<Self>) -> Self {
        let observer = cx.observe(&shell, |_, _, cx| cx.notify());
        Self {
            shell,
            kind,
            _observer: observer,
        }
    }
}

impl Render for ShellModalFooter {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.shell
            .update(cx, |this, cx| this.render_modal_footer(self.kind, cx))
    }
}

impl AppShell {
    #[cfg(test)]
    pub(super) fn add_dialog_entity(&self) -> Option<&Entity<AddDownloadDialogView>> {
        self.add_dialog_view()
    }

    #[cfg(test)]
    pub(super) fn output_name_modal(&self) -> Option<(&TaskOutputNameDialog, &Entity<InputState>)> {
        match &self.active_modal {
            Some(ActiveModal::OutputName { dialog, input, .. }) => Some((dialog, input)),
            _ => None,
        }
    }

    #[cfg(test)]
    pub(super) fn task_speed_limit_modal(
        &self,
    ) -> Option<(
        &TaskSpeedLimitDialog,
        &Entity<InputState>,
        &Entity<InputState>,
    )> {
        match &self.active_modal {
            Some(ActiveModal::TaskSpeedLimit {
                dialog,
                download_input,
                upload_input,
                ..
            }) => Some((dialog, download_input, upload_input)),
            _ => None,
        }
    }

    #[cfg(test)]
    pub(super) fn task_options_modal(
        &self,
    ) -> Option<(&TaskOptionsDialog, &Entity<InputState>, &Entity<InputState>)> {
        match &self.active_modal {
            Some(ActiveModal::TaskOptions {
                dialog,
                seed_ratio_input,
                seed_time_input,
                ..
            }) => Some((dialog, seed_ratio_input, seed_time_input)),
            _ => None,
        }
    }

    #[cfg(test)]
    pub(super) fn remove_confirmation_modal(&self) -> Option<&RemoveConfirmation> {
        match &self.active_modal {
            Some(ActiveModal::RemoveConfirmation(c)) => Some(c),
            _ => None,
        }
    }

    #[cfg(test)]
    pub(super) fn batch_failure_details_modal(&self) -> Option<&BatchFailureDetails> {
        match &self.active_modal {
            Some(ActiveModal::BatchFailureDetails(d)) => Some(d),
            _ => None,
        }
    }

    pub(crate) fn desired_kit_modal(&self) -> Option<ModalKind> {
        self.active_modal.as_ref().map(ActiveModal::kind)
    }

    pub(crate) fn sync_active_modal_context(
        &self,
        window: Option<&mut Window>,
        cx: &mut Context<Self>,
    ) {
        match &self.active_modal {
            Some(ActiveModal::AddDownload { view, .. }) => {
                let theme = self.theme;
                let translator = self.translator.clone();
                let categories = self.settings.categories.clone();
                let download_directory = self.settings.download_directory.clone();
                view.update(cx, |dialog, cx| {
                    dialog.sync_context(
                        theme,
                        translator,
                        categories,
                        download_directory,
                        window,
                        cx,
                    );
                });
            }
            Some(ActiveModal::CoreSetupOnboarding { panel, .. }) => {
                let theme = self.theme;
                let translator = self.translator.clone();
                let state = self.core_setup.clone();
                panel.update(cx, |panel, cx| {
                    panel.sync(theme, translator, &state, cx);
                });
            }
            Some(ActiveModal::TaskSpeedLimit {
                download_input,
                upload_input,
                ..
            }) => {
                if let Some(window) = window {
                    let dl = self.t("input-download-limit-placeholder");
                    let ul = self.t("input-upload-limit-placeholder");
                    download_input.update(cx, |input, cx| input.set_placeholder(dl, window, cx));
                    upload_input.update(cx, |input, cx| input.set_placeholder(ul, window, cx));
                }
            }
            Some(ActiveModal::TaskOptions {
                seed_ratio_input,
                seed_time_input,
                ..
            }) => {
                if let Some(window) = window {
                    let ratio = self.t("dialog-task-options-seed-ratio-placeholder");
                    let time = self.t("dialog-task-options-seed-time-placeholder");
                    seed_ratio_input
                        .update(cx, |input, cx| input.set_placeholder(ratio, window, cx));
                    seed_time_input.update(cx, |input, cx| input.set_placeholder(time, window, cx));
                }
            }
            Some(
                ActiveModal::OutputName { .. }
                | ActiveModal::RemoveConfirmation(_)
                | ActiveModal::BatchFailureDetails(_)
                | ActiveModal::PowerCountdown(_),
            )
            | None => {}
        }
    }

    /// Reconcile `AppShell`'s modal state with `Root`'s active dialog layer.
    pub(crate) fn sync_kit_modals(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !has_kit_root(window) {
            return;
        }
        let desired = self.desired_kit_modal();
        if desired == self.active_kit_modal {
            return;
        }
        if self.active_kit_modal.take().is_some() {
            window.close_dialog(cx);
        }
        if let Some(kind) = desired {
            self.active_kit_modal = Some(kind);
            self.open_kit_modal(kind, window, cx);
        }
    }

    /// Deferred modal sync for state transitions that do not hold `&mut Window`
    /// (such as background countdown ticks or async command results).
    pub(crate) fn sync_kit_modals_deferred(&self, cx: &mut Context<Self>) {
        let shell = cx.entity().downgrade();
        let handle = self.window_handle;
        cx.defer(move |cx| {
            let _ = handle.update(cx, |_, window, cx| {
                let _ = shell.update(cx, |this, cx| {
                    this.sync_kit_modals(window, cx);
                });
            });
        });
    }

    fn open_kit_modal(&mut self, kind: ModalKind, window: &mut Window, cx: &mut Context<Self>) {
        let shell = cx.entity();
        let body = cx.new(|cx| ShellModalBody::new(shell.clone(), kind, cx));
        let footer = cx.new(|cx| ShellModalFooter::new(shell.clone(), kind, cx));

        match kind {
            ModalKind::AddDownload => {
                let title = self.t("dialog-add-download");
                let initial_focus = Cell::new(true);
                let shell_focus = shell.downgrade();
                let shell_width = shell.downgrade();
                let shell_cancel = shell.downgrade();
                let shell_close = shell.downgrade();
                window.open_dialog(cx, move |dialog, window, cx| {
                    let width = shell_width
                        .read_with(cx, |this, cx| this.add_dialog_width(cx))
                        .unwrap_or(560.0);
                    if initial_focus.replace(false)
                        && let Some(focus) = shell_focus
                            .read_with(cx, |this, cx| {
                                this.add_dialog_view().and_then(|view| {
                                    let dialog = view.read(cx);
                                    (dialog.input_mode == AddDownloadInputModeView::Links)
                                        .then(|| dialog.add_input.focus_handle(cx))
                                })
                            })
                            .ok()
                            .flatten()
                    {
                        focus.focus(window, cx);
                    }
                    let shell_cancel = shell_cancel.clone();
                    let shell_close = shell_close.clone();
                    dialog
                        .title(title.clone())
                        .width(px(width))
                        .centered()
                        .overlay_closable(false)
                        .close_button(true)
                        .keyboard(true)
                        .child(body.clone())
                        .footer(footer.clone())
                        .on_ok(|_, _, _| false)
                        .on_cancel(move |_, window, cx| {
                            shell_cancel
                                .update(cx, |this, cx| {
                                    this.try_cancel_kit_modal(ModalKind::AddDownload, window, cx)
                                })
                                .unwrap_or(true)
                        })
                        .on_close(move |_, _, cx| {
                            let _ = shell_close.update(cx, |this, cx| {
                                this.on_kit_modal_closed(ModalKind::AddDownload, cx);
                            });
                        })
                });
            }
            ModalKind::TaskOutputName => {
                let title = self.t("dialog-output-name-title");
                let initial_focus = Cell::new(true);
                let shell_focus = shell.downgrade();
                let shell_ok = shell.downgrade();
                let shell_cancel = shell.downgrade();
                let shell_close = shell.downgrade();
                window.open_dialog(cx, move |dialog, window, cx| {
                    if initial_focus.replace(false)
                        && let Some(focus) = shell_focus
                            .read_with(cx, |this, cx| match &this.active_modal {
                                Some(ActiveModal::OutputName { input, .. }) => {
                                    Some(input.focus_handle(cx))
                                }
                                _ => None,
                            })
                            .ok()
                            .flatten()
                    {
                        focus.focus(window, cx);
                    }
                    let shell_ok = shell_ok.clone();
                    let shell_cancel = shell_cancel.clone();
                    let shell_close = shell_close.clone();
                    dialog
                        .title(title.clone())
                        .width(px(520.0))
                        .centered()
                        .overlay_closable(false)
                        .close_button(true)
                        .keyboard(true)
                        .child(body.clone())
                        .footer(footer.clone())
                        .on_ok(move |_, _, cx| {
                            let _ = shell_ok.update(cx, |this, cx| {
                                this.submit_task_output_name(cx);
                            });
                            false
                        })
                        .on_cancel(move |_, window, cx| {
                            shell_cancel
                                .update(cx, |this, cx| {
                                    this.try_cancel_kit_modal(ModalKind::TaskOutputName, window, cx)
                                })
                                .unwrap_or(true)
                        })
                        .on_close(move |_, _, cx| {
                            let _ = shell_close.update(cx, |this, cx| {
                                this.on_kit_modal_closed(ModalKind::TaskOutputName, cx);
                            });
                        })
                });
            }
            ModalKind::TaskSpeedLimit => {
                let title = self.t("dialog-speed-limit-title");
                let initial_focus = Cell::new(true);
                let shell_focus = shell.downgrade();
                let shell_ok = shell.downgrade();
                let shell_cancel = shell.downgrade();
                let shell_close = shell.downgrade();
                window.open_dialog(cx, move |dialog, window, cx| {
                    if initial_focus.replace(false)
                        && let Some(focus) = shell_focus
                            .read_with(cx, |this, cx| match &this.active_modal {
                                Some(ActiveModal::TaskSpeedLimit { download_input, .. }) => {
                                    Some(download_input.focus_handle(cx))
                                }
                                _ => None,
                            })
                            .ok()
                            .flatten()
                    {
                        focus.focus(window, cx);
                    }
                    let shell_ok = shell_ok.clone();
                    let shell_cancel = shell_cancel.clone();
                    let shell_close = shell_close.clone();
                    dialog
                        .title(title.clone())
                        .width(px(520.0))
                        .centered()
                        .overlay_closable(false)
                        .close_button(true)
                        .keyboard(true)
                        .child(body.clone())
                        .footer(footer.clone())
                        .on_ok(move |_, _, cx| {
                            let _ = shell_ok.update(cx, |this, cx| {
                                this.submit_task_speed_limit(cx);
                            });
                            false
                        })
                        .on_cancel(move |_, window, cx| {
                            shell_cancel
                                .update(cx, |this, cx| {
                                    this.try_cancel_kit_modal(ModalKind::TaskSpeedLimit, window, cx)
                                })
                                .unwrap_or(true)
                        })
                        .on_close(move |_, _, cx| {
                            let _ = shell_close.update(cx, |this, cx| {
                                this.on_kit_modal_closed(ModalKind::TaskSpeedLimit, cx);
                            });
                        })
                });
            }
            ModalKind::TaskOptions => {
                let title = self.t("dialog-task-options-title");
                let initial_focus = Cell::new(true);
                let shell_focus = shell.downgrade();
                let shell_ok = shell.downgrade();
                let shell_cancel = shell.downgrade();
                let shell_close = shell.downgrade();
                window.open_dialog(cx, move |dialog, window, cx| {
                    if initial_focus.replace(false)
                        && let Some(focus) = shell_focus
                            .read_with(cx, |this, cx| match &this.active_modal {
                                Some(ActiveModal::TaskOptions {
                                    dialog,
                                    seed_ratio_input,
                                    ..
                                }) if dialog.supports_seed_rules => {
                                    Some(seed_ratio_input.focus_handle(cx))
                                }
                                _ => None,
                            })
                            .ok()
                            .flatten()
                    {
                        focus.focus(window, cx);
                    }
                    let shell_ok = shell_ok.clone();
                    let shell_cancel = shell_cancel.clone();
                    let shell_close = shell_close.clone();
                    dialog
                        .title(title.clone())
                        .width(px(520.0))
                        .centered()
                        .overlay_closable(false)
                        .close_button(true)
                        .keyboard(true)
                        .child(body.clone())
                        .footer(footer.clone())
                        .on_ok(move |_, _, cx| {
                            let _ = shell_ok.update(cx, |this, cx| {
                                this.submit_task_options(cx);
                            });
                            false
                        })
                        .on_cancel(move |_, window, cx| {
                            shell_cancel
                                .update(cx, |this, cx| {
                                    this.try_cancel_kit_modal(ModalKind::TaskOptions, window, cx)
                                })
                                .unwrap_or(true)
                        })
                        .on_close(move |_, _, cx| {
                            let _ = shell_close.update(cx, |this, cx| {
                                this.on_kit_modal_closed(ModalKind::TaskOptions, cx);
                            });
                        })
                });
            }
            ModalKind::RemoveConfirmation => {
                let title = self.t("dialog-remove-title");
                let danger = self.theme.colors.danger;
                let shell_ok = shell.downgrade();
                let shell_cancel = shell.downgrade();
                let shell_close = shell.downgrade();
                window.open_alert_dialog(cx, move |alert, _, _| {
                    let shell_ok = shell_ok.clone();
                    let shell_cancel = shell_cancel.clone();
                    let shell_close = shell_close.clone();
                    alert
                        .title(title.clone())
                        .icon(
                            Icon::new(IconName::TriangleAlert)
                                .size(IconSize::Medium)
                                .color(danger),
                        )
                        .width(px(460.0))
                        .centered()
                        .keyboard(true)
                        .child(body.clone())
                        .footer(footer.clone())
                        .on_ok(move |_, _, cx| {
                            let _ = shell_ok.update(cx, |this, cx| {
                                this.active_kit_modal = None;
                                this.submit_remove_confirmation(cx);
                            });
                            true
                        })
                        .on_cancel(move |_, window, cx| {
                            shell_cancel
                                .update(cx, |this, cx| {
                                    this.try_cancel_kit_modal(
                                        ModalKind::RemoveConfirmation,
                                        window,
                                        cx,
                                    )
                                })
                                .unwrap_or(true)
                        })
                        .on_close(move |_, _, cx| {
                            let _ = shell_close.update(cx, |this, cx| {
                                this.on_kit_modal_closed(ModalKind::RemoveConfirmation, cx);
                            });
                        })
                });
            }
            ModalKind::PowerCountdown => {
                let title = self.t("dialog-power-countdown-title");
                let warning = self.theme.colors.warning;
                let shell_ok = shell.downgrade();
                let shell_cancel = shell.downgrade();
                let shell_close = shell.downgrade();
                window.open_alert_dialog(cx, move |alert, _, _| {
                    let shell_ok = shell_ok.clone();
                    let shell_cancel = shell_cancel.clone();
                    let shell_close = shell_close.clone();
                    alert
                        .title(title.clone())
                        .icon(
                            Icon::new(IconName::TriangleAlert)
                                .size(IconSize::Medium)
                                .color(warning),
                        )
                        .width(px(440.0))
                        .centered()
                        .keyboard(true)
                        .child(body.clone())
                        .footer(footer.clone())
                        .on_ok(move |_, _, cx| {
                            let _ = shell_ok.update(cx, |this, cx| {
                                this.active_kit_modal = None;
                                this.execute_power_action_now(cx);
                            });
                            true
                        })
                        .on_cancel(move |_, window, cx| {
                            shell_cancel
                                .update(cx, |this, cx| {
                                    this.try_cancel_kit_modal(ModalKind::PowerCountdown, window, cx)
                                })
                                .unwrap_or(true)
                        })
                        .on_close(move |_, _, cx| {
                            let _ = shell_close.update(cx, |this, cx| {
                                this.on_kit_modal_closed(ModalKind::PowerCountdown, cx);
                            });
                        })
                });
            }
            ModalKind::BatchFailureDetails => {
                let title = self.t("dialog-batch-title");
                let danger = self.theme.colors.danger;
                let shell_ok = shell.downgrade();
                let shell_cancel = shell.downgrade();
                let shell_close = shell.downgrade();
                window.open_alert_dialog(cx, move |alert, _, _| {
                    let shell_ok = shell_ok.clone();
                    let shell_cancel = shell_cancel.clone();
                    let shell_close = shell_close.clone();
                    alert
                        .title(title.clone())
                        .icon(
                            Icon::new(IconName::CircleAlert)
                                .size(IconSize::Medium)
                                .color(danger),
                        )
                        .width(px(560.0))
                        .centered()
                        .keyboard(true)
                        .child(body.clone())
                        .footer(footer.clone())
                        .on_ok(move |_, window, cx| {
                            shell_ok
                                .update(cx, |this, cx| {
                                    this.try_cancel_kit_modal(
                                        ModalKind::BatchFailureDetails,
                                        window,
                                        cx,
                                    )
                                })
                                .unwrap_or(true)
                        })
                        .on_cancel(move |_, window, cx| {
                            shell_cancel
                                .update(cx, |this, cx| {
                                    this.try_cancel_kit_modal(
                                        ModalKind::BatchFailureDetails,
                                        window,
                                        cx,
                                    )
                                })
                                .unwrap_or(true)
                        })
                        .on_close(move |_, _, cx| {
                            let _ = shell_close.update(cx, |this, cx| {
                                this.on_kit_modal_closed(ModalKind::BatchFailureDetails, cx);
                            });
                        })
                });
            }
            ModalKind::CoreSetupOnboarding => {
                let title = self.t("dialog-core-setup-title");
                let shell_cancel = shell.downgrade();
                let shell_close = shell.downgrade();
                window.open_dialog(cx, move |dialog, _, _| {
                    let shell_cancel = shell_cancel.clone();
                    let shell_close = shell_close.clone();
                    dialog
                        .title(title.clone())
                        .width(px(560.0))
                        .centered()
                        .overlay_closable(false)
                        .close_button(true)
                        .keyboard(true)
                        .child(body.clone())
                        .footer(footer.clone())
                        .on_ok(|_, _, _| false)
                        .on_cancel(move |_, window, cx| {
                            shell_cancel
                                .update(cx, |this, cx| {
                                    this.try_cancel_kit_modal(
                                        ModalKind::CoreSetupOnboarding,
                                        window,
                                        cx,
                                    )
                                })
                                .unwrap_or(true)
                        })
                        .on_close(move |_, _, cx| {
                            let _ = shell_close.update(cx, |this, cx| {
                                this.on_kit_modal_closed(ModalKind::CoreSetupOnboarding, cx);
                            });
                        })
                });
            }
        }
    }

    fn add_dialog_width(&self, cx: &App) -> f32 {
        self.add_dialog_view()
            .map_or(560.0, |view| view.read(cx).dialog_width())
    }

    fn try_cancel_kit_modal(
        &mut self,
        kind: ModalKind,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.active_modal.as_ref().map(ActiveModal::kind) != Some(kind) {
            if self.active_kit_modal == Some(kind) {
                self.active_kit_modal = None;
            }
            return true;
        }
        match kind {
            ModalKind::AddDownload => {
                if let Some(view) = self.add_dialog_view() {
                    if view.read(cx).is_busy() {
                        return false;
                    }
                    self.links_input_height = view.read(cx).links_input_height;
                }
                self.active_modal = None;
            }
            ModalKind::TaskOutputName => {
                if self.pending_task_command.as_ref().is_some_and(|pending| {
                    matches!(&pending.command, TaskCommandView::SetOutputName { .. })
                }) {
                    return false;
                }
                self.active_modal = None;
            }
            ModalKind::TaskSpeedLimit => {
                if self.pending_task_command.as_ref().is_some_and(|pending| {
                    matches!(&pending.command, TaskCommandView::SetSpeedLimit { .. })
                }) {
                    return false;
                }
                self.active_modal = None;
            }
            ModalKind::TaskOptions => {
                if self.pending_task_command.as_ref().is_some_and(|pending| {
                    matches!(&pending.command, TaskCommandView::SetOptions { .. })
                }) {
                    return false;
                }
                self.active_modal = None;
            }
            ModalKind::RemoveConfirmation | ModalKind::BatchFailureDetails => {
                self.active_modal = None;
            }
            ModalKind::PowerCountdown => {
                self.active_kit_modal = None;
                self.cancel_power_countdown(cx);
                return true;
            }
            ModalKind::CoreSetupOnboarding => {
                self.active_modal = None;
                cx.emit(AppShellEvent::CoreSetupOnboardingDismissed);
            }
        }
        if self.active_kit_modal == Some(kind) {
            self.active_kit_modal = None;
        }
        cx.notify();
        true
    }

    fn on_kit_modal_closed(&mut self, kind: ModalKind, cx: &mut Context<Self>) {
        if self.active_kit_modal == Some(kind) {
            self.active_kit_modal = None;
        }
        if self.active_modal.as_ref().map(ActiveModal::kind) != Some(kind) {
            return;
        }
        match kind {
            ModalKind::AddDownload => {
                if let Some(view) = self.add_dialog_view()
                    && !view.read(cx).is_busy()
                {
                    self.links_input_height = view.read(cx).links_input_height;
                    self.active_modal = None;
                    cx.notify();
                }
            }
            ModalKind::TaskOutputName
            | ModalKind::TaskSpeedLimit
            | ModalKind::TaskOptions
            | ModalKind::RemoveConfirmation
            | ModalKind::BatchFailureDetails => {
                self.active_modal = None;
                cx.notify();
            }
            ModalKind::PowerCountdown => {
                self.cancel_power_countdown(cx);
            }
            ModalKind::CoreSetupOnboarding => {
                self.active_modal = None;
                cx.emit(AppShellEvent::CoreSetupOnboardingDismissed);
                cx.notify();
            }
        }
    }

    fn render_modal_body(&mut self, kind: ModalKind, cx: &mut Context<Self>) -> AnyElement {
        match kind {
            ModalKind::AddDownload => self
                .add_dialog_view()
                .cloned()
                .map_or_else(|| div().into_any_element(), IntoElement::into_any_element),
            ModalKind::TaskOutputName => self.render_task_output_name_body(cx),
            ModalKind::TaskSpeedLimit => self.render_task_speed_limit_body(cx),
            ModalKind::TaskOptions => self.render_task_options_body(cx),
            ModalKind::RemoveConfirmation => self.render_remove_confirmation_body(cx),
            ModalKind::PowerCountdown => self.render_power_countdown_body(cx),
            ModalKind::BatchFailureDetails => self.render_batch_failure_details_body(cx),
            ModalKind::CoreSetupOnboarding => self.render_core_setup_onboarding_body(cx),
        }
    }

    fn render_modal_footer(&mut self, kind: ModalKind, cx: &mut Context<Self>) -> DialogFooter {
        match kind {
            ModalKind::AddDownload => self.render_add_download_footer(cx),
            ModalKind::TaskOutputName => self.render_task_output_name_footer(cx),
            ModalKind::TaskSpeedLimit => self.render_task_speed_limit_footer(cx),
            ModalKind::TaskOptions => self.render_task_options_footer(cx),
            ModalKind::RemoveConfirmation => self.render_remove_confirmation_footer(cx),
            ModalKind::PowerCountdown => self.render_power_countdown_footer(cx),
            ModalKind::BatchFailureDetails => self.render_batch_failure_details_footer(cx),
            ModalKind::CoreSetupOnboarding => self.render_core_setup_onboarding_footer(cx),
        }
    }

    fn render_add_download_footer(&mut self, cx: &mut Context<Self>) -> DialogFooter {
        let (add_pending, preview_pending) =
            self.add_dialog_view().map_or((false, false), |view| {
                let dialog = view.read(cx);
                (dialog.pending.is_some(), dialog.preview_pending.is_some())
            });
        let pending = add_pending || preview_pending;
        DialogFooter::new()
            .child(
                KitButton::new("cancel-add-download")
                    .label(self.t("button-cancel"))
                    .accessibility_label(self.t("dialog-add-cancel-aria"))
                    .disabled(pending)
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.close_add_download(window, cx);
                    })),
            )
            .child(
                KitButton::new("submit-add-download")
                    .primary()
                    .label(self.t("dialog-add-submit"))
                    .accessibility_label(if add_pending {
                        self.t("dialog-add-submitting")
                    } else {
                        self.t("dialog-add-submit")
                    })
                    .disabled(preview_pending)
                    .loading(add_pending)
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.submit_add_download(cx);
                    })),
            )
    }

    fn render_task_output_name_footer(&mut self, cx: &mut Context<Self>) -> DialogFooter {
        let pending = match &self.active_modal {
            Some(ActiveModal::OutputName { dialog, .. }) => {
                self.pending_task_command.as_ref().is_some_and(|pending| {
                    pending.identity == dialog.identity
                        && matches!(&pending.command, TaskCommandView::SetOutputName { .. })
                })
            }
            _ => false,
        };
        DialogFooter::new()
            .child(
                KitButton::new("cancel-task-output-name")
                    .label(self.t("button-cancel"))
                    .accessibility_label(self.t("dialog-output-name-cancel-aria"))
                    .disabled(pending)
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.close_task_output_name(window, cx);
                    })),
            )
            .child(
                KitButton::new("submit-task-output-name")
                    .primary()
                    .label(if pending {
                        self.t("dialog-output-name-saving")
                    } else {
                        self.t("button-save")
                    })
                    .accessibility_label(if pending {
                        self.t("dialog-output-name-saving")
                    } else {
                        self.t("dialog-output-name-save")
                    })
                    .loading(pending)
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.submit_task_output_name(cx);
                    })),
            )
    }

    fn render_task_speed_limit_footer(&mut self, cx: &mut Context<Self>) -> DialogFooter {
        let pending = match &self.active_modal {
            Some(ActiveModal::TaskSpeedLimit { dialog, .. }) => {
                self.pending_task_command.as_ref().is_some_and(|pending| {
                    pending.identity == dialog.identity
                        && matches!(&pending.command, TaskCommandView::SetSpeedLimit { .. })
                })
            }
            _ => false,
        };
        DialogFooter::new()
            .child(
                KitButton::new("cancel-task-speed-limit")
                    .label(self.t("button-cancel"))
                    .accessibility_label(self.t("dialog-speed-limit-cancel-aria"))
                    .disabled(pending)
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.close_task_speed_limit(window, cx);
                    })),
            )
            .child(
                KitButton::new("submit-task-speed-limit")
                    .primary()
                    .label(if pending {
                        self.t("dialog-speed-limit-saving")
                    } else {
                        self.t("button-save")
                    })
                    .accessibility_label(if pending {
                        self.t("dialog-speed-limit-saving")
                    } else {
                        self.t("dialog-speed-limit-save")
                    })
                    .loading(pending)
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.submit_task_speed_limit(cx);
                    })),
            )
    }

    fn render_task_options_footer(&mut self, cx: &mut Context<Self>) -> DialogFooter {
        let (supports_seed_rules, pending) = match &self.active_modal {
            Some(ActiveModal::TaskOptions { dialog, .. }) => (
                dialog.supports_seed_rules,
                self.pending_task_command.as_ref().is_some_and(|pending| {
                    pending.identity == dialog.identity
                        && matches!(&pending.command, TaskCommandView::SetOptions { .. })
                }),
            ),
            _ => (false, false),
        };
        DialogFooter::new()
            .child(
                KitButton::new("cancel-task-options")
                    .label(self.t("button-cancel"))
                    .accessibility_label(self.t("dialog-task-options-cancel-aria"))
                    .disabled(pending)
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.close_task_options(window, cx);
                    })),
            )
            .child(
                KitButton::new("submit-task-options")
                    .primary()
                    .label(if pending {
                        self.t("dialog-task-options-saving")
                    } else {
                        self.t("button-save")
                    })
                    .accessibility_label(if pending {
                        self.t("dialog-task-options-saving")
                    } else {
                        self.t("dialog-task-options-save")
                    })
                    .loading(pending)
                    .disabled(!supports_seed_rules)
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.submit_task_options(cx);
                    })),
            )
    }

    fn render_remove_confirmation_footer(&mut self, cx: &mut Context<Self>) -> DialogFooter {
        let delete_files = matches!(
            &self.active_modal,
            Some(ActiveModal::RemoveConfirmation(confirmation)) if confirmation.delete_files
        );
        DialogFooter::new()
            .child(
                KitButton::new("cancel-remove-task")
                    .label(self.t("button-cancel"))
                    .accessibility_label(self.t("dialog-remove-cancel-aria"))
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.close_remove_confirmation(window, cx);
                    })),
            )
            .child(
                KitButton::new("confirm-remove-task")
                    .danger()
                    .label(if delete_files {
                        self.t("dialog-remove-and-files")
                    } else {
                        self.t("dialog-remove-confirm")
                    })
                    .accessibility_label(if delete_files {
                        self.t("dialog-remove-submit-aria")
                    } else {
                        self.t("dialog-remove-confirm")
                    })
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.submit_remove_confirmation(cx);
                        this.sync_kit_modals(window, cx);
                    })),
            )
    }

    fn render_power_countdown_footer(&mut self, cx: &mut Context<Self>) -> DialogFooter {
        let action = self
            .power_countdown()
            .map(|c| c.action)
            .unwrap_or(CompletionPowerActionView::None);
        let action_btn = self.action_btn_label(action);
        DialogFooter::new()
            .child(
                KitButton::new("cancel-power-countdown")
                    .label(self.t("dialog-power-cancel"))
                    .accessibility_label(self.t("dialog-power-cancel"))
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.cancel_power_countdown(cx);
                        this.sync_kit_modals(window, cx);
                    })),
            )
            .child(
                KitButton::new("execute-power-now")
                    .primary()
                    .label(action_btn.clone())
                    .accessibility_label(action_btn)
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.execute_power_action_now(cx);
                        this.sync_kit_modals(window, cx);
                    })),
            )
    }

    fn render_batch_failure_details_footer(&mut self, cx: &mut Context<Self>) -> DialogFooter {
        DialogFooter::new().child(
            KitButton::new("close-batch-failures")
                .label(self.t("button-close"))
                .accessibility_label(self.t("dialog-batch-close-aria"))
                .on_click(cx.listener(|this, _, window, cx| {
                    this.close_batch_failure_details(window, cx);
                })),
        )
    }

    fn render_core_setup_onboarding_footer(&mut self, cx: &mut Context<Self>) -> DialogFooter {
        DialogFooter::new()
            .child(
                KitButton::new("core-setup-open-settings")
                    .label(self.t("dialog-core-setup-open-settings"))
                    .accessibility_label(self.t("dialog-core-setup-open-settings-aria"))
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.open_engine_settings_from_onboarding(window, cx);
                    })),
            )
            .child(
                KitButton::new("core-setup-dismiss")
                    .primary()
                    .label(self.t("dialog-core-setup-dismiss"))
                    .accessibility_label(self.t("dialog-core-setup-dismiss-aria"))
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.dismiss_core_setup_onboarding(window, cx);
                    })),
            )
    }

    fn render_power_countdown_body(&mut self, _cx: &mut Context<Self>) -> AnyElement {
        let colors = self.theme.colors;
        let (action, seconds) = self
            .power_countdown()
            .map(|c| (c.action, c.remaining_seconds))
            .unwrap_or((CompletionPowerActionView::None, 0));
        let action_verb = self.action_verb_label(action);
        let description = self.t_args(
            "dialog-power-countdown-desc",
            &[
                ("action", FluentValue::from(action_verb.as_str())),
                ("seconds", FluentValue::from(seconds.to_string().as_str())),
            ],
        );
        div()
            .flex()
            .flex_col()
            .gap_3()
            .child(
                div()
                    .text_sm()
                    .text_color(colors.text_secondary)
                    .child(description),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .text_xs()
                    .text_color(colors.text_secondary)
                    .child(
                        Icon::new(IconName::TriangleAlert)
                            .size(IconSize::Small)
                            .color(colors.warning),
                    )
                    .child(format!("{seconds}s")),
            )
            .into_any_element()
    }

    fn render_core_setup_onboarding_body(&mut self, _cx: &mut Context<Self>) -> AnyElement {
        let colors = self.theme.colors;
        let has_core = !self.cores.installations.is_empty();
        let panel = match &self.active_modal {
            Some(ActiveModal::CoreSetupOnboarding { panel, .. }) => Some(panel.clone()),
            _ => None,
        };
        div()
            .flex()
            .flex_col()
            .gap_3()
            .child(
                div()
                    .text_sm()
                    .text_color(colors.text_secondary)
                    .child(self.t("dialog-core-setup-description")),
            )
            .child(
                div()
                    .flex()
                    .items_start()
                    .gap_2()
                    .text_xs()
                    .text_color(colors.text_secondary)
                    .child(Icon::new(IconName::Info).size(IconSize::Small))
                    .child(self.t("dialog-core-setup-remote-hint")),
            )
            .when_some(panel, |body, panel| body.child(panel))
            .when(has_core, |body| {
                body.child(
                    div()
                        .flex()
                        .items_start()
                        .gap_2()
                        .text_xs()
                        .text_color(colors.text_secondary)
                        .child(
                            Icon::new(IconName::CircleCheck)
                                .size(IconSize::Small)
                                .color(colors.success),
                        )
                        .child(self.t("dialog-core-setup-restart-hint")),
                )
            })
            .into_any_element()
    }

    fn render_remove_confirmation_body(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let colors = self.theme.colors;
        let (display_name, has_live_tasks, has_terminal_tasks, delete_files) =
            match &self.active_modal {
                Some(ActiveModal::RemoveConfirmation(confirmation)) => (
                    confirmation.display_name.clone(),
                    confirmation.has_live_tasks,
                    confirmation.has_terminal_tasks,
                    confirmation.delete_files,
                ),
                _ => Default::default(),
            };
        let local_files_available = !matches!(self.engine_health, EngineHealthView::External);
        let removal_description_key = match (has_live_tasks, has_terminal_tasks) {
            (true, true) => "dialog-remove-description-mixed",
            (true, false) => "dialog-remove-description-live",
            (false, true) => "dialog-remove-description-terminal",
            (false, false) => "dialog-remove-description-generic",
        };
        let removal_description = self.t_args(
            removal_description_key,
            &[("name", FluentValue::from(display_name))],
        );
        let file_choice = if local_files_available {
            Checkbox::new("remove-task-files")
                .checked(delete_files)
                .accessibility_label(self.t("dialog-remove-files-aria"))
                .on_click(cx.listener(|this, _, _, cx| {
                    this.toggle_remove_files(cx);
                }))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_1()
                        .text_sm()
                        .text_color(colors.text_primary)
                        .child(self.t("dialog-remove-files-checkbox"))
                        .child(
                            div()
                                .text_xs()
                                .text_color(colors.text_muted)
                                .child(self.t("ui-remove-aria2-control")),
                        ),
                )
                .into_any_element()
        } else {
            div()
                .flex()
                .items_center()
                .gap_2()
                .text_xs()
                .text_color(colors.text_secondary)
                .child(Icon::new(IconName::Info).size(IconSize::Small))
                .child(self.t("ui-external-engine-files-kept"))
                .into_any_element()
        };
        div()
            .flex()
            .flex_col()
            .gap_3()
            .child(
                div()
                    .text_sm()
                    .text_color(colors.text_secondary)
                    .child(removal_description),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .text_xs()
                    .text_color(colors.text_secondary)
                    .child(
                        Icon::new(IconName::TriangleAlert)
                            .size(IconSize::Small)
                            .color(colors.danger),
                    )
                    .child(if delete_files {
                        self.t("dialog-remove-files-warning")
                    } else {
                        self.t("dialog-remove-files-kept")
                    }),
            )
            .child(file_choice)
            .into_any_element()
    }

    fn render_batch_failure_details_body(&mut self, _cx: &mut Context<Self>) -> AnyElement {
        let colors = self.theme.colors;
        let Some(ActiveModal::BatchFailureDetails(details)) = &self.active_modal else {
            return div().into_any_element();
        };
        let command = self.t(super::commands::batch_task_command_label_key(
            details.command,
        ));
        let summary = self.t_count(
            "dialog-batch-failure-summary",
            u64::try_from(details.failures.len()).unwrap_or(u64::MAX),
        );
        let failures = details
            .failures
            .iter()
            .enumerate()
            .map(|(index, failure)| {
                let task_name = failure.identity.as_ref().map_or_else(
                    || self.t("dialog-batch-request"),
                    |identity| {
                        self.snapshot
                            .tasks
                            .iter()
                            .find(|task| task.identity == *identity)
                            .map(|task| task_display_name(task, &self.translator))
                            .unwrap_or_else(|| {
                                self.t_args(
                                    "dialog-batch-task-name",
                                    &[("gid", FluentValue::from(identity.gid.clone()))],
                                )
                            })
                    },
                );
                let row_id = failure.identity.as_ref().map_or_else(
                    || SharedString::from(format!("batch-failure-request-{index}")),
                    |identity| {
                        SharedString::from(format!(
                            "batch-failure:{}:{}",
                            identity.profile_id, identity.gid
                        ))
                    },
                );
                div()
                    .id(row_id)
                    .role(Role::ListItem)
                    .flex()
                    .items_start()
                    .gap_2()
                    .child(
                        Icon::new(IconName::CircleAlert)
                            .size(IconSize::Small)
                            .color(colors.danger),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .child(
                                div()
                                    .text_xs()
                                    .font_weight(FontWeight::MEDIUM)
                                    .child(task_name),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(colors.text_muted)
                                    .child(self.te(&failure.error)),
                            ),
                    )
            })
            .collect::<Vec<_>>();
        div()
            .flex()
            .flex_col()
            .gap_3()
            .child(
                div()
                    .text_sm()
                    .text_color(colors.text_secondary)
                    .child(summary),
            )
            .child(
                div()
                    .id("batch-failure-list")
                    .role(Role::List)
                    .aria_label(self.t_args(
                        "dialog-batch-failure-list-aria",
                        &[("action", FluentValue::from(command))],
                    ))
                    .flex()
                    .flex_col()
                    .gap_3()
                    .children(failures),
            )
            .into_any_element()
    }

    fn render_task_output_name_body(&mut self, _cx: &mut Context<Self>) -> AnyElement {
        let colors = self.theme.colors;
        let Some(ActiveModal::OutputName { dialog, input, .. }) = &self.active_modal else {
            return div().into_any_element();
        };
        let display_name = dialog.display_name.clone();
        let active = dialog.active;
        let error = dialog.error.clone();
        let input = input.clone();
        div()
            .flex()
            .flex_col()
            .gap_3()
            .child(
                div()
                    .text_sm()
                    .text_color(colors.text_secondary)
                    .child(self.t_args(
                        "dialog-output-name-description",
                        &[("name", FluentValue::from(display_name))],
                    )),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child(
                        div()
                            .text_xs()
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(colors.text_secondary)
                            .child(self.t("dialog-output-name-filename")),
                    )
                    .child(Input::new(&input))
                    .when(active, |element| {
                        element.child(
                            div()
                                .id("active-output-name-warning")
                                .role(Role::Status)
                                .flex()
                                .items_center()
                                .gap_2()
                                .text_xs()
                                .text_color(colors.warning)
                                .child(
                                    Icon::new(IconName::TriangleAlert)
                                        .size(IconSize::Small)
                                        .color(colors.warning),
                                )
                                .child(self.t("ui-output-name-restart")),
                        )
                    })
                    .when_some(error, |element, error| {
                        element.child(
                            div()
                                .id("task-output-name-error")
                                .role(Role::Alert)
                                .aria_label(self.te(&error))
                                .text_xs()
                                .text_color(colors.danger)
                                .child(self.te(&error)),
                        )
                    }),
            )
            .into_any_element()
    }

    fn render_task_speed_limit_body(&mut self, _cx: &mut Context<Self>) -> AnyElement {
        let colors = self.theme.colors;
        let Some(ActiveModal::TaskSpeedLimit {
            dialog,
            download_input,
            upload_input,
            ..
        }) = &self.active_modal
        else {
            return div().into_any_element();
        };
        let display_name = dialog.display_name.clone();
        let error = dialog.error.clone();
        let download_input = download_input.clone();
        let upload_input = upload_input.clone();
        div()
            .flex()
            .flex_col()
            .gap_3()
            .child(
                div()
                    .text_sm()
                    .text_color(colors.text_secondary)
                    .child(self.t_args(
                        "dialog-speed-limit-description",
                        &[("name", FluentValue::from(display_name))],
                    )),
            )
            .child(
                div()
                    .flex()
                    .gap_3()
                    .child(
                        settings_labeled_input(
                            self.t("dialog-speed-limit-download"),
                            &download_input,
                            colors,
                        )
                        .flex_1()
                        .min_w_0(),
                    )
                    .child(
                        settings_labeled_input(
                            self.t("dialog-speed-limit-upload"),
                            &upload_input,
                            colors,
                        )
                        .flex_1()
                        .min_w_0(),
                    ),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(colors.text_muted)
                    .child(self.t("dialog-speed-limit-help")),
            )
            .when_some(error, |element, error| {
                element.child(
                    div()
                        .id("task-speed-limit-error")
                        .role(Role::Alert)
                        .aria_label(self.te(&error))
                        .text_xs()
                        .text_color(colors.danger)
                        .child(self.te(&error)),
                )
            })
            .into_any_element()
    }

    fn render_task_options_body(&mut self, _cx: &mut Context<Self>) -> AnyElement {
        let colors = self.theme.colors;
        let Some(ActiveModal::TaskOptions {
            dialog,
            seed_ratio_input,
            seed_time_input,
            ..
        }) = &self.active_modal
        else {
            return div().into_any_element();
        };
        let display_name = dialog.display_name.clone();
        let supports_seed_rules = dialog.supports_seed_rules;
        let error = dialog.error.clone();
        let seed_ratio_input = seed_ratio_input.clone();
        let seed_time_input = seed_time_input.clone();
        div()
            .flex()
            .flex_col()
            .gap_3()
            .child(
                div()
                    .text_sm()
                    .text_color(colors.text_secondary)
                    .child(self.t_args(
                        "dialog-task-options-description",
                        &[("name", FluentValue::from(display_name))],
                    )),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(colors.text_muted)
                    .child(if supports_seed_rules {
                        self.t("dialog-task-options-seed-help")
                    } else {
                        self.t("dialog-task-options-seed-unavailable")
                    }),
            )
            .when(supports_seed_rules, |element| {
                element.child(
                    div()
                        .flex()
                        .gap_3()
                        .child(
                            settings_labeled_input(
                                self.t("dialog-task-options-seed-ratio"),
                                &seed_ratio_input,
                                colors,
                            )
                            .flex_1()
                            .min_w_0(),
                        )
                        .child(
                            settings_labeled_input(
                                self.t("dialog-task-options-seed-time"),
                                &seed_time_input,
                                colors,
                            )
                            .flex_1()
                            .min_w_0(),
                        ),
                )
            })
            .when_some(error, |element, error| {
                element.child(
                    div()
                        .id("task-options-error")
                        .role(Role::Alert)
                        .aria_label(self.te(&error))
                        .text_xs()
                        .text_color(colors.danger)
                        .child(self.te(&error)),
                )
            })
            .into_any_element()
    }
}
