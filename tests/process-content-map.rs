use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{
	Arc,
	atomic::{AtomicUsize, Ordering},
};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use zmapr::{
	ContentMapDocument, ItemState, ItemStatus, MaprAiClient, MaprAiResponse, MaprAiSelector, ProcessContentOptions,
	ProcessStage, ProgressEvent, StageStatus, process_content, set_active_ai_selector,
};

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>; // For tests.

static TEST_MUTEX: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

#[tokio::test]
async fn test_process_content_map_respects_concurrency_limit() -> Result<()> {
	let _guard = TEST_MUTEX.lock().await;
	let current = Arc::new(AtomicUsize::new(0));
	let peak = Arc::new(AtomicUsize::new(0));
	set_active_ai_selector(Some(MaprAiSelector::Custom(Arc::new(ConcurrencyTrackingMapAiClient {
		current: Arc::clone(&current),
		peak: Arc::clone(&peak),
	}))));

	// -- Setup & Fixtures
	let root = fixture_root("test_process_content_map_respects_concurrency_limit")?;
	let source_root = root.join("source");
	fs::create_dir_all(&source_root)?;
	for index in 0..6 {
		fs::write(source_root.join(format!("item-{index}.md")), b"# Item\nContent to map.")?;
	}
	let destination = root.join("destination");
	let options = ProcessContentOptions::new(path_text(&destination))
		.with_source(path_text(&source_root))
		.with_map(true)
		.with_model("custom-model")
		.with_concurrency(2);

	// -- Exec
	let handle = process_content(options).await?;
	let output = handle.wait_output().await?;

	// -- Check
	let stats = output.stats.map.as_ref().ok_or("expected Map stats")?;
	assert_eq!(stats.completed, 6);
	assert_eq!(stats.failed, 0);
	assert_eq!(peak.load(Ordering::SeqCst), 2);

	set_active_ai_selector(None);
	Ok(())
}

