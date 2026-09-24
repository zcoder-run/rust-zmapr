use super::item::{ItemId, ItemStageState, ItemState, ItemStatus};
use super::progress::{ProgressEvent, ProgressUpdate};
use super::response::ProcessStage;
use super::stats::{FinalStats, ProgressStats, StageStatus, add_usage};
use crate::support::now_micro;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard};

// region:    --- Types

/// Provides read-only access to authoritative in-memory workflow state.
#[derive(Debug, Clone)]
pub struct ProcessQuery {
	inner: Arc<ProcessStateStore>,
}

/// A point-in-time copy of workflow state and retained progress history.
#[derive(Debug, Clone, Default)]
pub struct ProcessStateSnapshot {
	/// Current stage statistics.
	pub stats: ProgressStats,
	/// Current state of all registered items.
	pub items: Vec<ItemState>,
}

#[derive(Debug, Default)]
pub(crate) struct ProcessStateStore {
	inner: Mutex<StateInner>,
}

#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct StageSelection {
	pub(crate) fetch: bool,
	pub(crate) sanitize: bool,
	pub(crate) map: bool,
}

#[derive(Debug, Default)]
struct StateInner {
	snapshot: ProcessStateSnapshot,
	path_index: HashMap<String, ItemId>,
	seq: u64,
}

// endregion: --- Types

// region:    --- Constructors

impl ProcessQuery {
	pub(crate) fn new(inner: Arc<ProcessStateStore>) -> Self {
		Self { inner }
	}
}

impl ProcessStateStore {
	pub(crate) fn new(selection: StageSelection) -> Self {
		let mut stats = ProgressStats::default();
		stats.fetch.status = selected_stage_status(selection.fetch);
		stats.sanitize.status = selected_stage_status(selection.sanitize);
		stats.map.status = selected_stage_status(selection.map);
		stats.started_epoch_us = now_micro();

		Self {
			inner: Mutex::new(StateInner {
				snapshot: ProcessStateSnapshot {
					stats,
					..ProcessStateSnapshot::default()
				},
				..StateInner::default()
			}),
		}
	}
}

// endregion: --- Constructors

// region:    --- Operations

impl ProcessQuery {
	/// Returns a copy of the current workflow statistics.
	pub fn stats(&self) -> ProgressStats {
		self.inner.stats()
	}

	/// Returns a copy of an item by its run-scoped id.
	pub fn item(&self, id: ItemId) -> Option<ItemState> {
		self.inner.item(id)
	}

	/// Returns a copy of an item by its stored relative path.
	pub fn item_by_path(&self, relative_path: &str) -> Option<ItemState> {
		self.inner.item_by_path(relative_path)
	}

	/// Returns copies of all registered items in id order.
	pub fn items(&self) -> Vec<ItemState> {
		self.inner.items()
	}

	/// Returns item ids with the requested stage status, in id order.
	pub fn item_ids(&self, stage: ProcessStage, status: ItemStatus) -> Vec<ItemId> {
		self.inner.item_ids(stage, status)
	}

	/// Returns a point-in-time copy of the workflow state.
	pub fn snapshot(&self) -> ProcessStateSnapshot {
		self.inner.snapshot()
	}
}

impl ProcessStateStore {
	pub(crate) fn snapshot(&self) -> ProcessStateSnapshot {
		self.lock().snapshot.clone()
	}

	pub(crate) fn stats(&self) -> ProgressStats {
		self.lock().snapshot.stats.clone()
	}

	pub(crate) fn item(&self, id: ItemId) -> Option<ItemState> {
		self.lock().snapshot.items.get(id.index()).cloned()
	}

	pub(crate) fn item_by_path(&self, relative_path: &str) -> Option<ItemState> {
		let inner = self.lock();
		let id = inner.path_index.get(relative_path)?;
		inner.snapshot.items.get(id.index()).cloned()
	}

	pub(crate) fn items(&self) -> Vec<ItemState> {
		self.lock().snapshot.items.clone()
	}

	pub(crate) fn item_ids(&self, stage: ProcessStage, status: ItemStatus) -> Vec<ItemId> {
		self.lock()
			.snapshot
			.items
			.iter()
			.filter(|item| item.stage(stage).is_some_and(|state| state.status == status))
			.map(|item| item.id)
			.collect()
	}

	pub(crate) fn stage_started(&self, stage: ProcessStage) -> ProgressUpdate {
		self.apply(|inner| {
			let stage_progress = inner.snapshot.stats.stage_mut(stage);
			stage_progress.status = StageStatus::Running;
			stage_progress.started_epoch_us = Some(now_micro());
			ProgressEvent::StageStarted { stage }
		})
	}

