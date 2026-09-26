#![doc = include_str!("../../docs/rustdoc/mapr/mapr-journal.md")]

use crate::mapr::{FileMapEntry, FolderMapEntry, hash_file_bytes};
use crate::support::{JsonlAppender, read_jsonl, truncate_to};
use crate::{Error, Result};
use serde::{Deserialize, Serialize};
use std::fs::OpenOptions;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

// region:    --- Constants

/// Version number written in newly created journal headers.
pub const CURRENT_JOURNAL_VERSION: u32 = 1;

// endregion: --- Constants

// region:    --- Types

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
/// Indicates whether a journal operation produced a reusable entry.
pub enum JournalRecordStatus {
	/// The operation succeeded and produced an entry.
	Ok,

	/// The operation failed, so any previous entry for the path is invalidated.
	Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
/// Identifies the model, prompt, and artifact root used for a journal.
pub struct JournalHeader {
	/// Journal format version.
	pub journal_version: u32,

	/// Model used to generate the mapped entries.
	pub model: String,

	/// Version of the prompt used to generate the mapped entries.
	pub prompt_version: u32,

	/// Root identifier for the mapped source artifacts.
	pub artifact_root: String,

	/// Fingerprint derived from the model, prompt version, and artifact root.
	pub fingerprint: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
/// Journal record for the mapping result of one file.
pub struct JournalFileRecord {
	/// Source-relative path of the file.
	pub path: String,

	/// Hash of the source content used to generate the result.
	pub source_hash: String,

	/// Whether mapping succeeded or failed.
	pub status: JournalRecordStatus,

	/// Generated entry, present for successful records.
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub entry: Option<FileMapEntry>,

	/// Failure detail, present when available for failed records.
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
/// Journal record for the mapping result of one folder.
pub struct JournalFolderRecord {
	/// Source-relative path of the folder.
	pub path: String,

	/// Hash representing the folder source used to generate the result.
	pub source_hash: String,

	/// Whether mapping succeeded or failed.
	pub status: JournalRecordStatus,

	/// Generated entry, present for successful records.
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub entry: Option<FolderMapEntry>,

	/// Failure detail, present when available for failed records.
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
/// A header or result record in the newline-delimited journal format.
pub enum JournalRecord {
	/// Journal identity and compatibility information.
	Header(JournalHeader),

	/// Mapping result for a source file.
	File(JournalFileRecord),

