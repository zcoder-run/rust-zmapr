use derive_more::{Display, From};

pub type Result<T> = core::result::Result<T, Error>;

#[derive(Debug, Display, From)]
#[display("{self:?}")]
pub enum Error {
	#[from(String, &String, &str)]
	Custom(String),

	// -- Process
	InvalidConfiguration(String),

	Unsupported(String),

	InvalidCache(String),

	MalformedState(String),

	// -- Externals
	#[from]
	Io(std::io::Error),

	#[from]
	Reqwest(reqwest::Error),

	#[from]
	InvalidHeaderName(reqwest::header::InvalidHeaderName),

	#[from]
	InvalidHeaderValue(reqwest::header::InvalidHeaderValue),
}

// region:    --- Custom

impl Error {
	pub fn custom(val: impl Into<String>) -> Self {
		Self::Custom(val.into())
	}

	pub fn custom_from_err(err: impl std::error::Error) -> Self {
		Self::Custom(err.to_string())
	}
}

// endregion: --- Custom

// region:    --- Error Boilerplate

impl std::error::Error for Error {}

// endregion: --- Error Boilerplate
