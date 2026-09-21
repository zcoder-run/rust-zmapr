use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use zmapr::{
	AiAugmentOptions, ContentMapOptions, Error, LocalFetchRequest, ProcessContentOptions, ProcessProgress,
	ProcessStage, WebFetchRequest, process_content,
};

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>; // For tests.

#[tokio::test]
async fn test_process_fetch_local_file_returns_output_and_progress() -> Result<()> {
	// -- Setup & Fixtures
	let root = fixture_root("test_process_fetch_local_file_returns_output_and_progress")?;
	let source_path = root.join("guide.md");
	fs::write(&source_path, b"# Guide\n")?;
	let destination = root.join("destination");

	// -- Exec
	let mut handle = process_content(local_fetch_options(&source_path, &destination, false, false)).await?;
	let _progress_rx = handle.take_progress_rx().ok_or("Fetch should provide a progress receiver")?;
	let output = handle.wait_output().await?;

	// -- Check
	assert_eq!(output.completed_items.len(), 1);
	assert!(output.skipped_items.is_empty());
	assert!(output.failures.is_empty());

	let item = output
		.completed_items
		.first()
		.ok_or("Fetch output should contain one completed item")?;
	assert_eq!(item.source, "guide.md");
	assert_eq!(item.stage, ProcessStage::Fetch);

	let output_path = item
		.output_path
		.as_ref()
		.ok_or("non-copying Fetch should retain an output path")?;
	let output_path: &Path = output_path.as_ref();
	assert_eq!(output_path, source_path.as_path());
	assert_eq!(fs::read(output_path)?, b"# Guide\n".to_vec());

	let content_root: &Path = output.content_root.as_ref();
	assert_eq!(content_root, source_path.as_path());

	let manifest_path = output.manifest_path.as_ref().ok_or("Fetch should publish a manifest")?;
	assert!(manifest_path.is_file());

	let manifest: serde_json::Value = serde_json::from_str(&fs::read_to_string(manifest_path.as_std_path())?)?;
	assert_eq!(
		manifest.get("complete").and_then(serde_json::Value::as_bool),
		Some(true)
	);
	assert!(output.content_map_path.is_none());

	Ok(())
}

#[tokio::test]
async fn test_process_fetch_copies_directory_artifacts_and_publishes_manifest() -> Result<()> {
	// -- Setup & Fixtures
	let root = fixture_root("test_process_fetch_copies_directory_artifacts_and_publishes_manifest")?;
	let source_root = root.join("source");
	let nested = source_root.join("nested");
	fs::create_dir_all(&nested)?;
	fs::write(source_root.join("a.txt"), b"alpha\n")?;
	fs::write(nested.join("b.txt"), b"beta\n")?;
	let destination = root.join("destination");

	// -- Exec
	let handle = process_content(local_fetch_options(&source_root, &destination, true, false)).await?;
	let output = handle.wait_output().await?;

	// -- Check
	assert_eq!(output.completed_items.len(), 2);
	assert!(output.skipped_items.is_empty());
	assert!(output.failures.is_empty());

	let sources = output
		.completed_items
		.iter()
		.map(|item| item.source.as_str())
		.collect::<Vec<_>>();
	assert_eq!(sources, vec!["a.txt", "nested/b.txt"]);

	let first = output
		.completed_items
		.first()
		.ok_or("copied Fetch output should contain the first item")?;
	let first_path = first
		.output_path
		.as_ref()
		.ok_or("copied Fetch item should have an output path")?;
	assert!(first_path.is_file());
	assert_eq!(fs::read(first_path.as_std_path())?, b"alpha\n".to_vec());

	let second = output
		.completed_items
		.get(1)
		.ok_or("copied Fetch output should contain the second item")?;
	let second_path = second
		.output_path
		.as_ref()
		.ok_or("copied Fetch item should have an output path")?;
	assert!(second_path.is_file());
	assert_eq!(fs::read(second_path.as_std_path())?, b"beta\n".to_vec());

	let expected_root = destination.join(".zmapr").join("fetch");
	let content_root: &Path = output.content_root.as_ref();
	assert_eq!(content_root, expected_root.as_path());

	let manifest_path = output.manifest_path.as_ref().ok_or("copied Fetch should publish a manifest")?;
	let manifest: serde_json::Value = serde_json::from_str(&fs::read_to_string(manifest_path.as_std_path())?)?;
	let manifest_items = manifest
		.get("items")
		.and_then(serde_json::Value::as_array)
		.ok_or("Fetch manifest should contain items")?;
	assert_eq!(manifest_items.len(), 2);

	Ok(())
}

