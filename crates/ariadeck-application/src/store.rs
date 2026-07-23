use std::{
    collections::{BTreeMap, HashMap, HashSet},
    time::Instant,
};

use ariadeck_domain::{
    DownloadStatus, DownloadTask, EngineSession, Gid, GlobalStat, SessionGeneration, TaskFields,
    TaskMetadata, TaskNameState, TaskSnapshot, TaskUpdateError,
};
use thiserror::Error;

use crate::{HistoryRecord, SpeedHistory, SpeedSample};

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum TaskCollection {
    Active,
    Waiting,
    Stopped,
}

/// Progress of stopped-result pages loaded from aria2 into the local cache.
///
/// `total` is aria2's in-memory result count (`numStoppedTotal`), which is
/// itself bounded by the engine's `--max-download-result` setting. Durable
/// completed/failed rows may also live in local history (B6 / D-039) and are
/// merged for list/count views when the engine no longer holds the result.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct StoppedHistoryState {
    pub loaded: usize,
    pub total: Option<usize>,
    pub next_offset: usize,
    pub can_load_more: bool,
    /// Distinct durable history rows loaded for this profile (local SQLite).
    pub local_saved: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TaskFieldPatch {
    pub gid: Gid,
    pub fields: TaskFields,
    pub task_revision: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OrderPatch {
    pub collection: TaskCollection,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StorePatch {
    pub generation: SessionGeneration,
    pub store_revision: u64,
    pub inserted: Vec<Gid>,
    pub updated: Vec<TaskFieldPatch>,
    pub removed: Vec<Gid>,
    pub order_changes: Vec<OrderPatch>,
    pub global_stat_changed: bool,
    pub stale_changed: bool,
    pub session_changed: bool,
}

impl StorePatch {
    fn new(generation: SessionGeneration, store_revision: u64) -> Self {
        Self {
            generation,
            store_revision,
            inserted: Vec::new(),
            updated: Vec::new(),
            removed: Vec::new(),
            order_changes: Vec::new(),
            global_stat_changed: false,
            stale_changed: false,
            session_changed: false,
        }
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.inserted.is_empty()
            && self.updated.is_empty()
            && self.removed.is_empty()
            && self.order_changes.is_empty()
            && !self.global_stat_changed
            && !self.stale_changed
            && !self.session_changed
    }

    fn record_order(&mut self, collection: TaskCollection) {
        if !self
            .order_changes
            .iter()
            .any(|patch| patch.collection == collection)
        {
            self.order_changes.push(OrderPatch { collection });
        }
    }
}

/// Incremental state owned by one profile and engine-session generation.
pub struct DownloadStore {
    session: EngineSession,
    pub(crate) tasks: HashMap<Gid, DownloadTask>,
    pub(crate) active_order: Vec<Gid>,
    pub(crate) waiting_order: Vec<Gid>,
    pub(crate) stopped_order: Vec<Gid>,
    /// GIDs present only in durable local history (not in engine stopped pages).
    pub(crate) history_order: Vec<Gid>,
    history_only: HashSet<Gid>,
    /// Task → category id affiliations (C1); profile-scoped store instance.
    pub(crate) category_by_gid: HashMap<Gid, String>,
    stopped_pages: BTreeMap<usize, Vec<Gid>>,
    stopped_total: Option<usize>,
    pub(crate) search_index: HashMap<Gid, String>,
    global_stat: GlobalStat,
    speed_history: SpeedHistory,
    speed_history_started_at: Instant,
    seeding_started_at: HashMap<Gid, Instant>,
    stale: bool,
    revision: u64,
}

impl DownloadStore {
    #[must_use]
    pub fn new(session: EngineSession) -> Self {
        Self {
            session,
            tasks: HashMap::new(),
            active_order: Vec::new(),
            waiting_order: Vec::new(),
            stopped_order: Vec::new(),
            history_order: Vec::new(),
            history_only: HashSet::new(),
            category_by_gid: HashMap::new(),
            stopped_pages: BTreeMap::new(),
            stopped_total: None,
            search_index: HashMap::new(),
            global_stat: GlobalStat::default(),
            speed_history: SpeedHistory::default(),
            speed_history_started_at: Instant::now(),
            seeding_started_at: HashMap::new(),
            stale: false,
            revision: 0,
        }
    }

    #[must_use]
    pub const fn session(&self) -> EngineSession {
        self.session
    }

    #[must_use]
    pub const fn revision(&self) -> u64 {
        self.revision
    }

    #[must_use]
    pub const fn is_stale(&self) -> bool {
        self.stale
    }

    #[must_use]
    pub const fn global_stat(&self) -> GlobalStat {
        self.global_stat
    }

    #[must_use]
    pub fn speed_history(&self) -> &SpeedHistory {
        &self.speed_history
    }

    pub fn record_speed_sample(
        &mut self,
        generation: SessionGeneration,
        stat: GlobalStat,
    ) -> Result<(), StoreError> {
        self.ensure_generation(generation)?;
        self.speed_history.push(SpeedSample {
            elapsed: self.speed_history_started_at.elapsed(),
            download: stat.download_speed,
            upload: stat.upload_speed,
        });
        Ok(())
    }

    #[must_use]
    pub fn task(&self, gid: Gid) -> Option<&DownloadTask> {
        self.tasks.get(&gid)
    }

    #[must_use]
    pub fn stopped_total(&self) -> Option<usize> {
        self.stopped_total
    }

    /// Distinct stopped GIDs currently held in the local page cache.
    #[must_use]
    pub fn stopped_loaded(&self) -> usize {
        self.stopped_order.len()
    }

    /// Next `tellStopped` offset for a contiguous page append.
    ///
    /// Pages are stored by their starting offset. When earlier pages have been
    /// loaded without a gap, this is the number of distinct loaded GIDs. When a
    /// gap exists, the lowest missing offset is returned so the caller can fill
    /// it before extending further.
    #[must_use]
    pub fn next_stopped_offset(&self) -> usize {
        let mut expected = 0;
        for (offset, page) in &self.stopped_pages {
            if *offset > expected {
                return expected;
            }
            expected = expected.saturating_add(page.len());
        }
        expected
    }

    /// Whether another stopped page can be requested from the engine.
    #[must_use]
    pub fn can_load_more_stopped(&self) -> bool {
        match self.stopped_total {
            Some(total) => self.next_stopped_offset() < total,
            // Before the first authoritative total arrives, only request the
            // initial page. Further pages wait for `numStoppedTotal`.
            None => self.stopped_pages.is_empty(),
        }
    }

    /// Snapshot of stopped-history loading progress for UI disclosure.
    #[must_use]
    pub fn stopped_history(&self) -> StoppedHistoryState {
        StoppedHistoryState {
            loaded: self.stopped_loaded(),
            total: self.stopped_total,
            next_offset: self.next_stopped_offset(),
            can_load_more: self.can_load_more_stopped(),
            local_saved: self.history_order.len(),
        }
    }

    /// Whether this GID is present only via durable local history.
    #[must_use]
    pub fn is_history_only(&self, gid: Gid) -> bool {
        self.history_only.contains(&gid)
    }

    /// Replace in-memory category affiliations (loaded from SQLite).
    pub fn set_category_affiliations(
        &mut self,
        affiliations: impl IntoIterator<Item = (Gid, String)>,
    ) {
        self.category_by_gid.clear();
        for (gid, category_id) in affiliations {
            if !category_id.is_empty() {
                self.category_by_gid.insert(gid, category_id);
            }
        }
    }

    pub fn set_task_category_affiliation(&mut self, gid: Gid, category_id: Option<String>) {
        match category_id {
            Some(id) if !id.is_empty() => {
                self.category_by_gid.insert(gid, id);
            }
            _ => {
                self.category_by_gid.remove(&gid);
            }
        }
    }

    #[must_use]
    pub fn task_category_id(&self, gid: Gid) -> Option<&str> {
        self.category_by_gid.get(&gid).map(String::as_str)
    }

    /// Seeds durable local history into the in-memory store.
    ///
    /// Engine-owned tasks (live or stopped pages) win over history for the same
    /// GID. History-only rows appear in Completed/Failed filters after restarts
    /// or when aria2 has already purged the result.
    pub fn seed_local_history(
        &mut self,
        generation: SessionGeneration,
        records: Vec<HistoryRecord>,
    ) -> Result<StorePatch, StoreError> {
        self.ensure_generation(generation)?;
        let mut patch = StorePatch::new(generation, self.revision);
        let live: HashSet<Gid> = self
            .active_order
            .iter()
            .chain(&self.waiting_order)
            .copied()
            .collect();
        let engine_stopped: HashSet<Gid> = self.stopped_order.iter().copied().collect();

        let previous_history = self.history_order.clone();
        self.history_order.clear();
        self.history_only.clear();

        for record in records {
            if record.profile_id != self.session.profile_id {
                continue;
            }
            if !matches!(
                record.status,
                DownloadStatus::Complete | DownloadStatus::Error
            ) {
                continue;
            }
            if live.contains(&record.gid) || engine_stopped.contains(&record.gid) {
                // Engine is authoritative for this GID; keep a search hint if missing.
                if !self.tasks.contains_key(&record.gid) {
                    let task = download_task_from_history(&record);
                    self.search_index
                        .insert(record.gid, task.display_name.to_lowercase());
                    self.tasks.insert(record.gid, task);
                    patch.inserted.push(record.gid);
                }
                continue;
            }
            if self.history_only.insert(record.gid) {
                self.history_order.push(record.gid);
            }
            let task = download_task_from_history(&record);
            let search_name = task.display_name.to_lowercase();
            if self.tasks.insert(record.gid, task).is_none() {
                patch.inserted.push(record.gid);
            } else if !patch.updated.iter().any(|item| item.gid == record.gid) {
                patch.updated.push(TaskFieldPatch {
                    gid: record.gid,
                    fields: TaskFields::all(),
                    task_revision: 1,
                });
            }
            self.search_index.insert(record.gid, search_name);
        }

        if previous_history != self.history_order {
            patch.record_order(TaskCollection::Stopped);
        }
        Ok(self.finish_patch(patch))
    }

    /// Removes durable-history-only rows after a user Remove (or explicit purge).
    pub fn remove_history_only(
        &mut self,
        generation: SessionGeneration,
        gids: &[Gid],
    ) -> Result<StorePatch, StoreError> {
        self.ensure_generation(generation)?;
        let remove_set: HashSet<Gid> = gids.iter().copied().collect();
        let mut patch = StorePatch::new(generation, self.revision);
        let previous_history = self.history_order.clone();
        for gid in &remove_set {
            if self.history_only.remove(gid) && self.tasks.remove(gid).is_some() {
                self.search_index.remove(gid);
                patch.removed.push(*gid);
            }
        }
        self.history_order.retain(|gid| !remove_set.contains(gid));
        if previous_history != self.history_order {
            patch.record_order(TaskCollection::Stopped);
        }
        Ok(self.finish_patch(patch))
    }

    /// Application-observed seeding duration for the current engine session.
    ///
    /// aria2 does not expose authoritative elapsed seeding time, so this value
    /// starts when this store first observes `DownloadStatus::Seeding` and is
    /// cleared on state exit, removal, or engine-session change.
    #[must_use]
    pub fn observed_seeding_seconds(&self, gid: Gid) -> Option<u64> {
        self.seeding_started_at
            .get(&gid)
            .map(|started_at| started_at.elapsed().as_secs())
    }

    /// Starts a new connection generation while preserving last-known tasks.
    pub fn begin_session(&mut self, session: EngineSession) -> Result<StorePatch, StoreError> {
        if session.profile_id != self.session.profile_id {
            return Err(StoreError::WrongProfile {
                expected: self.session.profile_id,
                received: session.profile_id,
            });
        }

        let mut patch = StorePatch::new(session.generation, self.revision);
        if self.session != session {
            self.session = session;
            self.seeding_started_at.clear();
            patch.session_changed = true;
        }
        if !self.stale {
            self.stale = true;
            patch.stale_changed = true;
        }
        Ok(self.finish_patch(patch))
    }

    /// Reconciles the two authoritative live collections as one atomic snapshot.
    pub fn reconcile_live(
        &mut self,
        generation: SessionGeneration,
        active: Vec<TaskSnapshot>,
        waiting: Vec<TaskSnapshot>,
    ) -> Result<StorePatch, StoreError> {
        self.ensure_generation(generation)?;
        validate_unique(active.iter().chain(&waiting))?;

        let new_active = active.iter().map(|task| task.gid).collect::<Vec<_>>();
        let new_waiting = waiting.iter().map(|task| task.gid).collect::<Vec<_>>();
        let new_live = new_active
            .iter()
            .chain(&new_waiting)
            .copied()
            .collect::<HashSet<_>>();
        let old_live = self
            .active_order
            .iter()
            .chain(&self.waiting_order)
            .copied()
            .collect::<HashSet<_>>();

        let mut patch = StorePatch::new(generation, self.revision);
        for snapshot in active.into_iter().chain(waiting) {
            self.upsert(snapshot, &mut patch)?;
        }

        let previous_stopped = self.stopped_order.clone();
        for page in self.stopped_pages.values_mut() {
            page.retain(|gid| !new_live.contains(gid));
        }
        self.rebuild_stopped_order();
        if previous_stopped != self.stopped_order {
            patch.record_order(TaskCollection::Stopped);
        }

        let stopped = self.stopped_order.iter().copied().collect::<HashSet<_>>();
        for gid in old_live.difference(&new_live).copied() {
            if !stopped.contains(&gid) && self.tasks.remove(&gid).is_some() {
                self.search_index.remove(&gid);
                self.seeding_started_at.remove(&gid);
                patch.removed.push(gid);
            }
        }

        if self.active_order != new_active {
            self.active_order = new_active;
            patch.record_order(TaskCollection::Active);
        }
        if self.waiting_order != new_waiting {
            self.waiting_order = new_waiting;
            patch.record_order(TaskCollection::Waiting);
        }

        Ok(self.finish_patch(patch))
    }

    /// Applies one stopped-task page without treating absent page entries as deletions.
    pub fn apply_stopped_page(
        &mut self,
        generation: SessionGeneration,
        offset: usize,
        total: Option<usize>,
        tasks: Vec<TaskSnapshot>,
    ) -> Result<StorePatch, StoreError> {
        self.ensure_generation(generation)?;
        validate_unique(tasks.iter())?;

        let mut page_gids = tasks.iter().map(|task| task.gid).collect::<Vec<_>>();
        if let Some(total) = total {
            if offset > total {
                return Err(StoreError::InvalidStoppedPage { offset, total });
            }
            page_gids.truncate(total.saturating_sub(offset));
        }
        let page_set = page_gids.iter().copied().collect::<HashSet<_>>();
        let mut patch = StorePatch::new(generation, self.revision);

        for snapshot in tasks.into_iter().take(page_gids.len()) {
            self.upsert(snapshot, &mut patch)?;
        }

        let previous_active = self.active_order.clone();
        let previous_waiting = self.waiting_order.clone();
        self.active_order.retain(|gid| !page_set.contains(gid));
        self.waiting_order.retain(|gid| !page_set.contains(gid));
        if previous_active != self.active_order {
            patch.record_order(TaskCollection::Active);
        }
        if previous_waiting != self.waiting_order {
            patch.record_order(TaskCollection::Waiting);
        }

        let previous_stopped = self.stopped_order.clone();
        self.stopped_pages.insert(offset, page_gids);
        if let Some(total) = total {
            self.stopped_total = Some(total);
            self.stopped_pages
                .retain(|page_offset, _| *page_offset < total);
        }
        self.rebuild_stopped_order();
        if previous_stopped != self.stopped_order {
            patch.record_order(TaskCollection::Stopped);
        }

        Ok(self.finish_patch(patch))
    }

    pub fn update_global_stat(
        &mut self,
        generation: SessionGeneration,
        stat: GlobalStat,
    ) -> Result<StorePatch, StoreError> {
        self.ensure_generation(generation)?;
        let mut patch = StorePatch::new(generation, self.revision);
        if self.global_stat != stat {
            self.global_stat = stat;
            patch.global_stat_changed = true;
        }
        Ok(self.finish_patch(patch))
    }

    /// Applies a targeted `tellStatus` result without guessing queue position.
    /// Periodic authoritative collection refreshes repair ordering separately.
    pub fn apply_task_snapshot(
        &mut self,
        generation: SessionGeneration,
        gid: Gid,
        snapshot: Option<TaskSnapshot>,
    ) -> Result<StorePatch, StoreError> {
        self.ensure_generation(generation)?;
        let mut patch = StorePatch::new(generation, self.revision);
        match snapshot {
            Some(snapshot) => {
                if snapshot.gid != gid {
                    return Err(StoreError::TargetedGidMismatch {
                        expected: gid,
                        received: snapshot.gid,
                    });
                }
                self.upsert(snapshot, &mut patch)?;
            }
            None => {
                return self.remove_tasks(generation, &[gid]);
            }
        }
        Ok(self.finish_patch(patch))
    }

    pub fn set_custom_output_name(
        &mut self,
        generation: SessionGeneration,
        gid: Gid,
        output_name: impl Into<String>,
    ) -> Result<StorePatch, StoreError> {
        self.ensure_generation(generation)?;
        let (fields, task_revision, search_name) = {
            let task = self
                .tasks
                .get_mut(&gid)
                .ok_or(StoreError::TaskNotFound(gid))?;
            let fields = task.set_custom_output_name(output_name);
            (fields, task.revision, task.display_name.to_lowercase())
        };
        let mut patch = StorePatch::new(generation, self.revision);
        if !fields.is_empty() {
            self.search_index.insert(gid, search_name);
            patch.updated.push(TaskFieldPatch {
                gid,
                fields,
                task_revision,
            });
        }
        Ok(self.finish_patch(patch))
    }

    pub fn set_stale(
        &mut self,
        generation: SessionGeneration,
        stale: bool,
    ) -> Result<StorePatch, StoreError> {
        self.ensure_generation(generation)?;
        let mut patch = StorePatch::new(generation, self.revision);
        if self.stale != stale {
            self.stale = stale;
            patch.stale_changed = true;
        }
        Ok(self.finish_patch(patch))
    }

    pub fn remove_tasks(
        &mut self,
        generation: SessionGeneration,
        gids: &[Gid],
    ) -> Result<StorePatch, StoreError> {
        self.ensure_generation(generation)?;
        let remove_set = gids.iter().copied().collect::<HashSet<_>>();
        let mut patch = StorePatch::new(generation, self.revision);

        for gid in &remove_set {
            if self.tasks.remove(gid).is_some() {
                self.search_index.remove(gid);
                self.seeding_started_at.remove(gid);
                patch.removed.push(*gid);
            }
            self.history_only.remove(gid);
        }

        let previous_active = self.active_order.clone();
        let previous_waiting = self.waiting_order.clone();
        let previous_stopped = self.stopped_order.clone();
        let previous_history = self.history_order.clone();
        self.active_order.retain(|gid| !remove_set.contains(gid));
        self.waiting_order.retain(|gid| !remove_set.contains(gid));
        self.history_order.retain(|gid| !remove_set.contains(gid));
        for page in self.stopped_pages.values_mut() {
            page.retain(|gid| !remove_set.contains(gid));
        }
        self.rebuild_stopped_order();
        if previous_history != self.history_order {
            patch.record_order(TaskCollection::Stopped);
        }

        if previous_active != self.active_order {
            patch.record_order(TaskCollection::Active);
        }
        if previous_waiting != self.waiting_order {
            patch.record_order(TaskCollection::Waiting);
        }
        if previous_stopped != self.stopped_order {
            patch.record_order(TaskCollection::Stopped);
        }

        Ok(self.finish_patch(patch))
    }

    fn ensure_generation(&self, received: SessionGeneration) -> Result<(), StoreError> {
        if received != self.session.generation {
            return Err(StoreError::StaleGeneration {
                expected: self.session.generation,
                received,
            });
        }
        Ok(())
    }

    fn upsert(&mut self, snapshot: TaskSnapshot, patch: &mut StorePatch) -> Result<(), StoreError> {
        let gid = snapshot.gid;
        if let Some(task) = self.tasks.get_mut(&gid) {
            let fields = task.apply_snapshot(snapshot)?;
            let task_revision = task.revision;
            let status = task.status;
            let search_name = fields
                .contains(TaskFields::DISPLAY_NAME)
                .then(|| task.display_name.to_lowercase());
            if !fields.is_empty() {
                patch.updated.push(TaskFieldPatch {
                    gid,
                    fields,
                    task_revision,
                });
            }
            if let Some(search_name) = search_name {
                self.search_index.insert(gid, search_name);
            }
            self.update_seeding_observation(gid, status);
        } else {
            let task = DownloadTask::from_snapshot(snapshot);
            let status = task.status;
            self.search_index
                .insert(gid, task.display_name.to_lowercase());
            self.tasks.insert(gid, task);
            self.update_seeding_observation(gid, status);
            patch.inserted.push(gid);
        }
        // Engine snapshot is authoritative: drop history-only ownership for this GID.
        if self.history_only.remove(&gid) {
            self.history_order.retain(|item| *item != gid);
        }
        Ok(())
    }

    fn update_seeding_observation(&mut self, gid: Gid, status: ariadeck_domain::DownloadStatus) {
        if status == ariadeck_domain::DownloadStatus::Seeding {
            self.seeding_started_at
                .entry(gid)
                .or_insert_with(Instant::now);
        } else {
            self.seeding_started_at.remove(&gid);
        }
    }

    fn rebuild_stopped_order(&mut self) {
        let mut seen = HashSet::new();
        self.stopped_order = self
            .stopped_pages
            .values()
            .flatten()
            .copied()
            .filter(|gid| seen.insert(*gid) && self.tasks.contains_key(gid))
            .collect();
        // Engine stopped pages win over durable history for the same GID.
        if !self.history_only.is_empty() {
            for gid in &self.stopped_order {
                self.history_only.remove(gid);
            }
            self.history_order
                .retain(|gid| self.history_only.contains(gid));
        }
    }

    fn finish_patch(&mut self, mut patch: StorePatch) -> StorePatch {
        if !patch.is_empty() {
            self.revision = self.revision.saturating_add(1);
        }
        patch.store_revision = self.revision;
        patch
    }
}

fn download_task_from_history(record: &HistoryRecord) -> DownloadTask {
    let metadata = TaskMetadata {
        directory: record.directory.clone(),
        primary_uri: record.primary_uri_redacted.clone(),
        info_hash: record.info_hash.clone(),
        file_count: 0,
        followed_by: Vec::new(),
        belongs_to: None,
        source_kind: record.source_kind,
    };
    DownloadTask {
        gid: record.gid,
        status: record.status,
        display_name: record.display_name.clone(),
        name_state: TaskNameState::Resolved,
        total_length: record.total_length,
        completed_length: record.completed_length,
        upload_length: ariadeck_domain::ByteCount::default(),
        download_speed: ariadeck_domain::ByteRate::default(),
        upload_speed: ariadeck_domain::ByteRate::default(),
        connections: 0,
        error: record.error.clone(),
        metadata,
        revision: 1,
    }
}

fn validate_unique<'a>(
    tasks: impl IntoIterator<Item = &'a TaskSnapshot>,
) -> Result<(), StoreError> {
    let mut seen = HashSet::new();
    for task in tasks {
        if !seen.insert(task.gid) {
            return Err(StoreError::DuplicateGid(task.gid));
        }
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum StoreError {
    #[error("response belongs to stale generation {received}; current generation is {expected}")]
    StaleGeneration {
        expected: SessionGeneration,
        received: SessionGeneration,
    },
    #[error("snapshot contains duplicate GID {0}")]
    DuplicateGid(Gid),
    #[error("stopped page offset {offset} exceeds total {total}")]
    InvalidStoppedPage { offset: usize, total: usize },
    #[error("new engine session belongs to profile {received}, expected {expected}")]
    WrongProfile {
        expected: ariadeck_domain::ProfileId,
        received: ariadeck_domain::ProfileId,
    },
    #[error("targeted task response GID mismatch: expected {expected}, received {received}")]
    TargetedGidMismatch { expected: Gid, received: Gid },
    #[error("task {0} is not present in the current engine session")]
    TaskNotFound(Gid),
    #[error(transparent)]
    TaskUpdate(#[from] TaskUpdateError),
}

#[cfg(test)]
mod tests {
    use ariadeck_domain::{
        ByteCount, ByteRate, DownloadStatus, EngineSessionId, GlobalStat, ProfileId,
        SessionGeneration, TaskSnapshot,
    };

    use super::*;

    fn generation() -> SessionGeneration {
        SessionGeneration::initial()
    }

    fn store() -> DownloadStore {
        DownloadStore::new(EngineSession::new(
            ProfileId::new(),
            EngineSessionId::new(),
            generation(),
        ))
    }

    fn task(value: u64, status: DownloadStatus, name: &str) -> TaskSnapshot {
        TaskSnapshot::new(Gid::from_u64(value), status, name)
    }

    #[test]
    fn no_op_live_snapshot_keeps_store_and_task_revisions() {
        let mut store = store();
        let active = vec![task(1, DownloadStatus::Active, "one")];
        let first = match store.reconcile_live(generation(), active.clone(), Vec::new()) {
            Ok(patch) => patch,
            Err(error) => panic!("initial snapshot failed: {error}"),
        };
        let second = match store.reconcile_live(generation(), active, Vec::new()) {
            Ok(patch) => patch,
            Err(error) => panic!("repeated snapshot failed: {error}"),
        };

        assert_eq!(first.inserted, vec![Gid::from_u64(1)]);
        assert_eq!(store.revision(), 1);
        assert!(second.is_empty());
        assert_eq!(second.store_revision, 1);
        assert_eq!(
            store.task(Gid::from_u64(1)).map(|task| task.revision),
            Some(1)
        );
    }

    #[test]
    fn custom_output_name_updates_search_and_survives_targeted_refresh() {
        let mut store = store();
        store
            .reconcile_live(
                generation(),
                vec![task(1, DownloadStatus::Paused, "original.bin")],
                Vec::new(),
            )
            .expect("initial task");

        let patch = store
            .set_custom_output_name(generation(), Gid::from_u64(1), "renamed.bin")
            .expect("set custom output name");
        assert_eq!(patch.updated[0].gid, Gid::from_u64(1));
        assert_eq!(
            store
                .task(Gid::from_u64(1))
                .expect("renamed task")
                .display_name,
            "renamed.bin"
        );

        let mut refreshed = task(1, DownloadStatus::Paused, "original.bin");
        refreshed.name_state = ariadeck_domain::TaskNameState::Resolved;
        store
            .apply_task_snapshot(generation(), Gid::from_u64(1), Some(refreshed))
            .expect("targeted refresh");
        assert_eq!(
            store
                .task(Gid::from_u64(1))
                .expect("refreshed task")
                .display_name,
            "renamed.bin"
        );
        assert_eq!(
            store.search_index.get(&Gid::from_u64(1)),
            Some(&"renamed.bin".into())
        );
    }

    #[test]
    fn live_snapshot_handles_updates_transitions_and_removals() {
        let mut store = store();
        let initial = vec![
            task(1, DownloadStatus::Active, "one"),
            task(2, DownloadStatus::Active, "two"),
        ];
        if let Err(error) = store.reconcile_live(generation(), initial, Vec::new()) {
            panic!("initial snapshot failed: {error}");
        }

        let mut changed = task(1, DownloadStatus::Waiting, "one");
        changed.download_speed = ariadeck_domain::ByteRate::new(42);
        let patch = match store.reconcile_live(generation(), Vec::new(), vec![changed]) {
            Ok(patch) => patch,
            Err(error) => panic!("transition snapshot failed: {error}"),
        };

        assert_eq!(patch.removed, vec![Gid::from_u64(2)]);
        assert_eq!(patch.updated.len(), 1);
        assert_eq!(store.waiting_order, vec![Gid::from_u64(1)]);
        assert!(store.task(Gid::from_u64(2)).is_none());
    }

    #[test]
    fn local_history_merges_when_engine_purged() {
        use crate::HistoryRecord;
        use ariadeck_domain::{ByteCount, TaskSourceKind};

        let mut store = store();
        let profile = store.session().profile_id;
        let record = HistoryRecord {
            profile_id: profile,
            gid: Gid::from_u64(99),
            status: DownloadStatus::Complete,
            display_name: "saved.bin".into(),
            directory: None,
            info_hash: None,
            source_kind: TaskSourceKind::DirectUri,
            total_length: ByteCount::new(10),
            completed_length: ByteCount::new(10),
            error: None,
            primary_uri_redacted: Some("https://cdn.example/file".into()),
            recorded_at_ms: 100,
            updated_at_ms: 100,
        };
        store
            .seed_local_history(generation(), vec![record])
            .expect("seed");
        assert!(store.is_history_only(Gid::from_u64(99)));
        assert_eq!(store.stopped_history().local_saved, 1);
        assert_eq!(store.counts().completed, 1);

        // Engine page for same GID demotes history-only ownership.
        store
            .apply_stopped_page(
                generation(),
                0,
                Some(1),
                vec![task(99, DownloadStatus::Complete, "saved.bin")],
            )
            .expect("engine page");
        assert!(!store.is_history_only(Gid::from_u64(99)));
        assert_eq!(store.stopped_history().local_saved, 0);
        assert_eq!(store.counts().completed, 1);
    }

    #[test]
    fn stopped_history_tracks_loaded_total_and_next_page_offset() {
        let mut store = store();
        assert_eq!(
            store.stopped_history(),
            crate::StoppedHistoryState {
                loaded: 0,
                total: None,
                next_offset: 0,
                can_load_more: true,
                local_saved: 0,
            }
        );

        let first = vec![
            task(10, DownloadStatus::Complete, "ten"),
            task(11, DownloadStatus::Complete, "eleven"),
        ];
        store
            .apply_stopped_page(generation(), 0, Some(5), first)
            .expect("first page");
        assert_eq!(
            store.stopped_history(),
            crate::StoppedHistoryState {
                loaded: 2,
                total: Some(5),
                next_offset: 2,
                can_load_more: true,
                local_saved: 0,
            }
        );

        let second = vec![
            task(12, DownloadStatus::Error, "twelve"),
            task(13, DownloadStatus::Complete, "thirteen"),
            task(14, DownloadStatus::Complete, "fourteen"),
        ];
        store
            .apply_stopped_page(generation(), 2, Some(5), second)
            .expect("second page");
        assert_eq!(
            store.stopped_history(),
            crate::StoppedHistoryState {
                loaded: 5,
                total: Some(5),
                next_offset: 5,
                can_load_more: false,
                local_saved: 0,
            }
        );
    }

    #[test]
    fn stopped_page_absence_changes_order_but_does_not_delete_cache_entry() {
        let mut store = store();
        let first = vec![
            task(10, DownloadStatus::Complete, "ten"),
            task(11, DownloadStatus::Complete, "eleven"),
        ];
        if let Err(error) = store.apply_stopped_page(generation(), 0, Some(2), first) {
            panic!("initial stopped page failed: {error}");
        }

        let patch = match store.apply_stopped_page(
            generation(),
            0,
            Some(1),
            vec![task(11, DownloadStatus::Complete, "eleven")],
        ) {
            Ok(patch) => patch,
            Err(error) => panic!("updated stopped page failed: {error}"),
        };

        assert!(patch.removed.is_empty());
        assert!(store.task(Gid::from_u64(10)).is_some());
        assert_eq!(store.stopped_order, vec![Gid::from_u64(11)]);
    }

    #[test]
    fn stale_generation_is_rejected_without_mutation() {
        let mut store = store();
        let stale = generation().next();
        let result = store.reconcile_live(
            stale,
            vec![task(1, DownloadStatus::Active, "one")],
            Vec::new(),
        );

        assert_eq!(
            result,
            Err(StoreError::StaleGeneration {
                expected: generation(),
                received: stale,
            })
        );
        assert_eq!(store.revision(), 0);
        assert!(store.tasks.is_empty());
    }

    #[test]
    fn speed_samples_are_bounded_without_changing_store_revision() {
        let mut store = store();
        for rate in 0..=crate::DEFAULT_SPEED_HISTORY_CAPACITY as u64 {
            let stat = GlobalStat {
                download_speed: ByteRate::new(rate),
                upload_speed: ByteRate::new(rate / 2),
                ..GlobalStat::default()
            };
            store
                .record_speed_sample(generation(), stat)
                .expect("record speed sample");
        }

        assert_eq!(
            store.speed_history().samples().len(),
            crate::DEFAULT_SPEED_HISTORY_CAPACITY
        );
        assert_eq!(
            store
                .speed_history()
                .samples()
                .front()
                .map(|sample| sample.download),
            Some(ByteRate::new(1))
        );
        assert_eq!(store.revision(), 0);
        assert!(matches!(
            store.record_speed_sample(
                generation().next(),
                GlobalStat {
                    download_speed: ByteRate::new(999),
                    ..GlobalStat::default()
                }
            ),
            Err(StoreError::StaleGeneration { .. })
        ));
        assert_eq!(
            store
                .speed_history()
                .samples()
                .back()
                .map(|sample| sample.download),
            Some(ByteRate::new(crate::DEFAULT_SPEED_HISTORY_CAPACITY as u64))
        );
    }

    #[test]
    fn observed_seeding_time_is_session_bound_and_cleared_on_state_exit() {
        let mut store = store();
        let gid = Gid::from_u64(12);
        store
            .reconcile_live(
                generation(),
                vec![task(12, DownloadStatus::Seeding, "seed.bin")],
                Vec::new(),
            )
            .expect("initial seeding task");
        store
            .seeding_started_at
            .insert(gid, Instant::now() - std::time::Duration::from_secs(65));
        assert!(
            store
                .observed_seeding_seconds(gid)
                .is_some_and(|value| value >= 65)
        );

        store
            .reconcile_live(
                generation(),
                Vec::new(),
                vec![task(12, DownloadStatus::Paused, "seed.bin")],
            )
            .expect("paused task");
        assert_eq!(store.observed_seeding_seconds(gid), None);

        store
            .reconcile_live(
                generation(),
                vec![task(12, DownloadStatus::Seeding, "seed.bin")],
                Vec::new(),
            )
            .expect("resumed seeding task");
        let next_session = EngineSession::new(
            store.session().profile_id,
            EngineSessionId::new(),
            generation().next(),
        );
        store
            .begin_session(next_session)
            .expect("new engine session");
        assert_eq!(store.observed_seeding_seconds(gid), None);
    }

    #[test]
    fn new_session_preserves_tasks_and_rejects_old_generation() {
        let mut store = store();
        if let Err(error) = store.reconcile_live(
            generation(),
            vec![task(1, DownloadStatus::Active, "one")],
            Vec::new(),
        ) {
            panic!("initial snapshot failed: {error}");
        }
        let next = EngineSession::new(
            store.session().profile_id,
            EngineSessionId::new(),
            generation().next(),
        );
        let patch = match store.begin_session(next) {
            Ok(patch) => patch,
            Err(error) => panic!("session transition failed: {error}"),
        };

        assert!(patch.session_changed);
        assert!(patch.stale_changed);
        assert!(store.task(Gid::from_u64(1)).is_some());
        assert!(matches!(
            store.update_global_stat(generation(), GlobalStat::default()),
            Err(StoreError::StaleGeneration { .. })
        ));
    }

    #[test]
    fn stress_ten_thousand_stopped_pages_and_rapid_progress_patches() {
        // PERF-001: 10k stopped tasks across pages + rapid field-only updates stay bounded.
        let mut store = store();
        let total = 10_000usize;
        let page_size = 100usize;
        let mut offset = 0usize;
        while offset < total {
            let page = (offset..offset + page_size)
                .map(|index| {
                    task(
                        (index + 1) as u64,
                        DownloadStatus::Complete,
                        &format!("file-{index}.bin"),
                    )
                })
                .collect::<Vec<_>>();
            store
                .apply_stopped_page(generation(), offset, Some(total), page)
                .expect("apply stopped page");
            offset += page_size;
        }
        let history = store.stopped_history();
        assert_eq!(history.loaded, total);
        assert_eq!(history.total, Some(total));
        assert!(!history.can_load_more);
        assert_eq!(store.tasks.len(), total);

        // Rapid progress-like patches on a live task must not grow the map.
        let live = task(99_999, DownloadStatus::Active, "live.bin");
        store
            .reconcile_live(generation(), vec![live], Vec::new())
            .expect("seed live task");
        let live_gid = Gid::from_u64(99_999);
        for completed in 1..=200u64 {
            let mut snapshot = task(99_999, DownloadStatus::Active, "live.bin");
            snapshot.completed_length = ByteCount::new(completed * 1_024);
            snapshot.total_length = ByteCount::new(200 * 1_024);
            snapshot.download_speed = ByteRate::new(1_024);
            let patch = store
                .apply_task_snapshot(generation(), live_gid, Some(snapshot))
                .expect("progress patch");
            assert!(
                patch.inserted.is_empty(),
                "progress updates must not insert"
            );
            assert!(patch.removed.is_empty(), "progress updates must not remove");
        }
        assert_eq!(store.tasks.len(), total + 1);
        assert_eq!(
            store.speed_history().samples().len(),
            0,
            "progress patches must not touch speed history"
        );
    }

    #[test]
    fn stress_repeated_identical_live_reconcile_is_empty_and_stable() {
        let mut store = store();
        let active = (1..=500u64)
            .map(|id| task(id, DownloadStatus::Active, &format!("a{id}")))
            .collect::<Vec<_>>();
        store
            .reconcile_live(generation(), active.clone(), Vec::new())
            .expect("initial live");
        let revision = store.revision();
        for _ in 0..50 {
            let patch = store
                .reconcile_live(generation(), active.clone(), Vec::new())
                .expect("identical reconcile");
            assert!(patch.is_empty());
        }
        assert_eq!(store.revision(), revision);
        assert_eq!(store.tasks.len(), 500);
    }

    #[test]
    fn stress_fixed_task_set_survives_thousand_patch_cycles() {
        // PERF-001 memory: patch churn on a fixed set must not grow the task map
        // or leak seeding/search bookkeeping.
        let mut store = store();
        let active = (1..=100u64)
            .map(|id| task(id, DownloadStatus::Active, &format!("live-{id}")))
            .collect::<Vec<_>>();
        store
            .reconcile_live(generation(), active, Vec::new())
            .expect("seed live set");
        let task_count = store.tasks.len();
        let search_count = store.search_index.len();

        for cycle in 0..1_000u64 {
            let refreshed = (1..=100u64)
                .map(|id| {
                    let mut snapshot = task(id, DownloadStatus::Active, &format!("live-{id}"));
                    snapshot.completed_length = ByteCount::new(cycle + id);
                    snapshot.total_length = ByteCount::new(10_000);
                    snapshot.download_speed = ByteRate::new(1_024 + (cycle % 7));
                    snapshot
                })
                .collect::<Vec<_>>();
            store
                .reconcile_live(generation(), refreshed, Vec::new())
                .expect("patch cycle");
            store
                .record_speed_sample(
                    generation(),
                    GlobalStat {
                        download_speed: ByteRate::new(cycle),
                        upload_speed: ByteRate::new(cycle / 2),
                        ..GlobalStat::default()
                    },
                )
                .expect("speed sample");
        }

        assert_eq!(store.tasks.len(), task_count);
        assert_eq!(store.search_index.len(), search_count);
        assert!(store.speed_history().samples().len() <= crate::DEFAULT_SPEED_HISTORY_CAPACITY);
        assert_eq!(
            store.speed_history().samples().len(),
            crate::DEFAULT_SPEED_HISTORY_CAPACITY.min(1_000)
        );
    }
}
