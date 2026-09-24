use crate::process::SanitizePrompt;
use crate::{Error, Result};
use markex::tag::{self, TagOptions};

// region:    --- Constants

pub(crate) const SANITIZE_INSTRUCTIONS: &str = "Remove navigation, headers, footers, and boilerplate while preserving substantive content. Add language identifiers to fenced code blocks when they can be determined from the content. Normalize headings and formatting for readability. Never invent facts, examples, or other content. Preserve technical details and relevant links.";

// endregion: --- Constants

// region:    --- Public Functions

pub(crate) fn render_sanitize_prompt(instructions: &str, relative_path: &str, content: &str) -> String {
	format!(
"{instructions}

File path: {relative_path}

<SANITIZE_INPUT>
{content}
</SANITIZE_INPUT>

Return only the cleaned content between <SANITIZED_CONTENT> and </SANITIZED_CONTENT> tags."
	)
}

pub(crate) fn parse_sanitized_content(response: &str) -> Result<String> {
	let start_tag = "<SANITIZED_CONTENT>";
	let end_tag = "</SANITIZED_CONTENT>";

	let start_idx = response
		.find(start_tag)
		.ok_or_else(|| Error::custom("missing <SANITIZED_CONTENT> tag in AI response"))?
		+ start_tag.len();

	let parts = tag::extract(response, &["SANITIZED_CONTENT"], TagOptions::default());
	let Some(tag_elem) = parts.tag_elems().into_iter().next() else {
		let error = if response.contains(start_tag) {
			Error::custom(format!("missing {end_tag} tag in AI response"))
		} else {
			Error::custom(format!("missing {start_tag} tag in AI response"))
		};
		return Err(error);
	};
	Ok(strip_one_trailing_newline(strip_one_newline(&tag_elem.content)).to_owned())
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
