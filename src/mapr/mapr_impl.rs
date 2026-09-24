use crate::mapr::{
	ContentMapDocument, FileMapEntry, FileMapMetadata, JournalHeader, JournalRecord, PROMPT_VERSION, html_to_markdown,
	init_or_load_journal, is_html_item, is_text_mappable, parse_file_info, publish_content_map, remove_journal,
	render_file_prompt, select_active_ai_client,
};
use crate::process::pipeline::{ArtifactItem, ArtifactSet, StageOutput, WorkflowContext};
use crate::process::{ContentMapOptions, ProcessFailure, ProcessItem, ProcessProgress, ProcessStage};
use crate::support::hash_bytes;
use crate::Result;
use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

// region:    --- Types

struct PreparedArtifact {
	item: ArtifactItem,
	relative_path: String,
	content: Vec<u8>,
	metadata: FileMapMetadata,
}

// endregion: --- Types

// region:    --- Operations

pub(crate) async fn execute_content_map(
	context: &WorkflowContext,
	input: ArtifactSet,
	options: &ContentMapOptions,
) -> Result<StageOutput> {
	context.progress.publish(ProcessProgress::StageStarted {
		stage: ProcessStage::AiContentMap,
	});

	let journal_path = options
		.journal_path
		.as_ref()
		.map(|path| path.as_std_path())
		.unwrap_or_else(|| context.journal.as_std_path());

	let prompt_version = PROMPT_VERSION;
	let header = JournalHeader::new(&options.model, prompt_version, context.mapper_output.as_str());
	let (reuse_index, appender) = init_or_load_journal(journal_path, &header)?;

	let previous_metadata = load_previous_file_metadata(context.content_map.as_std_path());
	let (prepared_items, mut failures) =
		prepare_mapper_artifacts(context, &input, options.to_md.unwrap_or(true), &previous_metadata);
	let mut completed_items = Vec::new();
	let mut skipped_items = Vec::new();
	let mut file_map = BTreeMap::new();
	let mut pending_items = Vec::new();

	for prepared in &prepared_items {
		let item = &prepared.item;
		if !is_text_mappable(item.media_type.as_deref(), Path::new(&prepared.relative_path)) {
			let process_item = ProcessItem {
				source: item.relative_path.clone(),
				output_path: None,
				stage: ProcessStage::AiContentMap,
				usage: None,
			};
			context.progress.publish(ProcessProgress::ItemSkipped {
				item: process_item.clone(),
			});
			skipped_items.push(process_item);
			continue;
		}

		if let Some(max_size) = options.max_size
			&& prepared.content.len() > max_size
		{
			let process_item = ProcessItem {
				source: item.relative_path.clone(),
				output_path: None,
				stage: ProcessStage::AiContentMap,
				usage: None,
			};
			context.progress.publish(ProcessProgress::ItemSkipped {
				item: process_item.clone(),
			});
			skipped_items.push(process_item);
			continue;
		}

		let content_str = match std::str::from_utf8(&prepared.content) {
			Ok(str_val) => str_val,
			Err(_) => {
				let process_item = ProcessItem {
					source: item.relative_path.clone(),
					output_path: None,
					stage: ProcessStage::AiContentMap,
					usage: None,
				};
				context.progress.publish(ProcessProgress::ItemSkipped {
					item: process_item.clone(),
				});
				skipped_items.push(process_item);
				continue;
			}
		};

		if options.reuse_unchanged_records
			&& let Some(cached_entry) = reuse_index.get_file(&prepared.relative_path, &prepared.metadata.prepared_hash)
		{
			let process_item = ProcessItem {
				source: item.relative_path.clone(),
				output_path: Some(context.content_map.clone()),
				stage: ProcessStage::AiContentMap,
				usage: None,
			};
			context.progress.publish(ProcessProgress::ItemSkipped {
				item: process_item.clone(),
			});
			skipped_items.push(process_item);
			file_map.insert(item.relative_path.clone(), cached_entry.clone());
			continue;
		}

		pending_items.push((
			item.clone(),
			prepared.relative_path.clone(),
			prepared.metadata.prepared_hash.clone(),
			content_str.to_string(),
		));
	}

	let file_metadata = prepared_items
		.iter()
		.map(|prepared| (prepared.item.relative_path.clone(), prepared.metadata.clone()))
		.collect::<BTreeMap<_, _>>();
	let partial_document = ContentMapDocument::new(
		&options.model,
		prompt_version,
		format_utc_timestamp(),
		file_map.clone(),
		BTreeMap::new(),
	)
	.with_file_metadata(file_metadata.clone());
	publish_content_map(context.content_map.as_std_path(), &partial_document)?;

	let semaphore = Arc::new(tokio::sync::Semaphore::new(context.max_concurrency.max(1)));
	let ai_client = select_active_ai_client(&options.model);
	let mut join_set = tokio::task::JoinSet::new();

	for (item, prepared_path, prepared_hash, content) in pending_items {
		let sem = semaphore.clone();
		let client = ai_client.clone();
		let appender = appender.clone();
		let progress = context.progress.clone();
		let content_map_path = context.content_map.clone();

		join_set.spawn(async move {
			let _permit = match sem.acquire_owned().await {
				Ok(permit) => permit,
				Err(_) => return None,
			};

			let prompt = match render_file_prompt(&item.relative_path, &content) {
				Ok(rendered) => rendered,
				Err(err) => {
					let failure = ProcessFailure {
						item: ProcessItem {
							source: item.relative_path.clone(),
							output_path: None,
							stage: ProcessStage::AiContentMap,
							usage: None,
						},
						message: err.to_string(),
					};
					progress.publish(ProcessProgress::ItemFailed {
						failure: failure.clone(),
					});
					return Some(Err(failure));
				}
			};
			let ai_res = client.complete(&prompt).await;

			let result: std::result::Result<(FileMapEntry, Option<genai::chat::Usage>), String> = match ai_res {
				Ok(response) => parse_file_info(&response.content)
					.map(|entry| (entry, response.usage))
					.map_err(|err| err.to_string()),
				Err(err) => Err(err.to_string()),
			};

			match result {
				Ok((entry, usage)) => {
					let _ = appender.append(&JournalRecord::file_ok(&prepared_path, &prepared_hash, entry.clone()));
					let process_item = ProcessItem {
						source: item.relative_path.clone(),
						output_path: Some(content_map_path),
						stage: ProcessStage::AiContentMap,
						usage,
					};
					progress.publish(ProcessProgress::ItemCompleted {
						item: process_item.clone(),
					});
					Some(Ok((item.relative_path, process_item, entry)))
				}
				Err(err_msg) => {
					let _ = appender.append(&JournalRecord::file_failed(&prepared_path, &prepared_hash, &err_msg));
					let failure = ProcessFailure {
						item: ProcessItem {
							source: item.relative_path.clone(),
							output_path: None,
							stage: ProcessStage::AiContentMap,
							usage: None,
						},
						message: err_msg,
					};
					progress.publish(ProcessProgress::ItemFailed {
						failure: failure.clone(),
					});
					Some(Err(failure))
				}
			}
		});
	}

	while let Some(res) = join_set.join_next().await {
		if let Ok(Some(item_outcome)) = res {
			match item_outcome {
				Ok((rel_path, process_item, entry)) => {
					file_map.insert(rel_path, entry);
					completed_items.push(process_item);
				}
				Err(failure) => {
					failures.push(failure);
				}
			}
		}
	}

	completed_items.sort_by(|a, b| a.source.cmp(&b.source));
	skipped_items.sort_by(|a, b| a.source.cmp(&b.source));
	failures.sort_by(|a, b| a.item.source.cmp(&b.item.source));

	let document = ContentMapDocument::new(
		&options.model,
		prompt_version,
		format_utc_timestamp(),
		file_map,
		BTreeMap::new(),
	)
	.with_file_metadata(file_metadata);
	publish_content_map(context.content_map.as_std_path(), &document)?;

	if !options.retain_journal {
		remove_journal(journal_path)?;
	} else if !context.resume && failures.is_empty() {
		appender.empty()?;
	}

	context.progress.publish(ProcessProgress::StageCompleted {
		stage: ProcessStage::AiContentMap,
	});

	Ok(StageOutput {
		artifacts: input,
		completed_items,
		skipped_items,
		failures,
	})
}

