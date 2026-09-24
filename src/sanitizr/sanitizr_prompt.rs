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

Response format (required):
- Your entire response must contain the cleaned document enclosed by the exact, case-sensitive tags `<SANITIZED_CONTENT>` and `</SANITIZED_CONTENT>`.
- Always include both tags, even if the cleaned document is empty. Do not rename, omit, or alter them.
- Put only the cleaned document between the tags. Do not include explanations, markdown fences, or any other text outside the tags.
- Do not return the `<SANITIZE_INPUT>` wrapper.

Return the response in this form, replacing the placeholder with the cleaned document:
<SANITIZED_CONTENT>
[cleaned document]
</SANITIZED_CONTENT>
"
	)
}

pub(crate) fn parse_sanitized_content(response: &str) -> Result<String> {
	let start_tag = "<SANITIZED_CONTENT>";
	let end_tag = "</SANITIZED_CONTENT>";

	let start_idx = response
		.find(start_tag)
		.ok_or_else(|| Error::MissingTag(format!("missing {start_tag} tag in AI response")))?
		+ start_tag.len();

	let parts = tag::extract(response, &["SANITIZED_CONTENT"], TagOptions::default());
	let Some(tag_elem) = parts.tag_elems().into_iter().next() else {
		let error = if response.contains(start_tag) {
			Error::MissingTag(format!("missing {end_tag} tag in AI response"))
		} else {
			Error::MissingTag(format!("missing {start_tag} tag in AI response"))
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
	value.strip_prefix("\r\n").or_else(|| value.strip_prefix('\n')).unwrap_or(value)
}

fn strip_one_trailing_newline(value: &str) -> &str {
	value.strip_suffix("\r\n").or_else(|| value.strip_suffix('\n')).unwrap_or(value)
}

// endregion: --- Support

// region:    --- Tests

#[cfg(test)]
mod tests {
	type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

	use super::*;

	#[test]
	fn test_sanitizr_sanitizr_prompt_render_sanitize_prompt_requires_output_tags() -> Result<()> {
		// -- Setup & Fixtures
		let instructions = "Remove boilerplate.";
		let relative_path = "guide.md";
		let content = "# Guide";

		// -- Exec
		let prompt = render_sanitize_prompt(instructions, relative_path, content);

		// -- Check
		assert!(prompt.contains("exact, case-sensitive tags `<SANITIZED_CONTENT>` and `</SANITIZED_CONTENT>`"));
		assert!(prompt.contains("Always include both tags"));
		assert!(prompt.contains("Do not include explanations, markdown fences, or any other text outside the tags"));
		assert!(prompt.contains("Do not return the `<SANITIZE_INPUT>` wrapper"));

		Ok(())
	}

	#[test]
	fn test_sanitizr_sanitizr_prompt_parse_sanitized_content_missing_start_tag() -> Result<()> {
		// -- Setup & Fixtures
		let response = "</SANITIZED_CONTENT>";

		// -- Exec
		let error = parse_sanitized_content(response)
			.err()
			.ok_or("expected missing opening tag error")?;

		// -- Check
		assert!(matches!(&error, Error::MissingTag(_)));
		assert!(error.to_string().contains("missing <SANITIZED_CONTENT> tag"));

		Ok(())
	}

	#[test]
	fn test_sanitizr_sanitizr_prompt_parse_sanitized_content_missing_end_tag() -> Result<()> {
		// -- Setup & Fixtures
		let response = "<SANITIZED_CONTENT>content";

		// -- Exec
		let error = parse_sanitized_content(response)
			.err()
			.ok_or("expected missing closing tag error")?;

		// -- Check
		assert!(matches!(&error, Error::MissingTag(_)));
		assert!(error.to_string().contains("missing </SANITIZED_CONTENT> tag"));

		Ok(())
	}
}

// endregion: --- Tests
