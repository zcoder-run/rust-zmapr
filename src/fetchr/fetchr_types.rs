use serde::{Deserialize, Serialize};
use simple_fs::SPath;

use crate::process::{LocalContentSource, WebContentSource};

// region:    --- Types

#[derive(Debug, Clone, Default)]
pub struct FetchCommonOptions {
	/// Glob patterns selecting files or web paths to include.
	pub include: Vec<String>,
	/// Glob patterns excluding otherwise selected content.
	pub exclude: Vec<String>,
}

#[derive(Debug, Clone, Default)]
pub struct LocalFetchOptions {
	/// Copies selected local files into the deterministic cache.
	pub copy_local_files: bool,
}

#[derive(Debug, Clone)]
pub struct WebFetchOptions {
	/// Restricts web crawling to the source host.
	pub same_host_only: bool,
	/// Enables discovery of linked web pages.
	pub follow_links: bool,
	/// Maximum link depth from the starting web URL.
	pub max_depth: usize,
	/// Enables `llms.txt` driven discovery at the base remote directory.
	pub llms: Option<bool>,
}

impl Default for WebFetchOptions {
	fn default() -> Self {
		Self {
			same_host_only: true,
			follow_links: false,
			max_depth: 0,
			llms: None,
		}
	}
}

impl FetchCommonOptions {
	pub fn with_include(mut self, include: impl IntoIterator<Item = impl Into<String>>) -> Self {
		self.include = include.into_iter().map(|value| value.into()).collect();
		self
	}

	pub fn with_exclude(mut self, exclude: impl IntoIterator<Item = impl Into<String>>) -> Self {
		self.exclude = exclude.into_iter().map(|value| value.into()).collect();
		self
	}

	pub fn append_include(mut self, include: impl Into<String>) -> Self {
		self.include.push(include.into());
		self
	}

	pub fn append_includes(mut self, includes: impl IntoIterator<Item = impl Into<String>>) -> Self {
		self.include.extend(includes.into_iter().map(|value| value.into()));
		self
	}

	pub fn append_exclude(mut self, exclude: impl Into<String>) -> Self {
		self.exclude.push(exclude.into());
		self
	}

	pub fn append_excludes(mut self, excludes: impl IntoIterator<Item = impl Into<String>>) -> Self {
		self.exclude.extend(excludes.into_iter().map(|value| value.into()));
		self
	}
}

impl LocalFetchOptions {
	pub fn with_copy_local_files(mut self, copy_local_files: bool) -> Self {
		self.copy_local_files = copy_local_files;
		self
	}
}

impl WebFetchOptions {
	pub fn with_same_host_only(mut self, same_host_only: bool) -> Self {
		self.same_host_only = same_host_only;
		self
	}

	pub fn with_follow_links(mut self, follow_links: bool) -> Self {
		self.follow_links = follow_links;
		self
	}

	pub fn with_max_depth(mut self, max_depth: usize) -> Self {
		self.max_depth = max_depth;
		self
	}

	pub fn with_llms(mut self, llms: bool) -> Self {
		self.llms = Some(llms);
		self
	}

	pub(crate) fn llms_enabled(&self) -> bool {
		self.llms.unwrap_or(false)
	}
}

#[derive(Debug, Clone)]
pub struct LocalFetchRequest {
	pub source: LocalContentSource,
	pub common: FetchCommonOptions,
	pub options: LocalFetchOptions,
}

impl LocalFetchRequest {
	pub fn new(path: impl Into<SPath>) -> Self {
		Self {
			source: LocalContentSource::new(path),
			common: FetchCommonOptions::default(),
			options: LocalFetchOptions::default(),
		}
	}

	pub fn with_copy_local_files(mut self, copy_local_files: bool) -> Self {
		self.options.copy_local_files = copy_local_files;
		self
	}

	pub fn with_include(mut self, include: impl IntoIterator<Item = impl Into<String>>) -> Self {
		self.common.include = include.into_iter().map(|value| value.into()).collect();
		self
	}

