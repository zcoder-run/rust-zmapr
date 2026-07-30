// region:    --- Modules

mod derive_aliases;
mod macros;
mod webc;

#[allow(unused)]
use derive_aliases::*;

mod error;
mod process;

pub use error::{Error, Result};
pub use process::*;

// endregion: --- Modules
