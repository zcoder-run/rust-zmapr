use super::fetchr_types::FetchManifest;
use crate::fetchr::FetchCommonOptions;
use crate::process::FetchFormat;
use crate::support::{html_to_markdown, is_html_item, slim_html};
use crate::{Error, Result};
use simple_fs::{SPath, ensure_dir};
use std::fs::{rename, write};
use std::path::{Component, Path, PathBuf};

// region:    --- Support

pub(crate) fn ensure_parent(path: &SPath) -> Result<()> {
	let path_ref: &Path = path.as_ref();
	let parent = path_ref
		.parent()
		.ok_or_else(|| Error::MalformedState(format!("workflow path has no parent: {path}")))?;
	let parent = SPath::from(parent.to_string_lossy().into_owned());
	ensure_dir(&parent)?;
	Ok(())
}

pub(crate) fn write_fetch_artifact(path: &SPath, contents: &[u8]) -> Result<()> {
	ensure_parent(path)?;
	let temporary_path = SPath::from(format!("{path}.tmp"));
	write(temporary_path.as_std_path(), contents)
		.map_err(|error| Error::MalformedState(format!("failed to write Fetch artifact {temporary_path}: {error}")))?;
	rename(temporary_path.as_std_path(), path.as_std_path())
		.map_err(|error| Error::MalformedState(format!("failed to replace Fetch artifact {path}: {error}")))?;
	Ok(())
}

pub(crate) fn path_to_string(path: &SPath) -> Result<String> {
	let path_ref: &Path = path.as_ref();
	path_ref
		.to_str()
		.map(|value| value.replace('\\', "/"))
		.ok_or_else(|| Error::MalformedState("workflow path is not valid UTF-8".to_owned()))
}

pub(crate) fn paths_equivalent(left: &Path, right: &Path) -> bool {
	left.components().eq(right.components())
}

pub(crate) fn hash_file(path: &SPath) -> Result<String> {
	let contents = std::fs::read(path.as_std_path())?;

	Ok(hash_bytes(&contents))
}

pub(crate) fn hash_bytes(contents: &[u8]) -> String {
	crate::support::hash_bytes(contents)
}

pub(crate) struct FormattedArtifact {
	pub(crate) relative_path: String,
	pub(crate) media_type: Option<String>,
	pub(crate) bytes: Vec<u8>,
}

pub(crate) fn apply_fetch_format(
	relative_path: &str,
	media_type: Option<&str>,
	bytes: &[u8],
	format: FetchFormat,
) -> Result<FormattedArtifact> {
	if format == FetchFormat::Raw || !is_html_item(media_type, relative_path) {
		return Ok(FormattedArtifact {
			relative_path: relative_path.to_owned(),
			media_type: media_type.map(str::to_owned),
			bytes: bytes.to_vec(),
		});
	}

	let html =
		std::str::from_utf8(bytes).map_err(|error| Error::custom(format!("HTML input is not valid UTF-8: {error}")))?;

	match format {
		FetchFormat::Raw => Ok(FormattedArtifact {
			relative_path: relative_path.to_owned(),
			media_type: media_type.map(str::to_owned),
			bytes: bytes.to_vec(),
		}),
		FetchFormat::Slim => Ok(FormattedArtifact {
			relative_path: relative_path.to_owned(),
			media_type: media_type.map(str::to_owned),
			bytes: slim_html(html)?.into_bytes(),
		}),
		FetchFormat::Md => {
			let mut path = PathBuf::from(relative_path);
			path.set_extension("md");
			let relative_path = path
				.to_str()
				.ok_or_else(|| Error::InvalidConfiguration("fetch path is not valid UTF-8".to_owned()))?
				.replace('\\', "/");

			Ok(FormattedArtifact {
				relative_path,
				media_type: Some("text/markdown".to_owned()),
				bytes: html_to_markdown(html)?.into_bytes(),
			})
		}
	}
}

