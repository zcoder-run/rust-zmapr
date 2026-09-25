#![doc = include_str!("../../docs/rustdoc/process/source.md")]

use simple_fs::SPath;

// region:    --- Types

#[derive(Debug, Clone)]
/// A typed description of a local or web source for Fetch.
pub enum ContentSource {
	/// Content discovered from a local file or directory.
	LocalPath(LocalContentSource),

	/// Content discovered from an absolute web URL.
	Web(WebContentSource),
}

#[derive(Debug, Clone)]
/// A local file or directory from which Fetch selects content.
pub struct LocalContentSource {
	/// File or directory from which Fetch selects content.
	pub path: SPath,
}

#[derive(Debug, Clone)]
/// A web location at which Fetch begins crawling.
pub struct WebContentSource {
	/// Absolute web URL at which Fetch starts.
	pub url: String,
}

// endregion: --- Types

// region:    --- Constructors

impl ContentSource {
	/// Creates a local source from a path.
	pub fn local(path: impl Into<SPath>) -> Self {
		Self::LocalPath(LocalContentSource::new(path))
	}

	/// Creates a web source from a URL.
	pub fn web(url: impl Into<String>) -> Self {
		Self::Web(WebContentSource::new(url))
	}
}

impl LocalContentSource {
	/// Creates a local source from a path.
	pub fn new(path: impl Into<SPath>) -> Self {
		Self { path: path.into() }
	}
}

impl WebContentSource {
	/// Creates a web source from a URL.
	pub fn new(url: impl Into<String>) -> Self {
		Self { url: url.into() }
	}
}

// endregion: --- Constructors

// region:    --- Froms

impl From<LocalContentSource> for ContentSource {
	/// Converts a local source into a content source.
	fn from(source: LocalContentSource) -> Self {
		Self::LocalPath(source)
	}
}

impl From<WebContentSource> for ContentSource {
	/// Converts a web source into a content source.
	fn from(source: WebContentSource) -> Self {
		Self::Web(source)
	}
}

impl From<SPath> for ContentSource {
	/// Converts a path into a local content source.
	fn from(path: SPath) -> Self {
		Self::local(path)
	}
}

// endregion: --- Froms
