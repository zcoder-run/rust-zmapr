use serde_json::json;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use zmapr::{
	Error, FetchFormat, ItemState, ItemStatus, ProcessContentOptions, ProcessStage, ProgressEvent, StageStatus,
	process_content,
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
	let mut handle = process_content(local_fetch_options(&source_path, &destination, false)).await?;
	let _progress_rx = handle.take_progress_rx().ok_or("Fetch should provide a progress receiver")?;
	let output = handle.wait_output().await?;

	// -- Check
	assert_eq!(output.stats.fetch.as_ref().ok_or("expected Fetch stats")?.completed, 1);
	assert_eq!(output.stats.fetch.as_ref().ok_or("expected Fetch stats")?.skipped, 0);
	assert_eq!(output.stats.fetch.as_ref().ok_or("expected Fetch stats")?.failed, 0);

	let item = output.items.first().ok_or("Fetch output should contain one completed item")?;
	assert_eq!(item.relative_path, "guide.md");
	assert_eq!(
		item.fetch.as_ref().map(|state| state.status),
		Some(ItemStatus::Completed)
	);

	let expected_fetch_root = destination.join(".tmp-zmapr").join("01-fetch");
	let fetch_state = item.fetch.as_ref().ok_or("Fetch should have Fetch state")?;
	let output_path = fetch_state.path.as_ref().ok_or("Fetch should publish an output path")?;
	let output_path: &Path = output_path.as_ref();
	assert_eq!(output_path, expected_fetch_root.join("guide.md").as_path());
	assert_eq!(fs::read(output_path)?, b"# Guide\n".to_vec());

	let content_root: &Path = output.content_root.as_ref();
	assert_eq!(content_root, expected_fetch_root.as_path());

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
	let handle = process_content(local_fetch_options(&source_root, &destination, false)).await?;
	let query = handle.query();
	let output = handle.wait_output().await?;

	// -- Check
	let stats = query.stats();
	assert_eq!(output.stats.fetch.as_ref().ok_or("expected Fetch stats")?.failed, 0);
	assert_eq!(stats.fetch.total_items, Some(2));
	assert_eq!(stats.fetch.completed, 2);
	assert_eq!(stats.fetch.status, StageStatus::Completed);

	let sources = relative_paths(&output.items, ProcessStage::Fetch, ItemStatus::Completed);
	assert_eq!(sources, vec!["a.txt", "nested/b.txt"]);

	let first = output
		.items
		.first()
		.ok_or("copied Fetch output should contain the first item")?;
	let first_path = first
		.fetch
		.as_ref()
		.and_then(|state| state.path.as_ref())
		.ok_or("copied Fetch item should have an output path")?;
	assert!(first_path.is_file());
	assert_eq!(fs::read(first_path.as_std_path())?, b"alpha\n".to_vec());

	let second = output
		.items
		.get(1)
		.ok_or("copied Fetch output should contain the second item")?;
	let second_path = second
		.fetch
		.as_ref()
		.and_then(|state| state.path.as_ref())
		.ok_or("copied Fetch item should have an output path")?;
	assert!(second_path.is_file());
	assert_eq!(fs::read(second_path.as_std_path())?, b"beta\n".to_vec());

	let expected_root = destination.join(".tmp-zmapr").join("01-fetch");
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
async fn test_process_fetch_local_excluded_count() -> Result<()> {
	// -- Setup & Fixtures
	let root = fixture_root("test_process_fetch_local_excluded_count")?;
	let source_root = root.join("source");
	fs::create_dir_all(&source_root)?;
	fs::write(source_root.join("keep.txt"), b"keep\n")?;
	fs::write(source_root.join("excluded.txt"), b"excluded\n")?;
	let destination = root.join("destination");

	// -- Exec
	let handle =
		process_content(local_fetch_options(&source_root, &destination, false).with_exclude(["excluded.txt"])).await?;
	let query = handle.query();
	let _output = handle.wait_output().await?;

	// -- Check
	let stats = query.stats();
	assert_eq!(stats.fetch.excluded, 1);
	assert_eq!(stats.fetch.total_items, Some(1));
	assert_eq!(stats.fetch.completed, 1);

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
	let first_handle = process_content(local_fetch_options(&source_path, &destination, false)).await?;
	let first_output = first_handle.wait_output().await?;

	// -- Check
	assert_eq!(
		first_output.stats.fetch.as_ref().ok_or("expected Fetch stats")?.completed,
		1
	);
	assert_eq!(
		first_output.stats.fetch.as_ref().ok_or("expected Fetch stats")?.failed,
		0
	);

	let first_item = first_output.items.first().ok_or("initial Fetch should contain one item")?;
	let artifact_path = first_item
		.fetch
		.as_ref()
		.and_then(|state| state.path.as_ref())
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

	let second_handle = process_content(local_fetch_options(&source_path, &destination, true)).await?;
	let second_query = second_handle.query();
	let second_output = second_handle.wait_output().await?;
	assert_eq!(
		second_output.stats.fetch.as_ref().ok_or("expected Fetch stats")?.reused,
		1
	);
	assert_eq!(fs::read(&manifest_path)?, original_manifest);
	assert_eq!(second_query.stats().fetch.reused, 1);
	assert_eq!(second_query.stats().fetch.completed, 0);

	fs::remove_file(&artifact_path)?;

	let third_handle = process_content(local_fetch_options(&source_path, &destination, true)).await?;
	let third_output = third_handle.wait_output().await?;
	assert_eq!(
		third_output.stats.fetch.as_ref().ok_or("expected Fetch stats")?.completed,
		1
	);
	assert!(artifact_path.is_file());

	fs::write(&source_path, b"changed\n")?;

	let fourth_handle = process_content(local_fetch_options(&source_path, &destination, true)).await?;
	let fourth_output = fourth_handle.wait_output().await?;
	assert_eq!(
		fourth_output.stats.fetch.as_ref().ok_or("expected Fetch stats")?.completed,
		1
	);
	let changed_hash = manifest_hash(&manifest_path)?;
	assert_ne!(original_hash, changed_hash);

	Ok(())
}

#[tokio::test]
async fn test_process_fetch_resume_rebuilds_legacy_cache_layout() -> Result<()> {
	// -- Setup & Fixtures
	let root = fixture_root("test_process_fetch_resume_rebuilds_legacy_cache_layout")?;
	let source_path = root.join("source.txt");
	fs::write(&source_path, b"source\n")?;
	let destination = root.join("destination");

	let first_handle = process_content(local_fetch_options(&source_path, &destination, false)).await?;
	let first_output = first_handle.wait_output().await?;
	let first_item = first_output.items.first().ok_or("initial Fetch should contain one item")?;
	let artifact_path = first_item
		.fetch
		.as_ref()
		.and_then(|state| state.path.as_ref())
		.ok_or("initial Fetch item should have an artifact path")?
		.as_std_path()
		.to_path_buf();
	let legacy_fetch_cache = destination.join(".tmp-zmapr").join("fetch");
	fs::create_dir_all(&legacy_fetch_cache)?;
	let legacy_artifact_path = legacy_fetch_cache.join("source.txt");
	fs::copy(&artifact_path, &legacy_artifact_path)?;

	let manifest_path = first_output
		.manifest_path
		.as_ref()
		.ok_or("initial Fetch should publish a manifest")?
		.as_std_path()
		.to_path_buf();
	let mut manifest: serde_json::Value = serde_json::from_slice(&fs::read(&manifest_path)?)?;
	manifest["artifact_root"] = json!(path_text(&legacy_fetch_cache));
	manifest["items"][0]["artifact_path"] = json!(path_text(&legacy_artifact_path));
	fs::write(&manifest_path, serde_json::to_vec(&manifest)?)?;
	fs::remove_file(&artifact_path)?;

	// -- Exec
	let second_handle = process_content(local_fetch_options(&source_path, &destination, true)).await?;
	let second_output = second_handle.wait_output().await?;

	// -- Check
	let second_fetch_stats = second_output.stats.fetch.as_ref().ok_or("expected Fetch stats")?;
	assert_eq!(second_fetch_stats.completed, 1);
	assert_eq!(second_fetch_stats.reused, 0);
	assert_eq!(second_fetch_stats.skipped, 0);
	assert_eq!(second_fetch_stats.failed, 0);

	let item = second_output
		.items
		.first()
		.ok_or("Fetch should rebuild the artifact in the numbered cache")?;
	let output_path = item
		.fetch
		.as_ref()
		.and_then(|state| state.path.as_ref())
		.ok_or("rebuilt Fetch item should have an output path")?;
	let expected_artifact_path = destination.join(".tmp-zmapr").join("01-fetch").join("source.txt");
	let output_path: &Path = output_path.as_ref();
	assert_eq!(output_path, expected_artifact_path.as_path());
	assert!(expected_artifact_path.is_file());

	let updated_manifest: serde_json::Value = serde_json::from_slice(&fs::read(&manifest_path)?)?;
	let expected_artifact_root = path_text(&destination.join(".tmp-zmapr").join("01-fetch"));
	assert_eq!(
		updated_manifest.get("artifact_root").and_then(serde_json::Value::as_str),
		Some(expected_artifact_root.as_str())
	);

	Ok(())
}

#[tokio::test]
async fn test_process_fetch_without_fetch_rejects_legacy_cache_layout() -> Result<()> {
	// -- Setup & Fixtures
	let root = fixture_root("test_process_fetch_without_fetch_rejects_legacy_cache_layout")?;
	let source_path = root.join("source.txt");
	fs::write(&source_path, b"source\n")?;
	let destination = root.join("destination");
	let metadata_root = destination.join(".tmp-zmapr");
	let legacy_fetch_cache = metadata_root.join("fetch");
	fs::create_dir_all(&metadata_root)?;

	let manifest = json!({
		"version": 4,
		"complete": true,
		"source": path_text(&source_path),
		"source_path": path_text(&source_path),
		"options": {
			"type": "local",
			"include": [],
			"exclude": [],
			"format": "md"
		},
		"artifact_root": path_text(&legacy_fetch_cache),
		"items": []
	});
	fs::write(metadata_root.join("manifest.json"), serde_json::to_vec(&manifest)?)?;

	let options = ProcessContentOptions::new(path_text(&destination))
		.with_sanitize(true)
		.with_model("test-model");

	// -- Exec
	let handle = process_content(options).await?;
	let result = handle.wait_output().await;

	// -- Check
	let error = match result {
		Err(error) => error,
		Ok(_) => return Err("legacy Fetch cache should be rejected when Fetch is disabled".into()),
	};
	let message = match error {
		Error::MalformedState(message) => message,
		_ => return Err("legacy Fetch cache should return a malformed-state error".into()),
	};
	assert!(message.contains("rerun Fetch"));

	Ok(())
}

#[tokio::test]
async fn test_process_fetch_binary_file_hashes_and_resumes() -> Result<()> {
	// -- Setup & Fixtures
	let root = fixture_root("test_process_fetch_binary_file_hashes_and_resumes")?;
	let source_path = root.join("binary.bin");
	let contents = [0, 0xff, 0x80, 0x42];
	fs::write(&source_path, contents)?;
	let destination = root.join("destination");

	// -- Exec
	let first_handle = process_content(local_fetch_options(&source_path, &destination, false)).await?;
	let first_output = first_handle.wait_output().await?;
	let manifest_path = first_output.manifest_path.as_ref().ok_or("Fetch should publish a manifest")?;
	let actual_hash = manifest_hash(manifest_path.as_std_path())?;

	let second_handle = process_content(local_fetch_options(&source_path, &destination, true)).await?;
	let second_output = second_handle.wait_output().await?;

	// -- Check
	assert_eq!(
		first_output.stats.fetch.as_ref().ok_or("expected Fetch stats")?.completed,
		1
	);
	assert_eq!(
		first_output.stats.fetch.as_ref().ok_or("expected Fetch stats")?.failed,
		0
	);
	let item = first_output.items.first().ok_or("Fetch should complete the binary file")?;
	let artifact_path = item
		.fetch
		.as_ref()
		.and_then(|state| state.path.as_ref())
		.ok_or("Fetch should copy the binary file")?;
	assert_eq!(fs::read(artifact_path.as_std_path())?, contents);
	assert_eq!(
		actual_hash,
		bs58::encode(blake3::hash(&contents).as_bytes()).into_string()
	);
	assert_eq!(
		second_output.stats.fetch.as_ref().ok_or("expected Fetch stats")?.reused,
		1
	);
	assert_eq!(
		second_output.stats.fetch.as_ref().ok_or("expected Fetch stats")?.failed,
		0
	);

	Ok(())
}

#[tokio::test]
async fn test_process_fetch_invalid_local_source_returns_structured_error() -> Result<()> {
	// -- Setup & Fixtures
	let root = fixture_root("test_process_fetch_invalid_local_source_returns_structured_error")?;
	let missing_source = root.join("missing.txt");
	let destination = root.join("destination");

	// -- Exec
	let result = process_content(local_fetch_options(&missing_source, &destination, false)).await;

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
	let options = ProcessContentOptions::new(path_text(&destination)).with_source("not-a-valid-url");

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
async fn test_process_fetch_sanitize_without_model_returns_validation_error() -> Result<()> {
	// -- Setup & Fixtures
	let root = fixture_root("test_process_fetch_sanitize_without_model_returns_validation_error")?;
	let destination = root.join("destination");

	// -- Exec
	let options = ProcessContentOptions::new(path_text(&destination)).with_sanitize(true);
	let error = process_content(options).await.err().ok_or("Expected validation error")?;

	// -- Check
	assert!(matches!(error, Error::InvalidConfiguration(_)));
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

	let options = ProcessContentOptions::new(path_text(&destination))
		.with_source(&start_url)
		.with_max_depth(2);

	// -- Exec
	let mut handle = process_content(options).await?;
	let query = handle.query();
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
	let stats = query.stats();
	assert_eq!(output.stats.fetch.as_ref().ok_or("expected Fetch stats")?.completed, 4);
	assert_eq!(output.stats.fetch.as_ref().ok_or("expected Fetch stats")?.failed, 0);
	assert_eq!(stats.fetch.total_items, Some(4));
	assert_eq!(stats.fetch.completed, 4);

	let page1 = query.item_by_path("page1.md").ok_or("Fetch registry should contain page1.md")?;
	let page1_fetch = page1.fetch.as_ref().ok_or("page1.md should have Fetch state")?;
	assert_eq!(page1_fetch.status, ItemStatus::Completed);
	assert_eq!(page1.origin_path, "page1.html");

	let completed_sources = relative_paths(&output.items, ProcessStage::Fetch, ItemStatus::Completed);
	assert_eq!(
		completed_sources,
		vec!["index.md", "page1.md", "page2.md", "sub/page3.md"]
	);

	let fetch_dir = destination.join(".tmp-zmapr").join("01-fetch");
	assert!(fetch_dir.join("index.md").is_file());
	assert!(fetch_dir.join("page1.md").is_file());
	assert!(fetch_dir.join("page2.md").is_file());
	assert!(fetch_dir.join("sub/page3.md").is_file());

	let manifest_path = output.manifest_path.as_ref().ok_or("Web fetch should publish a manifest")?;
	assert!(manifest_path.is_file());
	let manifest: serde_json::Value = serde_json::from_str(&fs::read_to_string(manifest_path.as_std_path())?)?;
	assert_eq!(
		manifest.get("complete").and_then(serde_json::Value::as_bool),
		Some(true)
	);

	let has_stage_started = progress_events.iter().any(|update| {
		matches!(
			&update.event,
			ProgressEvent::StageStarted {
				stage: ProcessStage::Fetch
			}
		)
	});
	let has_stage_completed = progress_events.iter().any(|update| {
		matches!(
			&update.event,
			ProgressEvent::StageCompleted {
				stage: ProcessStage::Fetch
			}
		)
	});
	let item_completed_count = progress_events
		.iter()
		.filter(|update| {
			matches!(
				&update.event,
				ProgressEvent::ItemStatusChanged {
					stage: ProcessStage::Fetch,
					status: ItemStatus::Completed,
					..
				}
			)
		})
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

	let options = ProcessContentOptions::new(path_text(&destination))
		.with_source(&start_url)
		.with_max_depth(1);

	// -- Exec
	let handle = process_content(options).await?;
	let output = handle.wait_output().await?;

	// -- Check
	assert_eq!(output.stats.fetch.as_ref().ok_or("expected Fetch stats")?.completed, 2);
	assert_eq!(output.stats.fetch.as_ref().ok_or("expected Fetch stats")?.failed, 0);

	let completed_sources = relative_paths(&output.items, ProcessStage::Fetch, ItemStatus::Completed);
	assert_eq!(completed_sources, vec!["index.md", "level1.md"]);

	let fetch_dir = destination.join(".tmp-zmapr").join("01-fetch");
	assert!(fetch_dir.join("index.md").is_file());
	assert!(fetch_dir.join("level1.md").is_file());
	assert!(!fetch_dir.join("level2.md").exists());

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

	let options = ProcessContentOptions::new(path_text(&destination))
		.with_source(&start_url)
		.with_max_depth(1);

	// -- Exec
	let handle = process_content(options).await?;
	let query = handle.query();
	let output = handle.wait_output().await?;

	// -- Check
	let stats = query.stats();
	assert_eq!(output.stats.fetch.as_ref().ok_or("expected Fetch stats")?.completed, 1);
	assert_eq!(output.stats.fetch.as_ref().ok_or("expected Fetch stats")?.failed, 1);
	assert_eq!(stats.fetch.failed, 1);

	let broken = query
		.item_by_path("broken.html")
		.ok_or("Fetch registry should contain the failed broken.html item")?;
	assert_eq!(broken.relative_path, "broken.html");
	let broken_fetch = broken.fetch.as_ref().ok_or("broken.html should have Fetch state")?;
	assert_eq!(broken_fetch.status, ItemStatus::Failed);
	assert!(broken_fetch.error.as_deref().is_some_and(|message| message.contains("404")));

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
		.with_source(&start_url)
		.with_llms(true);

	// -- Exec
	let handle = process_content(options).await?;
	let output = handle.wait_output().await?;

	// -- Check
	assert_eq!(output.stats.fetch.as_ref().ok_or("expected Fetch stats")?.completed, 3);
	assert_eq!(output.stats.fetch.as_ref().ok_or("expected Fetch stats")?.failed, 0);

	let completed_sources = relative_paths(&output.items, ProcessStage::Fetch, ItemStatus::Completed);
	assert_eq!(completed_sources, vec!["concepts/arch.md", "intro.md", "llms.txt"]);

	let fetch_dir = destination.join(".tmp-zmapr").join("01-fetch");
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

	let options = ProcessContentOptions::new(path_text(&destination))
		.with_source(&start_url)
		.with_llms(true)
		.with_max_depth(1);

	// -- Exec
	let handle = process_content(options).await?;
	let output = handle.wait_output().await?;

	// -- Check
	assert_eq!(output.stats.fetch.as_ref().ok_or("expected Fetch stats")?.completed, 2);
	assert_eq!(output.stats.fetch.as_ref().ok_or("expected Fetch stats")?.failed, 0);

	let completed_sources = relative_paths(&output.items, ProcessStage::Fetch, ItemStatus::Completed);
	assert_eq!(completed_sources, vec!["index.md", "page1.md"]);

	let fetch_dir = destination.join(".tmp-zmapr").join("01-fetch");
	assert!(fetch_dir.join("index.md").is_file());
	assert!(fetch_dir.join("page1.md").is_file());

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

	let options = ProcessContentOptions::new(path_text(&destination))
		.with_source(&start_url)
		.with_llms(true)
		.with_max_depth(1);

	// -- Exec
	let handle = process_content(options).await?;
	let output = handle.wait_output().await?;

	// -- Check
	assert_eq!(output.stats.fetch.as_ref().ok_or("expected Fetch stats")?.completed, 2);
	assert_eq!(output.stats.fetch.as_ref().ok_or("expected Fetch stats")?.failed, 0);

	let completed_sources = relative_paths(&output.items, ProcessStage::Fetch, ItemStatus::Completed);
	assert_eq!(completed_sources, vec!["index.md", "page1.md"]);

	let fetch_dir = destination.join(".tmp-zmapr").join("01-fetch");
	assert!(fetch_dir.join("index.md").is_file());
	assert!(fetch_dir.join("page1.md").is_file());

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

	let options = ProcessContentOptions::new(path_text(&destination))
		.with_source(&start_url)
		.with_max_depth(2);

	// -- Exec
	let handle = process_content(options).await?;
	let output = handle.wait_output().await?;

	// -- Check
	assert_eq!(output.stats.fetch.as_ref().ok_or("expected Fetch stats")?.completed, 3);
	assert_eq!(output.stats.fetch.as_ref().ok_or("expected Fetch stats")?.failed, 0);

	let completed_sources = relative_paths(&output.items, ProcessStage::Fetch, ItemStatus::Completed);
	assert_eq!(completed_sources, vec!["concepts/arch.md", "index.md", "intro.md"]);

	let fetch_dir = destination.join(".tmp-zmapr").join("01-fetch");
	assert!(fetch_dir.join("index.md").is_file());
	assert!(fetch_dir.join("intro.md").is_file());
	assert!(fetch_dir.join("concepts").join("arch.md").is_file());
	assert!(!fetch_dir.join("intro.html").exists());
	assert!(!fetch_dir.join("concepts").join("arch.html").exists());

	Ok(())
}

#[tokio::test]
async fn test_process_fetch_local_html_formats_and_path_collisions() -> Result<()> {
	// -- Setup & Fixtures
	let root = fixture_root("test_process_fetch_local_html_formats_and_path_collisions")?;
	let source_root = root.join("source");
	fs::create_dir_all(&source_root)?;
	fs::write(source_root.join("page.html"), "<html><body><h1>Page</h1></body></html>")?;
	fs::write(source_root.join("page.md"), "# Existing Markdown\n")?;

	// -- Exec
	let markdown_destination = root.join("markdown");
	let markdown_handle = process_content(
		ProcessContentOptions::new(path_text(&markdown_destination)).with_source(path_text(&source_root)),
	)
	.await?;
	let markdown_output = markdown_handle.wait_output().await?;

	let raw_destination = root.join("raw");
	let raw_handle = process_content(
		ProcessContentOptions::new(path_text(&raw_destination))
			.with_source(path_text(&source_root))
			.with_format(FetchFormat::Raw),
	)
	.await?;
	let raw_output = raw_handle.wait_output().await?;

	let slim_destination = root.join("slim");
	let slim_handle = process_content(
		ProcessContentOptions::new(path_text(&slim_destination))
			.with_source(path_text(&source_root))
			.with_format(FetchFormat::Slim),
	)
	.await?;
	let slim_output = slim_handle.wait_output().await?;

	// -- Check
	assert_eq!(
		markdown_output.stats.fetch.as_ref().ok_or("expected Fetch stats")?.failed,
		2
	);
	assert!(
		markdown_output
			.items
			.iter()
			.filter_map(|item| item.fetch.as_ref())
			.all(|state| {
				state.status != ItemStatus::Failed
					|| state.error.as_deref().is_some_and(|message| {
						message.contains("multiple input artifacts resolve to fetch path page.md")
					})
			})
	);
	assert!(
		!markdown_destination
			.join(".tmp-zmapr")
			.join("01-fetch")
			.join("page.md")
			.exists()
	);

	assert_eq!(raw_output.stats.fetch.as_ref().ok_or("expected Fetch stats")?.failed, 0);
	assert!(raw_destination.join(".tmp-zmapr").join("01-fetch").join("page.html").is_file());

	assert_eq!(
		slim_output.stats.fetch.as_ref().ok_or("expected Fetch stats")?.failed,
		0
	);
	let slim_path = slim_destination.join(".tmp-zmapr").join("01-fetch").join("page.html");
	assert!(slim_path.is_file());
	assert!(fs::read_to_string(slim_path)?.contains("Page"));

	Ok(())
}

#[tokio::test]
async fn test_process_fetch_final_stats_match_query() -> Result<()> {
	// -- Setup & Fixtures
	let root = fixture_root("test_process_fetch_final_stats_match_query")?;
	let source_root = root.join("source");
	fs::create_dir_all(&source_root)?;
	fs::write(source_root.join("a.txt"), b"a\n")?;
	fs::write(source_root.join("b.txt"), b"b\n")?;
	let destination = root.join("destination");

	// -- Exec
	let mut handle = process_content(local_fetch_options(&source_root, &destination, false)).await?;
	let query = handle.query();
	let mut progress_rx = handle.take_progress_rx().ok_or("Fetch should provide a progress receiver")?;
	let progress_task = tokio::spawn(async move {
		let mut updates = Vec::new();
		while let Ok(update) = progress_rx.recv().await {
			updates.push(update);
		}
		updates
	});
	let output = handle.wait_output().await?;
	let updates = progress_task.await?;

	// -- Check
	let fetch_stats = output.stats.fetch.as_ref().ok_or("expected final Fetch stats")?;
	assert_eq!(fetch_stats.total_items, 2);
	assert!(output.stats.sanitize.is_none());
	assert!(output.stats.map.is_none());
	assert!(output.stats.duration() >= std::time::Duration::ZERO);
	assert_eq!(output.items.len(), query.items().len());
	assert!(updates.windows(2).all(|pair| pair[0].seq < pair[1].seq));
	assert!(matches!(
		updates.last().map(|update| &update.event),
		Some(ProgressEvent::WorkflowCompleted)
	));

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

fn local_fetch_options(source_path: &Path, destination: &Path, resume: bool) -> ProcessContentOptions {
	ProcessContentOptions::new(path_text(destination))
		.with_source(path_text(source_path))
		.with_resume(resume)
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
