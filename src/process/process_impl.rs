use super::pipeline::{ArtifactSet, WorkflowContext, run_pipeline};
use crate::{ContentSource, Error, ProcessContentOptions, ProcessContentResponse, ProcessStage, Result};
use simple_fs::SPath;

pub async fn process_content(
	source: impl Into<ContentSource>,
	options: ProcessContentOptions,
) -> Result<ProcessContentResponse> {
	let source = source.into();
	let layout = validate_request(&source, &options)?;
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
	};
	let _ = source;
	let _ = ArtifactSet::empty(context.fetch_cache.clone());

	match run_pipeline(&context, &options).await {
		Ok(_) => Err(Error::Unsupported("no executable processing stage was selected".into())),
		Err(error) => Err(error),
	}
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