#[tokio::test]
async fn test_process_fetch_resume_reuses_and_rebuilds_state() -> Result<()> {
	// -- Setup & Fixtures
	let root = fixture_root("test_process_fetch_resume_reuses_and_rebuilds_state")?;
	let source_path = root.join("source.txt");
	fs::write(&source_path, b"initial\n")?;
	let destination = root.join("destination");

	// -- Exec
	let first_handle = process_content(local_fetch_options(&source_path, &destination, true, false)).await?;
	let first_output = first_handle.wait_output().await?;

	// -- Check
	assert_eq!(first_output.completed_items.len(), 1);
	assert!(first_output.skipped_items.is_empty());
	assert!(first_output.failures.is_empty());

	let first_item = first_output
		.completed_items
		.first()
		.ok_or("initial Fetch should contain one item")?;
	let artifact_path = first_item
		.output_path
		.as_ref()
		.ok_or("initial Fetch item should have an artifact path")?
		.as_std_path()
		.to_path_buf();
	let manifest_path = first_output
		.manifest_path
		.as_ref()
		.ok_or("initial Fetch should publish a manifest")?
		.as_std_path()
		.to_path_buf();
	let original_manifest = fs::read(&manifest_path)?;
	let original_hash = manifest_hash(&manifest_path)?;

	let second_handle = process_content(local_fetch_options(&source_path, &destination, true, true)).await?;
	let second_output = second_handle.wait_output().await?;
	assert!(second_output.completed_items.is_empty());
	assert_eq!(second_output.skipped_items.len(), 1);
	assert_eq!(fs::read(&manifest_path)?, original_manifest);

	fs::remove_file(&artifact_path)?;

	let third_handle = process_content(local_fetch_options(&source_path, &destination, true, true)).await?;
	let third_output = third_handle.wait_output().await?;
	assert_eq!(third_output.completed_items.len(), 1);
	assert!(third_output.skipped_items.is_empty());
	assert!(artifact_path.is_file());

	fs::write(&source_path, b"changed\n")?;

	let fourth_handle = process_content(local_fetch_options(&source_path, &destination, true, true)).await?;
	let fourth_output = fourth_handle.wait_output().await?;
	assert_eq!(fourth_output.completed_items.len(), 1);
	assert!(fourth_output.skipped_items.is_empty());
	let changed_hash = manifest_hash(&manifest_path)?;
	assert_ne!(original_hash, changed_hash);

	Ok(())
}

#[tokio::test]
async fn test_process_fetch_invalid_local_source_returns_structured_error() -> Result<()> {
	// -- Setup & Fixtures
	let root = fixture_root("test_process_fetch_invalid_local_source_returns_structured_error")?;
	let missing_source = root.join("missing.txt");
	let destination = root.join("destination");

	// -- Exec
	let result = process_content(local_fetch_options(&missing_source, &destination, false, false)).await;

	// -- Check
	let error = match result {
		Err(error) => error,
		Ok(_) => return Err("invalid local source should fail before starting".into()),
	};
	assert!(matches!(error, Error::InvalidConfiguration(_)));
	assert!(!destination.exists());

	Ok(())
}

