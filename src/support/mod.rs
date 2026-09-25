// region:    --- Modules

mod time;
mod tasks;

pub(crate) use time::*;
pub(crate) use tasks::*;

// endregion: --- Modules

use crate::{Error, Result};
use htmlr::{SlimOptions, slim, to_md};
use std::path::Path;

pub(crate) fn hash_bytes(bytes: &[u8]) -> String {
	let hash = blake3::hash(bytes);
	bs58::encode(hash.as_bytes()).into_string()
}

pub(crate) fn is_html_item(media_type: Option<&str>, path: impl AsRef<Path>) -> bool {
	let path_ref = path.as_ref();

	if let Some(media_type) = media_type {
		let mime = media_type.split(';').next().unwrap_or(media_type).trim().to_ascii_lowercase();
		if matches!(mime.as_str(), "text/html" | "application/xhtml+xml") {
			return true;
		}
	}

	path_ref
		.extension()
		.and_then(|ext| ext.to_str())
		.is_some_and(|ext| matches!(ext.to_ascii_lowercase().as_str(), "html" | "htm" | "xhtml"))
}

pub(crate) fn slim_html(html: &str) -> Result<String> {
	slim(html, SlimOptions::from_indent(2)).map_err(Error::custom_from_err)
}

pub(crate) fn html_to_markdown(html: &str) -> Result<String> {
	let slimmed = slim_html(html)?;
	to_md(&slimmed, None).map_err(Error::custom_from_err)
}

// region:    --- Tests

#[cfg(test)]
mod tests {
	type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

	use super::*;

	#[test]
	fn test_support_is_html_item() -> Result<()> {
		// -- Exec & Check
		assert!(is_html_item(Some("text/html"), "page"));
		assert!(is_html_item(Some("text/html; charset=utf-8"), "page"));
		assert!(is_html_item(Some("application/xhtml+xml"), "page"));
		assert!(is_html_item(Some("application/octet-stream"), "docs/index.html"));

		assert!(is_html_item(None, "docs/index.html"));
		assert!(is_html_item(None, "docs/index.htm"));
		assert!(is_html_item(None, "docs/index.xhtml"));
		assert!(is_html_item(Some("text/plain"), "docs/index.HTML"));

		assert!(!is_html_item(Some("text/markdown"), "page.md"));
		assert!(!is_html_item(Some("application/json"), "data.json"));
		assert!(!is_html_item(None, "src/main.rs"));
		assert!(!is_html_item(None, "docs/index"));
		assert!(!is_html_item(None, "docs/index.htmlx"));

		Ok(())
	}

	#[test]
	fn test_support_html_to_markdown() -> Result<()> {
		// -- Setup & Fixtures
		let html = r#"<!DOCTYPE html><html><head><title>Doc</title></head><body><h1>Hello</h1><p>Some <strong>bold</strong> text.</p><a href="https://example.com">link</a></body></html>"#;

		// -- Exec
		let markdown = html_to_markdown(html)?;

		// -- Check
		assert!(markdown.contains("Hello"));
		assert!(markdown.contains("bold"));
		assert!(markdown.contains("https://example.com"));
		assert!(!markdown.contains("<h1>"));

		Ok(())
	}

	#[test]
	fn test_support_hash_bytes_stable() -> Result<()> {
		// -- Setup & Fixtures
		let bytes = b"shared content hash";

		// -- Exec
		let hash = hash_bytes(bytes);

		// -- Check
		assert_eq!(hash, bs58::encode(blake3::hash(bytes).as_bytes()).into_string());

		Ok(())
	}
}

// endregion: --- Tests
