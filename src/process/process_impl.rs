use super::pipeline::{StageOutput, WorkflowContext, build_fetch_request, run_pipeline};
use super::publish::publish_final_artifacts;
use super::progress::{ProcessProgressPublisher, new_completion_channel, new_progress_channel};
use super::response::{ProcessContentHandle, ProcessContentOutput};
use super::state::{ProcessQuery, StageSelection, new_process_state};
use crate::fetchr::{FetchRequest, validate_source, validate_web_source};
use crate::{ContentSource, Error, ProcessContentOptions, ProcessStage, Result, SanitizePrompt};
use simple_fs::SPath;

#[doc = include_str!("../../docs/rustdoc/process/process-content.md")]
pub async fn process_content(options: ProcessContentOptions) -> Result<ProcessContentHandle> {
	let fetch_request = build_fetch_request(&options);
	let layout = validate_request(&options, fetch_request.as_ref())?;
	let source = resolve_source(&options, fetch_request.as_ref(), &layout);
	let (progress_tx, progress_rx) = new_progress_channel()?;
	let state = new_process_state(StageSelection {
		fetch: fetch_request.is_some(),
		sanitize: options.sanitize,
		map: options.map,
	});
	let query = ProcessQuery::new(state.clone());
	let progress = ProcessProgressPublisher::new(progress_tx, state);
	let context = WorkflowContext {
		source,
		destination: layout.destination,
		fetch_cache: layout.fetch_cache,
		sanitize_output: layout.sanitize_output,
		sanitize_manifest: layout.sanitize_manifest,
		manifest: layout.manifest,
		journal: layout.journal,
		content_map: layout.content_map,
		concurrency: options.concurrency,
		resume: options.resume,
		progress,
	};
	let (completion_tx, completion_rx) = new_completion_channel();
	let handle = ProcessContentHandle::new(progress_rx, completion_rx, query);
	tokio::spawn(async move {
		let completion = match run_pipeline(&context, &options, fetch_request.as_ref()).await {
			Ok(output) => match process_content_output(&context, &options, output) {
				Ok(output) => Ok(output),
				Err(error) => {
					context.progress.fail_workflow(&error.to_string());
					Err(error)
				}
			},
			Err(error) => {
				context.progress.fail_workflow(&error.to_string());
				Err(error)
			}
		};
		let _ = completion_tx.send(completion);
	});

	Ok(handle)
}

// region:    --- Support

struct WorkflowLayout {
	destination: SPath,
	fetch_cache: SPath,
	sanitize_output: SPath,
	sanitize_manifest: SPath,
	manifest: SPath,
	journal: SPath,
	content_map: SPath,
}

fn process_content_output(
	context: &WorkflowContext,
	options: &ProcessContentOptions,
	output: StageOutput,
) -> Result<ProcessContentOutput> {
	let _ = publish_final_artifacts(&output.artifacts, &context.destination)?;
	let (stats, items) = context.progress.finish()?;
	Ok(ProcessContentOutput {
		destination: context.destination.clone(),
		manifest_path: context.manifest.is_file().then(|| context.manifest.clone()),
		content_root: context.destination.clone(),
		content_map_path: (options.map && context.content_map.is_file()).then(|| context.content_map.clone()),
		items,
		stats,
	})
}

fn validate_request(options: &ProcessContentOptions, fetch_request: Option<&FetchRequest>) -> Result<WorkflowLayout> {
	if options.source.is_none() && !options.sanitize && !options.map {
		return Err(Error::InvalidConfiguration(
			"at least one processing stage must be enabled".into(),
		));
	}

	if options.concurrency == 0 {
		return Err(Error::InvalidConfiguration(
			"concurrency must be greater than zero".into(),
		));
	}

	if let Some(fetch) = fetch_request {
		match fetch {
			FetchRequest::Local(local_request) => {
				validate_source(&local_request.source)?;
				if local_request.source.path.is_dir() && options.destination.as_std_path().exists() {
					let source_path = std::fs::canonicalize(local_request.source.path.as_std_path()).map_err(|error| {
						Error::InvalidConfiguration(format!("failed to resolve source directory: {error}"))
					})?;
					let destination_path = std::fs::canonicalize(options.destination.as_std_path()).map_err(|error| {
						Error::InvalidConfiguration(format!("failed to resolve destination directory: {error}"))
					})?;
					if source_path == destination_path {
						return Err(Error::InvalidConfiguration(
							"source directory must not be the destination".into(),
						));
					}
				}
			}
			FetchRequest::Web(web_request) => {
				validate_web_source(&web_request.source)?;
			}
		}
	}

	if options.sanitize {
		validate_model(ProcessStage::Sanitize, options.resolved_sanitize_model())?;
		if let Some(prompt) = &options.sanitize_prompt {
			match prompt {
				SanitizePrompt::FilePath(path) if !path.is_file() => {
					return Err(Error::InvalidConfiguration(format!(
						"Sanitize prompt file does not exist: {path}"
					)));
				}
				SanitizePrompt::Content(content) if content.trim().is_empty() => {
					return Err(Error::InvalidConfiguration(
						"Sanitize prompt content must not be empty".to_owned(),
					));
				}
				_ => {}
			}
		}
	}

	if options.map {
		validate_model(ProcessStage::Map, options.resolved_map_model())?;
	}

	let layout = resolve_layout(options);

	if options.source.is_none()
		&& (options.sanitize || options.map)
		&& !layout.fetch_cache.is_dir()
		&& !layout.manifest.is_file()
	{
		return Err(Error::MalformedState(format!(
			"Fetch manifest does not exist: {}",
			layout.manifest
		)));
	}

	Ok(layout)
}

fn resolve_source(
	options: &ProcessContentOptions,
	fetch_request: Option<&FetchRequest>,
	layout: &WorkflowLayout,
) -> Option<ContentSource> {
	match fetch_request {
		Some(FetchRequest::Local(local_request)) => Some(ContentSource::LocalPath(local_request.source.clone())),
		Some(FetchRequest::Web(web_request)) => Some(ContentSource::Web(web_request.source.clone())),
		None if options.source.is_none() => read_prior_manifest_source(&layout.manifest),
		None => None,
	}
}

fn read_prior_manifest_source(manifest_path: &SPath) -> Option<ContentSource> {
	let content = simple_fs::read_to_string(manifest_path).ok()?;
	let manifest: serde_json::Value = serde_json::from_str(&content).ok()?;
	let source_str = manifest.get("source")?.as_str()?;
	if source_str.starts_with("http://") || source_str.starts_with("https://") {
		Some(ContentSource::web(source_str))
	} else {
		Some(ContentSource::local(source_str))
	}
}

fn validate_model(stage: ProcessStage, model: Option<&str>) -> Result<()> {
	if model.is_none_or(|model| model.trim().is_empty()) {
		return Err(Error::InvalidConfiguration(format!(
			"{stage:?} requires a nonempty model"
		)));
	}

	Ok(())
}

fn resolve_layout(options: &ProcessContentOptions) -> WorkflowLayout {
	let destination = options.destination.clone();
	let metadata_root = destination.join(".tmp-zmapr");

	WorkflowLayout {
		destination,
		fetch_cache: metadata_root.join("01-fetch"),
		sanitize_output: metadata_root.join("02-sanitize"),
		sanitize_manifest: metadata_root.join("sanitize-manifest.json"),
		manifest: metadata_root.join("manifest.json"),
		journal: metadata_root.join("content-map.journal.jsonl"),
		content_map: options.destination.join("content-map.json"),
	}
}

// endregion: --- Support