	pub(crate) fn stage_completed(&self, stage: ProcessStage) -> ProgressUpdate {
		self.apply(|inner| {
			let total_items = inner.snapshot.stats.stage(stage).registered_items();
			let stage_progress = inner.snapshot.stats.stage_mut(stage);
			stage_progress.status = StageStatus::Completed;
			stage_progress.ended_epoch_us = Some(now_micro());
			if stage_progress.total_items.is_none() {
				stage_progress.total_items = Some(total_items);
			}
			ProgressEvent::StageCompleted { stage }
		})
	}

	pub(crate) fn fail_workflow(&self, message: &str) -> Vec<ProgressUpdate> {
		let mut updates = Vec::new();
		for stage in [ProcessStage::Fetch, ProcessStage::Sanitize, ProcessStage::Map] {
			let is_running = self.lock().snapshot.stats.stage(stage).status == StageStatus::Running;
			if is_running {
				let message = message.to_owned();
				updates.push(self.apply(|inner| {
					let stage_progress = inner.snapshot.stats.stage_mut(stage);
					stage_progress.status = StageStatus::Failed;
					stage_progress.ended_epoch_us = Some(now_micro());
					ProgressEvent::StageFailed { stage, message }
				}));
			}
		}

		let message = message.to_owned();
		updates.push(self.apply(|inner| {
			inner.snapshot.stats.ended_epoch_us = Some(now_micro());
			ProgressEvent::WorkflowFailed { message }
		}));
		updates
	}

	pub(crate) fn finish(&self) -> (ProgressUpdate, crate::Result<(FinalStats, Vec<ItemState>)>) {
		let mut inner = self.lock();
		inner.snapshot.stats.ended_epoch_us = Some(now_micro());
		inner.seq += 1;
		let stats = inner.snapshot.stats.clone();
		let items = inner.snapshot.items.clone();
		let update = ProgressUpdate {
			seq: inner.seq,
			event: ProgressEvent::WorkflowCompleted,
			stats: stats.clone(),
		};
		let result = FinalStats::from_progress(&stats).map(|final_stats| (final_stats, items));
		(update, result)
	}

	pub(crate) fn register_items(
		&self,
		stage: ProcessStage,
		entries: Vec<(String, String)>,
		lookup: bool,
	) -> (Vec<ItemId>, ProgressUpdate) {
		let count = entries.len();
		let mut ids = Vec::with_capacity(count);
		let update = self.apply(|inner| {
			for (source, relative_path) in entries {
				let existing_id = if lookup {
					inner
						.path_index
						.get(&relative_path)
						.copied()
						.filter(|id| {
							inner
								.snapshot
								.items
								.get(id.index())
								.is_some_and(|item| item.stage(stage).is_none())
						})
				} else {
					None
				};

				let id = if let Some(id) = existing_id {
					id
				} else {
					let id = ItemId::new(inner.snapshot.items.len());
					inner.snapshot.items.push(ItemState {
						id,
						source,
						origin_path: relative_path.clone(),
						relative_path: relative_path.clone(),
						fetch: None,
						sanitize: None,
						map: None,
					});
					inner.path_index.entry(relative_path.clone()).or_insert(id);
					id
				};

				*inner.snapshot.items[id.index()].stage_mut(stage) = Some(ItemStageState::pending());
				*inner
					.snapshot
					.stats
					.stage_mut(stage)
					.counter_mut(ItemStatus::Pending) += 1;
				ids.push(id);
			}

			ProgressEvent::ItemsRegistered { stage, count }
		});

		(ids, update)
	}

	pub(crate) fn set_stage_total(&self, stage: ProcessStage) -> ProgressUpdate {
		self.apply(|inner| {
			let total_items = inner.snapshot.stats.stage(stage).registered_items();
			inner.snapshot.stats.stage_mut(stage).total_items = Some(total_items);
			ProgressEvent::StageTotalKnown { stage, total_items }
		})
	}

	pub(crate) fn add_excluded(&self, stage: ProcessStage, count: usize) -> ProgressUpdate {
		self.apply(|inner| {
			inner.snapshot.stats.stage_mut(stage).excluded += count;
			ProgressEvent::ItemsExcluded { stage, count }
		})
	}

