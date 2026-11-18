//! Settings `updates` section logic and rendering for `AppShell`.

use super::*;

impl AppShell {
    pub fn configure_updates(&mut self, enabled: bool, portable: bool, cx: &mut Context<Self>) {
        self.update_enabled = enabled;
        self.update_portable = portable;
        self.update_status = if enabled {
            crate::UpdateStatusView::Checking
        } else {
            crate::UpdateStatusView::Disabled
        };
        cx.notify();
    }

    pub fn request_update_check(&mut self, cx: &mut Context<Self>) {
        if !self.update_enabled
            || self.pending_update_check.is_some()
            || self.pending_update_download.is_some()
            || matches!(
                self.update_status,
                crate::UpdateStatusView::Downloaded { .. }
            )
        {
            return;
        }
        let request_id = self.allocate_request_id();
        self.pending_update_check = Some(request_id);
        self.update_status = crate::UpdateStatusView::Checking;
        cx.emit(AppShellEvent::CheckForUpdatesRequested { request_id });
        cx.notify();
    }

    pub fn set_update_check_result(
        &mut self,
        request_id: RequestId,
        result: Result<Option<crate::UpdateOfferView>, String>,
        cx: &mut Context<Self>,
    ) {
        if self.pending_update_check != Some(request_id) {
            return;
        }
        self.pending_update_check = None;
        self.update_status = match result {
            Ok(None) => crate::UpdateStatusView::Current,
            Ok(Some(offer)) => crate::UpdateStatusView::Available {
                version: offer.version,
                title: offer.title,
                notes: offer.notes,
                release_url: offer.release_url,
                installer_available: offer.installer_available,
            },
            Err(error) => crate::UpdateStatusView::Failed(error),
        };
        cx.notify();
    }

    pub fn request_update_download(&mut self, cx: &mut Context<Self>) {
        let crate::UpdateStatusView::Available {
            version,
            installer_available: true,
            ..
        } = &self.update_status
        else {
            return;
        };
        if self.update_portable || self.pending_update_download.is_some() {
            return;
        }
        let version = version.clone();
        let request_id = self.allocate_request_id();
        self.pending_update_download = Some(request_id);
        self.update_status = crate::UpdateStatusView::Downloading {
            version,
            downloaded_bytes: 0,
            total_bytes: None,
        };
        cx.emit(AppShellEvent::DownloadUpdateRequested { request_id });
        cx.notify();
    }

    pub fn set_update_download_progress(
        &mut self,
        request_id: RequestId,
        downloaded_bytes: u64,
        total_bytes: Option<u64>,
        cx: &mut Context<Self>,
    ) {
        if self.pending_update_download != Some(request_id) {
            return;
        }
        if let crate::UpdateStatusView::Downloading { version, .. } = &self.update_status {
            let version = version.clone();
            self.update_status = crate::UpdateStatusView::Downloading {
                version,
                downloaded_bytes,
                total_bytes,
            };
            cx.notify();
        }
    }

    pub fn set_update_download_result(
        &mut self,
        request_id: RequestId,
        result: Result<String, String>,
        cx: &mut Context<Self>,
    ) {
        if self.pending_update_download != Some(request_id) {
            return;
        }
        self.pending_update_download = None;
        self.update_status = match result {
            Ok(version) => crate::UpdateStatusView::Downloaded { version },
            Err(error) => crate::UpdateStatusView::Failed(error),
        };
        cx.notify();
    }

    pub fn request_update_install(&mut self, cx: &mut Context<Self>) {
        if self.update_portable
            || !matches!(
                self.update_status,
                crate::UpdateStatusView::Downloaded { .. }
            )
        {
            return;
        }
        cx.emit(AppShellEvent::InstallUpdateRequested);
    }

    pub fn set_update_install_error(&mut self, error: String, cx: &mut Context<Self>) {
        self.update_status = crate::UpdateStatusView::Failed(error);
        cx.notify();
    }
}
