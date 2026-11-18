//! Local external-engine lifecycle, managed core registry, and profile persistence.
//!
//! This crate deliberately owns only process, core-installation, and profile
//! concerns. RPC synchronization remains in `ariadeck-rpc`, while profile
//! identity is shared through `ariadeck-domain`.

mod atomic_io;
mod cores;
pub mod discovery;
mod error;
mod fs_gateway;
mod profiles;
mod supervisor;

pub use cores::{
    Aria2Probe, CoreInstallStatus, CoreInstallation, CoreInstallationSummary, CoreInstallationView,
    CoreRegistry, CoreSource, CoreStore, parse_aria2_version_output, probe_aria2,
};
pub use discovery::{CoreDiscoveryOrigin, DiscoveredCore, aria2_executable_name, discover_cores};
pub use error::EngineError;
pub(crate) use error::io_error;
pub use fs_gateway::{
    LocalDownloadDestinationGateway, LocalDownloadRootRegistry, LocalTaskFileGateway,
};
pub use profiles::{
    JsonProfileStore, LoadedCatalog, LoadedProfile, PROFILE_CATALOG_SCHEMA_VERSION, ProfileCatalog,
    ProfileEntry, ProfileKind, ProfileStoreRecovery, RpcSecretRef,
};
pub use supervisor::{
    ChildSpawnHook, ExternalEngineProfile, LocalEngineConfig, LocalEngineHealth,
    LocalEngineHealthHandle, LocalEngineProcess, LocalEngineSupervisor, LocalRestartPolicy,
    ProfileOwnershipLock, SessionFilePreparation, prepare_session_file, reserve_loopback_port,
    validate_executable,
};

use std::{ffi::OsStr, process::Command};

pub(crate) const CONFIG_FILE_NAME: &str = "aria2.conf";
pub(crate) const SESSION_FILE_NAME: &str = "aria2.session";
pub(crate) const LOG_FILE_NAME: &str = "aria2.log";
pub(crate) const OWNERSHIP_LOCK_FILE_NAME: &str = ".ariadeck-engine.lock";

/// `CREATE_NO_WINDOW` — the child gets no console, inherits none, and keeps the
/// redirected stdio handles it is spawned with.
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Build a command for a console program that must not show a console window.
///
/// The desktop binary is `windows_subsystem = "windows"`, so it owns no console;
/// spawning a console-subsystem child like `aria2c` makes Windows allocate a
/// fresh one. That is a black window sitting next to the app for the managed
/// engine, and a flash for every `--version` probe. Every command this crate
/// runs is such a program, so route them all through here.
#[must_use]
pub fn console_free_command(program: impl AsRef<OsStr>) -> Command {
    let command = Command::new(program);
    #[cfg(windows)]
    let command = {
        use std::os::windows::process::CommandExt as _;
        let mut command = command;
        command.creation_flags(CREATE_NO_WINDOW);
        command
    };
    command
}
