use std::future::Future;
use std::pin::Pin;

use super::{ProcessContentOptions, ProcessFailure, ProcessItem, ProcessStage};
use crate::{Error, Result};
use simple_fs::SPath;

// region:    --- Types

#[derive(Debug, Clone)]
pub(crate) struct WorkflowContext {
	pub(crate) destination: SPath,
	pub(crate) fetch_cache: SPath,
	pub(crate) sanitize_output: SPath,
	pub(crate) ai_augment_output: SPath,
	pub(crate) manifest: SPath,
	pub(crate) journal: SPath,
	pub(crate) content_map: SPath,
	pub(crate) max_concurrency: usize,
	pub(crate) resume: bool,
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
	pub(crate) completed_items: Vec<ProcessItem>,
	pub(crate) skipped_items: Vec<ProcessItem>,
	pub(crate) failures: Vec<ProcessFailure>,
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
		Self {
			artifacts,
			completed_items: Vec::new(),
			skipped_items: Vec::new(),
			failures: Vec::new(),
		}
	}
}

// endregion: --- Types

// region:    --- Processing Stage

type StageFuture<'a> = Pin<Box<dyn Future<Output = Result<StageOutput>> + Send + 'a>>;

trait ProcessingStage {
	fn execute<'a>(
		&'a self,
		context: &'a WorkflowContext,
		input: ArtifactSet,
	) -> StageFuture<'a>;
}

struct DeferredStage {
	stage: ProcessStage,
	message: &'static str,
}

impl ProcessingStage for DeferredStage {
	fn execute<'a>(
		&'a self,
		_context: &'a WorkflowContext,
		_input: ArtifactSet,
	) -> StageFuture<'a> {
		Box::pin(async move {
			Err(Error::Unsupported(format!(
				"{:?} execution is not implemented yet: {}",
				self.stage, self.message
			)))
		})
	}
}

// endregion: --- Processing Stage

// region:    --- Pipeline

pub(crate) async fn run_pipeline(
	context: &WorkflowContext,
	options: &ProcessContentOptions,
) -> Result<StageOutput> {
	let mut output = StageOutput::passthrough(ArtifactSet::empty(context.fetch_cache.clone()));

	if options.fetch.is_some() {
		output = execute_deferred_stage(
			context,
			output.artifacts,
			ProcessStage::Fetch,
			"source acquisition is deferred",
		)
		.await?;
	}

	if options.sanitize.is_some() {
		output = execute_deferred_stage(
			context,
			output.artifacts,
			ProcessStage::Sanitize,
			"mechanical content preparation is deferred",
		)
		.await?;
	}

	if options.ai_augment.is_some() {
		output = execute_deferred_stage(
			context,
			output.artifacts,
			ProcessStage::AiAugment,
			"AI augmentation is deferred",
		)
		.await?;
	}

	if options.content_map.is_some() {
		output = execute_deferred_stage(
			context,
			output.artifacts,
			ProcessStage::AiContentMap,
			"AI content-map generation is deferred",
		)
		.await?;
	}

	Ok(output)
}

async fn execute_deferred_stage(
	context: &WorkflowContext,
	input: ArtifactSet,
	stage: ProcessStage,
	message: &'static str,
) -> Result<StageOutput> {
	let stage = DeferredStage { stage, message };
	stage.execute(context, input).await
}

// endregion: --- Pipeline
