use simple_fs::SPath;

// region:    --- Types

#[derive(Debug, Clone)]
#[doc = include_str!("../../../docs/rustdoc/process/options/sanitize-prompt.md")]
pub enum SanitizePrompt {
	/// A filesystem path containing custom Sanitize instructions.
	FilePath(SPath),
	/// Custom Sanitize instructions supplied directly as text.
	Content(String),
}

// endregion: --- Types

impl SanitizePrompt {
	/// Creates a file-backed prompt.
	pub fn file(path: impl Into<SPath>) -> Self {
		Self::FilePath(path.into())
	}

	/// Creates an inline prompt from `content`.
	pub fn content(content: impl Into<String>) -> Self {
		Self::Content(content.into())
	}
}
