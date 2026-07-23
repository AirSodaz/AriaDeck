use std::{cmp::Ordering, collections::HashSet};

use ariadeck_domain::{
    DownloadFilter, DownloadSort, DownloadStatus, DownloadTask, Gid, SortDirection, SortKey,
};

use crate::DownloadStore;

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct TaskListQuery {
    pub filter: DownloadFilter,
    pub search: String,
    pub sort: DownloadSort,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TaskListView {
    pub source_revision: u64,
    pub visible_gids: Vec<Gid>,
    pub query: TaskListQuery,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct TaskCounts {
    pub all: usize,
    pub active: usize,
    pub waiting: usize,
    pub paused: usize,
    pub completed: usize,
    pub failed: usize,
}

impl DownloadStore {
    #[must_use]
    pub fn view(&self, query: &TaskListQuery) -> TaskListView {
        let search = query.search.trim().to_lowercase();
        let mut seen = HashSet::new();
        let mut visible_gids = self
            .active_order
            .iter()
            .chain(&self.waiting_order)
            .chain(&self.stopped_order)
            .chain(&self.history_order)
            .copied()
            .filter(|gid| seen.insert(*gid))
            .filter(|gid| {
                self.tasks.get(gid).is_some_and(|task| {
                    query.filter.matches(task.status)
                        && (search.is_empty()
                            || self
                                .search_index
                                .get(gid)
                                .is_some_and(|name| name.contains(&search))
                            || gid.to_string().contains(&search))
                })
            })
            .collect::<Vec<_>>();

        if query.sort.key != SortKey::Queue {
            visible_gids.sort_by(|left, right| {
                let ordering = compare_tasks(
                    self.tasks.get(left),
                    self.tasks.get(right),
                    query.sort.key,
                    &self.search_index,
                );
                apply_direction(ordering.then_with(|| left.cmp(right)), query.sort.direction)
            });
        } else if query.sort.direction == SortDirection::Descending {
            visible_gids.reverse();
        }

        TaskListView {
            source_revision: self.revision(),
            visible_gids,
            query: query.clone(),
        }
    }

    #[must_use]
    pub fn counts(&self) -> TaskCounts {
        // Scan each task once in queue order without building a full `view()`
        // (PERF-001: counts must stay O(n) without an extra allocation pass).
        let mut counts = TaskCounts::default();
        let mut seen = HashSet::new();
        for gid in self
            .active_order
            .iter()
            .chain(&self.waiting_order)
            .chain(&self.stopped_order)
            .chain(&self.history_order)
            .copied()
            .filter(|gid| seen.insert(*gid))
        {
            let Some(task) = self.tasks.get(&gid) else {
                continue;
            };
            counts.all += 1;
            match task.status {
                DownloadStatus::Active | DownloadStatus::Seeding | DownloadStatus::Verifying => {
                    counts.active += 1
                }
                DownloadStatus::Waiting => counts.waiting += 1,
                DownloadStatus::Paused => counts.paused += 1,
                DownloadStatus::Complete => counts.completed += 1,
                DownloadStatus::Error => counts.failed += 1,
                DownloadStatus::Removed | DownloadStatus::Unknown => {}
            }
        }
        counts
    }
}

fn compare_tasks(
    left: Option<&DownloadTask>,
    right: Option<&DownloadTask>,
    key: SortKey,
    search_index: &std::collections::HashMap<Gid, String>,
) -> Ordering {
    let (Some(left), Some(right)) = (left, right) else {
        return left.is_some().cmp(&right.is_some());
    };

    match key {
        SortKey::Queue => Ordering::Equal,
        SortKey::Name => search_index
            .get(&left.gid)
            .cmp(&search_index.get(&right.gid)),
        SortKey::Status => status_rank(left.status).cmp(&status_rank(right.status)),
        SortKey::Progress => compare_progress(left, right),
        SortKey::DownloadSpeed => left.download_speed.cmp(&right.download_speed),
        SortKey::Size => left.total_length.cmp(&right.total_length),
    }
}

fn compare_progress(left: &DownloadTask, right: &DownloadTask) -> Ordering {
    let left_total = left.total_length.get();
    let right_total = right.total_length.get();
    match (left_total, right_total) {
        (0, 0) => Ordering::Equal,
        (0, _) => Ordering::Less,
        (_, 0) => Ordering::Greater,
        _ => (u128::from(left.completed_length.get()) * u128::from(right_total))
            .cmp(&(u128::from(right.completed_length.get()) * u128::from(left_total))),
    }
}

const fn status_rank(status: DownloadStatus) -> u8 {
    match status {
        DownloadStatus::Active => 0,
        DownloadStatus::Seeding => 1,
        DownloadStatus::Verifying => 2,
        DownloadStatus::Waiting => 3,
        DownloadStatus::Paused => 4,
        DownloadStatus::Error => 5,
        DownloadStatus::Complete => 6,
        DownloadStatus::Removed => 7,
        DownloadStatus::Unknown => 8,
    }
}

const fn apply_direction(ordering: Ordering, direction: SortDirection) -> Ordering {
    match direction {
        SortDirection::Ascending => ordering,
        SortDirection::Descending => ordering.reverse(),
    }
}

#[cfg(test)]
mod tests {
    use ariadeck_domain::{
        ByteCount, DownloadFilter, DownloadSort, DownloadStatus, EngineSession, EngineSessionId,
        ProfileId, SessionGeneration, SortDirection, SortKey, TaskSnapshot,
    };

    use super::*;

    fn store_with_tasks() -> DownloadStore {
        let generation = SessionGeneration::initial();
        let mut store = DownloadStore::new(EngineSession::new(
            ProfileId::new(),
            EngineSessionId::new(),
            generation,
        ));
        let mut slow = TaskSnapshot::new(Gid::from_u64(1), DownloadStatus::Active, "Zulu.iso");
        slow.total_length = ByteCount::new(100);
        slow.completed_length = ByteCount::new(10);
        let mut fast = TaskSnapshot::new(Gid::from_u64(2), DownloadStatus::Active, "alpha.iso");
        fast.total_length = ByteCount::new(200);
        fast.completed_length = ByteCount::new(100);
        if let Err(error) = store.reconcile_live(generation, vec![slow, fast], Vec::new()) {
            panic!("fixture snapshot failed: {error}");
        }
        store
    }

    #[test]
    fn filtering_and_search_return_stable_gids() {
        let store = store_with_tasks();
        let query = TaskListQuery {
            filter: DownloadFilter::Active,
            search: "ALPHA".into(),
            sort: DownloadSort::default(),
        };

        assert_eq!(store.view(&query).visible_gids, vec![Gid::from_u64(2)]);
    }

    #[test]
    fn progress_sort_uses_ratios_without_floating_point() {
        let store = store_with_tasks();
        let query = TaskListQuery {
            sort: DownloadSort {
                key: SortKey::Progress,
                direction: SortDirection::Descending,
            },
            ..TaskListQuery::default()
        };

        assert_eq!(
            store.view(&query).visible_gids,
            vec![Gid::from_u64(2), Gid::from_u64(1)]
        );
    }

    #[test]
    fn seeding_is_counted_and_filtered_as_active() {
        let generation = SessionGeneration::initial();
        let mut store = DownloadStore::new(EngineSession::new(
            ProfileId::new(),
            EngineSessionId::new(),
            generation,
        ));
        store
            .reconcile_live(
                generation,
                vec![TaskSnapshot::new(
                    Gid::from_u64(3),
                    DownloadStatus::Seeding,
                    "seed.iso",
                )],
                Vec::new(),
            )
            .expect("seeding fixture");

        let counts = store.counts();
        assert_eq!(counts.all, 1);
        assert_eq!(counts.active, 1);
        assert_eq!(counts.completed, 0);
        assert_eq!(
            store
                .view(&TaskListQuery {
                    filter: DownloadFilter::Active,
                    ..TaskListQuery::default()
                })
                .visible_gids,
            vec![Gid::from_u64(3)]
        );
    }

    #[test]
    fn stress_ten_thousand_view_filter_sort_and_counts() {
        let generation = SessionGeneration::initial();
        let mut store = DownloadStore::new(EngineSession::new(
            ProfileId::new(),
            EngineSessionId::new(),
            generation,
        ));
        let total = 10_000usize;
        let page = (0..total)
            .map(|index| {
                let status = if index % 5 == 0 {
                    DownloadStatus::Error
                } else {
                    DownloadStatus::Complete
                };
                TaskSnapshot::new(
                    Gid::from_u64((index + 1) as u64),
                    status,
                    format!("item-{index:05}.bin"),
                )
            })
            .collect::<Vec<_>>();
        store
            .apply_stopped_page(generation, 0, Some(total), page)
            .expect("load 10k stopped");

        let failed = store.view(&TaskListQuery {
            filter: DownloadFilter::Failed,
            ..TaskListQuery::default()
        });
        assert_eq!(failed.visible_gids.len(), total / 5);

        let sorted = store.view(&TaskListQuery {
            filter: DownloadFilter::All,
            sort: DownloadSort {
                key: SortKey::Name,
                direction: SortDirection::Descending,
            },
            ..TaskListQuery::default()
        });
        assert_eq!(sorted.visible_gids.len(), total);
        assert_eq!(
            sorted.visible_gids.first().copied(),
            Some(Gid::from_u64(total as u64)),
            "descending name should put highest index first with zero-padded names"
        );

        let counts = store.counts();
        assert_eq!(counts.all, total);
        assert_eq!(counts.completed, total - total / 5);
        assert_eq!(counts.failed, total / 5);
    }
}
