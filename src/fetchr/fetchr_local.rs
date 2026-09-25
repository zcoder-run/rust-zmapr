use super::fetchr_types::{
	FETCH_MANIFEST_VERSION, FetchManifest, FetchManifestItem, FetchManifestOptions, LocalFetchDiscovery,
	LocalFetchItem, LocalSourceKind,
};
use super::support::{
	apply_fetch_format, hash_bytes, hash_file, media_type_for, normalize_manifest_relative_path,
	normalize_relative_path, path_to_string, paths_equivalent, source_identity, write_fetch_artifact,
	write_fetch_manifest,
};
use crate::fetchr::{FetchCommonOptions, LocalFetchRequest};
use crate::process::pipeline::{ArtifactItem, ArtifactSet, StageOutput, WorkflowContext};
use crate::process::{ItemId, LocalContentSource, ProcessStage};
use crate::{Error, Result};
use simple_fs::{SPath, ensure_dir, list_files};
use std::path::Path;

// region:    --- Public Functions

pub(crate) fn discover_local(request: &LocalFetchRequest) -> Result<LocalFetchDiscovery> {
	let source_path = &request.source.path;
	let source_identity = source_identity(source_path)?;
	let source_kind = validate_source_kind(source_path, &source_identity)?;
	let patterns = build_glob_patterns(&request.common);

	let (candidates, excluded) = match source_kind {
		LocalSourceKind::File => discover_single_file(source_path, &patterns)?,
		LocalSourceKind::Directory => list_paths(source_path, &patterns)?,
	};

	let mut items = Vec::with_capacity(candidates.len());

	for candidate in candidates {
		let local_path = match source_kind {
			LocalSourceKind::File => source_path.clone(),
			LocalSourceKind::Directory => local_path_for_candidate(source_path, candidate),
		};

		if !local_path.is_file() {
			continue;
		}

		let relative_path = match source_kind {
			LocalSourceKind::File => relative_file_name(source_path)?,
			LocalSourceKind::Directory => relative_path_for(source_path, &local_path)?,
		};
		let content_hash = hash_file(&local_path)?;
		let media_type = media_type_for(local_path.as_ref());

		items.push(LocalFetchItem {
			source: source_identity.clone(),
			relative_path,
			local_path,
			media_type,
			content_hash,
		});
	}

	items.sort_by(|left, right| left.relative_path.cmp(&right.relative_path));
	items.dedup_by(|left, right| left.relative_path == right.relative_path);

	Ok(LocalFetchDiscovery {
		source: source_identity,
		source_path: source_path.clone(),
		items,
		excluded,
	})
}

