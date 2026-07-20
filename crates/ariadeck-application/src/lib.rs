//! UI-independent application services, ports, and incremental state.

mod commands;
mod history;
mod ports;
mod store;
mod sync;
mod view;

pub use commands::{
    AddDownloadRequest, AddDownloadSource, AppCommand, ApplicationError, ApplicationErrorCode,
    CommandItem, CommandOutcome, CommandService, DownloadProxyConfig, DownloadProxyMode,
    FileConflictPolicy, ItemFailure, RemoveTasksRequest, SetTaskOutputNameRequest,
    TaskCommandContext, TaskRemovalScope,
};
pub use history::{DEFAULT_SPEED_HISTORY_CAPACITY, SpeedHistory, SpeedHistoryError, SpeedSample};
pub use ports::{
    DownloadDestinationFile, DownloadDestinationGateway, DownloadDestinationReport,
    DownloadDestinationRequest, DownloadEngineGateway, GatewayError, GatewayErrorKind,
    TaskDetailsGateway, TaskFileGateway, TaskFileRemovalPreview, TaskFileRemovalReport,
    TaskFileRemovalRequest, TaskRemovalTarget,
};
pub use store::{
    DownloadStore, OrderPatch, StoreError, StorePatch, TaskCollection, TaskFieldPatch,
};
pub use sync::{
    ActivityMode, ConnectedSyncSession, CoordinatorConfig, DownloadSyncConnector,
    DownloadSyncSession, EngineCapabilities, InitialSyncSnapshot, LiveSyncSnapshot, PollIntervals,
    ReconnectPolicy, RefreshHint, RefreshPolicy, StoppedPage, StoreSnapshot, SyncError,
    SyncErrorKind, SyncEvent, SyncHandle, spawn_sync_coordinator,
};
pub use view::{TaskCounts, TaskListQuery, TaskListView};
