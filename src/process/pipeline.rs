use super::progress::ProcessProgressPublisher;
use super::source::ContentSource;
use super::{ProcessContentOptions, ProcessStage};
use crate::fetchr::{
	FetchRequest, LocalFetchRequest, WebFetchRequest, execute_http_fetch, execute_local_fetch, load_prior_local_fetch,
};
use crate::mapr::MapConfig;
use crate::sanitizr::{SanitizeConfig, execute_sanitize};
use crate::Result;
use simple_fs::SPath;

// region:    --- Types

#[derive(Debug, Clone)]
pub(crate) struct WorkflowContext {
	pub(crate) source: Option<ContentSource>,
	pub(crate) destination: SPath,
	pub(crate) fetch_cache: SPath,
	pub(crate) sanitize_output: SPath,
	pub(crate) sanitize_manifest: SPath,
	pub(crate) manifest: SPath,
	pub(crate) journal: SPath,
	pub(crate) content_map: SPath,
	pub(crate) max_concurrency: usize,
	pub(crate) resume: bool,
	pub(crate) progress: ProcessProgressPublisher,
}

#[derive(Debug, Clone)]
pub(crate) struct ArtifactSet {
	pub(crate) root: SPath,
	pub(crate) items: Vec<ArtifactItem>,
}

#[derive(Debug, Clone)]
pub(crate) struct ArtifactItem {
	pub(crate) source: String,
	pub(crate) relative_path: String,
	pub(crate) local_path: SPath,
	pub(crate) media_type: Option<String>,
	pub(crate) source_hash: Option<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct StageOutput {
	pub(crate) artifacts: ArtifactSet,
}

impl ArtifactSet {
	pub(crate) fn empty(root: SPath) -> Self {
		Self {
			root,
			items: Vec::new(),
		}
	}
}

impl StageOutput {
	fn passthrough(artifacts: ArtifactSet) -> Self {
		Self { artifacts }
	}
}

// endregion: --- Types

// region:    --- Processing Stage


// endregion: --- Processing Stage

// region:    --- Pipeline

pub(crate) fn build_fetch_request(options: &ProcessContentOptions) -> Option<FetchRequest> {
	let source = options.source.as_deref()?;

	if source.starts_with("http://") || source.starts_with("https://") {
		Some(FetchRequest::Web(
			WebFetchRequest::new(source)
				.with_same_host_only(true)
				.with_follow_links(options.max_depth > 0)
				.with_max_depth(options.max_depth)
				.with_format(options.format)
				.with_llms(options.llms)
				.with_include(options.include.clone())
				.with_exclude(options.exclude.clone()),
		))
	} else {
		Some(FetchRequest::Local(
			LocalFetchRequest::new(source)
				.with_format(options.format)
				.with_include(options.include.clone())
				.with_exclude(options.exclude.clone()),
		))
	}
}

pub(crate) async fn run_pipeline(
	context: &WorkflowContext,
	options: &ProcessContentOptions,
	fetch_request: Option<&FetchRequest>,
) -> Result<StageOutput> {
	let mut output = if let Some(fetch_request) = fetch_request {
		execute_fetch_stage(context, fetch_request).await?
	} else if requires_prior_fetch(options) {
		StageOutput::passthrough(load_prior_local_fetch(context)?)
	} else {
		StageOutput::passthrough(ArtifactSet::empty(context.fetch_cache.clone()))
	};

	if options.sanitize {
		let sanitize_config = SanitizeConfig {
			model: options.resolved_sanitize_model().unwrap_or_default().to_owned(),
			prompt: options.sanitize_prompt.clone(),
			max_size: 200_000,
		};
		let sanitize_output = execute_sanitize(context, output.artifacts, &sanitize_config).await?;
		output.artifacts = sanitize_output.artifacts;
	}

	if options.map {
		let map_config = MapConfig {
			model: options.resolved_map_model().unwrap_or_default().to_owned(),
			max_size: Some(200_000),
			reuse_unchanged_records: options.resume,
			retain_journal: true,
		};
		let map_output = crate::mapr::execute_content_map(context, output.artifacts, &map_config).await?;
		output.artifacts = map_output.artifacts;
	}

	Ok(output)
}

async fn execute_fetch_stage(context: &WorkflowContext, request: &FetchRequest) -> Result<StageOutput> {
	context.progress.stage_started(ProcessStage::Fetch);
	let result = match request {
		FetchRequest::Local(local_request) => execute_local_fetch(local_request, context).await,
		FetchRequest::Web(web_request) => execute_http_fetch(web_request, context).await,
	};

	if result.is_ok() {
		context.progress.stage_completed(ProcessStage::Fetch);
	}

	result
}

fn requires_prior_fetch(options: &ProcessContentOptions) -> bool {
	options.sanitize || options.map
}

// endregion: --- Pipeline