pub(crate) async fn execute_local_fetch(request: &LocalFetchRequest, context: &WorkflowContext) -> Result<StageOutput> {
	if context.max_concurrency == 0 {
		return Err(Error::InvalidConfiguration(
			"max_concurrency must be greater than zero".to_owned(),
		));
	}

	if context.resume
		&& let Some(output) = try_resume_local_fetch(request, context)?
	{
		return Ok(output);
	}

	let LocalFetchDiscovery {
		source: source_identity,
		source_path,
		items,
		excluded,
	} = discover_local(request)?;
	let ids = context.progress.register_fetch_items(
		items
			.iter()
			.map(|item| (item.local_path.as_str().to_owned(), item.relative_path.clone()))
			.collect(),
	);
	context.progress.add_excluded(ProcessStage::Fetch, excluded);
	context.progress.set_stage_total(ProcessStage::Fetch);

	let artifact_root = context.fetch_cache.clone();
	ensure_dir(&artifact_root)?;

	let mut artifacts = Vec::with_capacity(items.len());
	let mut has_failures = false;
	let mut manifest_items = Vec::with_capacity(items.len());
	let mut prepared_items = Vec::with_capacity(items.len());

	for (item, id) in items.iter().zip(ids) {
		let contents = match std::fs::read(item.local_path.as_std_path()) {
			Ok(contents) => contents,
			Err(error) => {
				let message = error.to_string();
				has_failures = true;
				context.progress.item_failed(id, ProcessStage::Fetch, message);
				manifest_items.push(manifest_item(
					item,
					&item.relative_path,
					item.media_type.clone(),
					None,
					String::new(),
				)?);
				continue;
			}
		};

		match apply_fetch_format(
			&item.relative_path,
			item.media_type.as_deref(),
			&contents,
			request.common.format,
		) {
			Ok(formatted) => prepared_items.push((id, item.clone(), formatted)),
			Err(error) => {
				let message = error.to_string();
				has_failures = true;
				context.progress.item_failed(id, ProcessStage::Fetch, message);
				manifest_items.push(manifest_item(
					item,
					&item.relative_path,
					item.media_type.clone(),
					None,
					String::new(),
				)?);
			}
		}
	}

	let mut stored_path_counts = std::collections::BTreeMap::<String, usize>::new();
	for (_, _, formatted) in &prepared_items {
		*stored_path_counts.entry(formatted.relative_path.clone()).or_default() += 1;
	}

	for (id, item, formatted) in prepared_items {
		let artifact_hash = hash_bytes(&formatted.bytes);
		if stored_path_counts.get(&formatted.relative_path).copied().unwrap_or_default() > 1 {
			let message = format!(
				"multiple input artifacts resolve to fetch path {}",
				formatted.relative_path
			);
			has_failures = true;
			context.progress.item_failed(id, ProcessStage::Fetch, message.clone());
			manifest_items.push(manifest_item(
				&item,
				&formatted.relative_path,
				formatted.media_type,
				None,
				artifact_hash,
			)?);
			continue;
		}

		let relative_path = formatted.relative_path;
		let media_type = formatted.media_type;
		let artifact_path = artifact_root.join(relative_path.as_str());
		context.progress.item_running(id, ProcessStage::Fetch);

		match write_fetch_artifact(&artifact_path, &formatted.bytes) {
			Ok(()) => {
				context.progress.fetch_completed(id, &relative_path, artifact_path.clone());
				let artifact = ArtifactItem {
					source: item.source.clone(),
					relative_path: relative_path.clone(),
					local_path: artifact_path.clone(),
					media_type: media_type.clone(),
					source_hash: Some(artifact_hash.clone()),
				};
				manifest_items.push(manifest_item(
					&item,
					&relative_path,
					media_type,
					Some(&artifact_path),
					artifact_hash,
				)?);
				artifacts.push(artifact);
			}
			Err(error) => {
				has_failures = true;
				context.progress.item_failed(id, ProcessStage::Fetch, error.to_string());
				manifest_items.push(manifest_item(&item, &relative_path, media_type, None, artifact_hash)?);
			}
		}
	}

	artifacts.sort_by(|left, right| left.relative_path.cmp(&right.relative_path));
	manifest_items.sort_by(|left, right| {
		left.relative_path
			.cmp(&right.relative_path)
			.then_with(|| left.origin_relative_path.cmp(&right.origin_relative_path))
	});

	let manifest = FetchManifest {
		version: FETCH_MANIFEST_VERSION,
		complete: !has_failures,
		source: source_identity,
		source_path: path_to_string(&source_path)?,
		options: FetchManifestOptions::from(request),
		artifact_root: path_to_string(&artifact_root)?,
		items: manifest_items,
	};
	write_fetch_manifest(&context.manifest, &manifest)?;

	Ok(StageOutput {
		artifacts: ArtifactSet {
			root: artifact_root,
			items: artifacts,
		},
	})
}

