use crate::Result;
use crate::support::{JsonlAppender, hash_bytes, read_jsonl, truncate_to};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

const JOURNAL_VERSION: u32 = 1;

// region:    --- Types

#[derive(Debug, Clone)]
pub(crate) struct SanitizeJournal {
	appender: JsonlAppender,
	index: HashMap<String, DoneEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct HeaderInfo {
	pub(crate) model: String,
	pub(crate) instruction_hash: String,
	pub(crate) input_root: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct JournalLoadReport {
	pub(crate) malformed_lines: Vec<usize>,
	pub(crate) started_fresh: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct DoneEntry {
	input_hash: String,
	output_hash: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum SanitizeJournalRecord {
	Header {
		version: u32,
		model: String,
		instruction_hash: String,
		input_root: String,
	},
	Done {
		path: String,
		input_hash: String,
		output_hash: String,
	},
	Failed {
		path: String,
		error: String,
	},
}

// endregion: --- Types

// region:    --- Public Functions

pub(crate) fn sanitize_journal_path(manifest_path: impl AsRef<Path>) -> PathBuf {
	manifest_path.as_ref().with_file_name("sanitize.journal.jsonl")
}

// endregion: --- Public Functions

// region:    --- Constructors & Inherent Implementations

impl SanitizeJournal {
	pub(crate) fn open(
		journal_path: impl AsRef<Path>,
		expected: HeaderInfo,
		resume: bool,
	) -> Result<(Self, JournalLoadReport)> {
		let journal_path = journal_path.as_ref();

		if !resume {
			return start_fresh(journal_path, &expected, Vec::new());
		}

		let loaded = read_jsonl::<SanitizeJournalRecord>(journal_path)?;
		let header_matches = matches!(
			loaded.records.first(),
			Some(SanitizeJournalRecord::Header {
				version,
				model,
				instruction_hash,
				input_root,
			}) if *version == JOURNAL_VERSION
				&& model == &expected.model
				&& instruction_hash == &expected.instruction_hash
				&& input_root == &expected.input_root
		);

		if !header_matches {
			return start_fresh(journal_path, &expected, loaded.malformed_lines);
		}

		let file_len = match std::fs::metadata(journal_path) {
			Ok(metadata) => metadata.len(),
			Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
				return start_fresh(journal_path, &expected, loaded.malformed_lines);
			}
			Err(error) => return Err(error.into()),
		};

		if loaded.valid_len < file_len {
			truncate_to(journal_path, loaded.valid_len)?;
		}

		let mut index = HashMap::new();
		for record in loaded.records.into_iter().skip(1) {
			match record {
				SanitizeJournalRecord::Done {
					path,
					input_hash,
					output_hash,
				} => {
					index.insert(
						normalize_path(&path),
						DoneEntry {
							input_hash,
							output_hash,
						},
					);
				}
				SanitizeJournalRecord::Failed { path, .. } => {
					index.remove(&normalize_path(&path));
				}
				SanitizeJournalRecord::Header { .. } => {}
			}
		}

		let appender = JsonlAppender::open(journal_path, false)?;
		Ok((
			Self { appender, index },
			JournalLoadReport {
				malformed_lines: loaded.malformed_lines,
				started_fresh: false,
			},
		))
	}

	pub(crate) fn reusable(&self, path: &str, current_input_hash: &str, output_file: impl AsRef<Path>) -> bool {
		let normalized_path = normalize_path(path);
		let Some(entry) = self.index.get(&normalized_path) else {
			return false;
		};
		if entry.input_hash != current_input_hash {
			return false;
		}

		let Ok(contents) = std::fs::read(output_file) else {
			return false;
		};

		hash_bytes(&contents) == entry.output_hash
	}

	pub(crate) fn record_done(&self, path: &str, input_hash: &str, output_hash: &str) -> Result<()> {
		self.appender.append(&SanitizeJournalRecord::Done {
			path: normalize_path(path),
			input_hash: input_hash.to_owned(),
			output_hash: output_hash.to_owned(),
		})
	}

	pub(crate) fn record_failed(&self, path: &str, error: &str) -> Result<()> {
		self.appender.append(&SanitizeJournalRecord::Failed {
			path: normalize_path(path),
			error: error.to_owned(),
		})
	}
}

// endregion: --- Constructors & Inherent Implementations

// region:    --- Support

fn start_fresh(
	journal_path: &Path,
	expected: &HeaderInfo,
	malformed_lines: Vec<usize>,
) -> Result<(SanitizeJournal, JournalLoadReport)> {
	let appender = JsonlAppender::open(journal_path, true)?;
	appender.append(&header_record(expected))?;

	Ok((
		SanitizeJournal {
			appender,
			index: HashMap::new(),
		},
		JournalLoadReport {
			malformed_lines,
			started_fresh: true,
		},
	))
}

fn header_record(expected: &HeaderInfo) -> SanitizeJournalRecord {
	SanitizeJournalRecord::Header {
		version: JOURNAL_VERSION,
		model: expected.model.clone(),
		instruction_hash: expected.instruction_hash.clone(),
		input_root: expected.input_root.clone(),
	}
}

fn normalize_path(path: &str) -> String {
	path.replace('\\', "/")
}

// endregion: --- Support

// region:    --- Tests

#[cfg(test)]
mod tests {
	type TestResult<T> = core::result::Result<T, Box<dyn std::error::Error>>;

	use super::*;
	use std::io::Write;

	#[test]
	fn test_sanitizr_sanitizr_journal_open_fresh() -> TestResult<()> {
		// -- Setup & Fixtures
		let test_root = journal_test_dir("test_sanitizr_sanitizr_journal_open_fresh");
		let journal_path = test_root.join("sanitize.journal.jsonl");
		let expected = test_header();

		// -- Exec
		let (journal, report) = SanitizeJournal::open(&journal_path, expected.clone(), false)?;

		// -- Check
		assert!(report.started_fresh);
		assert!(report.malformed_lines.is_empty());
		drop(journal);

		let contents = std::fs::read_to_string(&journal_path)?;
		let lines = contents.lines().collect::<Vec<_>>();
		assert_eq!(lines.len(), 1);

		let record = serde_json::from_str::<SanitizeJournalRecord>(lines.first().ok_or("missing journal header")?)?;
		assert_eq!(record, header_record(&expected));

		// std::fs::remove_dir_all(&test_root)?;
		Ok(())
	}

	#[test]
	fn test_sanitizr_sanitizr_journal_open_header_mismatch() -> TestResult<()> {
		// -- Setup & Fixtures
		let test_root = journal_test_dir("test_sanitizr_sanitizr_journal_open_header_mismatch");
		let expected = test_header();

		// -- Exec & Check
		for mismatch in ["model", "instruction_hash"] {
			let journal_path = test_root.join(mismatch).join("sanitize.journal.jsonl");
			let (journal, _) = SanitizeJournal::open(&journal_path, expected.clone(), false)?;
			journal.record_done("item.txt", "input-hash", "output-hash")?;
			drop(journal);

			let changed = if mismatch == "model" {
				HeaderInfo {
					model: "other-model".to_owned(),
					..expected.clone()
				}
			} else {
				HeaderInfo {
					instruction_hash: "other-instruction-hash".to_owned(),
					..expected.clone()
				}
			};

			let (journal, report) = SanitizeJournal::open(&journal_path, changed.clone(), true)?;
			assert!(report.started_fresh);
			drop(journal);

			let contents = std::fs::read_to_string(&journal_path)?;
			let lines = contents.lines().collect::<Vec<_>>();
			assert_eq!(lines.len(), 1);
			let record = serde_json::from_str::<SanitizeJournalRecord>(lines.first().ok_or("missing journal header")?)?;
			assert_eq!(record, header_record(&changed));
		}

		// std::fs::remove_dir_all(&test_root)?;
		Ok(())
	}

	#[test]
	fn test_sanitizr_sanitizr_journal_later_record_wins() -> TestResult<()> {
		// -- Setup & Fixtures
		let test_root = journal_test_dir("test_sanitizr_sanitizr_journal_later_record_wins");
		let journal_path = test_root.join("sanitize.journal.jsonl");
		let output_path = test_root.join("item.txt");
		let expected = test_header();
		let first_output = b"first output";
		let second_output = b"second output";

		// -- Exec
		std::fs::create_dir_all(&test_root)?;
		std::fs::write(&output_path, first_output)?;
		let (journal, _) = SanitizeJournal::open(&journal_path, expected.clone(), false)?;
		journal.record_done("item.txt", "input-hash", &hash_bytes(first_output))?;

		std::fs::write(&output_path, second_output)?;
		journal.record_done("item.txt", "input-hash", &hash_bytes(second_output))?;
		drop(journal);

		let (journal, _) = SanitizeJournal::open(&journal_path, expected.clone(), true)?;
		assert!(journal.reusable("item.txt", "input-hash", &output_path));

		journal.record_failed("item.txt", "later failure")?;
		drop(journal);

		let (journal, _) = SanitizeJournal::open(&journal_path, expected, true)?;

		// -- Check
		assert!(!journal.reusable("item.txt", "input-hash", &output_path));

		// std::fs::remove_dir_all(&test_root)?;
		Ok(())
	}

	#[test]
	fn test_sanitizr_sanitizr_journal_recovers_truncated_final_line() -> TestResult<()> {
		// -- Setup & Fixtures
		let test_root = journal_test_dir("test_sanitizr_sanitizr_journal_recovers_truncated_final_line");
		let journal_path = test_root.join("sanitize.journal.jsonl");
		let expected = test_header();
		let (journal, _) = SanitizeJournal::open(&journal_path, expected.clone(), false)?;
		drop(journal);

		let mut file = std::fs::OpenOptions::new().append(true).open(&journal_path)?;
		file.write_all(br#"{"type":"done""#)?;
		drop(file);

		// -- Exec
		let (journal, report) = SanitizeJournal::open(&journal_path, expected.clone(), true)?;

		// -- Check
		assert!(!report.started_fresh);
		assert!(report.malformed_lines.is_empty());
		journal.record_done("item.txt", "input-hash", "output-hash")?;
		drop(journal);

		let loaded = read_jsonl::<SanitizeJournalRecord>(&journal_path)?;
		assert_eq!(loaded.records.len(), 2);
		assert!(loaded.malformed_lines.is_empty());
		assert_eq!(loaded.valid_len, std::fs::metadata(&journal_path)?.len());

		// std::fs::remove_dir_all(&test_root)?;
		Ok(())
	}

	#[test]
	fn test_sanitizr_sanitizr_journal_reusable_revalidates_output() -> TestResult<()> {
		// -- Setup & Fixtures
		let test_root = journal_test_dir("test_sanitizr_sanitizr_journal_reusable_revalidates_output");
		let journal_path = test_root.join("sanitize.journal.jsonl");
		let output_path = test_root.join("item.txt");
		let expected = test_header();
		let input_hash = "input-hash";
		let output = b"output";

		// -- Exec
		std::fs::create_dir_all(&test_root)?;
		std::fs::write(&output_path, output)?;
		let (journal, _) = SanitizeJournal::open(&journal_path, expected.clone(), false)?;
		journal.record_done("item.txt", input_hash, &hash_bytes(output))?;
		drop(journal);

		let (journal, _) = SanitizeJournal::open(&journal_path, expected, true)?;

		// -- Check
		assert!(journal.reusable("item.txt", input_hash, &output_path));
		assert!(!journal.reusable("missing.txt", input_hash, &output_path));
		assert!(!journal.reusable("item.txt", "changed-input-hash", &output_path));

		std::fs::write(&output_path, b"modified output")?;
		assert!(!journal.reusable("item.txt", input_hash, &output_path));

		std::fs::write(test_root.join("item.txt"), output)?;
		assert!(!journal.reusable("item.txt", input_hash, test_root.join("missing.txt")));
		assert!(!journal.reusable("item\\file.txt", input_hash, &output_path));

		// std::fs::remove_dir_all(&test_root)?;
		Ok(())
	}

	// -- Test Support

	fn test_header() -> HeaderInfo {
		HeaderInfo {
			model: "test-model".to_owned(),
			instruction_hash: "instruction-hash".to_owned(),
			input_root: "input-root".to_owned(),
		}
	}

	fn journal_test_dir(name: &str) -> PathBuf {
		PathBuf::from("tests-data/.tmp").join(name)
	}
}

// endregion: --- Tests
