use crate::mapr::ContentMapDocument;
use crate::{Error, Result};
use std::collections::BTreeSet;
use std::fs::{create_dir_all, rename, write};
use std::path::Path;

// region:    --- Support

pub fn hash_file_bytes(bytes: &[u8]) -> String {
	crate::support::hash_bytes(bytes)
}

pub fn hash_file(path: impl AsRef<Path>) -> Result<String> {
	let path_ref = path.as_ref();
	let bytes = std::fs::read(path_ref).map_err(|error| {
		Error::MalformedState(format!(
			"failed to read file for hashing {}: {error}",
			path_ref.display()
		))
	})?;
	Ok(hash_file_bytes(&bytes))
}

pub fn hash_folder<I, P, H>(children: I) -> String
where
	I: IntoIterator<Item = (P, H)>,
	P: AsRef<str>,
	H: AsRef<str>,
{
	let mut sorted: Vec<(P, H)> = children.into_iter().collect();
	sorted.sort_by(|a, b| a.0.as_ref().cmp(b.0.as_ref()));
	let mut hasher = blake3::Hasher::new();
	for (rel_path, child_hash) in &sorted {
		hasher.update(rel_path.as_ref().as_bytes());
		hasher.update(child_hash.as_ref().as_bytes());
	}
	bs58::encode(hasher.finalize().as_bytes()).into_string()
}

pub fn is_text_mappable(media_type: Option<&str>, path: impl AsRef<Path>) -> bool {
	let path_ref = path.as_ref();

	if let Some(media_type) = media_type {
		let mime = media_type.split(';').next().unwrap_or(media_type).trim().to_ascii_lowercase();
		if is_text_media_type(&mime) {
			return true;
		}
		if is_binary_media_type(&mime) {
			return false;
		}
	}

	if let Some(ext) = path_ref
		.extension()
		.and_then(|ext| ext.to_str())
		.map(|ext| ext.to_ascii_lowercase())
	{
		if is_known_text_extension(&ext) {
			return true;
		}
		if is_known_binary_extension(&ext) {
			return false;
		}
	}

	if let Some(file_name) = path_ref
		.file_name()
		.and_then(|name| name.to_str())
		.map(|name| name.to_ascii_lowercase())
		&& is_known_text_filename(&file_name)
	{
		return true;
	}

	false
}

pub fn derive_folders(paths: impl IntoIterator<Item = impl AsRef<str>>) -> Vec<String> {
	let mut folders = BTreeSet::new();
	folders.insert(String::new());

	for item in paths {
		let path_str = item.as_ref().replace('\\', "/");
		let path_str = path_str.strip_prefix("./").unwrap_or(&path_str);
		let path_str = path_str.trim_start_matches('/');

		let mut current = Path::new(path_str);
		while let Some(parent) = current.parent() {
			let parent_str = parent.to_str().unwrap_or("").replace('\\', "/");
			let parent_str = parent_str.strip_prefix("./").unwrap_or(&parent_str);
			let parent_str = parent_str.trim_start_matches('/');

			if parent_str.is_empty() {
				break;
			}
			folders.insert(parent_str.to_string());
			current = Path::new(parent);
		}
	}

	folders.into_iter().collect()
}

fn is_text_media_type(mime: &str) -> bool {
	if mime.starts_with("text/") {
		return true;
	}

	matches!(
		mime,
		"application/json"
			| "application/toml"
			| "application/yaml"
			| "application/x-yaml"
			| "application/xml"
			| "application/javascript"
			| "application/ecmascript"
			| "application/typescript"
			| "application/x-sh"
			| "application/x-shellscript"
			| "application/x-bash"
			| "application/sql"
			| "application/graphql"
			| "image/svg+xml"
	) || mime.ends_with("+json")
		|| mime.ends_with("+xml")
		|| mime.ends_with("+yaml")
}

fn is_binary_media_type(mime: &str) -> bool {
	if mime.starts_with("image/") && mime != "image/svg+xml" {
		return true;
	}
	if mime.starts_with("video/") || mime.starts_with("audio/") || mime.starts_with("font/") {
		return true;
	}

	matches!(
		mime,
		"application/pdf"
			| "application/zip"
			| "application/gzip"
			| "application/tar"
			| "application/x-tar"
			| "application/x-bzip2"
			| "application/x-7z-compressed"
			| "application/vnd.rar"
			| "application/wasm"
	)
}