pub(crate) fn load_prior_local_fetch(context: &WorkflowContext) -> Result<ArtifactSet> {
	let manifest = read_fetch_manifest(&context.manifest)?;
	validate_prior_manifest(&manifest, context)?;

	let artifact_root = SPath::from(manifest.artifact_root.clone());
	if !artifact_root.is_dir() {
		return Err(Error::InvalidCache(format!(
			"Fetch artifact root does not exist: {artifact_root}"
		)));
	}

	let item_count = manifest.items.len();
	let mut items = Vec::with_capacity(item_count);

	for manifest_item in manifest.items {
		if manifest_item.source.trim().is_empty()
			|| manifest_item.local_path.trim().is_empty()
			|| manifest_item.content_hash.trim().is_empty()
			|| manifest_item.origin_relative_path.trim().is_empty()
			|| manifest_item.artifact_hash.trim().is_empty()
		{
			return Err(Error::MalformedState(format!(
				"Fetch manifest item is missing required metadata: {}",
				manifest_item.relative_path
			)));
		}

		let relative_path = normalize_manifest_relative_path(&manifest_item.relative_path).map_err(|error| {
			Error::MalformedState(format!(
				"invalid Fetch manifest relative path {}: {error}",
				manifest_item.relative_path
			))
		})?;

		if relative_path != manifest_item.relative_path.as_str() {
			return Err(Error::MalformedState(format!(
				"Fetch manifest relative path is not normalized: {}",
				manifest_item.relative_path
			)));
		}

		let artifact_path_text = manifest_item.artifact_path.as_ref().ok_or_else(|| {
			Error::MalformedState(format!(
				"Fetch manifest item has no artifact path: {}",
				manifest_item.relative_path
			))
		})?;

		if artifact_path_text.trim().is_empty() {
			return Err(Error::MalformedState(format!(
				"Fetch manifest item has an empty artifact path: {}",
				manifest_item.relative_path
			)));
		}

		let artifact_path = SPath::from(artifact_path_text.clone());
		let expected_artifact_path = artifact_root.join(relative_path.as_str());
		let expected_path: &Path = expected_artifact_path.as_ref();
		let actual_path: &Path = artifact_path.as_ref();

		if !paths_equivalent(expected_path, actual_path) {
			return Err(Error::MalformedState(format!(
				"Fetch manifest artifact path is incompatible: {artifact_path}"
			)));
		}

		if !artifact_path.is_file() {
			return Err(Error::InvalidCache(format!(
				"Fetch artifact does not exist: {artifact_path}"
			)));
		}

		let artifact_hash = hash_file(&artifact_path)
			.map_err(|error| Error::InvalidCache(format!("failed to read Fetch artifact {artifact_path}: {error}")))?;

		if artifact_hash != manifest_item.artifact_hash {
			return Err(Error::InvalidCache(format!(
				"Fetch artifact hash does not match the manifest: {artifact_path}"
			)));
		}

		items.push(ArtifactItem {
			source: manifest_item.source,
			relative_path,
			local_path: artifact_path,
			media_type: manifest_item.media_type,
			source_hash: Some(manifest_item.artifact_hash),
		});
	}

	if items
		.windows(2)
		.any(|window| window[0].relative_path.as_str() >= window[1].relative_path.as_str())
	{
		return Err(Error::MalformedState(
			"Fetch manifest items are not in deterministic order".to_owned(),
		));
	}

	Ok(ArtifactSet {
		root: artifact_root,
		items,
	})
}

pub(crate) fn validate_source(source: &LocalContentSource) -> Result<()> {
	let identity = source_identity(&source.path)?;
	validate_source_kind(&source.path, &identity).map(|_| ())
}

// endregion: --- Public Functions

// region:    --- Support

fn try_resume_local_fetch(request: &LocalFetchRequest, context: &WorkflowContext) -> Result<Option<StageOutput>> {
	let Some(manifest) = read_fetch_manifest_for_resume(&context.manifest) else {
		return Ok(None);
	};

	let discovery = discover_local(request)?;

	if !fetch_manifest_matches(&manifest, &discovery, request, context)? {
		return Ok(None);
	}

	build_reused_stage_output(&manifest, &discovery, context)
}