	/// Mapping result for a source folder.
	Folder(JournalFolderRecord),
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
/// Successful file and folder entries recovered from a compatible journal.
pub struct JournalReuseIndex {
	file_entries: BTreeMap<String, (String, FileMapEntry)>,
	folder_entries: BTreeMap<String, (String, FolderMapEntry)>,
}

#[derive(Clone)]
/// Append-only writer for journal records.
///
/// Clones share the same file handle, and appends through that handle are serialized.
pub struct JournalAppender {
	path: PathBuf,
	appender: JsonlAppender,
}

// endregion: --- Types

// region:    --- Public Functions

/// Computes the journal fingerprint for a model, prompt version, and artifact root.
pub fn compute_journal_fingerprint(model: &str, prompt_version: u32, artifact_root: &str) -> String {
	let payload = format!("{model}:{prompt_version}:{artifact_root}");
	hash_file_bytes(payload.as_bytes())
}

/// Removes a journal file if it exists.
pub fn remove_journal(path: impl AsRef<Path>) -> Result<()> {
	let path_ref = path.as_ref();
	if path_ref.exists() {
		std::fs::remove_file(path_ref).map_err(|err| {
			Error::MalformedState(format!("failed to remove journal at {}: {err}", path_ref.display()))
		})?;
	}
	Ok(())
}

/// Truncates an existing journal file without removing it.
pub fn empty_journal(path: impl AsRef<Path>) -> Result<()> {
	let path_ref = path.as_ref();
	if path_ref.exists() {
		let file = OpenOptions::new().write(true).truncate(true).open(path_ref)?;
		drop(file);
	}
	Ok(())
}

/// Loads a compatible journal and opens its appender, creating a fresh journal when needed.
///
/// A malformed final record is discarded during recovery. A malformed record before the
/// final line is reported as an invalid cache.
pub fn init_or_load_journal(
	path: impl AsRef<Path>,
	expected_header: &JournalHeader,
) -> Result<(JournalReuseIndex, JournalAppender)> {
	let path_ref = path.as_ref();

	if !path_ref.exists() {
		let appender = JournalAppender::create_new(path_ref, expected_header)?;
		return Ok((JournalReuseIndex::new(), appender));
	}

	let parsed_result = load_and_clean_journal(path_ref, expected_header);

	match parsed_result {
		Ok(Some(index)) => {
			let appender = JournalAppender::open_existing(path_ref)?;
			Ok((index, appender))
		}
		Ok(None) => {
			let appender = JournalAppender::create_new(path_ref, expected_header)?;
			Ok((JournalReuseIndex::new(), appender))
		}
		Err(err) => Err(err),
	}
}

/// Loads reusable entries when the journal matches the expected fingerprint and version.
///
/// Missing, empty, incompatible, or unusable journals return `Ok(None)`. A malformed final
/// line is ignored, while malformed earlier lines return an invalid-cache error.
pub fn load_journal(
	path: impl AsRef<Path>,
	expected_fingerprint: &str,
	expected_version: u32,
) -> Result<Option<JournalReuseIndex>> {
	let path_ref = path.as_ref();

	if !path_ref.exists() {
		return Ok(None);
	}

	let parsed = read_jsonl::<JournalRecord>(path_ref)?;
	if let Some(line) = parsed.malformed_lines.first() {
		return Err(Error::InvalidCache(format!(
			"malformed journal line {line} in {}",
			path_ref.display()
		)));
	}
	if parsed.records.is_empty() {
		return Ok(None);
	}

	let header = match &parsed.records[0] {
		JournalRecord::Header(header) => header,
		_ => {
			return Err(Error::InvalidCache(format!(
				"first journal line in {} must be a header",
				path_ref.display()
			)));
		}
	};

	if header.journal_version != expected_version || header.fingerprint != expected_fingerprint {
		return Ok(None);
	}

	let mut index = JournalReuseIndex::new();
	for rec in parsed.records.iter().skip(1) {
		index.apply_record(rec);
	}

	Ok(Some(index))
}

// endregion: --- Public Functions

// region:    --- Constructors & Inherent Implementations

impl JournalHeader {
	/// Creates a header and computes its compatibility fingerprint.
	pub fn new(model: impl Into<String>, prompt_version: u32, artifact_root: impl Into<String>) -> Self {
		let model = model.into();
		let artifact_root = artifact_root.into();
		let fingerprint = compute_journal_fingerprint(&model, prompt_version, &artifact_root);
		Self {
			journal_version: CURRENT_JOURNAL_VERSION,
			model,
			prompt_version,
			artifact_root,
			fingerprint,
		}
	}
}

impl JournalRecord {
	/// Creates a header record with the supplied journal identity fields.
	pub fn header(
		journal_version: u32,
		model: impl Into<String>,
		prompt_version: u32,
		artifact_root: impl Into<String>,
		fingerprint: impl Into<String>,
	) -> Self {
		Self::Header(JournalHeader {
			journal_version,
			model: model.into(),
			prompt_version,
			artifact_root: artifact_root.into(),
			fingerprint: fingerprint.into(),
		})
	}

	/// Creates a successful file record.
	pub fn file_ok(path: impl Into<String>, source_hash: impl Into<String>, entry: FileMapEntry) -> Self {
		Self::File(JournalFileRecord {
			path: path.into(),
			source_hash: source_hash.into(),
			status: JournalRecordStatus::Ok,
			entry: Some(entry),
			error: None,
		})
	}

	/// Creates a failed file record.
	pub fn file_failed(path: impl Into<String>, source_hash: impl Into<String>, error: impl Into<String>) -> Self {
		Self::File(JournalFileRecord {
			path: path.into(),
			source_hash: source_hash.into(),
			status: JournalRecordStatus::Failed,
			entry: None,
			error: Some(error.into()),
		})
	}

