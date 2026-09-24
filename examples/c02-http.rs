use zmapr::{ProcessContentOptions, process_content};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
	let options = ProcessContentOptions::new("examples/.out/c02-http")
		.with_source("https://docs.typesafe.ai/introduction")
		.with_max_depth(1);

	let handle = process_content(options).await?;
	let output = handle.wait_output().await?;

	println!("Fetched content into {}", output.content_root);
	println!("Completed items: {}", output.completed_items.len());

	for item in &output.completed_items {
		println!(" - {}", item.source);
	}

	Ok(())
}
