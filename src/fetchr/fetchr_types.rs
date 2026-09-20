use serde::{Deserialize, Serialize};
use simple_fs::SPath;

// region:    --- Types

#[derive(Debug, Clone, Default)]
pub struct FetchOptions {
	/// Glob patterns selecting files or web paths to include.
	pub include: Vec<String>,
	/// Glob patterns excluding otherwise selected content.
	pub exclude: Vec<String>,
	/// Copies selected local files into the deterministic cache.
	pub copy_local_files: bool,
	/// Restricts web crawling to the source host.
	pub same_host_only: bool,
	/// Maximum link depth from the starting web URL.
	pub max_depth: usize,
	/// Enables discovery of linked web pages.
	pub follow_links: bool,
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

#[derive(Debug, Deserialize, PartialEq, Eq, Serialize)]
pub(crate) struct FetchManifestOptions {
	pub(crate) include: Vec<String>,
	pub(crate) exclude: Vec<String>,
	pub(crate) copy_local_files: bool,
	pub(crate) same_host_only: bool,
	pub(crate) max_depth: usize,
	pub(crate) follow_links: bool,
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

impl FetchOptions {
	pub fn with_copy_local_files(mut self, copy_local_files: bool) -> Self {
		self.copy_local_files = copy_local_files;
		self
	}

	pub fn with_same_host_only(mut self, same_host_only: bool) -> Self {
		self.same_host_only = same_host_only;
		self
	}

	pub fn with_max_depth(mut self, max_depth: usize) -> Self {
		self.max_depth = max_depth;
		self
	}

	pub fn with_follow_links(mut self, follow_links: bool) -> Self {
		self.follow_links = follow_links;
		self
	}

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

// region:    --- Froms

impl From<&FetchOptions> for FetchManifestOptions {
	fn from(options: &FetchOptions) -> Self {
		Self {
			include: options.include.clone(),
			exclude: options.exclude.clone(),
			copy_local_files: options.copy_local_files,
			same_host_only: options.same_host_only,
			max_depth: options.max_depth,
			follow_links: options.follow_links,
		}
	}
}

// endregion: --- Froms
