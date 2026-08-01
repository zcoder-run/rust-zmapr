use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use zmapr::{
	process_content, AiAugmentOptions, ContentMapOptions, ContentSource, Error, FetchOptions,
	ProcessContentOptions, ProcessStage,
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
	let mut handle = process_content(
		ContentSource::local_path(path_text(&source_path)),
		local_fetch_options(&destination, false, false),
	)
	.await?;
	let _progress_rx = handle
		.take_progress_rx()
		.ok_or("Fetch should provide a progress receiver")?;
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

	let manifest_path = output
		.manifest_path
		.as_ref()
		.ok_or("Fetch should publish a manifest")?;
	assert!(manifest_path.is_file());

	let manifest: serde_json::Value =
		serde_json::from_str(&fs::read_to_string(manifest_path.as_std_path())?)?;
	assert_eq!(
		manifest
			.get("complete")
			.and_then(serde_json::Value::as_bool),
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
	let handle = process_content(
		ContentSource::local_path(path_text(&source_root)),
		local_fetch_options(&destination, true, false),
	)
	.await?;
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

	let manifest_path = output
		.manifest_path
		.as_ref()
		.ok_or("copied Fetch should publish a manifest")?;
	let manifest: serde_json::Value =
		serde_json::from_str(&fs::read_to_string(manifest_path.as_std_path())?)?;
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
	let first_handle = process_content(
		ContentSource::local_path(path_text(&source_path)),
		local_fetch_options(&destination, true, false),
	)
	.await?;
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

	let second_handle = process_content(
		ContentSource::local_path(path_text(&source_path)),
		local_fetch_options(&destination, true, true),
	)
	.await?;
	let second_output = second_handle.wait_output().await?;
	assert!(second_output.completed_items.is_empty());
	assert_eq!(second_output.skipped_items.len(), 1);
	assert_eq!(fs::read(&manifest_path)?, original_manifest);

	fs::remove_file(&artifact_path)?;

	let third_handle = process_content(
		ContentSource::local_path(path_text(&source_path)),
		local_fetch_options(&destination, true, true),
	)
	.await?;
	let third_output = third_handle.wait_output().await?;
	assert_eq!(third_output.completed_items.len(), 1);
	assert!(third_output.skipped_items.is_empty());
	assert!(artifact_path.is_file());

	fs::write(&source_path, b"changed\n")?;

	let fourth_handle = process_content(
		ContentSource::local_path(path_text(&source_path)),
		local_fetch_options(&destination, true, true),
	)
	.await?;
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
	let result = process_content(
		ContentSource::local_path(path_text(&missing_source)),
		local_fetch_options(&destination, false, false),
	)
	.await;

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
async fn test_process_fetch_website_source_remains_unsupported() -> Result<()> {
	// -- Setup & Fixtures
	let root = fixture_root("test_process_fetch_website_source_remains_unsupported")?;
	let destination = root.join("destination");
	let options = ProcessContentOptions::new(path_text(&destination)).with_fetch(FetchOptions {
		same_host_only: true,
		..FetchOptions::default()
	});

	// -- Exec
	let result = process_content(ContentSource::website("https://example.com"), options).await;

	// -- Check
	assert!(matches!(result, Err(Error::Unsupported(_))));
	assert!(!destination.exists());

	Ok(())
}

#[tokio::test]
async fn test_process_fetch_deferred_ai_stages_remain_unsupported() -> Result<()> {
	// -- Setup & Fixtures
	let root = fixture_root("test_process_fetch_deferred_ai_stages_remain_unsupported")?;
	let source_path = root.join("source.txt");
	fs::write(&source_path, b"source\n")?;
	let destination = root.join("destination");

	// -- Exec
	let fetch_handle = process_content(
		ContentSource::local_path(path_text(&source_path)),
		local_fetch_options(&destination, true, false),
	)
	.await?;
	let _ = fetch_handle.wait_output().await?;

	// -- Exec & Check
	for options in [
		ProcessContentOptions::new(path_text(&destination))
			.with_ai_augment(AiAugmentOptions::new("test-provider", "test-model")),
		ProcessContentOptions::new(path_text(&destination))
			.with_content_map(ContentMapOptions::new("test-provider", "test-model")),
	] {
		let handle =
			process_content(ContentSource::local_path(path_text(&source_path)), options).await?;
		let result = handle.wait_output().await;
		let error = match result {
			Err(error) => error,
			Ok(_) => return Err("deferred AI stage should not complete".into()),
		};
		assert!(matches!(error, Error::Unsupported(_)));
	}

	Ok(())
}

// region:    --- Support

fn fixture_root(test_name: &str) -> Result<PathBuf> {
	let timestamp = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
	let root = PathBuf::from("tests-data/.tmp").join(format!(
		"{test_name}-{}-{timestamp}",
		std::process::id()
	));
	fs::create_dir_all(&root)?;
	Ok(root)
}

fn local_fetch_options(
	destination: &Path,
	copy_local_files: bool,
	resume: bool,
) -> ProcessContentOptions {
	let mut options =
		ProcessContentOptions::new(path_text(destination)).with_fetch(FetchOptions {
			copy_local_files,
			..FetchOptions::default()
		});
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
