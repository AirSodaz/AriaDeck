use std::collections::{BTreeMap, HashMap, HashSet};

use ariadeck_domain::{
    DownloadTask, EngineSession, Gid, GlobalStat, SessionGeneration, TaskFields, TaskSnapshot,
    TaskUpdateError,
};
use thiserror::Error;

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum TaskCollection {
    Active,
    Waiting,
    Stopped,
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
    stopped_pages: BTreeMap<usize, Vec<Gid>>,
    stopped_total: Option<usize>,
    pub(crate) search_index: HashMap<Gid, String>,
    global_stat: GlobalStat,
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
            stopped_pages: BTreeMap::new(),
            stopped_total: None,
            search_index: HashMap::new(),
            global_stat: GlobalStat::default(),
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
    pub fn task(&self, gid: Gid) -> Option<&DownloadTask> {
        self.tasks.get(&gid)
    }

    #[must_use]
    pub fn stopped_total(&self) -> Option<usize> {
        self.stopped_total
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
                patch.removed.push(*gid);
            }
        }

        let previous_active = self.active_order.clone();
        let previous_waiting = self.waiting_order.clone();
        let previous_stopped = self.stopped_order.clone();
        self.active_order.retain(|gid| !remove_set.contains(gid));
        self.waiting_order.retain(|gid| !remove_set.contains(gid));
        for page in self.stopped_pages.values_mut() {
            page.retain(|gid| !remove_set.contains(gid));
        }
        self.rebuild_stopped_order();

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
        } else {
            let task = DownloadTask::from_snapshot(snapshot);
            self.search_index
                .insert(gid, task.display_name.to_lowercase());
            self.tasks.insert(gid, task);
            patch.inserted.push(gid);
        }
        Ok(())
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
    }

    fn finish_patch(&mut self, mut patch: StorePatch) -> StorePatch {
        if !patch.is_empty() {
            self.revision = self.revision.saturating_add(1);
        }
        patch.store_revision = self.revision;
        patch
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
    #[error(transparent)]
    TaskUpdate(#[from] TaskUpdateError),
}

#[cfg(test)]
mod tests {
    use ariadeck_domain::{
        DownloadStatus, EngineSessionId, ProfileId, SessionGeneration, TaskSnapshot,
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
}
