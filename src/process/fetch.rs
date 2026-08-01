use std::fs::{copy, rename, write};
use std::path::{Component, Path};
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use simple_fs::{ensure_dir, list_files, read_to_string, SPath};
use tokio::sync::Semaphore;

use super::pipeline::{ArtifactItem, ArtifactSet, StageOutput, WorkflowContext};
use super::progress::ProcessProgress;
use super::options::FetchOptions;
use super::response::{ProcessFailure, ProcessItem, ProcessStage};
use super::source::LocalContentSource;
use crate::{Error, Result};

// region:    --- Types

#[derive(Debug, Clone)]
pub(crate) struct LocalFetchDiscovery {
	pub(crate) source: String,
	pub(crate) source_path: SPath,
	pub(crate) items: Vec<LocalFetchItem>,
}

#[derive(Debug, Clone)]
pub(crate) struct LocalFetchItem {
	pub(crate) source: String,
	pub(crate) relative_path: String,
	pub(crate) local_path: SPath,
	pub(crate) media_type: Option<String>,
	pub(crate) content_hash: String,
}

#[derive(Debug, Deserialize, Serialize)]
struct FetchManifest {
	version: u32,
	complete: bool,
	source: String,
	source_path: String,
	options: FetchManifestOptions,
	artifact_root: String,
	items: Vec<FetchManifestItem>,
}

#[derive(Debug, Deserialize, PartialEq, Eq, Serialize)]
struct FetchManifestOptions {
	include: Vec<String>,
	exclude: Vec<String>,
	copy_local_files: bool,
	same_host_only: bool,
	max_depth: usize,
	follow_links: bool,
}

#[derive(Debug, Deserialize, Serialize)]
struct FetchManifestItem {
	source: String,
	relative_path: String,
	local_path: String,
	artifact_path: Option<String>,
	media_type: Option<String>,
	content_hash: String,
}

// endregion: --- Types

// region:    --- Public Functions

pub(crate) fn discover_local(
	source: &LocalContentSource,
	options: &FetchOptions,
) -> Result<LocalFetchDiscovery> {
	let source_path = &source.path;
	let source_identity = source_identity(source_path)?;
	let source_kind = validate_source_kind(source_path, &source_identity)?;
	let patterns = build_glob_patterns(options);

	let candidates = match source_kind {
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
	})
}

