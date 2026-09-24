use super::{FetchFormat, SanitizePrompt};
use simple_fs::SPath;

// region:    --- Types

#[derive(Debug, Clone)]
pub struct ProcessContentOptions {
	/// Root directory for cache, stage outputs, manifests, and maps.
	pub destination: SPath,
	/// Local path or HTTP(S) URL to fetch. When absent, the prior Fetch cache is used.
	pub source: Option<String>,
	/// Glob patterns selecting content to include.
	pub include: Vec<String>,
	/// Glob patterns excluding otherwise selected content.
	pub exclude: Vec<String>,
	/// Storage format for fetched HTML content.
	pub format: FetchFormat,
	/// Maximum web link depth from the starting URL.
	pub max_depth: usize,
	/// Enables `llms.txt` discovery for web sources.
	pub llms: bool,
	/// Enables the AI Sanitize stage.
	pub sanitize: bool,
	/// Enables the AI Map stage.
	pub map: bool,
	/// Default model for the AI stages.
	pub model: Option<String>,
	/// Model override for Sanitize.
	pub sanitize_model: Option<String>,
	/// Model override for Map.
	pub map_model: Option<String>,
	/// Custom Sanitize instructions replacing the built-in ones.
	pub sanitize_prompt: Option<SanitizePrompt>,
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
			source: None,
			include: Vec::new(),
			exclude: Vec::new(),
			format: FetchFormat::default(),
			max_depth: 0,
			llms: true,
			sanitize: false,
			map: false,
			model: None,
			sanitize_model: None,
			map_model: None,
			sanitize_prompt: None,
			resume: false,
			max_concurrency: 8,
		}
	}
}

// endregion: --- Constructors

// region:    --- Chainable

impl ProcessContentOptions {
	pub fn with_source(mut self, source: impl Into<String>) -> Self {
		self.source = Some(source.into());
		self
	}

	pub fn with_include(mut self, include: impl IntoIterator<Item = impl Into<String>>) -> Self {
		self.include = include.into_iter().map(Into::into).collect();
		self
	}

	pub fn append_include(mut self, include: impl Into<String>) -> Self {
		self.include.push(include.into());
		self
	}

	pub fn append_includes(mut self, includes: impl IntoIterator<Item = impl Into<String>>) -> Self {
		self.include.extend(includes.into_iter().map(Into::into));
		self
	}

	pub fn with_exclude(mut self, exclude: impl IntoIterator<Item = impl Into<String>>) -> Self {
		self.exclude = exclude.into_iter().map(Into::into).collect();
		self
	}

	pub fn append_exclude(mut self, exclude: impl Into<String>) -> Self {
		self.exclude.push(exclude.into());
		self
	}

	pub fn append_excludes(mut self, excludes: impl IntoIterator<Item = impl Into<String>>) -> Self {
		self.exclude.extend(excludes.into_iter().map(Into::into));
		self
	}

	pub fn with_format(mut self, format: FetchFormat) -> Self {
		self.format = format;
		self
	}

	pub fn with_max_depth(mut self, max_depth: usize) -> Self {
		self.max_depth = max_depth;
		self
	}

	pub fn with_llms(mut self, llms: bool) -> Self {
		self.llms = llms;
		self
	}

	pub fn with_sanitize(mut self, sanitize: bool) -> Self {
		self.sanitize = sanitize;
		self
	}

	pub fn with_map(mut self, map: bool) -> Self {
		self.map = map;
		self
	}

	pub fn with_model(mut self, model: impl Into<String>) -> Self {
		self.model = Some(model.into());
		self
	}

	pub fn with_sanitize_model(mut self, model: impl Into<String>) -> Self {
		self.sanitize_model = Some(model.into());
		self
	}

	pub fn with_map_model(mut self, model: impl Into<String>) -> Self {
		self.map_model = Some(model.into());
		self
	}

	pub fn with_sanitize_prompt(mut self, prompt: SanitizePrompt) -> Self {
		self.sanitize_prompt = Some(prompt);
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

impl ProcessContentOptions {
	pub(crate) fn resolved_sanitize_model(&self) -> Option<&str> {
		self.sanitize_model.as_deref().or(self.model.as_deref())
	}

	pub(crate) fn resolved_map_model(&self) -> Option<&str> {
		self.map_model.as_deref().or(self.model.as_deref())
	}
}

// endregion: --- Chainable

#[cfg(test)]
mod tests {
	use super::ProcessContentOptions;

	#[test]
	fn resolved_models_prefer_stage_overrides_and_fall_back_to_default() {
		let options = ProcessContentOptions::new("destination")
			.with_model("default-model")
			.with_sanitize_model("sanitize-model")
			.with_map_model("map-model");

		assert_eq!(options.resolved_sanitize_model(), Some("sanitize-model"));
		assert_eq!(options.resolved_map_model(), Some("map-model"));

		let options = ProcessContentOptions::new("destination").with_model("default-model");
		assert_eq!(options.resolved_sanitize_model(), Some("default-model"));
		assert_eq!(options.resolved_map_model(), Some("default-model"));
	}
}