#[tokio::test]
async fn test_process_fetch_web_source_invalid_url_returns_structured_error() -> Result<()> {
	// -- Setup & Fixtures
	let root = fixture_root("test_process_fetch_web_source_invalid_url_returns_structured_error")?;
	let destination = root.join("destination");
	let options = ProcessContentOptions::new(path_text(&destination))
		.with_fetch(WebFetchRequest::new("not-a-valid-url").with_same_host_only(true));

	// -- Exec
	let result = process_content(options).await;

	// -- Check
	let error = match result {
		Err(error) => error,
		Ok(_) => return Err("invalid web source should fail before starting".into()),
	};
	assert!(matches!(error, Error::InvalidConfiguration(_)));
	assert!(!destination.exists());

	Ok(())
}

#[tokio::test]
async fn test_process_fetch_deferred_ai_augment_remains_unsupported() -> Result<()> {
	// -- Setup & Fixtures
	let root = fixture_root("test_process_fetch_deferred_ai_augment_remains_unsupported")?;
	let source_path = root.join("source.txt");
	fs::write(&source_path, b"source\n")?;
	let destination = root.join("destination");

	// -- Exec
	let fetch_handle = process_content(local_fetch_options(&source_path, &destination, true, false)).await?;
	let _ = fetch_handle.wait_output().await?;

	// -- Exec & Check
	let options = ProcessContentOptions::new(path_text(&destination))
		.with_ai_augment(AiAugmentOptions::new("test-provider", "test-model"));
	let handle = process_content(options).await?;
	let result = handle.wait_output().await;
	let error = match result {
		Err(error) => error,
		Ok(_) => return Err("deferred AI augment stage should not complete".into()),
	};
	assert!(matches!(error, Error::Unsupported(_)));

	Ok(())
}

#[tokio::test]
async fn test_process_fetch_web_crawls_and_reports_progress() -> Result<()> {
	// -- Setup & Fixtures
	let (port, _shutdown) = spawn_mock_server(|path| match path {
		"/site/" | "/site/index.html" => (
			"200 OK",
			"text/html; charset=utf-8",
			"<html><body><h1>Home</h1><a href=\"page1.html\">P1</a><a href=\"/site/page2.html\">P2</a><a href=\"http://example.com/out.html\">Out</a></body></html>",
		),
		"/site/page1.html" => (
			"200 OK",
			"text/html; charset=utf-8",
			"<html><body><h1>Page 1</h1><a href=\"sub/page3.html\">P3</a></body></html>",
		),
		"/site/page2.html" => (
			"200 OK",
			"text/html; charset=utf-8",
			"<html><body><h1>Page 2</h1></body></html>",
		),
		"/site/sub/page3.html" => (
			"200 OK",
			"text/html; charset=utf-8",
			"<html><body><h1>Page 3</h1></body></html>",
		),
		_ => ("404 Not Found", "text/plain", "Not Found"),
	})
	.await?;

	let root = fixture_root("test_process_fetch_web_crawls_and_reports_progress")?;
	let destination = root.join("destination");
	let start_url = format!("http://127.0.0.1:{port}/site/");

	let options = ProcessContentOptions::new(path_text(&destination)).with_fetch(
		WebFetchRequest::new(&start_url)
			.with_follow_links(true)
			.with_max_depth(2)
			.with_same_host_only(true),
	);

	// -- Exec
	let mut handle = process_content(options).await?;
	let mut progress_rx = handle.take_progress_rx().ok_or("Fetch should provide a progress receiver")?;

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
	assert_eq!(output.completed_items.len(), 4);
	assert!(output.failures.is_empty());

	let completed_sources = output
		.completed_items
		.iter()
		.map(|item| item.source.as_str())
		.collect::<Vec<_>>();
	assert_eq!(
		completed_sources,
		vec!["index.html", "page1.html", "page2.html", "sub/page3.html"]
	);

	let fetch_dir = destination.join(".zmapr").join("fetch");
	assert!(fetch_dir.join("index.html").is_file());
	assert!(fetch_dir.join("page1.html").is_file());
	assert!(fetch_dir.join("page2.html").is_file());
	assert!(fetch_dir.join("sub/page3.html").is_file());

	let manifest_path = output.manifest_path.as_ref().ok_or("Web fetch should publish a manifest")?;
	assert!(manifest_path.is_file());
	let manifest: serde_json::Value = serde_json::from_str(&fs::read_to_string(manifest_path.as_std_path())?)?;
	assert_eq!(
		manifest.get("complete").and_then(serde_json::Value::as_bool),
		Some(true)
	);

	let has_stage_started = progress_events.iter().any(|event| {
		matches!(
			event,
			ProcessProgress::StageStarted {
				stage: ProcessStage::Fetch
			}
		)
	});
	let has_stage_completed = progress_events.iter().any(|event| {
		matches!(
			event,
			ProcessProgress::StageCompleted {
				stage: ProcessStage::Fetch
			}
		)
	});
	let item_completed_count = progress_events
		.iter()
		.filter(|event| matches!(event, ProcessProgress::ItemCompleted { .. }))
		.count();

	assert!(has_stage_started);
	assert!(has_stage_completed);
	assert_eq!(item_completed_count, 4);

	Ok(())
}

