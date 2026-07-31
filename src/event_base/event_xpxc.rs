use super::{EventBaseError, EventBaseResult};

// region:    --- Types

pub(crate) struct MpscTx<T> {
	channel: &'static str,
	inner: tokio::sync::mpsc::Sender<T>,
}

pub(crate) struct MpscRx<T> {
	channel: &'static str,
	inner: tokio::sync::mpsc::Receiver<T>,
}

// endregion: --- Types

// region:    --- Constructors

impl<T> MpscTx<T> {
	pub(crate) fn new(channel: &'static str, inner: tokio::sync::mpsc::Sender<T>) -> Self {
		Self { channel, inner }
	}
}

impl<T> MpscRx<T> {
	pub(crate) fn new(channel: &'static str, inner: tokio::sync::mpsc::Receiver<T>) -> Self {
		Self { channel, inner }
	}
}

// endregion: --- Constructors

// region:    --- Operations

impl<T> MpscTx<T> {
	pub(crate) async fn send(&self, message: T) -> EventBaseResult<()> {
		self.inner
			.send(message)
			.await
			.map_err(|_| EventBaseError::TxDisconnected {
				channel: self.channel,
			})
	}

	pub(crate) fn try_send(&self, message: T) -> EventBaseResult<bool> {
		match self.inner.try_send(message) {
			Ok(()) => Ok(true),
			Err(tokio::sync::mpsc::error::TrySendError::Full(_)) => Ok(false),
			Err(tokio::sync::mpsc::error::TrySendError::Closed(_)) => {
				Err(EventBaseError::TxDisconnected {
					channel: self.channel,
				})
			}
		}
	}

	pub(crate) fn is_disconnected(&self) -> bool {
		self.inner.is_closed()
	}
}

impl<T> MpscRx<T> {
	pub(crate) async fn recv(&mut self) -> EventBaseResult<T> {
		self.inner
			.recv()
			.await
			.ok_or(EventBaseError::RxDisconnected {
				channel: self.channel,
			})
	}

	pub(crate) fn is_disconnected(&self) -> bool {
		self.inner.is_closed()
	}
}

// endregion: --- Operations

// region:    --- Trait Implementations

impl<T> Clone for MpscTx<T> {
	fn clone(&self) -> Self {
		Self {
			channel: self.channel,
			inner: self.inner.clone(),
		}
	}
}

// endregion: --- Trait Implementations
