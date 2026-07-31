use std::sync::{Arc, Mutex, MutexGuard};

use super::progress::ProcessProgress;
use super::response::{ProcessFailure, ProcessItem, ProcessStage};

// region:    --- Types

/// Provides read-only access to authoritative in-memory workflow state.
#[derive(Debug, Clone)]
pub struct ProcessQuery {
	inner: Arc<ProcessStateStore>,
}

/// A point-in-time copy of workflow state and retained progress history.
#[derive(Debug, Clone, Default)]
pub struct ProcessStateSnapshot {
	/// The stage currently reported as running.
	pub current_stage: Option<ProcessStage>,
	/// Items reported as completed.
	pub completed_items: Vec<ProcessItem>,
	/// Items reported as skipped or reused.
	pub skipped_items: Vec<ProcessItem>,
	/// Item-level failures reported by the workflow.
	pub failures: Vec<ProcessFailure>,
	/// All progress notifications retained by the in-memory state.
	pub progress_history: Vec<ProcessProgress>,
}

#[derive(Debug, Default)]
pub(crate) struct ProcessStateStore {
	snapshot: Mutex<ProcessStateSnapshot>,
}

// endregion: --- Types

// region:    --- Constructors

impl ProcessQuery {
	pub(crate) fn new(inner: Arc<ProcessStateStore>) -> Self {
		Self { inner }
	}
}

// endregion: --- Constructors

// region:    --- Operations

impl ProcessQuery {
	/// Returns a point-in-time copy of the workflow state.
	pub fn snapshot(&self) -> ProcessStateSnapshot {
		self.inner.snapshot()
	}
}

impl ProcessStateStore {
	pub(crate) fn record(&self, progress: &ProcessProgress) {
		self.lock().record(progress);
	}

	pub(crate) fn snapshot(&self) -> ProcessStateSnapshot {
		self.lock().clone()
	}
}

// endregion: --- Operations

// region:    --- Support

pub(crate) fn new_process_state() -> Arc<ProcessStateStore> {
	Arc::new(ProcessStateStore::default())
}

impl ProcessStateStore {
	fn lock(&self) -> MutexGuard<'_, ProcessStateSnapshot> {
		self.snapshot.lock().unwrap_or_else(|error| error.into_inner())
	}
}

impl ProcessStateSnapshot {
	fn record(&mut self, progress: &ProcessProgress) {
		match progress {
			ProcessProgress::StageStarted { stage } => {
				self.current_stage = Some(*stage);
			}
			ProcessProgress::ItemCompleted { item } => {
				self.completed_items.push(item.clone());
			}
			ProcessProgress::ItemSkipped { item } => {
				self.skipped_items.push(item.clone());
			}
			ProcessProgress::ItemFailed { failure } => {
				self.failures.push(failure.clone());
			}
			ProcessProgress::StageCompleted { stage } => {
				if self.current_stage == Some(*stage) {
					self.current_stage = None;
				}
			}
		}

		self.progress_history.push(progress.clone());
	}
}

// endregion: --- Support
