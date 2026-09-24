use super::sanitizr_prompt::{parse_sanitized_content, render_sanitize_prompt, resolve_instructions};
use crate::mapr::{is_text_mappable, select_active_ai_client};
use crate::process::pipeline::{ArtifactItem, ArtifactSet, StageOutput, WorkflowContext};
use crate::process::{ItemId, ProcessStage, SanitizePrompt};
use crate::support::hash_bytes;
use crate::{Error, Result};
use serde::{Deserialize, Serialize};
use simple_fs::SPath;
use std::collections::HashMap;
use std::path::Path;

// region:    --- Types

#[derive(Debug, Clone)]
pub(crate) struct SanitizeConfig {
	pub(crate) model: String,
	pub(crate) prompt: Option<SanitizePrompt>,
	pub(crate) max_size: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SanitizeManifest {
	version: u32,
	model: String,
	prompt_hash: String,
	items: Vec<SanitizeManifestItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SanitizeManifestItem {
	relative_path: String,
	input_hash: String,
	output_hash: String,
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
	let prompt_hash = hash_bytes(instructions.as_bytes());
	let prior_manifest = load_sanitize_manifest(context, &config.model, &prompt_hash);
	let mut artifacts = Vec::new();
	let mut pending_items = Vec::new();
	let mut manifest_items = Vec::new();

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
			if let Some(output_hash) = reusable_output_hash(context, &item.relative_path, &input_hash, &prior_manifest)
			{
				let output_path = context.sanitize_output.join(item.relative_path.as_str());
				context
					.progress
					.item_reused(id, ProcessStage::Sanitize, Some(output_path.clone()));
				manifest_items.push(SanitizeManifestItem {
					relative_path: item.relative_path.clone(),
					input_hash,
					output_hash: output_hash.clone(),
				});
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
			manifest_items.push(SanitizeManifestItem {
				relative_path: item.relative_path.clone(),
				input_hash,
				output_hash: output_hash.clone(),
			});
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

	let semaphore = std::sync::Arc::new(tokio::sync::Semaphore::new(context.max_concurrency.max(1)));
	let ai_client = select_active_ai_client(&config.model);
	let mut join_set = tokio::task::JoinSet::new();

	for (item, id, content, input_hash) in pending_items {
		let semaphore = semaphore.clone();
		let ai_client = ai_client.clone();
		let progress = context.progress.clone();
		let output_root = context.sanitize_output.clone();
		let instructions = instructions.clone();

		join_set.spawn(async move {
			let _permit = match semaphore.acquire_owned().await {
				Ok(permit) => permit,
				Err(error) => {
					let message = format!("failed to acquire Sanitize processing permit: {error}");
					progress.item_failed(id, ProcessStage::Sanitize, message.clone());
					return Err(message);
				}
			};
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
					progress.item_completed(id, ProcessStage::Sanitize, Some(output_path.clone()), usage.clone());
					Ok((
						ArtifactItem {
							source: item.source,
							relative_path: item.relative_path.clone(),
							local_path: output_path,
							media_type: item.media_type,
							source_hash: Some(output_hash.clone()),
						},
						SanitizeManifestItem {
							relative_path: item.relative_path.clone(),
							input_hash,
							output_hash,
						},
					))
				}
				Err(message) => {
					progress.item_failed(id, ProcessStage::Sanitize, message.clone());
					Err(message)
				}
			}
		});
	}

	while let Some(result) = join_set.join_next().await {
		if let Ok((artifact, manifest_item)) =
			result.map_err(|error| Error::TaskJoin(format!("Sanitize task failed: {error}")))?
		{
			artifacts.push(artifact);
			manifest_items.push(manifest_item);
		}
	}

	artifacts.sort_by(|left, right| left.relative_path.cmp(&right.relative_path));
	manifest_items.sort_by(|left, right| left.relative_path.cmp(&right.relative_path));

	let manifest = SanitizeManifest {
		version: 1,
		model: config.model.clone(),
		prompt_hash,
		items: manifest_items,
	};
	let manifest_json = serde_json::to_string_pretty(&manifest)
		.map_err(|error| Error::MalformedState(format!("failed to serialize Sanitize manifest: {error}")))?;
	write_sanitize_artifact(&context.sanitize_manifest, format!("{manifest_json}\n").as_bytes())?;

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

fn load_sanitize_manifest(
	context: &WorkflowContext,
	model: &str,
	prompt_hash: &str,
) -> HashMap<String, SanitizeManifestItem> {
	if !context.resume {
		return HashMap::new();
	}

	let Ok(contents) = std::fs::read(context.sanitize_manifest.as_std_path()) else {
		return HashMap::new();
	};
	let Ok(manifest) = serde_json::from_slice::<SanitizeManifest>(&contents) else {
		return HashMap::new();
	};

	if manifest.version != 1 || manifest.model != model || manifest.prompt_hash != prompt_hash {
		return HashMap::new();
	}

	manifest
		.items
		.into_iter()
		.map(|item| (item.relative_path.clone(), item))
		.collect()
}

fn reusable_output_hash(
	context: &WorkflowContext,
	relative_path: &str,
	input_hash: &str,
	manifest: &HashMap<String, SanitizeManifestItem>,
) -> Option<String> {
	let item = manifest.get(relative_path)?;
	if item.input_hash != input_hash {
		return None;
	}

	let output_path = context.sanitize_output.join(relative_path);
	let contents = std::fs::read(output_path.as_std_path()).ok()?;
	let output_hash = hash_bytes(&contents);
	(output_hash == item.output_hash).then_some(output_hash)
}

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
