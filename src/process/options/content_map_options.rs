use simple_fs::SPath;

// region:    --- Types

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

impl ContentMapOptions {
	pub fn with_provider(mut self, provider: impl Into<String>) -> Self {
		self.provider = provider.into();
		self
	}

	pub fn with_model(mut self, model: impl Into<String>) -> Self {
		self.model = model.into();
		self
	}

	pub fn with_journal_path(mut self, journal_path: impl Into<SPath>) -> Self {
		self.journal_path = Some(journal_path.into());
		self
	}

	pub fn with_reuse_unchanged_records(mut self, reuse_unchanged_records: bool) -> Self {
		self.reuse_unchanged_records = reuse_unchanged_records;
		self
	}

	pub fn with_retain_journal(mut self, retain_journal: bool) -> Self {
		self.retain_journal = retain_journal;
		self
	}
}