	/// Creates a successful folder record.
	pub fn folder_ok(path: impl Into<String>, source_hash: impl Into<String>, entry: FolderMapEntry) -> Self {
		Self::Folder(JournalFolderRecord {
			path: path.into(),
			source_hash: source_hash.into(),
			status: JournalRecordStatus::Ok,
			entry: Some(entry),
			error: None,
		})
	}

	/// Creates a failed folder record.
	pub fn folder_failed(path: impl Into<String>, source_hash: impl Into<String>, error: impl Into<String>) -> Self {
		Self::Folder(JournalFolderRecord {
			path: path.into(),
			source_hash: source_hash.into(),
			status: JournalRecordStatus::Failed,
			entry: None,
			error: Some(error.into()),
		})
	}
}

impl JournalReuseIndex {
	/// Creates an empty reuse index.
	pub fn new() -> Self {
		Self::default()
	}

	/// Returns a cached file entry when both its path and source hash match.
	pub fn get_file(&self, path: &str, current_hash: &str) -> Option<&FileMapEntry> {
		if let Some((hash, entry)) = self.file_entries.get(path)
			&& hash == current_hash
		{
			Some(entry)
		} else {
			None
		}
	}

	/// Returns a cached folder entry when both its path and source hash match.
	pub fn get_folder(&self, path: &str, current_hash: &str) -> Option<&FolderMapEntry> {
		if let Some((hash, entry)) = self.folder_entries.get(path)
			&& hash == current_hash
		{
			Some(entry)
		} else {
			None
		}
	}

	/// Returns the number of successful file entries in the index.
	pub fn file_count(&self) -> usize {
		self.file_entries.len()
	}

	/// Returns the number of successful folder entries in the index.
	pub fn folder_count(&self) -> usize {
		self.folder_entries.len()
	}

	/// Inserts or replaces a successful file entry.
	pub fn record_file_ok(&mut self, path: impl Into<String>, source_hash: impl Into<String>, entry: FileMapEntry) {
		self.file_entries.insert(path.into(), (source_hash.into(), entry));
	}

	/// Removes any cached file entry for the path.
	pub fn record_file_failed(&mut self, path: &str) {
		self.file_entries.remove(path);
	}

	/// Inserts or replaces a successful folder entry.
	pub fn record_folder_ok(&mut self, path: impl Into<String>, source_hash: impl Into<String>, entry: FolderMapEntry) {
		self.folder_entries.insert(path.into(), (source_hash.into(), entry));
	}

	/// Removes any cached folder entry for the path.
	pub fn record_folder_failed(&mut self, path: &str) {
		self.folder_entries.remove(path);
	}

	/// Applies a file or folder record, invalidating its cached entry on failure.
	///
	/// Header records do not change the index.
	pub fn apply_record(&mut self, record: &JournalRecord) {
		match record {
			JournalRecord::File(file_rec) => {
				if file_rec.status == JournalRecordStatus::Ok
					&& let Some(entry) = &file_rec.entry
				{
					self.record_file_ok(&file_rec.path, &file_rec.source_hash, entry.clone());
				} else {
					self.record_file_failed(&file_rec.path);
				}
			}
			JournalRecord::Folder(folder_rec) => {
				if folder_rec.status == JournalRecordStatus::Ok
					&& let Some(entry) = &folder_rec.entry
				{
					self.record_folder_ok(&folder_rec.path, &folder_rec.source_hash, entry.clone());
				} else {
					self.record_folder_failed(&folder_rec.path);
				}
			}
			JournalRecord::Header(_) => {}
		}
	}
}

impl JournalAppender {
	/// Creates or truncates a journal, writes its header, and opens it for appending.
	pub fn create_new(path: impl AsRef<Path>, header: &JournalHeader) -> Result<Self> {
		let path_buf = path.as_ref().to_path_buf();
		let appender = JsonlAppender::open(&path_buf, true)?;
		appender.append(&JournalRecord::Header(header.clone()))?;

		Ok(Self {
			path: path_buf,
			appender,
		})
	}

