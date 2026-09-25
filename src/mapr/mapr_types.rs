#![doc = include_str!("../../docs/rustdoc/mapr/mapr-types.md")]

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

// region:    --- Types

/// In-memory guidance for source files and directories.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ContentMap {
	/// Maps source-relative file paths to file guidance.
	pub file_map: BTreeMap<String, FileMapEntry>,
	/// Maps source-relative directory paths to folder guidance.
	pub folder_map: BTreeMap<String, FolderMapEntry>,
}

/// Guidance generated for an individual source file.
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

/// Guidance generated for a source directory.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct FolderMapEntry {
	/// Concise description of the folder's responsibility.
	pub summary: String,
	/// Guidance for when a reader should inspect the folder.
	pub when_to_use: String,
	/// Searchable subject labels.
	pub topics: Vec<String>,
}

/// Provenance information associated with a mapped source file.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct FileMapMetadata {
	/// Source modification time in Unix nanoseconds, when available.
	pub last_modified_unix_nanos: Option<u64>,
	/// Hash identifying the source content used to generate the entry.
	pub source_hash: String,
}

/// Serialized document written to `content-map.json`, carrying provenance metadata.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ContentMapDocument {
	/// Schema version of the serialized document.
	pub version: u32,
	/// Model used to generate the content map.
	pub model: String,
	/// Version of the prompt used to generate the content map.
	pub prompt_version: u32,
	/// Timestamp recorded when the document was generated.
	pub generated_at: String,
	/// Guidance indexed by source-relative file path.
	pub file_map: BTreeMap<String, FileMapEntry>,
	/// Guidance indexed by source-relative directory path.
	pub folder_map: BTreeMap<String, FolderMapEntry>,
	#[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
	/// Optional provenance metadata indexed by source-relative file path.
	pub file_metadata: BTreeMap<String, FileMapMetadata>,
}

// endregion: --- Types

impl ContentMapDocument {
	/// Creates a document with schema version 1 and no file metadata.
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
			file_metadata: BTreeMap::new(),
		}
	}

	/// Creates a document from an in-memory content map.
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

	/// Replaces the document's file provenance metadata.
	pub fn with_file_metadata(mut self, file_metadata: BTreeMap<String, FileMapMetadata>) -> Self {
		self.file_metadata = file_metadata;
		self
	}
}
