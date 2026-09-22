use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use zmapr::{
	ContentMapDocument, ContentMapOptions, LocalFetchRequest, MaprAiClient, MaprAiSelector, ProcessContentOptions,
	ProcessProgress, ProcessStage, process_content, set_active_ai_selector,
};

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>; // For tests.

static TEST_MUTEX: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

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
		.with_fetch(LocalFetchRequest::new(path_text(&source_root)).with_copy_local_files(true))
		.with_content_map(
			ContentMapOptions::new("stub-model")
				.with_max_size(200_000)
				.with_retain_journal(true),
		);

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
	assert_eq!(output.completed_items.len(), 6); // 4 from fetch + 2 from mapr

	let mapr_completed = output
		.completed_items
		.iter()
		.filter(|item| item.stage == ProcessStage::AiContentMap)
		.collect::<Vec<_>>();
	assert_eq!(mapr_completed.len(), 2);
	let mapr_completed_sources = mapr_completed.iter().map(|item| item.source.as_str()).collect::<Vec<_>>();
	assert_eq!(mapr_completed_sources, vec!["code.rs", "intro.md"]);

	let mapr_skipped = output
		.skipped_items
		.iter()
		.filter(|item| item.stage == ProcessStage::AiContentMap)
		.collect::<Vec<_>>();
	assert_eq!(mapr_skipped.len(), 2);
	let mapr_skipped_sources = mapr_skipped.iter().map(|item| item.source.as_str()).collect::<Vec<_>>();
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
	let intro_entry = document.file_map.get("intro.md").ok_or("expected intro.md entry")?;
	assert_eq!(intro_entry.topics, vec!["stub".to_string(), "test".to_string()]);
	assert!(document.folder_map.is_empty());

	let mapr_completed_events = progress_events
		.iter()
		.filter_map(|ev| match ev {
			ProcessProgress::ItemCompleted { item } if item.stage == ProcessStage::AiContentMap => Some(item),
			_ => None,
		})
		.collect::<Vec<_>>();
	assert_eq!(mapr_completed_events.len(), 2);
	for item in &mapr_completed_events {
		assert!(item.usage.is_some());
	}

	let journal_file = destination.join(".tmp-zmapr").join("content-map.journal.jsonl");
	assert!(journal_file.is_file());
	assert_eq!(fs::metadata(&journal_file)?.len(), 0);

	let total_usage = output.total_usage.as_ref().ok_or("expected total_usage on output")?;
	let sum_prompt: i32 = mapr_completed_events
		.iter()
		.filter_map(|item| item.usage.as_ref().and_then(|u| u.prompt_tokens))
		.sum();
	let sum_completion: i32 = mapr_completed_events
		.iter()
		.filter_map(|item| item.usage.as_ref().and_then(|u| u.completion_tokens))
		.sum();
	let sum_total: i32 = mapr_completed_events
		.iter()
		.filter_map(|item| item.usage.as_ref().and_then(|u| u.total_tokens))
		.sum();

	assert_eq!(total_usage.prompt_tokens, Some(sum_prompt));
	assert_eq!(total_usage.completion_tokens, Some(sum_completion));
	assert_eq!(total_usage.total_tokens, Some(sum_total));

	let has_stage_started = progress_events.iter().any(|ev| {
		matches!(
			ev,
			ProcessProgress::StageStarted {
				stage: ProcessStage::AiContentMap
			}
		)
	});
	let has_stage_completed = progress_events.iter().any(|ev| {
		matches!(
			ev,
			ProcessProgress::StageCompleted {
				stage: ProcessStage::AiContentMap
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
			.with_fetch(LocalFetchRequest::new(path_text(&source_root)).with_copy_local_files(true))
			.with_content_map(
				ContentMapOptions::new("stub-model")
					.with_reuse_unchanged_records(true)
					.with_retain_journal(true),
			)
			.with_resume(true)
	};

	// -- Exec: first run
	let first_handle = process_content(run_options()).await?;
	let first_output = first_handle.wait_output().await?;

	let journal_file = destination.join(".tmp-zmapr").join("content-map.journal.jsonl");
	assert!(journal_file.is_file());
	assert!(fs::metadata(&journal_file)?.len() > 0);

	let first_mapr_completed = first_output
		.completed_items
		.iter()
		.filter(|item| item.stage == ProcessStage::AiContentMap)
		.count();
	assert_eq!(first_mapr_completed, 1);

	// -- Exec: second run with unchanged source
	let second_handle = process_content(run_options()).await?;
	let second_output = second_handle.wait_output().await?;

	// -- Check: item reused from journal
	let second_mapr_completed = second_output
		.completed_items
		.iter()
		.filter(|item| item.stage == ProcessStage::AiContentMap)
		.count();
	assert_eq!(second_mapr_completed, 0);

	let second_mapr_skipped = second_output
		.skipped_items
		.iter()
		.filter(|item| item.stage == ProcessStage::AiContentMap)
		.collect::<Vec<_>>();
	assert_eq!(second_mapr_skipped.len(), 1);
	assert_eq!(second_mapr_skipped[0].source, "intro.md");

	let content_map_path = second_output.content_map_path.as_ref().ok_or("expected content_map_path")?;
	let content_str = fs::read_to_string(content_map_path.as_std_path())?;
	let document: ContentMapDocument = serde_json::from_str(&content_str)?;
	assert_eq!(document.file_map.len(), 1);
	assert!(document.file_map.contains_key("intro.md"));

	assert_eq!(second_output.total_usage, None);

	set_active_ai_selector(None);
	Ok(())
}

#[tokio::test]
async fn test_process_content_map_retain_journal_false_removes_journal() -> Result<()> {
	let _guard = TEST_MUTEX.lock().await;
	set_active_ai_selector(Some(MaprAiSelector::Stub));

	// -- Setup & Fixtures
	let root = fixture_root("test_process_content_map_retain_journal_false_removes_journal")?;
	let source_root = root.join("source");
	fs::create_dir_all(&source_root)?;
	fs::write(source_root.join("intro.md"), b"# Intro\nClean journal.")?;
	let destination = root.join("destination");

	let options = ProcessContentOptions::new(path_text(&destination))
		.with_fetch(LocalFetchRequest::new(path_text(&source_root)).with_copy_local_files(true))
		.with_content_map(ContentMapOptions::new("stub-model").with_retain_journal(false));

	// -- Exec
	let handle = process_content(options).await?;
	let output = handle.wait_output().await?;

	// -- Check
	let content_map_path = output.content_map_path.as_ref().ok_or("expected content_map_path")?;
	assert!(content_map_path.is_file());

	let journal_file = destination.join(".tmp-zmapr").join("content-map.journal.jsonl");
	assert!(!journal_file.exists());

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
		.with_fetch(LocalFetchRequest::new(path_text(&source_root)).with_copy_local_files(true))
		.with_content_map(ContentMapOptions::new("custom-model"));

	// -- Exec
	let handle = process_content(options).await?;
	let output = handle.wait_output().await?;

	// -- Check
	assert_eq!(output.failures.len(), 1);
	let failure = &output.failures[0];
	assert_eq!(failure.item.source, "fail.md");
	assert_eq!(failure.item.stage, ProcessStage::AiContentMap);
	assert!(failure.message.contains("simulated AI provider failure"));

	let mapr_completed = output
		.completed_items
		.iter()
		.filter(|item| item.stage == ProcessStage::AiContentMap)
		.collect::<Vec<_>>();
	assert_eq!(mapr_completed.len(), 1);
	assert_eq!(mapr_completed[0].source, "good.md");

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
		.with_fetch(LocalFetchRequest::new(path_text(&source_root)).with_copy_local_files(true))
		.with_content_map(ContentMapOptions::new("custom-model").with_retain_journal(true));

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
	assert_eq!(output.completed_items.len(), 4);

	let completed_mapr_events = progress_events
		.iter()
		.filter_map(|ev| match ev {
			ProcessProgress::ItemCompleted { item } if item.stage == ProcessStage::AiContentMap => Some(item),
			_ => None,
		})
		.collect::<Vec<_>>();
	assert_eq!(completed_mapr_events.len(), 2);
	for item in &completed_mapr_events {
		let usage = item.usage.as_ref().ok_or("expected item usage")?;
		assert_eq!(usage.prompt_tokens, Some(100));
		assert_eq!(usage.completion_tokens, Some(50));
		assert_eq!(usage.total_tokens, Some(150));
	}

	let total_usage = output.total_usage.as_ref().ok_or("expected aggregate total_usage")?;
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

fn fixture_root(test_name: &str) -> Result<PathBuf> {
	let timestamp = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
	let root = PathBuf::from("tests-data/.tmp").join(format!("{test_name}-{}-{timestamp}", std::process::id()));
	fs::create_dir_all(&root)?;
	Ok(root)
}

fn path_text(path: &Path) -> String {
	path.to_string_lossy().replace('\\', "/")
}

// endregion: --- Support