	/// Opens an existing journal for appending without validating its contents.
	pub fn open_existing(path: impl AsRef<Path>) -> Result<Self> {
		let path_buf = path.as_ref().to_path_buf();
		let appender = JsonlAppender::open(&path_buf, false)?;

		Ok(Self {
			path: path_buf,
			appender,
		})
	}

	/// Serializes a record as one JSON line and appends it to the journal.
	pub fn append(&self, record: &JournalRecord) -> Result<()> {
		self.appender.append(record)
	}

	/// Truncates the journal through this appender without writing a new header.
	pub fn empty(&self) -> Result<()> {
		self.appender.truncate()
	}

	/// Returns the path associated with this appender.
	pub fn path(&self) -> &Path {
		&self.path
	}
}

// endregion: --- Constructors & Inherent Implementations

// region:    --- Support

fn load_and_clean_journal(path: &Path, expected_header: &JournalHeader) -> Result<Option<JournalReuseIndex>> {
	let parsed = read_jsonl::<JournalRecord>(path)?;
	if let Some(line) = parsed.malformed_lines.first() {
		return Err(Error::InvalidCache(format!(
			"malformed journal line {line} in {}",
			path.display()
		)));
	}

	let file_len = std::fs::metadata(path)?.len();
	if parsed.valid_len < file_len {
		truncate_to(path, parsed.valid_len)?;
	}

	if parsed.records.is_empty() {
		return Ok(None);
	}

	let header = match &parsed.records[0] {
		JournalRecord::Header(header) => header,
		_ => {
			return Err(Error::InvalidCache(format!(
				"first journal line in {} must be a header",
				path.display()
			)));
		}
	};

	if header.journal_version != expected_header.journal_version || header.fingerprint != expected_header.fingerprint {
		return Ok(None);
	}

	let mut index = JournalReuseIndex::new();
	for rec in parsed.records.iter().skip(1) {
		index.apply_record(rec);
	}

	Ok(Some(index))
}

// endregion: --- Support

// region:    --- Tests

#[cfg(test)]
mod tests {
	type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

	use super::*;
	use std::fs::{OpenOptions, read_to_string};
	use std::io::Write;

	#[test]
	fn test_mapr_journal_roundtrip_append_and_load() -> Result<()> {
		// -- Setup & Fixtures
		let test_dir = std::env::temp_dir().join(format!("zmapr_journal_rt_{}", std::process::id()));
		let journal_path = test_dir.join("content-map.journal.jsonl");

		let header = JournalHeader::new("mock-model", 1, "src");

		// -- Exec: create journal and append records
		let (mut index, appender) = init_or_load_journal(&journal_path, &header)?;
		assert_eq!(index.file_count(), 0);

		let file_entry = FileMapEntry {
			summary: "Entry point".to_string(),
			when_to_use: "Startup".to_string(),
			public_types: vec!["App".to_string()],
			public_functions: vec!["main".to_string()],
			topics: vec!["cli".to_string()],
		};
		appender.append(&JournalRecord::file_ok(
			"src/main.rs",
			"hash-main-1",
			file_entry.clone(),
		))?;
		appender.append(&JournalRecord::file_failed(
			"src/broken.rs",
			"hash-broken",
			"compile error",
		))?;

		let folder_entry = FolderMapEntry {
			summary: "Source folder".to_string(),
			when_to_use: "Consult files".to_string(),
			topics: vec!["src".to_string()],
		};
		appender.append(&JournalRecord::folder_ok("src", "hash-folder-1", folder_entry.clone()))?;

		// -- Check: reload journal from disk
		let loaded = load_journal(&journal_path, &header.fingerprint, header.journal_version)?;
		let loaded_index = loaded.ok_or("expected loaded journal index")?;

		assert_eq!(loaded_index.file_count(), 1);
		assert_eq!(loaded_index.folder_count(), 1);

		let main_cached = loaded_index.get_file("src/main.rs", "hash-main-1");
		assert_eq!(main_cached, Some(&file_entry));

		let main_stale = loaded_index.get_file("src/main.rs", "hash-main-different");
		assert_eq!(main_stale, None);

		let broken_cached = loaded_index.get_file("src/broken.rs", "hash-broken");
		assert_eq!(broken_cached, None);

		let folder_cached = loaded_index.get_folder("src", "hash-folder-1");
		assert_eq!(folder_cached, Some(&folder_entry));

		// Clean up
		remove_journal(&journal_path)?;
		std::fs::remove_dir_all(&test_dir)?;

		Ok(())
	}