fn fetch_manifest_matches(
	manifest: &FetchManifest,
	discovery: &LocalFetchDiscovery,
	request: &LocalFetchRequest,
	context: &WorkflowContext,
) -> Result<bool> {
	if manifest.version != FETCH_MANIFEST_VERSION
		|| !manifest.complete
		|| manifest.options != FetchManifestOptions::from(request)
	{
		return Ok(false);
	}

	let expected_source_path = path_to_string(&discovery.source_path)?;
	let expected_artifact_root_path = context.fetch_cache.clone();
	let expected_artifact_root = path_to_string(&expected_artifact_root_path)?;

	if manifest.source != discovery.source
		|| manifest.source_path != expected_source_path
		|| manifest.artifact_root != expected_artifact_root
		|| manifest.items.len() != discovery.items.len()
	{
		return Ok(false);
	}

	for current_item in &discovery.items {
		let Ok(contents) = std::fs::read(current_item.local_path.as_std_path()) else {
			return Ok(false);
		};
		let Ok(formatted) = apply_fetch_format(
			&current_item.relative_path,
			current_item.media_type.as_deref(),
			&contents,
			request.common.format,
		) else {
			return Ok(false);
		};
		let Some(manifest_item) = manifest
			.items
			.iter()
			.find(|manifest_item| manifest_item.origin_relative_path == current_item.relative_path)
		else {
			return Ok(false);
		};
		let expected_local_path = path_to_string(&current_item.local_path)?;
		let expected_artifact_path = expected_artifact_root_path.join(formatted.relative_path.as_str());
		let expected_artifact_path = path_to_string(&expected_artifact_path)?;

		if manifest_item.source != current_item.source
			|| manifest_item.relative_path != formatted.relative_path
			|| manifest_item.local_path != expected_local_path
			|| manifest_item.artifact_path.as_deref() != Some(expected_artifact_path.as_str())
			|| manifest_item.media_type != formatted.media_type
			|| manifest_item.content_hash != current_item.content_hash
		{
			return Ok(false);
		}

		let artifact_path = SPath::from(expected_artifact_path);
		let Ok(artifact_hash) = hash_file(&artifact_path) else {
			return Ok(false);
		};
		if artifact_hash != manifest_item.artifact_hash {
			return Ok(false);
		}
	}

	Ok(true)
}

fn build_reused_stage_output(
	manifest: &FetchManifest,
	discovery: &LocalFetchDiscovery,
	context: &WorkflowContext,
) -> Result<Option<StageOutput>> {
	let artifact_root = context.fetch_cache.clone();
	let mut reusable_items = Vec::with_capacity(discovery.items.len());

	for current_item in &discovery.items {
		let Some(manifest_item) = manifest
			.items
			.iter()
			.find(|manifest_item| manifest_item.origin_relative_path == current_item.relative_path)
		else {
			return Ok(None);
		};
		let Some(artifact_path) = manifest_item.artifact_path.as_ref() else {
			return Ok(None);
		};
		let artifact_path = SPath::from(artifact_path.clone());

		if !artifact_path.is_file() {
			return Ok(None);
		}

		let artifact_hash = match hash_file(&artifact_path) {
			Ok(value) => value,
			Err(_) => return Ok(None),
		};

		if artifact_hash != manifest_item.artifact_hash {
			return Ok(None);
		}

		reusable_items.push((current_item, manifest_item, artifact_path));
	}

	let ids = context.progress.register_fetch_items(
		discovery
			.items
			.iter()
			.map(|item| (item.local_path.as_str().to_owned(), item.relative_path.clone()))
			.collect(),
	);
	context.progress.add_excluded(ProcessStage::Fetch, discovery.excluded);
	context.progress.set_stage_total(ProcessStage::Fetch);

	let mut artifacts = Vec::with_capacity(discovery.items.len());

	for ((current_item, manifest_item, artifact_path), id) in reusable_items.into_iter().zip(ids) {
		let relative_path = manifest_item.relative_path.clone();
		let artifact = ArtifactItem {
			source: current_item.source.clone(),
			relative_path: relative_path.clone(),
			local_path: artifact_path.clone(),
			media_type: manifest_item.media_type.clone(),
			source_hash: Some(manifest_item.artifact_hash.clone()),
		};

		context.progress.fetch_reused(id, &relative_path, artifact.local_path.clone());
		artifacts.push(artifact);
	}

	artifacts.sort_by(|left, right| left.relative_path.cmp(&right.relative_path));

	Ok(Some(StageOutput {
		artifacts: ArtifactSet {
			root: artifact_root,
			items: artifacts,
		},
	}))
}

fn read_fetch_manifest(path: &SPath) -> Result<FetchManifest> {
	if !path.is_file() {
		return Err(Error::InvalidCache(format!("Fetch manifest does not exist: {path}")));
	}

	let content = simple_fs::read_to_string(path)
		.map_err(|error| Error::MalformedState(format!("failed to read Fetch manifest {path}: {error}")))?;

	serde_json::from_str(&content)
		.map_err(|error| Error::MalformedState(format!("failed to deserialize Fetch manifest {path}: {error}")))
}