#[tokio::test]
async fn test_process_fetch_web_respects_max_depth() -> Result<()> {
	// -- Setup & Fixtures
	let (port, _shutdown) = spawn_mock_server(|path| match path {
		"/docs/" | "/docs/index.html" => (
			"200 OK",
			"text/html; charset=utf-8",
			"<html><body><a href=\"level1.html\">L1</a></body></html>",
		),
		"/docs/level1.html" => (
			"200 OK",
			"text/html; charset=utf-8",
			"<html><body><a href=\"level2.html\">L2</a></body></html>",
		),
		"/docs/level2.html" => (
			"200 OK",
			"text/html; charset=utf-8",
			"<html><body><a href=\"level3.html\">L3</a></body></html>",
		),
		"/docs/level3.html" => ("200 OK", "text/html; charset=utf-8", "<html><body>End</body></html>"),
		_ => ("404 Not Found", "text/plain", "Not Found"),
	})
	.await?;

	let root = fixture_root("test_process_fetch_web_respects_max_depth")?;
	let destination = root.join("destination");
	let start_url = format!("http://127.0.0.1:{port}/docs/");

	let options = ProcessContentOptions::new(path_text(&destination)).with_fetch(
		WebFetchRequest::new(&start_url)
			.with_follow_links(true)
			.with_max_depth(1)
			.with_same_host_only(true),
	);

	// -- Exec
	let handle = process_content(options).await?;
	let output = handle.wait_output().await?;

	// -- Check
	assert_eq!(output.completed_items.len(), 2);
	assert!(output.failures.is_empty());

	let completed_sources = output
		.completed_items
		.iter()
		.map(|item| item.source.as_str())
		.collect::<Vec<_>>();
	assert_eq!(completed_sources, vec!["index.html", "level1.html"]);

	let fetch_dir = destination.join(".zmapr").join("fetch");
	assert!(fetch_dir.join("index.html").is_file());
	assert!(fetch_dir.join("level1.html").is_file());
	assert!(!fetch_dir.join("level2.html").exists());

	Ok(())
}

#[tokio::test]
async fn test_process_fetch_web_records_failures_for_broken_links() -> Result<()> {
	// -- Setup & Fixtures
	let (port, _shutdown) = spawn_mock_server(|path| match path {
		"/site/" | "/site/index.html" => (
			"200 OK",
			"text/html; charset=utf-8",
			"<html><body><a href=\"broken.html\">Broken</a></body></html>",
		),
		_ => ("404 Not Found", "text/plain", "Not Found"),
	})
	.await?;

	let root = fixture_root("test_process_fetch_web_records_failures_for_broken_links")?;
	let destination = root.join("destination");
	let start_url = format!("http://127.0.0.1:{port}/site/");

	let options = ProcessContentOptions::new(path_text(&destination)).with_fetch(
		WebFetchRequest::new(&start_url)
			.with_follow_links(true)
			.with_max_depth(1)
			.with_same_host_only(true),
	);

	// -- Exec
	let handle = process_content(options).await?;
	let output = handle.wait_output().await?;

	// -- Check
	assert_eq!(output.completed_items.len(), 1);
	assert_eq!(output.failures.len(), 1);

	let failure = output.failures.first().ok_or("Output should contain one failure")?;
	assert_eq!(failure.item.source, "broken.html");
	assert_eq!(failure.item.stage, ProcessStage::Fetch);
	assert!(failure.message.contains("404"));

	let manifest_path = output.manifest_path.as_ref().ok_or("Output should include a manifest")?;
	let manifest: serde_json::Value = serde_json::from_str(&fs::read_to_string(manifest_path.as_std_path())?)?;
	assert_eq!(
		manifest.get("complete").and_then(serde_json::Value::as_bool),
		Some(false)
	);

	Ok(())
}

