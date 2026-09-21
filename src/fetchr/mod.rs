// region:    --- Modules

mod fetchr_http;
mod fetchr_local;
mod fetchr_types;
mod support;

pub(crate) use fetchr_http::*;
pub(crate) use fetchr_local::*;
pub(crate) use fetchr_types::*;
pub use fetchr_types::{
	FetchCommonOptions, FetchRequest, LocalFetchOptions, LocalFetchRequest, WebFetchOptions, WebFetchRequest,
};
pub(crate) use support::*;

// endregion: --- Modules
