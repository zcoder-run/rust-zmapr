use std::path::Path;

use zmapr::{
	AiAugmentOptions, ContentMapOptions, FetchOptions, ProcessContentOptions, SanitizeOptions,
};

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>; // For tests.

#[test]
fn test_process_options_process_content_chainable_configuration() -> Result<()> {
	// -- Setup & Fixtures
	let options = ProcessContentOptions::new("tests-data/.tmp/options-destination")
		.with_fetch(FetchOptions::default().with_copy_local_files(true))
		.with_sanitize(
			SanitizeOptions::default()
				.with_slim_html(true)
				.with_convert_to_markdown(true),
		)
		.with_ai_augment(AiAugmentOptions::new("initial-provider", "initial-model"))
		.with_content_map(ContentMapOptions::new("map-provider", "map-model"))
		.with_resume(true)
		.with_max_concurrency(3);

	// -- Exec

	// -- Check
	assert_eq!(
		options.destination.as_std_path(),
		Path::new("tests-data/.tmp/options-destination")
	);
	assert!(options.resume);
	assert_eq!(options.max_concurrency, 3);

	let fetch = options
		.fetch
		.as_ref()
		.ok_or("Process options should contain Fetch options")?;
	assert!(fetch.copy_local_files);

	let sanitize = options
		.sanitize
		.as_ref()
		.ok_or("Process options should contain Sanitize options")?;
	assert!(sanitize.slim_html);
	assert!(sanitize.convert_to_markdown);

	let ai_augment = options
		.ai_augment
		.as_ref()
		.ok_or("Process options should contain AI Augment options")?;
	assert_eq!(ai_augment.provider, "initial-provider");
	assert_eq!(ai_augment.model, "initial-model");

	let content_map = options
		.content_map
		.as_ref()
		.ok_or("Process options should contain Content Map options")?;
	assert_eq!(content_map.provider, "map-provider");
	assert_eq!(content_map.model, "map-model");

	Ok(())
}

#[test]
fn test_process_options_fetch_chainable_collection_configuration() -> Result<()> {
	// -- Setup & Fixtures
	let options = FetchOptions::default()
		.with_copy_local_files(true)
		.with_same_host_only(true)
		.with_max_depth(4)
		.with_follow_links(true)
		.with_include(["**/*.md"])
		.append_include("README.md")
		.append_includes(["guide/*.md", "docs/*.md"])
		.with_exclude(["target/**"])
		.append_exclude("tmp/**")
		.append_excludes(["cache/**", "vendor/**"]);

	// -- Exec

	// -- Check
	assert!(options.copy_local_files);
	assert!(options.same_host_only);
	assert_eq!(options.max_depth, 4);
	assert!(options.follow_links);
	assert_eq!(
		options.include,
		vec![
			"**/*.md".to_owned(),
			"README.md".to_owned(),
			"guide/*.md".to_owned(),
			"docs/*.md".to_owned(),
		]
	);
	assert_eq!(
		options.exclude,
		vec![
			"target/**".to_owned(),
			"tmp/**".to_owned(),
			"cache/**".to_owned(),
			"vendor/**".to_owned(),
		]
	);

	Ok(())
}

#[test]
fn test_process_options_sanitize_chainable_configuration() -> Result<()> {
	// -- Setup & Fixtures
	let options = SanitizeOptions::default()
		.with_slim_html(true)
		.with_convert_to_markdown(true);

	// -- Exec

	// -- Check
	assert!(options.slim_html);
	assert!(options.convert_to_markdown);

	Ok(())
}

#[test]
fn test_process_options_ai_augment_chainable_configuration() -> Result<()> {
	// -- Setup & Fixtures
	let options = AiAugmentOptions::new("initial-provider", "initial-model")
		.with_provider("updated-provider")
		.with_model("updated-model");

	// -- Exec

	// -- Check
	assert_eq!(options.provider, "updated-provider");
	assert_eq!(options.model, "updated-model");

	Ok(())
}

#[test]
fn test_process_options_content_map_chainable_configuration() -> Result<()> {
	// -- Setup & Fixtures
	let options = ContentMapOptions::new("initial-provider", "initial-model")
		.with_provider("updated-provider")
		.with_model("updated-model")
		.with_journal_path("tests-data/.tmp/content-map.journal.jsonl")
		.with_reuse_unchanged_records(false)
		.with_retain_journal(false);

	// -- Exec

	// -- Check
	assert_eq!(options.provider, "updated-provider");
	assert_eq!(options.model, "updated-model");
	assert!(!options.reuse_unchanged_records);
	assert!(!options.retain_journal);

	let journal_path = options
		.journal_path
		.as_ref()
		.ok_or("Content Map options should contain a journal path")?;
	assert_eq!(
		journal_path.as_std_path(),
		Path::new("tests-data/.tmp/content-map.journal.jsonl")
	);

	Ok(())
}
