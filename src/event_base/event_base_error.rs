use derive_more::Display;

// region:    --- Types

pub(crate) type EventBaseResult<T> = core::result::Result<T, EventBaseError>;

#[derive(Debug, Display)]
#[display("{self:?}")]
pub(crate) enum EventBaseError {
	Custom(String),

	InvalidCapacity {
		channel: &'static str,
		capacity: usize,
	},

	TxDisconnected {
		channel: &'static str,
	},

	RxDisconnected {
		channel: &'static str,
	},
}

// endregion: --- Types

// region:    --- Custom

impl EventBaseError {
	pub(crate) fn custom(val: impl Into<String>) -> Self {
		Self::Custom(val.into())
	}

	pub(crate) fn custom_from_err(err: impl std::error::Error) -> Self {
		Self::Custom(err.to_string())
	}

	pub(crate) fn is_disconnected(&self) -> bool {
		matches!(
			self,
			Self::TxDisconnected { .. } | Self::RxDisconnected { .. }
		)
	}
}

// endregion: --- Custom

// region:    --- Error Boilerplate

impl std::error::Error for EventBaseError {}

// endregion: --- Error Boilerplate
