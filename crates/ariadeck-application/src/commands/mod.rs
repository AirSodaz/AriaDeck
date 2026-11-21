mod outcome;
mod requests;
mod service;

#[cfg(test)]
mod tests;

pub use outcome::{
    ApplicationError, ApplicationErrorCode, CommandItem, CommandOutcome, ItemFailure,
};
pub use requests::{
    AddDownloadAdvancedOptions, AddDownloadRequest, AddDownloadSource, AppCommand,
    DownloadProxyConfig, DownloadProxyMode, FileConflictPolicy, MoveTaskInQueueRequest, QueueMove,
    RemoveTasksRequest, SetTaskConnectionPolicyRequest, SetTaskOptionsRequest,
    SetTaskOutputNameRequest, SetTaskSpeedLimitRequest, TaskCommandContext, TaskRemovalScope,
    format_selected_file_indices,
};
pub use service::CommandService;
