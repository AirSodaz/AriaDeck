# AriaDeck Technical Design Document

**Project:** AriaDeck
**Document Type:** Software Architecture and Product Design
**Status:** Initial Design
**Primary Language:** Rust
**Desktop UI Framework:** GPUI
**Backend Engine:** aria2
**Communication Protocol:** aria2 JSON-RPC over WebSocket or HTTP

------

## 1. Executive Summary

AriaDeck is a modern, high-performance, native desktop interface for aria2.

The project is designed around a strict separation between the graphical application and the aria2 download engine. AriaDeck does not embed aria2 as a library. Instead, it manages or connects to an independent aria2 process through JSON-RPC.

This architecture allows users to:

- Use an aria2 executable managed by AriaDeck.
- Select an existing aria2 executable from any supported path.
- Install and switch between multiple aria2 versions.
- Connect to an aria2 instance running on another computer, server, router, NAS, or container.
- Choose where application data, engine files, profiles, and downloads are stored.
- Use AriaDeck in either system-installed or portable mode.

The user interface will be implemented with GPUI. AriaDeck will define its own design system and component library on top of GPUI. Existing components may be adapted where appropriate, but all application pages will depend only on the AriaDeck UI abstraction.

The primary engineering priorities are:

1. High performance with large download queues.
2. A modern and consistent native desktop experience.
3. Reliable management of local and remote aria2 engines.
4. Strong isolation between UI, application logic, RPC, storage, and process management.
5. Safe installation, verification, switching, and rollback of aria2 versions.
6. Cross-platform support for Windows, macOS, and Linux.
7. Maintainable architecture that can evolve independently of GPUI and aria2.

------

# 2. Product Vision

AriaDeck should feel like a modern desktop productivity application rather than a browser-based administration panel.

The application should combine:

- The performance of a native Rust application.
- The operational flexibility of aria2.
- The visual quality of modern desktop tools.
- The reliability expected from a long-running download manager.
- The control required by advanced users.
- A simple default experience for non-technical users.

AriaDeck should not merely reproduce an existing aria2 Web UI in a native window. Its interaction model should be redesigned for desktop use.

The application should provide:

- Fast navigation.
- High-density task management.
- Keyboard-first workflows.
- Responsive batch actions.
- Efficient search and filtering.
- Smooth real-time progress updates.
- Clear visibility into engine health and configuration.
- Reliable handling of background processes and failures.

------

# 3. Goals

## 3.1 Functional Goals

AriaDeck must support:

- HTTP, HTTPS, FTP, SFTP, Metalink, and BitTorrent tasks supported by the connected aria2 build.
- Adding downloads through URLs, files, magnets, Metalink files, and torrent files.
- Starting, pausing, resuming, retrying, and removing downloads.
- Viewing active, waiting, paused, completed, and failed tasks.
- Viewing task details, files, peers, servers, trackers, and task options.
- Selecting files inside BitTorrent and Metalink tasks.
- Global and per-task speed limits.
- Queue ordering and task priority.
- Batch operations.
- Search, filtering, and sorting.
- Session persistence.
- Multiple aria2 profiles.
- Multiple local aria2 versions.
- Custom aria2 executable paths.
- Remote aria2 RPC connections.
- Portable mode.
- System tray operation.
- Light, dark, and system themes.
- Engine health monitoring and automatic recovery.

## 3.2 Performance Goals

AriaDeck should:

- Remain responsive with at least 10,000 historical tasks.
- Efficiently display thousands of filtered task rows.
- Avoid rendering off-screen list items.
- Avoid replacing the entire application state on every polling cycle.
- Limit unnecessary JSON-RPC fields and payloads.
- Use incremental state updates by task GID.
- Reduce refresh frequency when the application is minimized or inactive.
- Avoid unbounded storage of speed history.
- Avoid blocking the UI thread with network, file, archive, hashing, or process operations.
- Provide predictable memory use during long-running sessions.

## 3.3 User Experience Goals

The application should:

- Start quickly.
- Display useful content immediately after launch.
- Present common actions without overwhelming the user.
- Keep advanced settings accessible but out of the main workflow.
- Use native keyboard and pointer interaction patterns.
- Provide clear loading, empty, error, and disconnected states.
- Preserve user context during RPC reconnects.
- Avoid full-screen modal flows for routine operations.
- Provide reversible actions where practical.
- Remain visually consistent across custom and third-party components.

## 3.4 Maintainability Goals

The codebase should:

- Keep business logic independent of GPUI.
- Keep aria2 RPC models independent of UI models.
- Hide third-party UI components behind AriaDeck-owned interfaces.
- Use strong Rust types for task states, engine states, versions, and paths.
- Permit replacement of individual subsystems.
- Support deterministic unit and integration testing.
- Keep platform-specific logic isolated.
- Make failures observable through structured logging and diagnostics.

------

# 4. Non-Goals

The initial release will not attempt to:

- Reimplement the aria2 download engine.
- Embed aria2 as a statically linked library.
- Provide a browser-accessible Web UI.
- Provide mobile applications.
- Synchronize download state through an AriaDeck cloud service.
- Implement a general-purpose BitTorrent engine.
- Replace all aria2 configuration options with custom abstractions.
- Support arbitrary third-party plugin execution inside the application process.
- Guarantee identical visuals across every operating system.
- Edit partially downloaded file contents.
- Act as a media library or content catalog.
- Provide peer-to-peer remote access without an existing network path.

These capabilities may be evaluated later but are outside the initial architecture.

------

# 5. Core Design Principles

## 5.1 The GUI and aria2 Must Remain Independent

AriaDeck communicates with aria2 only through its RPC interface.

The GUI must not depend on a fixed executable location, fixed aria2 version, or embedded binary.

This allows AriaDeck to support:

- Managed local engines.
- User-provided engines.
- System-installed engines.
- Custom aria2 builds.
- Containerized engines.
- Remote engines.

## 5.2 AriaDeck Depends on Capabilities, Not Versions Alone

Different aria2 builds may expose different transport and protocol capabilities.

AriaDeck should detect:

- aria2 version.
- Enabled features reported by the engine.
- WebSocket RPC availability.
- Supported protocols.
- Encryption and transport capabilities where detectable.
- Required RPC methods.

Compatibility decisions should use capabilities rather than only comparing version numbers.

