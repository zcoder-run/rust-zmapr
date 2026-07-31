use super::{EventBaseError, EventBaseResult, MpscRx, MpscTx, OnceRx, OnceTx};

pub(crate) const DEFAULT_CHANNEL_CAPACITY: usize = 64;

pub(crate) fn new_mpsc_bounded<T>(
	channel: &'static str,
	capacity: usize,
) -> EventBaseResult<(MpscTx<T>, MpscRx<T>)> {
	if capacity == 0 {
		return Err(EventBaseError::InvalidCapacity { channel, capacity });
	}

	let (sender, receiver) = tokio::sync::mpsc::channel(capacity);

	Ok((
		MpscTx::new(channel, sender),
		MpscRx::new(channel, receiver),
	))
}

pub(crate) fn new_mpsc_bounded_default<T>(
	channel: &'static str,
) -> EventBaseResult<(MpscTx<T>, MpscRx<T>)> {
	new_mpsc_bounded(channel, DEFAULT_CHANNEL_CAPACITY)
}

pub(crate) fn new_once<T>(channel: &'static str) -> (OnceTx<T>, OnceRx<T>) {
	let (sender, receiver) = tokio::sync::oneshot::channel::<T>();

	(
		OnceTx::new(channel, sender),
		OnceRx::new(channel, receiver),
	)
}
