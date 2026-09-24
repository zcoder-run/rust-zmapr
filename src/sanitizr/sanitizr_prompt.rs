use crate::process::SanitizePrompt;
use crate::{Error, Result};

// region:    --- Constants

pub(crate) const SANITIZE_INSTRUCTIONS: &str = "Remove navigation, headers, footers, and boilerplate while preserving substantive content. Add language identifiers to fenced code blocks when they can be determined from the content. Normalize headings and formatting for readability. Never invent facts, examples, or other content. Preserve technical details and relevant links.";

// endregion: --- Constants

// region:    --- Public Functions

pub(crate) fn render_sanitize_prompt(instructions: &str, relative_path: &str, content: &str) -> String {
	format!(
		"{instructions}\n\nFile path: {relative_path}\n\n<SANITIZE_INPUT>\n{content}\n</SANITIZE_INPUT>\n\nReturn only the cleaned content between <SANITIZED_CONTENT> and </SANITIZED_CONTENT> tags."
	)
}

pub(crate) fn parse_sanitized_content(response: &str) -> Result<String> {
	let start_tag = "<SANITIZED_CONTENT>";
	let end_tag = "</SANITIZED_CONTENT>";

	let start_idx = response
		.find(start_tag)
		.ok_or_else(|| Error::custom("missing <SANITIZED_CONTENT> tag in AI response"))?
		+ start_tag.len();

	let end_idx = response[start_idx..]
		.find(end_tag)
		.ok_or_else(|| Error::custom("missing </SANITIZED_CONTENT> tag in AI response"))?
		+ start_idx;

	let content = &response[start_idx..end_idx];
	Ok(strip_one_trailing_newline(strip_one_newline(content)).to_owned())
}

pub(crate) fn resolve_instructions(prompt: Option<&SanitizePrompt>) -> Result<String> {
	match prompt {
		Some(SanitizePrompt::FilePath(path)) => simple_fs::read_to_string(path).map_err(|error| {
			Error::InvalidConfiguration(format!("failed to read Sanitize prompt file {path}: {error}"))
		}),
		Some(SanitizePrompt::Content(content)) => Ok(content.clone()),
		None => Ok(SANITIZE_INSTRUCTIONS.to_owned()),
	}
}

// endregion: --- Public Functions

// region:    --- Support

fn strip_one_newline(value: &str) -> &str {
	value
		.strip_prefix("\r\n")
		.or_else(|| value.strip_prefix('\n'))
		.unwrap_or(value)
}

fn strip_one_trailing_newline(value: &str) -> &str {
	value
		.strip_suffix("\r\n")
		.or_else(|| value.strip_suffix('\n'))
		.unwrap_or(value)
}

// endregion: --- Support