## 5.3 All UI Pages Depend on AriaDeck UI Components

Application pages must not directly depend on third-party component APIs.

The dependency direction must be:

```text
GPUI
  ↓
Optional third-party GPUI components
  ↓
ariadeck-ui
  ↓
Application pages
```

This protects the application from framework changes and guarantees visual consistency.

## 5.4 State Updates Must Be Incremental

The application must not replace the complete download collection after every RPC response.

Tasks should be indexed by GID and updated using patches.

Only changed fields and affected views should be notified.

## 5.5 Background Work Must Never Block Rendering

The UI thread must not perform:

- RPC network requests.
- Archive extraction.
- File hashing.
- Executable verification.
- Directory scanning.
- Database migration.
- Large JSON parsing.
- Process waiting.
- Update downloads.

All such work must be performed asynchronously or on dedicated worker threads.

## 5.6 Destructive Operations Must Be Explicit

Removing an aria2 task and deleting downloaded files are different operations.

The UI must clearly distinguish:

- Remove task from aria2.
- Remove task and delete files.
- Delete incomplete files.
- Delete metadata only.
- Remove an installed engine version.

------

# 6. Proposed Technology Stack

## 6.1 Primary Stack

| Area                     | Technology                                     |
| ------------------------ | ---------------------------------------------- |
| Programming language     | Rust                                           |
| Desktop UI               | GPUI                                           |
| Async runtime            | Tokio                                          |
| WebSocket RPC            | tokio-tungstenite or equivalent                |
| HTTP RPC and downloads   | reqwest or equivalent                          |
| Serialization            | serde and serde_json                           |
| Error types              | thiserror                                      |
| Application-level errors | anyhow where appropriate                       |
| Structured logging       | tracing                                        |
| Semantic versions        | semver                                         |
| URL parsing              | url                                            |
| Secret handling          | secrecy                                        |
| Credential storage       | OS keychain through a suitable abstraction     |
| Local database           | SQLite                                         |
| Database access          | rusqlite or SQLx                               |
| Hash verification        | SHA-256 implementation from RustCrypto         |
| Archive extraction       | Format-specific safe Rust libraries            |
| Configuration            | serde with JSON or TOML                        |
| File dialogs             | Platform-native dialog integration             |
| System information       | Platform-specific modules behind common traits |

## 6.2 UI Dependency Policy

GPUI is the rendering, layout, event, and windowing foundation.

AriaDeck may use selected external GPUI components for common controls, provided that:

- They are wrapped by `ariadeck-ui`.
- Their styles are mapped to AriaDeck design tokens.
- Their behavior follows AriaDeck accessibility and interaction rules.
- They can be replaced without changing application pages.

Domain-specific components should normally be implemented by AriaDeck.

------

# 7. High-Level Architecture

```text
┌───────────────────────────────────────────────────────────────┐
│                         AriaDeck UI                            │
│                                                               │
│  Pages  ──  Domain Components  ──  AriaDeck Design System     │
└──────────────────────────────┬────────────────────────────────┘
                               │ Commands and view models
┌──────────────────────────────▼────────────────────────────────┐
│                     Application Services                      │
│                                                               │
│ Download Service      Profile Service      Settings Service   │
│ Engine Service        Search Service       Notification Svc   │
└───────────────┬──────────────────────┬────────────────────────┘
                │                      │
┌───────────────▼────────────┐  ┌──────▼───────────────────────┐
│      Download Store       │  │       Engine Supervisor      │
│                           │  │                              │
│ Tasks by GID              │  │ Process lifecycle            │
│ Global statistics         │  │ Health checks                │
│ Filters and ordering      │  │ Restart and rollback         │
│ Incremental patches       │  │ RPC connection ownership     │
└───────────────┬───────────┘  └──────┬───────────────────────┘
                │                     │
┌───────────────▼─────────────────────▼─────────────────────────┐
│                         aria2 RPC Layer                        │
│                                                               │
│ Typed methods   Batch requests   Notifications   Reconnection │
└──────────────────────────────┬────────────────────────────────┘
                               │ JSON-RPC
              ┌────────────────┴───────────────────┐
              │                                    │
┌─────────────▼───────────────┐       ┌────────────▼────────────┐
│ Managed or External aria2c  │       │      Remote aria2       │
└─────────────────────────────┘       └─────────────────────────┘
```

Additional supporting systems:

```text
Core Version Manager
Profile Storage
SQLite Database
Credential Store
Application Updater
Diagnostics and Logging
Platform Integration
```

------

# 8. Workspace Structure

A proposed Rust workspace:

```text
ariadeck/
├── Cargo.toml
├── rust-toolchain.toml
├── LICENSE
├── README.md
├── docs/
│   ├── architecture.md
│   ├── rpc.md
│   ├── ui-system.md
│   └── release-process.md
│
├── apps/
│   └── ariadeck-desktop/
│       ├── Cargo.toml
│       └── src/
│
└── crates/
    ├── ariadeck-domain/
    ├── ariadeck-rpc/
    ├── ariadeck-engine/
    ├── ariadeck-core-manager/
    ├── ariadeck-storage/
    ├── ariadeck-settings/
    ├── ariadeck-platform/
    ├── ariadeck-ui/
    ├── ariadeck-telemetry/
    └── ariadeck-test-support/
```

## 8.1 `ariadeck-domain`

Contains business-level types that do not depend on GPUI or RPC transport models.

Examples:

- `DownloadTask`
- `DownloadStatus`
- `TransferStats`
- `EngineProfile`
- `EngineSource`
- `EngineState`
- `CoreVersion`
- `InstalledCore`
- `DownloadFilter`
- `SortOrder`
- `TaskPatch`

## 8.2 `ariadeck-rpc`

Contains:

- JSON-RPC request and response types.
- aria2 method wrappers.
- WebSocket and HTTP transports.
- Authentication token injection.
- Notification handling.
- Batch and multicall support.
- Reconnection logic.
- RPC-level errors.
- Conversion from RPC models into domain models.

This crate must not depend on GPUI.

## 8.3 `ariadeck-engine`

Contains:

- Managed process startup.
- Process shutdown.
- Health checks.
- Port selection.
- Runtime secret generation.
- Crash detection.
- Restart policy.
- Engine compatibility checks.
- Local and remote engine session management.

