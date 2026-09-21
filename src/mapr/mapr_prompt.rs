use crate::mapr::FileMapEntry;
use crate::{Error, Result};
use aho_corasick::AhoCorasick;
use serde::Deserialize;
use std::sync::LazyLock;

// region:    --- Constants

pub const PROMPT_VERSION: u32 = 2;

static PROMPT_TEMPLATE: &str = include_str!("content-map.tmpl");

static PROMPT_AC: LazyLock<std::result::Result<AhoCorasick, String>> =
	LazyLock::new(|| AhoCorasick::new(["{{file_path}}", "{{file_content}}"]).map_err(|e| e.to_string()));

// endregion: --- Constants

// region:    --- Types

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum StringOrVec {
	Single(String),
	Multiple(Vec<String>),
}

impl StringOrVec {
	fn into_vec(self) -> Vec<String> {
		match self {
			Self::Single(s) => s
				.split(',')
				.map(|part| part.trim().to_string())
				.filter(|part| !part.is_empty())
				.collect(),
			Self::Multiple(vec) => vec,
		}
	}
}

#[derive(Debug, Deserialize)]
struct RawFileInfo {
	#[serde(default)]
	summary: String,

	#[serde(default)]
	when_to_use: String,

	#[serde(default)]
	public_types: Option<StringOrVec>,

	#[serde(default)]
	public_functions: Option<StringOrVec>,

	#[serde(default)]
	topics: Option<StringOrVec>,
}

// endregion: --- Types

// region:    --- Public Functions

pub fn render_file_prompt(file_path: &str, file_content: &str) -> Result<String> {
	let ac = PROMPT_AC
		.as_ref()
		.map_err(|err| Error::custom(format!("failed to initialize prompt template: {err}")))?;

	Ok(ac.replace_all(PROMPT_TEMPLATE, &[file_path, file_content]))
}

pub fn parse_file_info(response: &str) -> Result<FileMapEntry> {
	let start_tag = "<FILE_INFO>";
	let end_tag = "</FILE_INFO>";

	let start_idx = response
		.find(start_tag)
		.ok_or_else(|| Error::custom("missing <FILE_INFO> tag in AI response"))?
		+ start_tag.len();

	let end_idx = response[start_idx..]
		.find(end_tag)
		.ok_or_else(|| Error::custom("missing </FILE_INFO> tag in AI response"))?
		+ start_idx;

	let content_between = &response[start_idx..end_idx];
	let clean_json = strip_markdown_fences(content_between);

	let raw_info: RawFileInfo = serde_json::from_str(clean_json)
		.map_err(|err| Error::custom(format!("failed to parse FILE_INFO JSON: {err}")))?;

	let public_types = raw_info
		.public_types
		.map(|t| t.into_vec())
		.unwrap_or_default()
		.into_iter()
		.map(|s| s.trim().to_string())
		.filter(|s| !s.is_empty())
		.collect();

	let public_functions = raw_info
		.public_functions
		.map(|t| t.into_vec())
		.unwrap_or_default()
		.into_iter()
		.map(|s| s.trim().to_string())
		.filter(|s| !s.is_empty())
		.collect();

	let raw_topics = raw_info.topics.map(|t| t.into_vec()).unwrap_or_default();
	let topics = normalize_topics(raw_topics);

	Ok(FileMapEntry {
		summary: raw_info.summary,
		when_to_use: raw_info.when_to_use,
		public_types,
		public_functions,
		topics,
	})
}

// endregion: --- Public Functions

// region:    --- Support

fn normalize_topics(raw_topics: Vec<String>) -> Vec<String> {
	let mut normalized = Vec::new();
	for topic in raw_topics {
		let words: Vec<&str> = topic.split_whitespace().collect();
		if !words.is_empty() && normalized.len() < 7 {
			let topic_words = if words.len() > 3 {
				&words[..3]
			} else {
				&words[..]
			};
			normalized.push(topic_words.join(" "));
		}
	}
	normalized
}

fn strip_markdown_fences(raw: &str) -> &str {
	let trimmed = raw.trim();
	if let Some(rest) = trimmed.strip_prefix("```") {
		let inner = if let Some(newline_pos) = rest.find('\n') {
			&rest[newline_pos + 1..]
		} else if let Some(without_json) = rest.strip_prefix("json") {
			without_json
		} else {
			rest
		};
		let inner = inner.trim();
		inner.strip_suffix("```").map(|s| s.trim()).unwrap_or(inner)
	} else {
		trimmed
	}
}

// endregion: --- Support

// region:    --- Tests

#[cfg(test)]
mod tests {
	type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

	use super::*;

	#[test]
	fn test_render_file_prompt_replaces_placeholders() -> Result<()> {
		// -- Exec
		let rendered = render_file_prompt("src/lib.rs", "pub fn answer() -> u32 { 42 }")?;

		// -- Check
		assert!(rendered.contains("<FILE_CONTENT file_path=\"src/lib.rs\">"));
		assert!(rendered.contains("pub fn answer() -> u32 { 42 }"));
		assert!(!rendered.contains("{{file_path}}"));
		assert!(!rendered.contains("{{file_content}}"));

		Ok(())
	}

