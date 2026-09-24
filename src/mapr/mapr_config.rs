// region:    --- Types

pub(crate) struct MapConfig {
	pub(crate) model: String,
	pub(crate) max_size: Option<usize>,
	pub(crate) reuse_unchanged_records: bool,
	pub(crate) retain_journal: bool,
}

// endregion: --- Types