#[tokio::test]
async fn test_process_content_map_with_stub_publishes_output_and_content_map() -> Result<()> {
	let _guard = TEST_MUTEX.lock().await;
	set_active_ai_selector(Some(MaprAiSelector::Stub));

	// -- Setup & Fixtures
	let root = fixture_root("test_process_content_map_with_stub_publishes_output_and_content_map")?;
	let source_root = root.join("source");
	fs::create_dir_all(&source_root)?;
	fs::write(source_root.join("intro.md"), b"# Introduction\nWelcome.")?;
	fs::write(source_root.join("code.rs"), b"pub fn run() {}\n")?;
	fs::write(source_root.join("image.png"), b"PNG dummy image data")?;
	fs::write(source_root.join("oversize.txt"), vec![b'a'; 300_000])?;
	let destination = root.join("destination");

	let options = ProcessContentOptions::new(path_text(&destination))
		.with_source(path_text(&source_root))
		.with_map(true)
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
		output.stats.fetch.as_ref().ok_or("expected Fetch stats")?.completed
			+ output.stats.map.as_ref().ok_or("expected Map stats")?.completed,
		6
	); // 4 from fetch + 2 from mapr
	let stats = query.stats();
	assert_eq!(stats.map.completed, 2);
	assert_eq!(stats.map.skipped, 2);
	assert_eq!(stats.map.total_items, Some(4));
	assert_eq!(stats.map.status, StageStatus::Completed);
	assert_eq!(stats.total_usage, output.stats.total_usage);
	let intro_item = query.item_by_path("intro.md").ok_or("expected intro.md item")?;
	assert_eq!(
		intro_item.fetch.as_ref().map(|state| state.status),
		Some(ItemStatus::Completed)
	);
	assert_eq!(
		intro_item.map.as_ref().map(|state| state.status),
		Some(ItemStatus::Completed)
	);

	let manifest_path = output.manifest_path.as_ref().ok_or("expected manifest_path")?;
	assert!(manifest_path.is_file());
	assert_eq!(
		manifest_path.as_std_path(),
		destination.join(".tmp-zmapr").join("manifest.json").as_path()
	);
	let fetch_root = destination.join(".tmp-zmapr").join("01-fetch");
	assert_eq!(fs::read(fetch_root.join("intro.md"))?, b"# Introduction\nWelcome.");
	assert!(fetch_root.join("image.png").is_file());
	assert_eq!(fs::metadata(fetch_root.join("oversize.txt"))?.len(), 300_000);
	assert_eq!(fs::read(source_root.join("intro.md"))?, b"# Introduction\nWelcome.");

	let mapr_completed_sources = relative_paths(&output.items, ProcessStage::Map, ItemStatus::Completed);
	assert_eq!(mapr_completed_sources, vec!["code.rs", "intro.md"]);

	let mapr_skipped_sources = relative_paths(&output.items, ProcessStage::Map, ItemStatus::Skipped);
	assert_eq!(mapr_skipped_sources, vec!["image.png", "oversize.txt"]);

	let content_map_path = output.content_map_path.as_ref().ok_or("expected content_map_path")?;
	assert!(content_map_path.is_file());
	assert_eq!(
		content_map_path.as_std_path(),
		destination.join("content-map.json").as_path()
	);

	let content_str = fs::read_to_string(content_map_path.as_std_path())?;
	let document: ContentMapDocument = serde_json::from_str(&content_str)?;
	assert_eq!(document.version, 1);
	assert_eq!(document.model, "stub-model");
	assert_eq!(document.prompt_version, 2);
	assert_eq!(document.file_map.len(), 2);
	assert!(document.file_map.contains_key("intro.md"));
	assert!(document.file_map.contains_key("code.rs"));
	assert_eq!(document.file_metadata.len(), 4);
	assert!(!destination.join("content-map.md").exists());
	let intro_metadata = document.file_metadata.get("intro.md").ok_or("expected intro.md metadata")?;
	assert!(intro_metadata.last_modified_unix_nanos.is_some());
	assert!(!intro_metadata.source_hash.is_empty());
	let intro_entry = document.file_map.get("intro.md").ok_or("expected intro.md entry")?;
	assert_eq!(intro_entry.topics, vec!["stub".to_string(), "test".to_string()]);
	assert!(document.folder_map.is_empty());

	let mapr_completed_ids = progress_events
		.iter()
		.filter_map(|update| match &update.event {
			ProgressEvent::ItemStatusChanged {
				id,
				stage: ProcessStage::Map,
				status: ItemStatus::Completed,
			} => Some(*id),
			_ => None,
		})
		.collect::<Vec<_>>();
	assert_eq!(mapr_completed_ids.len(), 2);
	for id in &mapr_completed_ids {
		let item = query.item(*id).ok_or("expected completed Map item")?;
		assert!(item.map.as_ref().and_then(|state| state.usage.as_ref()).is_some());
	}

	let journal_file = destination.join(".tmp-zmapr").join("content-map.journal.jsonl");
	assert!(journal_file.is_file());
	assert_eq!(fs::metadata(&journal_file)?.len(), 0);

	let total_usage = output.stats.total_usage.as_ref().ok_or("expected total_usage on output")?;
	let sum_prompt: i32 = mapr_completed_ids
		.iter()
		.filter_map(|id| {
			query
				.item(*id)
				.and_then(|item| item.map)
				.and_then(|state| state.usage)
				.and_then(|usage| usage.prompt_tokens)
		})
		.sum();
	let sum_completion: i32 = mapr_completed_ids
		.iter()
		.filter_map(|id| {
			query
				.item(*id)
				.and_then(|item| item.map)
				.and_then(|state| state.usage)
				.and_then(|usage| usage.completion_tokens)
		})
		.sum();
	let sum_total: i32 = mapr_completed_ids
		.iter()
		.filter_map(|id| {
			query
				.item(*id)
				.and_then(|item| item.map)
				.and_then(|state| state.usage)
				.and_then(|usage| usage.total_tokens)
		})
		.sum();

	assert_eq!(total_usage.prompt_tokens, Some(sum_prompt));
	assert_eq!(total_usage.completion_tokens, Some(sum_completion));
	assert_eq!(total_usage.total_tokens, Some(sum_total));

	let has_stage_started = progress_events.iter().any(|update| {
		matches!(
			&update.event,
			ProgressEvent::StageStarted {
				stage: ProcessStage::Map
			}
		)
	});
	let has_stage_completed = progress_events.iter().any(|update| {
		matches!(
			&update.event,
			ProgressEvent::StageCompleted {
				stage: ProcessStage::Map
			}
		)
	});
	assert!(has_stage_started);
	assert!(has_stage_completed);

	set_active_ai_selector(None);
	Ok(())
}

