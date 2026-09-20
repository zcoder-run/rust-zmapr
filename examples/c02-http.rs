use zmapr::{ContentSource, FetchOptions, ProcessContentOptions, process_content};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
	let source = ContentSource::web("https://docs.typesafe.ai/introduction");

	let options = ProcessContentOptions::new("examples/.out/c02-http").with_fetch(FetchOptions {
		same_host_only: true,
		follow_links: true,
		max_depth: 1,
		..Default::default()
	});

	let handle = process_content(source, options).await?;
	let output = handle.wait_output().await?;

	println!("Fetched content into {}", output.content_root);
	println!("Completed items: {}", output.completed_items.len());

	for item in &output.completed_items {
		println!(" - {}", item.source);
	}

	Ok(())
}
