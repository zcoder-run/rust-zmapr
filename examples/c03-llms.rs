use zmapr::{FetchFormat, ItemStatus, ProcessContentOptions, ProgressEvent, process_content};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
	// -- Configure processing
	let options = ProcessContentOptions::new("https://docs.typesafe.ai/introduction")
		.with_dest("examples/.out/c03-llms")
		.with_llms(true) // default anyway
		.with_max_depth(10);

	// -- Run processing
	let mut handle = process_content(options).await?;
	let query = handle.query();

	// -- Track progress
	if let Some(mut rx) = handle.take_progress_rx() {
		while let Ok(update) = rx.recv().await {
			if let ProgressEvent::ItemStatusChanged {
				id,
				stage,
				status: ItemStatus::Completed,
			} = update.event
				&& let Some(item) = query.item(id)
			{
				let output_path = item
					.stage(stage)
					.and_then(|state| state.path.as_ref())
					.map_or("(no output path)", |path| path.as_str());
				println!("{output_path}");
			}
		}
	}

	// -- Report results
	let output = handle.wait_output().await?;

	println!("Fetched content into {}", output.content_root);
	println!(
		"Completed items: {}",
		output.stats.fetch.as_ref().map_or(0, |stats| stats.completed)
	);

	// for item in &output.completed_items {
	// 	println!(" - {}", item.source);
	// }

	Ok(())
}