## 8.4 `ariadeck-core-manager`

Contains:

- Available version manifests.
- Downloading aria2 packages.
- SHA-256 verification.
- Archive extraction.
- Executable validation.
- Parallel version installation.
- Activation and rollback metadata.
- Core directory migration.
- Cleanup of unused versions.

## 8.5 `ariadeck-storage`

Contains:

- SQLite schema.
- Migrations.
- Task metadata cache.
- Profile persistence.
- UI state persistence.
- Download history.
- Engine installation records.
- Update records.
- Diagnostic records.

## 8.6 `ariadeck-settings`

Contains:

- Typed settings.
- Validation.
- Defaults.
- Import and export.
- Portable and system path resolution.
- Settings migrations.

## 8.7 `ariadeck-platform`

Contains platform-specific implementations for:

- Application data directories.
- File and folder dialogs.
- Opening paths in the system file manager.
- System tray.
- Notifications.
- Autostart.
- Window effects.
- Credential storage.
- Process groups.
- Executable permissions.
- Code-signing-aware installation rules.

## 8.8 `ariadeck-ui`

Contains:

- Design tokens.
- Theme implementation.
- Base components.
- Composite patterns.
- Download-specific components.
- Icons.
- Accessibility helpers.
- Animation definitions.
- Input and focus policies.

## 8.9 `ariadeck-desktop`

Contains:

- Application bootstrap.
- GPUI windows.
- Routing and page composition.
- Command dispatch.
- View models.
- Dialog orchestration.
- Application lifecycle.
- System tray behavior.

------

# 9. Engine Source Model

AriaDeck supports three primary engine sources.

```rust
pub enum EngineSource {
    Managed {
        core_id: CoreInstallationId,
    },
    External {
        executable: PathBuf,
    },
    Remote {
        endpoint: Url,
        credential_id: Option<CredentialId>,
    },
}
```

## 9.1 Managed Engine

AriaDeck owns the installation and lifecycle of the aria2 executable.

Capabilities:

- Install versions.
- Activate a version.
- Start and stop aria2.
- Configure runtime arguments.
- Detect crashes.
- Roll back after failed upgrades.
- Remove unused versions.

## 9.2 External Engine

The user selects an existing aria2 executable.

AriaDeck may start and manage the process but does not own the executable.

Rules:

- Never modify or replace the selected executable.
- Validate the executable before use.
- Store the canonicalized path where possible.
- Display a warning if the path becomes unavailable.
- Allow the user to rescan capabilities.
- Keep configuration and session files separate unless explicitly configured otherwise.

## 9.3 Remote Engine

AriaDeck connects to an existing RPC endpoint.

Supported endpoint forms should include:

```text
http://host:port/jsonrpc
https://host:port/jsonrpc
ws://host:port/jsonrpc
wss://host:port/jsonrpc
```

Remote profiles should support:

- Display name.
- Endpoint.
- Secret or authentication reference.
- TLS policy.
- Connection timeout.
- Reconnect policy.
- Optional download-path mapping.
- Optional host capability notes.

AriaDeck must not assume that a remote file path is accessible from the local computer.

------

# 10. Engine Lifecycle

## 10.1 Managed Startup Sequence

```text
Resolve active profile
    ↓
Resolve selected aria2 installation
    ↓
Validate executable and required files
    ↓
Select available loopback port
    ↓
Generate temporary RPC secret
    ↓
Build command-line arguments
    ↓
Start child process
    ↓
Connect RPC transport
    ↓
Call aria2.getVersion
    ↓
Verify expected process and capabilities
    ↓
Start synchronization loop
```

## 10.2 Suggested Runtime Arguments

For a locally managed engine:

```text
--enable-rpc=true
--rpc-listen-all=false
--rpc-listen-port=<selected-port>
--rpc-secret=<generated-secret>
--conf-path=<profile-config>
--input-file=<profile-session>
--save-session=<profile-session>
--save-session-interval=30
```

Additional arguments may be generated from profile settings.

The runtime secret should normally be ephemeral and stored only in memory.

## 10.3 Graceful Shutdown

The shutdown sequence should:

1. Stop accepting new UI commands.
2. Request session persistence.
3. Wait briefly for the save operation.
4. Request graceful engine shutdown where supported.
5. Wait for the process.
6. Terminate the process only if it fails to exit.
7. Record whether shutdown was clean.

## 10.4 Crash Recovery

The supervisor should distinguish:

- User-requested shutdown.
- Application shutdown.
- Unexpected process exit.
- Repeated crash loop.
- RPC transport failure while the process remains alive.

A suggested policy:

- First unexpected crash: restart automatically.
- Second crash in a short period: restart once with diagnostics.
- Repeated crashes: stop automatic restarts and show recovery UI.

Recovery options:

- Restart the same version.
- Start without loading the session.
- Switch to the previous working version.
- Open logs.
- Select another executable.

------

# 11. Core Version Management

## 11.1 Directory Layout

```text
data/
├── cores/
│   └── aria2/
│       ├── 1.36.0/
│       │   └── windows-x86_64/
│       │       ├── aria2c.exe
│       │       └── installation.json
│       └── 1.37.0/
│           └── windows-x86_64/
│               ├── aria2c.exe
│               └── installation.json
│
├── profiles/
├── database/
├── cache/
├── logs/
└── settings/
```

Engine binaries and mutable profile data must never share the same directory.

## 11.2 Installation Manifest

Each installed version should have metadata similar to:

```json
{
  "schema": 1,
  "version": "1.37.0",
  "target": "windows-x86_64",
  "source": "managed",
  "sha256": "...",
  "installed_at": "2026-07-19T12:00:00Z",
  "executable": "aria2c.exe",
  "validated_version": "1.37.0",
  "features": [
    "BitTorrent",
    "HTTPS",
    "WebSocket"
  ]
}
```

## 11.3 Installation Process

```text
Fetch signed or trusted version manifest
    ↓
Select platform and architecture package
    ↓
Download into cache
    ↓
Verify expected size and SHA-256
    ↓
Extract into temporary directory
    ↓
Validate paths and reject traversal entries
    ↓
Set executable permission if required
    ↓
Execute aria2c --version
    ↓
Record detected capabilities
    ↓
Atomically move into final version directory
    ↓
Register installation in the database
```

## 11.4 Version Switching

Switching versions should:

1. Save the current session.
2. Stop the current managed process.
3. Mark the old version as `last_working`.
4. Start the selected version.
5. Complete RPC and capability validation.
6. Mark the new version as active after successful health checks.
7. Roll back automatically if startup fails.

## 11.5 Update Channels

Possible channels:

- Stable.
- Preview.
- Custom manifest.

The initial release should default to stable and require explicit user action before switching versions.

## 11.6 Core Storage Relocation

When the user changes the core storage path:

- Stop managed engines.
- Verify destination permissions and free space.
- Copy or move installations into a staging directory.
- Verify copied executables and metadata.
- Update the root path atomically.
- Keep the original directory until success is confirmed.
- Offer cleanup after the new path is validated.

------

# 12. Profile Model

A profile represents an aria2 runtime configuration.

```rust
pub struct EngineProfile {
    pub id: ProfileId,
    pub name: String,
    pub source: EngineSource,
    pub paths: ProfilePaths,
    pub runtime: RuntimeSettings,
    pub rpc: RpcSettings,
    pub lifecycle: LifecycleSettings,
}
```

A profile may contain:

- Engine source.
- Configuration file.
- Session file.
- DHT files.
- Default download directory.
- RPC endpoint.
- RPC secret reference.
- Runtime arguments.
- Speed limits.
- Proxy settings.
- Automatic startup preference.
- Automatic reconnect preference.

Profiles allow users to maintain separate environments such as:

- Local desktop downloads.
- NAS downloads.
- Private tracker downloads.
- Temporary or isolated sessions.
- Different aria2 versions.

------

# 13. RPC Architecture

## 13.1 Transport Abstraction

```rust
#[async_trait]
pub trait RpcTransport {
    async fn call(
        &self,
        method: &str,
        params: serde_json::Value,
    ) -> Result<serde_json::Value, RpcError>;

    async fn batch(
        &self,
        requests: Vec<RpcRequest>,
    ) -> Result<Vec<RpcResult>, RpcError>;
}
```

Implementations:

- `WebSocketTransport`
- `HttpTransport`

WebSocket should be preferred because it supports aria2 notifications and persistent connections.

HTTP remains useful for compatibility and fallback.

## 13.2 Authentication

RPC secrets should be injected centrally.

Individual method implementations must not manually handle token prefixes.

```rust
pub struct AuthenticatedTransport<T> {
    inner: T,
    secret: Option<SecretString>,
}
```

Secrets must not be:

- Written to logs.
- Included in panic messages.
- Stored in plain-text settings when a platform credential store is available.
- Exposed through diagnostics exports without explicit redaction.

## 13.3 Typed Method Layer

The RPC crate should provide typed wrappers for commonly used methods:

```rust
pub trait Aria2Client {
    async fn get_version(&self) -> Result<VersionInfo>;
    async fn get_global_stat(&self) -> Result<GlobalStat>;
    async fn tell_active(&self, keys: &[TaskKey]) -> Result<Vec<RpcTask>>;
    async fn tell_waiting(
        &self,
        offset: i64,
        count: u32,
        keys: &[TaskKey],
    ) -> Result<Vec<RpcTask>>;
    async fn tell_stopped(
        &self,
        offset: i64,
        count: u32,
        keys: &[TaskKey],
    ) -> Result<Vec<RpcTask>>;
}
```

The UI should never construct raw JSON-RPC requests.

## 13.4 Refresh Strategy

Recommended initial intervals:

| Data                       | Foreground interval            |
| -------------------------- | ------------------------------ |
| Global transfer statistics | 500 ms                         |
| Active tasks               | 500 ms                         |
| Selected task details      | 750 ms                         |
| Waiting tasks              | 2 seconds                      |
| Stopped tasks              | 5 seconds                      |
| Peer information           | 1–2 seconds while visible      |
| Server information         | On demand or low frequency     |
| Engine version and options | On connect or explicit refresh |

When minimized:

- Global statistics: 2–5 seconds.
- Active tasks: 2–5 seconds.
- Waiting and stopped tasks: event-driven or 10 seconds.
- Expensive details: paused.

## 13.5 Field Selection

List refreshes should request only fields needed for rendering.

A lightweight task projection may include:

```text
gid
status
totalLength
completedLength
uploadLength
downloadSpeed
uploadSpeed
connections
errorCode
verifiedLength
verifyIntegrityPending
```

Static or expensive fields should be loaded separately and cached:

```text
files
bittorrent
followedBy
belongsTo
dir
pieceLength
numPieces
infoHash
```

## 13.6 Notifications

WebSocket notifications should trigger targeted refreshes.

Examples:

- Download started.
- Download paused.
- Download stopped.
- Download completed.
- Download error.
- BitTorrent download completed.

Notifications should not be treated as complete state updates. They should schedule a focused refresh for the affected GID.

## 13.7 Reconnection

Connection states:

```rust
pub enum ConnectionState {
    Disconnected,
    Connecting,
    Authenticating,
    Synchronizing,
    Connected,
    Reconnecting {
        attempt: u32,
    },
    Failed {
        reason: ConnectionFailure,
    },
}
```

Reconnection should use exponential backoff with jitter and a maximum delay.

User commands should fail clearly while disconnected. Commands should not be silently queued indefinitely unless explicitly designed as offline actions.

------

# 14. Domain State Model

## 14.1 Download Store

```rust
pub struct DownloadStore {
    pub tasks: HashMap<Gid, DownloadTask>,
    pub active_order: Vec<Gid>,
    pub waiting_order: Vec<Gid>,
    pub stopped_order: Vec<Gid>,
    pub global_stat: GlobalStat,
    pub revision: u64,
}
```

## 14.2 Task Model

```rust
pub struct DownloadTask {
    pub gid: Gid,
    pub status: DownloadStatus,
    pub display_name: String,
    pub total_length: ByteCount,
    pub completed_length: ByteCount,
    pub upload_length: ByteCount,
    pub download_speed: ByteRate,
    pub upload_speed: ByteRate,
    pub connections: u32,
    pub error: Option<TaskError>,
    pub metadata: TaskMetadata,
    pub revision: u64,
}
```

Frequently updated values should avoid unnecessary heap allocation.

Static metadata should be stored separately where useful.

## 14.3 Incremental Patches

