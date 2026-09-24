use zmapr::{ProcessContentOptions, ProcessProgress, process_content};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
	let options = ProcessContentOptions::new("examples/.out/c04-mapr")
		.with_source("https://docs.rs/genai/0.7.0-beta.23/genai/")
		.with_max_depth(1)
		.with_llms(true)
		.with_map(true)
		.with_model("gpt-6-luna");

	let mut handle = process_content(options).await?;
	let mut progress_rx = handle.take_progress_rx().ok_or("expected progress receiver")?;
	let progress_task = tokio::spawn(async move {
		while let Ok(event) = progress_rx.recv().await {
			match event {
				ProcessProgress::ItemCompleted { item } | ProcessProgress::ItemSkipped { item } => {
					println!(" - {}", item.source);
				}
				ProcessProgress::ItemFailed { failure } => {
					println!(" - (FAIL_ {}", failure.item.source);
				}
				_ => {}
			}
		}
	});

	let output = handle.wait_output().await?;
	progress_task.await?;

	println!("Fetched content into {}", output.content_root);
	if let Some(map_path) = output.content_map_path {
		println!("Generated content map at {map_path}");
	}
	println!("Completed items: {}", output.completed_items.len());

	println!();
	if let Some(usage) = &output.total_usage {
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