pub(crate) fn media_type_for(path: &Path) -> Option<String> {
	let extension = path.extension()?.to_str()?.to_ascii_lowercase();
	let media_type = match extension.as_str() {
		"html" | "htm" => "text/html",
		"md" | "markdown" => "text/markdown",
		"txt" => "text/plain",
		"rs" => "text/rust",
		"json" => "application/json",
		"toml" => "application/toml",
		"yaml" | "yml" => "application/yaml",
		"xml" => "application/xml",
		"csv" => "text/csv",
		"css" => "text/css",
		"js" => "text/javascript",
		"ts" => "text/typescript",
		"png" => "image/png",
		"jpg" | "jpeg" => "image/jpeg",
		"gif" => "image/gif",
		"svg" => "image/svg+xml",
		"pdf" => "application/pdf",
		"zip" => "application/zip",
		_ => return None,
	};

	Some(media_type.to_owned())
}

pub(crate) fn normalize_relative_path(path: &Path) -> Result<String> {
	if path.is_absolute() || path.components().any(|component| component == Component::ParentDir) {
		return Err(Error::InvalidConfiguration(
			"local source produced an invalid relative path".to_owned(),
		));
	}

	let value = path
		.to_str()
		.ok_or_else(|| Error::InvalidConfiguration("local source path is not valid UTF-8".to_owned()))?;
	let value = value.replace('\\', "/");
	let value = value.strip_prefix("./").unwrap_or(&value);

	if value.is_empty() || value == "." {
		return Err(Error::InvalidConfiguration(
			"local source produced an empty relative path".to_owned(),
		));
	}

	Ok(value.to_owned())
}

pub(crate) fn normalize_manifest_relative_path(value: &str) -> Result<String> {
	let value = value.replace('\\', "/");
	normalize_relative_path(Path::new(&value))
}

pub(crate) fn source_identity(path: &SPath) -> Result<String> {
	let path: &Path = path.as_ref();
	let value = path
		.to_str()
		.ok_or_else(|| Error::InvalidConfiguration("local source path is not valid UTF-8".to_owned()))?;

	Ok(value.replace('\\', "/"))
}

pub(crate) fn write_fetch_manifest(path: &SPath, manifest: &FetchManifest) -> Result<()> {
	ensure_parent(path)?;
	let manifest_json = serde_json::to_string_pretty(manifest)
		.map_err(|error| Error::MalformedState(format!("failed to serialize Fetch manifest: {error}")))?;
	let temporary_path = SPath::from(format!("{path}.tmp"));
	let manifest_content = format!("{manifest_json}\n");
	write(temporary_path.as_std_path(), manifest_content.as_bytes())
		.map_err(|error| Error::MalformedState(format!("failed to write Fetch manifest {temporary_path}: {error}")))?;
	rename(temporary_path.as_std_path(), path.as_std_path())
		.map_err(|error| Error::MalformedState(format!("failed to replace Fetch manifest {path}: {error}")))?;
	Ok(())
}

pub(crate) fn path_matches_glob(path: &str, pattern: &str) -> bool {
	let path = path.trim_start_matches('/');
	let pattern = pattern.trim_start_matches('/');

	if pattern == "**" || pattern == "**/*" {
		return true;
	}

	let path_segments: Vec<&str> = path.split('/').filter(|segment| !segment.is_empty()).collect();
	let pattern_segments: Vec<&str> = pattern.split('/').filter(|segment| !segment.is_empty()).collect();

	match_segments(&path_segments, &pattern_segments)
}

fn match_segments(path: &[&str], pattern: &[&str]) -> bool {
	match (path.first(), pattern.first()) {
		(None, None) => true,
		(_, Some(&"**")) => {
			if pattern.len() == 1 {
				true
			} else {
				(0..=path.len()).any(|idx| match_segments(&path[idx..], &pattern[1..]))
			}
		}
		(Some(p), Some(pat)) if segment_matches(p, pat) => match_segments(&path[1..], &pattern[1..]),
		_ => false,
	}
}

fn segment_matches(segment: &str, pattern: &str) -> bool {
	if pattern == "*" {
		return true;
	}
	if !pattern.contains('*') {
		return segment == pattern;
	}
	let parts: Vec<&str> = pattern.split('*').collect();
	if parts.is_empty() {
		return true;
	}
	if !segment.starts_with(parts[0]) {
		return false;
	}
	let mut current = &segment[parts[0].len()..];
	for &part in &parts[1..parts.len() - 1] {
		if let Some(pos) = current.find(part) {
			current = &current[pos + part.len()..];
		} else {
			return false;
		}
	}
	let last = parts[parts.len() - 1];
	current.ends_with(last)
}

