// region:    --- Modules

mod derive_aliases;
mod error;

#[allow(unused)]
use derive_aliases::*;

pub mod build;
pub mod macros;
pub mod webc;

pub use error::{Error, Result};

// endregion: --- Modules