// endregion: --- Operations

// region:    --- Support

fn prepare_mapper_artifacts(
	context: &WorkflowContext,
	input: &ArtifactSet,
	to_md: bool,
	previous_metadata: &BTreeMap<String, FileMapMetadata>,
) -> (Vec<PreparedArtifact>, Vec<ProcessFailure>) {
	let target_paths = input
		.items
		.iter()
		.map(|item| mapper_relative_path(item, to_md))
		.collect::<Vec<_>>();
	let mut target_counts = BTreeMap::<String, usize>::new();
	for target_path in &target_paths {
		if let Ok(target_path) = target_path {
			*target_counts.entry(target_path.clone()).or_default() += 1;
		}
	}

	let mut prepared_items = Vec::new();
	let mut failures = Vec::new();
	for (item, target_path) in input.items.iter().zip(target_paths) {
		let relative_path = match target_path {
			Ok(relative_path) => relative_path,
			Err(message) => {
				record_preparation_failure(context, item, message, &mut failures);
				continue;
			}
		};
		if target_counts.get(&relative_path).copied().unwrap_or_default() > 1 {
			record_preparation_failure(
				context,
				item,
				format!("multiple input artifacts resolve to mapper path {relative_path}"),
				&mut failures,
			);
			continue;
		}

		match prepare_mapper_artifact(
			context,
			item,
			&relative_path,
			to_md,
			previous_metadata.get(&item.relative_path),
		) {
			Ok(prepared) => prepared_items.push(prepared),
			Err(message) => record_preparation_failure(context, item, message, &mut failures),
		}
	}

	(prepared_items, failures)
}

