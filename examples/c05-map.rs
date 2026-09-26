use zmapr::{ItemStatus, ProcessContentOptions, ProcessQuery, ProgressEvent, ProgressRx, process_content};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
	// -- Configure processing
	let options = ProcessContentOptions::new("examples/.out/c05-map")
		.with_source("https://docs.rs/genai/0.7.0-beta.23/genai/")
		.with_sanitize(true)
		.with_map(true)
		.with_max_depth(1)
		.with_concurrency(12) // default 8
		.with_model("gpt-6-luna");

	// -- Run processing
	let mut handle = process_content(options).await?;

	// -- Track progress
	let query = handle.query();

	// -- Read current query statistics
	let stats_task = tokio::spawn(async move {
		tokio::time::sleep(std::time::Duration::from_millis(100)).await;
		let stats = query.stats();
		println!(
			"Fetch: {} of {} registered items completed",
			stats.fetch.completed,
			stats.fetch.registered_items()
		);
	});
	let progress_rx = handle.take_progress_rx().ok_or("expected progress receiver")?;
	let progress_task = tokio::spawn(print_progress(progress_rx, handle.query()));

	let output = handle.wait_output().await?;
	progress_task.await?;
	stats_task.await?;

	// -- Report results
	println!("\n\nProcessed content into {}", output.content_root);
	if let Some(map_path) = &output.content_map_path {
		println!("Generated content map at {map_path}");
	}
	let completed_items = output.stats.fetch.as_ref().map_or(0, |stats| stats.completed)
		+ output.stats.sanitize.as_ref().map_or(0, |stats| stats.completed)
		+ output.stats.map.as_ref().map_or(0, |stats| stats.completed);
	println!("Completed items: {completed_items}");

	let input_tokens = output.stats.total_usage.as_ref().and_then(|usage| usage.prompt_tokens);
	let output_tokens = output.stats.total_usage.as_ref().and_then(|usage| usage.completion_tokens);
	println!(
		"Total input tokens: {}",
		input_tokens.map_or_else(|| "unavailable".to_string(), |tokens| tokens.to_string())
	);
	println!(
		"Total output tokens: {}",
		output_tokens.map_or_else(|| "unavailable".to_string(), |tokens| tokens.to_string())
	);

	Ok(())
}

async fn print_progress(mut progress_rx: ProgressRx, query: ProcessQuery) {
	while let Ok(update) = progress_rx.recv().await {
		if let ProgressEvent::ItemStatusChanged { id, stage, status } = update.event
			&& let Some(item) = query.item(id)
		{
			match status {
				ItemStatus::Completed | ItemStatus::Reused | ItemStatus::Skipped => {
					println!("{stage:?} - {}", item.source);
				}
				ItemStatus::Failed => {
					let message = item.stage(stage).and_then(|state| state.error.as_deref()).unwrap_or("unknown");
					println!(" - (FAIL) {} (cause: {message})", item.source);
				}
				_ => {}
			}
		}
	}
}