	pub fn with_exclude(mut self, exclude: impl IntoIterator<Item = impl Into<String>>) -> Self {
		self.common.exclude = exclude.into_iter().map(|value| value.into()).collect();
		self
	}

	pub fn append_include(mut self, include: impl Into<String>) -> Self {
		self.common.include.push(include.into());
		self
	}

	pub fn append_includes(mut self, includes: impl IntoIterator<Item = impl Into<String>>) -> Self {
		self.common.include.extend(includes.into_iter().map(|value| value.into()));
		self
	}

	pub fn append_exclude(mut self, exclude: impl Into<String>) -> Self {
		self.common.exclude.push(exclude.into());
		self
	}

	pub fn append_excludes(mut self, excludes: impl IntoIterator<Item = impl Into<String>>) -> Self {
		self.common.exclude.extend(excludes.into_iter().map(|value| value.into()));
		self
	}
}

#[derive(Debug, Clone)]
pub struct WebFetchRequest {
	pub source: WebContentSource,
	pub common: FetchCommonOptions,
	pub options: WebFetchOptions,
}

impl WebFetchRequest {
	pub fn new(url: impl Into<String>) -> Self {
		Self {
			source: WebContentSource::new(url),
			common: FetchCommonOptions::default(),
			options: WebFetchOptions::default(),
		}
	}

	pub fn with_same_host_only(mut self, same_host_only: bool) -> Self {
		self.options.same_host_only = same_host_only;
		self
	}

	pub fn with_follow_links(mut self, follow_links: bool) -> Self {
		self.options.follow_links = follow_links;
		self
	}

	pub fn with_max_depth(mut self, max_depth: usize) -> Self {
		self.options.max_depth = max_depth;
		self
	}

	pub fn with_llms(mut self, llms: bool) -> Self {
		self.options.llms = Some(llms);
		self
	}

	pub fn with_include(mut self, include: impl IntoIterator<Item = impl Into<String>>) -> Self {
		self.common.include = include.into_iter().map(|value| value.into()).collect();
		self
	}

	pub fn with_exclude(mut self, exclude: impl IntoIterator<Item = impl Into<String>>) -> Self {
		self.common.exclude = exclude.into_iter().map(|value| value.into()).collect();
		self
	}

	pub fn append_include(mut self, include: impl Into<String>) -> Self {
		self.common.include.push(include.into());
		self
	}

	pub fn append_includes(mut self, includes: impl IntoIterator<Item = impl Into<String>>) -> Self {
		self.common.include.extend(includes.into_iter().map(|value| value.into()));
		self
	}

	pub fn append_exclude(mut self, exclude: impl Into<String>) -> Self {
		self.common.exclude.push(exclude.into());
		self
	}

	pub fn append_excludes(mut self, excludes: impl IntoIterator<Item = impl Into<String>>) -> Self {
		self.common.exclude.extend(excludes.into_iter().map(|value| value.into()));
		self
	}
}

#[derive(Debug, Clone)]
pub enum FetchRequest {
	Local(LocalFetchRequest),
	Web(WebFetchRequest),
}

#[derive(Debug, Clone)]
pub(crate) struct LocalFetchDiscovery {
	pub(crate) source: String,
	pub(crate) source_path: SPath,
	pub(crate) items: Vec<LocalFetchItem>,
}

#[derive(Debug, Clone)]
pub(crate) struct LocalFetchItem {
	pub(crate) source: String,
	pub(crate) relative_path: String,
	pub(crate) local_path: SPath,
	pub(crate) media_type: Option<String>,
	pub(crate) content_hash: String,
}

pub(crate) const FETCH_MANIFEST_VERSION: u32 = 3;

