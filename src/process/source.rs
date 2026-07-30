use simple_fs::SPath;

// region:    --- Types

#[derive(Debug, Clone)]
pub enum ContentSource {
	LocalPath(LocalContentSource),
	Website(WebsiteContentSource),
}

#[derive(Debug, Clone)]
pub struct LocalContentSource {
	/// File or directory from which Fetch selects content.
	pub path: SPath,
}

#[derive(Debug, Clone)]
pub struct WebsiteContentSource {
	/// Absolute website URL at which Fetch starts.
	pub url: String,
}

// endregion: --- Types

// region:    --- Constructors

impl ContentSource {
	pub fn local_path(path: impl Into<SPath>) -> Self {
		Self::LocalPath(LocalContentSource::new(path))
	}

	pub fn website(url: impl Into<String>) -> Self {
		Self::Website(WebsiteContentSource::new(url))
	}
}

impl LocalContentSource {
	pub fn new(path: impl Into<SPath>) -> Self {
		Self { path: path.into() }
	}
}

impl WebsiteContentSource {
	pub fn new(url: impl Into<String>) -> Self {
		Self { url: url.into() }
	}
}

// endregion: --- Constructors

// region:    --- Froms

impl From<LocalContentSource> for ContentSource {
	fn from(source: LocalContentSource) -> Self {
		Self::LocalPath(source)
	}
}

impl From<WebsiteContentSource> for ContentSource {
	fn from(source: WebsiteContentSource) -> Self {
		Self::Website(source)
	}
}

impl From<SPath> for ContentSource {
	fn from(path: SPath) -> Self {
		Self::local_path(path)
	}
}

// endregion: --- Froms