#[tokio::test]
async fn test_process_content_map_journal_reuse_on_second_run() -> Result<()> {
	let _guard = TEST_MUTEX.lock().await;
	set_active_ai_selector(Some(MaprAiSelector::Stub));

	// -- Setup & Fixtures
	let root = fixture_root("test_process_content_map_journal_reuse_on_second_run")?;
	let source_root = root.join("source");
	fs::create_dir_all(&source_root)?;
	fs::write(source_root.join("intro.md"), b"# Intro\nReused content.")?;
	let destination = root.join("destination");

	let run_options = || {
		ProcessContentOptions::new(path_text(&destination))
			.with_source(path_text(&source_root))
			.with_map(true)
			.with_model("stub-model")
			.with_resume(true)
	};

	// -- Exec: first run
	let first_handle = process_content(run_options()).await?;
	let first_output = first_handle.wait_output().await?;
	let first_content_map_path = first_output
		.content_map_path
		.as_ref()
		.ok_or("expected first content_map_path")?;
	let first_document: ContentMapDocument = serde_json::from_slice(&fs::read(first_content_map_path.as_std_path())?)?;

	let journal_file = destination.join(".tmp-zmapr").join("content-map.journal.jsonl");
	assert!(journal_file.is_file());
	assert!(fs::metadata(&journal_file)?.len() > 0);
	let journal_content = fs::read_to_string(&journal_file)?;
	assert!(journal_content.lines().any(|line| line.contains(r#""path":"intro.md""#)));

	let first_mapr_completed = first_output.stats.map.as_ref().ok_or("expected Map stats")?.completed;
	assert_eq!(first_mapr_completed, 1);

	let fetch_copy = destination.join(".tmp-zmapr").join("01-fetch").join("intro.md");
	assert_eq!(fs::read(&fetch_copy)?, b"# Intro\nReused content.");

	// -- Exec: second run with unchanged source
	let second_handle = process_content(run_options()).await?;
	let second_query = second_handle.query();
	let second_output = second_handle.wait_output().await?;

	// -- Check: item reused from journal
	assert_eq!(second_query.stats().map.reused, 1);
	let second_mapr_completed = second_output.stats.map.as_ref().ok_or("expected Map stats")?.completed;
	assert_eq!(second_mapr_completed, 0);

	let second_mapr_reused = relative_paths(&second_output.items, ProcessStage::Map, ItemStatus::Reused);
	assert_eq!(second_mapr_reused, vec!["intro.md"]);
	assert_eq!(fs::read(&fetch_copy)?, b"# Intro\nReused content.");
	assert_eq!(fs::read(source_root.join("intro.md"))?, b"# Intro\nReused content.");

	let content_map_path = second_output.content_map_path.as_ref().ok_or("expected content_map_path")?;
	let content_str = fs::read_to_string(content_map_path.as_std_path())?;
	let document: ContentMapDocument = serde_json::from_str(&content_str)?;
	assert_eq!(document.file_map.len(), 1);
	assert!(document.file_map.contains_key("intro.md"));
	assert_eq!(document.file_map, first_document.file_map);
	let first_metadata = first_document
		.file_metadata
		.get("intro.md")
		.ok_or("expected first intro.md metadata")?;
	let second_metadata = document
		.file_metadata
		.get("intro.md")
		.ok_or("expected second intro.md metadata")?;
	assert_eq!(second_metadata.source_hash, first_metadata.source_hash);

	assert_eq!(second_output.stats.total_usage, None);

	set_active_ai_selector(None);
	Ok(())
}

#[tokio::test]
async fn test_process_content_map_copies_html_as_markdown() -> Result<()> {
	let _guard = TEST_MUTEX.lock().await;
	set_active_ai_selector(Some(MaprAiSelector::Stub));

	// -- Setup & Fixtures
	let root = fixture_root("test_process_content_map_copies_html_as_markdown")?;
	let source_root = root.join("source");
	fs::create_dir_all(&source_root)?;
	let html = "<html><body><h1>Mapper copy</h1><p>Prepared Markdown.</p></body></html>";
	fs::write(source_root.join("index.html"), html)?;
	let destination = root.join("destination");

	let options = ProcessContentOptions::new(path_text(&destination))
		.with_source(path_text(&source_root))
		.with_map(true)
		.with_model("stub-model")
		.with_resume(true);

	// -- Exec
	let handle = process_content(options).await?;
	let output = handle.wait_output().await?;

	// -- Check
	assert_eq!(output.stats.map.as_ref().ok_or("expected Map stats")?.failed, 0);
	let markdown = fs::read_to_string(destination.join(".tmp-zmapr").join("01-fetch").join("index.md"))?;
	assert!(markdown.contains("Mapper copy"));
	assert!(markdown.contains("Prepared Markdown."));

	let content_map_path = output.content_map_path.as_ref().ok_or("expected content_map_path")?;
	let document: ContentMapDocument = serde_json::from_slice(&fs::read(content_map_path.as_std_path())?)?;
	assert!(document.file_map.contains_key("index.md"));
	let metadata = document.file_metadata.get("index.md").ok_or("expected HTML input metadata")?;
	assert!(!metadata.source_hash.is_empty());
	let journal_file = destination.join(".tmp-zmapr").join("content-map.journal.jsonl");
	let journal_content = fs::read_to_string(journal_file)?;
	assert!(journal_content.lines().any(|line| line.contains(r#""path":"index.md""#)));

	set_active_ai_selector(None);
	Ok(())
}

#[tokio::test]
async fn test_process_content_map_publishes_recovered_entries_before_ai_work() -> Result<()> {
	let _guard = TEST_MUTEX.lock().await;
	set_active_ai_selector(Some(MaprAiSelector::Stub));

	// -- Setup & Fixtures
	let root = fixture_root("test_process_content_map_publishes_recovered_entries_before_ai_work")?;
	let source_root = root.join("source");
	fs::create_dir_all(&source_root)?;
	fs::write(source_root.join("intro.md"), "# Intro\nCached entry.")?;
	let destination = root.join("destination");

	let run_options = || {
		ProcessContentOptions::new(path_text(&destination))
			.with_source(path_text(&source_root))
			.with_map(true)
			.with_model("stub-model")
			.with_resume(true)
	};

	// -- Exec: seed a successful journal record
	let first_handle = process_content(run_options()).await?;
	let _first_output = first_handle.wait_output().await?;

	fs::write(source_root.join("new.md"), "# New\nRequires AI work.")?;
	let empty_document = ContentMapDocument::new(
		"stub-model",
		2,
		"2026-09-23T00:00:00Z",
		Default::default(),
		Default::default(),
	);
	fs::write(
		destination.join("content-map.json"),
		serde_json::to_vec(&empty_document)?,
	)?;

	set_active_ai_selector(Some(MaprAiSelector::Custom(Arc::new(PartialMapCheckingAiClient {
		content_map_path: destination.join("content-map.json"),
	}))));

	// -- Exec: the AI client checks that journal recovery was published first
	let second_handle = process_content(run_options()).await?;
	let second_output = second_handle.wait_output().await?;

	// -- Check
	assert_eq!(second_output.stats.map.as_ref().ok_or("expected Map stats")?.failed, 0);
	let content_map_path = second_output.content_map_path.as_ref().ok_or("expected content_map_path")?;
	let document: ContentMapDocument = serde_json::from_slice(&fs::read(content_map_path.as_std_path())?)?;
	assert!(document.file_map.contains_key("intro.md"));
	assert!(document.file_map.contains_key("new.md"));
	let recovered_metadata = document
		.file_metadata
		.get("intro.md")
		.ok_or("expected recovered intro.md metadata")?;
	assert!(!recovered_metadata.source_hash.is_empty());

	set_active_ai_selector(None);
	Ok(())
}

#[derive(Debug)]
struct FailingAiClient;

impl MaprAiClient for FailingAiClient {
	fn complete<'a>(&'a self, prompt: &'a str) -> zmapr::BoxFuture<'a, zmapr::Result<zmapr::MaprAiResponse>> {
		Box::pin(async move {
			if prompt.contains("fail.md") {
				Err(zmapr::Error::custom("simulated AI provider failure"))
			} else {
				Ok(zmapr::MaprAiResponse::new(
					"<FILE_INFO>\n{\"summary\": \"OK file\", \"when_to_use\": \"Usage\", \"public_types\": [], \"public_functions\": [], \"topics\": []}\n</FILE_INFO>",
				))
			}
		})
	}
}

#[tokio::test]
async fn test_process_content_map_item_failure_is_recorded_and_stage_completes() -> Result<()> {
	let _guard = TEST_MUTEX.lock().await;
	set_active_ai_selector(Some(MaprAiSelector::Custom(Arc::new(FailingAiClient))));

	// -- Setup & Fixtures
	let root = fixture_root("test_process_content_map_item_failure_is_recorded_and_stage_completes")?;
	let source_root = root.join("source");
	fs::create_dir_all(&source_root)?;
	fs::write(source_root.join("good.md"), b"# Good\nWorks fine.")?;
	fs::write(source_root.join("fail.md"), b"# Fail\nTriggers error.")?;
	let destination = root.join("destination");

	let options = ProcessContentOptions::new(path_text(&destination))
		.with_source(path_text(&source_root))
		.with_map(true)
		.with_model("custom-model");

	// -- Exec
	let handle = process_content(options).await?;
	let query = handle.query();
	let output = handle.wait_output().await?;

	// -- Check
	assert_eq!(output.stats.map.as_ref().ok_or("expected Map stats")?.failed, 1);
	let stats = query.stats();
	assert_eq!(stats.map.failed, 1);
	let failed_item = query.item_by_path("fail.md").ok_or("expected fail.md item")?;
	let map_state = failed_item.map.as_ref().ok_or("expected Map state")?;
	assert_eq!(map_state.status, ItemStatus::Failed);
	assert_eq!(failed_item.relative_path, "fail.md");
	assert!(
		map_state
			.error
			.as_deref()
			.ok_or("expected Map failure detail")?
			.contains("simulated AI provider failure")
	);
	let mapr_completed = relative_paths(&output.items, ProcessStage::Map, ItemStatus::Completed);
	assert_eq!(mapr_completed, vec!["good.md"]);

	let content_map_path = output.content_map_path.as_ref().ok_or("expected content_map_path")?;
	let content_str = fs::read_to_string(content_map_path.as_std_path())?;
	let document: ContentMapDocument = serde_json::from_str(&content_str)?;
	assert_eq!(document.file_map.len(), 1);
	assert!(document.file_map.contains_key("good.md"));
	assert!(!document.file_map.contains_key("fail.md"));

	let journal_file = destination.join(".tmp-zmapr").join("content-map.journal.jsonl");
	assert!(journal_file.is_file());
	assert!(fs::metadata(&journal_file)?.len() > 0);

	set_active_ai_selector(None);
	Ok(())
}

#[derive(Debug)]
struct PanickingMapAiClient;

impl MaprAiClient for PanickingMapAiClient {
	fn complete<'a>(&'a self, prompt: &'a str) -> zmapr::BoxFuture<'a, zmapr::Result<zmapr::MaprAiResponse>> {
		Box::pin(async move {
			if prompt.is_empty() {
				Ok(zmapr::MaprAiResponse::new(""))
			} else {
				panic!("simulated Map task panic")
			}
		})
	}
}

#[tokio::test]
async fn test_process_content_map_task_panic_returns_task_join_error() -> Result<()> {
	let _guard = TEST_MUTEX.lock().await;
	set_active_ai_selector(Some(MaprAiSelector::Custom(Arc::new(PanickingMapAiClient))));

	// -- Setup & Fixtures
	let root = fixture_root("test_process_content_map_task_panic_returns_task_join_error")?;
	let source_root = root.join("source");
	fs::create_dir_all(&source_root)?;
	fs::write(source_root.join("panic.md"), b"# Panic\nTrigger a task panic.")?;
	let destination = root.join("destination");
	let options = ProcessContentOptions::new(path_text(&destination))
		.with_source(path_text(&source_root))
		.with_map(true)
		.with_model("custom-model");

	// -- Exec
	let handle = process_content(options).await?;
	let error = handle.wait_output().await.err().ok_or("expected Map task join error")?;
	set_active_ai_selector(None);

	// -- Check
	assert!(matches!(error, zmapr::Error::TaskJoin(_)));

	Ok(())
}

#[derive(Debug)]
struct ExactUsageAiClient;

impl MaprAiClient for ExactUsageAiClient {
	fn complete<'a>(&'a self, _prompt: &'a str) -> zmapr::BoxFuture<'a, zmapr::Result<zmapr::MaprAiResponse>> {
		Box::pin(async move {
			let usage = genai::chat::Usage {
				prompt_tokens: Some(100),
				completion_tokens: Some(50),
				total_tokens: Some(150),
				..Default::default()
			};
			Ok(zmapr::MaprAiResponse::new(
				"<FILE_INFO>\n{\"summary\": \"Summary\", \"when_to_use\": \"Usage\", \"public_types\": [], \"public_functions\": [], \"topics\": [\"topic\"]}\n</FILE_INFO>",
			).with_usage(usage))
		})
	}
}

#[tokio::test]
async fn test_process_content_map_exact_usage_and_journal_emptied() -> Result<()> {
	let _guard = TEST_MUTEX.lock().await;
	set_active_ai_selector(Some(MaprAiSelector::Custom(Arc::new(ExactUsageAiClient))));

	// -- Setup & Fixtures
	let root = fixture_root("test_process_content_map_exact_usage_and_journal_emptied")?;
	let source_root = root.join("source");
	fs::create_dir_all(&source_root)?;
	fs::write(source_root.join("one.md"), b"# Doc 1\nFirst doc.")?;
	fs::write(source_root.join("two.md"), b"# Doc 2\nSecond doc.")?;
	let destination = root.join("destination");

	let options = ProcessContentOptions::new(path_text(&destination))
		.with_source(path_text(&source_root))
		.with_map(true)
		.with_model("custom-model");

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
		output.stats.fetch.as_ref().ok_or("expected Fetch stats")?.completed
			+ output.stats.map.as_ref().ok_or("expected Map stats")?.completed,
		4
	);

	let completed_mapr_ids = progress_events
		.iter()
		.filter_map(|update| match &update.event {
			ProgressEvent::ItemStatusChanged {
				id,
				stage: ProcessStage::Map,
				status: ItemStatus::Completed,
			} => Some(*id),
			_ => None,
		})
		.collect::<Vec<_>>();
	assert_eq!(completed_mapr_ids.len(), 2);
	for id in &completed_mapr_ids {
		let item = query.item(*id).ok_or("expected completed Map item")?;
		let usage = item
			.map
			.as_ref()
			.and_then(|state| state.usage.as_ref())
			.ok_or("expected item usage")?;
		assert_eq!(usage.prompt_tokens, Some(100));
		assert_eq!(usage.completion_tokens, Some(50));
		assert_eq!(usage.total_tokens, Some(150));
	}

	let total_usage = output.stats.total_usage.as_ref().ok_or("expected aggregate total_usage")?;
	assert_eq!(total_usage.prompt_tokens, Some(200));
	assert_eq!(total_usage.completion_tokens, Some(100));
	assert_eq!(total_usage.total_tokens, Some(300));

	let journal_file = destination.join(".tmp-zmapr").join("content-map.journal.jsonl");
	assert!(journal_file.is_file());
	assert_eq!(fs::metadata(&journal_file)?.len(), 0);

	set_active_ai_selector(None);
	Ok(())
}

// region:    --- Support

#[derive(Debug)]
struct ConcurrencyTrackingMapAiClient {
	current: Arc<AtomicUsize>,
	peak: Arc<AtomicUsize>,
}

impl MaprAiClient for ConcurrencyTrackingMapAiClient {
	fn complete<'a>(&'a self, _prompt: &'a str) -> zmapr::BoxFuture<'a, zmapr::Result<MaprAiResponse>> {
		let current = Arc::clone(&self.current);
		let peak = Arc::clone(&self.peak);

		Box::pin(async move {
			let active = current.fetch_add(1, Ordering::SeqCst) + 1;
			let _ = peak.fetch_max(active, Ordering::SeqCst);
			tokio::time::sleep(Duration::from_millis(30)).await;
			let _ = current.fetch_sub(1, Ordering::SeqCst);
			Ok(MaprAiResponse::new(
				"<FILE_INFO>\n{\"summary\": \"Mapped file\", \"when_to_use\": \"Usage\", \"public_types\": [], \"public_functions\": [], \"topics\": [\"mapped\"]}\n</FILE_INFO>",
			))
		})
	}
}

#[derive(Debug)]
struct PartialMapCheckingAiClient {
	content_map_path: PathBuf,
}

impl MaprAiClient for PartialMapCheckingAiClient {
	fn complete<'a>(&'a self, _prompt: &'a str) -> zmapr::BoxFuture<'a, zmapr::Result<zmapr::MaprAiResponse>> {
		let content_map_path = self.content_map_path.clone();
		Box::pin(async move {
			let content =
				fs::read_to_string(content_map_path).map_err(|error| zmapr::Error::custom(error.to_string()))?;
			let document: ContentMapDocument =
				serde_json::from_str(&content).map_err(|error| zmapr::Error::custom(error.to_string()))?;
			if !document.file_map.contains_key("intro.md") {
				return Err(zmapr::Error::custom(
					"journal entry was not published before AI processing",
				));
			}
			let Some(metadata) = document.file_metadata.get("intro.md") else {
				return Err(zmapr::Error::custom(
					"journal metadata was not published before AI processing",
				));
			};
			if metadata.source_hash.is_empty() {
				return Err(zmapr::Error::custom("journal metadata is incomplete"));
			}

			Ok(zmapr::MaprAiResponse::new(
				"<FILE_INFO>\n{\"summary\": \"New file\", \"when_to_use\": \"Usage\", \"public_types\": [], \"public_functions\": [], \"topics\": [\"new\"]}\n</FILE_INFO>",
			))
		})
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