	#[test]
	fn test_parse_file_info_clean() -> Result<()> {
		// -- Setup & Fixtures
		let raw = r#"<FILE_INFO>
{
  "summary": "Module summary",
  "when_to_use": "When parsing files",
  "public_types": ["MyType"],
  "public_functions": ["my_func"],
  "topics": ["parsing"]
}
</FILE_INFO>"#;

		// -- Exec
		let entry = parse_file_info(raw)?;

		// -- Check
		assert_eq!(entry.summary, "Module summary");
		assert_eq!(entry.when_to_use, "When parsing files");
		assert_eq!(entry.public_types, vec!["MyType".to_string()]);
		assert_eq!(entry.public_functions, vec!["my_func".to_string()]);
		assert_eq!(entry.topics, vec!["parsing".to_string()]);

		Ok(())
	}

	#[test]
	fn test_parse_file_info_normalizes_topics_and_string_fallback() -> Result<()> {
		// -- Setup & Fixtures
		let raw = r#"<FILE_INFO>
{
  "summary": "Topics test",
  "when_to_use": "Testing normalization",
  "public_types": "TypeA, TypeB",
  "public_functions": "func_a, func_b",
  "topics": "rust programming language details, quick start, cli"
}
</FILE_INFO>"#;

		// -- Exec
		let entry = parse_file_info(raw)?;

		// -- Check
		assert_eq!(entry.public_types, vec!["TypeA".to_string(), "TypeB".to_string()]);
		assert_eq!(entry.public_functions, vec!["func_a".to_string(), "func_b".to_string()]);
		assert_eq!(
			entry.topics,
			vec![
				"rust programming language".to_string(),
				"quick start".to_string(),
				"cli".to_string(),
			]
		);

		Ok(())
	}

	#[test]
	fn test_parse_file_info_fenced() -> Result<()> {
		// -- Setup & Fixtures
		let raw = "<FILE_INFO>\n```json\n{\n  \"summary\": \"Fenced\",\n  \"when_to_use\": \"Usage\",\n  \"public_types\": [],\n  \"public_functions\": [],\n  \"topics\": []\n}\n```\n</FILE_INFO>";

		// -- Exec
		let entry = parse_file_info(raw)?;

		// -- Check
		assert_eq!(entry.summary, "Fenced");
		assert_eq!(entry.when_to_use, "Usage");

		Ok(())
	}

	#[test]
	fn test_parse_file_info_fenced_no_lang() -> Result<()> {
		// -- Setup & Fixtures
		let raw =
			"<FILE_INFO>\n```\n{\n  \"summary\": \"No lang fence\",\n  \"when_to_use\": \"Test\"\n}\n```\n</FILE_INFO>";

		// -- Exec
		let entry = parse_file_info(raw)?;

		// -- Check
		assert_eq!(entry.summary, "No lang fence");
		assert_eq!(entry.when_to_use, "Test");
		assert!(entry.public_types.is_empty());
		assert!(entry.public_functions.is_empty());

		Ok(())
	}

	#[test]
	fn test_parse_file_info_tolerates_extra_fields() -> Result<()> {
		// -- Setup & Fixtures
		let raw = r#"<FILE_INFO>
{
  "file_path": "src/main.rs",
  "kind": "text",
  "summary": "Main entry",
  "when_to_use": "CLI startup",
  "extra_ignored": 123
}
</FILE_INFO>"#;

		// -- Exec
		let entry = parse_file_info(raw)?;

		// -- Check
		assert_eq!(entry.summary, "Main entry");
		assert_eq!(entry.when_to_use, "CLI startup");

		Ok(())
	}

	#[test]
	fn test_parse_file_info_missing_start_tag() -> Result<()> {
		// -- Setup & Fixtures
		let raw = "{\"summary\": \"No tag\"}</FILE_INFO>";

		// -- Exec
		let res = parse_file_info(raw);

		// -- Check
		assert!(res.is_err());
		let err_msg = res.err().ok_or("expected error")?.to_string();
		assert!(err_msg.contains("missing <FILE_INFO> tag"));

		Ok(())
	}

	#[test]
	fn test_parse_file_info_missing_end_tag() -> Result<()> {
		// -- Setup & Fixtures
		let raw = "<FILE_INFO>{\"summary\": \"No end tag\"}";

		// -- Exec
		let res = parse_file_info(raw);

		// -- Check
		assert!(res.is_err());
		let err_msg = res.err().ok_or("expected error")?.to_string();
		assert!(err_msg.contains("missing </FILE_INFO> tag"));

		Ok(())
	}

	#[test]
	fn test_parse_file_info_malformed_json() -> Result<()> {
		// -- Setup & Fixtures
		let raw = "<FILE_INFO>{ not valid json }</FILE_INFO>";

		// -- Exec
		let res = parse_file_info(raw);

		// -- Check
		assert!(res.is_err());
		let err_msg = res.err().ok_or("expected error")?.to_string();
		assert!(err_msg.contains("failed to parse FILE_INFO JSON"));

		Ok(())
	}
}

// endregion: --- Tests