fn read_fetch_manifest_for_resume(path: &SPath) -> Option<FetchManifest> {
	if !path.is_file() {
		return None;
	}

	let content = simple_fs::read_to_string(path).ok()?;
	serde_json::from_str(&content).ok()
}

fn validate_prior_manifest(manifest: &FetchManifest, context: &WorkflowContext) -> Result<()> {
	if manifest.version != FETCH_MANIFEST_VERSION {
		return Err(Error::MalformedState(format!(
			"unsupported Fetch manifest version: {}",
			manifest.version
		)));
	}

	if !manifest.complete {
		return Err(Error::MalformedState("Fetch manifest is incomplete".to_owned()));
	}

	if manifest.source.trim().is_empty()
		|| manifest.source_path.trim().is_empty()
		|| manifest.artifact_root.trim().is_empty()
	{
		return Err(Error::MalformedState(
			"Fetch manifest is missing source or artifact-root metadata".to_owned(),
		));
	}

	let expected_artifact_root = path_to_string(&context.fetch_cache)?;
	if manifest.artifact_root != expected_artifact_root {
		return Err(Error::MalformedState(
			"Fetch artifact root is incompatible with the current cache; rerun Fetch to rebuild it".to_owned(),
		));
	}

	Ok(())
}

fn manifest_item(
	item: &LocalFetchItem,
	relative_path: &str,
	media_type: Option<String>,
	artifact_path: Option<&SPath>,
	artifact_hash: String,
) -> Result<FetchManifestItem> {
	Ok(FetchManifestItem {
		source: item.source.clone(),
		relative_path: relative_path.to_owned(),
		origin_relative_path: item.relative_path.clone(),
		local_path: path_to_string(&item.local_path)?,
		artifact_path: artifact_path.map(path_to_string).transpose()?,
		media_type,
		content_hash: item.content_hash.clone(),
		artifact_hash,
	})
}

fn validate_source_kind(path: &SPath, identity: &str) -> Result<LocalSourceKind> {
	if !path.exists() {
		return Err(Error::InvalidConfiguration(format!(
			"local source does not exist: {identity}"
		)));
	}

	if path.is_file() {
		return Ok(LocalSourceKind::File);
	}

	if path.is_dir() {
		return Ok(LocalSourceKind::Directory);
	}

	Err(Error::InvalidConfiguration(format!(
		"local source is not a regular file or directory: {identity}"
	)))
}

fn build_glob_patterns(common: &FetchCommonOptions) -> Vec<String> {
	let mut patterns = if common.include.is_empty() {
		vec!["**/*".to_owned()]
	} else {
		common.include.clone()
	};

	patterns.extend(common.exclude.iter().map(|pattern| {
		if pattern.starts_with('!') {
			pattern.clone()
		} else {
			format!("!{pattern}")
		}
	}));

	patterns
}

fn list_paths(root: &SPath, patterns: &[String]) -> Result<(Vec<SPath>, usize)> {
	let mut include_patterns = Vec::new();
	let mut exclude_patterns = Vec::new();

	for pattern in patterns {
		if let Some(exclude_pattern) = pattern.strip_prefix('!')
			&& !exclude_pattern.is_empty()
		{
			exclude_patterns.push(exclude_pattern);
		} else if !pattern.starts_with('!') {
			include_patterns.push(pattern.as_str());
		}
	}

	if include_patterns.is_empty() {
		include_patterns.push("**/*");
	}

	let paths = list_files(root, Some(include_patterns.as_slice()), None)?;

	if exclude_patterns.is_empty() {
		return Ok((paths, 0));
	}

	let matched_count = paths.len();
	let excluded_paths = list_files(root, Some(exclude_patterns.as_slice()), None)?
		.into_iter()
		.map(|path| local_path_for_candidate(root, path))
		.collect::<Vec<_>>();

	let selected_paths = paths
		.into_iter()
		.filter(|candidate| {
			let candidate = local_path_for_candidate(root, candidate.clone());
			let candidate_path: &Path = candidate.as_ref();

			!excluded_paths.iter().any(|excluded| {
				let excluded_path: &Path = excluded.as_ref();
				paths_equivalent(candidate_path, excluded_path)
			})
		})
		.collect::<Vec<_>>();
	let excluded_count = matched_count.saturating_sub(selected_paths.len());

	Ok((selected_paths, excluded_count))
}