#[derive(Debug, Deserialize, Serialize)]
pub(crate) struct FetchManifest {
	pub(crate) version: u32,
	pub(crate) complete: bool,
	pub(crate) source: String,
	pub(crate) source_path: String,
	pub(crate) options: FetchManifestOptions,
	pub(crate) artifact_root: String,
	pub(crate) items: Vec<FetchManifestItem>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(crate) enum FetchManifestOptions {
	Local {
		include: Vec<String>,
		exclude: Vec<String>,
		copy_local_files: bool,
	},
	Web {
		include: Vec<String>,
		exclude: Vec<String>,
		same_host_only: bool,
		follow_links: bool,
		max_depth: usize,
		llms: Option<bool>,
	},
}

#[derive(Debug, Deserialize, Serialize)]
pub(crate) struct FetchManifestItem {
	pub(crate) source: String,
	pub(crate) relative_path: String,
	pub(crate) local_path: String,
	pub(crate) artifact_path: Option<String>,
	pub(crate) media_type: Option<String>,
	pub(crate) content_hash: String,
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum LocalSourceKind {
	File,
	Directory,
}

// endregion: --- Types

// region:    --- Froms

impl From<LocalFetchRequest> for FetchRequest {
	fn from(request: LocalFetchRequest) -> Self {
		Self::Local(request)
	}
}

impl From<WebFetchRequest> for FetchRequest {
	fn from(request: WebFetchRequest) -> Self {
		Self::Web(request)
	}
}

impl From<&LocalFetchRequest> for FetchManifestOptions {
	fn from(request: &LocalFetchRequest) -> Self {
		Self::Local {
			include: request.common.include.clone(),
			exclude: request.common.exclude.clone(),
			copy_local_files: request.options.copy_local_files,
		}
	}
}

impl From<&WebFetchRequest> for FetchManifestOptions {
	fn from(request: &WebFetchRequest) -> Self {
		Self::Web {
			include: request.common.include.clone(),
			exclude: request.common.exclude.clone(),
			same_host_only: request.options.same_host_only,
			follow_links: request.options.follow_links,
			max_depth: request.options.max_depth,
			llms: request.options.llms,
		}
	}
}

impl From<&FetchRequest> for FetchManifestOptions {
	fn from(request: &FetchRequest) -> Self {
		match request {
			FetchRequest::Local(local) => Self::from(local),
			FetchRequest::Web(web) => Self::from(web),
		}
	}
}

// endregion: --- Froms

// region:    --- Tests

#[cfg(test)]
mod tests {
	type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>; // For tests.

	use super::*;

	#[test]
	fn test_fetchr_types_local_request_chaining() -> Result<()> {
		// -- Setup & Fixtures
		let request = LocalFetchRequest::new("src")
			.with_copy_local_files(true)
			.with_include(["*.rs"])
			.append_include("*.md")
			.append_includes(["*.txt", "*.json"])
			.with_exclude(["**/target/**"])
			.append_exclude("**/tmp/**")
			.append_excludes(["**/dist/**"]);

		// -- Check
		assert_eq!(request.source.path.to_string(), "src");
		assert!(request.options.copy_local_files);
		assert_eq!(request.common.include, vec!["*.rs", "*.md", "*.txt", "*.json"]);
		assert_eq!(request.common.exclude, vec!["**/target/**", "**/tmp/**", "**/dist/**"]);

		Ok(())
	}

	#[test]
	fn test_fetchr_types_web_request_chaining() -> Result<()> {
		// -- Setup & Fixtures
		let request = WebFetchRequest::new("https://example.com/docs")
			.with_same_host_only(false)
			.with_follow_links(true)
			.with_max_depth(3)
			.with_llms(true)
			.with_include(["*.html"])
			.append_include("*.pdf")
			.with_exclude(["**/login"])
			.append_exclude("**/logout");

		// -- Check
		assert_eq!(request.source.url, "https://example.com/docs");
		assert!(!request.options.same_host_only);
		assert!(request.options.follow_links);
		assert_eq!(request.options.max_depth, 3);
		assert_eq!(request.options.llms, Some(true));
		assert!(request.options.llms_enabled());
		assert_eq!(request.common.include, vec!["*.html", "*.pdf"]);
		assert_eq!(request.common.exclude, vec!["**/login", "**/logout"]);

		Ok(())
	}

