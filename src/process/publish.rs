use super::pipeline::{ArtifactItem, ArtifactSet};
use crate::{Error, Result};
use simple_fs::SPath;
use std::collections::HashSet;
use std::path::{Component, Path};

// region:    --- Publication

pub(crate) fn publish_final_artifacts(artifacts: &ArtifactSet, destination: &SPath) -> Result<usize> {
	let mut normalized_paths = HashSet::with_capacity(artifacts.items.len());
	let mut targets = Vec::with_capacity(artifacts.items.len());

	for item in &artifacts.items {
		let relative_path = normalize_publish_path(&item.relative_path)?;

		if is_reserved_publish_path(&relative_path) {
			return Err(Error::MalformedState(format!(
				"final artifact path is reserved: {}",
				item.relative_path
			)));
		}

		if !normalized_paths.insert(relative_path.clone()) {
			return Err(Error::MalformedState(format!(
				"multiple final artifacts resolve to destination path {relative_path}"
			)));
		}

		if !item.local_path.is_file() {
			return Err(Error::MalformedState(format!(
				"final artifact is missing for {relative_path}: {}",
				item.local_path
			)));
		}

		targets.push((destination.join(relative_path.as_str()), &item.local_path));
	}

	let mut published_count = 0;

	for (target, source) in targets {
		let target_path: &Path = target.as_ref();
		let source_path: &Path = source.as_ref();

		if paths_equivalent(source_path, target_path) {
			continue;
		}

		copy_atomic(source_path, target_path)?;
		published_count += 1;
	}

	Ok(published_count)
}

// endregion: --- Publication

// region:    --- Support

fn normalize_publish_path(value: &str) -> Result<String> {
	let replaced = value.replace('\\', "/");
	let normalized = replaced.strip_prefix("./").unwrap_or(&replaced);
	let path = Path::new(normalized);

	if path.is_absolute() {
		return Err(invalid_publish_path(value));
	}

	let mut components = Vec::new();

	for component in path.components() {
		match component {
			Component::Normal(component) => {
				let Some(component) = component.to_str() else {
					return Err(invalid_publish_path(value));
				};
				components.push(component);
			}
			Component::CurDir => {}
			Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
				return Err(invalid_publish_path(value));
			}
		}
	}

	if components.is_empty() {
		return Err(invalid_publish_path(value));
	}

	Ok(components.join("/"))
}

fn is_reserved_publish_path(path: &str) -> bool {
	let mut components = path.split('/');
	let first = components.next().unwrap_or_default();

	first.eq_ignore_ascii_case(".tmp-zmapr")
		|| (first.eq_ignore_ascii_case("content-map.json") && components.next().is_none())
}

fn paths_equivalent(left: &Path, right: &Path) -> bool {
	left.components().eq(right.components())
}

fn copy_atomic(source: &Path, target: &Path) -> Result<()> {
	let parent = target.parent().ok_or_else(|| {
		Error::MalformedState(format!(
			"failed to publish final artifact {}: target has no parent directory",
			target.display()
		))
	})?;
	std::fs::create_dir_all(parent).map_err(|error| {
		Error::MalformedState(format!(
			"failed to create final artifact directory for {}: {error}",
			target.display()
		))
	})?;

	let file_name = target.file_name().and_then(|name| name.to_str()).ok_or_else(|| {
		Error::MalformedState(format!(
			"failed to publish final artifact {}: target has no valid file name",
			target.display()
		))
	})?;
	let temporary_path = parent.join(format!(".{file_name}.zmapr-publish.tmp"));

	if let Err(error) = std::fs::copy(source, &temporary_path).and_then(|_| std::fs::rename(&temporary_path, target)) {
		let _ = std::fs::remove_file(&temporary_path);
		return Err(Error::MalformedState(format!(
			"failed to publish final artifact {}: {error}",
			target.display()
		)));
	}

	Ok(())
}

fn invalid_publish_path(value: &str) -> Error {
	Error::MalformedState(format!("invalid final artifact path: {value}"))
}

// endregion: --- Support

// region:    --- Tests

#[cfg(test)]
mod tests {
	type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

	use super::*;
	use std::fs;
	use std::path::{Path, PathBuf};

	#[test]
	fn test_process_publish_final_artifacts_nested_paths() -> Result<()> {
		// -- Setup & Fixtures
		let root = fixture_root("test_process_publish_final_artifacts_nested_paths")?;
		let destination = root.join("destination");
		let artifacts = make_artifact_set(&root, &[("nested/guide.md", "guide.txt", b"# Guide\n")])?;

		// -- Exec
		let published_count = publish_final_artifacts(&artifacts, &SPath::from(path_text(&destination)))?;

		// -- Check
		assert_eq!(published_count, 1);
		assert_eq!(fs::read(destination.join("nested/guide.md"))?, b"# Guide\n");

		// std::fs::remove_dir_all(&root)?;
		Ok(())
	}

	#[test]
	fn test_process_publish_final_artifacts_replaces_existing_and_keeps_unrelated() -> Result<()> {
		// -- Setup & Fixtures
		let root = fixture_root("test_process_publish_final_artifacts_replaces_existing_and_keeps_unrelated")?;
		let destination = root.join("destination");
		let target = destination.join("nested/result.txt");
		fs::create_dir_all(target.parent().ok_or("target should have a parent")?)?;
		fs::write(&target, b"old content")?;
		fs::write(destination.join("unrelated.txt"), b"keep this")?;
		let artifacts = make_artifact_set(&root, &[("nested/result.txt", "result.txt", b"new content")])?;

		// -- Exec
		let published_count = publish_final_artifacts(&artifacts, &SPath::from(path_text(&destination)))?;

		// -- Check
		assert_eq!(published_count, 1);
		assert_eq!(fs::read(&target)?, b"new content");
		assert_eq!(fs::read(destination.join("unrelated.txt"))?, b"keep this");

		// std::fs::remove_dir_all(&root)?;
		Ok(())
	}

