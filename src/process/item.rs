use super::response::ProcessStage;
use simple_fs::SPath;

// region:    --- Types

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ItemId(usize);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ItemStatus {
	Pending,
	Running,
	Completed,
	Reused,
	Skipped,
	Failed,
}

#[derive(Debug, Clone)]
pub struct ItemStageState {
	pub status: ItemStatus,
	pub path: Option<SPath>,
	pub usage: Option<genai::chat::Usage>,
	pub error: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ItemState {
	pub id: ItemId,
	pub source: String,
	pub origin_path: String,
	pub relative_path: String,
	pub fetch: Option<ItemStageState>,
	pub sanitize: Option<ItemStageState>,
	pub map: Option<ItemStageState>,
}

// endregion: --- Types

impl ItemId {
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
