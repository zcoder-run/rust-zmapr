use super::progress::{event_base_error_to_error, ProcessCompletionRx, ProgressRx};
use super::state::ProcessQuery;
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
pub struct ProcessContentOutput {
	/// Root directory containing generated workflow artifacts.
	pub destination: SPath,
	/// Durable workflow manifest when one was written.
	pub manifest_path: Option<SPath>,
	/// Latest content artifact root produced by the selected stages.
	pub content_root: SPath,
	/// Published `content-map.json` when mapping was selected.
	pub content_map_path: Option<SPath>,
	/// Items completed by the selected stages.
	pub completed_items: Vec<ProcessItem>,
	/// Items intentionally skipped or reused from prior work.
	pub skipped_items: Vec<ProcessItem>,
	/// Item-level failures retained for observability and retry.
	pub failures: Vec<ProcessFailure>,
}

#[derive(Debug, Clone)]
pub struct ProcessItem {
	/// Stable source identity or source-relative path.
	pub source: String,
	/// Generated artifact path when one was produced.
	pub output_path: Option<SPath>,
	/// Stage responsible for the outcome.
	pub stage: ProcessStage,
}

#[derive(Debug, Clone)]
pub struct ProcessFailure {
	/// Failed item and its responsible stage.
	pub item: ProcessItem,
	/// Human-readable failure detail.
	pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcessStage {
	Fetch,
	Sanitize,
	AiAugment,
	AiContentMap,
}

// endregion: --- Types
 
// region:    --- Constructors

impl ProcessContentHandle {
	pub(crate) fn new(
		progress_rx: ProgressRx,
		final_rx: ProcessCompletionRx,
		query: ProcessQuery,
	) -> Self {
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
		self
			.final_rx
			.recv()
			.await
			.map_err(event_base_error_to_error)?
	}
}

// endregion: --- Operations