```rust
pub struct StorePatch {
    pub inserted: Vec<Gid>,
    pub updated: Vec<TaskFieldPatch>,
    pub removed: Vec<Gid>,
    pub order_changes: Vec<OrderPatch>,
    pub global_stat_changed: bool,
}
```

UI observers should subscribe at the narrowest practical scope:

- Global header.
- Sidebar counters.
- Visible task list.
- Selected task details.
- Speed chart.
- Tray status.

## 14.4 Derived Views

Filtering and sorting should generate GID lists rather than duplicate complete tasks.

```rust
pub struct TaskListView {
    pub source_revision: u64,
    pub visible_gids: Vec<Gid>,
    pub filter: DownloadFilter,
    pub sort: DownloadSort,
}
```

Expensive filtering should be debounced and performed outside the rendering path.

------

# 15. Command Architecture

UI interactions should dispatch typed commands.

```rust
pub enum AppCommand {
    AddDownload(AddDownloadRequest),
    PauseTasks(Vec<Gid>),
    ResumeTasks(Vec<Gid>),
    RemoveTasks(RemoveTasksRequest),
    RetryTasks(Vec<Gid>),
    ChangePosition(ChangePositionRequest),
    SwitchProfile(ProfileId),
    StartEngine(ProfileId),
    StopEngine(ProfileId),
    InstallCore(CoreVersion),
    ActivateCore(CoreInstallationId),
}
```

Commands should return structured outcomes:

```rust
pub enum CommandOutcome {
    Success,
    PartialSuccess {
        succeeded: Vec<ItemId>,
        failed: Vec<ItemFailure>,
    },
    Failure(AppError),
}
```

Partial failures are important for batch task operations.

------

# 16. UI Design System

## 16.1 Design Direction

AriaDeck should use a modern, restrained, high-density visual style.

The visual language should emphasize:

- Clear hierarchy.
- Compact information presentation.
- Subtle surfaces.
- Strong typography.
- Minimal visual noise.
- Limited shadows.
- Functional animation.
- Consistent spacing.
- Accessible contrast.

The interface should not rely heavily on:

- Glass effects.
- Large decorative gradients.
- Excessively rounded cards.
- Permanent borders around every region.
- Large empty spacing.
- Continuous decorative animations.

## 16.2 Design Tokens

The UI must use semantic design tokens.

### Colors

```rust
pub struct ThemeColors {
    pub background: Hsla,
    pub surface: Hsla,
    pub elevated_surface: Hsla,
    pub surface_hover: Hsla,
    pub surface_active: Hsla,

    pub text_primary: Hsla,
    pub text_secondary: Hsla,
    pub text_muted: Hsla,
    pub text_inverse: Hsla,

    pub border: Hsla,
    pub border_strong: Hsla,
    pub focus_ring: Hsla,

    pub accent: Hsla,
    pub accent_hover: Hsla,
    pub accent_active: Hsla,

    pub success: Hsla,
    pub warning: Hsla,
    pub danger: Hsla,
    pub information: Hsla,

    pub progress_track: Hsla,
    pub progress_download: Hsla,
    pub progress_upload: Hsla,
}
```

### Spacing

Recommended scale:

```text
4 px
8 px
12 px
16 px
20 px
24 px
32 px
40 px
```

### Radius

Recommended scale:

```text
Small control: 4 px
Button and field: 6 px
Menu and popover: 8 px
Dialog and large panel: 10–12 px
```

### Typography

Typography roles:

- Window title.
- Page title.
- Section title.
- Body.
- Secondary body.
- Caption.
- Monospaced numeric value.
- Button label.
- Table header.

Transfer speeds, percentages, sizes, and durations should use tabular numbers to prevent horizontal movement during updates.

### Motion

Recommended durations:

```text
Hover and pressed state: 80–120 ms
Popover and menu: 120–160 ms
Panel and drawer: 160–220 ms
Progress state transition: 150–250 ms
```

Animations should be disabled or reduced when the operating system requests reduced motion.

------

# 17. UI Component Architecture

## 17.1 Component Layers

```text
Foundation
    Button, Input, Icon, Text, Scrollbar, Divider

Composite
    SearchInput, Select, Menu, Dialog, Tooltip, Toast

Patterns
    SettingsRow, PropertyRow, SidebarItem, EmptyState

Domain
    DownloadRow, TaskProgress, SpeedChart, CoreVersionItem
```

## 17.2 Component Contract

Every interactive component must define:

- Size variants.
- Visual variants.
- Disabled state.
- Hover state.
- Pressed state.
- Focus state.
- Keyboard behavior.
- Loading behavior.
- Error behavior.
- Theme behavior.
- Accessibility metadata.

## 17.3 Third-Party Component Wrapping

Application pages may use:

```rust
use ariadeck_ui::Button;
```

They must not use:

```rust
use third_party_library::Button;
```

The wrapper may delegate internally, but the public component API belongs to AriaDeck.

## 17.4 Core Custom Components

### DownloadRow

Responsibilities:

- Display file or task name.
- Display progress.
- Display status.
- Display speed and estimated time.
- Display upload information when relevant.
- Support selection.
- Support keyboard focus.
- Support hover actions.
- Support context menu actions.
- Work inside a virtualized list.
- Avoid owning a duplicate task state.

### TaskProgress

States:

- Active.
- Waiting.
- Paused.
- Verifying.
- Complete.
- Failed.
- Unknown total.
- Metadata download.

### SpeedChart

Requirements:

- Download and upload series.
- Fixed-capacity sample storage.
- Automatic vertical range.
- Hover inspection.
- Time range selection.
- Efficient point reduction.
- Theme support.
- Rendering suspension when hidden.

### CoreVersionItem

Displays:

- Version.
- Platform and architecture.
- Detected capabilities.
- Installed status.
- Current status.
- Verification status.
- Last successful usage.
- Available actions.

### BatchActionBar

Appears when one or more tasks are selected.

Actions may include:

- Start.
- Pause.
- Retry.
- Move.
- Change options.
- Remove.
- Delete files.
- Clear selection.

### PathPicker

Must support:

- Native folder selection.
- Path validation.
- Permission errors.
- Missing paths.
- Opening the path.
- Copying the path.
- Truncated display with full tooltip.

------

# 18. Main Window Information Architecture

