use super::fetchr_types::{FETCH_MANIFEST_VERSION, FetchManifest, FetchManifestItem, FetchManifestOptions};
use super::support::{
	FormattedArtifact, apply_fetch_format, ensure_parent, hash_bytes, is_path_selected, media_type_for, path_to_string,
	write_fetch_artifact, write_fetch_manifest,
};
use crate::fetchr::{FetchCommonOptions, WebFetchOptions, WebFetchRequest};
use crate::process::pipeline::{ArtifactItem, ArtifactSet, StageOutput, WorkflowContext};
use crate::process::{FetchFormat, ItemId, ProcessStage, WebContentSource};
use crate::webc::{WebClient, new_client};
use crate::{Error, Result};
use reqwest::Url;
use simple_fs::{SPath, ensure_dir};
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::Arc;
use tokio::sync::Semaphore;

// region:    --- Execution

pub(crate) async fn execute_http_fetch(request: &WebFetchRequest, context: &WorkflowContext) -> Result<StageOutput> {
	if context.max_concurrency == 0 {
		return Err(Error::InvalidConfiguration(
			"max_concurrency must be greater than zero".to_owned(),
		));
	}

	let mut start_url = Url::parse(&request.source.url)
		.map_err(|err| Error::InvalidConfiguration(format!("invalid web URL '{}': {err}", request.source.url)))?;
	if start_url.scheme() != "http" && start_url.scheme() != "https" {
		return Err(Error::InvalidConfiguration(format!(
			"web URL must use http or https scheme: {}",
			request.source.url
		)));
	}
	start_url.set_fragment(None);

	let base_folder_url = compute_base_folder_url(&start_url);
	let artifact_root = context.fetch_cache.clone();
	ensure_dir(&artifact_root)?;

	let client = new_client(None)?;
	let semaphore = Arc::new(Semaphore::new(context.max_concurrency));

	if request.options.llms_enabled()
		&& let Some(probe) = probe_llms_txt(&client, &base_folder_url).await
	{
		return execute_llms_fetch(
			request,
			context,
			&client,
			&base_folder_url,
			&artifact_root,
			semaphore,
			probe,
		)
		.await;
	}

	let mut visited = HashSet::new();
	visited.insert(start_url.as_str().to_owned());
	let mut registered = HashMap::new();
	register_http_fetch_url(&start_url, &base_folder_url, &request.common, context, &mut registered);

	let mut current_level = vec![start_url];
	let mut current_depth = 0;

	let mut artifacts = Vec::new();
	let mut has_failures = false;
	let mut manifest_items = Vec::new();
	let mut stored_paths = HashSet::new();

	while !current_level.is_empty() {
		let mut tasks = Vec::with_capacity(current_level.len());

		for url in current_level {
			let permit = semaphore
				.clone()
				.acquire_owned()
				.await
				.map_err(|_| Error::MalformedState("Fetch HTTP concurrency control closed".to_owned()))?;
			let client = client.clone();
			let base_folder = base_folder_url.clone();

			if let Some(id) = registered.get(url.as_str()) {
				context.progress.item_running(*id, ProcessStage::Fetch);
			}
			let format = request.common.format;
			let task = tokio::spawn(async move {
				let _permit = permit;
				fetch_single_url(&client, &url, &base_folder, format).await
			});
			tasks.push(task);
		}

		let mut next_level = Vec::new();

		for task in tasks {
			let fetch_outcome = task
				.await
				.map_err(|err| Error::MalformedState(format!("Fetch HTTP task failed: {err}")))?;

			match fetch_outcome {
				Ok(fetched) => {
					let relative_path = fetched.relative_path.clone();
					let origin_relative_path = fetched.origin_relative_path.clone();
					let source_url = fetched.url.as_str().to_owned();

					if is_path_selected(&origin_relative_path, &request.common)
						&& !stored_paths.insert(relative_path.clone())
					{
						let message = format!("multiple input artifacts resolve to fetch path {relative_path}");
						let artifact_path = artifact_root.join(&relative_path);
						has_failures = true;
						if let Some(id) = registered.get(fetched.url.as_str()) {
							context.progress.item_failed(*id, ProcessStage::Fetch, message);
						}
						manifest_items.push(FetchManifestItem {
							source: source_url,
							relative_path: relative_path.clone(),
							origin_relative_path,
							local_path: path_to_string(&artifact_path).unwrap_or_default(),
							artifact_path: None,
							media_type: fetched.media_type.clone(),
							content_hash: fetched.content_hash.clone(),
							artifact_hash: fetched.artifact_hash.clone(),
						});
					} else if is_path_selected(&origin_relative_path, &request.common) {
						let artifact_path = artifact_root.join(&relative_path);
						if let Err(err) = ensure_parent(&artifact_path) {
							let message = err.to_string();
							has_failures = true;
							if let Some(id) = registered.get(fetched.url.as_str()) {
								context.progress.item_failed(*id, ProcessStage::Fetch, message);
							}
							continue;
						}

						if let Err(err) = write_fetch_artifact(&artifact_path, &fetched.body) {
							let message = err.to_string();
							has_failures = true;
							if let Some(id) = registered.get(fetched.url.as_str()) {
								context.progress.item_failed(*id, ProcessStage::Fetch, message);
							}
							let artifact_path_str = path_to_string(&artifact_path).ok();
							manifest_items.push(FetchManifestItem {
								source: source_url,
								relative_path: relative_path.clone(),
								origin_relative_path: origin_relative_path.clone(),
								local_path: artifact_path_str.clone().unwrap_or_default(),
								artifact_path: None,
								media_type: fetched.media_type.clone(),
								content_hash: fetched.content_hash.clone(),
								artifact_hash: fetched.artifact_hash.clone(),
							});
							continue;
						}

						let artifact_path_str = path_to_string(&artifact_path)?;
						if let Some(id) = registered.get(fetched.url.as_str()) {
							context.progress.fetch_completed(*id, &relative_path, artifact_path.clone());
						}
						manifest_items.push(FetchManifestItem {
							source: source_url,
							relative_path: relative_path.clone(),
							origin_relative_path,
							local_path: artifact_path_str.clone(),
							artifact_path: Some(artifact_path_str),
							media_type: fetched.media_type.clone(),
							content_hash: fetched.content_hash.clone(),
							artifact_hash: fetched.artifact_hash.clone(),
						});

						artifacts.push(ArtifactItem {
							source: fetched.url.as_str().to_owned(),
							relative_path: relative_path.clone(),
							local_path: artifact_path,
							media_type: fetched.media_type.clone(),
							source_hash: Some(fetched.artifact_hash.clone()),
						});
					}

					if is_depth_allowed(current_depth + 1, &request.options)
						&& fetched
							.source_media_type
							.as_deref()
							.is_some_and(|media| media.starts_with("text/html"))
						&& let Ok(html_text) = std::str::from_utf8(&fetched.source_body)
						&& let Ok(links) = extract_links(html_text, &fetched.url)
					{
						for link in links {
							if is_url_in_scope(&link, &base_folder_url, &request.options)
								&& visited.insert(link.as_str().to_owned())
							{
								register_http_fetch_url(
									&link,
									&base_folder_url,
									&request.common,
									context,
									&mut registered,
								);
								next_level.push(link);
							}
						}
					}
				}
				Err((url, err_msg)) => {
					has_failures = true;
					if let Some(id) = registered.get(url.as_str()) {
						context.progress.item_failed(*id, ProcessStage::Fetch, err_msg);
					}
				}
			}
		}

		current_depth += 1;
		current_level = next_level;
	}

	context.progress.set_stage_total(ProcessStage::Fetch);
	artifacts.sort_by(|left, right| left.relative_path.cmp(&right.relative_path));
	manifest_items.sort_by(|left, right| left.relative_path.cmp(&right.relative_path));

	let manifest = FetchManifest {
		version: FETCH_MANIFEST_VERSION,
		complete: !has_failures,
		source: request.source.url.clone(),
		source_path: request.source.url.clone(),
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

fn register_http_fetch_url(
	url: &Url,
	base_folder_url: &Url,
	common: &FetchCommonOptions,
	context: &WorkflowContext,
	registered: &mut HashMap<String, ItemId>,
) {
	let key = url.as_str().to_owned();
	if let Ok(relative_path) = url_to_relative_path(url, base_folder_url)
		&& is_path_selected(&relative_path, common)
	{
		let ids = context.progress.register_fetch_items(vec![(key.clone(), relative_path)]);
		if let Some(id) = ids.first() {
			registered.insert(key, *id);
		}
	} else {
		context.progress.add_excluded(ProcessStage::Fetch, 1);
	}
}

async fn execute_llms_fetch(
	request: &WebFetchRequest,
	context: &WorkflowContext,
	client: &WebClient,
	base_folder_url: &Url,
	artifact_root: &SPath,
	semaphore: Arc<Semaphore>,
	probe: LlmsProbeResult,
) -> Result<StageOutput> {
	let mut artifacts = Vec::new();
	let mut has_failures = false;
	let mut manifest_items = Vec::new();

	let origin_llms_relative_path = url_to_relative_path_with_options(&probe.probe_url, base_folder_url, false)?;
	let llms_formatted = apply_fetch_format(
		&origin_llms_relative_path,
		Some("text/plain"),
		&probe.body,
		request.common.format,
	)?;
	let llms_relative_path = llms_formatted.relative_path;
	let llms_artifact_path = artifact_root.join(&llms_relative_path);
	write_fetch_artifact(&llms_artifact_path, &llms_formatted.bytes)
		.map_err(|err| Error::MalformedState(format!("failed to write llms.txt artifact: {err}")))?;

	let llms_artifact_path_str = path_to_string(&llms_artifact_path)?;
	let llms_hash = hash_bytes(&probe.body);
	let mut registered = HashMap::new();
	let llms_ids = context.progress.register_fetch_items(vec![(
		probe.probe_url.as_str().to_owned(),
		origin_llms_relative_path.clone(),
	)]);
	if let Some(id) = llms_ids.first() {
		registered.insert(probe.probe_url.as_str().to_owned(), *id);
		context
			.progress
			.fetch_completed(*id, &llms_relative_path, llms_artifact_path.clone());
	}

	manifest_items.push(FetchManifestItem {
		source: probe.probe_url.as_str().to_owned(),
		relative_path: llms_relative_path.clone(),
		origin_relative_path: origin_llms_relative_path,
		local_path: llms_artifact_path_str.clone(),
		artifact_path: Some(llms_artifact_path_str),
		media_type: Some("text/plain".to_owned()),
		content_hash: llms_hash.clone(),
		artifact_hash: hash_bytes(&llms_formatted.bytes),
	});
	artifacts.push(ArtifactItem {
		source: probe.probe_url.as_str().to_owned(),
		relative_path: llms_relative_path.clone(),
		local_path: llms_artifact_path,
		media_type: Some("text/plain".to_owned()),
		source_hash: Some(llms_hash),
	});

	let mut seen_relative_paths = HashSet::new();
	seen_relative_paths.insert(llms_relative_path.clone());
	let mut stored_paths = HashSet::new();
	stored_paths.insert(llms_relative_path);

	let mut candidates = Vec::new();
	for entry_url in probe.entries {
		if !is_url_in_scope(&entry_url, base_folder_url, &request.options) {
			continue;
		}

		let relative_path = match url_to_relative_path_with_options(&entry_url, base_folder_url, false) {
			Ok(path) => path,
			Err(_) => continue,
		};

		if !seen_relative_paths.insert(relative_path.clone()) {
			continue;
		}

		if is_path_selected(&relative_path, &request.common) {
			candidates.push((entry_url, relative_path));
		} else {
			context.progress.add_excluded(ProcessStage::Fetch, 1);
		}
	}

	let candidate_ids = context.progress.register_fetch_items(
		candidates
			.iter()
			.map(|(url, relative_path)| (url.as_str().to_owned(), relative_path.clone()))
			.collect(),
	);
	for ((url, _), id) in candidates.iter().zip(candidate_ids) {
		registered.insert(url.as_str().to_owned(), id);
	}
	context.progress.set_stage_total(ProcessStage::Fetch);

	let format = request.common.format;
	let mut tasks = Vec::with_capacity(candidates.len());
	for (url, _relative_path) in candidates {
		let permit = semaphore
			.clone()
			.acquire_owned()
			.await
			.map_err(|_| Error::MalformedState("Fetch HTTP concurrency control closed".to_owned()))?;
		let client = client.clone();
		let base_folder = base_folder_url.clone();

		if let Some(id) = registered.get(url.as_str()) {
			context.progress.item_running(*id, ProcessStage::Fetch);
		}
		let task = tokio::spawn(async move {
			let _permit = permit;
			fetch_single_url_with_options(&client, &url, &base_folder, false, format).await
		});
		tasks.push(task);
	}

	for task in tasks {
		let fetch_outcome = task
			.await
			.map_err(|err| Error::MalformedState(format!("Fetch HTTP task failed: {err}")))?;

		match fetch_outcome {
			Ok(fetched) => {
				let relative_path = fetched.relative_path.clone();
				let origin_relative_path = fetched.origin_relative_path.clone();
				let source_url = fetched.url.as_str().to_owned();

				if !stored_paths.insert(relative_path.clone()) {
					let message = format!("multiple input artifacts resolve to fetch path {relative_path}");
					has_failures = true;
					if let Some(id) = registered.get(fetched.url.as_str()) {
						context.progress.item_failed(*id, ProcessStage::Fetch, message);
					}
					let artifact_path = artifact_root.join(&relative_path);
					manifest_items.push(FetchManifestItem {
						source: source_url,
						relative_path: relative_path.clone(),
						origin_relative_path,
						local_path: path_to_string(&artifact_path).unwrap_or_default(),
						artifact_path: None,
						media_type: fetched.media_type.clone(),
						content_hash: fetched.content_hash.clone(),
						artifact_hash: fetched.artifact_hash.clone(),
					});
					continue;
				}

				let artifact_path = artifact_root.join(&relative_path);

				if let Err(err) = ensure_parent(&artifact_path) {
					let message = err.to_string();
					has_failures = true;
					if let Some(id) = registered.get(fetched.url.as_str()) {
						context.progress.item_failed(*id, ProcessStage::Fetch, message);
					}
					continue;
				}

				if let Err(err) = write_fetch_artifact(&artifact_path, &fetched.body) {
					let message = err.to_string();
					has_failures = true;
					if let Some(id) = registered.get(fetched.url.as_str()) {
						context.progress.item_failed(*id, ProcessStage::Fetch, message);
					}
					let artifact_path_str = path_to_string(&artifact_path).ok();
					manifest_items.push(FetchManifestItem {
						source: source_url,
						relative_path: relative_path.clone(),
						origin_relative_path: origin_relative_path.clone(),
						local_path: artifact_path_str.clone().unwrap_or_default(),
						artifact_path: None,
						media_type: fetched.media_type.clone(),
						content_hash: fetched.content_hash.clone(),
						artifact_hash: fetched.artifact_hash.clone(),
					});
					continue;
				}

				let artifact_path_str = path_to_string(&artifact_path)?;
				if let Some(id) = registered.get(fetched.url.as_str()) {
					context.progress.fetch_completed(*id, &relative_path, artifact_path.clone());
				}
				manifest_items.push(FetchManifestItem {
					source: source_url,
					relative_path: relative_path.clone(),
					origin_relative_path,
					local_path: artifact_path_str.clone(),
					artifact_path: Some(artifact_path_str),
					media_type: fetched.media_type.clone(),
					content_hash: fetched.content_hash.clone(),
					artifact_hash: fetched.artifact_hash.clone(),
				});

				artifacts.push(ArtifactItem {
					source: fetched.url.as_str().to_owned(),
					relative_path: relative_path.clone(),
					local_path: artifact_path,
					media_type: fetched.media_type.clone(),
					source_hash: Some(fetched.artifact_hash),
				});
			}
			Err((url, err_msg)) => {
				has_failures = true;
				if let Some(id) = registered.get(url.as_str()) {
					context.progress.item_failed(*id, ProcessStage::Fetch, err_msg);
				}
			}
		}
	}

	artifacts.sort_by(|left, right| left.relative_path.cmp(&right.relative_path));
	manifest_items.sort_by(|left, right| left.relative_path.cmp(&right.relative_path));

	let manifest = FetchManifest {
		version: FETCH_MANIFEST_VERSION,
		complete: !has_failures,
		source: request.source.url.clone(),
		source_path: request.source.url.clone(),
		options: FetchManifestOptions::from(request),
		artifact_root: path_to_string(artifact_root)?,
		items: manifest_items,
	};
	write_fetch_manifest(&context.manifest, &manifest)?;

	Ok(StageOutput {
		artifacts: ArtifactSet {
			root: artifact_root.clone(),
			items: artifacts,
		},
	})
}

pub(crate) fn validate_web_source(source: &WebContentSource) -> Result<()> {
	let url = Url::parse(&source.url)
		.map_err(|err| Error::InvalidConfiguration(format!("invalid web URL '{}': {err}", source.url)))?;
	if url.scheme() != "http" && url.scheme() != "https" {
		return Err(Error::InvalidConfiguration(format!(
			"web URL must use http or https scheme: {}",
			source.url
		)));
	}
	Ok(())
}

struct FetchedHttpResource {
	url: Url,
	origin_relative_path: String,
	relative_path: String,
	body: Vec<u8>,
	source_body: Vec<u8>,
	source_media_type: Option<String>,
	media_type: Option<String>,
	content_hash: String,
	artifact_hash: String,
}

async fn fetch_single_url(
	client: &WebClient,
	url: &Url,
	base_folder_url: &Url,
	format: FetchFormat,
) -> core::result::Result<FetchedHttpResource, (Url, String)> {
	fetch_single_url_with_options(client, url, base_folder_url, true, format).await
}

async fn fetch_single_url_with_options(
	client: &WebClient,
	url: &Url,
	base_folder_url: &Url,
	default_html: bool,
	format: FetchFormat,
) -> core::result::Result<FetchedHttpResource, (Url, String)> {
	let origin_relative_path = match url_to_relative_path_with_options(url, base_folder_url, default_html) {
		Ok(path) => path,
		Err(err) => return Err((url.clone(), err.to_string())),
	};

	let response = match client.get(url.as_str()).await {
		Ok(resp) => resp,
		Err(err) => return Err((url.clone(), err.to_string())),
	};

	if !response.status().is_success() {
		return Err((
			url.clone(),
			format!("HTTP request failed with status: {}", response.status()),
		));
	}

	let header_media_type = response
		.headers()
		.get(reqwest::header::CONTENT_TYPE)
		.and_then(|val| val.to_str().ok())
		.map(|val| val.split(';').next().unwrap_or(val).trim().to_ascii_lowercase())
		.filter(|val| !val.is_empty());

	let media_type = header_media_type
		.or_else(|| media_type_for(Path::new(&origin_relative_path)))
		.or_else(|| {
			if origin_relative_path.ends_with(".html") || origin_relative_path.ends_with(".htm") {
				Some("text/html".to_owned())
			} else {
				None
			}
		});

	let bytes = match response.bytes().await {
		Ok(body) => body.to_vec(),
		Err(err) => return Err((url.clone(), err.to_string())),
	};

	let content_hash = hash_bytes(&bytes);
	let formatted = match apply_fetch_format(&origin_relative_path, media_type.as_deref(), &bytes, format) {
		Ok(formatted) => formatted,
		Err(err) => return Err((url.clone(), err.to_string())),
	};
	let artifact_hash = hash_bytes(&formatted.bytes);

	Ok(FetchedHttpResource {
		url: url.clone(),
		origin_relative_path,
		relative_path: formatted.relative_path,
		body: formatted.bytes,
		source_body: bytes,
		source_media_type: media_type,
		media_type: formatted.media_type,
		content_hash,
		artifact_hash,
	})
}

// endregion: --- Execution

// region:    --- Base Folder & Scope

/// Computes the base folder URL used to constrain web crawling.
///
/// If the URL ends with a trailing slash, it is treated as a directory URL.
/// If it does not end with a trailing slash, it is treated as a leaf resource,
/// and its enclosing parent directory URL is returned.
pub(crate) fn compute_base_folder_url(url: &Url) -> Url {
	let mut base = url.clone();
	base.set_query(None);
	base.set_fragment(None);
	let path = base.path().to_owned();

	if !path.ends_with('/') {
		let last_slash = path.rfind('/').unwrap_or(0);
		base.set_path(&path[..=last_slash]);
	}

	base
}

/// Checks whether a candidate URL falls within the crawling scope.
pub(crate) fn is_url_in_scope(url: &Url, base_folder_url: &Url, options: &WebFetchOptions) -> bool {
	if url.scheme() != "http" && url.scheme() != "https" {
		return false;
	}

	if options.same_host_only && url.host_str() != base_folder_url.host_str() {
		return false;
	}

	let url_str = url.as_str();
	let base_str = base_folder_url.as_str();

	if url_str.starts_with(base_str) {
		return true;
	}

	let trimmed_base = base_str.trim_end_matches('/');
	url_str == trimmed_base
}

/// Validates whether a given crawl depth is allowed by the fetch options.
pub(crate) fn is_depth_allowed(depth: usize, options: &WebFetchOptions) -> bool {
	if depth == 0 {
		return true;
	}

	if !options.follow_links {
		return false;
	}

	depth <= options.max_depth
}

// endregion: --- Base Folder & Scope

// region:    --- llms.txt Discovery & Parsing

struct LlmsProbeResult {
	probe_url: Url,
	body: Vec<u8>,
	entries: Vec<Url>,
}

async fn probe_llms_txt(client: &WebClient, base_folder_url: &Url) -> Option<LlmsProbeResult> {
	let probe_url = base_folder_url.join("llms.txt").ok()?;
	let response = client.get(probe_url.as_str()).await.ok()?;
	if !response.status().is_success() {
		return None;
	}
	let bytes = response.bytes().await.ok()?.to_vec();
	let text = std::str::from_utf8(&bytes).ok()?;
	if text.trim().is_empty() {
		return None;
	}
	let entries = parse_llms_entries(text, &probe_url);
	if entries.is_empty() {
		return None;
	}
	Some(LlmsProbeResult {
		probe_url,
		body: bytes,
		entries,
	})
}

pub(crate) fn parse_llms_entries(content: &str, probe_url: &Url) -> Vec<Url> {
	let mut entries = Vec::new();
	let mut seen = HashSet::new();

	for line in content.lines() {
		let trimmed = line.trim();
		if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with('>') {
			continue;
		}

		if let Some(open_bracket) = trimmed.find('[')
			&& let Some(rel_close) = trimmed[open_bracket..].find("](")
		{
			let start_url = open_bracket + rel_close + 2;
			if let Some(end_url) = trimmed[start_url..].find(')') {
				let raw_url = trimmed[start_url..start_url + end_url].trim();
				if let Some(url) = resolve_llms_url(raw_url, probe_url)
					&& seen.insert(url.as_str().to_owned())
				{
					entries.push(url);
					continue;
				}
			}
		}

		let bare_candidate = trimmed
			.strip_prefix("- ")
			.or_else(|| trimmed.strip_prefix("* "))
			.unwrap_or(trimmed)
			.trim();

		let first_word = bare_candidate.split_whitespace().next().unwrap_or("");
		if (first_word.starts_with("http://") || first_word.starts_with("https://"))
			&& let Some(url) = resolve_llms_url(first_word, probe_url)
			&& seen.insert(url.as_str().to_owned())
		{
			entries.push(url);
		}
	}

	entries
}

fn resolve_llms_url(raw_url: &str, probe_url: &Url) -> Option<Url> {
	let raw_url = raw_url.trim();
	if raw_url.is_empty() {
		return None;
	}

	let clean_url = if let Some(hash_pos) = raw_url.find('#') {
		&raw_url[..hash_pos]
	} else {
		raw_url
	};

	let mut resolved = probe_url.join(clean_url).ok()?;
	if resolved.scheme() != "http" && resolved.scheme() != "https" {
		return None;
	}
	resolved.set_fragment(None);
	Some(resolved)
}

// endregion: --- llms.txt Discovery & Parsing

// region:    --- Link Extraction

/// Extracts and normalizes links from HTML content using `htmlr`.
pub(crate) fn extract_links(html: &str, current_url: &Url) -> Result<Vec<Url>> {
	let elems = htmlr::select(html, ["a"]).map_err(|err| Error::MalformedState(err.to_string()))?;
	let mut links = Vec::new();
	let mut seen = HashSet::new();

	for elem in elems {
		if let Some(attrs) = elem.attrs
			&& let Some(href) = attrs.get("href")
		{
			let href = href.trim();
			if href.is_empty() || href.starts_with('#') {
				continue;
			}

			let href_lower = href.to_ascii_lowercase();
			if href_lower.starts_with("javascript:")
				|| href_lower.starts_with("mailto:")
				|| href_lower.starts_with("tel:")
				|| href_lower.starts_with("data:")
			{
				continue;
			}

			if let Ok(mut resolved) = current_url.join(href) {
				if resolved.scheme() != "http" && resolved.scheme() != "https" {
					continue;
				}

				resolved.set_fragment(None);
				let normalized = resolved.as_str().to_owned();

				if seen.insert(normalized) {
					links.push(resolved);
				}
			}
		}
	}

	Ok(links)
}

/// Converts a scoped URL into a deterministic relative path for artifact storage.
pub(crate) fn url_to_relative_path(url: &Url, base_folder_url: &Url) -> Result<String> {
	url_to_relative_path_with_options(url, base_folder_url, true)
}

pub(crate) fn url_to_relative_path_with_options(
	url: &Url,
	base_folder_url: &Url,
	default_html: bool,
) -> Result<String> {
	let mut clean_url = url.clone();
	clean_url.set_query(None);
	clean_url.set_fragment(None);

	let base_str = base_folder_url.as_str();
	let clean_str = clean_url.as_str();

	let relative = if let Some(stripped) = clean_str.strip_prefix(base_str) {
		stripped
	} else if clean_str == base_str.trim_end_matches('/') {
		""
	} else {
		return Err(Error::InvalidConfiguration(format!(
			"URL '{url}' is outside base folder scope '{base_folder_url}'"
		)));
	};

	let relative = relative.trim_start_matches('/');

	let relative_path = if relative.is_empty() {
		"index.html".to_owned()
	} else if relative.ends_with('/') {
		format!("{relative}index.html")
	} else if default_html && Path::new(relative).extension().is_none() {
		format!("{relative}.html")
	} else {
		relative.to_owned()
	};

	Ok(relative_path)
}

// endregion: --- Link Extraction

// region:    --- Tests

#[cfg(test)]
mod tests {
	type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>; // For tests.

	use super::*;

	#[test]
	fn test_fetchr_http_compute_base_folder_url_trailing_slash() -> Result<()> {
		// -- Setup & Fixtures
		let url = Url::parse("https://docs.typesafe.ai/doc/")?;

		// -- Exec
		let base = compute_base_folder_url(&url);

		// -- Check
		assert_eq!(base.as_str(), "https://docs.typesafe.ai/doc/");

		Ok(())
	}

	#[test]
	fn test_fetchr_http_compute_base_folder_url_leaf_path() -> Result<()> {
		// -- Setup & Fixtures
		let url_leaf = Url::parse("https://docs.typesafe.ai/doc")?;
		let url_intro = Url::parse("https://docs.typesafe.ai/introduction")?;
		let url_nested = Url::parse("https://docs.typesafe.ai/doc/guide/page.html")?;
		let url_query = Url::parse("https://docs.typesafe.ai/doc/?tab=1#section")?;

		// -- Exec
		let base_leaf = compute_base_folder_url(&url_leaf);
		let base_intro = compute_base_folder_url(&url_intro);
		let base_nested = compute_base_folder_url(&url_nested);
		let base_query = compute_base_folder_url(&url_query);

		// -- Check
		assert_eq!(base_leaf.as_str(), "https://docs.typesafe.ai/");
		assert_eq!(base_intro.as_str(), "https://docs.typesafe.ai/");
		assert_eq!(base_nested.as_str(), "https://docs.typesafe.ai/doc/guide/");
		assert_eq!(base_query.as_str(), "https://docs.typesafe.ai/doc/");

		Ok(())
	}

	#[test]
	fn test_fetchr_http_is_url_in_scope_rules() -> Result<()> {
		// -- Setup & Fixtures
		let base = Url::parse("https://docs.typesafe.ai/doc/")?;
		let options_same_host = WebFetchOptions {
			same_host_only: true,
			..WebFetchOptions::default()
		};

		let inside_sub = Url::parse("https://docs.typesafe.ai/doc/guide/intro.html")?;
		let inside_root = Url::parse("https://docs.typesafe.ai/doc")?;
		let outside_sibling = Url::parse("https://docs.typesafe.ai/other")?;
		let outside_host = Url::parse("https://example.com/doc/guide")?;
		let non_http = Url::parse("mailto:user@example.com")?;

		// -- Exec & Check
		assert!(is_url_in_scope(&inside_sub, &base, &options_same_host));
		assert!(is_url_in_scope(&inside_root, &base, &options_same_host));
		assert!(!is_url_in_scope(&outside_sibling, &base, &options_same_host));
		assert!(!is_url_in_scope(&outside_host, &base, &options_same_host));
		assert!(!is_url_in_scope(&non_http, &base, &options_same_host));

		Ok(())
	}

	#[test]
	fn test_fetchr_http_is_depth_allowed_rules() -> Result<()> {
		// -- Setup & Fixtures
		let no_follow = WebFetchOptions {
			follow_links: false,
			max_depth: 3,
			..WebFetchOptions::default()
		};
		let follow_depth_2 = WebFetchOptions {
			follow_links: true,
			max_depth: 2,
			..WebFetchOptions::default()
		};

		// -- Exec & Check
		assert!(is_depth_allowed(0, &no_follow));
		assert!(!is_depth_allowed(1, &no_follow));

		assert!(is_depth_allowed(0, &follow_depth_2));
		assert!(is_depth_allowed(1, &follow_depth_2));
		assert!(is_depth_allowed(2, &follow_depth_2));
		assert!(!is_depth_allowed(3, &follow_depth_2));

		Ok(())
	}

	#[test]
	fn test_fetchr_http_extract_links_filters_and_dedup() -> Result<()> {
		// -- Setup & Fixtures
		let current_url = Url::parse("https://example.com/doc/")?;
		let html = r##"
		<html>
			<body>
				<a href="page1.html">Page 1</a>
				<a href="/doc/page2.html">Page 2</a>
				<a href="page1.html#section">Duplicate Page 1</a>
				<a href="#just-fragment">Fragment Only</a>
				<a href="javascript:void(0)">JS link</a>
				<a href="mailto:info@example.com">Email link</a>
				<a href="tel:+123456789">Phone link</a>
				<a href="https://example.com/doc/page3.html">Absolute link</a>
			</body>
		</html>
		"##;

		// -- Exec
		let links = extract_links(html, &current_url)?;

		// -- Check
		let urls = links.iter().map(Url::as_str).collect::<Vec<_>>();
		assert_eq!(
			urls,
			vec![
				"https://example.com/doc/page1.html",
				"https://example.com/doc/page2.html",
				"https://example.com/doc/page3.html",
			]
		);

		Ok(())
	}

	#[test]
	fn test_fetchr_http_url_to_relative_path_variants() -> Result<()> {
		// -- Setup & Fixtures
		let base = Url::parse("https://docs.typesafe.ai/doc/")?;

		let url_root_slash = Url::parse("https://docs.typesafe.ai/doc/")?;
		let url_root_bare = Url::parse("https://docs.typesafe.ai/doc")?;
		let url_nested_dir = Url::parse("https://docs.typesafe.ai/doc/sub/")?;
		let url_file = Url::parse("https://docs.typesafe.ai/doc/guide.html?v=1#sec")?;
		let url_clean = Url::parse("https://docs.typesafe.ai/doc/introduction")?;
		let url_clean_nested = Url::parse("https://docs.typesafe.ai/doc/concepts/state")?;

		// -- Exec
		let rel_root_slash = url_to_relative_path(&url_root_slash, &base)?;
		let rel_root_bare = url_to_relative_path(&url_root_bare, &base)?;
		let rel_nested_dir = url_to_relative_path(&url_nested_dir, &base)?;
		let rel_file = url_to_relative_path(&url_file, &base)?;
		let rel_clean = url_to_relative_path(&url_clean, &base)?;
		let rel_clean_nested = url_to_relative_path(&url_clean_nested, &base)?;
		let rel_clean_no_default = url_to_relative_path_with_options(&url_clean, &base, false)?;

		// -- Check
		assert_eq!(rel_root_slash, "index.html");
		assert_eq!(rel_root_bare, "index.html");
		assert_eq!(rel_nested_dir, "sub/index.html");
		assert_eq!(rel_file, "guide.html");
		assert_eq!(rel_clean, "introduction.html");
		assert_eq!(rel_clean_nested, "concepts/state.html");
		assert_eq!(rel_clean_no_default, "introduction");

		Ok(())
	}

	#[test]
	fn test_fetchr_http_parse_llms_entries_markdown_links_and_descriptions() -> Result<()> {
		// -- Setup & Fixtures
		let probe_url = Url::parse("https://docs.typesafe.ai/llms.txt")?;
		let content = r##"# TypeSafe AI

> Summary of the documentation.

- [Introduction](https://docs.typesafe.ai/introduction.md): Model overview.
* [Quick start](https://docs.typesafe.ai/introduction/quickstart.md): Getting started.
- [System One](concepts/system-one.md): Architecture description.
- [Primitives (Questions)](/primitives.md): Question types.
"##;

		// -- Exec
		let entries = parse_llms_entries(content, &probe_url);

		// -- Check
		let urls = entries.iter().map(Url::as_str).collect::<Vec<_>>();
		assert_eq!(
			urls,
			vec![
				"https://docs.typesafe.ai/introduction.md",
				"https://docs.typesafe.ai/introduction/quickstart.md",
				"https://docs.typesafe.ai/concepts/system-one.md",
				"https://docs.typesafe.ai/primitives.md",
			]
		);

		Ok(())
	}

	#[test]
	fn test_fetchr_http_parse_llms_entries_bare_urls_and_deduplication() -> Result<()> {
		// -- Setup & Fixtures
		let probe_url = Url::parse("https://docs.typesafe.ai/doc/llms.txt")?;
		let content = r##"
# Bare URLs and duplicates
https://docs.typesafe.ai/doc/page1.md
- https://docs.typesafe.ai/doc/page2.md
* https://docs.typesafe.ai/doc/page1.md
- [Duplicate Page 1](page1.md)
https://docs.typesafe.ai/doc/page2.md#anchor
"##;

		// -- Exec
		let entries = parse_llms_entries(content, &probe_url);

		// -- Check
		let urls = entries.iter().map(Url::as_str).collect::<Vec<_>>();
		assert_eq!(
			urls,
			vec!["https://docs.typesafe.ai/doc/page1.md", "https://docs.typesafe.ai/doc/page2.md"]
		);

		Ok(())
	}

	#[tokio::test]
	async fn test_fetchr_http_execute_http_fetch_crawl_and_manifest() -> Result<()> {
		// -- Setup & Fixtures
		use crate::process::progress::new_progress_channel;
		use crate::process::state::ProcessStateStore;
		use std::sync::Arc;
		use tokio::io::{AsyncReadExt, AsyncWriteExt};
		use tokio::net::TcpListener;

		let listener = TcpListener::bind("127.0.0.1:0").await?;
		let port = listener.local_addr()?.port();

		tokio::spawn(async move {
			while let Ok((mut socket, _)) = listener.accept().await {
				tokio::spawn(async move {
					let mut buf = vec![0u8; 1024];
					let n = socket.read(&mut buf).await.unwrap_or(0);
					let req = String::from_utf8_lossy(&buf[..n]);
					let first_line = req.lines().next().unwrap_or("");
					let path = first_line.split_whitespace().nth(1).unwrap_or("/");

					let (status, content_type, body) = match path {
						"/" | "/index.html" => (
							"200 OK",
							"text/html; charset=utf-8",
							"<html><body><h1>Home</h1><a href=\"page1.html\">P1</a></body></html>",
						),
						"/page1.html" => (
							"200 OK",
							"text/html; charset=utf-8",
							"<html><body><h1>Page 1</h1></body></html>",
						),
						_ => ("404 Not Found", "text/plain", "Not Found"),
					};

					let resp = format!(
						"HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
						body.len()
					);
					let _ = socket.write_all(resp.as_bytes()).await;
				});
			}
		});

		let test_id = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)?.as_millis();
		let dest = SPath::from(format!("tests-data/.tmp/test_http_fetch_{test_id}"));
		let fetch_cache = dest.join(".tmp-zmapr/01-fetch");
		let manifest = dest.join(".tmp-zmapr/manifest.json");

		let request = WebFetchRequest::new(format!("http://127.0.0.1:{port}/"))
			.with_follow_links(true)
			.with_max_depth(2);

		let (tx, _rx) = new_progress_channel()?;
		let state = Arc::new(ProcessStateStore::default());
		let query = crate::process::ProcessQuery::new(state.clone());
		let progress = crate::process::progress::ProcessProgressPublisher::new(tx, state);

		let context = WorkflowContext {
			source: Some(crate::process::ContentSource::Web(request.source.clone())),
			destination: dest.clone(),
			fetch_cache: fetch_cache.clone(),
			sanitize_output: dest.join(".tmp-zmapr/stages/sanitize"),
			sanitize_manifest: dest.join(".tmp-zmapr/sanitize-manifest.json"),
			manifest: manifest.clone(),
			journal: dest.join(".tmp-zmapr/content-map.journal.jsonl"),
			content_map: dest.join("content-map.json"),
			max_concurrency: 2,
			resume: false,
			progress,
		};

		// -- Exec
		let stage_output = execute_http_fetch(&request, &context).await?;

		// -- Check
		assert_eq!(query.stats().fetch.completed, 2);
		assert_eq!(query.stats().fetch.failed, 0);
		assert_eq!(stage_output.artifacts.items.len(), 2);

		let index_artifact = fetch_cache.join("index.md");
		let page1_artifact = fetch_cache.join("page1.md");
		assert!(index_artifact.is_file());
		assert!(page1_artifact.is_file());
		assert!(manifest.is_file());

		Ok(())
	}
}

// endregion: --- Tests