fn is_known_text_extension(ext: &str) -> bool {
	matches!(
		ext,
		"txt"
			| "text" | "md"
			| "markdown"
			| "mdown" | "mkdn"
			| "rst" | "adoc"
			| "asciidoc"
			| "html" | "htm"
			| "xhtml" | "css"
			| "scss" | "sass"
			| "less" | "js"
			| "mjs" | "cjs"
			| "ts" | "mts"
			| "cts" | "jsx"
			| "tsx" | "vue"
			| "svelte"
			| "astro" | "json"
			| "jsonc" | "json5"
			| "toml" | "yaml"
			| "yml" | "xml"
			| "csv" | "tsv"
			| "ini" | "conf"
			| "cfg" | "properties"
			| "env" | "rs"
			| "c" | "h"
			| "cpp" | "hpp"
			| "cc" | "hh"
			| "cxx" | "hxx"
			| "py" | "rb"
			| "go" | "java"
			| "kt" | "kts"
			| "swift" | "cs"
			| "fs" | "scala"
			| "clj" | "php"
			| "lua" | "pl"
			| "pm" | "r"
			| "dart" | "zig"
			| "nim" | "v"
			| "sh" | "bash"
			| "zsh" | "fish"
			| "ps1" | "bat"
			| "cmd" | "sql"
			| "graphql"
			| "gql" | "proto"
			| "prisma"
			| "diff" | "patch"
			| "svg"
	)
}

fn is_known_binary_extension(ext: &str) -> bool {
	matches!(
		ext,
		"png"
			| "jpg" | "jpeg"
			| "gif" | "webp"
			| "ico" | "bmp"
			| "tiff" | "avif"
			| "pdf" | "doc"
			| "docx" | "xls"
			| "xlsx" | "ppt"
			| "pptx" | "zip"
			| "tar" | "gz"
			| "tgz" | "bz2"
			| "xz" | "7z"
			| "rar" | "wasm"
			| "exe" | "dll"
			| "so" | "dylib"
			| "bin" | "iso"
			| "mp3" | "mp4"
			| "wav" | "ogg"
			| "webm" | "flac"
			| "m4a" | "mov"
			| "avi" | "mkv"
			| "ttf" | "otf"
			| "woff" | "woff2"
			| "eot" | "class"
			| "pyc" | "o"
			| "obj" | "rlib"
	)
}

fn is_known_text_filename(file_name: &str) -> bool {
	if matches!(
		file_name,
		"dockerfile"
			| "makefile"
			| "gnumakefile"
			| "gemfile"
			| "rakefile"
			| "procfile"
			| "vagrantfile"
			| "license"
			| "licence"
			| "readme"
			| "contributing"
			| "changelog"
			| ".gitignore"
			| ".gitattributes"
			| ".editorconfig"
			| ".env" | ".env.example"
			| ".dockerignore"
	) {
		return true;
	}

	file_name.starts_with("dockerfile.")
		|| file_name.starts_with("license.")
		|| file_name.starts_with("licence.")
		|| file_name.starts_with("readme.")
		|| file_name.starts_with("contributing.")
		|| file_name.starts_with("changelog.")
}

pub fn publish_content_map(path: impl AsRef<Path>, document: &ContentMapDocument) -> Result<()> {
	let target_path = path.as_ref();
	let parent = target_path
		.parent()
		.ok_or_else(|| Error::MalformedState(format!("target path has no parent: {}", target_path.display())))?;

	create_dir_all(parent).map_err(|error| {
		Error::MalformedState(format!(
			"failed to create parent directory {}: {error}",
			parent.display()
		))
	})?;

	let json = serde_json::to_string_pretty(document)
		.map_err(|error| Error::MalformedState(format!("failed to serialize content map: {error}")))?;
	let content = format!("{json}\n");

	let file_name = target_path.file_name().and_then(|name| name.to_str()).ok_or_else(|| {
		Error::MalformedState(format!("target path has no valid file name: {}", target_path.display()))
	})?;
	let temporary_path = parent.join(format!("{file_name}.tmp"));

	write(&temporary_path, content.as_bytes()).map_err(|error| {
		Error::MalformedState(format!(
			"failed to write temporary content map {}: {error}",
			temporary_path.display()
		))
	})?;

	rename(&temporary_path, target_path).map_err(|error| {
		Error::MalformedState(format!(
			"failed to replace content map {}: {error}",
			target_path.display()
		))
	})?;

	Ok(())
}

// endregion: --- Support

// region:    --- Tests

#[cfg(test)]
mod tests {
	type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

	use super::*;
	use crate::mapr::FileMapEntry;
	use std::collections::BTreeMap;
	use std::fs::read_to_string;

