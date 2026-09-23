use crate::mapr::{
	ContentMapDocument, FileMapEntry, JournalHeader, JournalRecord, PROMPT_VERSION, hash_file_bytes, html_to_markdown,
	init_or_load_journal, is_html_item, is_text_mappable, parse_file_info, publish_content_map, remove_journal,
	render_file_prompt, select_active_ai_client,
};
use crate::process::pipeline::{ArtifactItem, ArtifactSet, StageOutput, WorkflowContext};
use crate::process::{ContentMapOptions, ProcessFailure, ProcessItem, ProcessProgress, ProcessStage};
use crate::{Error, Result};
use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

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
	let header = JournalHeader::new(&options.model, prompt_version, input.root.as_str());
	let (reuse_index, appender) = init_or_load_journal(journal_path, &header)?;

	let mut completed_items = Vec::new();
	let mut skipped_items = Vec::new();
	let mut failures = Vec::new();
	let mut file_map = BTreeMap::new();

	let mut pending_items = Vec::new();

	for item in &input.items {
		if !is_text_mappable(item.media_type.as_deref(), item.local_path.as_std_path()) {
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

		let file_len = match std::fs::metadata(item.local_path.as_std_path()) {
			Ok(meta) => meta.len() as usize,
			Err(err) => {
				let failure = ProcessFailure {
					item: ProcessItem {
						source: item.relative_path.clone(),
						output_path: None,
						stage: ProcessStage::AiContentMap,
						usage: None,
					},
					message: format!("cannot access file {}: {err}", item.local_path),
				};
				context.progress.publish(ProcessProgress::ItemFailed {
					failure: failure.clone(),
				});
				failures.push(failure);
				continue;
			}
		};

		if let Some(max_size) = options.max_size
			&& file_len > max_size
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

		let bytes = match std::fs::read(item.local_path.as_std_path()) {
			Ok(bytes) => bytes,
			Err(err) => {
				let failure = ProcessFailure {
					item: ProcessItem {
						source: item.relative_path.clone(),
						output_path: None,
						stage: ProcessStage::AiContentMap,
						usage: None,
					},
					message: format!("failed to read file {}: {err}", item.local_path),
				};
				context.progress.publish(ProcessProgress::ItemFailed {
					failure: failure.clone(),
				});
				failures.push(failure);
				continue;
			}
		};

		let content_str = match std::str::from_utf8(&bytes) {
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

		let source_hash = hash_file_bytes(&bytes);

		if options.reuse_unchanged_records
			&& let Some(cached_entry) = reuse_index.get_file(&item.relative_path, &source_hash)
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

		let prompt_content = if options.to_md.unwrap_or(true)
			&& is_html_item(item.media_type.as_deref(), item.local_path.as_std_path())
		{
			match html_to_markdown(content_str) {
				Ok(markdown) => markdown,
				Err(err) => {
					let failure = ProcessFailure {
						item: ProcessItem {
							source: item.relative_path.clone(),
							output_path: None,
							stage: ProcessStage::AiContentMap,
							usage: None,
						},
						message: format!("failed to convert HTML to Markdown for {}: {err}", item.local_path),
					};
					context.progress.publish(ProcessProgress::ItemFailed {
						failure: failure.clone(),
					});
					failures.push(failure);
					continue;
				}
			}
		} else {
			content_str.to_string()
		};

		pending_items.push((item.clone(), source_hash, prompt_content));
	}

	let semaphore = Arc::new(tokio::sync::Semaphore::new(context.max_concurrency.max(1)));
	let ai_client = select_active_ai_client(&options.model);
	let mut join_set = tokio::task::JoinSet::new();

	for (item, source_hash, content) in pending_items {
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
					let _ = appender.append(&JournalRecord::file_ok(
						&item.relative_path,
						&source_hash,
						entry.clone(),
					));
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
					let _ = appender.append(&JournalRecord::file_failed(&item.relative_path, &source_hash, &err_msg));
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
	);
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
