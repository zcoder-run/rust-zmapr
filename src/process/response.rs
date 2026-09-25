#![doc = include_str!("../../docs/rustdoc/process/response.md")]

use super::item::ItemState;
use super::progress::{ProcessCompletionRx, ProgressRx, event_base_error_to_error};
use super::state::ProcessQuery;
use super::stats::FinalStats;
use crate::Result;
use simple_fs::SPath;

// region:    --- Types

/// Provides progress observation and final output for a running workflow.
pub struct ProcessContentHandle {
	progress_rx: Option<ProgressRx>,
	final_rx: ProcessCompletionRx,
	query: ProcessQuery,
}

#[derive(Debug, Clone)]
/// The successful result of a completed content-processing workflow.
pub struct ProcessContentOutput {
	/// Root directory containing generated workflow artifacts.
	pub destination: SPath,
	/// Durable workflow manifest when one was written.
	pub manifest_path: Option<SPath>,
	/// Destination root containing the published final content.
	pub content_root: SPath,
	/// Published `content-map.json` when mapping was selected.
	pub content_map_path: Option<SPath>,
	/// Final state of all registered workflow items.
	pub items: Vec<ItemState>,
	/// Validated statistics for the completed workflow.
	pub stats: FinalStats,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcessStage {
	/// Retrieves source content into the workflow destination.
	Fetch,

	/// Sanitizes fetched content.
	Sanitize,

	/// Builds a content map from processed content.
	Map,
}

// endregion: --- Types

// region:    --- Constructors

impl ProcessContentHandle {
	pub(crate) fn new(progress_rx: ProgressRx, final_rx: ProcessCompletionRx, query: ProcessQuery) -> Self {
		Self {
			progress_rx: Some(progress_rx),
			final_rx,
			query,
		}
	}
}

// endregion: --- Constructors

// region:    --- Operations

impl ProcessContentHandle {
	/// Transfers ownership of the single progress receiver.
	pub fn take_progress_rx(&mut self) -> Option<ProgressRx> {
		self.progress_rx.take()
	}

	/// Returns a read-only query handle for authoritative in-memory state.
	pub fn query(&self) -> ProcessQuery {
		self.query.clone()
	}

	/// Waits for the completed workflow output.
	pub async fn wait_output(self) -> Result<ProcessContentOutput> {
		self.final_rx.recv().await.map_err(event_base_error_to_error)?
	}
}

// endregion: --- Operations