fn prepare_mapper_artifact(
	context: &WorkflowContext,
	item: &ArtifactItem,
	prepared_relative_path: &str,
	to_md: bool,
	previous_metadata: Option<&FileMapMetadata>,
) -> std::result::Result<PreparedArtifact, String> {
	let source_path = item.local_path.as_std_path();
	let source_metadata = std::fs::metadata(source_path)
		.map_err(|error| format!("cannot access file {}: {error}", item.local_path))?;
	let source_modified = source_metadata
		.modified()
		.ok()
		.and_then(|modified| modified.duration_since(UNIX_EPOCH).ok())
		.and_then(|duration| u64::try_from(duration.as_nanos()).ok());
	let target_path = context.mapper_output.as_std_path().join(prepared_relative_path);

	if let Some(previous) = previous_metadata
		&& previous.prepared_path == prepared_relative_path
		&& source_modified.is_some()
		&& previous.last_modified_unix_nanos == source_modified
		&& let Ok(contents) = std::fs::read(&target_path)
	{
		let prepared_hash = hash_bytes(&contents);
		if prepared_hash == previous.prepared_hash && !previous.source_hash.is_empty() {
			return Ok(PreparedArtifact {
				item: item.clone(),
				relative_path: prepared_relative_path.to_owned(),
				content: contents,
				metadata: FileMapMetadata {
					last_modified_unix_nanos: source_modified,
					source_hash: previous.source_hash.clone(),
					prepared_path: prepared_relative_path.to_owned(),
					prepared_hash,
				},
			});
		}
	}

	let source_contents = std::fs::read(source_path)
		.map_err(|error| format!("failed to read file {}: {error}", item.local_path))?;
	let source_hash = hash_bytes(&source_contents);

	if let Some(previous) = previous_metadata
		&& previous.prepared_path == prepared_relative_path
		&& previous.source_hash == source_hash
		&& let Ok(contents) = std::fs::read(&target_path)
	{
		let prepared_hash = hash_bytes(&contents);
		if prepared_hash == previous.prepared_hash {
			return Ok(PreparedArtifact {
				item: item.clone(),
				relative_path: prepared_relative_path.to_owned(),
				content: contents,
				metadata: FileMapMetadata {
					last_modified_unix_nanos: source_modified,
					source_hash,
					prepared_path: prepared_relative_path.to_owned(),
					prepared_hash,
				},
			});
		}
	}

	let prepared_contents = if to_md && is_html_item(item.media_type.as_deref(), source_path) {
		let html = std::str::from_utf8(&source_contents)
			.map_err(|error| format!("HTML input {} is not valid UTF-8: {error}", item.local_path))?;
		html_to_markdown(html)
			.map_err(|error| format!("failed to convert HTML to Markdown for {}: {error}", item.local_path))?
			.into_bytes()
	} else {
		source_contents
	};
	write_mapper_copy(&target_path, &prepared_contents)?;
	let prepared_hash = hash_bytes(&prepared_contents);

	Ok(PreparedArtifact {
		item: item.clone(),
		relative_path: prepared_relative_path.to_owned(),
		content: prepared_contents,
		metadata: FileMapMetadata {
			last_modified_unix_nanos: source_modified,
			source_hash,
			prepared_path: prepared_relative_path.to_owned(),
			prepared_hash,
		},
	})
}

