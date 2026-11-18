//! Local managed `aria2c` process lifecycle, restart supervisor, ownership
//! locks, and session-file recovery.

mod config;
mod lock;
mod monitor;
mod process;
mod session;

#[cfg(test)]
mod tests;

pub use config::{
    ChildSpawnHook, ExternalEngineProfile, LocalEngineConfig, reserve_loopback_port,
    validate_executable,
};
pub use lock::ProfileOwnershipLock;
pub use monitor::{
    LocalEngineHealth, LocalEngineHealthHandle, LocalEngineSupervisor, LocalRestartPolicy,
};
pub use process::LocalEngineProcess;
pub use session::{SessionFilePreparation, prepare_session_file};

#[cfg(test)]
pub(crate) use config::local_engine_arguments;
pub(crate) use config::run_aria2_version;
#[cfg(test)]
pub(crate) use lock::process_appears_alive;