```text
┌──────────────────────────────────────────────────────────────┐
│ Search                                      Speeds   Add Task │
├───────────────┬──────────────────────────────────────────────┤
│ All           │                                              │
│ Active        │              Download List                   │
│ Waiting       │                                              │
│ Paused        │                                              │
│ Completed     │                                              │
│ Failed        │                                              │
│               │                                              │
│ Profiles      │                                              │
│ Settings      │                                              │
└───────────────┴──────────────────────────────────────────────┘
```

## 18.1 Sidebar

The sidebar should contain:

- All tasks.
- Active.
- Waiting.
- Paused.
- Completed.
- Failed.
- Optional tags or categories.
- Profiles.
- Settings.

Counters should update independently from the main task list.

## 18.2 Header

The header should contain:

- Search.
- Current download speed.
- Current upload speed.
- Connection state.
- Add-task button.
- Optional quick actions.

## 18.3 Task Details

Task details should appear in a right-side drawer where space permits.

Suggested sections:

- Overview.
- Files.
- Connections.
- Peers.
- Trackers or servers.
- Options.
- Logs or diagnostics.

The drawer should preserve the main list context.

## 18.4 Add Download Flow

The add-download dialog should support:

- One or more URLs.
- Magnet links.
- Torrent files.
- Metalink files.
- Destination directory.
- File selection where available.
- Per-task options.
- Profile selection when multiple engines exist.

The common path should remain simple, with advanced options collapsed.

------

# 19. Virtualization and Rendering Performance

All potentially large collections must support virtualization:

- Download task list.
- Completed history.
- Torrent file list.
- Peer list.
- Server list.
- Logs.
- Core version history where necessary.

The virtual list should:

- Render only visible rows plus a small overscan area.
- Use stable item identifiers.
- Avoid recreating expensive child state.
- Support keyboard navigation.
- Preserve selection across filtering.
- Handle variable-height rows only where necessary.

Task rows should use a fixed or limited height whenever possible because fixed heights improve virtualization efficiency.

------

# 20. Speed History

A fixed-capacity ring buffer should be used:

```rust
pub struct SpeedHistory {
    pub samples: VecDeque<SpeedSample>,
    pub capacity: usize,
}
```

Example:

- One sample every 500 milliseconds.
- 120 samples for one minute.
- Optional aggregated samples for longer time ranges.

The application should not store unlimited raw speed samples.

For longer history, values may be downsampled:

- 500 ms samples for one minute.
- 5-second averages for one hour.
- 1-minute averages for one day.

Persistent speed history is optional and should not be required for the first release.

------

# 21. Local Storage

## 21.1 SQLite Responsibilities

SQLite should store:

- Profiles.
- Managed core installations.
- Last active profile.
- UI preferences.
- Cached task metadata.
- Task history.
- Custom labels.
- Saved filters.
- Engine health history.
- Schema version.

## 21.2 Filesystem Responsibilities

The filesystem should store:

- aria2 executables.
- aria2 configuration files.
- aria2 session files.
- DHT state.
- Application logs.
- Downloaded update archives.
- Diagnostic bundles.
- Exported settings.

## 21.3 Database Rules

- All schema changes require migrations.
- Migrations must be transactional where possible.
- The database must not be accessed directly from UI components.
- Long database operations must run outside the render path.
- Corruption recovery should preserve the original database before repair.
- Sensitive credentials should not be stored directly in SQLite when a credential store is available.

------

# 22. Data Directory Modes

## 22.1 System Mode

Recommended platform locations:

- Windows: user-local application data.
- macOS: Application Support.
- Linux: XDG data and configuration directories.

## 22.2 Portable Mode

Portable mode may be activated by:

- A `portable.flag` file beside the executable.
- A command-line argument such as `--data-dir`.
- A portable distribution package.

Example:

```text
AriaDeck/
├── AriaDeck.exe
├── portable.flag
└── data/
    ├── cores/
    ├── profiles/
    ├── database/
    ├── logs/
    └── settings/
```

The application should clearly display the active data directory in settings and diagnostics.

------

# 23. Security Design

## 23.1 Local RPC Security

Managed aria2 instances should:

- Listen only on loopback.
- Use a randomly generated RPC secret.
- Prefer a dynamically selected port.
- Avoid exposing RPC to the local network by default.
- Avoid writing the temporary secret to logs.

## 23.2 Remote RPC Security

For remote connections:

- Prefer `wss` or `https`.
- Warn before using unencrypted remote RPC.
- Store secrets in the platform credential store.
- Allow certificate validation errors to be inspected.
- Do not silently disable TLS verification.
- Support explicit custom certificate trust only as an advanced option.

## 23.3 Core Download Security

Managed engine packages must be verified.

Minimum requirements:

- HTTPS transport.
- Expected SHA-256 from a trusted manifest.
- Safe archive extraction.
- Rejection of absolute paths and path traversal.
- Validation of the extracted executable.
- Atomic installation.

A future version may add signed manifests.

## 23.4 Path Safety

Before deleting files, AriaDeck must:

- Resolve the intended task paths.
- Prevent deletion outside the expected download directory unless explicitly confirmed.
- Avoid following unexpected symbolic links where unsafe.
- Display the exact deletion scope.
- Distinguish metadata removal from file deletion.

## 23.5 Log Redaction

Logs must redact:

- RPC secrets.
- Authentication headers.
- URLs containing embedded credentials.
- Proxy passwords.
- Private tracker passkeys where detectable.
- Local paths when the user requests a privacy-reduced diagnostic export.

------

# 24. Error Handling

Errors should be categorized rather than displayed as raw strings.

```rust
pub enum AppError {
    Rpc(RpcError),
    Engine(EngineError),
    CoreInstall(CoreInstallError),
    Storage(StorageError),
    Configuration(ConfigurationError),
    Permission(PermissionError),
    Network(NetworkError),
    Validation(ValidationError),
}
```

Each error should provide:

- A user-facing summary.
- Technical details.
- A stable error code.
- Whether retry is possible.
- Suggested recovery actions.
- Source error for diagnostics.

The UI should avoid generic messages such as “Something went wrong” when a specific recovery action is available.

------

# 25. Offline and Disconnected Behavior

When the RPC connection is lost:

- Preserve the last known task list.
- Mark data as stale.
- Disable commands that require the engine.
- Keep local navigation and settings available.
- Show reconnect progress.
- Allow the user to edit connection settings.
- Avoid clearing the task list immediately.

