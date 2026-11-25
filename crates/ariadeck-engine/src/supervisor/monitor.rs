use std::{
    collections::VecDeque,
    fmt,
    path::{Path, PathBuf},
    sync::{Arc, Condvar, Mutex, Weak},
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use ariadeck_domain::ProfileId;
use secrecy::{ExposeSecret as _, SecretString};
use url::Url;

use crate::EngineError;

use super::{
    config::{ChildSpawnHook, LocalEngineConfig},
    process::LocalEngineProcess,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LocalRestartPolicy {
    pub max_restarts: u32,
    pub window: Duration,
    pub poll_interval: Duration,
}

impl Default for LocalRestartPolicy {
    fn default() -> Self {
        Self {
            max_restarts: 2,
            window: Duration::from_secs(30),
            poll_interval: Duration::from_millis(250),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LocalEngineHealth {
    Running { restarts: u32 },
    Restarting { attempt: u32 },
    Failed { restarts: u32, reason: String },
}

/// Read-only weak handle for observing a supervisor without extending its lifetime.
#[derive(Clone, Default)]
pub struct LocalEngineHealthHandle {
    shared: Weak<SupervisorShared>,
}

impl fmt::Debug for LocalEngineHealthHandle {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("LocalEngineHealthHandle")
            .field("health", &self.health())
            .finish_non_exhaustive()
    }
}

impl LocalEngineHealthHandle {
    #[must_use]
    pub fn health(&self) -> Option<LocalEngineHealth> {
        let shared = self.shared.upgrade()?;
        Some(
            shared
                .health
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .clone(),
        )
    }
}

pub(crate) struct SupervisorShared {
    pub(crate) process: Mutex<LocalEngineProcess>,
    health: Mutex<LocalEngineHealth>,
    stop: Mutex<bool>,
    stop_signal: Condvar,
    policy: LocalRestartPolicy,
}

/// Monitors a local aria2 child and restarts short-lived crashes in place.
pub struct LocalEngineSupervisor {
    pub(crate) shared: Arc<SupervisorShared>,
    monitor: Option<JoinHandle<()>>,
    endpoint: Url,
    secret: SecretString,
    profile_id: ProfileId,
}

impl fmt::Debug for LocalEngineSupervisor {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("LocalEngineSupervisor")
            .field("endpoint", &self.endpoint)
            .field("secret", &"[REDACTED]")
            .field("profile_id", &self.profile_id)
            .field("health", &self.health())
            .finish_non_exhaustive()
    }
}

impl LocalEngineSupervisor {
    pub fn spawn(config: &LocalEngineConfig) -> Result<Self, EngineError> {
        Self::spawn_with_policy_and_hook(config, LocalRestartPolicy::default(), None)
    }

    pub fn spawn_with_child_hook(
        config: &LocalEngineConfig,
        on_child_spawned: Option<ChildSpawnHook>,
    ) -> Result<Self, EngineError> {
        Self::spawn_with_policy_and_hook(config, LocalRestartPolicy::default(), on_child_spawned)
    }

    pub fn spawn_with_policy(
        config: &LocalEngineConfig,
        policy: LocalRestartPolicy,
    ) -> Result<Self, EngineError> {
        Self::spawn_with_policy_and_hook(config, policy, None)
    }

    pub fn spawn_with_policy_and_hook(
        config: &LocalEngineConfig,
        policy: LocalRestartPolicy,
        on_child_spawned: Option<ChildSpawnHook>,
    ) -> Result<Self, EngineError> {
        let process = LocalEngineProcess::spawn_with_child_hook(config, on_child_spawned)?;
        let endpoint = process.endpoint().clone();
        let secret = SecretString::from(process.secret().to_owned());
        let profile_id = process.config().profile_id;
        let shared = Arc::new(SupervisorShared {
            process: Mutex::new(process),
            health: Mutex::new(LocalEngineHealth::Running { restarts: 0 }),
            stop: Mutex::new(false),
            stop_signal: Condvar::new(),
            policy,
        });
        let monitor_shared = shared.clone();
        let monitor = thread::Builder::new()
            .name("ariadeck-engine-supervisor".into())
            .spawn(move || monitor_local_engine(&monitor_shared))
            .map_err(EngineError::SpawnSupervisor)?;

        Ok(Self {
            shared,
            monitor: Some(monitor),
            endpoint,
            secret,
            profile_id,
        })
    }

    #[must_use]
    pub fn endpoint(&self) -> &Url {
        &self.endpoint
    }

    #[must_use]
    pub fn secret(&self) -> &str {
        self.secret.expose_secret()
    }

    #[must_use]
    pub fn profile_id(&self) -> ProfileId {
        self.profile_id
    }

    /// True when startup replaced a corrupt aria2 session file.
    #[must_use]
    pub fn session_was_recovered(&self) -> bool {
        self.shared
            .process
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .session_was_recovered()
    }

    #[must_use]
    pub fn session_recovery_backup(&self) -> Option<PathBuf> {
        self.shared
            .process
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .session_recovery_backup()
            .map(Path::to_path_buf)
    }

    #[must_use]
    pub fn health(&self) -> LocalEngineHealth {
        self.shared
            .health
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    #[must_use]
    pub fn health_handle(&self) -> LocalEngineHealthHandle {
        LocalEngineHealthHandle {
            shared: Arc::downgrade(&self.shared),
        }
    }

    /// Stop crash monitoring before the composition root requests RPC shutdown.
    pub fn stop_monitoring(&mut self) {
        {
            let mut stopped = self
                .shared
                .stop
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            *stopped = true;
            self.shared.stop_signal.notify_all();
        }
        if let Some(monitor) = self.monitor.take() {
            let _ = monitor.join();
        }
    }

    /// Mark that an RPC `aria2.shutdown` was sent so `shutdown()` allows a brief
    /// grace period for session flush before hard termination.
    pub fn mark_graceful_shutdown_requested(&self) {
        self.shared
            .process
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .mark_graceful_shutdown_requested();
    }

    pub fn shutdown(&mut self) -> Result<(), EngineError> {
        self.stop_monitoring();
        self.shared
            .process
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .shutdown()
    }
}

impl Drop for LocalEngineSupervisor {
    fn drop(&mut self) {
        let _ = self.shutdown();
    }
}

fn monitor_local_engine(shared: &SupervisorShared) {
    let mut restarts = VecDeque::new();
    loop {
        {
            let stopped = shared
                .stop
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            if *stopped {
                break;
            }
            let (stopped, _) = shared
                .stop_signal
                .wait_timeout(stopped, shared.policy.poll_interval)
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            if *stopped {
                break;
            }
        }

        let now = Instant::now();
        let previous_restart_count = restarts.len();
        while restarts
            .front()
            .is_some_and(|restart| now.duration_since(*restart) > shared.policy.window)
        {
            restarts.pop_front();
        }
        let restart_count_changed = restarts.len() != previous_restart_count;
        let is_running = restart_count_changed && {
            let health = shared
                .health
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            matches!(&*health, LocalEngineHealth::Running { .. })
        };
        if is_running {
            set_supervisor_health(
                shared,
                LocalEngineHealth::Running {
                    restarts: u32::try_from(restarts.len()).unwrap_or(u32::MAX),
                },
            );
        }

        let mut process = shared
            .process
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let exit = match process.try_wait() {
            Ok(exit) => exit,
            Err(error) => {
                set_supervisor_health(
                    shared,
                    LocalEngineHealth::Failed {
                        restarts: u32::try_from(restarts.len()).unwrap_or(u32::MAX),
                        reason: error.to_string(),
                    },
                );
                break;
            }
        };
        let Some(status) = exit else {
            continue;
        };
        if *shared
            .stop
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
        {
            break;
        }
        let restart_count = u32::try_from(restarts.len()).unwrap_or(u32::MAX);
        if restart_count >= shared.policy.max_restarts {
            set_supervisor_health(
                shared,
                LocalEngineHealth::Failed {
                    restarts: restart_count,
                    reason: format!("aria2 exited unexpectedly with {status}"),
                },
            );
            break;
        }

        let attempt = restart_count.saturating_add(1);
        set_supervisor_health(shared, LocalEngineHealth::Restarting { attempt });
        match process.restart() {
            Ok(()) => {
                restarts.push_back(now);
                set_supervisor_health(
                    shared,
                    LocalEngineHealth::Running {
                        restarts: u32::try_from(restarts.len()).unwrap_or(u32::MAX),
                    },
                );
            }
            Err(error) => {
                restarts.push_back(now);
                if attempt >= shared.policy.max_restarts {
                    set_supervisor_health(
                        shared,
                        LocalEngineHealth::Failed {
                            restarts: attempt,
                            reason: error.to_string(),
                        },
                    );
                    break;
                }
            }
        }
    }
}

fn set_supervisor_health(shared: &SupervisorShared, health: LocalEngineHealth) {
    *shared
        .health
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) = health;
}
