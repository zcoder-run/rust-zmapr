use super::{FetchFormat, SanitizePrompt};
use simple_fs::SPath;
use std::path::{Path, PathBuf};

// region:    --- Types

#[doc = include_str!("../../../docs/rustdoc/process/options/process-content-options.md")]
#[derive(Debug, Clone)]
pub struct ProcessContentOptions {
	/// Root directory for cache, stage outputs, manifests, and maps.
	pub destination: Option<SPath>,
	/// Local path or HTTP(S) URL to fetch. When absent, the prior Fetch cache is used.
	pub source: String,
	/// Controls whether the workflow fetches the configured source.
	pub fetch: bool,
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
	pub concurrency: usize,
}

// endregion: --- Types

// region:    --- Constructors

impl ProcessContentOptions {
	/// Creates a workflow with every optional stage disabled.
	pub fn new(source: impl Into<String>) -> Self {
		Self {
			destination: None,
			source: source.into(),
			fetch: true,
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
			concurrency: 8,
		}
	}
}

// endregion: --- Constructors

// region:    --- Chainable

impl ProcessContentOptions {
	/// Sets the destination directory for workflow outputs.
	pub fn with_dest(mut self, destination: impl Into<SPath>) -> Self {
		self.destination = Some(destination.into());
		self
	}

	/// Enables or disables fetching the configured source.
	pub fn with_fetch(mut self, fetch: bool) -> Self {
		self.fetch = fetch;
		self
	}

	/// Replaces the include patterns used to select source content.
	pub fn with_include(mut self, include: impl IntoIterator<Item = impl Into<String>>) -> Self {
		self.include = include.into_iter().map(Into::into).collect();
		self
	}

	/// Adds one include pattern.
	pub fn append_include(mut self, include: impl Into<String>) -> Self {
		self.include.push(include.into());
		self
	}

	/// Adds multiple include patterns.
	pub fn append_includes(mut self, includes: impl IntoIterator<Item = impl Into<String>>) -> Self {
		self.include.extend(includes.into_iter().map(Into::into));
		self
	}

	/// Replaces the exclude patterns used to select source content.
	pub fn with_exclude(mut self, exclude: impl IntoIterator<Item = impl Into<String>>) -> Self {
		self.exclude = exclude.into_iter().map(Into::into).collect();
		self
	}

	/// Adds one exclude pattern.
	pub fn append_exclude(mut self, exclude: impl Into<String>) -> Self {
		self.exclude.push(exclude.into());
		self
	}

	/// Adds multiple exclude patterns.
	pub fn append_excludes(mut self, excludes: impl IntoIterator<Item = impl Into<String>>) -> Self {
		self.exclude.extend(excludes.into_iter().map(Into::into));
		self
	}

	/// Sets the storage format used by Fetch.
	pub fn with_format(mut self, format: FetchFormat) -> Self {
		self.format = format;
		self
	}

	/// Sets the maximum web crawl depth from the starting URL.
	pub fn with_max_depth(mut self, max_depth: usize) -> Self {
		self.max_depth = max_depth;
		self
	}

	/// Enables or disables `llms.txt` discovery for web sources.
	pub fn with_llms(mut self, llms: bool) -> Self {
		self.llms = llms;
		self
	}

	/// Enables or disables the Sanitize stage.
	pub fn with_sanitize(mut self, sanitize: bool) -> Self {
		self.sanitize = sanitize;
		self
	}

	/// Enables or disables the Map stage.
	pub fn with_map(mut self, map: bool) -> Self {
		self.map = map;
		self
	}

	/// Sets the fallback model for enabled AI stages.
	pub fn with_model(mut self, model: impl Into<String>) -> Self {
		self.model = Some(model.into());
		self
	}

	/// Sets the model used by the Sanitize stage.
	pub fn with_sanitize_model(mut self, model: impl Into<String>) -> Self {
		self.sanitize_model = Some(model.into());
		self
	}

	/// Sets the model used by the Map stage.
	pub fn with_map_model(mut self, model: impl Into<String>) -> Self {
		self.map_model = Some(model.into());
		self
	}

	/// Sets custom instructions that replace the built-in Sanitize instructions.
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
	pub fn with_concurrency(mut self, concurrency: usize) -> Self {
		self.concurrency = concurrency;
		self
	}
}

impl ProcessContentOptions {
	pub(crate) fn resolved_destination(&self) -> SPath {
		self.destination
			.clone()
			.unwrap_or_else(|| derive_destination(&self.source))
	}

	pub(crate) fn resolved_sanitize_model(&self) -> Option<&str> {
		self.sanitize_model.as_deref().or(self.model.as_deref())
	}

	pub(crate) fn resolved_map_model(&self) -> Option<&str> {
		self.map_model.as_deref().or(self.model.as_deref())
	}
}

// endregion: --- Chainable

// region:    --- Support

