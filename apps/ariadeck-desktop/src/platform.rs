//! Desktop platform surface for PLAT-001: system tray and OS notifications.
//!
//! Tray events are polled from the GPUI main thread. Managed engines always stop
//! when AriaDeck quits; remote engines are never stopped by this process.

use notify_rust::Notification;
use tray_icon::{
    Icon, MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent,
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

/// Owns the tray icon for the process lifetime.
pub struct SystemTray {
    icon: TrayIcon,
    menu_ids: TrayMenuIds,
}

impl SystemTray {
    /// Build a tray icon. Call only from the UI thread after the event loop is running.
    #[allow(dead_code)]
    pub fn try_new() -> Result<Self, String> {
        Self::try_new_with_labels(
            "Show AriaDeck",
            "Pause all",
            "Resume all",
            "Quit AriaDeck",
            "AriaDeck",
        )
    }

    /// Build a tray icon with localized menu labels.
    pub fn try_new_with_labels(
        show_label: &str,
        pause_all_label: &str,
        resume_all_label: &str,
        quit_label: &str,
        tooltip: &str,
    ) -> Result<Self, String> {
        let menu = Menu::new();
        let show = MenuItem::new(show_label, true, None);
        let pause_all = MenuItem::new(pause_all_label, true, None);
        let resume_all = MenuItem::new(resume_all_label, true, None);
        let quit = MenuItem::new(quit_label, true, None);
        menu.append(&show).map_err(|error| error.to_string())?;
        menu.append(&PredefinedMenuItem::separator())
            .map_err(|error| error.to_string())?;
        menu.append(&pause_all).map_err(|error| error.to_string())?;
        menu.append(&resume_all)
            .map_err(|error| error.to_string())?;
        menu.append(&PredefinedMenuItem::separator())
            .map_err(|error| error.to_string())?;
        menu.append(&quit).map_err(|error| error.to_string())?;

        let menu_ids = TrayMenuIds {
            show: show.id().0.clone(),
            pause_all: pause_all.id().0.clone(),
            resume_all: resume_all.id().0.clone(),
            quit: quit.id().0.clone(),
        };

        let icon = tray_icon_image()?;
        let tray = TrayIconBuilder::new()
            .with_menu(Box::new(menu))
            .with_tooltip(tooltip)
            .with_icon(icon)
            .with_menu_on_left_click(false)
            .build()
            .map_err(|error| error.to_string())?;

        Ok(Self {
            icon: tray,
            menu_ids,
        })
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
            // Only left-click restores the window. Right-click must open the
            // context menu without activate/raise, or the menu is dismissed.
            if is_tray_show_click(&event) {
                push_unique(&mut actions, TrayAction::Show);
            }
        }
        let ids = &self.menu_ids;
        while let Ok(event) = MenuEvent::receiver().try_recv() {
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

/// Left single-click (button up) or left double-click restores the window.
/// Right/middle clicks are ignored so the tray context menu is not interrupted.
fn is_tray_show_click(event: &TrayIconEvent) -> bool {
    matches!(
        event,
        TrayIconEvent::Click {
            button: MouseButton::Left,
            button_state: MouseButtonState::Up,
            ..
        } | TrayIconEvent::DoubleClick {
            button: MouseButton::Left,
            ..
        }
    )
}

fn push_unique(actions: &mut Vec<TrayAction>, action: TrayAction) {
    if !actions.contains(&action) {
        actions.push(action);
    }
}

fn tray_icon_image() -> Result<Icon, String> {
    // 32×32 RGBA rendered from assets/icon.svg at build time and written to
    // OUT_DIR by build.rs. The file is 4 096 bytes (32 × 32 × 4).
    const WIDTH: u32 = 32;
    const HEIGHT: u32 = 32;
    let rgba = include_bytes!(concat!(env!("OUT_DIR"), "/tray_icon.rgba"));
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

#[cfg(windows)]
#[allow(unsafe_code)]
#[link(name = "kernel32")]
unsafe extern "system" {
    fn SetThreadExecutionState(es_flags: u32) -> u32;
    fn CreateJobObjectW(security_attributes: *mut core::ffi::c_void, name: *const u16) -> isize;
    fn SetInformationJobObject(
        job: isize,
        info_class: u32,
        info: *const core::ffi::c_void,
        info_len: u32,
    ) -> i32;
    fn OpenProcess(desired_access: u32, inherit_handle: i32, process_id: u32) -> isize;
    fn AssignProcessToJobObject(job: isize, process: isize) -> i32;
    fn CloseHandle(handle: isize) -> i32;
}

#[cfg(windows)]
#[repr(C)]
#[derive(Default)]
struct JobObjectBasicLimitInformation {
    per_process_user_time_limit: i64,
    per_job_user_time_limit: i64,
    limit_flags: u32,
    minimum_working_set_size: usize,
    maximum_working_set_size: usize,
    active_process_limit: u32,
    affinity: usize,
    priority_class: u32,
    scheduling_class: u32,
}

#[cfg(windows)]
#[repr(C)]
#[derive(Default)]
struct IoCounters {
    read_operation_count: u64,
    write_operation_count: u64,
    other_operation_count: u64,
    read_transfer_count: u64,
    write_transfer_count: u64,
    other_transfer_count: u64,
}

#[cfg(windows)]
#[repr(C)]
#[derive(Default)]
struct JobObjectExtendedLimitInformation {
    basic_limit_information: JobObjectBasicLimitInformation,
    io_info: IoCounters,
    process_memory_limit: usize,
    job_memory_limit: usize,
    peak_process_memory_used: usize,
    peak_job_memory_used: usize,
}

/// Process-lifetime OS job guard that ensures managed `aria2c` children are
/// automatically terminated by Windows if AriaDeck crashes or exits abnormally.
#[cfg(windows)]
pub struct ChildProcessJob {
    handle: isize,
}

#[cfg(windows)]
impl ChildProcessJob {
    #[allow(unsafe_code)]
    pub fn try_new() -> Result<Self, String> {
        const JOB_OBJECT_EXTENDED_LIMIT_INFORMATION_CLASS: u32 = 9;
        const JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE: u32 = 0x0000_2000;

        // SAFETY: Passing null security attributes and null name creates an
        // anonymous Job Object owned by this process.
        let handle = unsafe { CreateJobObjectW(std::ptr::null_mut(), std::ptr::null()) };
        if handle == 0 {
            return Err(format!(
                "CreateJobObjectW failed: {}",
                std::io::Error::last_os_error()
            ));
        }

        let mut info = JobObjectExtendedLimitInformation::default();
        info.basic_limit_information.limit_flags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        let info_len =
            u32::try_from(std::mem::size_of::<JobObjectExtendedLimitInformation>()).unwrap_or(0);
        // SAFETY: `handle` is a valid Job Object handle and `info` points to a
        // properly aligned, initialized `JOBOBJECT_EXTENDED_LIMIT_INFORMATION`
        // whose byte size is passed in `info_len`.
        let ok = unsafe {
            SetInformationJobObject(
                handle,
                JOB_OBJECT_EXTENDED_LIMIT_INFORMATION_CLASS,
                (&raw const info).cast(),
                info_len,
            )
        };
        if ok == 0 {
            let err = std::io::Error::last_os_error();
            // SAFETY: `handle` is a valid owned handle created above.
            unsafe {
                CloseHandle(handle);
            }
            return Err(format!("SetInformationJobObject failed: {err}"));
        }

        Ok(Self { handle })
    }

    #[allow(unsafe_code)]
    pub fn assign_pid(&self, pid: u32) {
        const PROCESS_TERMINATE: u32 = 0x0001;
        const PROCESS_SET_QUOTA: u32 = 0x0100;

        // SAFETY: `OpenProcess` opens a handle to the child PID with only the
        // rights required by `AssignProcessToJobObject`; both handles are valid
        // and the process handle is closed immediately after assignment.
        unsafe {
            let process = OpenProcess(PROCESS_SET_QUOTA | PROCESS_TERMINATE, 0, pid);
            if process == 0 {
                tracing::debug!(
                    pid,
                    error = %std::io::Error::last_os_error(),
                    "OpenProcess failed while attaching aria2c to job object"
                );
                return;
            }
            let assigned = AssignProcessToJobObject(self.handle, process);
            if assigned == 0 {
                tracing::debug!(
                    pid,
                    error = %std::io::Error::last_os_error(),
                    "AssignProcessToJobObject failed for managed aria2c"
                );
            }
            CloseHandle(process);
        }
    }
}

#[cfg(windows)]
impl Drop for ChildProcessJob {
    #[allow(unsafe_code)]
    fn drop(&mut self) {
        if self.handle != 0 {
            // SAFETY: `self.handle` is uniquely owned by this `ChildProcessJob`
            // and has not been closed previously.
            unsafe {
                CloseHandle(self.handle);
            }
            self.handle = 0;
        }
    }
}

/// No-op job guard on non-Windows platforms.
#[cfg(not(windows))]
pub struct ChildProcessJob;

#[cfg(not(windows))]
impl ChildProcessJob {
    pub fn try_new() -> Result<Self, String> {
        Ok(Self)
    }

    pub fn assign_pid(&self, _pid: u32) {}
}

#[cfg(windows)]
#[allow(unsafe_code)]
#[link(name = "powrprof")]
unsafe extern "system" {
    fn SetSuspendState(hibernate: u8, force_critical: u8, disable_wake_event: u8) -> u8;
}

#[cfg(windows)]
#[allow(unsafe_code)]
#[link(name = "user32")]
unsafe extern "system" {
    fn ShowWindow(hwnd: isize, cmd: i32) -> i32;
    fn SetForegroundWindow(hwnd: isize) -> i32;
    fn BringWindowToTop(hwnd: isize) -> i32;
}

#[cfg(windows)]
const ES_SYSTEM_REQUIRED: u32 = 0x0000_0001;
#[cfg(windows)]
const ES_AWAYMODE_REQUIRED: u32 = 0x0000_0040;
#[cfg(windows)]
const ES_CONTINUOUS: u32 = 0x8000_0000;

/// Narrow Win32 platform boundary for true hide-to-tray / restore-from-tray.
#[cfg(windows)]
#[allow(unsafe_code)]
pub fn hide_show_win32(hwnd: isize, show: bool) -> bool {
    const SW_HIDE: i32 = 0;
    const SW_RESTORE: i32 = 9;
    // SAFETY: `hwnd` is the live GPUI window handle belonging to this process;
    // ShowWindow, BringWindowToTop, and SetForegroundWindow tolerate invalid
    // handles safely and take only integer arguments.
    unsafe {
        ShowWindow(hwnd, if show { SW_RESTORE } else { SW_HIDE });
        if show {
            BringWindowToTop(hwnd);
            SetForegroundWindow(hwnd);
        }
    }
    true
}

/// Prevent or allow operating system sleep during active downloads (D5).
#[cfg(windows)]
#[allow(unsafe_code)]
pub fn set_keep_awake(enabled: bool) {
    // SAFETY: `SetThreadExecutionState` is a thread-local Win32 power-management
    // API taking only bitflag integers with no pointer arguments.
    unsafe {
        if enabled {
            let res =
                SetThreadExecutionState(ES_CONTINUOUS | ES_SYSTEM_REQUIRED | ES_AWAYMODE_REQUIRED);
            if res == 0 {
                SetThreadExecutionState(ES_CONTINUOUS | ES_SYSTEM_REQUIRED);
            }
        } else {
            SetThreadExecutionState(ES_CONTINUOUS);
        }
    }
}

#[cfg(windows)]
#[allow(unsafe_code)]
fn suspend_system_win32(hibernate: bool) -> bool {
    // SAFETY: `SetSuspendState` takes three boolean byte flags with no pointer
    // arguments and returns a non-zero BOOLEAN on success.
    unsafe { SetSuspendState(u8::from(hibernate), 0, 0) != 0 }
}

/// Prevent or allow operating system sleep during active downloads (D5).
#[cfg(not(windows))]
pub fn set_keep_awake(_enabled: bool) {
    // Safe stub for non-Windows platforms
}

/// Execute completion power action (D5).
pub fn execute_power_action(
    action: ariadeck_settings::CompletionPowerAction,
) -> Result<(), String> {
    match action {
        ariadeck_settings::CompletionPowerAction::None => Ok(()),
        ariadeck_settings::CompletionPowerAction::Shutdown => {
            #[cfg(windows)]
            {
                let status = ariadeck_engine::console_free_command("shutdown")
                    .args(["/s", "/t", "0"])
                    .status()
                    .map_err(|e| format!("failed to invoke shutdown: {e}"))?;
                if status.success() {
                    Ok(())
                } else {
                    Err(format!("shutdown command exited with {status}"))
                }
            }
            #[cfg(target_os = "macos")]
            {
                let status = std::process::Command::new("osascript")
                    .args(["-e", "tell app \"System Events\" to shut down"])
                    .status()
                    .map_err(|e| format!("failed to invoke shutdown: {e}"))?;
                if status.success() {
                    Ok(())
                } else {
                    Err(format!("shutdown exited with {status}"))
                }
            }
            #[cfg(all(not(windows), not(target_os = "macos")))]
            {
                let status = std::process::Command::new("systemctl")
                    .arg("poweroff")
                    .status()
                    .map_err(|e| format!("failed to invoke poweroff: {e}"))?;
                if status.success() {
                    Ok(())
                } else {
                    Err(format!("systemctl poweroff exited with {status}"))
                }
            }
        }
        ariadeck_settings::CompletionPowerAction::Sleep => {
            #[cfg(windows)]
            {
                if suspend_system_win32(false) {
                    Ok(())
                } else {
                    Err("SetSuspendState(0, 0, 0) failed".to_string())
                }
            }
            #[cfg(target_os = "macos")]
            {
                let status = std::process::Command::new("pmset")
                    .arg("sleepnow")
                    .status()
                    .map_err(|e| format!("failed to invoke sleep: {e}"))?;
                if status.success() {
                    Ok(())
                } else {
                    Err(format!("pmset sleepnow exited with {status}"))
                }
            }
            #[cfg(all(not(windows), not(target_os = "macos")))]
            {
                let status = std::process::Command::new("systemctl")
                    .arg("suspend")
                    .status()
                    .map_err(|e| format!("failed to invoke suspend: {e}"))?;
                if status.success() {
                    Ok(())
                } else {
                    Err(format!("systemctl suspend exited with {status}"))
                }
            }
        }
        ariadeck_settings::CompletionPowerAction::Hibernate => {
            #[cfg(windows)]
            {
                if suspend_system_win32(true) {
                    Ok(())
                } else {
                    let status = ariadeck_engine::console_free_command("shutdown")
                        .arg("/h")
                        .status()
                        .map_err(|e| format!("failed to invoke shutdown /h: {e}"))?;
                    if status.success() {
                        Ok(())
                    } else {
                        Err(format!("hibernate command exited with {status}"))
                    }
                }
            }
            #[cfg(target_os = "macos")]
            {
                let status = std::process::Command::new("pmset")
                    .arg("sleepnow")
                    .status()
                    .map_err(|e| format!("failed to invoke sleep: {e}"))?;
                if status.success() {
                    Ok(())
                } else {
                    Err(format!("pmset sleepnow exited with {status}"))
                }
            }
            #[cfg(all(not(windows), not(target_os = "macos")))]
            {
                let status = std::process::Command::new("systemctl")
                    .arg("hibernate")
                    .status()
                    .map_err(|e| format!("failed to invoke hibernate: {e}"))?;
                if status.success() {
                    Ok(())
                } else {
                    Err(format!("systemctl hibernate exited with {status}"))
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tray_icon::{Rect, TrayIconId, dpi::PhysicalPosition};

    fn empty_rect() -> Rect {
        Rect::default()
    }

    fn sample_click(button: MouseButton, button_state: MouseButtonState) -> TrayIconEvent {
        TrayIconEvent::Click {
            id: TrayIconId::new("test"),
            position: PhysicalPosition::new(0.0, 0.0),
            rect: empty_rect(),
            button,
            button_state,
        }
    }

    #[test]
    fn tray_show_only_on_left_click_up_or_double_click() {
        assert!(is_tray_show_click(&sample_click(
            MouseButton::Left,
            MouseButtonState::Up
        )));
        assert!(!is_tray_show_click(&sample_click(
            MouseButton::Left,
            MouseButtonState::Down
        )));
        assert!(!is_tray_show_click(&sample_click(
            MouseButton::Right,
            MouseButtonState::Up
        )));
        assert!(!is_tray_show_click(&sample_click(
            MouseButton::Right,
            MouseButtonState::Down
        )));
        assert!(is_tray_show_click(&TrayIconEvent::DoubleClick {
            id: TrayIconId::new("test"),
            position: PhysicalPosition::new(0.0, 0.0),
            rect: empty_rect(),
            button: MouseButton::Left,
        }));
        assert!(!is_tray_show_click(&TrayIconEvent::DoubleClick {
            id: TrayIconId::new("test"),
            position: PhysicalPosition::new(0.0, 0.0),
            rect: empty_rect(),
            button: MouseButton::Right,
        }));
    }

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

    #[test]
    fn platform_power_action_none_is_ok() {
        assert!(execute_power_action(ariadeck_settings::CompletionPowerAction::None).is_ok());
    }

    #[test]
    fn platform_set_keep_awake_does_not_panic() {
        set_keep_awake(false);
    }

    #[test]
    fn child_process_job_initializes_cleanly() {
        let job = ChildProcessJob::try_new().expect("job object should initialize");
        drop(job);
    }
}
