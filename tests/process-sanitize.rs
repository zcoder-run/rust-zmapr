use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use zmapr::{
	ContentMapDocument, MaprAiClient, MaprAiResponse, MaprAiSelector, ProcessContentOptions,
	ProcessProgress, ProcessStage, SanitizePrompt, process_content, set_active_ai_selector,
};

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

static TEST_MUTEX: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

#[tokio::test]
async fn test_process_sanitize_cleans_text_and_copies_ineligible_items() -> Result<()> {
	let _guard = TEST_MUTEX.lock().await;
	set_active_ai_selector(Some(MaprAiSelector::Stub));

	// -- Setup & Fixtures
	let root = fixture_root("test_process_sanitize_cleans_text_and_copies_ineligible_items")?;
	let source_root = root.join("source");
	fs::create_dir_all(&source_root)?;
	fs::write(source_root.join("intro.md"), b"# Intro\nKeep the substantive content.\n")?;
	fs::write(source_root.join("image.png"), b"image bytes")?;
	let destination = root.join("destination");
	let options = ProcessContentOptions::new(path_text(&destination))
		.with_source(path_text(&source_root))
		.with_sanitize(true)
		.with_model("stub-model");

	// -- Exec
	let mut handle = process_content(options).await?;
	let mut progress_rx = handle.take_progress_rx().ok_or("expected progress receiver")?;
	let progress_task = tokio::spawn(async move {
		let mut events = Vec::new();
		while let Ok(event) = progress_rx.recv().await {
			events.push(event);
		}
		events
	});
	let output = handle.wait_output().await?;
	let progress_events = progress_task.await?;

	// -- Check
	assert!(output.failures.is_empty());
	let sanitize_root = destination.join(".tmp-zmapr").join("02-sanitize");
	assert!(sanitize_root.join("intro.md").is_file());
	assert_eq!(
		fs::read(sanitize_root.join("intro.md"))?,
		b"# Intro\nKeep the substantive content.\n"
	);
	assert_eq!(fs::read(sanitize_root.join("image.png"))?, b"image bytes");

	let completed_item = output
		.completed_items
		.iter()
		.find(|item| item.stage == ProcessStage::Sanitize && item.source == "intro.md")
		.ok_or("expected completed Sanitize item")?;
	assert!(completed_item.usage.is_some());
	assert_eq!(
		completed_item.output_path.as_ref().map(|path| path.as_std_path()),
		Some(sanitize_root.join("intro.md").as_path())
	);

	let skipped_item = output
		.skipped_items
		.iter()
		.find(|item| item.stage == ProcessStage::Sanitize && item.source == "image.png")
		.ok_or("expected skipped image item")?;
	assert_eq!(
		skipped_item.output_path.as_ref().map(|path| path.as_std_path()),
		Some(sanitize_root.join("image.png").as_path())
	);
	assert!(progress_events.iter().any(|event| matches!(
		event,
		ProcessProgress::StageCompleted {
			stage: ProcessStage::Sanitize
		}
	)));

	set_active_ai_selector(None);
	Ok(())
}

#[tokio::test]
async fn test_process_sanitize_map_uses_sanitized_artifacts() -> Result<()> {
	let _guard = TEST_MUTEX.lock().await;
	set_active_ai_selector(Some(MaprAiSelector::Stub));

	// -- Setup & Fixtures
	let root = fixture_root("test_process_sanitize_map_uses_sanitized_artifacts")?;
	let source_root = root.join("source");
	fs::create_dir_all(&source_root)?;
	fs::write(source_root.join("intro.md"), b"# Intro\nSanitize before mapping.")?;
	let destination = root.join("destination");
	let options = ProcessContentOptions::new(path_text(&destination))
		.with_source(path_text(&source_root))
		.with_sanitize(true)
		.with_map(true)
		.with_model("stub-model");

	// -- Exec
	let handle = process_content(options).await?;
	let output = handle.wait_output().await?;

	// -- Check
	assert!(output.failures.is_empty());
	assert_eq!(
		output.content_root.as_std_path(),
		destination.join(".tmp-zmapr").join("02-sanitize").as_path()
	);
	let content_map_path = output.content_map_path.as_ref().ok_or("expected content map")?;
	let document: ContentMapDocument = serde_json::from_slice(&fs::read(content_map_path.as_std_path())?)?;
	assert!(document.file_map.contains_key("intro.md"));
	assert!(destination
		.join(".tmp-zmapr")
		.join("02-sanitize")
		.join("intro.md")
		.is_file());

	set_active_ai_selector(None);
	Ok(())
}

