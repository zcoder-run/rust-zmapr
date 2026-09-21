use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

// region:    --- Types

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ContentMap {
	/// Maps source-relative file paths to file guidance.
	pub file_map: BTreeMap<String, FileMapEntry>,
	/// Maps source-relative directory paths to folder guidance.
	pub folder_map: BTreeMap<String, FolderMapEntry>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct FileMapEntry {
	/// Concise description of the file's content.
	pub summary: String,
	/// Guidance for when a reader should consult the file.
	pub when_to_use: String,
	/// Public types exposed by the file, when applicable.
	pub public_types: Vec<String>,
	/// Public functions exposed by the file, when applicable.
	pub public_functions: Vec<String>,
	/// Searchable subject labels.
	pub topics: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct FolderMapEntry {
	/// Concise description of the folder's responsibility.
	pub summary: String,
	/// Guidance for when a reader should inspect the folder.
	pub when_to_use: String,
	/// Searchable subject labels.
	pub topics: Vec<String>,
}

/// Serialized document written to `content-map.json`, carrying provenance metadata.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ContentMapDocument {
	pub version: u32,
	pub model: String,
	pub prompt_version: u32,
	pub generated_at: String,
	pub file_map: BTreeMap<String, FileMapEntry>,
	pub folder_map: BTreeMap<String, FolderMapEntry>,
}

// endregion: --- Types

impl ContentMapDocument {
	pub fn new(
		model: impl Into<String>,
		prompt_version: u32,
		generated_at: impl Into<String>,
		file_map: BTreeMap<String, FileMapEntry>,
		folder_map: BTreeMap<String, FolderMapEntry>,
	) -> Self {
		Self {
			version: 1,
			model: model.into(),
			prompt_version,
			generated_at: generated_at.into(),
			file_map,
			folder_map,
		}
	}

	pub fn from_content_map(
		model: impl Into<String>,
		prompt_version: u32,
		generated_at: impl Into<String>,
		content_map: ContentMap,
	) -> Self {
		Self::new(
			model,
			prompt_version,
			generated_at,
			content_map.file_map,
			content_map.folder_map,
		)
	}
}
