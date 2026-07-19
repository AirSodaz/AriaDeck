//! UI-independent application services, ports, and incremental state.

mod commands;
mod history;
mod ports;
mod store;
mod sync;
mod view;

pub use commands::{
    AddDownloadRequest, AppCommand, ApplicationError, ApplicationErrorCode, CommandItem,
    CommandOutcome, CommandService, ItemFailure, RemoveTasksRequest,
};
pub use history::{SpeedHistory, SpeedHistoryError, SpeedSample};
pub use ports::{DownloadEngineGateway, GatewayError, GatewayErrorKind};
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
