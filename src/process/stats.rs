use super::item::ItemStatus;
use super::response::ProcessStage;
use crate::support::{duration_between_micros, now_micro};
use crate::{Error, Result};
use std::time::Duration;

// region:    --- Types

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum StageStatus {
	#[default]
	NotSelected,
	Pending,
	Running,
	Completed,
	Failed,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct StageProgress {
	pub status: StageStatus,
	pub total_items: Option<usize>,
	pub pending: usize,
	pub running: usize,
	pub completed: usize,
	pub reused: usize,
	pub skipped: usize,
	pub failed: usize,
	pub excluded: usize,
	pub usage: Option<genai::chat::Usage>,
	pub started_epoch_us: Option<i64>,
	pub ended_epoch_us: Option<i64>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ProgressStats {
	pub fetch: StageProgress,
	pub sanitize: StageProgress,
	pub map: StageProgress,
	pub total_usage: Option<genai::chat::Usage>,
	pub started_epoch_us: i64,
	pub ended_epoch_us: Option<i64>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct StageFinal {
	pub total_items: usize,
	pub completed: usize,
	pub reused: usize,
	pub skipped: usize,
	pub failed: usize,
	pub excluded: usize,
	pub usage: Option<genai::chat::Usage>,
	pub started_epoch_us: i64,
	pub ended_epoch_us: i64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FinalStats {
	pub fetch: Option<StageFinal>,
	pub sanitize: Option<StageFinal>,
	pub map: Option<StageFinal>,
	pub total_usage: Option<genai::chat::Usage>,
	pub started_epoch_us: i64,
	pub ended_epoch_us: i64,
}

// endregion: --- Types

impl StageProgress {
	pub fn registered_items(&self) -> usize {
		self.pending + self.running + self.completed + self.reused + self.skipped + self.failed
	}

	pub fn duration(&self) -> Option<Duration> {
		let started_epoch_us = self.started_epoch_us?;
		let ended_epoch_us = self.ended_epoch_us.unwrap_or_else(now_micro);
		Some(duration_between_micros(started_epoch_us, ended_epoch_us))
	}

	pub(crate) fn counter_mut(&mut self, status: ItemStatus) -> &mut usize {
		match status {
			ItemStatus::Pending => &mut self.pending,
			ItemStatus::Running => &mut self.running,
			ItemStatus::Completed => &mut self.completed,
			ItemStatus::Reused => &mut self.reused,
			ItemStatus::Skipped => &mut self.skipped,
			ItemStatus::Failed => &mut self.failed,
		}
	}
}

impl ProgressStats {
	pub fn stage(&self, stage: ProcessStage) -> &StageProgress {
		match stage {
			ProcessStage::Fetch => &self.fetch,
			ProcessStage::Sanitize => &self.sanitize,
			ProcessStage::Map => &self.map,
		}
	}

	pub(crate) fn stage_mut(&mut self, stage: ProcessStage) -> &mut StageProgress {
		match stage {
			ProcessStage::Fetch => &mut self.fetch,
			ProcessStage::Sanitize => &mut self.sanitize,
			ProcessStage::Map => &mut self.map,
		}
	}

	pub fn duration(&self) -> Duration {
		let ended_epoch_us = self.ended_epoch_us.unwrap_or_else(now_micro);
		duration_between_micros(self.started_epoch_us, ended_epoch_us)
	}
}

impl StageFinal {
	pub fn duration(&self) -> Duration {
		duration_between_micros(self.started_epoch_us, self.ended_epoch_us)
	}
}

impl FinalStats {
	pub fn duration(&self) -> Duration {
		duration_between_micros(self.started_epoch_us, self.ended_epoch_us)
	}

	pub fn stage(&self, stage: ProcessStage) -> Option<&StageFinal> {
		match stage {
			ProcessStage::Fetch => self.fetch.as_ref(),
			ProcessStage::Sanitize => self.sanitize.as_ref(),
			ProcessStage::Map => self.map.as_ref(),
		}
	}

	pub(crate) fn from_progress(stats: &ProgressStats) -> Result<FinalStats> {
		let ended_epoch_us = stats
			.ended_epoch_us
			.ok_or_else(|| Error::MalformedState("workflow end time is missing".to_owned()))?;

		Ok(FinalStats {
			fetch: stage_final("Fetch", &stats.fetch)?,
			sanitize: stage_final("Sanitize", &stats.sanitize)?,
			map: stage_final("Map", &stats.map)?,
			total_usage: stats.total_usage.clone(),
			started_epoch_us: stats.started_epoch_us,
			ended_epoch_us,
		})
	}
}

// region:    --- Support

fn stage_final(name: &str, stage: &StageProgress) -> Result<Option<StageFinal>> {
	if stage.status == StageStatus::NotSelected {
		return Ok(None);
	}

	if stage.status != StageStatus::Completed {
		return Err(Error::MalformedState(format!("{name} stage is not completed")));
	}

	if stage.pending != 0 || stage.running != 0 {
		return Err(Error::MalformedState(format!(
			"{name} stage still has pending or running items"
		)));
	}

	let started_epoch_us = stage
		.started_epoch_us
		.ok_or_else(|| Error::MalformedState(format!("{name} stage start time is missing")))?;
	let ended_epoch_us = stage
		.ended_epoch_us
		.ok_or_else(|| Error::MalformedState(format!("{name} stage end time is missing")))?;

	let total_items = stage.total_items.unwrap_or_else(|| stage.registered_items());
	let outcome_count = stage.completed + stage.reused + stage.skipped + stage.failed;
	if total_items != outcome_count {
		return Err(Error::MalformedState(format!(
			"{name} stage total does not match its completed outcomes"
		)));
	}

	Ok(Some(StageFinal {
		total_items,
		completed: stage.completed,
		reused: stage.reused,
		skipped: stage.skipped,
		failed: stage.failed,
		excluded: stage.excluded,
		usage: stage.usage.clone(),
		started_epoch_us,
		ended_epoch_us,
	}))
}

pub(crate) fn add_usage(acc: &mut Option<genai::chat::Usage>, usage: &genai::chat::Usage) {
	let acc = acc.get_or_insert_with(Default::default);
	accumulate_usage(acc, usage);
}

pub(crate) fn accumulate_usage(acc: &mut genai::chat::Usage, other: &genai::chat::Usage) {
	if let Some(tokens) = other.prompt_tokens {
		acc.prompt_tokens = Some(acc.prompt_tokens.unwrap_or(0) + tokens);
	}
	if let Some(tokens) = other.completion_tokens {
		acc.completion_tokens = Some(acc.completion_tokens.unwrap_or(0) + tokens);
	}
	if let Some(tokens) = other.total_tokens {
		acc.total_tokens = Some(acc.total_tokens.unwrap_or(0) + tokens);
	} else if other.prompt_tokens.is_some() || other.completion_tokens.is_some() {
		let prompt = other.prompt_tokens.unwrap_or(0);
		let completion = other.completion_tokens.unwrap_or(0);
		acc.total_tokens = Some(acc.total_tokens.unwrap_or(0) + prompt + completion);
	}
}

// endregion: --- Support

// region:    --- Tests

#[cfg(test)]
mod tests {
	type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

	use super::*;

	#[test]
	fn test_process_stats_counter_mut_maps_status() -> Result<()> {
		// -- Setup & Fixtures
		let mut stage = StageProgress::default();

		// -- Exec
		*stage.counter_mut(ItemStatus::Pending) = 1;
		*stage.counter_mut(ItemStatus::Running) = 2;
		*stage.counter_mut(ItemStatus::Completed) = 3;
		*stage.counter_mut(ItemStatus::Reused) = 4;
		*stage.counter_mut(ItemStatus::Skipped) = 5;
		*stage.counter_mut(ItemStatus::Failed) = 6;

		// -- Check
		assert_eq!(stage.pending, 1);
		assert_eq!(stage.running, 2);
		assert_eq!(stage.completed, 3);
		assert_eq!(stage.reused, 4);
		assert_eq!(stage.skipped, 5);
		assert_eq!(stage.failed, 6);
		Ok(())
	}

	#[test]
	fn test_process_stats_final_from_progress_ok() -> Result<()> {
		// -- Setup & Fixtures
		let progress = ProgressStats {
			fetch: StageProgress {
				status: StageStatus::Completed,
				total_items: Some(2),
				completed: 1,
				reused: 1,
				started_epoch_us: Some(100),
				ended_epoch_us: Some(200),
				..StageProgress::default()
			},
			started_epoch_us: 100,
			ended_epoch_us: Some(300),
			..ProgressStats::default()
		};

		// -- Exec
		let final_stats = FinalStats::from_progress(&progress)?;

		// -- Check
		let fetch = final_stats.stage(ProcessStage::Fetch).ok_or("expected Fetch stats")?;
		assert_eq!(fetch.total_items, 2);
		assert_eq!(fetch.completed, 1);
		assert_eq!(fetch.reused, 1);
		assert_eq!(fetch.duration(), Duration::from_micros(100));
		assert!(final_stats.sanitize.is_none());
		assert!(final_stats.map.is_none());
		Ok(())
	}

	#[test]
	fn test_process_stats_final_from_progress_rejects_pending() -> Result<()> {
		// -- Setup & Fixtures
		let progress = ProgressStats {
			fetch: StageProgress {
				status: StageStatus::Completed,
				total_items: Some(1),
				pending: 1,
				started_epoch_us: Some(100),
				ended_epoch_us: Some(200),
				..StageProgress::default()
			},
			ended_epoch_us: Some(300),
			..ProgressStats::default()
		};

		// -- Exec
		let final_stats = FinalStats::from_progress(&progress);

		// -- Check
		assert!(matches!(final_stats, Err(crate::Error::MalformedState(_))));
		Ok(())
	}

	#[test]
	fn test_process_stats_final_from_progress_not_selected_is_none() -> Result<()> {
		// -- Setup & Fixtures
		let progress = ProgressStats {
			ended_epoch_us: Some(200),
			..ProgressStats::default()
		};

		// -- Exec
		let final_stats = FinalStats::from_progress(&progress)?;

		// -- Check
		assert!(final_stats.fetch.is_none());
		assert!(final_stats.sanitize.is_none());
		assert!(final_stats.map.is_none());
		Ok(())
	}

	#[test]
	fn test_process_stats_add_usage_accumulates() -> Result<()> {
		// -- Setup & Fixtures
		let mut total = None;
		let first = genai::chat::Usage {
			prompt_tokens: Some(10),
			completion_tokens: Some(20),
			total_tokens: Some(30),
			..Default::default()
		};
		let second = genai::chat::Usage {
			prompt_tokens: Some(5),
			completion_tokens: Some(15),
			total_tokens: Some(20),
			..Default::default()
		};

		// -- Exec
		add_usage(&mut total, &first);
		add_usage(&mut total, &second);

		// -- Check
		let total = total.ok_or("expected accumulated usage")?;
		assert_eq!(total.prompt_tokens, Some(15));
		assert_eq!(total.completion_tokens, Some(35));
		assert_eq!(total.total_tokens, Some(50));
		Ok(())
	}

	#[test]
	fn test_process_stats_accumulate_usage_sums_fields() -> Result<()> {
		// -- Setup & Fixtures
		let mut total = genai::chat::Usage {
			prompt_tokens: Some(10),
			completion_tokens: Some(20),
			total_tokens: Some(30),
			..Default::default()
		};
		let other = genai::chat::Usage {
			prompt_tokens: Some(5),
			completion_tokens: Some(15),
			total_tokens: Some(20),
			..Default::default()
		};

		// -- Exec
		accumulate_usage(&mut total, &other);

		// -- Check
		assert_eq!(total.prompt_tokens, Some(15));
		assert_eq!(total.completion_tokens, Some(35));
		assert_eq!(total.total_tokens, Some(50));
		Ok(())
	}
}

// endregion: --- Tests
