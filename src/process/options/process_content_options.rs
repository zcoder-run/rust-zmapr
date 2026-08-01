use simple_fs::SPath;
use super::{AiAugmentOptions, ContentMapOptions, FetchOptions, SanitizeOptions};

// region:    --- Types

#[derive(Debug, Clone)]
pub struct ProcessContentOptions {
	/// Root directory for cache, stage outputs, manifests, and maps.
	pub destination: SPath,
	/// Enables source selection and acquisition when present.
	pub fetch: Option<FetchOptions>,
	/// Enables mechanical content preparation when present.
	pub sanitize: Option<SanitizeOptions>,
	/// Enables AI cleanup and formatting when present.
	pub ai_augment: Option<AiAugmentOptions>,
	/// Enables AI content-map generation when present.
	pub content_map: Option<ContentMapOptions>,
	/// Reuses successful unchanged stage work when possible.
	pub resume: bool,
	/// Limits parallel item processing within a stage.
	pub max_concurrency: usize,
}

// endregion: --- Types

// region:    --- Constructors

impl ProcessContentOptions {
	/// Creates a workflow with every optional stage disabled.
	pub fn new(destination: impl Into<SPath>) -> Self {
		Self {
			destination: destination.into(),
			fetch: None,
			sanitize: None,
			ai_augment: None,
			content_map: None,
			resume: false,
			max_concurrency: 8,
		}
	}
}

// endregion: --- Constructors

// region:    --- Chainable

impl ProcessContentOptions {
	/// Enables and configures Fetch.
	pub fn with_fetch(mut self, options: FetchOptions) -> Self {
		self.fetch = Some(options);
		self
	}

	/// Enables and configures Sanitize.
	pub fn with_sanitize(mut self, options: SanitizeOptions) -> Self {
		self.sanitize = Some(options);
		self
	}

	/// Enables and configures AI Augment.
	pub fn with_ai_augment(mut self, options: AiAugmentOptions) -> Self {
		self.ai_augment = Some(options);
		self
	}

	/// Enables and configures AI Content Map.
	pub fn with_content_map(mut self, options: ContentMapOptions) -> Self {
		self.content_map = Some(options);
		self
	}

	/// Controls whether successful unchanged stage work may be reused.
	pub fn with_resume(mut self, resume: bool) -> Self {
		self.resume = resume;
		self
	}

	/// Sets the maximum parallel item processing within a stage.
	pub fn with_max_concurrency(mut self, max_concurrency: usize) -> Self {
		self.max_concurrency = max_concurrency;
		self
	}
}

// endregion: --- Chainable