#[tokio::test]
async fn test_process_fetch_web_llms_discovery_and_fetch() -> Result<()> {
	// -- Setup & Fixtures
	let (port, _shutdown) = spawn_mock_server(|path| match path {
		"/site/llms.txt" => (
			"200 OK",
			"text/plain",
			"# Site Docs\n\n- [Intro](intro.md): Introduction\n- [Architecture](concepts/arch.md): System architecture\n",
		),
		"/site/intro.md" => ("200 OK", "text/markdown", "# Introduction\n"),
		"/site/concepts/arch.md" => ("200 OK", "text/markdown", "# Architecture\n"),
		_ => ("404 Not Found", "text/plain", "Not Found"),
	})
	.await?;

	let root = fixture_root("test_process_fetch_web_llms_discovery_and_fetch")?;
	let destination = root.join("destination");
	let start_url = format!("http://127.0.0.1:{port}/site/");

	let options = ProcessContentOptions::new(path_text(&destination))
		.with_fetch(WebFetchRequest::new(&start_url).with_llms(true).with_same_host_only(true));

	// -- Exec
	let handle = process_content(options).await?;
	let output = handle.wait_output().await?;

	// -- Check
	assert_eq!(output.completed_items.len(), 3);
	assert!(output.failures.is_empty());

	let completed_sources = output
		.completed_items
		.iter()
		.map(|item| item.source.as_str())
		.collect::<Vec<_>>();
	assert_eq!(completed_sources, vec!["concepts/arch.md", "intro.md", "llms.txt"]);

	let fetch_dir = destination.join(".zmapr").join("fetch");
	assert!(fetch_dir.join("llms.txt").is_file());
	assert!(fetch_dir.join("intro.md").is_file());
	assert!(fetch_dir.join("concepts").join("arch.md").is_file());

	let manifest_path = output.manifest_path.as_ref().ok_or("Output should include a manifest")?;
	let manifest: serde_json::Value = serde_json::from_str(&fs::read_to_string(manifest_path.as_std_path())?)?;
	assert_eq!(
		manifest.get("complete").and_then(serde_json::Value::as_bool),
		Some(true)
	);
	let options_json = manifest.get("options").ok_or("Manifest should contain options")?;
	assert_eq!(
		options_json.get("llms").and_then(serde_json::Value::as_bool),
		Some(true)
	);

	Ok(())
}