	#[test]
	fn test_mapr_journal_reuse_hit_and_miss() -> Result<()> {
		// -- Setup & Fixtures
		let mut index = JournalReuseIndex::new();
		let file_entry = FileMapEntry {
			summary: "Module docs".to_string(),
			when_to_use: "Consult for modules".to_string(),
			public_types: vec![],
			public_functions: vec![],
			topics: vec!["module".to_string()],
		};

		index.record_file_ok("src/lib.rs", "hash-lib-v1", file_entry.clone());

		// -- Exec & Check: hit with matching hash
		assert_eq!(index.get_file("src/lib.rs", "hash-lib-v1"), Some(&file_entry));

		// Miss with different hash
		assert_eq!(index.get_file("src/lib.rs", "hash-lib-v2"), None);

		// Miss with unknown path
		assert_eq!(index.get_file("src/unknown.rs", "hash-lib-v1"), None);

		// Failure removes previous entry
		index.record_file_failed("src/lib.rs");
		assert_eq!(index.get_file("src/lib.rs", "hash-lib-v1"), None);

		Ok(())
	}

	#[test]
	fn test_mapr_journal_fingerprint_invalidation() -> Result<()> {
		// -- Setup & Fixtures
		let test_dir = std::env::temp_dir().join(format!("zmapr_journal_inv_{}", std::process::id()));
		let journal_path = test_dir.join("content-map.journal.jsonl");

		let header_v1 = JournalHeader::new("model-v1", 1, "src");
		let (_, appender) = init_or_load_journal(&journal_path, &header_v1)?;

		let file_entry = FileMapEntry {
			summary: "Old entry".to_string(),
			when_to_use: "Old usage".to_string(),
			public_types: vec![],
			public_functions: vec![],
			topics: vec![],
		};
		appender.append(&JournalRecord::file_ok("src/lib.rs", "hash-v1", file_entry))?;

		// Verify file has 2 lines
		let content_before = read_to_string(&journal_path)?;
		assert_eq!(content_before.lines().count(), 2);

		// -- Exec: init with different model -> fingerprint mismatch
		let header_v2 = JournalHeader::new("model-v2", 1, "src");
		let (new_index, _) = init_or_load_journal(&journal_path, &header_v2)?;

		// -- Check: journal was rewritten and reuse index is empty
		assert_eq!(new_index.file_count(), 0);
		let content_after = read_to_string(&journal_path)?;
		assert_eq!(content_after.lines().count(), 1);
		assert!(content_after.contains("model-v2"));

		// Clean up
		remove_journal(&journal_path)?;
		std::fs::remove_dir_all(&test_dir)?;

		Ok(())
	}

	#[test]
	fn test_mapr_journal_recovery_from_truncated_final_line() -> Result<()> {
		// -- Setup & Fixtures
		let test_dir = std::env::temp_dir().join(format!("zmapr_journal_trunc_{}", std::process::id()));
		let journal_path = test_dir.join("content-map.journal.jsonl");

		let header = JournalHeader::new("mock-model", 1, "src");
		let (_, appender) = init_or_load_journal(&journal_path, &header)?;

		let valid_entry = FileMapEntry {
			summary: "Valid file".to_string(),
			when_to_use: "Inspect".to_string(),
			public_types: vec![],
			public_functions: vec![],
			topics: vec![],
		};
		appender.append(&JournalRecord::file_ok(
			"src/valid.rs",
			"hash-valid",
			valid_entry.clone(),
		))?;

		// Simulate incomplete append (truncated final line)
		{
			let mut file = OpenOptions::new().append(true).open(&journal_path)?;
			write!(file, "{{\"kind\":\"file\",\"path\":\"src/truncated.rs\"")?;
			file.flush()?;
		}

		// -- Exec: re-open journal
		let (recovered_index, recovered_appender) = init_or_load_journal(&journal_path, &header)?;

		// -- Check: valid item is present, truncated one was safely skipped and removed from file
		assert_eq!(recovered_index.file_count(), 1);
		assert_eq!(
			recovered_index.get_file("src/valid.rs", "hash-valid"),
			Some(&valid_entry)
		);

		// Appending a new record works cleanly without syntax collision
		let next_entry = FileMapEntry {
			summary: "Next file".to_string(),
			when_to_use: "Inspect next".to_string(),
			public_types: vec![],
			public_functions: vec![],
			topics: vec![],
		};
		recovered_appender.append(&JournalRecord::file_ok("src/next.rs", "hash-next", next_entry.clone()))?;

		let loaded = load_journal(&journal_path, &header.fingerprint, header.journal_version)?;
		let loaded_index = loaded.ok_or("expected clean journal")?;
		assert_eq!(loaded_index.file_count(), 2);
		assert_eq!(loaded_index.get_file("src/next.rs", "hash-next"), Some(&next_entry));

		// Clean up
		remove_journal(&journal_path)?;
		std::fs::remove_dir_all(&test_dir)?;

		Ok(())
	}

