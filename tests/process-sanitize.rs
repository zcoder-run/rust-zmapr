use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{
	Arc,
	atomic::{AtomicUsize, Ordering},
};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use zmapr::{
	ContentMapDocument, ItemState, ItemStatus, MaprAiClient, MaprAiResponse, MaprAiSelector, ProcessContentOptions,
	ProcessStage, ProgressEvent, SanitizePrompt, StageStatus, process_content, set_active_ai_selector,
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
	fs::write(
		source_root.join("intro.md"),
		b"# Intro\nKeep the substantive content.\n",
	)?;
	fs::write(source_root.join("image.png"), b"image bytes")?;
	let destination = root.join("destination");
	let options = ProcessContentOptions::new(path_text(&source_root))
		.with_dest(path_text(&destination))
		.with_sanitize(true)
		.with_model("stub-model");

	// -- Exec
	let mut handle = process_content(options).await?;
	let query = handle.query();
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
	assert_eq!(
		output.stats.sanitize.as_ref().ok_or("expected Sanitize stats")?.failed,
		0
	);
	let stats = query.stats();
	assert_eq!(stats.sanitize.completed, 1);
	assert_eq!(stats.sanitize.skipped, 1);
	assert_eq!(stats.sanitize.total_items, Some(2));
	assert_eq!(stats.sanitize.status, StageStatus::Completed);
	let item = query.item_by_path("intro.md").ok_or("expected intro.md item")?;
	let sanitize_state = item.sanitize.as_ref().ok_or("expected Sanitize state")?;
	assert_eq!(sanitize_state.status, ItemStatus::Completed);
	assert!(sanitize_state.usage.is_some());
	assert_eq!(
		item.content_path().ok_or("expected sanitized content path")?.as_std_path(),
		destination.join(".tmp-zmapr").join("02-sanitize").join("intro.md").as_path()
	);
	let sanitize_root = destination.join(".tmp-zmapr").join("02-sanitize");
	assert!(sanitize_root.join("intro.md").is_file());
	assert_eq!(
		fs::read(sanitize_root.join("intro.md"))?,
		b"# Intro\nKeep the substantive content.\n"
	);
	assert_eq!(fs::read(sanitize_root.join("image.png"))?, b"image bytes");

	let completed_item = output
		.items
		.iter()
		.find(|item| {
			item.relative_path == "intro.md"
				&& item
					.sanitize
					.as_ref()
					.is_some_and(|state| state.status == ItemStatus::Completed)
		})
		.ok_or("expected completed Sanitize item")?;
	let completed_sanitize = completed_item.sanitize.as_ref().ok_or("expected Sanitize state")?;
	assert!(completed_sanitize.usage.is_some());
	assert_eq!(
		completed_sanitize.path.as_ref().map(|path| path.as_std_path()),
		Some(sanitize_root.join("intro.md").as_path())
	);

	let skipped_item = output
		.items
		.iter()
		.find(|item| {
			item.relative_path == "image.png"
				&& item.sanitize.as_ref().is_some_and(|state| state.status == ItemStatus::Skipped)
		})
		.ok_or("expected skipped image item")?;
	let skipped_sanitize = skipped_item.sanitize.as_ref().ok_or("expected Sanitize state")?;
	assert_eq!(
		skipped_sanitize.path.as_ref().map(|path| path.as_std_path()),
		Some(sanitize_root.join("image.png").as_path())
	);
	assert!(progress_events.iter().any(|event| matches!(
		&event.event,
		ProgressEvent::StageCompleted {
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
	fs::write(source_root.join("image.png"), b"image bytes")?;
	let destination = root.join("destination");
	let options = ProcessContentOptions::new(path_text(&source_root))
		.with_dest(path_text(&destination))
		.with_sanitize(true)
		.with_map(true)
		.with_model("stub-model");

	// -- Exec
	let handle = process_content(options).await?;
	let output = handle.wait_output().await?;

	// -- Check
	assert_eq!(
		output.stats.sanitize.as_ref().ok_or("expected Sanitize stats")?.failed,
		0
	);
	assert_eq!(output.content_root.as_std_path(), destination.as_path());
	assert_eq!(
		fs::read(destination.join("intro.md"))?,
		b"# Intro\nSanitize before mapping."
	);
	assert_eq!(fs::read(destination.join("image.png"))?, b"image bytes");
	let content_map_path = output.content_map_path.as_ref().ok_or("expected content map")?;
	let document: ContentMapDocument = serde_json::from_slice(&fs::read(content_map_path.as_std_path())?)?;
	assert!(document.file_map.contains_key("intro.md"));
	assert!(destination.join(".tmp-zmapr").join("02-sanitize").join("intro.md").is_file());
	assert!(destination.join(".tmp-zmapr").join("02-sanitize").join("image.png").is_file());

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
	let options = ProcessContentOptions::new(path_text(&source_root))
		.with_dest(path_text(&destination))
		.with_sanitize(true)
		.with_model("custom-model")
		.with_sanitize_prompt(SanitizePrompt::content("CUSTOM RULES"));

	// -- Exec
	let handle = process_content(options).await?;
	let output = handle.wait_output().await?;

	// -- Check
	assert_eq!(
		output.stats.sanitize.as_ref().ok_or("expected Sanitize stats")?.failed,
		0
	);
	assert!(destination.join(".tmp-zmapr").join("02-sanitize").join("guide.md").is_file());

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
	let options = ProcessContentOptions::new(path_text(&source_root))
		.with_dest(path_text(&destination))
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
	assert_eq!(
		output.stats.sanitize.as_ref().ok_or("expected Sanitize stats")?.failed,
		1
	);
	let failed_item = output
		.items
		.iter()
		.find(|item| item.relative_path == "guide.md")
		.ok_or("expected failed Sanitize item")?;
	let failed_sanitize = failed_item.sanitize.as_ref().ok_or("expected Sanitize state")?;
	assert_eq!(failed_sanitize.status, ItemStatus::Failed);
	assert!(
		failed_sanitize
			.error
			.as_deref()
			.is_some_and(|message| message.contains("SANITIZED_CONTENT"))
	);
	assert!(progress_events.iter().any(|event| matches!(
		&event.event,
		ProgressEvent::StageCompleted {
			stage: ProcessStage::Sanitize
		}
	)));
	assert!(!destination.join(".tmp-zmapr").join("02-sanitize").join("guide.md").exists());
	assert!(!destination.join("guide.md").exists());

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
	let second_query = second_handle.query();
	let second_output = second_handle.wait_output().await?;

	// -- Check
	assert_eq!(second_query.stats().sanitize.reused, 2);
	assert_eq!(
		first_output.stats.sanitize.as_ref().ok_or("expected Sanitize stats")?.completed,
		2
	);
	assert_eq!(
		second_output
			.stats
			.sanitize
			.as_ref()
			.ok_or("expected Sanitize stats")?
			.completed,
		0
	);
	assert_eq!(second_output.stats.total_usage, None);
	assert_eq!(
		second_output.stats.sanitize.as_ref().ok_or("expected Sanitize stats")?.reused,
		2
	);
	let journal_path = destination.join(".tmp-zmapr").join("sanitize.journal.jsonl");
	let journal_contents = fs::read_to_string(&journal_path)?;
	let journal_records = journal_contents
		.lines()
		.map(serde_json::from_str::<serde_json::Value>)
		.collect::<std::result::Result<Vec<_>, _>>()?;
	assert_eq!(journal_records.len(), 3);
	assert_eq!(journal_records[0]["type"], "header");
	assert_eq!(
		journal_records.iter().filter(|record| record["type"] == "done").count(),
		2
	);

	// -- Exec & Check
	let sanitize_root = destination.join(".tmp-zmapr").join("02-sanitize");
	fs::write(sanitize_root.join("guide.md"), b"tampered output")?;
	let modified_output_handle =
		process_content(sanitize_options(&destination, &source_root, "stub-model", None)).await?;
	let modified_output = modified_output_handle.wait_output().await?;
	assert_eq!(
		relative_paths(&modified_output.items, ProcessStage::Sanitize, ItemStatus::Completed),
		vec!["guide.md"]
	);
	assert_eq!(
		modified_output.stats.sanitize.as_ref().ok_or("expected Sanitize stats")?.reused,
		1
	);

	fs::write(source_root.join("intro.md"), b"# Intro\nUpdated content.")?;
	let changed_source_handle =
		process_content(sanitize_options(&destination, &source_root, "stub-model", None)).await?;
	let changed_source_output = changed_source_handle.wait_output().await?;
	let completed_sources = relative_paths(
		&changed_source_output.items,
		ProcessStage::Sanitize,
		ItemStatus::Completed,
	);
	assert_eq!(completed_sources, vec!["intro.md"]);

	let changed_model_handle =
		process_content(sanitize_options(&destination, &source_root, "stub-model-v2", None)).await?;
	let changed_model_output = changed_model_handle.wait_output().await?;
	assert_eq!(
		changed_model_output
			.stats
			.sanitize
			.as_ref()
			.ok_or("expected Sanitize stats")?
			.completed,
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
			.stats
			.sanitize
			.as_ref()
			.ok_or("expected Sanitize stats")?
			.completed,
		2
	);

	fs::remove_file(destination.join(".tmp-zmapr").join("02-sanitize").join("intro.md"))?;
	let missing_output_handle = process_content(sanitize_options(
		&destination,
		&source_root,
		"stub-model-v2",
		custom_prompt,
	))
	.await?;
	let missing_output = missing_output_handle.wait_output().await?;
	let completed_sources = relative_paths(&missing_output.items, ProcessStage::Sanitize, ItemStatus::Completed);
	assert_eq!(completed_sources, vec!["intro.md"]);

	set_active_ai_selector(None);
	Ok(())
}

#[tokio::test]
async fn test_process_sanitize_respects_concurrency_limit() -> Result<()> {
	let _guard = TEST_MUTEX.lock().await;
	let current = Arc::new(AtomicUsize::new(0));
	let peak = Arc::new(AtomicUsize::new(0));
	set_active_ai_selector(Some(MaprAiSelector::Custom(Arc::new(ConcurrencyTrackingAiClient {
		current: Arc::clone(&current),
		peak: Arc::clone(&peak),
	}))));

	// -- Setup & Fixtures
	let root = fixture_root("test_process_sanitize_respects_concurrency_limit")?;
	let source_root = root.join("source");
	fs::create_dir_all(&source_root)?;
	for index in 0..6 {
		fs::write(source_root.join(format!("item-{index}.md")), b"Content to sanitize.")?;
	}
	let destination = root.join("destination");
	let options = ProcessContentOptions::new(path_text(&source_root))
		.with_dest(path_text(&destination))
		.with_sanitize(true)
		.with_model("custom-model")
		.with_concurrency(2);

	// -- Exec
	let handle = process_content(options).await?;
	let output = handle.wait_output().await?;

	// -- Check
	let stats = output.stats.sanitize.as_ref().ok_or("expected Sanitize stats")?;
	assert_eq!(stats.completed, 6);
	assert_eq!(stats.failed, 0);
	assert_eq!(peak.load(Ordering::SeqCst), 2);

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
	let options = ProcessContentOptions::new(path_text(&source_root))
		.with_dest(path_text(&destination))
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
struct ConcurrencyTrackingAiClient {
	current: Arc<AtomicUsize>,
	peak: Arc<AtomicUsize>,
}

impl MaprAiClient for ConcurrencyTrackingAiClient {
	fn complete<'a>(&'a self, _prompt: &'a str) -> zmapr::BoxFuture<'a, zmapr::Result<MaprAiResponse>> {
		let current = Arc::clone(&self.current);
		let peak = Arc::clone(&self.peak);

		Box::pin(async move {
			let active = current.fetch_add(1, Ordering::SeqCst) + 1;
			let _ = peak.fetch_max(active, Ordering::SeqCst);
			tokio::time::sleep(Duration::from_millis(30)).await;
			let _ = current.fetch_sub(1, Ordering::SeqCst);
			Ok(MaprAiResponse::new("<SANITIZED_CONTENT>\nok\n</SANITIZED_CONTENT>"))
		})
	}
}

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
	let options = ProcessContentOptions::new(path_text(source_root))
		.with_dest(path_text(destination))
		.with_sanitize(true)
		.with_model(model)
		.with_resume(true);

	if let Some(prompt) = prompt {
		options.with_sanitize_prompt(prompt)
	} else {
		options
	}
}

fn relative_paths(items: &[ItemState], stage: ProcessStage, status: ItemStatus) -> Vec<String> {
	let mut paths = items
		.iter()
		.filter(|item| item.stage(stage).is_some_and(|state| state.status == status))
		.map(|item| item.relative_path.clone())
		.collect::<Vec<_>>();
	paths.sort();
	paths
}

// endregion: --- Support
