use zmapr::{FetchFormat, ItemStatus, ProcessContentOptions, process_content};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
	// -- Configure processing
	let options = ProcessContentOptions::new("examples/.out/c02-http")
		.with_source("https://docs.rs/genai/0.6.5/genai/")
		.with_format(FetchFormat::Md)
		.with_llms(false) // default true
		.with_max_depth(1);

	// -- Run processing
	let handle = process_content(options).await?;
	let output = handle.wait_output().await?;

	// -- Report results
	println!("Fetched content into {}", output.content_root);
	println!(
		"Completed items: {}",
		output.stats.fetch.as_ref().map_or(0, |stats| stats.completed)
	);

	for item in output
		.items
		.iter()
		.filter(|item| item.fetch.as_ref().is_some_and(|state| state.status == ItemStatus::Completed))
	{
		println!(" - {}", item.source);
	}

	Ok(())
}
