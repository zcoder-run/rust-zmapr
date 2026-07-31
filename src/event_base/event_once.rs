use super::{EventBaseError, EventBaseResult};

// region:    --- Types

pub(crate) struct OnceTx<T> {
	channel: &'static str,
	inner: tokio::sync::oneshot::Sender<T>,
}

pub(crate) struct OnceRx<T> {
	channel: &'static str,
	inner: tokio::sync::oneshot::Receiver<T>,
}

// endregion: --- Types

// region:    --- Constructors

impl<T> OnceTx<T> {
	pub(crate) fn new(channel: &'static str, inner: tokio::sync::oneshot::Sender<T>) -> Self {
		Self { channel, inner }
	}
}

impl<T> OnceRx<T> {
	pub(crate) fn new(channel: &'static str, inner: tokio::sync::oneshot::Receiver<T>) -> Self {
		Self { channel, inner }
	}
}

// endregion: --- Constructors

// region:    --- Operations

impl<T> OnceTx<T> {
	pub(crate) fn send(self, value: T) -> EventBaseResult<()> {
		self.inner
			.send(value)
			.map_err(|_| EventBaseError::TxDisconnected {
				channel: self.channel,
			})
	}
}

impl<T> OnceRx<T> {
	pub(crate) async fn recv(self) -> EventBaseResult<T> {
		self.inner
			.await
			.map_err(|_| EventBaseError::RxDisconnected {
				channel: self.channel,
			})
	}
}

// endregion: --- Operations
