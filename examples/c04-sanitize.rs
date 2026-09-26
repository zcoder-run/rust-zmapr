use zmapr::{ItemStatus, ProcessContentOptions, ProgressEvent, process_content};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
	// -- Configure processing
	let options = ProcessContentOptions::new("examples/.out/c04-sanitize")
		.with_source("https://docs.rs/genai/0.7.0-beta.23/genai/")
		.with_max_depth(1)
		.with_sanitize(true)
		.with_map(true)
		.with_model("gpt-6-luna");

	// -- Run processing
	let mut handle = process_content(options).await?;

	// -- Track progress
	let query = handle.query();
	let mut progress_rx = handle.take_progress_rx().ok_or("expected progress receiver")?;
	let progress_task = tokio::spawn(async move {
		while let Ok(update) = progress_rx.recv().await {
			if let ProgressEvent::ItemStatusChanged { id, stage, status } = update.event
				&& let Some(item) = query.item(id)
			{
				match status {
					ItemStatus::Completed | ItemStatus::Reused | ItemStatus::Skipped => {
						println!(" - {}", item.source);
					}
					ItemStatus::Failed => {
						let message = item.stage(stage).and_then(|state| state.error.as_deref()).unwrap_or("unknown");
						println!(" - (FAIL) {} (cause: {message})", item.source);
					}
					_ => {}
				}
			}
		}
	});

	// -- Report results
	let output = handle.wait_output().await?;
	progress_task.await?;

	println!("Fetched content into {}", output.content_root);
	if let Some(map_path) = output.content_map_path {
		println!("Generated content map at {map_path}");
	}
	let completed_items = output.stats.fetch.as_ref().map_or(0, |stats| stats.completed)
		+ output.stats.sanitize.as_ref().map_or(0, |stats| stats.completed)
		+ output.stats.map.as_ref().map_or(0, |stats| stats.completed);
	println!("Completed items: {completed_items}");

	println!();
	if let Some(usage) = &output.stats.total_usage {
		let input_tokens = usage.prompt_tokens.unwrap_or(0);
		let output_tokens = usage.completion_tokens.unwrap_or(0);
		let total_tokens = usage.total_tokens.unwrap_or(input_tokens + output_tokens);

		println!("Total input tokens: {input_tokens}");
		println!("Total output tokens: {output_tokens}");
		println!("Total tokens: {total_tokens}");
	} else {
		println!("Total tokens: n/a");
	}

	Ok(())
}
