use super::pipeline::{run_pipeline, StageOutput, WorkflowContext};
use super::progress::{
	new_completion_channel, new_progress_channel, ProcessProgressPublisher,
};
use super::response::{ProcessContentHandle, ProcessContentOutput};
use super::state::{new_process_state, ProcessQuery};
use crate::{ContentSource, Error, ProcessContentOptions, ProcessStage, Result};
use simple_fs::SPath;

pub async fn process_content(
	source: impl Into<ContentSource>,
	options: ProcessContentOptions,
) -> Result<ProcessContentHandle> {
	let source = source.into();
	let layout = validate_request(&source, &options)?;
	let (progress_tx, progress_rx) = new_progress_channel()?;
	let state = new_process_state();
	let query = ProcessQuery::new(state.clone());
	let progress = ProcessProgressPublisher::new(progress_tx, state);
	let context = WorkflowContext {
		destination: layout.destination,
		fetch_cache: layout.fetch_cache,
		sanitize_output: layout.sanitize_output,
		ai_augment_output: layout.ai_augment_output,
		manifest: layout.manifest,
		journal: layout.journal,
		content_map: layout.content_map,
		max_concurrency: options.max_concurrency,
		resume: options.resume,
		progress,
	};
	let (completion_tx, completion_rx) = new_completion_channel();
	let handle = ProcessContentHandle::new(progress_rx, completion_rx, query);
	let _ = source;
	tokio::spawn(async move {
		let completion = run_pipeline(&context, &options)
			.await
			.map(|output| process_content_output(&context, &options, output));
		let _ = completion_tx.send(completion);
	});

	Ok(handle)
}

// region:    --- Support

struct WorkflowLayout {
	destination: SPath,
	fetch_cache: SPath,
	sanitize_output: SPath,
	ai_augment_output: SPath,
	manifest: SPath,
	journal: SPath,
	content_map: SPath,
}

fn process_content_output(
	context: &WorkflowContext,
	options: &ProcessContentOptions,
	output: StageOutput,
) -> ProcessContentOutput {
	ProcessContentOutput {
		destination: context.destination.clone(),
		manifest_path: context.manifest.is_file().then(|| context.manifest.clone()),
		content_root: output.artifacts.root,
		content_map_path: (options.content_map.is_some() && context.content_map.is_file())
			.then(|| context.content_map.clone()),
		completed_items: output.completed_items,
		skipped_items: output.skipped_items,
		failures: output.failures,
	}
}

fn validate_request(source: &ContentSource, options: &ProcessContentOptions) -> Result<WorkflowLayout> {
	if options.fetch.is_none()
		&& options.sanitize.is_none()
		&& options.ai_augment.is_none()
		&& options.content_map.is_none()
	{
		return Err(Error::InvalidConfiguration(
			"at least one processing stage must be enabled".into(),
		));
	}

	if options.max_concurrency == 0 {
		return Err(Error::InvalidConfiguration(
			"max_concurrency must be greater than zero".into(),
		));
	}

	if let Some(fetch) = &options.fetch
		&& let ContentSource::Website(_) = source
		&& !fetch.same_host_only
	{
		return Err(Error::InvalidConfiguration(
			"website Fetch requires same_host_only to be enabled".into(),
		));
	}

	if let Some(ai_augment) = &options.ai_augment {
		validate_ai_configuration(ProcessStage::AiAugment, &ai_augment.provider, &ai_augment.model)?;
	}

	if let Some(content_map) = &options.content_map {
		validate_ai_configuration(ProcessStage::AiContentMap, &content_map.provider, &content_map.model)?;
	}

	let layout = resolve_layout(options);

	if options.fetch.is_none() && (options.sanitize.is_some() || options.ai_augment.is_some()) {
		if !layout.fetch_cache.is_dir() {
			return Err(Error::InvalidCache(format!(
				"Fetch cache does not exist: {}",
				layout.fetch_cache
			)));
		}

		if !layout.manifest.is_file() {
			return Err(Error::MalformedState(format!(
				"Fetch manifest does not exist: {}",
				layout.manifest
			)));
		}
	}

	if let ContentSource::Website(_) = source {
		if options.fetch.is_none() {
			return Err(Error::InvalidConfiguration(
				"website sources require Fetch to be enabled".into(),
			));
		}

		return Err(Error::Unsupported(
			"website Fetch execution is not implemented yet".into(),
		));
	}

	Ok(layout)
}

fn validate_ai_configuration(stage: ProcessStage, provider: &str, model: &str) -> Result<()> {
	if provider.trim().is_empty() || model.trim().is_empty() {
		return Err(Error::InvalidConfiguration(format!(
			"{stage:?} requires a nonempty provider and model"
		)));
	}

	Ok(())
}

fn resolve_layout(options: &ProcessContentOptions) -> WorkflowLayout {
	let destination = options.destination.clone();
	let metadata_root = destination.join(".zmapr");

	WorkflowLayout {
		destination,
		fetch_cache: metadata_root.join("fetch"),
		sanitize_output: metadata_root.join("stages/sanitize"),
		ai_augment_output: metadata_root.join("stages/ai-augment"),
		manifest: metadata_root.join("manifest.json"),
		journal: metadata_root.join("content-map.journal.jsonl"),
		content_map: options.destination.join("content-map.json"),
	}
}

// endregion: --- Support