	#[allow(clippy::too_many_arguments)]
	pub(crate) fn set_item_status(
		&self,
		id: ItemId,
		stage: ProcessStage,
		status: ItemStatus,
		path: Option<simple_fs::SPath>,
		usage: Option<genai::chat::Usage>,
		error: Option<String>,
		relative_path: Option<&str>,
	) -> Option<ProgressUpdate> {
		self.lock().snapshot.items.get(id.index())?;

		let relative_path = relative_path.map(str::to_owned);
		let usage_for_aggregation = usage.clone();
		Some(self.apply(|inner| {
			let stage_is_missing = inner.snapshot.items[id.index()].stage(stage).is_none();
			if stage_is_missing {
				*inner
					.snapshot
					.stats
					.stage_mut(stage)
					.counter_mut(ItemStatus::Pending) += 1;
			}

			let previous_status = {
				let item = &mut inner.snapshot.items[id.index()];
				let stage_state = item.stage_mut(stage).get_or_insert_with(ItemStageState::pending);
				let previous_status = stage_state.status;
				stage_state.status = status;
				if let Some(path) = path {
					stage_state.path = Some(path);
				}
				if let Some(usage) = usage {
					stage_state.usage = Some(usage);
				}
				stage_state.error = error;
				if let Some(relative_path) = &relative_path {
					item.relative_path = relative_path.clone();
				}
				previous_status
			};

			{
				let stage_progress = inner.snapshot.stats.stage_mut(stage);
				let previous_count = stage_progress.counter_mut(previous_status);
				*previous_count = previous_count.saturating_sub(1);
				*stage_progress.counter_mut(status) += 1;
			}

			if status == ItemStatus::Completed
				&& let Some(usage) = usage_for_aggregation.as_ref()
			{
				add_usage(&mut inner.snapshot.stats.stage_mut(stage).usage, usage);
				add_usage(&mut inner.snapshot.stats.total_usage, usage);
			}

			if let Some(relative_path) = &relative_path {
				inner.path_index.insert(relative_path.clone(), id);
			}

			ProgressEvent::ItemStatusChanged { id, stage, status }
		}))
	}

	fn apply(&self, f: impl FnOnce(&mut StateInner) -> ProgressEvent) -> ProgressUpdate {
		let mut inner = self.lock();
		let event = f(&mut inner);
		inner.seq += 1;
		ProgressUpdate {
			seq: inner.seq,
			event,
			stats: inner.snapshot.stats.clone(),
		}
	}
}

// endregion: --- Operations

// region:    --- Support

pub(crate) fn new_process_state(selection: StageSelection) -> Arc<ProcessStateStore> {
	Arc::new(ProcessStateStore::new(selection))
}

impl ProcessStateStore {
	fn lock(&self) -> MutexGuard<'_, StateInner> {
		self.inner.lock().unwrap_or_else(|error| error.into_inner())
	}
}

fn selected_stage_status(selected: bool) -> StageStatus {
	if selected {
		StageStatus::Pending
	} else {
		StageStatus::NotSelected
	}
}

// endregion: --- Support

// region:    --- Tests

#[cfg(test)]
mod tests {
	type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

	use super::*;

	#[test]
	fn test_process_state_register_and_transition_counters() -> Result<()> {
		// -- Setup & Fixtures
		let state = ProcessStateStore::new(StageSelection {
			fetch: true,
			..StageSelection::default()
		});
		let (ids, _) = state.register_items(
			ProcessStage::Fetch,
			vec![("a".to_owned(), "a.md".to_owned()), ("b".to_owned(), "b.md".to_owned())],
			false,
		);
		let first = *ids.first().ok_or("expected first item id")?;
		let second = *ids.get(1).ok_or("expected second item id")?;

		// -- Exec
		let _ = state.set_item_status(first, ProcessStage::Fetch, ItemStatus::Running, None, None, None, None);
		let _ = state.set_item_status(first, ProcessStage::Fetch, ItemStatus::Completed, None, None, None, None);
		let _ = state.set_item_status(
			second,
			ProcessStage::Fetch,
			ItemStatus::Failed,
			None,
			None,
			Some("fetch failed".to_owned()),
			None,
		);

		// -- Check
		let stats = state.stats();
		assert_eq!(stats.fetch.pending, 0);
		assert_eq!(stats.fetch.running, 0);
		assert_eq!(stats.fetch.completed, 1);
		assert_eq!(stats.fetch.failed, 1);
		assert_eq!(state.item_ids(ProcessStage::Fetch, ItemStatus::Completed), vec![first]);
		assert_eq!(state.item_ids(ProcessStage::Fetch, ItemStatus::Failed), vec![second]);
		Ok(())
	}

