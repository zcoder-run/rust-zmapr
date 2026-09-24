use super::item::{ItemId, ItemStatus};
use super::response::{ProcessContentOutput, ProcessStage};
use super::state::ProcessStateStore;
use super::stats::{FinalStats, ProgressStats};
use crate::event_base::{EventBaseError, MpscRx, MpscTx, OnceRx, OnceTx, new_mpsc_bounded_default, new_once};
use crate::{Error, Result};
use simple_fs::SPath;
use std::fmt;
use std::sync::Arc;

// region:    --- Types

#[derive(Debug, Clone)]
pub enum ProgressEvent {
	StageStarted {
		stage: ProcessStage,
	},
	StageCompleted {
		stage: ProcessStage,
	},
	StageFailed {
		stage: ProcessStage,
		message: String,
	},
	ItemsRegistered {
		stage: ProcessStage,
		count: usize,
	},
	ItemsExcluded {
		stage: ProcessStage,
		count: usize,
	},
	StageTotalKnown {
		stage: ProcessStage,
		total_items: usize,
	},
	ItemStatusChanged {
		id: ItemId,
		stage: ProcessStage,
		status: ItemStatus,
	},
	WorkflowCompleted,
	WorkflowFailed {
		message: String,
	},
}

#[derive(Debug, Clone)]
pub struct ProgressUpdate {
	pub seq: u64,
	pub event: ProgressEvent,
	pub stats: ProgressStats,
}

pub(crate) type ProcessProgressTx = MpscTx<ProgressUpdate>;
pub(crate) type ProcessProgressRx = MpscRx<ProgressUpdate>;
pub(crate) type ProcessCompletionTx = OnceTx<Result<ProcessContentOutput>>;
pub(crate) type ProcessCompletionRx = OnceRx<Result<ProcessContentOutput>>;

/// Receives progress notifications from one running workflow.
pub struct ProgressRx {
	inner: ProcessProgressRx,
}

#[derive(Clone)]
pub(crate) struct ProcessProgressPublisher {
	tx: ProcessProgressTx,
	state: Arc<ProcessStateStore>,
}

// endregion: --- Types

// region:    --- Factories

pub(crate) fn new_progress_channel() -> Result<(ProcessProgressTx, ProgressRx)> {
	let (tx, rx) = new_mpsc_bounded_default::<ProgressUpdate>("process-progress").map_err(event_base_error_to_error)?;

	Ok((tx, ProgressRx::new(rx)))
}

pub(crate) fn new_completion_channel() -> (ProcessCompletionTx, ProcessCompletionRx) {
	new_once::<Result<ProcessContentOutput>>("process-content-completion")
}

// endregion: --- Factories

// region:    --- Constructors

impl ProgressRx {
	pub(crate) fn new(inner: ProcessProgressRx) -> Self {
		Self { inner }
	}
}

impl ProcessProgressPublisher {
	pub(crate) fn new(tx: ProcessProgressTx, state: Arc<ProcessStateStore>) -> Self {
		Self { tx, state }
	}
}

// endregion: --- Constructors

// region:    --- Operations

impl ProcessProgressPublisher {
	pub(crate) fn stage_started(&self, stage: ProcessStage) {
		let update = self.state.stage_started(stage);
		self.send_update(update);
	}

	pub(crate) fn stage_completed(&self, stage: ProcessStage) {
		let update = self.state.stage_completed(stage);
		self.send_update(update);
	}

	pub(crate) fn fail_workflow(&self, message: &str) {
		for update in self.state.fail_workflow(message) {
			self.send_update(update);
		}
	}

	pub(crate) fn finish(&self) -> Result<(FinalStats, Vec<super::item::ItemState>)> {
		let (update, result) = self.state.finish();
		self.send_update(update);
		result
	}

	pub(crate) fn register_fetch_items(&self, entries: Vec<(String, String)>) -> Vec<ItemId> {
		let (ids, update) = self.state.register_items(ProcessStage::Fetch, entries, false);
		self.send_update(update);
		ids
	}

