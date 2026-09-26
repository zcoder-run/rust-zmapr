use super::pipeline::{StageOutput, WorkflowContext, build_fetch_request, run_pipeline};
use super::publish::publish_final_artifacts;
use super::progress::{ProcessProgressPublisher, new_completion_channel, new_progress_channel};
use super::response::{ProcessContentHandle, ProcessContentOutput};
use super::state::{ProcessQuery, StageSelection, new_process_state};
use crate::fetchr::{FetchRequest, validate_source, validate_web_source};
use crate::sanitizr::sanitize_journal_path;
use crate::{ContentSource, Error, ProcessContentOptions, ProcessStage, Result, SanitizePrompt};
use simple_fs::SPath;
use std::path::{Component, Path, PathBuf};

#[doc = include_str!("../../docs/rustdoc/process/process-content.md")]
pub async fn process_content(options: ProcessContentOptions) -> Result<ProcessContentHandle> {
	let fetch_request = build_fetch_request(&options);
	let layout = validate_request(&options, fetch_request.as_ref())?;
	let source = resolve_source(&options);
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
		sanitize_journal: layout.sanitize_journal,
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
	sanitize_journal: SPath,
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
		journal_errors: context.progress.journal_errors(),
	})
}

fn validate_request(options: &ProcessContentOptions, fetch_request: Option<&FetchRequest>) -> Result<WorkflowLayout> {
	if !options.fetch && !options.sanitize && !options.map {
		return Err(Error::InvalidConfiguration(
			"at least one processing stage must be enabled".into(),
		));
	}

	let destination = options.resolved_destination();

	if options.concurrency == 0 {
		return Err(Error::InvalidConfiguration(
			"concurrency must be greater than zero".into(),
		));
	}

	if let Some(fetch) = fetch_request {
		match fetch {
			FetchRequest::Local(local_request) => {
				validate_source(&local_request.source)?;
			}
			FetchRequest::Web(web_request) => {
				validate_web_source(&web_request.source)?;
			}
		}
	}

	if !options.source.starts_with("http://") && !options.source.starts_with("https://") {
		let source_path = Path::new(&options.source);
		if source_path.is_dir() {
			validate_destination_outside_source(source_path, &destination)?;
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

	let layout = resolve_layout(options)?;

	if !options.fetch
		&& (options.sanitize || options.map)
		&& !layout.fetch_cache.is_dir()
		&& !layout.manifest.is_file()
	{
		return Err(Error::MalformedState(format!(
			"Fetch manifest does not exist: {}",
			layout.manifest
		)));
	}

	if !options.fetch && (options.sanitize || options.map) {
		validate_prior_manifest_source(&layout.manifest, &options.source)?;
	}

	Ok(layout)
}

fn validate_destination_outside_source(source_path: &Path, destination: &SPath) -> Result<()> {
	let source_path = std::fs::canonicalize(source_path).map_err(|error| {
		Error::InvalidConfiguration(format!("failed to resolve source directory {}: {error}", source_path.display()))
	})?;
	let destination_path = canonical_destination_path(destination.as_std_path())?;

	if destination_path == source_path || destination_path.starts_with(&source_path) {
		return Err(Error::InvalidConfiguration(
			"destination must not be equal to or inside the local source directory".into(),
		));
	}

	Ok(())
}

fn canonical_destination_path(path: &Path) -> Result<PathBuf> {
	let absolute_path = if path.is_absolute() {
		path.to_path_buf()
	} else {
		std::env::current_dir()
			.map_err(|error| Error::InvalidConfiguration(format!("failed to resolve current directory: {error}")))?
			.join(path)
	};
	let absolute_path = normalize_path(&absolute_path);

	if absolute_path.exists() {
		return std::fs::canonicalize(&absolute_path).map_err(|error| {
			Error::InvalidConfiguration(format!("failed to resolve destination directory {}: {error}", path.display()))
		});
	}

	let mut existing_path = absolute_path.as_path();
	let mut missing = Vec::new();
	while !existing_path.exists() {
		let Some(file_name) = existing_path.file_name() else {
			break;
		};
		missing.push(file_name.to_os_string());
		let Some(parent) = existing_path.parent() else {
			break;
		};
		existing_path = parent;
	}

	let mut destination_path = std::fs::canonicalize(existing_path).map_err(|error| {
		Error::InvalidConfiguration(format!("failed to resolve destination directory {}: {error}", path.display()))
	})?;
	for component in missing.into_iter().rev() {
		destination_path.push(component);
	}

	Ok(normalize_path(&destination_path))
}

fn normalize_path(path: &Path) -> PathBuf {
	let mut normalized = PathBuf::new();
	for component in path.components() {
		match component {
			Component::CurDir => {}
			Component::ParentDir => {
				if !normalized.pop() {
					normalized.push("..");
				}
			}
			component => normalized.push(component.as_os_str()),
		}
	}

	normalized
}

fn resolve_source(options: &ProcessContentOptions) -> Option<ContentSource> {
	let source = options.source.as_str();
	if source.starts_with("http://") || source.starts_with("https://") {
		Some(ContentSource::web(source))
	} else {
		Some(ContentSource::local(source))
	}
}

fn validate_prior_manifest_source(manifest_path: &SPath, configured_source: &str) -> Result<()> {
	let content = simple_fs::read_to_string(manifest_path)
		.map_err(|error| Error::MalformedState(format!("failed to read Fetch manifest {manifest_path}: {error}")))?;
	let manifest: serde_json::Value = serde_json::from_str(&content)
		.map_err(|error| Error::MalformedState(format!("failed to deserialize Fetch manifest {manifest_path}: {error}")))?;
	let manifest_source = manifest
		.get("source")
		.and_then(serde_json::Value::as_str)
		.ok_or_else(|| Error::MalformedState("Fetch manifest is missing source metadata".to_owned()))?;

	if manifest_source != configured_source {
		return Err(Error::InvalidConfiguration(
			"configured source does not match the Fetch manifest source".to_owned(),
		));
	}

	Ok(())
}

fn validate_model(stage: ProcessStage, model: Option<&str>) -> Result<()> {
	if model.is_none_or(|model| model.trim().is_empty()) {
		return Err(Error::InvalidConfiguration(format!(
			"{stage:?} requires a nonempty model"
		)));
	}

	Ok(())
}

fn resolve_layout(options: &ProcessContentOptions) -> Result<WorkflowLayout> {
	let destination = options.resolved_destination();
	let metadata_root = destination.join(".tmp-zmapr");
	let content_map = destination.join("_content-map.json");

	Ok(WorkflowLayout {
		destination,
		fetch_cache: metadata_root.join("01-fetch"),
		sanitize_output: metadata_root.join("02-sanitize"),
		sanitize_journal: SPath::from(
			sanitize_journal_path(metadata_root.as_std_path().join("manifest.json"))
				.to_string_lossy()
				.into_owned(),
		),
		manifest: metadata_root.join("manifest.json"),
		journal: metadata_root.join("content-map.journal.jsonl"),
		content_map,
	})
}

// endregion: --- Support
