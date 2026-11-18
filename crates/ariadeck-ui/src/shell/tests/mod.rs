use gpui::{TestAppContext, px};

use super::*;
use crate::{
    AddDownloadMetadataFileView, AddDownloadMetadataPreviewItemView, CoreDiscoveryOriginView,
    CoreInstallStatusView, CoreInstallationView, CoreSourceView, TaskCountsView, TaskNameStateView,
    TaskSourceKindView, TaskStatusView,
};

fn secondary_modifier() -> &'static str {
    if cfg!(target_os = "macos") {
        "cmd"
    } else {
        "ctrl"
    }
}

/// Open an [`AppShell`] inside a [`gpui_kit::component::Root`] matching production
/// `gpui_kit::open_window`, so `WindowExt` dialogs, sheets, and notifications work.
#[allow(dead_code)]
fn open_test_shell(
    cx: &mut TestAppContext,
    build: impl FnOnce(&mut Window, &mut Context<AppShell>) -> AppShell + 'static,
) -> (Entity<AppShell>, gpui::VisualTestContext) {
    open_test_shell_sized(cx, gpui::size(px(1280.0), px(800.0)), build)
}

#[allow(dead_code)]
fn open_test_shell_sized(
    cx: &mut TestAppContext,
    viewport: gpui::Size<Pixels>,
    build: impl FnOnce(&mut Window, &mut Context<AppShell>) -> AppShell + 'static,
) -> (Entity<AppShell>, gpui::VisualTestContext) {
    cx.update(crate::init);
    let mut shell_entity: Option<Entity<AppShell>> = None;
    let window = cx
        .update(|cx| {
            cx.open_window(
                gpui::WindowOptions {
                    window_bounds: Some(gpui::WindowBounds::Windowed(gpui::Bounds::new(
                        Default::default(),
                        viewport,
                    ))),
                    ..Default::default()
                },
                |window, cx| {
                    let shell = cx.new(|cx| build(window, cx));
                    shell_entity = Some(shell.clone());
                    cx.new(|cx| gpui_kit::component::Root::new(shell, window, cx))
                },
            )
        })
        .expect("open test root window");
    let shell = shell_entity.expect("shell created inside root window");
    let visual_cx = gpui::VisualTestContext::from_window(window.into(), cx);
    (shell, visual_cx)
}

fn task(index: usize) -> DownloadRowView {
    DownloadRowView {
        identity: TaskIdentity {
            profile_id: "profile".into(),
            gid: format!("{index:016x}"),
        },
        display_name: format!("archive-{index:05}.bin"),
        name_state: TaskNameStateView::Resolved,
        source_kind: TaskSourceKindView::DirectUri,
        primary_source: Some("https://example.test/file.bin".into()),
        directory: Some("C:/downloads".into()),
        category_id: None,
        followed_by: Vec::new(),
        belongs_to: None,
        status: TaskStatusView::Complete,
        error: None,
        total_bytes: 1_048_576,
        completed_bytes: 1_048_576,
        uploaded_bytes: 0,
        download_rate: 0,
        upload_rate: 0,
        eta_seconds: Some(0),
        observed_seeding_seconds: None,
        revision: 1,
    }
}

fn snapshot(count: usize) -> WorkspaceSnapshot {
    WorkspaceSnapshot {
        profile_id: "profile".into(),
        session_id: "session".into(),
        generation: 1,
        source_revision: 1,
        connection: ConnectionView::Connected,
        stale: false,
        local_path_actions_available: true,
        download_rate: 0,
        upload_rate: 0,
        speed_history: Vec::new(),
        counts: TaskCountsView {
            all: count,
            completed: count,
            ..TaskCountsView::default()
        },
        stopped_history: crate::StoppedHistoryView {
            loaded: count,
            total: Some(count),
            can_load_more: false,
            local_saved: 0,
        },
        tasks: (0..count).map(task).collect(),
        capabilities: crate::EngineCapabilitiesView::unknown(),
    }
}

fn details(file_count: usize) -> TaskDetailsView {
    TaskDetailsView {
        directory: Some("C:/downloads".into()),
        primary_source: Some("https://example.test/file.bin".into()),
        output_path: Some("C:/downloads".into()),
        path_validation: TaskPathValidationView::Valid {
            existing_files: file_count,
            missing_paths: 0,
        },
        info_hash: Some("0123456789abcdef".into()),
        piece_length: Some(1_048_576),
        piece_count: Some(file_count as u32),
        trackers: vec![TaskTrackerView {
            tier: 1,
            uri: "https://tracker.example/announce".into(),
        }],
        uris: vec![TaskUriView {
            uri: "https://example.test/file.bin".into(),
            status: crate::TaskUriStatusView::Used,
        }],
        servers: Vec::new(),
        peers: Vec::new(),
        options: vec![TaskOptionView {
            key: "max-download-limit".into(),
            value: "0".into(),
            redacted: false,
        }],
        files: (0..file_count)
            .map(|index| TaskFileView {
                index: index as u32 + 1,
                path: format!("C:/downloads/file-{index:05}.bin"),
                length: 1_048_576,
                completed_length: 524_288,
                selected: true,
            })
            .collect(),
    }
}

fn metadata_preview(
    path: &str,
    kind: AddDownloadMetadataKindView,
    file_count: u32,
) -> AddDownloadMetadataPreviewView {
    AddDownloadMetadataPreviewView {
        path: PathBuf::from(path),
        kind,
        content_sha256: "digest".into(),
        info_hash: (kind == AddDownloadMetadataKindView::Torrent)
            .then(|| "0123456789abcdef0123456789abcdef01234567".into()),
        files: (1..=file_count)
            .map(|index| AddDownloadMetadataFileView {
                index,
                path: format!("file-{index}.bin"),
                length: Some(u64::from(index) * 100),
            })
            .collect(),
        selected_file_indices: (1..=file_count).collect(),
    }
}

mod chrome_and_power;
mod commands_and_dialogs;
mod details;
mod list_and_selection;
mod settings;

fn bridge_download(url: &str, referer: &str, cookie: Option<&str>) -> BridgeDownloadView {
    BridgeDownloadView {
        url: url.into(),
        referer: referer.into(),
        user_agent: "AriaDeck-Bridge/1.0".into(),
        cookie: cookie.map(SecretStringView::new),
        filename: Some("file.bin".into()),
        file_size: Some(1_048_576),
    }
}
