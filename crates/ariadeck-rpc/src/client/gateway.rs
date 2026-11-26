use ariadeck_application::{
    AddDownloadRequest, AddDownloadSource, DownloadEngineGateway, DownloadProxyConfig,
    GatewayError, GatewayErrorKind, QueueMove, TaskConnectionDetailsGateway, TaskDetailsGateway,
    TaskRemovalTarget,
};
use ariadeck_domain::{
    EnginePath, Gid, SpeedLimitConfig, TaskConnectionDetails, TaskDetails, TaskEditableState,
    TransferPolicyConfig,
};
use async_trait::async_trait;

use crate::{RpcError, RpcTransport, error::is_unauthorized_message};

use super::{
    Aria2Client, RetryRpcError,
    options::{
        bt_tracker_options, download_proxy_options, speed_limit_options, transfer_policy_options,
    },
};

impl<T> Aria2Client<T>
where
    T: RpcTransport,
{
    async fn remove_task(
        &self,
        gid: Gid,
        target: TaskRemovalTarget,
        force: bool,
    ) -> Result<(), GatewayError> {
        match target {
            TaskRemovalTarget::LiveTask => {
                let result = if force {
                    Aria2Client::force_remove(self, gid).await
                } else {
                    Aria2Client::remove(self, gid).await
                };
                match result {
                    Ok(_) => Ok(()),
                    Err(error) if live_task_became_stopped(&error) => {
                        self.remove_download_result(gid).await
                    }
                    Err(error) => Err(error),
                }
            }
            // A stopped result has no live peers to force-teardown; the ordinary
            // result-removal path is the only supported contract.
            TaskRemovalTarget::DownloadResult => self.remove_download_result(gid).await,
        }
        .map_err(map_mutation_error)
    }
}

#[async_trait]
impl<T> DownloadEngineGateway for Aria2Client<T>
where
    T: RpcTransport,
{
    async fn add_download(&self, request: &AddDownloadRequest) -> Result<Vec<Gid>, GatewayError> {
        let result = match &request.source {
            AddDownloadSource::Uris(_) => self.add_uri(request).await.map(|gid| vec![gid]),
            AddDownloadSource::Torrent(_) => self.add_torrent(request).await.map(|gid| vec![gid]),
            AddDownloadSource::Metalink(_) => self.add_metalink(request).await,
        };
        result.map_err(map_mutation_error)
    }

    async fn retry_download(
        &self,
        gid: Gid,
        fallback: &AddDownloadRequest,
    ) -> Result<Gid, GatewayError> {
        match self.retry_uri(gid, fallback).await {
            Ok(gid) => Ok(gid),
            Err(RetryRpcError::Query(error)) => Err(map_query_error(error)),
            Err(RetryRpcError::Mutation(error)) => Err(map_mutation_error(error)),
        }
    }

    async fn pause(&self, gid: Gid) -> Result<(), GatewayError> {
        Aria2Client::pause(self, gid)
            .await
            .map(|_| ())
            .map_err(map_mutation_error)
    }

    async fn force_pause(&self, gid: Gid) -> Result<(), GatewayError> {
        Aria2Client::force_pause(self, gid)
            .await
            .map(|_| ())
            .map_err(map_mutation_error)
    }

    async fn resume(&self, gid: Gid) -> Result<(), GatewayError> {
        Aria2Client::resume(self, gid)
            .await
            .map(|_| ())
            .map_err(map_mutation_error)
    }

    async fn pause_all(&self) -> Result<(), GatewayError> {
        Aria2Client::pause_all(self)
            .await
            .map_err(map_mutation_error)
    }

    async fn force_pause_all(&self) -> Result<(), GatewayError> {
        Aria2Client::force_pause_all(self)
            .await
            .map_err(map_mutation_error)
    }

    async fn resume_all(&self) -> Result<(), GatewayError> {
        Aria2Client::resume_all(self)
            .await
            .map_err(map_mutation_error)
    }

    async fn move_in_queue(&self, gid: Gid, movement: QueueMove) -> Result<(), GatewayError> {
        Aria2Client::move_in_queue(self, gid, movement)
            .await
            .map(|_| ())
            .map_err(map_mutation_error)
    }

    async fn change_options(
        &self,
        gid: Gid,
        options: &[(String, String)],
    ) -> Result<(), GatewayError> {
        Aria2Client::change_options(self, gid, options)
            .await
            .map_err(map_mutation_error)
    }

    async fn get_uris(&self, gid: Gid) -> Result<Vec<String>, GatewayError> {
        match Aria2Client::get_uris(self, gid).await {
            Ok(uris) => Ok(uris),
            Err(error) if super::decode::is_no_uri_data(&error) => Ok(Vec::new()),
            Err(error) => Err(map_query_error(error)),
        }
    }

    async fn change_uri(
        &self,
        gid: Gid,
        file_index: u32,
        del_uris: &[String],
        add_uris: &[String],
        position: Option<u32>,
    ) -> Result<(u32, u32), GatewayError> {
        Aria2Client::change_uri(self, gid, file_index, del_uris, add_uris, position)
            .await
            .map_err(map_mutation_error)
    }

    async fn relink_error_download(
        &self,
        gid: Gid,
        new_uris: &[String],
        override_options: &[(String, String)],
        fallback_dir: Option<&EnginePath>,
    ) -> Result<Gid, GatewayError> {
        match Aria2Client::relink_error_download(
            self,
            gid,
            new_uris,
            override_options,
            fallback_dir,
        )
        .await
        {
            Ok(new_gid) => Ok(new_gid),
            Err(RetryRpcError::Query(error)) => Err(map_query_error(error)),
            Err(RetryRpcError::Mutation(error)) => Err(map_mutation_error(error)),
        }
    }

    async fn apply_download_proxy(&self, config: &DownloadProxyConfig) -> Result<(), GatewayError> {
        self.change_global_options(&download_proxy_options(config))
            .await
            .map_err(map_mutation_error)
    }

    async fn apply_speed_limit(&self, config: &SpeedLimitConfig) -> Result<(), GatewayError> {
        self.change_global_options(&speed_limit_options(config))
            .await
            .map_err(map_mutation_error)
    }

    async fn apply_transfer_policy(
        &self,
        config: &TransferPolicyConfig,
    ) -> Result<(), GatewayError> {
        self.change_global_options(&transfer_policy_options(config))
            .await
            .map_err(map_mutation_error)
    }

    async fn apply_bt_tracker(&self, trackers: &[String]) -> Result<(), GatewayError> {
        self.change_global_options(&bt_tracker_options(trackers))
            .await
            .map_err(map_mutation_error)
    }

    async fn remove(&self, gid: Gid, target: TaskRemovalTarget) -> Result<(), GatewayError> {
        self.remove_task(gid, target, false).await
    }

    async fn force_remove(&self, gid: Gid, target: TaskRemovalTarget) -> Result<(), GatewayError> {
        self.remove_task(gid, target, true).await
    }
}

