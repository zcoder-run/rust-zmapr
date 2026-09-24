use zmapr::{ProcessContentOptions, ProcessProgress, ProgressRx, process_content};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
	let options = ProcessContentOptions::new("examples/.out/c05-sanetizer")
		.with_source("https://docs.rs/genai/0.7.0-beta.23/genai/")
		.with_sanitize(true)
		.with_map(true)
		.with_max_depth(1)
		.with_model("gpt-6-luna");

	let mut handle = process_content(options).await?;

	let progress_rx = handle.take_progress_rx().ok_or("expected progress receiver")?;
	let progress_task = tokio::spawn(print_progress(progress_rx));

	let output = handle.wait_output().await?;
	progress_task.await?;

	println!("\n\nProcessed content into {}", output.content_root);
	if let Some(map_path) = &output.content_map_path {
		println!("Generated content map at {map_path}");
	}
	println!("Completed items: {}", output.completed_items.len());

	Ok(())
}

async fn print_progress(mut progress_rx: ProgressRx) {
	while let Ok(event) = progress_rx.recv().await {
		match event {
			ProcessProgress::ItemCompleted { item } | ProcessProgress::ItemSkipped { item } => {
				println!(" - {}", item.source);
			}
			ProcessProgress::ItemFailed { failure } => {
				println!(" - (FAIL) {} (cause: {})", failure.item.source, failure.message);
			}
			_ => {}
		}
	}
}
