use zmapr::{ProcessContentOptions, process_content};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
	let options = ProcessContentOptions::new("examples/.out/c01-fetch")
		.with_source("src");

	let handle = process_content(options).await?;
	let output = handle.wait_output().await?;

	println!("Fetched content into {}", output.content_root);

	Ok(())
}
