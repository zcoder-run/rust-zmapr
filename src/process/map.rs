use std::collections::BTreeMap;

// region:    --- Types

#[derive(Debug, Clone, Default)]
pub struct ContentMap {
	/// Maps source-relative file paths to file guidance.
	pub file_map: BTreeMap<String, FileMapEntry>,
	/// Maps source-relative directory paths to folder guidance.
	pub folder_map: BTreeMap<String, FolderMapEntry>,
}

#[derive(Debug, Clone, Default)]
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

#[derive(Debug, Clone, Default)]
pub struct FolderMapEntry {
	/// Concise description of the folder's responsibility.
	pub summary: String,
	/// Guidance for when a reader should inspect the folder.
	pub when_to_use: String,
	/// Searchable subject labels.
	pub topics: Vec<String>,
}

// endregion: --- Types
