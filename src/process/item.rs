#![doc = include_str!("../../docs/rustdoc/process/item.md")]

use super::response::ProcessStage;
use simple_fs::SPath;

// region:    --- Types

/// Identifies an item registered in a process run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ItemId(usize);

/// Lifecycle status of a processing stage for an item.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ItemStatus {
	/// The stage has not started.
	Pending,

	/// The stage is currently running.
	Running,

	/// The stage completed successfully.
	Completed,

	/// The stage output was reused rather than produced again.
	Reused,

	/// The stage was skipped.
	Skipped,

	/// The stage failed.
	Failed,
}

/// Recorded status and output details for one item stage.
#[derive(Debug, Clone)]
pub struct ItemStageState {
	/// Current lifecycle status of the stage.
	pub status: ItemStatus,

	/// Path to the stage output, when one is available.
	pub path: Option<SPath>,

	/// Token usage reported for the stage, when available.
	pub usage: Option<genai::chat::Usage>,

	/// Error details when the stage failed.
	pub error: Option<String>,
}

/// Identity, source information, and stage states for one process item.
#[derive(Debug, Clone)]
pub struct ItemState {
	/// Identifier assigned to this item in the process run.
	pub id: ItemId,

	/// Source string supplied for this item.
	pub source: String,

	/// Original path associated with this item.
	pub origin_path: String,

	/// Path used to identify this item relative to its source root.
	pub relative_path: String,

	/// Fetch stage state, when recorded for this item.
	pub fetch: Option<ItemStageState>,

	/// Sanitize stage state, when recorded for this item.
	pub sanitize: Option<ItemStageState>,

	/// Map stage state, when recorded for this item.
	pub map: Option<ItemStageState>,
}

// endregion: --- Types

impl ItemId {
	/// Returns the numeric index of this item in the process run.
	pub fn index(&self) -> usize {
		self.0
	}

	pub(crate) fn new(index: usize) -> Self {
		Self(index)
	}
}

impl ItemStageState {
	pub(crate) fn pending() -> Self {
		Self {
			status: ItemStatus::Pending,
			path: None,
			usage: None,
			error: None,
		}
	}
}

impl ItemState {
	/// Returns the recorded state for `stage`, if one exists.
	pub fn stage(&self, stage: ProcessStage) -> Option<&ItemStageState> {
		match stage {
			ProcessStage::Fetch => self.fetch.as_ref(),
			ProcessStage::Sanitize => self.sanitize.as_ref(),
			ProcessStage::Map => self.map.as_ref(),
		}
	}

	pub(crate) fn stage_mut(&mut self, stage: ProcessStage) -> &mut Option<ItemStageState> {
		match stage {
			ProcessStage::Fetch => &mut self.fetch,
			ProcessStage::Sanitize => &mut self.sanitize,
			ProcessStage::Map => &mut self.map,
		}
	}

	/// Returns the Sanitize output path, falling back to the Fetch output path.
	///
	/// The selection is based on which paths are present, not on stage status.
	/// Map output paths are not considered.
	pub fn content_path(&self) -> Option<&SPath> {
		self.sanitize
			.as_ref()
			.and_then(|state| state.path.as_ref())
			.or_else(|| self.fetch.as_ref().and_then(|state| state.path.as_ref()))
	}
}

// region:    --- Tests

#[cfg(test)]
mod tests {
	type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

	use super::*;

	#[test]
	fn test_process_item_content_path_prefers_sanitize() -> Result<()> {
		// -- Setup & Fixtures
		let fetch_path = SPath::from("fetch.md");
		let sanitize_path = SPath::from("sanitize.md");
		let item = ItemState {
			id: ItemId::new(0),
			source: "source.md".to_owned(),
			origin_path: "source.md".to_owned(),
			relative_path: "source.md".to_owned(),
			fetch: Some(ItemStageState {
				status: ItemStatus::Completed,
				path: Some(fetch_path),
				..ItemStageState::pending()
			}),
			sanitize: Some(ItemStageState {
				status: ItemStatus::Completed,
				path: Some(sanitize_path),
				..ItemStageState::pending()
			}),
			map: None,
		};

		// -- Exec
		let content_path = item.content_path().ok_or("expected a content path")?;

		// -- Check
		assert_eq!(content_path.as_str(), "sanitize.md");
		Ok(())
	}

	#[test]
	fn test_process_item_stage_accessor() -> Result<()> {
		// -- Setup & Fixtures
		let item = ItemState {
			id: ItemId::new(0),
			source: "source.md".to_owned(),
			origin_path: "source.md".to_owned(),
			relative_path: "source.md".to_owned(),
			fetch: Some(ItemStageState::pending()),
			sanitize: None,
			map: None,
		};

		// -- Exec
		let fetch = item.stage(ProcessStage::Fetch).ok_or("expected Fetch state")?;

		// -- Check
		assert_eq!(fetch.status, ItemStatus::Pending);
		assert!(item.stage(ProcessStage::Sanitize).is_none());
		Ok(())
	}
}

// endregion: --- Tests
