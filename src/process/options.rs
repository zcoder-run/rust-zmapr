use simple_fs::SPath;

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

#[derive(Debug, Clone, Default)]
pub struct FetchOptions {
	/// Glob patterns selecting files or website paths to include.
	pub include: Vec<String>,
	/// Glob patterns excluding otherwise selected content.
	pub exclude: Vec<String>,
	/// Copies selected local files into the deterministic cache.
	pub copy_local_files: bool,
	/// Restricts website crawling to the source host.
	pub same_host_only: bool,
	/// Maximum link depth from the starting website URL.
	pub max_depth: usize,
	/// Enables discovery of linked website pages.
	pub follow_links: bool,
}

#[derive(Debug, Clone, Default)]
pub struct SanitizeOptions {
	/// Removes nonessential HTML structure and metadata.
	pub slim_html: bool,
	/// Converts supported content into Markdown.
	pub convert_to_markdown: bool,
}

#[derive(Debug, Clone)]
pub struct AiAugmentOptions {
	/// Identifies the AI provider used to augment content.
	pub provider: String,
	/// Identifies the provider model used to augment content.
	pub model: String,
}

#[derive(Debug, Clone)]
pub struct ContentMapOptions {
	/// Identifies the AI provider used to analyze content.
	pub provider: String,
	/// Identifies the provider model used to analyze content.
	pub model: String,
	/// Optional location of the durable JSONL progress journal.
	pub journal_path: Option<SPath>,
	/// Reuses successful journal records whose source hash is unchanged.
	pub reuse_unchanged_records: bool,
	/// Retains the journal after publishing the completed map.
	pub retain_journal: bool,
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

impl AiAugmentOptions {
	/// Creates AI augmentation options for a provider and model.
	pub fn new(provider: impl Into<String>, model: impl Into<String>) -> Self {
		Self {
			provider: provider.into(),
			model: model.into(),
		}
	}
}

impl ContentMapOptions {
	/// Creates content-map options for a provider and model.
	pub fn new(provider: impl Into<String>, model: impl Into<String>) -> Self {
		Self {
			provider: provider.into(),
			model: model.into(),
			journal_path: None,
			reuse_unchanged_records: true,
			retain_journal: true,
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
}

// endregion: --- Chainable