	#[test]
	fn test_mapr_journal_interior_malformed_line_errors() -> Result<()> {
		// -- Setup & Fixtures
		let test_dir = std::env::temp_dir().join(format!("zmapr_journal_err_{}", std::process::id()));
		let journal_path = test_dir.join("content-map.journal.jsonl");

		let header = JournalHeader::new("mock-model", 1, "src");
		let (_, appender) = init_or_load_journal(&journal_path, &header)?;

		// Manually inject a malformed interior line followed by another line
		{
			let mut file = OpenOptions::new().append(true).open(&journal_path)?;
			writeln!(file, "not valid json")?;
			writeln!(
				file,
				"{{\"kind\":\"file\",\"path\":\"src/a.rs\",\"source_hash\":\"h\",\"status\":\"ok\"}}"
			)?;
			file.flush()?;
		}

		// -- Exec & Check: loading should return InvalidCache
		let load_res = load_journal(&journal_path, &header.fingerprint, header.journal_version);
		assert!(load_res.is_err());
		let err_str = load_res.err().map(|e| e.to_string()).unwrap_or_default();
		assert!(err_str.contains("malformed journal line 2"));

		// Clean up
		remove_journal(&journal_path)?;
		std::fs::remove_dir_all(&test_dir)?;

		Ok(())
	}

	#[test]
	fn test_mapr_journal_empty_clears_file_and_reinitializes() -> Result<()> {
		// -- Setup & Fixtures
		let test_dir = std::env::temp_dir().join(format!("zmapr_journal_empty_{}", std::process::id()));
		let journal_path = test_dir.join("content-map.journal.jsonl");

		let header = JournalHeader::new("mock-model", 1, "src");
		let (_, appender) = init_or_load_journal(&journal_path, &header)?;

		let entry = FileMapEntry {
			summary: "File entry".to_string(),
			when_to_use: "Inspect".to_string(),
			public_types: vec![],
			public_functions: vec![],
			topics: vec![],
		};
		appender.append(&JournalRecord::file_ok("src/a.rs", "hash-a", entry))?;

		let meta_before = std::fs::metadata(&journal_path)?;
		assert!(meta_before.len() > 0);

		// -- Exec: empty journal
		appender.empty()?;

		// -- Check: file exists but is 0 bytes
		assert!(journal_path.is_file());
		let meta_after = std::fs::metadata(&journal_path)?;
		assert_eq!(meta_after.len(), 0);

		// Loading an empty journal returns None
		let loaded = load_journal(&journal_path, &header.fingerprint, header.journal_version)?;
		assert_eq!(loaded, None);

		// Reopening reinitializes header properly
		let (reloaded_index, _) = init_or_load_journal(&journal_path, &header)?;
		assert_eq!(reloaded_index.file_count(), 0);
		let meta_reinit = std::fs::metadata(&journal_path)?;
		assert!(meta_reinit.len() > 0);

		// Clean up
		remove_journal(&journal_path)?;
		std::fs::remove_dir_all(&test_dir)?;

		Ok(())
	}
}

// endregion: --- Tests