	#[test]
	fn test_process_state_register_stage_items_lookup_by_path() -> Result<()> {
		// -- Setup & Fixtures
		let state = ProcessStateStore::new(StageSelection {
			fetch: true,
			sanitize: true,
			..StageSelection::default()
		});
		let (fetch_ids, _) = state.register_items(
			ProcessStage::Fetch,
			vec![("source.html".to_owned(), "source.html".to_owned())],
			false,
		);
		let id = *fetch_ids.first().ok_or("expected Fetch item id")?;

		// -- Exec
		let _ = state.set_item_status(
			id,
			ProcessStage::Fetch,
			ItemStatus::Completed,
			Some(simple_fs::SPath::from("source.md")),
			None,
			None,
			Some("source.md"),
		);
		let (sanitize_ids, _) = state.register_items(
			ProcessStage::Sanitize,
			vec![("source.md".to_owned(), "source.md".to_owned())],
			true,
		);

		// -- Check
		assert_eq!(sanitize_ids, vec![id]);
		let item = state.item(id).ok_or("expected registered item")?;
		assert_eq!(item.origin_path, "source.html");
		assert_eq!(item.relative_path, "source.md");
		Ok(())
	}

	#[test]
	fn test_process_state_usage_aggregation() -> Result<()> {
		// -- Setup & Fixtures
		let state = ProcessStateStore::new(StageSelection {
			sanitize: true,
			map: true,
			..StageSelection::default()
		});
		let usage = genai::chat::Usage {
			prompt_tokens: Some(10),
			completion_tokens: Some(5),
			total_tokens: Some(15),
			..Default::default()
		};
		let (sanitize_ids, _) = state.register_items(
			ProcessStage::Sanitize,
			vec![("a.md".to_owned(), "a.md".to_owned())],
			true,
		);
		let id = *sanitize_ids.first().ok_or("expected Sanitize item id")?;

		// -- Exec
		let _ = state.set_item_status(
			id,
			ProcessStage::Sanitize,
			ItemStatus::Completed,
			None,
			Some(usage.clone()),
			None,
			None,
		);
		let (map_ids, _) = state.register_items(
			ProcessStage::Map,
			vec![("a.md".to_owned(), "a.md".to_owned())],
			true,
		);
		let map_id = *map_ids.first().ok_or("expected Map item id")?;
		let _ = state.set_item_status(
			map_id,
			ProcessStage::Map,
			ItemStatus::Completed,
			None,
			Some(usage),
			None,
			None,
		);

		// -- Check
		let stats = state.stats();
		assert_eq!(stats.sanitize.usage.as_ref().and_then(|value| value.total_tokens), Some(15));
		assert_eq!(stats.map.usage.as_ref().and_then(|value| value.total_tokens), Some(15));
		assert_eq!(stats.total_usage.as_ref().and_then(|value| value.total_tokens), Some(30));
		Ok(())
	}

	#[test]
	fn test_process_state_fail_workflow_marks_running_stages() -> Result<()> {
		// -- Setup & Fixtures
		let state = ProcessStateStore::new(StageSelection {
			fetch: true,
			sanitize: true,
			..StageSelection::default()
		});
		let _ = state.stage_started(ProcessStage::Fetch);
		let _ = state.stage_started(ProcessStage::Sanitize);

		// -- Exec
		let updates = state.fail_workflow("workflow failed");

		// -- Check
		assert_eq!(updates.len(), 3);
		let stats = state.stats();
		assert_eq!(stats.fetch.status, StageStatus::Failed);
		assert_eq!(stats.sanitize.status, StageStatus::Failed);
		assert!(stats.fetch.ended_epoch_us.is_some());
		assert!(stats.sanitize.ended_epoch_us.is_some());
		assert!(stats.ended_epoch_us.is_some());
		assert!(matches!(updates.last().map(|update| &update.event), Some(ProgressEvent::WorkflowFailed { .. })));
		Ok(())
	}

	#[test]
	fn test_process_state_seq_increases() -> Result<()> {
		// -- Setup & Fixtures
		let state = ProcessStateStore::new(StageSelection {
			fetch: true,
			..StageSelection::default()
		});

		// -- Exec
		let first = state.stage_started(ProcessStage::Fetch);
		let (_, second) = state.register_items(
			ProcessStage::Fetch,
			vec![("a".to_owned(), "a.md".to_owned())],
			false,
		);
		let third = state.set_stage_total(ProcessStage::Fetch);

		// -- Check
		assert!(first.seq < second.seq);
		assert!(second.seq < third.seq);
		Ok(())
	}
}

// endregion: --- Tests