#[tokio::test]
async fn test_process_fetch_web_llms_fallback_on_missing() -> Result<()> {
	// -- Setup & Fixtures
	let (port, _shutdown) = spawn_mock_server(|path| match path {
		"/site/llms.txt" => ("404 Not Found", "text/plain", "Not Found"),
		"/site/" | "/site/index.html" => (
			"200 OK",
			"text/html; charset=utf-8",
			"<html><body><a href=\"page1.html\">P1</a></body></html>",
		),
		"/site/page1.html" => (
			"200 OK",
			"text/html; charset=utf-8",
			"<html><body><h1>Page 1</h1></body></html>",
		),
		_ => ("404 Not Found", "text/plain", "Not Found"),
	})
	.await?;

	let root = fixture_root("test_process_fetch_web_llms_fallback_on_missing")?;
	let destination = root.join("destination");
	let start_url = format!("http://127.0.0.1:{port}/site/");

	let options = ProcessContentOptions::new(path_text(&destination)).with_fetch(
		WebFetchRequest::new(&start_url)
			.with_llms(true)
			.with_follow_links(true)
			.with_max_depth(1)
			.with_same_host_only(true),
	);

	// -- Exec
	let handle = process_content(options).await?;
	let output = handle.wait_output().await?;

	// -- Check
	assert_eq!(output.completed_items.len(), 2);
	assert!(output.failures.is_empty());

	let completed_sources = output
		.completed_items
		.iter()
		.map(|item| item.source.as_str())
		.collect::<Vec<_>>();
	assert_eq!(completed_sources, vec!["index.html", "page1.html"]);

	let fetch_dir = destination.join(".zmapr").join("fetch");
	assert!(fetch_dir.join("index.html").is_file());
	assert!(fetch_dir.join("page1.html").is_file());

	let manifest_path = output.manifest_path.as_ref().ok_or("Output should include a manifest")?;
	let manifest: serde_json::Value = serde_json::from_str(&fs::read_to_string(manifest_path.as_std_path())?)?;
	assert_eq!(
		manifest.get("complete").and_then(serde_json::Value::as_bool),
		Some(true)
	);

	Ok(())
}

#[tokio::test]
async fn test_process_fetch_web_llms_fallback_on_empty() -> Result<()> {
	// -- Setup & Fixtures
	let (port, _shutdown) = spawn_mock_server(|path| match path {
		"/site/llms.txt" => ("200 OK", "text/plain", ""),
		"/site/" | "/site/index.html" => (
			"200 OK",
			"text/html; charset=utf-8",
			"<html><body><a href=\"page1.html\">P1</a></body></html>",
		),
		"/site/page1.html" => (
			"200 OK",
			"text/html; charset=utf-8",
			"<html><body><h1>Page 1</h1></body></html>",
		),
		_ => ("404 Not Found", "text/plain", "Not Found"),
	})
	.await?;

	let root = fixture_root("test_process_fetch_web_llms_fallback_on_empty")?;
	let destination = root.join("destination");
	let start_url = format!("http://127.0.0.1:{port}/site/");

	let options = ProcessContentOptions::new(path_text(&destination)).with_fetch(
		WebFetchRequest::new(&start_url)
			.with_llms(true)
			.with_follow_links(true)
			.with_max_depth(1)
			.with_same_host_only(true),
	);

	// -- Exec
	let handle = process_content(options).await?;
	let output = handle.wait_output().await?;

	// -- Check
	assert_eq!(output.completed_items.len(), 2);
	assert!(output.failures.is_empty());

	let completed_sources = output
		.completed_items
		.iter()
		.map(|item| item.source.as_str())
		.collect::<Vec<_>>();
	assert_eq!(completed_sources, vec!["index.html", "page1.html"]);

	let fetch_dir = destination.join(".zmapr").join("fetch");
	assert!(fetch_dir.join("index.html").is_file());
	assert!(fetch_dir.join("page1.html").is_file());

	Ok(())
}

