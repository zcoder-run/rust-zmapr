use crate::process::FetchOptions;
use serde::{Deserialize, Serialize};
use simple_fs::SPath;

// region:    --- Types

#[derive(Debug, Clone)]
pub(crate) struct LocalFetchDiscovery {
	pub(crate) source: String,
	pub(crate) source_path: SPath,
	pub(crate) items: Vec<LocalFetchItem>,
}

#[derive(Debug, Clone)]
pub(crate) struct LocalFetchItem {
	pub(crate) source: String,
	pub(crate) relative_path: String,
	pub(crate) local_path: SPath,
	pub(crate) media_type: Option<String>,
	pub(crate) content_hash: String,
}

#[derive(Debug, Deserialize, Serialize)]
pub(crate) struct FetchManifest {
	pub(crate) version: u32,
	pub(crate) complete: bool,
	pub(crate) source: String,
	pub(crate) source_path: String,
	pub(crate) options: FetchManifestOptions,
	pub(crate) artifact_root: String,
	pub(crate) items: Vec<FetchManifestItem>,
}

#[derive(Debug, Deserialize, PartialEq, Eq, Serialize)]
pub(crate) struct FetchManifestOptions {
	pub(crate) include: Vec<String>,
	pub(crate) exclude: Vec<String>,
	pub(crate) copy_local_files: bool,
	pub(crate) same_host_only: bool,
	pub(crate) max_depth: usize,
	pub(crate) follow_links: bool,
}

#[derive(Debug, Deserialize, Serialize)]
pub(crate) struct FetchManifestItem {
	pub(crate) source: String,
	pub(crate) relative_path: String,
	pub(crate) local_path: String,
	pub(crate) artifact_path: Option<String>,
	pub(crate) media_type: Option<String>,
	pub(crate) content_hash: String,
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum LocalSourceKind {
	File,
	Directory,
}

// endregion: --- Types

// region:    --- Froms

impl From<&FetchOptions> for FetchManifestOptions {
	fn from(options: &FetchOptions) -> Self {
		Self {
			include: options.include.clone(),
			exclude: options.exclude.clone(),
			copy_local_files: options.copy_local_files,
			same_host_only: options.same_host_only,
			max_depth: options.max_depth,
			follow_links: options.follow_links,
		}
	}
}

// endregion: --- Froms
