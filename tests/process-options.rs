use std::path::Path;
use zmapr::{
	AiAugmentOptions, ContentMapOptions, FetchRequest, LocalFetchRequest, ProcessContentOptions, SanitizeOptions,
	WebFetchRequest,
};

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>; // For tests.

#[test]
fn test_process_options_process_content_chainable_configuration() -> Result<()> {
	// -- Setup & Fixtures
	let options = ProcessContentOptions::new("tests-data/.tmp/options-destination")
		.with_fetch(LocalFetchRequest::new("tests-data/source").with_copy_local_files(true))
		.with_sanitize(SanitizeOptions::default().with_slim_html(true).with_convert_to_markdown(true))
		.with_ai_augment(AiAugmentOptions::new("initial-provider", "initial-model"))
		.with_content_map(ContentMapOptions::new("map-model"))
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

	let fetch = options.fetch.as_ref().ok_or("Process options should contain Fetch options")?;
	if let FetchRequest::Local(local) = fetch {
		assert!(local.options.copy_local_files);
		assert_eq!(local.source.path.to_string(), "tests-data/source");
	} else {
		return Err("Expected Local fetch request variant".into());
	}

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
	assert_eq!(content_map.model, "map-model");

	Ok(())
}

#[test]
fn test_process_options_local_fetch_request_chainable_configuration() -> Result<()> {
	// -- Setup & Fixtures
	let request = LocalFetchRequest::new("tests-data/source")
		.with_copy_local_files(true)
		.with_include(["**/*.md"])
		.append_include("README.md")
		.append_includes(["guide/*.md", "docs/*.md"])
		.with_exclude(["target/**"])
		.append_exclude("tmp/**")
		.append_excludes(["cache/**", "vendor/**"]);

	// -- Exec

	// -- Check
	assert_eq!(request.source.path.to_string(), "tests-data/source");
	assert!(request.options.copy_local_files);
	assert_eq!(
		request.common.include,
		vec![
			"**/*.md".to_owned(),
			"README.md".to_owned(),
			"guide/*.md".to_owned(),
			"docs/*.md".to_owned(),
		]
	);
	assert_eq!(
		request.common.exclude,
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
fn test_process_options_web_fetch_request_chainable_configuration() -> Result<()> {
	// -- Setup & Fixtures
	let request = WebFetchRequest::new("https://example.com/docs")
		.with_same_host_only(true)
		.with_follow_links(true)
		.with_max_depth(3)
		.with_llms(true)
		.with_include(["**/*.html"])
		.append_include("api/**/*.html")
		.with_exclude(["**/deprecated/**"])
		.append_exclude("**/internal/**");

	// -- Exec

	// -- Check
	assert_eq!(request.source.url, "https://example.com/docs");
	assert!(request.options.same_host_only);
	assert!(request.options.follow_links);
	assert_eq!(request.options.max_depth, 3);
	assert_eq!(request.options.llms, Some(true));
	assert_eq!(
		request.common.include,
		vec!["**/*.html".to_owned(), "api/**/*.html".to_owned()]
	);
	assert_eq!(
		request.common.exclude,
		vec!["**/deprecated/**".to_owned(), "**/internal/**".to_owned()]
	);

	Ok(())
}

#[test]
fn test_process_options_sanitize_chainable_configuration() -> Result<()> {
	// -- Setup & Fixtures
	let options = SanitizeOptions::default().with_slim_html(true).with_convert_to_markdown(true);

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
	let options = ContentMapOptions::new("initial-model")
		.with_model("updated-model")
		.with_journal_path("tests-data/.tmp/content-map.journal.jsonl")
		.with_reuse_unchanged_records(false)
		.with_retain_journal(false)
		.with_max_size(100_000)
		.with_max_cost(2.5);

	// -- Exec

	// -- Check
	assert_eq!(options.model, "updated-model");
	assert!(!options.reuse_unchanged_records);
	assert!(!options.retain_journal);
	assert_eq!(options.max_size, Some(100_000));
	assert_eq!(options.max_cost, Some(2.5));

	let journal_path = options
		.journal_path
		.as_ref()
		.ok_or("Content Map options should contain a journal path")?;
	assert_eq!(
		journal_path.as_std_path(),
		Path::new("tests-data/.tmp/content-map.journal.jsonl")
	);

	let default_options = ContentMapOptions::new("initial-model");
	assert_eq!(default_options.max_size, Some(200_000));
	assert_eq!(default_options.max_cost, None);

	Ok(())
}

#[tokio::test]
async fn test_process_options_content_map_validation_zero_max_size() -> Result<()> {
	// -- Setup & Fixtures
	let options = ProcessContentOptions::new("tests-data/.tmp/invalid-max-size")
		.with_content_map(ContentMapOptions::new("map-model").with_max_size(0));

	// -- Exec
	let err = zmapr::process_content(options).await.err().ok_or("Expected validation error")?;

	// -- Check
	assert!(matches!(err, zmapr::Error::InvalidConfiguration(_)));

	Ok(())
}

#[tokio::test]
async fn test_process_options_content_map_validation_negative_max_cost() -> Result<()> {
	// -- Setup & Fixtures
	let options = ProcessContentOptions::new("tests-data/.tmp/invalid-max-cost")
		.with_content_map(ContentMapOptions::new("map-model").with_max_cost(-1.0));

	// -- Exec
	let err = zmapr::process_content(options).await.err().ok_or("Expected validation error")?;

	// -- Check
	assert!(matches!(err, zmapr::Error::InvalidConfiguration(_)));

	Ok(())
}

#[test]
fn test_process_options_content_map_to_md_chainable_configuration() -> Result<()> {
	// -- Setup & Fixtures
	let default_options = ContentMapOptions::new("map-model");
	let enabled_options = ContentMapOptions::new("map-model").with_to_md(true);
	let disabled_options = ContentMapOptions::new("map-model").with_to_md(false);

	// -- Exec

	// -- Check
	assert_eq!(default_options.to_md, None);
	assert_eq!(enabled_options.to_md, Some(true));
	assert_eq!(disabled_options.to_md, Some(false));

	Ok(())
}
