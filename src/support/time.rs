use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub(crate) fn now_micro() -> i64 {
	SystemTime::now()
		.duration_since(UNIX_EPOCH)
		.map(|duration| i64::try_from(duration.as_micros()).unwrap_or(i64::MAX))
		.unwrap_or(0)
}

pub(crate) fn duration_between_micros(start: i64, end: i64) -> Duration {
	let micros = end.saturating_sub(start);
	Duration::from_micros(u64::try_from(micros).unwrap_or(0))
}

// region:    --- Tests

#[cfg(test)]
mod tests {
	type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

	use super::*;

	#[test]
	fn test_support_time_now_micro_positive() -> Result<()> {
		// -- Setup & Fixtures

		// -- Exec
		let now = now_micro();

		// -- Check
		assert!(now > 0);
		Ok(())
	}

	#[test]
	fn test_support_time_duration_between_micros_simple() -> Result<()> {
		// -- Setup & Fixtures
		let start = 100;
		let end = 150;

		// -- Exec
		let duration = duration_between_micros(start, end);

		// -- Check
		assert_eq!(duration, Duration::from_micros(50));
		Ok(())
	}

	#[test]
	fn test_support_time_duration_between_micros_negative_is_zero() -> Result<()> {
		// -- Setup & Fixtures
		let start = 150;
		let end = 100;

		// -- Exec
		let duration = duration_between_micros(start, end);

		// -- Check
		assert_eq!(duration, Duration::ZERO);
		Ok(())
	}
}

// endregion: --- Tests