pub(crate) async fn execute_local_fetch(
	source: &LocalContentSource,
	options: &FetchOptions,
	context: &WorkflowContext,
) -> Result<StageOutput> {
	if context.max_concurrency == 0 {
		return Err(Error::InvalidConfiguration(
			"max_concurrency must be greater than zero".to_owned(),
		));
	}

	if context.resume
		&& let Some(output) = try_resume_local_fetch(source, options, context)?
	{
		return Ok(output);
	}

	let LocalFetchDiscovery {
		source: source_identity,
		source_path,
		items,
	} = discover_local(source, options)?;
	let artifact_root = if options.copy_local_files {
		ensure_dir(&context.fetch_cache)?;
		context.fetch_cache.clone()
	} else {
		source_path.clone()
	};

	let mut artifacts = Vec::with_capacity(items.len());
	let mut completed_items = Vec::with_capacity(items.len());
	let mut failures = Vec::new();
	let mut manifest_items = Vec::with_capacity(items.len());

	let semaphore = Arc::new(Semaphore::new(context.max_concurrency));
	let mut materializations = Vec::with_capacity(items.len());

	for item in &items {
		let artifact_path = if options.copy_local_files {
			context.fetch_cache.join(item.relative_path.as_str())
		} else {
			item.local_path.clone()
		};
		let source_path = item.local_path.clone();
		let task_artifact_path = artifact_path.clone();
		let copy_local_files = options.copy_local_files;
		let permit = semaphore.clone().acquire_owned().await.map_err(|_| {
			Error::MalformedState(
				"Fetch materialization concurrency control closed".to_owned(),
			)
		})?;
		let task = tokio::task::spawn_blocking(move || {
			let _permit = permit;
			if copy_local_files {
				copy_local_file(&source_path, &task_artifact_path)
					.map_err(|error| error.to_string())
			} else {
				Ok(())
			}
		});
		materializations.push((item.clone(), artifact_path, task));
	}

	for (item, artifact_path, task) in materializations {
		let process_item = ProcessItem {
			source: item.relative_path.clone(),
			output_path: Some(artifact_path.clone()),
			stage: ProcessStage::Fetch,
		};
		let item_result = task.await.map_err(|error| {
			Error::MalformedState(format!("Fetch materialization task failed: {error}"))
		})?;

		match item_result {
			Ok(()) => {
				let artifact = ArtifactItem {
					source: item.source.clone(),
					relative_path: item.relative_path.clone(),
					local_path: artifact_path.clone(),
					media_type: item.media_type.clone(),
					source_hash: Some(item.content_hash.clone()),
				};
				manifest_items.push(manifest_item(&item, Some(&artifact_path))?);
				artifacts.push(artifact);
				completed_items.push(process_item.clone());
				context
					.progress
					.publish(ProcessProgress::ItemCompleted { item: process_item });
			}
			Err(error) => {
				let failed_item = ProcessItem {
					source: item.relative_path.clone(),
					output_path: None,
					stage: ProcessStage::Fetch,
				};
				let failure = ProcessFailure {
					item: failed_item,
					message: error,
				};
				manifest_items.push(manifest_item(&item, None)?);
				failures.push(failure.clone());
				context
					.progress
					.publish(ProcessProgress::ItemFailed { failure });
			}
		}
	}

	let manifest = FetchManifest {
		version: 1,
		complete: failures.is_empty(),
		source: source_identity,
		source_path: path_to_string(&source_path)?,
		options: FetchManifestOptions::from(options),
		artifact_root: path_to_string(&artifact_root)?,
		items: manifest_items,
	};
	write_fetch_manifest(&context.manifest, &manifest)?;

	Ok(StageOutput {
		artifacts: ArtifactSet {
			root: artifact_root,
			items: artifacts,
		},
		completed_items,
		skipped_items: Vec::new(),
		failures,
	})
}

