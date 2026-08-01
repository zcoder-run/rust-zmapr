// region:    --- Types

#[derive(Debug, Clone, Default)]
pub struct SanitizeOptions {
	/// Removes nonessential HTML structure and metadata.
	pub slim_html: bool,
	/// Converts supported content into Markdown.
	pub convert_to_markdown: bool,
}

// endregion: --- Types

impl SanitizeOptions {
	pub fn with_slim_html(mut self, slim_html: bool) -> Self {
		self.slim_html = slim_html;
		self
	}

	pub fn with_convert_to_markdown(mut self, convert_to_markdown: bool) -> Self {
		self.convert_to_markdown = convert_to_markdown;
		self
	}
}
