use std::{
    collections::HashMap,
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use ariadeck_domain::{Gid, ProfileId, SessionGeneration, TaskSnapshot};
use tokio::sync::{broadcast, mpsc, oneshot};

use crate::{DownloadStore, HistoryRecord, TaskHistoryStore};

use super::{
    connected::{emit_global_update, emit_patch},
    types::{EngineCapabilities, InitialSyncSnapshot, SyncError, SyncEvent, validate_capabilities},
};

fn now_unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

pub(super) async fn persist_task_category(
    history: &Arc<dyn TaskHistoryStore>,
    profile_id: ProfileId,
    gid: Gid,
    category_id: Option<String>,
) {
    let history = Arc::clone(history);
    let result = tokio::task::spawn_blocking(move || {
        history.set_task_category(profile_id, gid, category_id.as_deref())
    })
    .await;
    match result {
        Ok(Ok(())) => {}
        Ok(Err(error)) => {
            tracing::warn!(%error, %gid, "failed to persist task category");
        }
        Err(error) => {
            tracing::warn!(%error, %gid, "task category persistence task failed");
        }
    }
}

#[derive(Clone, Eq, PartialEq)]
pub(super) struct TerminalHistoryFingerprint {
    status: ariadeck_domain::DownloadStatus,
    display_name: String,
    total_length: ariadeck_domain::ByteCount,
    completed_length: ariadeck_domain::ByteCount,
    error: Option<ariadeck_domain::TaskError>,
}

pub(super) struct PendingHistoryBatch {
    pub(super) records: Vec<HistoryRecord>,
    fingerprints: Vec<(Gid, TerminalHistoryFingerprint)>,
}

impl PendingHistoryBatch {
    pub(super) fn is_empty(&self) -> bool {
        self.records.is_empty()
    }

    pub(super) fn into_parts(self) -> (Vec<HistoryRecord>, Vec<(Gid, TerminalHistoryFingerprint)>) {
        (self.records, self.fingerprints)
    }
}

#[derive(Default)]
pub(super) struct HistoryWriteTracker {
    persisted: HashMap<Gid, TerminalHistoryFingerprint>,
}

impl HistoryWriteTracker {
    pub(super) fn forget(&mut self, gids: &[Gid]) {
        for gid in gids {
            self.persisted.remove(gid);
        }
    }

    /// Compute changed terminal records without marking them persisted yet.
    /// Call [`Self::commit_enqueued`] only after the batch is accepted by the
    /// writer channel so a full queue retries on the next tick instead of
    /// permanently dropping the batch.
    pub(super) fn diff_changed_records(
        &self,
        profile_id: ProfileId,
        tasks: &[TaskSnapshot],
    ) -> PendingHistoryBatch {
        let now = now_unix_ms();
        let mut records = Vec::new();
        let mut fingerprints = Vec::new();
        for snapshot in tasks {
            if !matches!(
                snapshot.status,
                ariadeck_domain::DownloadStatus::Complete | ariadeck_domain::DownloadStatus::Error
            ) {
                continue;
            }
            let fingerprint = TerminalHistoryFingerprint {
                status: snapshot.status,
                display_name: snapshot.display_name.clone(),
                total_length: snapshot.total_length,
                completed_length: snapshot.completed_length,
                error: snapshot.error.clone(),
            };
            if self.persisted.get(&snapshot.gid) == Some(&fingerprint) {
                continue;
            }
            if let Some(record) = HistoryRecord::from_snapshot(profile_id, snapshot, now) {
                records.push(record);
                fingerprints.push((snapshot.gid, fingerprint));
            }
        }
        PendingHistoryBatch {
            records,
            fingerprints,
        }
    }

    pub(super) fn commit_enqueued(&mut self, fingerprints: Vec<(Gid, TerminalHistoryFingerprint)>) {
        if self.persisted.len().saturating_add(fingerprints.len()) >= 12_000 {
            self.persisted.clear();
        }
        for (gid, fingerprint) in fingerprints {
            self.persisted.insert(gid, fingerprint);
        }
    }
}

enum HistoryWriteCmd {
    UpsertBatch(Vec<HistoryRecord>),
    RemoveBatch {
        profile_id: ProfileId,
        gids: Vec<Gid>,
        ack: oneshot::Sender<()>,
    },
    SetTaskCategory {
        profile_id: ProfileId,
        gid: Gid,
        category_id: Option<String>,
        ack: oneshot::Sender<()>,
    },
    Flush(oneshot::Sender<()>),
}

pub(super) struct HistoryWriter {
    tx: Option<mpsc::Sender<HistoryWriteCmd>>,
    handle: Option<tokio::task::JoinHandle<()>>,
}

impl HistoryWriter {
    pub(super) fn spawn(history: Arc<dyn TaskHistoryStore>) -> Self {
        let (tx, mut rx) = mpsc::channel::<HistoryWriteCmd>(64);
        let handle = tokio::spawn(async move {
            while let Some(command) = rx.recv().await {
                match command {
                    HistoryWriteCmd::UpsertBatch(records) => {
                        let history = Arc::clone(&history);
                        match tokio::task::spawn_blocking(move || history.upsert_batch(&records))
                            .await
                        {
                            Ok(Ok(())) => {}
                            Ok(Err(error)) => {
                                tracing::warn!(%error, "failed to persist task history batch");
                            }
                            Err(error) => {
                                tracing::warn!(%error, "task history persistence task failed");
                            }
                        }
                    }
                    HistoryWriteCmd::RemoveBatch {
                        profile_id,
                        gids,
                        ack,
                    } => {
                        delete_history_for_gids(&history, profile_id, gids).await;
                        let _ = ack.send(());
                    }
                    HistoryWriteCmd::SetTaskCategory {
                        profile_id,
                        gid,
                        category_id,
                        ack,
                    } => {
                        persist_task_category(&history, profile_id, gid, category_id).await;
                        let _ = ack.send(());
                    }
                    HistoryWriteCmd::Flush(ack) => {
                        let _ = ack.send(());
                    }
                }
            }
        });
        Self {
            tx: Some(tx),
            handle: Some(handle),
        }
    }

    pub(super) fn try_upsert_batch(&self, records: Vec<HistoryRecord>) -> bool {
        let Some(tx) = self.tx.as_ref() else {
            return false;
        };
        tx.try_send(HistoryWriteCmd::UpsertBatch(records)).is_ok()
    }

    pub(super) async fn remove_batch(
        &self,
        history: &Arc<dyn TaskHistoryStore>,
        profile_id: ProfileId,
        gids: Vec<Gid>,
    ) {
        if gids.is_empty() {
            return;
        }
        if let Some(tx) = self.tx.as_ref() {
            let (ack, done) = oneshot::channel();
            if tx
                .send(HistoryWriteCmd::RemoveBatch {
                    profile_id,
                    gids: gids.clone(),
                    ack,
                })
                .await
                .is_ok()
            {
                let _ = done.await;
                return;
            }
        }
        delete_history_for_gids(history, profile_id, gids).await;
    }

    pub(super) async fn set_task_category(
        &self,
        history: &Arc<dyn TaskHistoryStore>,
        profile_id: ProfileId,
        gid: Gid,
        category_id: Option<String>,
    ) {
        if let Some(tx) = self.tx.as_ref() {
            let (ack, done) = oneshot::channel();
            if tx
                .send(HistoryWriteCmd::SetTaskCategory {
                    profile_id,
                    gid,
                    category_id: category_id.clone(),
                    ack,
                })
                .await
                .is_ok()
            {
                let _ = done.await;
                return;
            }
        }
        persist_task_category(history, profile_id, gid, category_id).await;
    }

    pub(super) async fn flush(&self) {
        if let Some(tx) = self.tx.as_ref() {
            let (ack, done) = oneshot::channel();
            if tx.send(HistoryWriteCmd::Flush(ack)).await.is_ok() {
                let _ = done.await;
            }
        }
    }

    pub(super) async fn shutdown(mut self) {
        self.tx.take();
        if let Some(handle) = self.handle.take()
            && tokio::time::timeout(Duration::from_secs(2), handle)
                .await
                .is_err()
        {
            tracing::warn!("history writer task did not drain within 2 seconds");
        }
    }
}

pub(super) async fn persist_terminal_snapshots(
    history: &Arc<dyn TaskHistoryStore>,
    profile_id: ProfileId,
    tasks: impl IntoIterator<Item = TaskSnapshot>,
) {
    let now = now_unix_ms();
    let records: Vec<HistoryRecord> = tasks
        .into_iter()
        .filter_map(|snapshot| HistoryRecord::from_snapshot(profile_id, &snapshot, now))
        .collect();
    if records.is_empty() {
        return;
    }
    let history = Arc::clone(history);
    match tokio::task::spawn_blocking(move || history.upsert_batch(&records)).await {
        Ok(Ok(())) => {}
        Ok(Err(error)) => tracing::warn!(%error, "failed to persist task history batch"),
        Err(error) => tracing::warn!(%error, "task history persistence task failed"),
    }
}

pub(super) async fn seed_initial_history_from_disk(
    store: &mut DownloadStore,
    generation: SessionGeneration,
    history: &Arc<dyn TaskHistoryStore>,
    profile_id: ProfileId,
) {
    let history = Arc::clone(history);
    let loaded = tokio::task::spawn_blocking(move || {
        let records = history.list(profile_id, 10_000);
        let categories = history.list_task_categories(profile_id);
        (records, categories)
    })
    .await;
    let (records_result, categories_result) = match loaded {
        Ok(pair) => pair,
        Err(error) => {
            tracing::warn!(%error, "startup task history task failed");
            return;
        }
    };
    match records_result {
        Ok(records) => {
            if let Err(error) = store.seed_local_history(generation, records) {
                tracing::warn!(%error, "failed to seed local history at startup");
            }
        }
        Err(error) => tracing::warn!(%error, "failed to load local task history at startup"),
    }
    match categories_result {
        Ok(categories) => store.set_category_affiliations(categories),
        Err(error) => {
            tracing::warn!(%error, "failed to load task category affiliations at startup")
        }
    }
}

pub(super) async fn seed_history_into_store(
    store: &mut DownloadStore,
    generation: SessionGeneration,
    history: &Arc<dyn TaskHistoryStore>,
    profile_id: ProfileId,
    events: &broadcast::Sender<SyncEvent>,
) {
    let history = Arc::clone(history);
    let loaded = tokio::task::spawn_blocking(move || {
        let records = history.list(profile_id, 10_000);
        let categories = history.list_task_categories(profile_id);
        (records, categories)
    })
    .await;
    let (records_result, categories_result) = match loaded {
        Ok(pair) => pair,
        Err(error) => {
            tracing::warn!(%error, "local task history load task failed");
            return;
        }
    };
    match records_result {
        Ok(records) => match store.seed_local_history(generation, records) {
            Ok(patch) => emit_patch(events, patch),
            Err(error) => {
                tracing::warn!(%error, "failed to seed local history into store");
            }
        },
        Err(error) => {
            tracing::warn!(%error, "failed to load local task history");
        }
    }
    match categories_result {
        Ok(affiliations) => store.set_category_affiliations(affiliations),
        Err(error) => {
            tracing::warn!(%error, "failed to load task category affiliations");
        }
    }
}

pub(super) async fn delete_history_for_gids(
    history: &Arc<dyn TaskHistoryStore>,
    profile_id: ProfileId,
    gids: Vec<Gid>,
) {
    if gids.is_empty() {
        return;
    }
    let history = Arc::clone(history);
    match tokio::task::spawn_blocking(move || history.remove_batch(profile_id, &gids)).await {
        Ok(Ok(())) => {}
        Ok(Err(error)) => tracing::warn!(%error, "failed to remove task history batch"),
        Err(error) => tracing::warn!(%error, "task history removal task failed"),
    }
}

pub(super) async fn apply_initial_snapshot(
    store: &mut DownloadStore,
    generation: SessionGeneration,
    initial: InitialSyncSnapshot,
    events: &broadcast::Sender<SyncEvent>,
    history: &Arc<dyn TaskHistoryStore>,
    profile_id: ProfileId,
) -> Result<EngineCapabilities, SyncError> {
    validate_capabilities(&initial.capabilities)?;
    let capabilities = initial.capabilities.clone();
    let _ = events.send(SyncEvent::CapabilitiesChanged(capabilities.clone()));
    store
        .record_speed_sample(generation, initial.global_stat)
        .map_err(SyncError::store)?;
    emit_global_update(
        events,
        store
            .update_global_stat(generation, initial.global_stat)
            .map_err(SyncError::store)?,
    );
    persist_terminal_snapshots(history, profile_id, initial.stopped.tasks.iter().cloned()).await;
    emit_patch(
        events,
        store
            .apply_stopped_page(
                generation,
                initial.stopped.offset,
                Some(initial.stopped.total),
                initial.stopped.tasks,
            )
            .map_err(SyncError::store)?,
    );
    emit_patch(
        events,
        store
            .reconcile_live(generation, initial.live.active, initial.live.waiting)
            .map_err(SyncError::store)?,
    );
    seed_history_into_store(store, generation, history, profile_id, events).await;
    emit_patch(
        events,
        store
            .prune_unreferenced_tasks(generation)
            .map_err(SyncError::store)?,
    );
    emit_patch(
        events,
        store
            .set_stale(generation, false)
            .map_err(SyncError::store)?,
    );
    Ok(capabilities)
}
