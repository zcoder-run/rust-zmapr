use std::fmt;
use std::sync::Arc;

use super::state::ProcessStateStore;
use super::response::{ProcessContentOutput, ProcessFailure, ProcessItem, ProcessStage};
use crate::event_base::{
	new_mpsc_bounded_default, new_once, EventBaseError, MpscRx, MpscTx, OnceRx, OnceTx,
};
use crate::{Error, Result};

// region:    --- Types

/// A progress notification emitted by the workflow.
///
/// Notifications are observational and may be dropped when the bounded progress
/// channel is full.
#[derive(Debug, Clone)]
pub enum ProcessProgress {
	/// A processing stage started.
	StageStarted {
		stage: ProcessStage,
	},

	/// An item completed successfully.
	ItemCompleted {
		item: ProcessItem,
	},

	/// An item was skipped or reused.
	ItemSkipped {
		item: ProcessItem,
	},

	/// An item failed during processing.
	ItemFailed {
		failure: ProcessFailure,
	},

	/// A processing stage completed.
	StageCompleted {
		stage: ProcessStage,
	},
}

pub(crate) type ProcessProgressTx = MpscTx<ProcessProgress>;
pub(crate) type ProcessProgressRx = MpscRx<ProcessProgress>;
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
	let (tx, rx) =
		new_mpsc_bounded_default::<ProcessProgress>("process-progress")
			.map_err(event_base_error_to_error)?;

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
	pub(crate) fn publish(&self, progress: ProcessProgress) {
		self.state.record(&progress);
		let _ = self.tx.try_send(progress);
	}
}

impl ProgressRx {
	/// Receives the next progress notification.
	pub async fn recv(&mut self) -> Result<ProcessProgress> {
		self.inner
			.recv()
			.await
			.map_err(event_base_error_to_error)
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
		formatter
			.debug_struct("ProcessProgressPublisher")
			.finish_non_exhaustive()
	}
}

// endregion: --- Trait Implementations

// region:    --- Support

pub(crate) fn event_base_error_to_error(error: EventBaseError) -> Error {
	match error {
		EventBaseError::Custom(message) => Error::custom(message),
		EventBaseError::InvalidCapacity { channel, capacity } => Error::InvalidConfiguration(
			format!("event channel {channel} has invalid capacity {capacity}"),
		),
		EventBaseError::TxDisconnected { channel } => Error::MalformedState(format!(
			"event channel {channel} sender disconnected"
		)),
		EventBaseError::RxDisconnected { channel } => Error::MalformedState(format!(
			"event channel {channel} receiver disconnected"
		)),
	}
}

// endregion: --- Support
