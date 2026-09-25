# zmapr

`zmapr` is a Rust library for turning local or web content into AI-oriented context. Its main entry point, [`process_content`], accepts [`ProcessContentOptions`] and starts an asynchronous workflow. The returned [`ProcessContentHandle`] lets callers observe progress and live state, then collect the final [`ProcessContentOutput`] with artifact paths, item states, and statistics.

Configure local or web sources, Fetch formats and selection filters, web crawl depth, stage-specific models, and resume or concurrency behavior through the options API. The selected stages run in this fixed order:

## Stages

- **Fetch** retrieves content from a local path or an HTTP(S) source. It supports file selection, HTML conversion, and web crawling options such as maximum depth and `llms.txt` discovery.

- **Sanitize** optionally applies AI-based sanitization to fetched content, using the configured model and either the built-in instructions or a custom prompt.

- **Map** optionally analyzes the processed content with AI and creates a structured [`ContentMap`] with file and folder guidance.

Stages can be combined or run individually. A disabled stage passes the current artifacts through unchanged. If Fetch is disabled while a later stage is enabled, the workflow uses a valid prior Fetch cache.

## Workflow

Use [`process_content`] to start the workflow. Sources are represented by [`ContentSource`], and [`FetchFormat`] configures how fetched HTML is stored. The workflow handle provides progress notifications, read-only access to live state, and the final output after processing completes.

The returned [`ProcessContentHandle`] exposes progress through [`ProgressRx`], live state through [`ProcessQuery`], and final results through [`ProcessContentHandle::wait_output`]. Use [`ProcessStateSnapshot`] for a point-in-time view.

## Example

This example fetches local Rust source files and waits for the workflow output. Optional Sanitize and Map stages are disabled by default.

```rust
use zmapr::{ProcessContentOptions, process_content};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
	// -- Configure workflow
	let options = ProcessContentOptions::new("target/zmapr").with_source("src");
	
	// -- Start workflow
	let handle = process_content(options).await?;

	// -- Read current query statistics
	let query = handle.query();
	let stats_task = tokio::spawn(async move {
		tokio::time::sleep(std::time::Duration::from_millis(100)).await;
		let stats = query.stats();
		println!(
			"Fetch: {} of {} registered items completed",
			stats.fetch.completed,
			stats.fetch.registered_items()
		);
	});
	
	// -- Wait for completion
	let output = handle.wait_output().await?;
	stats_task.await?;

	println!("Fetched content into {}", output.content_root);
	Ok(())
}
```

To enable both AI stages, configure a model along with the source:

```rust
let options = ProcessContentOptions::new("target/zmapr")
    .with_source("src")
    .with_sanitize(true)
    .with_map(true)
    .with_model("gpt-6-luna");
```

## Mapping

Mapped content is represented by [`ContentMap`]. The mapping API also provides AI client selection through [`MaprAiClient`] and [`MaprAiSelector`], prompt construction, and journal support. Use [`set_active_ai_selector`] to choose the active client selector.

## Errors

Fallible public APIs use the crate's [`Result`] alias and [`Error`] type.