#[tokio::test]
async fn test_process_sanitize_custom_instructions_replace_built_in_prompt() -> Result<()> {
	let _guard = TEST_MUTEX.lock().await;
	set_active_ai_selector(Some(MaprAiSelector::Custom(Arc::new(PromptCheckingAiClient))));

	// -- Setup & Fixtures
	let root = fixture_root("test_process_sanitize_custom_instructions_replace_built_in_prompt")?;
	let source_root = root.join("source");
	fs::create_dir_all(&source_root)?;
	fs::write(source_root.join("guide.md"), b"Customizable content.")?;
	let destination = root.join("destination");
	let options = ProcessContentOptions::new(path_text(&destination))
		.with_source(path_text(&source_root))
		.with_sanitize(true)
		.with_model("custom-model")
		.with_sanitize_prompt(SanitizePrompt::content("CUSTOM RULES"));

	// -- Exec
	let handle = process_content(options).await?;
	let output = handle.wait_output().await?;

	// -- Check
	assert!(output.failures.is_empty());
	assert!(destination
		.join(".tmp-zmapr")
		.join("02-sanitize")
		.join("guide.md")
		.is_file());

	set_active_ai_selector(None);
	Ok(())
}

#[tokio::test]
async fn test_process_sanitize_missing_output_tags_records_failure_and_completes() -> Result<()> {
	let _guard = TEST_MUTEX.lock().await;
	set_active_ai_selector(Some(MaprAiSelector::Custom(Arc::new(MissingSanitizedContentAiClient))));

	// -- Setup & Fixtures
	let root = fixture_root("test_process_sanitize_missing_output_tags_records_failure_and_completes")?;
	let source_root = root.join("source");
	fs::create_dir_all(&source_root)?;
	fs::write(source_root.join("guide.md"), b"Content that fails parsing.")?;
	let destination = root.join("destination");
	let options = ProcessContentOptions::new(path_text(&destination))
		.with_source(path_text(&source_root))
		.with_sanitize(true)
		.with_model("custom-model");

	// -- Exec
	let mut handle = process_content(options).await?;
	let mut progress_rx = handle.take_progress_rx().ok_or("expected progress receiver")?;
	let progress_task = tokio::spawn(async move {
		let mut events = Vec::new();
		while let Ok(event) = progress_rx.recv().await {
			events.push(event);
		}
		events
	});
	let output = handle.wait_output().await?;
	let progress_events = progress_task.await?;

	// -- Check
	assert_eq!(output.failures.len(), 1);
	assert_eq!(output.failures[0].item.stage, ProcessStage::Sanitize);
	assert!(output.failures[0].message.contains("SANITIZED_CONTENT"));
	assert!(progress_events.iter().any(|event| matches!(
		event,
		ProcessProgress::StageCompleted {
			stage: ProcessStage::Sanitize
		}
	)));
	assert!(!destination
		.join(".tmp-zmapr")
		.join("02-sanitize")
		.join("guide.md")
		.exists());

	set_active_ai_selector(None);
	Ok(())
}

#[tokio::test]
async fn test_process_sanitize_resume_reuses_unchanged_items_and_invalidates_changes() -> Result<()> {
	let _guard = TEST_MUTEX.lock().await;
	set_active_ai_selector(Some(MaprAiSelector::Stub));

	// -- Setup & Fixtures
	let root = fixture_root("test_process_sanitize_resume_reuses_unchanged_items_and_invalidates_changes")?;
	let source_root = root.join("source");
	fs::create_dir_all(&source_root)?;
	fs::write(source_root.join("intro.md"), b"# Intro\nOriginal content.")?;
	fs::write(source_root.join("guide.md"), b"# Guide\nUnchanged content.")?;
	let destination = root.join("destination");

	// -- Exec
	let first_handle = process_content(sanitize_options(&destination, &source_root, "stub-model", None)).await?;
	let first_output = first_handle.wait_output().await?;
	let second_handle = process_content(sanitize_options(&destination, &source_root, "stub-model", None)).await?;
	let second_output = second_handle.wait_output().await?;

	// -- Check
	assert_eq!(
		first_output
			.completed_items
			.iter()
			.filter(|item| item.stage == ProcessStage::Sanitize)
			.count(),
		2
	);
	assert_eq!(
		second_output
			.completed_items
			.iter()
			.filter(|item| item.stage == ProcessStage::Sanitize)
			.count(),
		0
	);
	assert_eq!(second_output.total_usage, None);
	assert_eq!(
		second_output
			.skipped_items
			.iter()
			.filter(|item| item.stage == ProcessStage::Sanitize)
			.count(),
		2
	);

	// -- Exec & Check
	fs::write(source_root.join("intro.md"), b"# Intro\nUpdated content.")?;
	let changed_source_handle =
		process_content(sanitize_options(&destination, &source_root, "stub-model", None)).await?;
	let changed_source_output = changed_source_handle.wait_output().await?;
	let completed_sources = changed_source_output
		.completed_items
		.iter()
		.filter(|item| item.stage == ProcessStage::Sanitize)
		.map(|item| item.source.as_str())
		.collect::<Vec<_>>();
	assert_eq!(completed_sources, vec!["intro.md"]);

	let changed_model_handle =
		process_content(sanitize_options(&destination, &source_root, "stub-model-v2", None)).await?;
	let changed_model_output = changed_model_handle.wait_output().await?;
	assert_eq!(
		changed_model_output
			.completed_items
			.iter()
			.filter(|item| item.stage == ProcessStage::Sanitize)
			.count(),
		2
	);

	let custom_prompt = Some(SanitizePrompt::content("CUSTOM RULES"));
	let changed_prompt_handle = process_content(sanitize_options(
		&destination,
		&source_root,
		"stub-model-v2",
		custom_prompt.clone(),
	))
	.await?;
	let changed_prompt_output = changed_prompt_handle.wait_output().await?;
	assert_eq!(
		changed_prompt_output
			.completed_items
			.iter()
			.filter(|item| item.stage == ProcessStage::Sanitize)
			.count(),
		2
	);

	fs::remove_file(
		destination
			.join(".tmp-zmapr")
			.join("02-sanitize")
			.join("intro.md"),
	)?;
	let missing_output_handle = process_content(sanitize_options(
		&destination,
		&source_root,
		"stub-model-v2",
		custom_prompt,
	))
	.await?;
	let missing_output = missing_output_handle.wait_output().await?;
	let completed_sources = missing_output
		.completed_items
		.iter()
		.filter(|item| item.stage == ProcessStage::Sanitize)
		.map(|item| item.source.as_str())
		.collect::<Vec<_>>();
	assert_eq!(completed_sources, vec!["intro.md"]);

	set_active_ai_selector(None);
	Ok(())
}

