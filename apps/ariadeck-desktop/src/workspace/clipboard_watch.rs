//! Opt-in clipboard link monitoring state machine for [`super::DesktopRoot`].

use ariadeck_domain::extract_clipboard_download_uris;

/// Tracks clipboard changes on an 800 ms cadence (every second 400 ms platform
/// poll tick) while avoiding baseline re-triggers, self-copy loops, and modal
/// clobbering.
pub(crate) struct ClipboardWatchController {
    enabled: bool,
    initialized: bool,
    last_seen_text: Option<String>,
    poll_phase: u8,
}

impl ClipboardWatchController {
    pub(crate) fn new(enabled: bool) -> Self {
        Self {
            enabled,
            initialized: false,
            last_seen_text: None,
            poll_phase: 0,
        }
    }

    pub(crate) fn sync_enabled(&mut self, enabled: bool) {
        if self.enabled == enabled {
            return;
        }
        self.enabled = enabled;
        self.initialized = false;
        self.last_seen_text = None;
        self.poll_phase = 0;
    }

    /// Record text copied by AriaDeck itself so the next poll does not treat it
    /// as a newly copied external download link.
    pub(crate) fn suppress(&mut self, text: String) {
        self.last_seen_text = Some(text);
        if self.enabled {
            self.initialized = true;
        }
    }

    /// Returns `true` on every second 400 ms platform tick (~800 ms) while
    /// monitoring is enabled. Always returns `true` on the initial uninitialized
    /// tick so the current clipboard baseline is captured promptly.
    pub(crate) fn should_poll_now(&mut self) -> bool {
        if !self.enabled {
            return false;
        }
        if !self.initialized {
            self.poll_phase = 0;
            return true;
        }
        self.poll_phase = self.poll_phase.wrapping_add(1) % 2;
        self.poll_phase == 0
    }

    pub(crate) fn poll_step(
        &mut self,
        current_text: Option<String>,
        can_open_dialog: bool,
        extra_extensions: &[&str],
    ) -> Option<Vec<String>> {
        if !self.enabled {
            return None;
        }
        if !self.initialized {
            self.last_seen_text = current_text;
            self.initialized = true;
            return None;
        }
        if current_text == self.last_seen_text {
            return None;
        }
        self.last_seen_text = current_text.clone();
        if !can_open_dialog {
            return None;
        }
        let text = current_text?;
        let uris = extract_clipboard_download_uris(&text, extra_extensions);
        (!uris.is_empty()).then_some(uris)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn poll_cadence_skips_when_disabled_and_alternates_after_baseline() {
        let mut disabled = ClipboardWatchController::new(false);
        assert!(!disabled.should_poll_now());
        assert!(!disabled.should_poll_now());

        let mut enabled = ClipboardWatchController::new(true);
        // Uninitialized tick always polls immediately to capture the baseline.
        assert!(enabled.should_poll_now());
        assert_eq!(enabled.poll_step(None, true, &[]), None);
        // Subsequent ticks alternate on every second 400 ms platform poll (~800 ms).
        assert!(!enabled.should_poll_now());
        assert!(enabled.should_poll_now());
        assert!(!enabled.should_poll_now());
        assert!(enabled.should_poll_now());
    }

    #[test]
    fn baseline_is_ignored_and_subsequent_new_link_triggers_once() {
        let mut controller = ClipboardWatchController::new(true);

        assert_eq!(
            controller.poll_step(Some("https://cdn.example.test/old.zip".into()), true, &[]),
            None
        );
        assert_eq!(
            controller.poll_step(Some("https://cdn.example.test/old.zip".into()), true, &[]),
            None
        );
        assert_eq!(
            controller.poll_step(Some("https://cdn.example.test/new.zip".into()), true, &[]),
            Some(vec!["https://cdn.example.test/new.zip".to_owned()])
        );
        assert_eq!(
            controller.poll_step(Some("https://cdn.example.test/new.zip".into()), true, &[]),
            None
        );
    }

    #[test]
    fn toggling_enabled_reestablishes_baseline_without_triggering_stale_link() {
        let mut controller = ClipboardWatchController::new(false);
        assert_eq!(
            controller.poll_step(Some("https://cdn.example.test/a.zip".into()), true, &[]),
            None
        );

        controller.sync_enabled(true);
        assert_eq!(
            controller.poll_step(Some("https://cdn.example.test/a.zip".into()), true, &[]),
            None
        );
        assert_eq!(
            controller.poll_step(Some("https://cdn.example.test/b.iso".into()), true, &[]),
            Some(vec!["https://cdn.example.test/b.iso".to_owned()])
        );
    }

    #[test]
    fn self_copy_and_busy_modal_copies_do_not_trigger_dialog() {
        let mut controller = ClipboardWatchController::new(true);
        assert_eq!(controller.poll_step(None, true, &[]), None);

        controller.suppress("https://cdn.example.test/copied.zip".into());
        assert_eq!(
            controller.poll_step(
                Some("https://cdn.example.test/copied.zip".into()),
                true,
                &[]
            ),
            None
        );

        // Copying while Settings or another modal is open marks it seen without ambushing on close.
        assert_eq!(
            controller.poll_step(
                Some("https://cdn.example.test/while-busy.zip".into()),
                false,
                &[],
            ),
            None
        );
        assert_eq!(
            controller.poll_step(
                Some("https://cdn.example.test/while-busy.zip".into()),
                true,
                &[],
            ),
            None
        );
    }
}
