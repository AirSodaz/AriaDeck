//! UI-independent application services, ports, and incremental state.

mod commands;
mod history;
mod ports;
mod store;
mod sync;
mod view;

pub use commands::{
    AddDownloadAdvancedOptions, AddDownloadRequest, AddDownloadSource, AppCommand,
    ApplicationError, ApplicationErrorCode, CommandItem, CommandOutcome, CommandService,
    DownloadProxyConfig, DownloadProxyMode, FileConflictPolicy, ItemFailure,
    MoveTaskInQueueRequest, QueueMove, RemoveTasksRequest, SetTaskOptionsRequest,
    SetTaskOutputNameRequest, SetTaskSpeedLimitRequest, TaskCommandContext, TaskRemovalScope,
};
pub use history::{DEFAULT_SPEED_HISTORY_CAPACITY, SpeedHistory, SpeedHistoryError, SpeedSample};
pub use ports::{
    DownloadDestinationFile, DownloadDestinationGateway, DownloadDestinationReport,
    DownloadDestinationRequest, DownloadEngineGateway, GatewayError, GatewayErrorKind,
    TaskConnectionDetailsGateway, TaskDetailsGateway, TaskFileGateway, TaskFileRemovalPreview,
    TaskFileRemovalReport, TaskFileRemovalRequest, TaskOpenRequest, TaskOpenTarget,
    TaskRemovalTarget,
};
pub use store::{
    DownloadStore, OrderPatch, StoppedHistoryState, StoreError, StorePatch, TaskCollection,
    TaskFieldPatch,
};
pub use sync::{
    ActivityMode, ConnectedSyncSession, CoordinatorConfig, DownloadSyncConnector,
    DownloadSyncSession, EngineCapabilities, InitialSyncSnapshot, LiveSyncSnapshot, PollIntervals,
    ReconnectPolicy, RefreshHint, RefreshPolicy, StoppedPage, StoreSnapshot, SyncError,
    SyncErrorKind, SyncEvent, SyncHandle, spawn_sync_coordinator,
};
pub use view::{TaskCounts, TaskListQuery, TaskListView};
