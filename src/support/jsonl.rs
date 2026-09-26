use crate::{Error, Result};
use serde::{Serialize, de::DeserializeOwned};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;
use std::sync::{Arc, Mutex};

// region:    --- Types

#[derive(Debug, Clone)]
pub(crate) struct JsonlAppender {
	file: Arc<Mutex<std::fs::File>>,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct JsonlRead<T> {
	pub records: Vec<T>,
	pub malformed_lines: Vec<usize>,
	pub valid_len: u64,
}

// endregion: --- Types

// region:    --- Public Functions

pub(crate) fn read_jsonl<T: DeserializeOwned>(path: impl AsRef<Path>) -> Result<JsonlRead<T>> {
	let path = path.as_ref();
	let bytes = match fs::read(path) {
		Ok(bytes) => bytes,
		Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
			return Ok(JsonlRead {
				records: Vec::new(),
				malformed_lines: Vec::new(),
				valid_len: 0,
			});
		}
		Err(err) => return Err(err.into()),
	};

	let line_ranges = extract_line_ranges(&bytes);
	let last_non_empty_line = line_ranges.iter().rposition(|(start, end, _)| end > start);
	let mut result = JsonlRead {
		records: Vec::new(),
		malformed_lines: Vec::new(),
		valid_len: 0,
	};

	for (idx, &(start, end, delim_end)) in line_ranges.iter().enumerate() {
		if end <= start {
			continue;
		}

		let is_last = Some(idx) == last_non_empty_line;
		let is_newline_terminated = bytes.get(delim_end.saturating_sub(1)) == Some(&b'\n');
		if is_last && !is_newline_terminated {
			break;
		}

		let parsed = std::str::from_utf8(&bytes[start..end])
			.ok()
			.and_then(|line| serde_json::from_str::<T>(line).ok());

		if let Some(record) = parsed {
			result.records.push(record);
			result.valid_len = delim_end as u64;
		} else if is_last {
			break;
		} else {
			result.malformed_lines.push(idx + 1);
		}
	}

	Ok(result)
}

/// Truncates a file to the supplied byte offset.
pub(crate) fn truncate_to(path: impl AsRef<Path>, len: u64) -> Result<()> {
	let file = OpenOptions::new().write(true).open(path)?;
	file.set_len(len)?;
	Ok(())
}

// endregion: --- Public Functions

// region:    --- Constructors & Inherent Implementations

impl JsonlAppender {
	pub(crate) fn open(path: impl AsRef<Path>, truncate: bool) -> Result<Self> {
		let path = path.as_ref();

		if truncate
			&& let Some(parent) = path.parent().filter(|parent| !parent.as_os_str().is_empty())
		{
			fs::create_dir_all(parent)?;
		}

		let mut options = OpenOptions::new();
		options.create(truncate).write(true);
		if truncate {
			options.truncate(true);
		} else {
			options.append(true);
		}

		let file = options.open(path)?;
		Ok(Self {
			file: Arc::new(Mutex::new(file)),
		})
	}

	pub(crate) fn append<T: Serialize>(&self, record: &T) -> Result<()> {
		let mut line = serde_json::to_string(record)
			.map_err(|err| Error::MalformedState(format!("failed to serialize JSONL record: {err}")))?;
		line.push('\n');

		let mut file = self
			.file
			.lock()
			.map_err(|_| Error::MalformedState("failed to lock JSONL file".to_string()))?;
		file.write_all(line.as_bytes())?;
		file.flush()?;
		Ok(())
	}

