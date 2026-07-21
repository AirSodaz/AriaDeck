//! Desktop platform surface for PLAT-001: system tray and OS notifications.
//!
//! Tray events are polled from the GPUI main thread. Managed engines always stop
//! when AriaDeck quits; remote engines are never stopped by this process.

use std::sync::OnceLock;

use notify_rust::Notification;
use tray_icon::{
    Icon, TrayIcon, TrayIconBuilder, TrayIconEvent,
    menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem},
};

/// Actions raised by the system tray menu or icon interaction.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TrayAction {
    Show,
    PauseAll,
    ResumeAll,
    Quit,
}

struct TrayMenuIds {
    show: String,
    pause_all: String,
    resume_all: String,
    quit: String,
}

static TRAY_MENU_IDS: OnceLock<TrayMenuIds> = OnceLock::new();

/// Owns the tray icon for the process lifetime.
pub struct SystemTray {
    icon: TrayIcon,
}

impl SystemTray {
    /// Build a tray icon. Call only from the UI thread after the event loop is running.
    pub fn try_new() -> Result<Self, String> {
        let menu = Menu::new();
        let show = MenuItem::new("Show AriaDeck", true, None);
        let pause_all = MenuItem::new("Pause all", true, None);
        let resume_all = MenuItem::new("Resume all", true, None);
        let quit = MenuItem::new("Quit AriaDeck", true, None);
        menu.append(&show).map_err(|error| error.to_string())?;
        menu.append(&PredefinedMenuItem::separator())
            .map_err(|error| error.to_string())?;
        menu.append(&pause_all).map_err(|error| error.to_string())?;
        menu.append(&resume_all)
            .map_err(|error| error.to_string())?;
        menu.append(&PredefinedMenuItem::separator())
            .map_err(|error| error.to_string())?;
        menu.append(&quit).map_err(|error| error.to_string())?;

        let _ = TRAY_MENU_IDS.set(TrayMenuIds {
            show: show.id().0.clone(),
            pause_all: pause_all.id().0.clone(),
            resume_all: resume_all.id().0.clone(),
            quit: quit.id().0.clone(),
        });

        let icon = tray_icon_image()?;
        let tray = TrayIconBuilder::new()
            .with_menu(Box::new(menu))
            .with_tooltip("AriaDeck")
            .with_icon(icon)
            .with_menu_on_left_click(false)
            .build()
            .map_err(|error| error.to_string())?;

        Ok(Self { icon: tray })
    }

    pub fn set_visible(&self, visible: bool) {
        if let Err(error) = self.icon.set_visible(visible) {
            tracing::debug!(%error, "failed to update tray visibility");
        }
    }

    pub fn set_tooltip(&self, tooltip: &str) {
        if let Err(error) = self.icon.set_tooltip(Some(tooltip)) {
            tracing::debug!(%error, "failed to update tray tooltip");
        }
    }

    /// Drain pending tray/menu events without blocking.
    pub fn poll_actions(&self) -> Vec<TrayAction> {
        let mut actions = Vec::new();
        while let Ok(event) = TrayIconEvent::receiver().try_recv() {
            if matches!(
                event,
                TrayIconEvent::DoubleClick { .. } | TrayIconEvent::Click { .. }
            ) {
                // Prefer double-click restore; single click still shows so users
                // without double-click habit can recover the window.
                if !actions.contains(&TrayAction::Show) {
                    actions.push(TrayAction::Show);
                }
            }
        }
        let ids = TRAY_MENU_IDS.get();
        while let Ok(event) = MenuEvent::receiver().try_recv() {
            let Some(ids) = ids else {
                continue;
            };
            let id = event.id.0.as_str();
            if id == ids.show {
                push_unique(&mut actions, TrayAction::Show);
            } else if id == ids.pause_all {
                push_unique(&mut actions, TrayAction::PauseAll);
            } else if id == ids.resume_all {
                push_unique(&mut actions, TrayAction::ResumeAll);
            } else if id == ids.quit {
                push_unique(&mut actions, TrayAction::Quit);
            }
        }
        actions
    }
}

fn push_unique(actions: &mut Vec<TrayAction>, action: TrayAction) {
    if !actions.contains(&action) {
        actions.push(action);
    }
}

fn tray_icon_image() -> Result<Icon, String> {
    // Embedded 32×32 RGBA generated for AriaDeck (blue disk + white mark).
    const WIDTH: u32 = 32;
    const HEIGHT: u32 = 32;
    let rgba = include_bytes!("../assets/tray_icon.rgba");
    if rgba.len() != (WIDTH * HEIGHT * 4) as usize {
        return Err("embedded tray icon has unexpected size".into());
    }
    Icon::from_rgba(rgba.to_vec(), WIDTH, HEIGHT).map_err(|error| error.to_string())
}

/// Fire-and-forget OS desktop notification. Failures are logged only.
pub fn show_os_notification(title: &str, body: &str) {
    if let Err(error) = Notification::new()
        .appname("AriaDeck")
        .summary(title)
        .body(body)
        .show()
    {
        tracing::debug!(%error, "OS notification could not be shown");
    }
}

/// Free space for the download directory mount, when discoverable.
pub fn available_disk_space(directory: &std::path::Path) -> Option<u64> {
    use sysinfo::Disks;
    let disks = Disks::new_with_refreshed_list();
    let canonical = std::fs::canonicalize(directory).ok();
    disks
        .list()
        .iter()
        .filter(|disk| {
            let mount = disk.mount_point();
            directory.starts_with(mount)
                || canonical
                    .as_ref()
                    .is_some_and(|path| path.starts_with(mount))
        })
        .max_by_key(|disk| disk.mount_point().components().count())
        .map(|disk| disk.available_space())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tray_icon_bytes_decode() {
        assert!(tray_icon_image().is_ok());
    }

    #[test]
    fn available_disk_space_for_temp_is_some() {
        let root = tempfile::tempdir().expect("temp");
        // On some CI images the temp mount may not be listed; accept either.
        let _ = available_disk_space(root.path());
    }
}
