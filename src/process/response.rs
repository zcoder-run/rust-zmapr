use simple_fs::SPath;

// region:    --- Types

#[derive(Debug, Clone)]
pub struct ProcessContentResponse {
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