When reconnected:

1. Verify the engine identity.
2. Fetch global state.
3. Reconcile known tasks.
4. Apply inserted, updated, and removed task patches.
5. Restore the selected task where possible.

------

# 26. Multi-Profile and Multi-Engine Strategy

The initial release may allow one active profile per window or application instance.

A later version may support multiple simultaneous engines.

The architecture should therefore avoid global singleton assumptions.

Recommended identifier hierarchy:

```text
ProfileId
EngineSessionId
Gid
```

A task identity across engines should use:

```rust
pub struct TaskIdentity {
    pub profile_id: ProfileId,
    pub gid: Gid,
}
```

A GID alone is not globally unique across multiple aria2 instances.

------

# 27. System Tray

Optional tray capabilities:

- Show global download and upload speeds.
- Pause all tasks.
- Resume tasks.
- Open AriaDeck.
- Show engine connection state.
- Exit AriaDeck.
- Exit AriaDeck and stop the managed engine.
- Exit AriaDeck while leaving an external or remote engine running.

The shutdown behavior must be explicit because closing the GUI and stopping aria2 are separate actions.

------

# 28. Notifications

Desktop notifications may be generated for:

- Download completed.
- Download failed.
- Engine disconnected.
- Engine crash.
- Managed core update available.
- Managed core update failed.
- Low disk space.

Notification volume should be configurable.

Batch completion should avoid producing one notification per task when many tasks finish together.

------

# 29. Accessibility

AriaDeck should support:

- Keyboard navigation.
- Visible focus indicators.
- Logical focus order.
- Screen-reader labels where supported.
- Sufficient text and control contrast.
- Reduced-motion preferences.
- Scalable text.
- Non-color status indicators.
- Accessible names for icon-only buttons.
- Predictable modal focus trapping.

Task status must not be communicated through color alone.

------

# 30. Localization

The architecture should support localization from the beginning, even if the first release has limited languages.

Rules:

- Do not construct translated sentences through string concatenation.
- Use message identifiers.
- Support pluralization.
- Format numbers and dates through locale-aware helpers.
- Keep technical identifiers such as GIDs copyable.
- Allow compact and expanded byte units.

English should be the source language for interface text and documentation.

------

# 31. Testing Strategy

## 31.1 Unit Tests

Unit tests should cover:

- RPC model conversion.
- Byte and speed formatting.
- ETA calculation.
- Task patch generation.
- Sorting and filtering.
- Version comparison.
- Manifest parsing.
- Path resolution.
- Settings validation.
- Retry and backoff policies.
- Redaction.

## 31.2 RPC Contract Tests

Use recorded or simulated aria2 responses to test:

- Normal responses.
- Missing optional fields.
- Unknown fields.
- RPC errors.
- Authentication failures.
- Batch responses.
- Notification messages.
- Reconnection.

## 31.3 Engine Integration Tests

Run a real aria2 process in controlled tests.

Test:

- Startup.
- RPC readiness.
- Adding a local test download.
- Pausing and resuming.
- Session saving.
- Graceful shutdown.
- Unexpected termination.
- Version detection.

## 31.4 Core Manager Tests

Test:

- Successful installation.
- Invalid hash.
- Corrupt archive.
- Path traversal archive.
- Unsupported platform.
- Existing installation.
- Failed executable validation.
- Rollback.
- Interrupted installation cleanup.

## 31.5 UI Tests

UI tests should cover:

- Task rendering.
- Selection.
- Batch actions.
- Keyboard navigation.
- Dialog behavior.
- Focus restoration.
- Theme switching.
- Disconnected state.
- Large list behavior.
- Long file names.
- Empty and error states.

## 31.6 Performance Tests

Performance scenarios:

- 10,000 completed tasks.
- 1,000 waiting tasks.
- 100 active tasks updating twice per second.
- Rapid filtering.
- Continuous speed chart updates.
- Repeated opening and closing of task details.
- Long-running memory stability.
- Minimized background operation.

Performance tests should track:

- Frame time.
- UI thread blocking time.
- Memory usage.
- RPC payload size.
- Database query duration.
- Number of rendered list rows.
- Update propagation count.

------

# 32. Observability and Diagnostics

Use structured tracing throughout the application.

Suggested spans:

```text
engine.start
engine.health_check
rpc.call
rpc.batch
store.apply_patch
core.install
core.verify
database.migrate
ui.command
```

Diagnostics should include:

- AriaDeck version.
- Operating system.
- Architecture.
- Active profile type.
- aria2 version.
- Detected aria2 capabilities.
- Database schema version.
- Installed managed core versions.
- Connection state history.
- Recent non-sensitive errors.
- Redacted logs.

The diagnostics exporter must exclude credentials by default.

------

# 33. Packaging and Distribution

## 33.1 Windows

Consider:

- Signed executable and installer.
- Per-user installation by default.
- Portable ZIP distribution.
- Native file associations for torrent and Metalink files.
- Optional startup registration.
- Proper process-group cleanup.
- Side-by-side managed aria2 versions outside the application installation directory.

## 33.2 macOS

Consider:

- Signed and notarized application bundle.
- Managed aria2 executables stored outside the signed application bundle.
- Application Support for mutable data.
- Hardened runtime compatibility.
- Native notifications and file dialogs.

## 33.3 Linux

Consider:

- AppImage.
- Flatpak where feasible.
- Distribution packages later.
- XDG directory conventions.
- Wayland and X11 behavior.
- Desktop entry and MIME associations.

Container or sandbox distributions may require special handling for user-selected executables and download paths.

------

# 34. Licensing and Third-Party Compliance

AriaDeck must maintain a third-party notices document.

When distributing aria2 binaries, the release process must include:

- The relevant license text.
- Upstream project attribution.
- The distributed aria2 version.
- Information for obtaining corresponding source code.
- Any required build notices.

AriaDeck licensing should be selected independently, subject to legal review of bundled components and distribution obligations.

------

# 35. Release Strategy

## 35.1 Release Channels

Suggested channels:

- Stable.
- Preview.
- Nightly or development.

## 35.2 Compatibility Policy

Each AriaDeck release should declare:

- Supported operating systems.
- Supported architectures.
- Minimum tested aria2 version.
- Recommended aria2 version.
- Known incompatible builds.
- Database migration behavior.