	#[test]
	fn test_fetchr_types_web_options_llms_default() -> Result<()> {
		// -- Setup & Fixtures
		let options = WebFetchOptions::default();

		// -- Check
		assert_eq!(options.llms, None);
		assert!(!options.llms_enabled());

		Ok(())
	}

	#[test]
	fn test_fetchr_types_web_request_with_llms() -> Result<()> {
		// -- Setup & Fixtures
		let request = WebFetchRequest::new("https://example.com/docs")
			.with_llms(true)
			.with_follow_links(true)
			.with_max_depth(2);

		// -- Check
		assert_eq!(request.options.llms, Some(true));
		assert!(request.options.llms_enabled());
		assert!(request.options.follow_links);
		assert_eq!(request.options.max_depth, 2);

		Ok(())
	}

	#[test]
	fn test_fetchr_types_web_options_llms_enabled_tri_state() -> Result<()> {
		// -- Setup & Fixtures
		let default_options = WebFetchOptions::default();
		let explicit_false = WebFetchOptions::default().with_llms(false);
		let explicit_true = WebFetchOptions::default().with_llms(true);

		// -- Check
		assert!(!default_options.llms_enabled());
		assert!(!explicit_false.llms_enabled());
		assert!(explicit_true.llms_enabled());

		Ok(())
	}

	#[test]
	fn test_fetchr_types_manifest_options_llms_differentiates() -> Result<()> {
		// -- Setup & Fixtures
		let req_without_llms = WebFetchRequest::new("https://example.com");
		let req_with_llms = WebFetchRequest::new("https://example.com").with_llms(true);

		let options_without = FetchManifestOptions::from(&req_without_llms);
		let options_with = FetchManifestOptions::from(&req_with_llms);

		// -- Check
		assert_ne!(options_without, options_with);

		Ok(())
	}

	#[test]
	fn test_fetchr_types_fetch_request_from() -> Result<()> {
		// -- Setup & Fixtures
		let local_req = LocalFetchRequest::new("src");
		let web_req = WebFetchRequest::new("https://example.com");

		// -- Exec
		let fetch_local: FetchRequest = local_req.into();
		let fetch_web: FetchRequest = web_req.into();

		// -- Check
		if let FetchRequest::Local(req) = fetch_local {
			assert_eq!(req.source.path.to_string(), "src");
		} else {
			return Err("Expected FetchRequest::Local variant".into());
		}

		if let FetchRequest::Web(req) = fetch_web {
			assert_eq!(req.source.url, "https://example.com");
		} else {
			return Err("Expected FetchRequest::Web variant".into());
		}

		Ok(())
	}

	#[test]
	fn test_fetchr_types_manifest_options_serde_tagged_representation() -> Result<()> {
		// -- Setup & Fixtures
		let local_req = LocalFetchRequest::new("src")
			.with_copy_local_files(true)
			.with_include(["*.rs"])
			.with_exclude(["target/**"]);
		let local_options = FetchManifestOptions::from(&local_req);

		let web_req = WebFetchRequest::new("https://example.com")
			.with_same_host_only(true)
			.with_follow_links(true)
			.with_max_depth(2)
			.with_llms(true)
			.with_include(["*.html"]);
		let web_options = FetchManifestOptions::from(&web_req);

		// -- Exec
		let local_json = serde_json::to_string(&local_options)?;
		let local_deserialized: FetchManifestOptions = serde_json::from_str(&local_json)?;

		let web_json = serde_json::to_string(&web_options)?;
		let web_deserialized: FetchManifestOptions = serde_json::from_str(&web_json)?;

		// -- Check
		assert!(local_json.contains(r#""type":"local""#));
		assert!(local_json.contains(r#""copy_local_files":true"#));
		assert_eq!(local_deserialized, local_options);

		assert!(web_json.contains(r#""type":"web""#));
		assert!(web_json.contains(r#""max_depth":2"#));
		assert!(web_json.contains(r#""llms":true"#));
		assert_eq!(web_deserialized, web_options);

		Ok(())
	}
}

// endregion: --- Tests
