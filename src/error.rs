use derive_more::{Display, From};

/// Result type returned by fallible crate operations.
pub type Result<T> = core::result::Result<T, Error>;

#[doc = include_str!("../docs/rustdoc/error.md")]
#[derive(Debug, Display, From)]
#[display("{self:?}")]
pub enum Error {
	/// An application-defined error message.
	#[from(String, &String, &str)]
	Custom(String),

	// -- Process
	/// The process configuration is invalid.
	InvalidConfiguration(String),

	/// The requested operation or input is unsupported.
	Unsupported(String),

	/// A required tag is missing from a response.
	MissingTag(String),

	/// A response does not match the expected format.
	MalformedResponse(String),

	/// A background task failed to complete successfully.
	TaskJoin(String),

	/// Cache content is invalid or cannot be used.
	InvalidCache(String),

	/// A state value does not match the expected format.
	MalformedState(String),

	// -- Externals
	/// An I/O operation failed.
	#[from]
	Io(std::io::Error),

	/// A `simple_fs` operation failed.
	#[from]
	SimpleFs(simple_fs::Error),

	/// An HTTP request failed.
	#[from]
	Reqwest(reqwest::Error),

	/// An HTTP header name is invalid.
	#[from]
	InvalidHeaderName(reqwest::header::InvalidHeaderName),

	/// An HTTP header value is invalid.
	#[from]
	InvalidHeaderValue(reqwest::header::InvalidHeaderValue),
}

// region:    --- Custom

impl Error {
	/// Creates a custom error from a value convertible to `String`.
	pub fn custom(val: impl Into<String>) -> Self {
		Self::Custom(val.into())
	}

	/// Creates a custom error containing the error's display text.
	pub fn custom_from_err(err: impl std::error::Error) -> Self {
		Self::Custom(err.to_string())
	}
}

// endregion: --- Custom

// region:    --- Error Boilerplate

impl std::error::Error for Error {}

// endregion: --- Error Boilerplate