## 35.3 Rollback

Application updates and aria2 core updates should be treated separately.

A failed aria2 core update must not require rolling back the AriaDeck application.

A failed AriaDeck update should preserve:

- User profiles.
- Managed core installations.
- aria2 session data.
- Download history.
- Settings.

------

# 36. Development Milestones

## Milestone 1: Foundation

Deliverables:

- Rust workspace.
- Domain models.
- Typed settings.
- Logging.
- Basic GPUI window.
- AriaDeck design tokens.
- Initial button, input, and sidebar components.

## Milestone 2: RPC Client

Deliverables:

- WebSocket JSON-RPC transport.
- HTTP fallback.
- Authentication.
- `aria2.getVersion`.
- Global statistics.
- Active, waiting, and stopped task retrieval.
- Notification handling.
- Reconnection.

## Milestone 3: Download Store

Deliverables:

- GID-indexed task store.
- Incremental patches.
- Filtering.
- Sorting.
- Sidebar counters.
- Fixed-capacity speed history.

## Milestone 4: Main Download UI

Deliverables:

- Main window.
- Virtualized task list.
- Download row.
- Task progress.
- Search.
- Task details drawer.
- Add-download dialog.
- Basic task operations.

## Milestone 5: Local Engine Management

Deliverables:

- External executable selection.
- Managed process startup.
- Runtime secret generation.
- Health checks.
- Graceful shutdown.
- Crash recovery.
- Profile-specific configuration and session files.

## Milestone 6: Core Version Manager

Deliverables:

- Version manifest.
- Package download.
- Hash verification.
- Safe extraction.
- Multiple installed versions.
- Activation.
- Automatic rollback.
- Core management UI.

## Milestone 7: Advanced Download Features

Deliverables:

- Torrent file selection.
- Peer and server views.
- Queue reordering.
- Batch operations.
- Task option editing.
- Global option editing.
- Session recovery.

## Milestone 8: Platform Integration

Deliverables:

- System tray.
- Native notifications.
- File associations.
- Autostart.
- Portable mode.
- Packaging for supported platforms.

## Milestone 9: Reliability and Release

Deliverables:

- Integration test suite.
- Performance benchmarks.
- Diagnostics exporter.
- Accessibility review.
- Localization framework.
- Signed release packages.
- Licensing notices.
- User documentation.

------

# 37. Initial MVP Scope

The first usable MVP should include:

- One active aria2 profile.
- Local managed or external aria2 executable.
- WebSocket RPC.
- Active, waiting, stopped, completed, and failed task views.
- Add URL or magnet task.
- Pause, resume, retry, and remove.
- Virtualized task list.
- Search and filtering.
- Basic task details.
- Global speed display.
- Speed chart.
- Configurable download directory.
- Dark and light themes.
- Session persistence.
- Basic engine health monitoring.

The following may be postponed until after the MVP:

- Multiple simultaneous engines.
- Full peer management.
- Advanced proxy UI.
- Application auto-update.
- Preview core update channel.
- Persistent historical analytics.
- Custom tags.
- Remote path mapping.
- Complex automation rules.

------

# 38. Key Technical Risks

## 38.1 GPUI API Evolution

Mitigation:

- Pin known-good dependency revisions.
- Keep GPUI-specific code inside `ariadeck-ui` and the desktop application.
- Prevent business logic from depending on GPUI types.
- Maintain component-level visual tests.

## 38.2 Large Task Collections

Mitigation:

- Virtualization from the first implementation.
- GID-indexed state.
- Incremental patches.
- Cached derived views.
- Lightweight RPC projections.
- Paginated stopped-task loading.

## 38.3 aria2 Build Variability

Mitigation:

- Detect capabilities.
- Validate selected executables.
- Show compatibility information.
- Maintain tested managed builds.
- Provide HTTP fallback where WebSocket is unavailable.

## 38.4 Process and Session Corruption

Mitigation:

- Atomic writes.
- Session backups.
- Clean shutdown.
- Health checks.
- Recovery mode.
- Previous working core version.

## 38.5 Cross-Platform Differences

Mitigation:

- Isolate platform services.
- Avoid embedding mutable engine binaries in signed application bundles.
- Test path and permission behavior on each platform.
- Use native integrations behind stable traits.

------

# 39. Architectural Decisions

The following decisions are accepted for the initial design:

1. The product name is **AriaDeck**.
2. AriaDeck will use Rust.
3. AriaDeck will use GPUI for its native desktop interface.
4. AriaDeck will define its own UI design system.
5. Application pages will depend only on `ariadeck-ui`.
6. aria2 will run as an independent process or remote service.
7. AriaDeck will communicate through JSON-RPC.
8. WebSocket will be the preferred RPC transport.
9. AriaDeck will support managed, external, and remote engine sources.
10. Managed aria2 versions will be installed side by side.
11. Engine binaries and user profile data will remain separate.
12. Task state will be indexed by GID and updated incrementally.
13. Large lists will use virtualization from the beginning.
14. Secrets will be redacted and stored securely where possible.
15. Application updates and aria2 core updates will be independent.

------

# 40. Final Architecture Summary

AriaDeck will be a native Rust desktop application with a carefully separated architecture.

```text
GPUI
  ↓
AriaDeck Design System
  ↓
AriaDeck Pages and Domain Components
  ↓
Application Services
  ↓
Incremental Download Store
  ↓
Typed aria2 RPC Client
  ↓
Managed, External, or Remote aria2 Engine
```

The design intentionally avoids coupling the product to:

- A fixed aria2 executable.
- A fixed aria2 version.
- A single installation path.
- A single operating system.
- A third-party component library.
- A WebView.
- A complete state replacement model.

The most important implementation rules are:

- Keep aria2 independent.
- Keep business logic independent of GPUI.
- Wrap all UI components behind `ariadeck-ui`.
- Use virtualization for all large collections.
- Apply task updates incrementally by GID.
- Keep mutable profile data separate from engine versions.
- Verify all managed engine packages.
- Treat local, external, and remote engines as first-class configurations.
- Design failure recovery as part of the normal product experience.
- Prioritize consistency and responsiveness over decorative complexity.

AriaDeck should ultimately provide the flexibility of aria2 with the quality, performance, and usability of a modern native desktop application.