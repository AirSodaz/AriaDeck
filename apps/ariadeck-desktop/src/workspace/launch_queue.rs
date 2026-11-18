//! Single-instance launch request polling and bounded launch queues
//! (metadata files, magnet links, and browser-bridge downloads).

use std::{path::PathBuf, sync::mpsc::Receiver};

use ariadeck_ipc::{BridgeDownload, LaunchRequest, MAX_LAUNCH_ITEMS};
use ariadeck_ui::{AppShell, BridgeDownloadView, SecretStringView};
use gpui::{App, Entity, Window};
use secrecy::ExposeSecret as _;

pub(crate) struct LaunchQueue {
    pending_metadata_paths: Vec<PathBuf>,
    pending_magnet_uris: Vec<String>,
    pending_downloads: Vec<BridgeDownload>,
    instance_requests: Option<Receiver<LaunchRequest>>,
}

impl LaunchQueue {
    pub(crate) fn new(
        initial_request: LaunchRequest,
        instance_requests: Option<Receiver<LaunchRequest>>,
    ) -> Self {
        let mut queue = Self {
            pending_metadata_paths: Vec::new(),
            pending_magnet_uris: Vec::new(),
            pending_downloads: Vec::new(),
            instance_requests,
        };
        queue.enqueue(initial_request);
        queue
    }

    /// Remaining slots before the shared launch-item flood cap is reached.
    fn budget(&self) -> usize {
        MAX_LAUNCH_ITEMS.saturating_sub(
            self.pending_metadata_paths.len()
                + self.pending_magnet_uris.len()
                + self.pending_downloads.len(),
        )
    }

    fn enqueue(&mut self, request: LaunchRequest) {
        let remaining = self.budget();
        self.pending_metadata_paths
            .extend(request.metadata_paths.into_iter().take(remaining));
        let remaining = self.budget();
        self.pending_magnet_uris
            .extend(request.magnet_uris.into_iter().take(remaining));
        let remaining = self.budget();
        self.pending_downloads
            .extend(request.downloads.into_iter().take(remaining));
    }

    /// Whether the queued startup items will stop to ask the user for confirmation.
    pub(crate) fn needs_window_for_confirmation(&self, bridge_requires_confirmation: bool) -> bool {
        !self.pending_metadata_paths.is_empty()
            || !self.pending_magnet_uris.is_empty()
            || (!self.pending_downloads.is_empty() && bridge_requires_confirmation)
    }

    /// Drain incoming IPC requests and return `true` if the window should be
    /// raised from tray before dispatching queued items into `AppShell`.
    pub(crate) fn drain_incoming(&mut self, bridge_requires_confirmation: bool) -> bool {
        let mut activate = false;
        let incoming: Vec<LaunchRequest> = self
            .instance_requests
            .as_ref()
            .map(|receiver| receiver.try_iter().collect())
            .unwrap_or_default();
        for request in incoming {
            activate |= !request.metadata_paths.is_empty()
                || !request.magnet_uris.is_empty()
                || (!request.downloads.is_empty() && bridge_requires_confirmation);
            self.enqueue(request);
        }
        activate
    }

    /// Hand ready launch items to `AppShell` when the shell can accept them.
    pub(crate) fn dispatch_ready(
        &mut self,
        workspace: &Entity<AppShell>,
        window: &mut Window,
        cx: &mut App,
    ) {
        if !self.pending_metadata_paths.is_empty() && workspace.read(cx).can_open_metadata_paths() {
            let paths = std::mem::take(&mut self.pending_metadata_paths);
            let opened = workspace.update(cx, |workspace, cx| {
                workspace.open_metadata_paths(paths.clone(), window, cx)
            });
            if !opened {
                self.pending_metadata_paths = paths;
            }
        }

        if !self.pending_magnet_uris.is_empty() && workspace.read(cx).can_open_magnet_uris() {
            let uris = std::mem::take(&mut self.pending_magnet_uris);
            let opened = workspace.update(cx, |workspace, cx| {
                workspace.open_magnet_uris(uris.clone(), window, cx)
            });
            if !opened {
                self.pending_magnet_uris = uris;
            }
        }

        // D-045: bridge downloads carry per-item headers, but the add dialog holds a
        // single shared advanced-options block, so only the leading run of items that
        // agree on headers is consumed per dialog. The rest stays queued.
        if !self.pending_downloads.is_empty() && workspace.read(cx).can_open_bridge_downloads() {
            let views: Vec<BridgeDownloadView> =
                self.pending_downloads.iter().map(bridge_view).collect();
            let consumed = workspace.update(cx, |workspace, cx| {
                workspace.accept_bridge_downloads(&views, window, cx)
            });
            if consumed > 0 {
                self.pending_downloads.drain(..consumed);
            }
        }
    }
}

/// Map a validated bridge payload onto the add-dialog view model (D-045).
///
/// `filename` stays a display hint only and is never mapped onto `out` (D-001).
fn bridge_view(download: &BridgeDownload) -> BridgeDownloadView {
    BridgeDownloadView {
        url: download.url.clone(),
        referer: download.referer.clone().unwrap_or_default(),
        user_agent: download.user_agent.clone().unwrap_or_default(),
        cookie: download
            .cookie
            .as_ref()
            .map(|cookie| SecretStringView::new(cookie.expose_secret())),
        filename: download.filename.clone(),
        file_size: download.file_size,
    }
}
