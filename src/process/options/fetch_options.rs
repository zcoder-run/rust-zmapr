// region:    --- Types

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

	pub fn with_include(
		mut self,
		include: impl IntoIterator<Item = impl Into<String>>,
	) -> Self {
		self.include = include.into_iter().map(|value| value.into()).collect();
		self
	}

	pub fn with_exclude(
		mut self,
		exclude: impl IntoIterator<Item = impl Into<String>>,
	) -> Self {
		self.exclude = exclude.into_iter().map(|value| value.into()).collect();
		self
	}

	pub fn append_include(mut self, include: impl Into<String>) -> Self {
		self.include.push(include.into());
		self
	}

	pub fn append_includes(
		mut self,
		includes: impl IntoIterator<Item = impl Into<String>>,
	) -> Self {
		self.include
			.extend(includes.into_iter().map(|value| value.into()));
		self
	}

	pub fn append_exclude(mut self, exclude: impl Into<String>) -> Self {
		self.exclude.push(exclude.into());
		self
	}

	pub fn append_excludes(
		mut self,
		excludes: impl IntoIterator<Item = impl Into<String>>,
	) -> Self {
		self.exclude
			.extend(excludes.into_iter().map(|value| value.into()));
		self
	}
}
