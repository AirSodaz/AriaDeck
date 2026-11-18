//! Local single-instance IPC shared by `ariadeck-desktop` (server) and
//! `ariadeck-bridge` (client).
//!
//! The transport is a bounded, per-data-directory local socket. It carries three
//! kinds of launch items: metadata file paths (D-037), magnet URIs (D-038), and
//! browser-bridge downloads (D-045).
//!
//! This crate must stay free of GPUI so the native messaging host can link it
//! without pulling in the UI stack.

mod paths;
mod protocol;
mod transport;

pub use paths::{PORTABLE_MARKER_FILE, default_data_dir, resolve_data_dir};
pub use protocol::{
    BridgeDownload, LaunchRequest, MAX_COOKIE_BYTES, MAX_FILENAME_BYTES, MAX_LAUNCH_ITEMS,
    MAX_REFERER_BYTES, MAX_URL_BYTES, MAX_USER_AGENT_BYTES, is_supported_magnet_uri,
};
pub use transport::{
    InstanceRole, InstanceShutdownHandle, coordinate_instance, coordinate_instance_with_shutdown,
    forward_to_primary,
};