fn mapper_relative_path(item: &ArtifactItem, to_md: bool) -> std::result::Result<String, String> {
	let normalized_path = item.relative_path.replace('\\', "/");
	let normalized_path = normalized_path.strip_prefix("./").unwrap_or(&normalized_path);
	let path = Path::new(normalized_path);
	if path.is_absolute()
		|| path.components().any(|component| {
			matches!(
				component,
				Component::ParentDir | Component::RootDir | Component::Prefix(_)
			)
		})
	{
		return Err(format!("invalid mapper input path: {}", item.relative_path));
	}

	let mut prepared_path = PathBuf::from(normalized_path);
	if to_md && is_html_item(item.media_type.as_deref(), path) {
		prepared_path.set_extension("md");
	}
	let prepared_path = prepared_path
		.to_str()
		.ok_or_else(|| format!("mapper input path is not valid UTF-8: {}", item.relative_path))?
		.replace('\\', "/");
	if prepared_path.is_empty() || prepared_path == "." {
		return Err(format!("invalid mapper input path: {}", item.relative_path));
	}

	Ok(prepared_path)
}

fn write_mapper_copy(path: &Path, contents: &[u8]) -> std::result::Result<(), String> {
	let parent = path
		.parent()
		.ok_or_else(|| format!("mapper output path has no parent: {}", path.display()))?;
	std::fs::create_dir_all(parent)
		.map_err(|error| format!("failed to create mapper directory {}: {error}", parent.display()))?;
	let file_name = path
		.file_name()
		.ok_or_else(|| format!("mapper output path has no file name: {}", path.display()))?;
	let temporary_path = parent.join(format!("{}.tmp", file_name.to_string_lossy()));
	std::fs::write(&temporary_path, contents)
		.map_err(|error| format!("failed to write mapper copy {}: {error}", temporary_path.display()))?;
	std::fs::rename(&temporary_path, path)
		.map_err(|error| format!("failed to replace mapper copy {}: {error}", path.display()))?;
	Ok(())
}

fn load_previous_file_metadata(path: &Path) -> BTreeMap<String, FileMapMetadata> {
	let Ok(contents) = std::fs::read(path) else {
		return BTreeMap::new();
	};
	let Ok(document) = serde_json::from_slice::<ContentMapDocument>(&contents) else {
		return BTreeMap::new();
	};
	document.file_metadata
}

fn record_preparation_failure(
	context: &WorkflowContext,
	item: &ArtifactItem,
	message: String,
	failures: &mut Vec<ProcessFailure>,
) {
	let failure = ProcessFailure {
		item: ProcessItem {
			source: item.relative_path.clone(),
			output_path: None,
			stage: ProcessStage::AiContentMap,
			usage: None,
		},
		message,
	};
	context.progress.publish(ProcessProgress::ItemFailed {
		failure: failure.clone(),
	});
	failures.push(failure);
}

fn format_utc_timestamp() -> String {
	let secs = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
	let sec = secs % 60;
	let mins = secs / 60;
	let min = mins % 60;
	let hours = mins / 60;
	let hour = hours % 24;
	let days = (hours / 24) as i64;

	let z = days + 719468;
	let era = if z >= 0 { z } else { z - 146096 } / 146097;
	let doe = (z - era * 146097) as u64;
	let yoe = (doe - doe / 1024 + doe / 1461 - doe / 142400) / 365;
	let y = yoe as i64 + era * 400;
	let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
	let mp = (5 * doy + 2) / 153;
	let d = doy - (153 * mp + 2) / 5 + 1;
	let m = if mp < 10 { mp + 3 } else { mp - 9 };
	let y = if m <= 2 { y + 1 } else { y };

	format!("{y:04}-{m:02}-{d:02}T{hour:02}:{min:02}:{sec:02}Z")
}

// endregion: --- Support