fn derive_destination(source: &str) -> SPath {
	if let Some(web_source) = source
		.strip_prefix("https://")
		.or_else(|| source.strip_prefix("http://"))
	{
		let authority_end = web_source.find(['/', '?', '#']).unwrap_or(web_source.len());
		let authority = &web_source[..authority_end];
		let remainder = &web_source[authority_end..];
		let host = authority.rsplit('@').next().unwrap_or_default();
		let host = sanitize_web_segment(host).unwrap_or_else(|| "source".to_owned());
		let path = remainder
			.split(['?', '#'])
			.next()
			.unwrap_or_default()
			.trim_matches('/');

		let mut destination = PathBuf::from("zmapr");
		destination.push(host);
		let mut path_segments = Vec::new();
		for segment in path.split('/') {
			match segment {
				"" | "." => {}
				".." => {
					let _ = path_segments.pop();
				}
				segment => {
					if let Some(segment) = sanitize_web_segment(segment) {
						path_segments.push(segment);
					}
				}
			}
		}
		for segment in path_segments {
			destination.push(segment);
		}

		return SPath::from(destination.to_string_lossy().into_owned());
	}

	let source_path = Path::new(source);
	let is_directory = source_path.is_dir() || (!source_path.exists() && source_path.extension().is_none());
	let base_name = if is_directory {
		source_path.file_name()
	} else {
		source_path.file_stem()
	};
	let base_name = base_name
		.map(|name| name.to_string_lossy().into_owned())
		.filter(|name| !name.is_empty())
		.unwrap_or_else(|| "source".to_owned());

	let mut destination = source_path
		.parent()
		.filter(|parent| !parent.as_os_str().is_empty())
		.map_or_else(PathBuf::new, Path::to_path_buf);
	destination.push(format!("{base_name}-zmapr"));

	SPath::from(destination.to_string_lossy().into_owned())
}

fn sanitize_web_segment(segment: &str) -> Option<String> {
	if segment.is_empty() || segment == "." || segment == ".." {
		return None;
	}

	let sanitized = segment
		.chars()
		.map(|character| {
			if character.is_ascii_alphanumeric() || matches!(character, '.' | '-' | '_') {
				character
			} else {
				'-'
			}
		})
		.collect::<String>();
	let sanitized = sanitized.trim_matches('.');

	if sanitized.is_empty() {
		None
	} else {
		Some(sanitized.to_owned())
	}
}

// endregion: --- Support

// region:    --- Tests

#[cfg(test)]
mod tests {
	use super::*;
	use std::fs;
	use std::path::{Path, PathBuf};

	type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

	#[test]
	fn resolved_models_prefer_stage_overrides_and_fall_back_to_default() {
		// -- Setup & Fixtures
		let options = ProcessContentOptions::new("source")
			.with_model("default-model")
			.with_sanitize_model("sanitize-model")
			.with_map_model("map-model");

		// -- Check
		assert_eq!(options.resolved_sanitize_model(), Some("sanitize-model"));
		assert_eq!(options.resolved_map_model(), Some("map-model"));

		// -- Exec
		let options = ProcessContentOptions::new("source").with_model("default-model");

		// -- Check
		assert_eq!(options.resolved_sanitize_model(), Some("default-model"));
		assert_eq!(options.resolved_map_model(), Some("default-model"));
	}

	#[test]
	fn test_process_options_resolved_destination_local_directory() -> Result<()> {
		// -- Setup & Fixtures
		let source_path =
			PathBuf::from("tests-data/.tmp/test_process_options_resolved_destination_local_directory/docs");
		fs::create_dir_all(&source_path)?;
		let options = ProcessContentOptions::new(path_text(&source_path));

		// -- Exec
		let destination = options.resolved_destination();

		// -- Check
		let expected = source_path.with_file_name("docs-zmapr");
		assert_eq!(destination.as_std_path(), expected.as_path());

		Ok(())
	}

	#[test]
	fn test_process_options_resolved_destination_local_file() -> Result<()> {
		// -- Setup & Fixtures
		let source_path =
			PathBuf::from("tests-data/.tmp/test_process_options_resolved_destination_local_file/notes/guide.md");
		fs::create_dir_all(source_path.parent().ok_or("expected source parent")?)?;
		fs::write(&source_path, b"# Guide\n")?;
		let options = ProcessContentOptions::new(path_text(&source_path));

		// -- Exec
		let destination = options.resolved_destination();

		// -- Check
		let expected = source_path.with_file_name("guide-zmapr");
		assert_eq!(destination.as_std_path(), expected.as_path());

		Ok(())
	}

	#[test]
	fn test_process_options_resolved_destination_web_source() -> Result<()> {
		// -- Setup & Fixtures
		let options = ProcessContentOptions::new("https://example.com/docs/");

		// -- Exec
		let destination = options.resolved_destination();

		// -- Check
		let expected = PathBuf::from("zmapr").join("example.com").join("docs");
		assert_eq!(destination.as_std_path(), expected.as_path());

		Ok(())
	}

	#[test]
	fn test_process_options_resolved_destination_web_sanitizes_segments() -> Result<()> {
		// -- Setup & Fixtures
		let options = ProcessContentOptions::new("https://example.com/docs/../guide%20one/?version=1#top");

		// -- Exec
		let destination = options.resolved_destination();

		// -- Check
		let expected = PathBuf::from("zmapr").join("example.com").join("guide-20one");
		assert_eq!(destination.as_std_path(), expected.as_path());

		Ok(())
	}

	// -- Test Support
	fn path_text(path: &Path) -> String {
		path.to_string_lossy().replace('\\', "/")
	}
}

// endregion: --- Tests
