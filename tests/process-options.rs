use zmapr::{FetchFormat, ProcessContentOptions, SanitizePrompt};

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>; // For tests.

#[test]
fn test_process_options_defaults() -> Result<()> {
	// -- Setup & Fixtures
	let options = ProcessContentOptions::new("tests-data/.tmp/options-source");

	// -- Check
	assert_eq!(options.source, "tests-data/.tmp/options-source");
	assert!(options.destination.is_none());
	assert!(options.fetch);
	assert!(options.include.is_empty());
	assert!(options.exclude.is_empty());
	assert_eq!(options.format, FetchFormat::Md);
	assert_eq!(options.max_depth, 0);
	assert!(options.llms);
	assert!(!options.sanitize);
	assert!(!options.map);
	assert_eq!(options.model, None);
	assert_eq!(options.sanitize_model, None);
	assert_eq!(options.map_model, None);
	assert!(options.sanitize_prompt.is_none());
	assert!(!options.resume);
	assert_eq!(options.concurrency, 8);

	Ok(())
}

#[test]
fn test_process_options_flat_chainable_configuration() -> Result<()> {
	// -- Setup & Fixtures
	let options = ProcessContentOptions::new("docs")
		.with_dest("tests-data/.tmp/options-destination")
		.with_include(["**/*.md"])
		.append_include("README.md")
		.append_includes(["guide/*.md", "docs/*.md"])
		.with_exclude(["target/**"])
		.append_exclude("tmp/**")
		.append_excludes(["cache/**", "vendor/**"])
		.with_format(FetchFormat::Slim)
		.with_max_depth(3)
		.with_llms(false)
		.with_sanitize(true)
		.with_map(true)
		.with_model("default-model")
		.with_sanitize_model("sanitize-model")
		.with_map_model("map-model")
		.with_sanitize_prompt(SanitizePrompt::content("Custom instructions"))
		.with_resume(true)
		.with_concurrency(3);

	// -- Check
	assert_eq!(options.source, "docs");
	assert!(options.destination.is_some());
	assert_eq!(options.include, vec!["**/*.md", "README.md", "guide/*.md", "docs/*.md"]);
	assert_eq!(options.exclude, vec!["target/**", "tmp/**", "cache/**", "vendor/**"]);
	assert_eq!(options.format, FetchFormat::Slim);
	assert_eq!(options.max_depth, 3);
	assert!(!options.llms);
	assert!(options.sanitize);
	assert!(options.map);
	assert_eq!(options.sanitize_model.as_deref(), Some("sanitize-model"));
	assert_eq!(options.map_model.as_deref(), Some("map-model"));
	assert!(options.resume);
	assert_eq!(options.concurrency, 3);

	Ok(())
}

#[test]
fn test_process_options_default_model_is_stored() -> Result<()> {
	// -- Setup & Fixtures
	let options = ProcessContentOptions::new("source").with_model("default-model");

	// -- Check
	assert_eq!(options.model.as_deref(), Some("default-model"));
	assert!(options.sanitize_model.is_none());
	assert!(options.map_model.is_none());

	Ok(())
}

#[tokio::test]
async fn test_process_options_validation_no_enabled_stage() -> Result<()> {
	// -- Setup & Fixtures
	let options = ProcessContentOptions::new("source")
		.with_dest("tests-data/.tmp/no-stage")
		.with_fetch(false);

	// -- Exec
	let err = zmapr::process_content(options).await.err().ok_or("Expected validation error")?;

	// -- Check
	assert!(matches!(err, zmapr::Error::InvalidConfiguration(_)));

	Ok(())
}

#[tokio::test]
async fn test_process_options_validation_zero_concurrency() -> Result<()> {
	// -- Setup & Fixtures
	let options = ProcessContentOptions::new("source")
		.with_dest("tests-data/.tmp/zero-concurrency")
		.with_map(true)
		.with_concurrency(0);

	// -- Exec
	let err = zmapr::process_content(options).await.err().ok_or("Expected validation error")?;

	// -- Check
	assert!(matches!(err, zmapr::Error::InvalidConfiguration(_)));

	Ok(())
}

#[tokio::test]
async fn test_process_options_validation_ai_stage_without_model() -> Result<()> {
	// -- Setup & Fixtures
	let options = ProcessContentOptions::new("source")
		.with_dest("tests-data/.tmp/missing-model")
		.with_fetch(false)
		.with_map(true);

	// -- Exec
	let err = zmapr::process_content(options).await.err().ok_or("Expected validation error")?;

	// -- Check
	assert!(matches!(err, zmapr::Error::InvalidConfiguration(_)));

	Ok(())
}

#[tokio::test]
async fn test_process_options_validation_empty_sanitize_prompt_content() -> Result<()> {
	// -- Setup & Fixtures
	let options = ProcessContentOptions::new("source")
		.with_dest("tests-data/.tmp/empty-sanitize-prompt")
		.with_fetch(false)
		.with_sanitize(true)
		.with_model("sanitize-model")
		.with_sanitize_prompt(SanitizePrompt::content("  "));

	// -- Exec
	let err = zmapr::process_content(options).await.err().ok_or("Expected validation error")?;

	// -- Check
	assert!(matches!(err, zmapr::Error::InvalidConfiguration(_)));

	Ok(())
}

#[tokio::test]
async fn test_process_options_validation_missing_sanitize_prompt_file() -> Result<()> {
	// -- Setup & Fixtures
	let options = ProcessContentOptions::new("source")
		.with_dest("tests-data/.tmp/missing-sanitize-prompt")
		.with_fetch(false)
		.with_sanitize(true)
		.with_model("sanitize-model")
		.with_sanitize_prompt(SanitizePrompt::file(
			"tests-data/.tmp/missing-sanitize-prompt/instructions.md",
		));

	// -- Exec
	let err = zmapr::process_content(options).await.err().ok_or("Expected validation error")?;

	// -- Check
	assert!(matches!(err, zmapr::Error::InvalidConfiguration(_)));

	Ok(())
}