pub(crate) fn is_path_selected(relative_path: &str, common: &FetchCommonOptions) -> bool {
	let mut include_patterns = Vec::new();
	let mut exclude_patterns = common.exclude.clone();

	for pat in &common.include {
		if let Some(neg) = pat.strip_prefix('!') {
			exclude_patterns.push(neg.to_owned());
		} else {
			include_patterns.push(pat.as_str());
		}
	}

	let included = if include_patterns.is_empty() {
		true
	} else {
		include_patterns.iter().any(|pat| path_matches_glob(relative_path, pat))
	};

	if !included {
		return false;
	}

	let excluded = exclude_patterns.iter().any(|pat| {
		let pat = pat.strip_prefix('!').unwrap_or(pat);
		path_matches_glob(relative_path, pat)
	});

	!excluded
}

// endregion: --- Support

// region:    --- Tests

#[cfg(test)]
mod tests {
	type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

	use super::*;
	use crate::process::FetchFormat;

	#[test]
	fn test_fetchr_support_path_matches_glob_rules() -> Result<()> {
		// -- Exec & Check
		assert!(path_matches_glob("index.html", "**/*"));
		assert!(path_matches_glob("nested/page.html", "**/*"));
		assert!(path_matches_glob("nested/page.html", "**/*.html"));
		assert!(path_matches_glob("page.html", "*.html"));
		assert!(path_matches_glob("guide/intro.html", "guide/*"));
		assert!(!path_matches_glob("guide/nested/intro.html", "guide/*"));
		assert!(path_matches_glob("guide/nested/intro.html", "guide/**"));
		assert!(!path_matches_glob("nested/page.txt", "**/*.html"));

		Ok(())
	}

	#[test]
	fn test_fetchr_support_is_path_selected_rules() -> Result<()> {
		// -- Setup & Fixtures
		let default_options = FetchCommonOptions::default();
		let filter_options = FetchCommonOptions {
			include: vec!["docs/**".to_owned()],
			exclude: vec!["docs/secret.html".to_owned()],
			format: FetchFormat::default(),
		};

		// -- Exec & Check
		assert!(is_path_selected("index.html", &default_options));
		assert!(is_path_selected("docs/guide.html", &filter_options));
		assert!(!is_path_selected("docs/secret.html", &filter_options));
		assert!(!is_path_selected("blog/post.html", &filter_options));

		Ok(())
	}

	#[test]
	fn test_fetchr_support_apply_fetch_format_variants() -> Result<()> {
		// -- Setup & Fixtures
		let html = b"<html><body><h1>Hello</h1><script>remove me</script></body></html>";

		// -- Exec
		let raw = apply_fetch_format("page.html", Some("text/html"), html, FetchFormat::Raw)?;
		let slimmed = apply_fetch_format("page.html", Some("text/html"), html, FetchFormat::Slim)?;
		let markdown = apply_fetch_format("page.html", Some("text/html"), html, FetchFormat::Md)?;
		let plain = apply_fetch_format("page.txt", Some("text/plain"), b"unchanged", FetchFormat::Md)?;

		// -- Check
		assert_eq!(raw.relative_path, "page.html");
		assert_eq!(raw.media_type.as_deref(), Some("text/html"));
		assert_eq!(raw.bytes, html);

		assert_eq!(slimmed.relative_path, "page.html");
		assert_eq!(slimmed.media_type.as_deref(), Some("text/html"));
		assert!(String::from_utf8(slimmed.bytes)?.contains("Hello"));

		assert_eq!(markdown.relative_path, "page.md");
		assert_eq!(markdown.media_type.as_deref(), Some("text/markdown"));
		assert!(String::from_utf8(markdown.bytes)?.contains("Hello"));

		assert_eq!(plain.relative_path, "page.txt");
		assert_eq!(plain.media_type.as_deref(), Some("text/plain"));
		assert_eq!(plain.bytes, b"unchanged");

		Ok(())
	}
}

// endregion: --- Tests