#[tokio::test]
async fn test_process_sanitize_task_panic_returns_task_join_error() -> Result<()> {
	let _guard = TEST_MUTEX.lock().await;
	set_active_ai_selector(Some(MaprAiSelector::Custom(Arc::new(PanickingSanitizeAiClient))));

	// -- Setup & Fixtures
	let root = fixture_root("test_process_sanitize_task_panic_returns_task_join_error")?;
	let source_root = root.join("source");
	fs::create_dir_all(&source_root)?;
	fs::write(source_root.join("panic.md"), b"# Panic\nTrigger a task panic.")?;
	let destination = root.join("destination");
	let options = ProcessContentOptions::new(path_text(&destination))
		.with_source(path_text(&source_root))
		.with_sanitize(true)
		.with_model("custom-model");

	// -- Exec
	let handle = process_content(options).await?;
	let error = handle.wait_output().await.err().ok_or("expected Sanitize task join error")?;
	set_active_ai_selector(None);

	// -- Check
	assert!(matches!(error, zmapr::Error::TaskJoin(_)));

	Ok(())
}

// region:    --- Support

#[derive(Debug)]
struct PanickingSanitizeAiClient;

impl MaprAiClient for PanickingSanitizeAiClient {
	fn complete<'a>(&'a self, prompt: &'a str) -> zmapr::BoxFuture<'a, zmapr::Result<MaprAiResponse>> {
		Box::pin(async move {
			if prompt.is_empty() {
				Ok(MaprAiResponse::new(""))
			} else {
				panic!("simulated Sanitize task panic")
			}
		})
	}
}

#[derive(Debug)]
struct PromptCheckingAiClient;

impl MaprAiClient for PromptCheckingAiClient {
	fn complete<'a>(&'a self, prompt: &'a str) -> zmapr::BoxFuture<'a, zmapr::Result<MaprAiResponse>> {
		Box::pin(async move {
			if !prompt.contains("CUSTOM RULES") || prompt.contains("Remove navigation") {
				return Err(zmapr::Error::custom("custom Sanitize instructions were not applied"));
			}

			Ok(MaprAiResponse::new(
				"<SANITIZED_CONTENT>\nCustom instruction output.\n</SANITIZED_CONTENT>",
			))
		})
	}
}

#[derive(Debug)]
struct MissingSanitizedContentAiClient;

impl MaprAiClient for MissingSanitizedContentAiClient {
	fn complete<'a>(&'a self, _prompt: &'a str) -> zmapr::BoxFuture<'a, zmapr::Result<MaprAiResponse>> {
		Box::pin(async move { Ok(MaprAiResponse::new("response without the required tags")) })
	}
}

fn fixture_root(test_name: &str) -> Result<PathBuf> {
	let timestamp = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
	let root = PathBuf::from("tests-data/.tmp").join(format!("{test_name}-{}-{timestamp}", std::process::id()));
	fs::create_dir_all(&root)?;
	Ok(root)
}

fn path_text(path: &Path) -> String {
	path.to_string_lossy().replace('\\', "/")
}

fn sanitize_options(
	destination: &Path,
	source_root: &Path,
	model: &str,
	prompt: Option<SanitizePrompt>,
) -> ProcessContentOptions {
	let options = ProcessContentOptions::new(path_text(destination))
		.with_source(path_text(source_root))
		.with_sanitize(true)
		.with_model(model)
		.with_resume(true);

	if let Some(prompt) = prompt {
		options.with_sanitize_prompt(prompt)
	} else {
		options
	}
}

// endregion: --- Support
