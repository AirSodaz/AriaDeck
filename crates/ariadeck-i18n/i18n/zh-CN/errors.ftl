## 应用 / RPC 错误码（简体中文）
## 键与 ApplicationErrorCode::as_str() 对应（点号改为连字符）。

error-validation-invalid-request = 请求无效。
error-validation-duplicate-task = 该下载已存在。
error-command-wrong-profile = 该任务属于其他引擎配置。
error-command-stale-session = 该任务已不在当前引擎会话中。
error-rpc-disconnected = 已与 aria2 断开连接。
error-rpc-command-outcome-unknown = 命令结果未知；已重新同步引擎状态。
error-rpc-add-not-observed = 刷新后未观察到新的匹配任务。
error-rpc-retry-not-observed = 刷新后未观察到新的重试任务。
error-rpc-remove-not-observed = 刷新后未确认任务已移除。
error-rpc-authentication-failed = aria2 认证失败。
error-rpc-timeout = 请求 aria2 超时。
error-rpc-command-rejected = aria2 拒绝了该命令。
error-command-unsupported = 当前 aria2 构建不支持该命令。
error-filesystem-unsafe-path = 路径超出托管下载根目录。
error-filesystem-operation-failed = 本地文件系统操作失败。
error-application-internal = 发生内部应用错误。
error-sync-unavailable = 同步协调器不可用。
error-command-no-result = 命令未返回结果。
