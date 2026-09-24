use simple_fs::SPath;

// region:    --- Types

#[derive(Debug, Clone)]
pub enum SanitizePrompt {
	FilePath(SPath),
	Content(String),
}

// endregion: --- Types

impl SanitizePrompt {
	pub fn file(path: impl Into<SPath>) -> Self {
		Self::FilePath(path.into())
	}

	pub fn content(content: impl Into<String>) -> Self {
		Self::Content(content.into())
	}
}
