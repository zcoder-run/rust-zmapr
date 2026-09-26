use super::sanitizr_journal::{HeaderInfo, SanitizeJournal};
use super::sanitizr_prompt::{parse_sanitized_content, render_sanitize_prompt, resolve_instructions};
use crate::mapr::{is_text_mappable, select_active_ai_client};
use crate::process::pipeline::{ArtifactItem, ArtifactSet, StageOutput, WorkflowContext};
use crate::process::{ItemId, ProcessStage, SanitizePrompt};
use crate::support::{hash_bytes, run_bounded};
use crate::{Error, Result};
use simple_fs::SPath;
use std::path::Path;

// region:    --- Types

#[derive(Debug, Clone)]
pub(crate) struct SanitizeConfig {
	pub(crate) model: String,
	pub(crate) prompt: Option<SanitizePrompt>,
	pub(crate) max_size: usize,
}

// endregion: --- Types

// region:    --- Operations

pub(crate) async fn execute_sanitize(
	context: &WorkflowContext,
	input: ArtifactSet,
	config: &SanitizeConfig,
) -> Result<StageOutput> {
	context.progress.stage_started(ProcessStage::Sanitize);
	let item_ids = context.progress.register_stage_items(
		ProcessStage::Sanitize,
		input
			.items
			.iter()
			.map(|item| (item.source.clone(), item.relative_path.clone()))
			.collect(),
	);
	context.progress.set_stage_total(ProcessStage::Sanitize);

	let instructions = resolve_instructions(config.prompt.as_ref())?;
	let instruction_hash = hash_bytes(instructions.as_bytes());
	let (journal, journal_report) = SanitizeJournal::open(
		&context.sanitize_journal,
		HeaderInfo {
			model: config.model.clone(),
			instruction_hash,
			input_root: input.root.as_str().to_owned(),
		},
		context.resume,
	)?;
	for line in journal_report.malformed_lines {
		context
			.progress
			.record_journal_warning(ProcessStage::Sanitize, format!("malformed record on line {line}"));
	}
	let mut artifacts = Vec::new();
	let mut pending_items = Vec::new();

	for (item, id) in input.items.into_iter().zip(item_ids) {
		let contents = match std::fs::read(item.local_path.as_std_path()) {
			Ok(contents) => contents,
			Err(error) => {
				record_failure(
					context,
					id,
					item.relative_path,
					format!("failed to read file {}: {error}", item.local_path),
				);
				continue;
			}
		};

		let input_hash = hash_bytes(&contents);

		if is_text_mappable(item.media_type.as_deref(), Path::new(&item.relative_path))
			&& contents.len() <= config.max_size
			&& let Ok(content) = std::str::from_utf8(&contents)
		{
			let output_path = context.sanitize_output.join(item.relative_path.as_str());
			if journal.reusable(&item.relative_path, &input_hash, output_path.as_std_path()) {
				let output_hash = hash_bytes(&std::fs::read(output_path.as_std_path())?);
				context
					.progress
					.item_reused(id, ProcessStage::Sanitize, Some(output_path.clone()));
				artifacts.push(ArtifactItem {
					source: item.source,
					relative_path: item.relative_path.clone(),
					local_path: output_path,
					media_type: item.media_type,
					source_hash: Some(output_hash),
				});
			} else {
				pending_items.push((item, id, content.to_owned(), input_hash));
			}
		} else {
			let output_path = context.sanitize_output.join(item.relative_path.as_str());
			if let Err(error) = write_sanitize_artifact(&output_path, &contents) {
				record_failure(context, id, item.relative_path, error.to_string());
				continue;
			}

			let output_hash = hash_bytes(&contents);
			context
				.progress
				.item_skipped(id, ProcessStage::Sanitize, Some(output_path.clone()));
			artifacts.push(ArtifactItem {
				source: item.source,
				relative_path: item.relative_path,
				local_path: output_path,
				media_type: item.media_type,
				source_hash: Some(output_hash),
			});
		}
	}

	let ai_client = select_active_ai_client(&config.model);
	let tasks = pending_items.into_iter().map(|(item, id, content, input_hash)| {
		let ai_client = ai_client.clone();
		let journal = journal.clone();
		let progress = context.progress.clone();
		let output_root = context.sanitize_output.clone();
		let instructions = instructions.clone();

		async move {
			progress.item_running(id, ProcessStage::Sanitize);

			let prompt = render_sanitize_prompt(&instructions, &item.relative_path, &content);
			let output_path = output_root.join(item.relative_path.as_str());
			let result: std::result::Result<(Vec<u8>, Option<genai::chat::Usage>), String> = async {
				let response = ai_client.complete(&prompt).await.map_err(|error| error.to_string())?;
				let sanitized_content =
					parse_sanitized_content(&response.content).map_err(|error| error.to_string())?;
				let output_bytes = sanitized_content.into_bytes();
				write_sanitize_artifact(&output_path, &output_bytes).map_err(|error| error.to_string())?;
				Ok((output_bytes, response.usage))
			}
			.await;

			match result {
				Ok((output_bytes, usage)) => {
					let output_hash = hash_bytes(&output_bytes);
					if let Err(error) = journal.record_done(&item.relative_path, &input_hash, &output_hash) {
						progress.record_journal_error(ProcessStage::Sanitize, &item.relative_path, error);
					}
					progress.item_completed(id, ProcessStage::Sanitize, Some(output_path.clone()), usage.clone());
					Ok(ArtifactItem {
						source: item.source,
						relative_path: item.relative_path.clone(),
						local_path: output_path,
						media_type: item.media_type,
						source_hash: Some(output_hash.clone()),
					})
				}
				Err(message) => {
					if let Err(error) = journal.record_failed(&item.relative_path, &message) {
						progress.record_journal_error(ProcessStage::Sanitize, &item.relative_path, error);
					}
					progress.item_failed(id, ProcessStage::Sanitize, message.clone());
					Err(message)
				}
			}
		}
	});

	let results = run_bounded(tasks, context.concurrency, "Sanitize").await?;
	for artifact in results.into_iter().flatten() {
		artifacts.push(artifact);
	}

	artifacts.sort_by(|left, right| left.relative_path.cmp(&right.relative_path));

	context.progress.stage_completed(ProcessStage::Sanitize);

	Ok(StageOutput {
		artifacts: ArtifactSet {
			root: context.sanitize_output.clone(),
			items: artifacts,
		},
	})
}

