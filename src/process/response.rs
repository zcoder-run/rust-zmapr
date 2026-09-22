use super::progress::{ProcessCompletionRx, ProgressRx, event_base_error_to_error};
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
	/// Aggregate GenAI usage across all completed items, when available.
	pub total_usage: Option<genai::chat::Usage>,
}

#[derive(Debug, Clone)]
pub struct ProcessItem {
	/// Stable source identity or source-relative path.
	pub source: String,
	/// Generated artifact path when one was produced.
	pub output_path: Option<SPath>,
	/// Stage responsible for the outcome.
	pub stage: ProcessStage,
	/// Optional GenAI usage metadata reported for this item.
	pub usage: Option<genai::chat::Usage>,
}

impl ProcessItem {
	pub fn new(source: impl Into<String>, output_path: Option<SPath>, stage: ProcessStage) -> Self {
		Self {
			source: source.into(),
			output_path,
			stage,
			usage: None,
		}
	}

	pub fn with_usage(mut self, usage: genai::chat::Usage) -> Self {
		self.usage = Some(usage);
		self
	}

	pub fn output_path_str_or<'a>(&'a self, fallback: &'a str) -> &'a str {
		self.output_path.as_ref().map(|p| p.as_str()).unwrap_or(fallback)
	}
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

/// Adds token metrics from `other` into `acc`, initializing missing totals as needed.
pub fn accumulate_usage(acc: &mut genai::chat::Usage, other: &genai::chat::Usage) {
	if let Some(tokens) = other.prompt_tokens {
		acc.prompt_tokens = Some(acc.prompt_tokens.unwrap_or(0) + tokens);
	}
	if let Some(tokens) = other.completion_tokens {
		acc.completion_tokens = Some(acc.completion_tokens.unwrap_or(0) + tokens);
	}
	if let Some(tokens) = other.total_tokens {
		acc.total_tokens = Some(acc.total_tokens.unwrap_or(0) + tokens);
	} else if other.prompt_tokens.is_some() || other.completion_tokens.is_some() {
		let prompt = other.prompt_tokens.unwrap_or(0);
		let completion = other.completion_tokens.unwrap_or(0);
		acc.total_tokens = Some(acc.total_tokens.unwrap_or(0) + prompt + completion);
	}
}

/// Calculates aggregate GenAI usage across a collection of process items.
pub fn compute_total_usage<'a>(items: impl IntoIterator<Item = &'a ProcessItem>) -> Option<genai::chat::Usage> {
	let mut total: Option<genai::chat::Usage> = None;
	for item in items {
		if let Some(usage) = &item.usage {
			let acc = total.get_or_insert_with(Default::default);
			accumulate_usage(acc, usage);
		}
	}
	total
}

// endregion: --- Operations

// region:    --- Tests

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn test_accumulate_usage_sums_fields() {
		let mut acc = genai::chat::Usage {
			prompt_tokens: Some(10),
			completion_tokens: Some(20),
			total_tokens: Some(30),
			..Default::default()
		};

		let other = genai::chat::Usage {
			prompt_tokens: Some(5),
			completion_tokens: Some(15),
			total_tokens: Some(20),
			..Default::default()
		};

		accumulate_usage(&mut acc, &other);

		assert_eq!(acc.prompt_tokens, Some(15));
		assert_eq!(acc.completion_tokens, Some(35));
		assert_eq!(acc.total_tokens, Some(50));
	}

	#[test]
	fn test_compute_total_usage_across_items() -> crate::Result<()> {
		let u1 = genai::chat::Usage {
			prompt_tokens: Some(10),
			completion_tokens: Some(20),
			total_tokens: Some(30),
			..Default::default()
		};

		let u2 = genai::chat::Usage {
			prompt_tokens: Some(5),
			completion_tokens: Some(5),
			total_tokens: Some(10),
			..Default::default()
		};

		let items = vec![
			ProcessItem::new("a.md", None, ProcessStage::AiContentMap).with_usage(u1),
			ProcessItem::new("b.md", None, ProcessStage::Fetch),
			ProcessItem::new("c.md", None, ProcessStage::AiContentMap).with_usage(u2),
		];

		let total = compute_total_usage(&items).ok_or("expected total usage")?;
		assert_eq!(total.prompt_tokens, Some(15));
		assert_eq!(total.completion_tokens, Some(25));
		assert_eq!(total.total_tokens, Some(40));
		Ok(())
	}
}

// endregion: --- Tests