	pub(crate) fn register_stage_items(&self, stage: ProcessStage, entries: Vec<(String, String)>) -> Vec<ItemId> {
		let (ids, update) = self.state.register_items(stage, entries, true);
		self.send_update(update);
		ids
	}

	pub(crate) fn set_stage_total(&self, stage: ProcessStage) {
		self.send_update(self.state.set_stage_total(stage));
	}

	pub(crate) fn add_excluded(&self, stage: ProcessStage, count: usize) {
		if count == 0 {
			return;
		}
		self.send_update(self.state.add_excluded(stage, count));
	}

	pub(crate) fn item_running(&self, id: ItemId, stage: ProcessStage) {
		if let Some(update) = self
			.state
			.set_item_status(id, stage, ItemStatus::Running, None, None, None, None)
		{
			self.send_update(update);
		}
	}

	pub(crate) fn item_completed(
		&self,
		id: ItemId,
		stage: ProcessStage,
		path: Option<SPath>,
		usage: Option<genai::chat::Usage>,
	) {
		if let Some(update) = self
			.state
			.set_item_status(id, stage, ItemStatus::Completed, path, usage, None, None)
		{
			self.send_update(update);
		}
	}

	pub(crate) fn item_reused(&self, id: ItemId, stage: ProcessStage, path: Option<SPath>) {
		if let Some(update) = self
			.state
			.set_item_status(id, stage, ItemStatus::Reused, path, None, None, None)
		{
			self.send_update(update);
		}
	}

	pub(crate) fn item_skipped(&self, id: ItemId, stage: ProcessStage, path: Option<SPath>) {
		if let Some(update) = self
			.state
			.set_item_status(id, stage, ItemStatus::Skipped, path, None, None, None)
		{
			self.send_update(update);
		}
	}

	pub(crate) fn item_failed(&self, id: ItemId, stage: ProcessStage, message: impl Into<String>) {
		if let Some(update) =
			self.state
				.set_item_status(id, stage, ItemStatus::Failed, None, None, Some(message.into()), None)
		{
			self.send_update(update);
		}
	}

	pub(crate) fn fetch_completed(&self, id: ItemId, relative_path: &str, path: SPath) {
		if let Some(update) = self.state.set_item_status(
			id,
			ProcessStage::Fetch,
			ItemStatus::Completed,
			Some(path),
			None,
			None,
			Some(relative_path),
		) {
			self.send_update(update);
		}
	}

	pub(crate) fn fetch_reused(&self, id: ItemId, relative_path: &str, path: SPath) {
		if let Some(update) = self.state.set_item_status(
			id,
			ProcessStage::Fetch,
			ItemStatus::Reused,
			Some(path),
			None,
			None,
			Some(relative_path),
		) {
			self.send_update(update);
		}
	}

	fn send_update(&self, update: ProgressUpdate) {
		let _ = self.tx.try_send(update);
	}
}

impl ProgressRx {
	/// Receives the next progress notification.
	pub async fn recv(&mut self) -> Result<ProgressUpdate> {
		self.inner.recv().await.map_err(event_base_error_to_error)
	}

	/// Returns whether the progress channel has been disconnected.
	pub fn is_disconnected(&self) -> bool {
		self.inner.is_disconnected()
	}
}

// endregion: --- Operations

// region:    --- Trait Implementations

impl fmt::Debug for ProcessProgressPublisher {
	fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
		formatter.debug_struct("ProcessProgressPublisher").finish_non_exhaustive()
	}
}

// endregion: --- Trait Implementations

// region:    --- Support

pub(crate) fn event_base_error_to_error(error: EventBaseError) -> Error {
	match error {
		EventBaseError::Custom(message) => Error::custom(message),
		EventBaseError::InvalidCapacity { channel, capacity } => {
			Error::InvalidConfiguration(format!("event channel {channel} has invalid capacity {capacity}"))
		}
		EventBaseError::TxDisconnected { channel } => {
			Error::MalformedState(format!("event channel {channel} sender disconnected"))
		}
		EventBaseError::RxDisconnected { channel } => {
			Error::MalformedState(format!("event channel {channel} receiver disconnected"))
		}
	}
}

// endregion: --- Support
