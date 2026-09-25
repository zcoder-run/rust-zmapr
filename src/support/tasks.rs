use crate::{Error, Result};
use std::future::Future;

/// Runs the given futures as Tokio tasks, keeping at most `concurrency` tasks spawned at once.
/// A new task is spawned only when a running task finishes, until the input is exhausted.
/// Results are returned in completion order. A task panic or cancellation returns `Error::TaskJoin`.
pub(crate) async fn run_bounded<T, Fut>(
	futures: impl IntoIterator<Item = Fut>,
	concurrency: usize,
	task_label: &str,
) -> Result<Vec<T>>
where
	Fut: Future<Output = T> + Send + 'static,
	T: Send + 'static,
{
	let concurrency = concurrency.max(1);
	let mut futures = futures.into_iter();
	let mut join_set = tokio::task::JoinSet::new();
	let mut results = Vec::new();

	for future in futures.by_ref().take(concurrency) {
		join_set.spawn(future);
	}

	while let Some(res) = join_set.join_next().await {
		let value = res.map_err(|error| Error::TaskJoin(format!("{task_label} task failed: {error}")))?;
		results.push(value);

		if let Some(future) = futures.next() {
			join_set.spawn(future);
		}
	}

	Ok(results)
}

// region:    --- Tests

#[cfg(test)]
mod tests {
	type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

	use super::*;
	use std::{
		sync::{
			Arc,
			atomic::{AtomicUsize, Ordering},
		},
		time::Duration,
	};

	#[tokio::test]
	async fn test_support_tasks_run_bounded_respects_limit() -> Result<()> {
		// -- Setup & Fixtures
		let current = Arc::new(AtomicUsize::new(0));
		let peak = Arc::new(AtomicUsize::new(0));
		let tasks = (0..10).map(|index| {
			let current = Arc::clone(&current);
			let peak = Arc::clone(&peak);

			async move {
				let active = current.fetch_add(1, Ordering::SeqCst) + 1;
				let _ = peak.fetch_max(active, Ordering::SeqCst);
				tokio::time::sleep(Duration::from_millis(20)).await;
				let _ = current.fetch_sub(1, Ordering::SeqCst);
				index
			}
		});

		// -- Exec
		let mut results = run_bounded(tasks, 3, "Test").await?;

		// -- Check
		results.sort_unstable();
		assert_eq!(peak.load(Ordering::SeqCst), 3);
		assert_eq!(results, (0..10).collect::<Vec<_>>());
		Ok(())
	}

	#[tokio::test]
	async fn test_support_tasks_run_bounded_zero_concurrency_runs_sequentially() -> Result<()> {
		// -- Setup & Fixtures
		let current = Arc::new(AtomicUsize::new(0));
		let peak = Arc::new(AtomicUsize::new(0));
		let tasks = (0..4).map(|index| {
			let current = Arc::clone(&current);
			let peak = Arc::clone(&peak);

			async move {
				let active = current.fetch_add(1, Ordering::SeqCst) + 1;
				let _ = peak.fetch_max(active, Ordering::SeqCst);
				tokio::time::sleep(Duration::from_millis(20)).await;
				let _ = current.fetch_sub(1, Ordering::SeqCst);
				index
			}
		});

		// -- Exec
		let mut results = run_bounded(tasks, 0, "Test").await?;

		// -- Check
		results.sort_unstable();
		assert_eq!(peak.load(Ordering::SeqCst), 1);
		assert_eq!(results, (0..4).collect::<Vec<_>>());
		Ok(())
	}

	#[tokio::test]
	async fn test_support_tasks_run_bounded_panic_returns_task_join() -> Result<()> {
		// -- Setup & Fixtures
		let tasks = [async { panic_task() }];

		// -- Exec
		let result = run_bounded(tasks, 1, "Test").await;

		// -- Check
		assert!(matches!(result, Err(Error::TaskJoin(_))));
		Ok(())
	}

	// -- Test Support
	fn panic_task() -> usize {
		panic!("intentional task panic")
	}
}

// endregion: --- Tests
