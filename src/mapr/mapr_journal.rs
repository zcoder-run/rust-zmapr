use crate::mapr::{FileMapEntry, FolderMapEntry, hash_file_bytes};
use crate::{Error, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

// region:    --- Constants

pub const CURRENT_JOURNAL_VERSION: u32 = 1;

// endregion: --- Constants

// region:    --- Types

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JournalRecordStatus {
	Ok,
	Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct JournalHeader {
	pub journal_version: u32,
	pub provider: String,
	pub model: String,
	pub prompt_version: u32,
	pub artifact_root: String,
	pub fingerprint: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct JournalFileRecord {
	pub path: String,
	pub source_hash: String,
	pub status: JournalRecordStatus,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub entry: Option<FileMapEntry>,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct JournalFolderRecord {
	pub path: String,
	pub source_hash: String,
	pub status: JournalRecordStatus,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub entry: Option<FolderMapEntry>,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum JournalRecord {
	Header(JournalHeader),
	File(JournalFileRecord),
	Folder(JournalFolderRecord),
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct JournalReuseIndex {
	file_entries: BTreeMap<String, (String, FileMapEntry)>,
	folder_entries: BTreeMap<String, (String, FolderMapEntry)>,
}

#[derive(Clone)]
pub struct JournalAppender {
	path: PathBuf,
	file: Arc<Mutex<std::fs::File>>,
}

// endregion: --- Types

// region:    --- Public Functions

pub fn compute_journal_fingerprint(provider: &str, model: &str, prompt_version: u32, artifact_root: &str) -> String {
	let payload = format!("{provider}:{model}:{prompt_version}:{artifact_root}");
	hash_file_bytes(payload.as_bytes())
}

pub fn remove_journal(path: impl AsRef<Path>) -> Result<()> {
	let path_ref = path.as_ref();
	if path_ref.exists() {
		std::fs::remove_file(path_ref).map_err(|err| {
			Error::MalformedState(format!("failed to remove journal at {}: {err}", path_ref.display()))
		})?;
	}
	Ok(())
}

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

pub fn load_journal(
	path: impl AsRef<Path>,
	expected_fingerprint: &str,
	expected_version: u32,
) -> Result<Option<JournalReuseIndex>> {
	let path_ref = path.as_ref();

	if !path_ref.exists() {
		return Ok(None);
	}

	let bytes = std::fs::read(path_ref)?;
	if bytes.is_empty() {
		return Ok(None);
	}

	let lines = extract_line_ranges(&bytes);
	if lines.is_empty() {
		return Ok(None);
	}

	let mut non_empty_lines = Vec::new();
	for (start, end, delim_end) in lines {
		if end > start {
			non_empty_lines.push((start, end, delim_end));
		}
	}

	if non_empty_lines.is_empty() {
		return Ok(None);
	}

	let mut records = Vec::new();
	let total = non_empty_lines.len();

	for (idx, &(start, end, _)) in non_empty_lines.iter().enumerate() {
		let is_last = idx == total - 1;
		let line_str = match std::str::from_utf8(&bytes[start..end]) {
			Ok(s) => s,
			Err(_) if is_last => break,
			Err(err) => {
				return Err(Error::InvalidCache(format!(
					"malformed UTF-8 journal line {} in {}: {err}",
					idx + 1,
					path_ref.display()
				)));
			}
		};

		match serde_json::from_str::<JournalRecord>(line_str) {
			Ok(rec) => records.push(rec),
			Err(_) if is_last => break,
			Err(err) => {
				return Err(Error::InvalidCache(format!(
					"malformed journal line {} in {}: {err}",
					idx + 1,
					path_ref.display()
				)));
			}
		}
	}

	if records.is_empty() {
		return Ok(None);
	}

	let header = match &records[0] {
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
	for rec in records.into_iter().skip(1) {
		index.apply_record(&rec);
	}

	Ok(Some(index))
}

// endregion: --- Public Functions

// region:    --- Constructors & Inherent Implementations

impl JournalHeader {
	pub fn new(
		provider: impl Into<String>,
		model: impl Into<String>,
		prompt_version: u32,
		artifact_root: impl Into<String>,
	) -> Self {
		let provider = provider.into();
		let model = model.into();
		let artifact_root = artifact_root.into();
		let fingerprint = compute_journal_fingerprint(&provider, &model, prompt_version, &artifact_root);
		Self {
			journal_version: CURRENT_JOURNAL_VERSION,
			provider,
			model,
			prompt_version,
			artifact_root,
			fingerprint,
		}
	}
}

impl JournalRecord {
	pub fn header(
		journal_version: u32,
		provider: impl Into<String>,
		model: impl Into<String>,
		prompt_version: u32,
		artifact_root: impl Into<String>,
		fingerprint: impl Into<String>,
	) -> Self {
		Self::Header(JournalHeader {
			journal_version,
			provider: provider.into(),
			model: model.into(),
			prompt_version,
			artifact_root: artifact_root.into(),
			fingerprint: fingerprint.into(),
		})
	}

	pub fn file_ok(path: impl Into<String>, source_hash: impl Into<String>, entry: FileMapEntry) -> Self {
		Self::File(JournalFileRecord {
			path: path.into(),
			source_hash: source_hash.into(),
			status: JournalRecordStatus::Ok,
			entry: Some(entry),
			error: None,
		})
	}

	pub fn file_failed(path: impl Into<String>, source_hash: impl Into<String>, error: impl Into<String>) -> Self {
		Self::File(JournalFileRecord {
			path: path.into(),
			source_hash: source_hash.into(),
			status: JournalRecordStatus::Failed,
			entry: None,
			error: Some(error.into()),
		})
	}

	pub fn folder_ok(path: impl Into<String>, source_hash: impl Into<String>, entry: FolderMapEntry) -> Self {
		Self::Folder(JournalFolderRecord {
			path: path.into(),
			source_hash: source_hash.into(),
			status: JournalRecordStatus::Ok,
			entry: Some(entry),
			error: None,
		})
	}

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
	pub fn new() -> Self {
		Self::default()
	}

	pub fn get_file(&self, path: &str, current_hash: &str) -> Option<&FileMapEntry> {
		if let Some((hash, entry)) = self.file_entries.get(path)
			&& hash == current_hash
		{
			Some(entry)
		} else {
			None
		}
	}

	pub fn get_folder(&self, path: &str, current_hash: &str) -> Option<&FolderMapEntry> {
		if let Some((hash, entry)) = self.folder_entries.get(path)
			&& hash == current_hash
		{
			Some(entry)
		} else {
			None
		}
	}

	pub fn file_count(&self) -> usize {
		self.file_entries.len()
	}

	pub fn folder_count(&self) -> usize {
		self.folder_entries.len()
	}

	pub fn record_file_ok(&mut self, path: impl Into<String>, source_hash: impl Into<String>, entry: FileMapEntry) {
		self.file_entries.insert(path.into(), (source_hash.into(), entry));
	}

	pub fn record_file_failed(&mut self, path: &str) {
		self.file_entries.remove(path);
	}

	pub fn record_folder_ok(&mut self, path: impl Into<String>, source_hash: impl Into<String>, entry: FolderMapEntry) {
		self.folder_entries.insert(path.into(), (source_hash.into(), entry));
	}

	pub fn record_folder_failed(&mut self, path: &str) {
		self.folder_entries.remove(path);
	}

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
	pub fn create_new(path: impl AsRef<Path>, header: &JournalHeader) -> Result<Self> {
		let path_buf = path.as_ref().to_path_buf();
		if let Some(parent) = path_buf.parent() {
			std::fs::create_dir_all(parent)?;
		}

		let mut file = OpenOptions::new().create(true).write(true).truncate(true).open(&path_buf)?;

		let header_record = JournalRecord::Header(header.clone());
		let line = serde_json::to_string(&header_record)
			.map_err(|err| Error::MalformedState(format!("failed to serialize journal header: {err}")))?;
		writeln!(file, "{line}")?;
		file.flush()?;

		Ok(Self {
			path: path_buf,
			file: Arc::new(Mutex::new(file)),
		})
	}

	pub fn open_existing(path: impl AsRef<Path>) -> Result<Self> {
		let path_buf = path.as_ref().to_path_buf();
		let file = OpenOptions::new().append(true).open(&path_buf)?;

		Ok(Self {
			path: path_buf,
			file: Arc::new(Mutex::new(file)),
		})
	}

	pub fn append(&self, record: &JournalRecord) -> Result<()> {
		let line = serde_json::to_string(record)
			.map_err(|err| Error::MalformedState(format!("failed to serialize journal record: {err}")))?;
		let mut file = self
			.file
			.lock()
			.map_err(|_| Error::MalformedState("failed to lock journal file".to_string()))?;
		writeln!(file, "{line}")?;
		file.flush()?;
		Ok(())
	}

	pub fn path(&self) -> &Path {
		&self.path
	}
}

// endregion: --- Constructors & Inherent Implementations

// region:    --- Support

fn extract_line_ranges(bytes: &[u8]) -> Vec<(usize, usize, usize)> {
	let mut line_ranges = Vec::new();
	let mut line_start = 0;

	for (i, &b) in bytes.iter().enumerate() {
		if b == b'\n' {
			let content_end = if i > line_start && bytes[i - 1] == b'\r' {
				i - 1
			} else {
				i
			};
			line_ranges.push((line_start, content_end, i + 1));
			line_start = i + 1;
		}
	}

	if line_start < bytes.len() {
		line_ranges.push((line_start, bytes.len(), bytes.len()));
	}

	line_ranges
}

fn load_and_clean_journal(path: &Path, expected_header: &JournalHeader) -> Result<Option<JournalReuseIndex>> {
	let bytes = std::fs::read(path)?;
	if bytes.is_empty() {
		return Ok(None);
	}

	let lines = extract_line_ranges(&bytes);
	if lines.is_empty() {
		return Ok(None);
	}

	let mut non_empty_lines = Vec::new();
	for (start, end, delim_end) in lines {
		if end > start {
			non_empty_lines.push((start, end, delim_end));
		}
	}

	if non_empty_lines.is_empty() {
		return Ok(None);
	}

	let mut records = Vec::new();
	let total = non_empty_lines.len();
	let mut valid_byte_len = 0;

	for (idx, &(start, end, delim_end)) in non_empty_lines.iter().enumerate() {
		let is_last = idx == total - 1;
		let line_str = match std::str::from_utf8(&bytes[start..end]) {
			Ok(s) => s,
			Err(_) if is_last => {
				truncate_file_to(path, valid_byte_len)?;
				break;
			}
			Err(err) => {
				return Err(Error::InvalidCache(format!(
					"malformed UTF-8 journal line {} in {}: {err}",
					idx + 1,
					path.display()
				)));
			}
		};

		match serde_json::from_str::<JournalRecord>(line_str) {
			Ok(rec) => {
				records.push(rec);
				valid_byte_len = delim_end;
			}
			Err(_) if is_last => {
				truncate_file_to(path, valid_byte_len)?;
				break;
			}
			Err(err) => {
				return Err(Error::InvalidCache(format!(
					"malformed journal line {} in {}: {err}",
					idx + 1,
					path.display()
				)));
			}
		}
	}

	if records.is_empty() {
		return Ok(None);
	}

	let header = match &records[0] {
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
	for rec in records.into_iter().skip(1) {
		index.apply_record(&rec);
	}

	Ok(Some(index))
}

fn truncate_file_to(path: &Path, len: usize) -> Result<()> {
	let file = OpenOptions::new().write(true).open(path)?;
	file.set_len(len as u64)?;
	Ok(())
}

// endregion: --- Support

// region:    --- Tests

#[cfg(test)]
mod tests {
	type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

	use super::*;
	use std::fs::read_to_string;

	#[test]
	fn test_mapr_journal_roundtrip_append_and_load() -> Result<()> {
		// -- Setup & Fixtures
		let test_dir = std::env::temp_dir().join(format!("zmapr_journal_rt_{}", std::process::id()));
		let journal_path = test_dir.join("content-map.journal.jsonl");

		let header = JournalHeader::new("mock-provider", "mock-model", 1, "src");

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

		let header_v1 = JournalHeader::new("mock-provider", "model-v1", 1, "src");
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
		let header_v2 = JournalHeader::new("mock-provider", "model-v2", 1, "src");
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

		let header = JournalHeader::new("mock-provider", "mock-model", 1, "src");
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

		let header = JournalHeader::new("mock-provider", "mock-model", 1, "src");
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
}

// endregion: --- Tests