	#[test]
	fn test_process_publish_final_artifacts_rejects_reserved_paths() -> Result<()> {
		// -- Setup & Fixtures
		let root = fixture_root("test_process_publish_final_artifacts_rejects_reserved_paths")?;
		let destination = root.join("destination");

		// -- Exec & Check
		for relative_path in [
			".tmp-zmapr/private.txt",
			".TMP-ZMAPR/private.txt",
			"content-map.json",
			"CONTENT-MAP.JSON",
		] {
			let artifacts = make_artifact_set(&root, &[(relative_path, "reserved.txt", b"content")])?;
			let error = publish_final_artifacts(&artifacts, &SPath::from(path_text(&destination)))
				.err()
				.ok_or("reserved final artifact path should be rejected")?;
			assert!(matches!(error, Error::MalformedState(_)));
		}

		assert!(!destination.exists());

		// std::fs::remove_dir_all(&root)?;
		Ok(())
	}

	#[test]
	fn test_process_publish_final_artifacts_rejects_traversal_and_absolute() -> Result<()> {
		// -- Setup & Fixtures
		let root = fixture_root("test_process_publish_final_artifacts_rejects_traversal_and_absolute")?;
		fs::remove_dir_all(&root)?;
		fs::create_dir_all(&root)?;
		let destination = root.join("destination");
		let absolute_path = fs::canonicalize(&root)?.join("absolute.txt");
		let absolute_path_text = path_text(&absolute_path);

		// -- Exec & Check
		for relative_path in ["../escaped.txt", absolute_path_text.as_str()] {
			let artifacts = make_artifact_set(&root, &[(relative_path, "source.txt", b"content")])?;
			let error = publish_final_artifacts(&artifacts, &SPath::from(path_text(&destination)))
				.err()
				.ok_or("unsafe final artifact path should be rejected")?;
			assert!(matches!(error, Error::MalformedState(_)));
		}

		assert!(!root.join("escaped.txt").exists());
		assert!(!absolute_path.exists());
		assert!(!destination.exists());

		// std::fs::remove_dir_all(&root)?;
		Ok(())
	}

	#[test]
	fn test_process_publish_final_artifacts_rejects_duplicates() -> Result<()> {
		// -- Setup & Fixtures
		let root = fixture_root("test_process_publish_final_artifacts_rejects_duplicates")?;
		let destination = root.join("destination");
		let artifacts = make_artifact_set(
			&root,
			&[
				("nested\\file.txt", "first.txt", b"first"),
				("nested/file.txt", "second.txt", b"second"),
			],
		)?;

		// -- Exec
		let error = publish_final_artifacts(&artifacts, &SPath::from(path_text(&destination)))
			.err()
			.ok_or("duplicate final artifact paths should be rejected")?;

		// -- Check
		assert!(matches!(error, Error::MalformedState(_)));
		assert!(!destination.exists());

		// std::fs::remove_dir_all(&root)?;
		Ok(())
	}

	#[test]
	fn test_process_publish_final_artifacts_missing_artifact_writes_nothing() -> Result<()> {
		// -- Setup & Fixtures
		let root = fixture_root("test_process_publish_final_artifacts_missing_artifact_writes_nothing")?;
		let destination = root.join("destination");
		fs::create_dir_all(&destination)?;
		fs::write(destination.join("unrelated.txt"), b"keep this")?;
		let mut artifacts = make_artifact_set(&root, &[("present.txt", "present.txt", b"present")])?;
		let missing_path = root.join("source/missing.txt");
		artifacts.items.push(ArtifactItem {
			source: path_text(&missing_path),
			relative_path: "missing.txt".to_owned(),
			local_path: SPath::from(path_text(&missing_path)),
			media_type: None,
			source_hash: None,
		});

		// -- Exec
		let error = publish_final_artifacts(&artifacts, &SPath::from(path_text(&destination)))
			.err()
			.ok_or("missing final artifact should be rejected")?;

		// -- Check
		assert!(matches!(error, Error::MalformedState(_)));
		assert!(!destination.join("present.txt").exists());
		assert!(!destination.join("missing.txt").exists());
		assert_eq!(fs::read(destination.join("unrelated.txt"))?, b"keep this");

		// std::fs::remove_dir_all(&root)?;
		Ok(())
	}

	// -- Test Support

	fn fixture_root(test_name: &str) -> Result<PathBuf> {
		let root = PathBuf::from("tests-data/.tmp").join(test_name);
		fs::create_dir_all(&root)?;
		Ok(root)
	}

	fn make_artifact_set(root: &Path, entries: &[(&str, &str, &[u8])]) -> Result<ArtifactSet> {
		let source_root = root.join("source");
		fs::create_dir_all(&source_root)?;
		let mut items = Vec::with_capacity(entries.len());

		for (relative_path, source_name, contents) in entries {
			let local_path = source_root.join(source_name);
			if let Some(parent) = local_path.parent() {
				fs::create_dir_all(parent)?;
			}
			fs::write(&local_path, contents)?;

			items.push(ArtifactItem {
				source: path_text(&local_path),
				relative_path: (*relative_path).to_owned(),
				local_path: SPath::from(path_text(&local_path)),
				media_type: None,
				source_hash: None,
			});
		}

		Ok(ArtifactSet {
			root: SPath::from(path_text(&source_root)),
			items,
		})
	}

	fn path_text(path: &Path) -> String {
		path.to_string_lossy().replace('\\', "/")
	}
}

// endregion: --- Tests