fn discover_single_file(path: &SPath, patterns: &[String]) -> Result<(Vec<SPath>, usize)> {
	let source_path: &Path = path.as_ref();
	let file_name = source_path
		.file_name()
		.ok_or_else(|| Error::InvalidConfiguration("local file source has no file name".to_owned()))?;
	let file_name = Path::new(file_name);
	let parent = source_path
		.parent()
		.filter(|parent| !parent.as_os_str().is_empty())
		.unwrap_or_else(|| Path::new("."));
	let parent = SPath::from(parent.to_string_lossy().into_owned());
	let (candidates, _) = list_paths(&parent, patterns)?;

	if candidates
		.iter()
		.any(|candidate| candidate_matches_file(candidate, path, file_name))
	{
		Ok((vec![path.clone()], 0))
	} else {
		Ok((Vec::new(), 1))
	}
}

fn candidate_matches_file(candidate: &SPath, source: &SPath, file_name: &Path) -> bool {
	let candidate_path: &Path = candidate.as_ref();

	paths_equivalent(candidate_path, source.as_ref()) || paths_equivalent(candidate_path, file_name)
}

fn local_path_for_candidate(root: &SPath, candidate: SPath) -> SPath {
	let root_path: &Path = root.as_ref();
	let candidate_path: &Path = candidate.as_ref();

	if candidate_path.starts_with(root_path) {
		candidate
	} else {
		SPath::from(root_path.join(candidate_path).to_string_lossy().into_owned())
	}
}

fn relative_path_for(root: &SPath, path: &SPath) -> Result<String> {
	let root_path: &Path = root.as_ref();
	let path: &Path = path.as_ref();
	let relative = path.strip_prefix(root_path).unwrap_or(path);

	normalize_relative_path(relative)
}

fn relative_file_name(path: &SPath) -> Result<String> {
	let path: &Path = path.as_ref();
	let file_name = path
		.file_name()
		.ok_or_else(|| Error::InvalidConfiguration("local file source has no file name".to_owned()))?;

	normalize_relative_path(Path::new(file_name))
}

// endregion:    --- Support

// region:    --- Tests

#[cfg(test)]
mod tests {
	type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

	use super::*;
	use std::fs::{create_dir_all, write as write_file};
	use std::path::{Path, PathBuf};
	use std::time::{SystemTime, UNIX_EPOCH};

	#[test]
	fn test_process_fetch_discover_local_single_file_metadata_and_hash() -> Result<()> {
		// -- Setup & Fixtures
		let root = fixture_root("test_process_fetch_discover_local_single_file_metadata_and_hash")?;
		let source_path = root.join("lib.rs");
		let contents = b"pub fn answer() -> u32 { 42 }\n";
		write_file(&source_path, contents)?;
		let request = LocalFetchRequest::new(path_text(&source_path));

		// -- Exec
		let discovery = discover_local(&request)?;

		// -- Check
		assert_eq!(discovery.items.len(), 1);
		assert_eq!(discovery.source, path_text(&source_path));

		let item = discovery.items.first().ok_or("single-file discovery should return one item")?;
		assert_eq!(item.source, path_text(&source_path));
		assert_eq!(item.relative_path, "lib.rs");
		assert_eq!(item.media_type.as_deref(), Some("text/rust"));

		let local_path: &Path = item.local_path.as_ref();
		assert_eq!(local_path, source_path.as_path());

		let expected_hash = bs58::encode(blake3::hash(contents).as_bytes()).into_string();
		assert_eq!(item.content_hash, expected_hash);

		Ok(())
	}

