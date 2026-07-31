use zmapr::{process_content, ContentSource, FetchOptions, ProcessContentOptions};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
	let source = ContentSource::local_path("src");
	let options = ProcessContentOptions::new("examples/.out/c01-fetch").with_fetch(
		FetchOptions {
			copy_local_files: true,
			..Default::default()
		},
	);

	let handle = process_content(source, options).await?;
	let output = handle.wait_output().await?;

	println!("Fetched content into {}", output.content_root);

	Ok(())
}
