use zmapr::{ProcessContentOptions, process_content};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
	// -- Configure processing
	let options = ProcessContentOptions::new("examples/.out/c01-fetch").with_source("src");

	// -- Run processing
	let handle = process_content(options).await?;
	let output = handle.wait_output().await?;

	// -- Report results
	println!("Fetched content into {}", output.content_root);

	Ok(())
}