	#[test]
	fn test_process_fetch_discover_local_directory_order_and_exclusions() -> Result<()> {
		// -- Setup & Fixtures
		let root = fixture_root("test_process_fetch_discover_local_directory_order_and_exclusions")?;
		let nested = root.join("nested");
		create_dir_all(&nested)?;
		write_file(root.join("alpha.txt"), b"alpha\n")?;
		write_file(nested.join("beta.txt"), b"beta\n")?;
		write_file(nested.join("skip.txt"), b"skip\n")?;

		let request = LocalFetchRequest::new(path_text(&root))
			.with_include(["**/*"])
			.with_exclude(["nested/skip.txt"]);

		// -- Exec
		let discovery = discover_local(&request)?;

		// -- Check
		let relative_paths = discovery
			.items
			.iter()
			.map(|item| item.relative_path.as_str())
			.collect::<Vec<_>>();
		assert_eq!(relative_paths, vec!["alpha.txt", "nested/beta.txt"]);
		assert_eq!(discovery.excluded, 1);

		Ok(())
	}

	#[test]
	fn test_process_fetch_discover_local_negative_glob_selection() -> Result<()> {
		// -- Setup & Fixtures
		let root = fixture_root("test_process_fetch_discover_local_negative_glob_selection")?;
		let nested = root.join("nested");
		create_dir_all(&nested)?;
		write_file(root.join("keep.txt"), b"keep\n")?;
		write_file(nested.join("skip.txt"), b"skip\n")?;

		let request = LocalFetchRequest::new(path_text(&root)).with_include(["**/*", "!nested/skip.txt"]);

		// -- Exec
		let discovery = discover_local(&request)?;

		// -- Check
		let relative_paths = discovery
			.items
			.iter()
			.map(|item| item.relative_path.as_str())
			.collect::<Vec<_>>();
		assert_eq!(relative_paths, vec!["keep.txt"]);

		Ok(())
	}

	#[cfg(unix)]
	#[test]
	fn test_process_fetch_discover_local_skips_symbolic_links() -> Result<()> {
		// -- Setup & Fixtures
		use std::os::unix::fs::symlink;

		let root = fixture_root("test_process_fetch_discover_local_skips_symbolic_links")?;
		let target_path = root.join("real.txt");
		let link_path = root.join("link.txt");
		write_file(&target_path, b"real\n")?;
		symlink("real.txt", &link_path)?;

		let request = LocalFetchRequest::new(path_text(&root));

		// -- Exec
		let discovery = discover_local(&request)?;

		// -- Check
		assert_eq!(discovery.items.len(), 1);
		let item = discovery
			.items
			.first()
			.ok_or("symbolic-link discovery should retain the regular file")?;
		assert_eq!(item.relative_path, "real.txt");

		Ok(())
	}

	#[test]
	fn test_fetchr_local_matches_rejects_version_or_variant_mismatch() -> Result<()> {
		// -- Setup & Fixtures
		let request = LocalFetchRequest::new("src");
		let manifest_local = FetchManifest {
			version: 1,
			complete: true,
			source: "src".to_owned(),
			source_path: "src".to_owned(),
			options: FetchManifestOptions::from(&request),
			artifact_root: "src".to_owned(),
			items: Vec::new(),
		};

		// -- Exec & Check
		assert_ne!(manifest_local.version, FETCH_MANIFEST_VERSION);

		let manifest_web = FetchManifest {
			version: FETCH_MANIFEST_VERSION,
			complete: true,
			source: "src".to_owned(),
			source_path: "src".to_owned(),
			options: FetchManifestOptions::Web {
				include: Vec::new(),
				exclude: Vec::new(),
				format: crate::process::FetchFormat::default(),
				max_depth: 0,
				llms: None,
			},
			artifact_root: "src".to_owned(),
			items: Vec::new(),
		};

		assert_ne!(manifest_web.options, FetchManifestOptions::from(&request));

		Ok(())
	}

	// region:    --- Support

	fn fixture_root(name: &str) -> Result<PathBuf> {
		let timestamp = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
		let root = PathBuf::from("tests-data/.tmp").join(format!("{name}-{}-{timestamp}", std::process::id()));
		create_dir_all(&root)?;
		Ok(root)
	}

	fn path_text(path: &Path) -> String {
		path.to_string_lossy().replace('\\', "/")
	}

	// endregion: --- Support
}

// endregion: --- Tests