#[async_trait]
impl<T> TaskDetailsGateway for Aria2Client<T>
where
    T: RpcTransport,
{
    async fn task_details(&self, gid: Gid) -> Result<TaskDetails, GatewayError> {
        Aria2Client::task_details(self, gid)
            .await
            .map_err(map_query_error)
    }
}

#[async_trait]
impl<T> TaskConnectionDetailsGateway for Aria2Client<T>
where
    T: RpcTransport,
{
    async fn connection_details(
        &self,
        gid: Gid,
        active: bool,
        is_bittorrent: bool,
    ) -> Result<TaskConnectionDetails, GatewayError> {
        Aria2Client::connection_details(self, gid, active, is_bittorrent)
            .await
            .map_err(map_query_error)
    }

    async fn editable_state(&self, gid: Gid) -> Result<TaskEditableState, GatewayError> {
        Aria2Client::editable_state(self, gid)
            .await
            .map_err(map_query_error)
    }
}

pub(super) fn map_query_error(error: RpcError) -> GatewayError {
    let (kind, retryable) = match &error {
        RpcError::Closed | RpcError::Transport(_) => (GatewayErrorKind::Disconnected, true),
        RpcError::Tls(_) => (GatewayErrorKind::Disconnected, false),
        RpcError::Authentication(_) => (GatewayErrorKind::Authentication, false),
        RpcError::Timeout { .. } => (GatewayErrorKind::Timeout, true),
        RpcError::Remote { .. } if error.is_unauthorized() => {
            (GatewayErrorKind::Authentication, false)
        }
        RpcError::Remote { .. } => (GatewayErrorKind::Rejected, false),
        RpcError::Configuration(_)
        | RpcError::Protocol(_)
        | RpcError::Serialization(_)
        | RpcError::InvalidData { .. } => (GatewayErrorKind::Internal, false),
    };
    GatewayError::new(kind, error.to_string(), retryable)
}

pub(super) fn map_mutation_error(error: RpcError) -> GatewayError {
    let kind = match &error {
        RpcError::Closed
        | RpcError::Transport(_)
        | RpcError::Tls(_)
        | RpcError::Timeout { .. }
        | RpcError::Protocol(_)
        | RpcError::InvalidData { .. } => GatewayErrorKind::OutcomeUnknown,
        RpcError::Authentication(_) => GatewayErrorKind::Authentication,
        RpcError::Remote { .. } if error.is_unauthorized() => GatewayErrorKind::Authentication,
        RpcError::Remote { .. } => GatewayErrorKind::Rejected,
        RpcError::Configuration(_) | RpcError::Serialization(_) => GatewayErrorKind::Internal,
    };
    GatewayError::new(kind, error.to_string(), false)
}

fn live_task_became_stopped(error: &RpcError) -> bool {
    let RpcError::Remote { message, .. } = error else {
        return false;
    };
    if is_unauthorized_message(message) {
        return false;
    }
    let message = message.to_ascii_lowercase();
    message.contains("not found") || message.contains("not active") || message.contains("stopped")
}