// endregion: --- Operations

// region:    --- Support

fn record_failure(context: &WorkflowContext, id: ItemId, source: String, message: String) {
	let _ = source;
	context.progress.item_failed(id, ProcessStage::Sanitize, message);
}

fn write_sanitize_artifact(path: &SPath, contents: &[u8]) -> Result<()> {
	let target_path: &Path = path.as_ref();
	let parent = target_path
		.parent()
		.ok_or_else(|| Error::MalformedState(format!("Sanitize path has no parent: {path}")))?;

	std::fs::create_dir_all(parent).map_err(|error| {
		Error::MalformedState(format!(
			"failed to create Sanitize output directory {}: {error}",
			parent.display()
		))
	})?;

	let file_name = target_path
		.file_name()
		.and_then(|name| name.to_str())
		.ok_or_else(|| Error::MalformedState(format!("Sanitize path has no valid file name: {path}")))?;
	let temporary_path = parent.join(format!("{file_name}.tmp"));

	std::fs::write(&temporary_path, contents).map_err(|error| {
		Error::MalformedState(format!(
			"failed to write temporary Sanitize artifact {}: {error}",
			temporary_path.display()
		))
	})?;

	std::fs::rename(&temporary_path, target_path)
		.map_err(|error| Error::MalformedState(format!("failed to replace Sanitize artifact {path}: {error}")))?;

	Ok(())
}

// endregion: --- Support
