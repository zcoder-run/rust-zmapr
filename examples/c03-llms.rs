use zmapr::{ProcessContentOptions, ProcessProgress, process_content};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
	let options = ProcessContentOptions::new("examples/.out/c03-llms")
		.with_source("https://docs.typesafe.ai/introduction")
		.with_max_depth(1)
		.with_llms(true); // default anyway

	let mut handle = process_content(options).await?;

	if let Some(mut rx) = handle.take_progress_rx() {
		while let Ok(pp) = rx.recv().await {
			if let ProcessProgress::ItemCompleted { item } = pp {
				println!("{}", item.output_path_str_or("(no output path)"));
			}
		}
	}
	let output = handle.wait_output().await?;

	println!("Fetched content into {}", output.content_root);
	println!("Completed items: {}", output.completed_items.len());

	// for item in &output.completed_items {
	// 	println!(" - {}", item.source);
	// }

	Ok(())
}