#[tokio::test]
async fn test_process_fetch_web_extensionless_path_defaults_html() -> Result<()> {
	// -- Setup & Fixtures
	let (port, _shutdown) = spawn_mock_server(|path| match path {
		"/site/" | "/site/index.html" => (
			"200 OK",
			"text/html; charset=utf-8",
			"<html><body><a href=\"intro\">Intro</a><a href=\"concepts/arch\">Arch</a></body></html>",
		),
		"/site/intro" => (
			"200 OK",
			"text/html; charset=utf-8",
			"<html><body><h1>Intro</h1></body></html>",
		),
		"/site/concepts/arch" => (
			"200 OK",
			"text/html; charset=utf-8",
			"<html><body><h1>Arch</h1></body></html>",
		),
		_ => ("404 Not Found", "text/plain", "Not Found"),
	})
	.await?;

	let root = fixture_root("test_process_fetch_web_extensionless_path_defaults_html")?;
	let destination = root.join("destination");
	let start_url = format!("http://127.0.0.1:{port}/site/");

	let options = ProcessContentOptions::new(path_text(&destination)).with_fetch(
		WebFetchRequest::new(&start_url)
			.with_follow_links(true)
			.with_max_depth(2)
			.with_same_host_only(true),
	);

	// -- Exec
	let handle = process_content(options).await?;
	let output = handle.wait_output().await?;

	// -- Check
	assert_eq!(output.completed_items.len(), 3);
	assert!(output.failures.is_empty());

	let completed_sources = output
		.completed_items
		.iter()
		.map(|item| item.source.as_str())
		.collect::<Vec<_>>();
	assert_eq!(
		completed_sources,
		vec!["concepts/arch.html", "index.html", "intro.html"]
	);

	let fetch_dir = destination.join(".zmapr").join("fetch");
	assert!(fetch_dir.join("index.html").is_file());
	assert!(fetch_dir.join("intro.html").is_file());
	assert!(fetch_dir.join("concepts").join("arch.html").is_file());
	assert!(!fetch_dir.join("intro").exists());
	assert!(!fetch_dir.join("concepts").join("arch").exists());

	Ok(())
}

// region:    --- Support

async fn spawn_mock_server<H>(handler: H) -> Result<(u16, tokio::sync::oneshot::Sender<()>)>
where
	H: Fn(&str) -> (&'static str, &'static str, &'static str) + Send + Sync + 'static,
{
	let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
	let port = listener.local_addr()?.port();
	let (shutdown_tx, mut shutdown_rx) = tokio::sync::oneshot::channel();
	let handler = std::sync::Arc::new(handler);

	tokio::spawn(async move {
		loop {
			tokio::select! {
				_ = &mut shutdown_rx => break,
				accepted = listener.accept() => {
					let Ok((mut socket, _)) = accepted else { break };
					let handler = handler.clone();
					tokio::spawn(async move {
						use tokio::io::{AsyncReadExt, AsyncWriteExt};
						let mut buf = vec![0u8; 2048];
						let n = socket.read(&mut buf).await.unwrap_or(0);
						let req = String::from_utf8_lossy(&buf[..n]);
						let first_line = req.lines().next().unwrap_or("");
						let path = first_line.split_whitespace().nth(1).unwrap_or("/");

						let (status, content_type, body) = handler(path);
						let resp = format!(
							"HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
							body.len()
						);
						let _ = socket.write_all(resp.as_bytes()).await;
					});
				}
			}
		}
	});

	Ok((port, shutdown_tx))
}

fn fixture_root(test_name: &str) -> Result<PathBuf> {
	let timestamp = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
	let root = PathBuf::from("tests-data/.tmp").join(format!("{test_name}-{}-{timestamp}", std::process::id()));
	fs::create_dir_all(&root)?;
	Ok(root)
}

fn local_fetch_options(
	source_path: &Path,
	destination: &Path,
	copy_local_files: bool,
	resume: bool,
) -> ProcessContentOptions {
	let mut options = ProcessContentOptions::new(path_text(destination))
		.with_fetch(LocalFetchRequest::new(path_text(source_path)).with_copy_local_files(copy_local_files));
	options.resume = resume;
	options
}

fn manifest_hash(path: &Path) -> Result<String> {
	let manifest: serde_json::Value = serde_json::from_str(&fs::read_to_string(path)?)?;
	let item = manifest
		.get("items")
		.and_then(serde_json::Value::as_array)
		.and_then(|items| items.first())
		.ok_or("Fetch manifest should contain one item")?;
	let hash = item
		.get("content_hash")
		.and_then(serde_json::Value::as_str)
		.ok_or("Fetch manifest item should contain a content hash")?;
	Ok(hash.to_owned())
}

fn path_text(path: &Path) -> String {
	path.to_string_lossy().replace('\\', "/")
}

// endregion: --- Support