pub(crate) fn load_prior_local_fetch(context: &WorkflowContext) -> Result<ArtifactSet> {
	let manifest = read_fetch_manifest(&context.manifest)?;
	validate_prior_manifest(&manifest, context)?;

	let copy_local_files = manifest.options.copy_local_files;
	let artifact_root = SPath::from(manifest.artifact_root.clone());
	let root_available = if copy_local_files {
		artifact_root.is_dir()
	} else {
		artifact_root.exists()
	};

	if !root_available {
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
		{
			return Err(Error::MalformedState(format!(
				"Fetch manifest item is missing required metadata: {}",
				manifest_item.relative_path
			)));
		}

		let relative_path = normalize_manifest_relative_path(&manifest_item.relative_path)
			.map_err(|error| {
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
		let expected_artifact_path = if copy_local_files {
			artifact_root.join(relative_path.as_str())
		} else {
			SPath::from(manifest_item.local_path.clone())
		};
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

		let artifact_hash = hash_file(&artifact_path).map_err(|error| {
			Error::InvalidCache(format!(
				"failed to read Fetch artifact {artifact_path}: {error}"
			))
		})?;

		if artifact_hash != manifest_item.content_hash {
			return Err(Error::InvalidCache(format!(
				"Fetch artifact hash does not match the manifest: {artifact_path}"
			)));
		}

		items.push(ArtifactItem {
			source: manifest_item.source,
			relative_path,
			local_path: artifact_path,
			media_type: manifest_item.media_type,
			source_hash: Some(manifest_item.content_hash),
		});
	}

	if items.windows(2).any(|window| {
		window[0].relative_path.as_str() >= window[1].relative_path.as_str()
	}) {
		return Err(Error::MalformedState(
			"Fetch manifest items are not in deterministic order".to_owned(),
		));
	}

	Ok(ArtifactSet {
		root: artifact_root,
		items,
	})
}

// endregion: --- Public Functions

// region:    --- Froms

impl From<&FetchOptions> for FetchManifestOptions {
	fn from(options: &FetchOptions) -> Self {
		Self {
			include: options.include.clone(),
			exclude: options.exclude.clone(),
			copy_local_files: options.copy_local_files,
			same_host_only: options.same_host_only,
			max_depth: options.max_depth,
			follow_links: options.follow_links,
		}
	}
}

// endregion: --- Froms

// region:    --- Support

fn try_resume_local_fetch(
	source: &LocalContentSource,
	options: &FetchOptions,
	context: &WorkflowContext,
) -> Result<Option<StageOutput>> {
	let Some(manifest) = read_fetch_manifest_for_resume(&context.manifest) else {
		return Ok(None);
	};

	let discovery = discover_local(source, options)?;

	if !fetch_manifest_matches(&manifest, &discovery, options, context)? {
		return Ok(None);
	}

	build_reused_stage_output(&manifest, &discovery, options, context)
}

fn fetch_manifest_matches(
	manifest: &FetchManifest,
	discovery: &LocalFetchDiscovery,
	options: &FetchOptions,
	context: &WorkflowContext,
) -> Result<bool> {
	if manifest.version != 1
		|| !manifest.complete
		|| manifest.options != FetchManifestOptions::from(options)
	{
		return Ok(false);
	}

	let expected_source_path = path_to_string(&discovery.source_path)?;
	let expected_artifact_root_path =
		artifact_root_for(&discovery.source_path, options, context);
	let expected_artifact_root = path_to_string(&expected_artifact_root_path)?;

	if manifest.source != discovery.source
		|| manifest.source_path != expected_source_path
		|| manifest.artifact_root != expected_artifact_root
		|| manifest.items.len() != discovery.items.len()
	{
		return Ok(false);
	}

	for (manifest_item, current_item) in manifest.items.iter().zip(discovery.items.iter()) {
		let expected_local_path = path_to_string(&current_item.local_path)?;
		let expected_artifact_path = if options.copy_local_files {
			expected_artifact_root_path.join(current_item.relative_path.as_str())
		} else {
			current_item.local_path.clone()
		};
		let expected_artifact_path = path_to_string(&expected_artifact_path)?;

		if manifest_item.source != current_item.source
			|| manifest_item.relative_path != current_item.relative_path
			|| manifest_item.local_path != expected_local_path
			|| manifest_item.artifact_path.as_deref()
				!= Some(expected_artifact_path.as_str())
			|| manifest_item.media_type != current_item.media_type
			|| manifest_item.content_hash != current_item.content_hash
		{
			return Ok(false);
		}
	}

	Ok(true)
}

fn build_reused_stage_output(
	manifest: &FetchManifest,
	discovery: &LocalFetchDiscovery,
	options: &FetchOptions,
	context: &WorkflowContext,
) -> Result<Option<StageOutput>> {
	let artifact_root = artifact_root_for(&discovery.source_path, options, context);
	let mut artifacts = Vec::with_capacity(discovery.items.len());
	let mut skipped_items = Vec::with_capacity(discovery.items.len());

	for (manifest_item, current_item) in manifest.items.iter().zip(discovery.items.iter()) {
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

		if artifact_hash != current_item.content_hash {
			return Ok(None);
		}

		let artifact = ArtifactItem {
			source: current_item.source.clone(),
			relative_path: current_item.relative_path.clone(),
			local_path: artifact_path.clone(),
			media_type: current_item.media_type.clone(),
			source_hash: Some(current_item.content_hash.clone()),
		};
		let process_item = ProcessItem {
			source: current_item.relative_path.clone(),
			output_path: Some(artifact_path),
			stage: ProcessStage::Fetch,
		};

		context
			.progress
			.publish(ProcessProgress::ItemSkipped {
				item: process_item.clone(),
			});
		artifacts.push(artifact);
		skipped_items.push(process_item);
	}

	Ok(Some(StageOutput {
		artifacts: ArtifactSet {
			root: artifact_root,
			items: artifacts,
		},
		completed_items: Vec::new(),
		skipped_items,
		failures: Vec::new(),
	}))
}

fn read_fetch_manifest(path: &SPath) -> Result<FetchManifest> {
	if !path.is_file() {
		return Err(Error::InvalidCache(format!(
			"Fetch manifest does not exist: {path}"
		)));
	}

	let content = read_to_string(path).map_err(|error| {
		Error::MalformedState(format!("failed to read Fetch manifest {path}: {error}"))
	})?;

	serde_json::from_str(&content).map_err(|error| {
		Error::MalformedState(format!(
			"failed to deserialize Fetch manifest {path}: {error}"
		))
	})
}

fn read_fetch_manifest_for_resume(path: &SPath) -> Option<FetchManifest> {
	if !path.is_file() {
		return None;
	}

	let content = read_to_string(path).ok()?;
	serde_json::from_str(&content).ok()
}

fn validate_prior_manifest(
	manifest: &FetchManifest,
	context: &WorkflowContext,
) -> Result<()> {
	if manifest.version != 1 {
		return Err(Error::MalformedState(format!(
			"unsupported Fetch manifest version: {}",
			manifest.version
		)));
	}

	if !manifest.complete {
		return Err(Error::MalformedState(
			"Fetch manifest is incomplete".to_owned(),
		));
	}

	if manifest.source.trim().is_empty()
		|| manifest.source_path.trim().is_empty()
		|| manifest.artifact_root.trim().is_empty()
	{
		return Err(Error::MalformedState(
			"Fetch manifest is missing source or artifact-root metadata".to_owned(),
		));
	}

	if manifest.options.copy_local_files {
		let expected_artifact_root = path_to_string(&context.fetch_cache)?;
		if manifest.artifact_root != expected_artifact_root {
			return Err(Error::MalformedState(
				"Fetch manifest artifact root is incompatible with the workflow cache"
					.to_owned(),
			));
		}
	}

	Ok(())
}

fn artifact_root_for(
	source_path: &SPath,
	options: &FetchOptions,
	context: &WorkflowContext,
) -> SPath {
	if options.copy_local_files {
		context.fetch_cache.clone()
	} else {
		source_path.clone()
	}
}

fn normalize_manifest_relative_path(value: &str) -> Result<String> {
	let value = value.replace('\\', "/");
	normalize_relative_path(Path::new(&value))
}

fn copy_local_file(source: &SPath, destination: &SPath) -> Result<()> {
	ensure_parent(destination)?;
	copy(source.as_std_path(), destination.as_std_path()).map_err(|error| {
		Error::MalformedState(format!(
			"failed to copy local artifact from {source} to {destination}: {error}"
		))
	})?;
	Ok(())
}

fn manifest_item(
	item: &LocalFetchItem,
	artifact_path: Option<&SPath>,
) -> Result<FetchManifestItem> {
	Ok(FetchManifestItem {
		source: item.source.clone(),
		relative_path: item.relative_path.clone(),
		local_path: path_to_string(&item.local_path)?,
		artifact_path: artifact_path.map(path_to_string).transpose()?,
		media_type: item.media_type.clone(),
		content_hash: item.content_hash.clone(),
	})
}

fn write_fetch_manifest(path: &SPath, manifest: &FetchManifest) -> Result<()> {
	ensure_parent(path)?;
	let manifest_json = serde_json::to_string_pretty(manifest).map_err(|error| {
		Error::MalformedState(format!("failed to serialize Fetch manifest: {error}"))
	})?;
	let temporary_path = SPath::from(format!("{path}.tmp"));
	let manifest_content = format!("{manifest_json}\n");
	write(
		temporary_path.as_std_path(),
		manifest_content.as_bytes(),
	)
	.map_err(|error| {
		Error::MalformedState(format!(
			"failed to write Fetch manifest {temporary_path}: {error}"
		))
	})?;
	rename(temporary_path.as_std_path(), path.as_std_path()).map_err(|error| {
		Error::MalformedState(format!(
			"failed to replace Fetch manifest {path}: {error}"
		))
	})?;
	Ok(())
}

fn ensure_parent(path: &SPath) -> Result<()> {
	let path_ref: &Path = path.as_ref();
	let parent = path_ref.parent().ok_or_else(|| {
		Error::MalformedState(format!("workflow path has no parent: {path}"))
	})?;
	let parent = SPath::from(parent.to_string_lossy().into_owned());
	ensure_dir(&parent)?;
	Ok(())
}

fn path_to_string(path: &SPath) -> Result<String> {
	let path_ref: &Path = path.as_ref();
	path_ref
		.to_str()
		.map(|value| value.replace('\\', "/"))
		.ok_or_else(|| Error::MalformedState("workflow path is not valid UTF-8".to_owned()))
}

#[derive(Debug, Clone, Copy)]
enum LocalSourceKind {
	File,
	Directory,
}

pub(super) fn validate_source(source: &LocalContentSource) -> Result<()> {
	let identity = source_identity(&source.path)?;
	validate_source_kind(&source.path, &identity).map(|_| ())
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

fn build_glob_patterns(options: &FetchOptions) -> Vec<String> {
	let mut patterns = if options.include.is_empty() {
		vec!["**/*".to_owned()]
	} else {
		options.include.clone()
	};

	patterns.extend(options.exclude.iter().map(|pattern| {
		if pattern.starts_with('!') {
			pattern.clone()
		} else {
			format!("!{pattern}")
		}
	}));

	patterns
}

fn list_paths(root: &SPath, patterns: &[String]) -> Result<Vec<SPath>> {
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
		return Ok(paths);
	}

	let excluded_paths = list_files(root, Some(exclude_patterns.as_slice()), None)?
		.into_iter()
		.map(|path| local_path_for_candidate(root, path))
		.collect::<Vec<_>>();

	Ok(paths
		.into_iter()
		.filter(|candidate| {
			let candidate = local_path_for_candidate(root, candidate.clone());
			let candidate_path: &Path = candidate.as_ref();

			!excluded_paths.iter().any(|excluded| {
				let excluded_path: &Path = excluded.as_ref();
				paths_equivalent(candidate_path, excluded_path)
			})
		})
		.collect())
}

fn discover_single_file(path: &SPath, patterns: &[String]) -> Result<Vec<SPath>> {
	let source_path: &Path = path.as_ref();
	let file_name = source_path.file_name().ok_or_else(|| {
		Error::InvalidConfiguration("local file source has no file name".to_owned())
	})?;
	let file_name = Path::new(file_name);
	let parent = source_path
		.parent()
		.filter(|parent| !parent.as_os_str().is_empty())
		.unwrap_or_else(|| Path::new("."));
	let parent = SPath::from(parent.to_string_lossy().into_owned());
	let candidates = list_paths(&parent, patterns)?;

	if candidates
		.iter()
		.any(|candidate| candidate_matches_file(candidate, path, file_name))
	{
		Ok(vec![path.clone()])
	} else {
		Ok(Vec::new())
	}
}

fn candidate_matches_file(candidate: &SPath, source: &SPath, file_name: &Path) -> bool {
	let candidate_path: &Path = candidate.as_ref();

	paths_equivalent(candidate_path, source.as_ref())
		|| paths_equivalent(candidate_path, file_name)
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
	let file_name = path.file_name().ok_or_else(|| {
		Error::InvalidConfiguration("local file source has no file name".to_owned())
	})?;

	normalize_relative_path(Path::new(file_name))
}

fn normalize_relative_path(path: &Path) -> Result<String> {
	if path.is_absolute()
		|| path
			.components()
			.any(|component| component == Component::ParentDir)
	{
		return Err(Error::InvalidConfiguration(
			"local source produced an invalid relative path".to_owned(),
		));
	}

	let value = path.to_str().ok_or_else(|| {
		Error::InvalidConfiguration("local source path is not valid UTF-8".to_owned())
	})?;
	let value = value.replace('\\', "/");
	let value = value.strip_prefix("./").unwrap_or(&value);

	if value.is_empty() || value == "." {
		return Err(Error::InvalidConfiguration(
			"local source produced an empty relative path".to_owned(),
		));
	}

	Ok(value.to_owned())
}

fn source_identity(path: &SPath) -> Result<String> {
	let path: &Path = path.as_ref();
	let value = path.to_str().ok_or_else(|| {
		Error::InvalidConfiguration("local source path is not valid UTF-8".to_owned())
	})?;

	Ok(value.replace('\\', "/"))
}

fn paths_equivalent(left: &Path, right: &Path) -> bool {
	left.components().eq(right.components())
}

fn hash_file(path: &SPath) -> Result<String> {
	let contents = read_to_string(path)?;
	let digest = Sha256::digest(contents.as_bytes());

	Ok(format!("{digest:x}"))
}

fn media_type_for(path: &Path) -> Option<String> {
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
		let root =
			fixture_root("test_process_fetch_discover_local_single_file_metadata_and_hash")?;
		let source_path = root.join("lib.rs");
		let contents = b"pub fn answer() -> u32 { 42 }\n";
		write_file(&source_path, contents)?;
		let source = LocalContentSource::new(path_text(&source_path));
		let options = FetchOptions::default();

		// -- Exec
		let discovery = discover_local(&source, &options)?;

		// -- Check
		assert_eq!(discovery.items.len(), 1);
		assert_eq!(discovery.source, path_text(&source_path));

		let item = discovery
			.items
			.first()
			.ok_or("single-file discovery should return one item")?;
		assert_eq!(item.source, path_text(&source_path));
		assert_eq!(item.relative_path, "lib.rs");
		assert_eq!(item.media_type.as_deref(), Some("text/rust"));

		let local_path: &Path = item.local_path.as_ref();
		assert_eq!(local_path, source_path.as_path());

		let digest = Sha256::digest(contents);
		let expected_hash = format!("{digest:x}");
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

		let source = LocalContentSource::new(path_text(&root));
		let options = FetchOptions {
			include: vec!["**/*".to_owned()],
			exclude: vec!["nested/skip.txt".to_owned()],
			..FetchOptions::default()
		};

		// -- Exec
		let discovery = discover_local(&source, &options)?;

		// -- Check
		let relative_paths = discovery
			.items
			.iter()
			.map(|item| item.relative_path.as_str())
			.collect::<Vec<_>>();
		assert_eq!(relative_paths, vec!["alpha.txt", "nested/beta.txt"]);

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

		let source = LocalContentSource::new(path_text(&root));
		let options = FetchOptions {
			include: vec!["**/*".to_owned(), "!nested/skip.txt".to_owned()],
			..FetchOptions::default()
		};

		// -- Exec
		let discovery = discover_local(&source, &options)?;

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

		let source = LocalContentSource::new(path_text(&root));
		let options = FetchOptions::default();

		// -- Exec
		let discovery = discover_local(&source, &options)?;

		// -- Check
		assert_eq!(discovery.items.len(), 1);
		let item = discovery
			.items
			.first()
			.ok_or("symbolic-link discovery should retain the regular file")?;
		assert_eq!(item.relative_path, "real.txt");

		Ok(())
	}

	// region:    --- Support

	fn fixture_root(name: &str) -> Result<PathBuf> {
		let timestamp = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
		let root = PathBuf::from("tests-data/.tmp").join(format!(
			"{name}-{}-{timestamp}",
			std::process::id()
		));
		create_dir_all(&root)?;
		Ok(root)
	}

	fn path_text(path: &Path) -> String {
		path.to_string_lossy().replace('\\', "/")
	}

	// endregion: --- Support
}

// endregion: --- Tests