	#[test]
	fn test_mapr_support_publish_content_map() -> Result<()> {
		// -- Setup & Fixtures
		let test_dir = std::env::temp_dir().join(format!("zmapr_test_publish_{}", std::process::id()));
		let target_file = test_dir.join("sub").join("content-map.json");

		let mut file_map = BTreeMap::new();
		file_map.insert(
			"src/main.rs".to_string(),
			FileMapEntry {
				summary: "Application entry point".to_string(),
				when_to_use: "Consult for startup sequence".to_string(),
				public_types: vec!["App".to_string()],
				public_functions: vec!["main".to_string()],
				topics: vec!["entry".to_string(), "cli".to_string()],
			},
		);

		let document = ContentMapDocument::new("mock-model", 1, "2026-09-20T20:15:16Z", file_map, BTreeMap::new());

		// -- Exec
		publish_content_map(&target_file, &document)?;

		// -- Check
		assert!(target_file.exists());
		let tmp_sibling = test_dir.join("sub").join("content-map.json.tmp");
		assert!(!tmp_sibling.exists());

		let saved_content = read_to_string(&target_file)?;
		assert!(saved_content.ends_with('\n'));

		let parsed: ContentMapDocument = serde_json::from_str(&saved_content)?;
		assert_eq!(parsed, document);
		assert_eq!(parsed.version, 1);
		assert_eq!(parsed.model, "mock-model");
		assert_eq!(parsed.prompt_version, 1);
		assert!(parsed.folder_map.is_empty());
		assert_eq!(parsed.file_map.len(), 1);

		// Clean up
		std::fs::remove_dir_all(&test_dir)?;

		Ok(())
	}

	#[test]
	fn test_mapr_support_hash_stability() -> Result<()> {
		// -- Setup & Fixtures
		let test_dir = std::env::temp_dir().join(format!("zmapr_test_hash_{}", std::process::id()));
		create_dir_all(&test_dir)?;
		let sample_file = test_dir.join("sample.txt");
		write(&sample_file, b"sample content for testing blake3")?;

		// -- Exec
		let bytes_hash = hash_file_bytes(b"sample content for testing blake3");
		let file_hash = hash_file(&sample_file)?;
		let different_hash = hash_file_bytes(b"different content");

		let folder_hash1 = hash_folder([("b.txt", "hash2"), ("a.txt", "hash1")]);
		let folder_hash2 = hash_folder([("a.txt", "hash1"), ("b.txt", "hash2")]);
		let folder_hash_diff = hash_folder([("a.txt", "hash1"), ("b.txt", "hash3")]);

		// -- Check
		assert_eq!(bytes_hash, file_hash);
		assert_ne!(bytes_hash, different_hash);
		assert_eq!(folder_hash1, folder_hash2);
		assert_ne!(folder_hash1, folder_hash_diff);

		// Clean up
		std::fs::remove_dir_all(&test_dir)?;

		Ok(())
	}

	#[test]
	fn test_mapr_support_derive_folders() -> Result<()> {
		// -- Setup & Fixtures
		let paths = vec![
			"src/mapr/support.rs",
			"src/main.rs",
			"README.md",
			"tests/support/helpers.rs",
			"src/mapr/mapr_impl.rs",
		];

		// -- Exec
		let folders = derive_folders(paths);
		let empty_folders = derive_folders(Vec::<String>::new());

		// -- Check
		assert_eq!(
			folders,
			vec![
				"".to_string(),
				"src".to_string(),
				"src/mapr".to_string(),
				"tests".to_string(),
				"tests/support".to_string(),
			]
		);
		assert_eq!(empty_folders, vec!["".to_string()]);

		Ok(())
	}

	#[test]
	fn test_mapr_support_is_text_mappable() -> Result<()> {
		// -- Exec & Check
		assert!(is_text_mappable(Some("text/plain"), "doc.bin"));
		assert!(is_text_mappable(Some("application/json"), "payload"));
		assert!(is_text_mappable(Some("image/svg+xml"), "vector.unknown"));
		assert!(is_text_mappable(Some("application/ld+json"), "data"));

		assert!(!is_text_mappable(Some("image/png"), "code.rs"));
		assert!(!is_text_mappable(Some("application/pdf"), "guide.md"));

		assert!(is_text_mappable(Some("application/octet-stream"), "main.rs"));
		assert!(!is_text_mappable(Some("application/octet-stream"), "blob.bin"));

		assert!(is_text_mappable(None, "Dockerfile"));
		assert!(is_text_mappable(None, "LICENSE"));
		assert!(is_text_mappable(None, "README"));
		assert!(is_text_mappable(None, ".gitignore"));
		assert!(is_text_mappable(None, "src/lib.rs"));
		assert!(is_text_mappable(None, "docs/index.html"));

		assert!(!is_text_mappable(None, "archive.zip"));
		assert!(!is_text_mappable(None, "binary_file"));

		Ok(())
	}
}

// endregion: --- Tests
