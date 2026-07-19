//! UI-independent application services, ports, and incremental state.

mod commands;
mod history;
mod ports;
mod store;
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
pub use view::{TaskCounts, TaskListQuery, TaskListView};