	pub(crate) fn truncate(&self) -> Result<()> {
		let mut file = self
			.file
			.lock()
			.map_err(|_| Error::MalformedState("failed to lock JSONL file".to_string()))?;
		file.set_len(0)?;
		file.flush()?;
		Ok(())
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

// endregion: --- Support

// region:    --- Tests

#[cfg(test)]
mod tests {
	type TestResult<T> = core::result::Result<T, Box<dyn std::error::Error>>;

	use super::*;
	use serde_json::{Value, json};
	use std::collections::HashSet;
	use std::path::PathBuf;
	use std::time::{SystemTime, UNIX_EPOCH};

	fn test_path(name: &str) -> PathBuf {
		let timestamp = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
		std::env::temp_dir().join(format!("zmapr_jsonl_{name}_{}_{timestamp}", std::process::id()))
	}

	#[test]
	fn test_jsonl_roundtrip() -> TestResult<()> {
		let path = test_path("roundtrip");
		let expected = vec![json!({"id": 1}), json!({"id": 2}), json!({"id": 3})];

		let appender = JsonlAppender::open(&path, true)?;
		for record in &expected {
			appender.append(record)?;
		}
		drop(appender);

		let loaded = read_jsonl::<Value>(&path)?;
		assert_eq!(loaded.records, expected);
		assert!(loaded.malformed_lines.is_empty());
		assert_eq!(loaded.valid_len, std::fs::metadata(&path)?.len());

		std::fs::remove_file(path)?;
		Ok(())
	}

	#[test]
	fn test_jsonl_drops_truncated_final_line_and_recovers_for_append() -> TestResult<()> {
		let path = test_path("truncated");
		let valid_line = br#"{"value":1}"#;
		let mut content = valid_line.to_vec();
		content.push(b'\n');
		content.extend_from_slice(br#"{"value":2}"#);
		std::fs::write(&path, content)?;

		let loaded = read_jsonl::<Value>(&path)?;
		assert_eq!(loaded.records, vec![json!({"value": 1})]);
		assert_eq!(loaded.valid_len, (valid_line.len() + 1) as u64);

		truncate_to(&path, loaded.valid_len)?;
		let appender = JsonlAppender::open(&path, false)?;
		appender.append(&json!({"value": 3}))?;
		drop(appender);

		let recovered = read_jsonl::<Value>(&path)?;
		assert_eq!(recovered.records, vec![json!({"value": 1}), json!({"value": 3})]);
		assert!(recovered.malformed_lines.is_empty());

		std::fs::remove_file(path)?;
		Ok(())
	}

	#[test]
	fn test_jsonl_reports_and_skips_malformed_interior_lines() -> TestResult<()> {
		let path = test_path("malformed");
		std::fs::write(&path, b"{\"value\":1}\nnot json\n{\"value\":3}\n")?;

		let loaded = read_jsonl::<Value>(&path)?;
		assert_eq!(loaded.records, vec![json!({"value": 1}), json!({"value": 3})]);
		assert_eq!(loaded.malformed_lines, vec![2]);
		assert_eq!(loaded.valid_len, std::fs::metadata(&path)?.len());

		std::fs::remove_file(path)?;
		Ok(())
	}

	#[test]
	fn test_jsonl_concurrent_appends_write_complete_lines() -> TestResult<()> {
		let path = test_path("concurrent");
		let appender = JsonlAppender::open(&path, true)?;
		let worker_count = 8;
		let records_per_worker = 100;
		let mut workers = Vec::new();

		for worker_id in 0..worker_count {
			let appender = appender.clone();
			workers.push(std::thread::spawn(move || {
				for sequence in 0..records_per_worker {
					appender
						.append(&json!({"worker": worker_id, "sequence": sequence}))
						.expect("append should succeed");
				}
			}));
		}

		for worker in workers {
			worker.join().expect("worker should complete");
		}
		drop(appender);

		let loaded = read_jsonl::<Value>(&path)?;
		assert_eq!(loaded.records.len(), worker_count * records_per_worker);
		assert!(loaded.malformed_lines.is_empty());

		let mut seen = HashSet::new();
		for record in &loaded.records {
			let worker = record["worker"].as_u64().ok_or("missing worker id")?;
			let sequence = record["sequence"].as_u64().ok_or("missing sequence")?;
			assert!(seen.insert((worker, sequence)));
		}
		assert_eq!(seen.len(), worker_count * records_per_worker);

		std::fs::remove_file(path)?;
		Ok(())
	}
}

// endregion:    --- Tests
