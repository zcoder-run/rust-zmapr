use simple_fs::SPath;

// region:    --- Types

#[derive(Debug, Clone)]
pub enum ContentSource {
	LocalPath(LocalContentSource),
	Web(WebContentSource),
}

#[derive(Debug, Clone)]
pub struct LocalContentSource {
	/// File or directory from which Fetch selects content.
	pub path: SPath,
}

#[derive(Debug, Clone)]
pub struct WebContentSource {
	/// Absolute web URL at which Fetch starts.
	pub url: String,
}

// endregion: --- Types

// region:    --- Constructors

impl ContentSource {
	pub fn local(path: impl Into<SPath>) -> Self {
		Self::LocalPath(LocalContentSource::new(path))
	}

	pub fn web(url: impl Into<String>) -> Self {
		Self::Web(WebContentSource::new(url))
	}
}

impl LocalContentSource {
	pub fn new(path: impl Into<SPath>) -> Self {
		Self { path: path.into() }
	}
}

impl WebContentSource {
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

impl From<WebContentSource> for ContentSource {
	fn from(source: WebContentSource) -> Self {
		Self::Web(source)
	}
}

impl From<SPath> for ContentSource {
	fn from(path: SPath) -> Self {
		Self::local(path)
	}
}

// endregion: --- Froms
