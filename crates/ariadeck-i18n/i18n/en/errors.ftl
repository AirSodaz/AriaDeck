## Application / RPC error codes (English)
## Keys match ApplicationErrorCode::as_str() with dots replaced by hyphens.

error-validation-invalid-request = Invalid request.
error-validation-duplicate-task = This download already exists.
error-command-wrong-profile = The task belongs to a different engine profile.
error-command-stale-session = The task is no longer present in the current engine session.
error-rpc-disconnected = Disconnected from aria2.
error-rpc-command-outcome-unknown = The command result is unknown; refreshed engine state once.
error-rpc-add-not-observed = aria2 did not report a new matching task after refresh.
error-rpc-retry-not-observed = aria2 did not report a new retry task after refresh.
error-rpc-remove-not-observed = Task removal was not confirmed after refresh.
error-rpc-authentication-failed = aria2 authentication failed.
error-rpc-timeout = The request to aria2 timed out.
error-rpc-command-rejected = aria2 rejected the command.
error-command-unsupported = This aria2 build does not support the requested command.
error-filesystem-unsafe-path = The path is outside the managed download roots.
error-filesystem-operation-failed = A local filesystem operation failed.
error-application-internal = An internal application error occurred.
error-sync-unavailable = The synchronization coordinator is unavailable.
error-command-no-result = The command returned no result.
