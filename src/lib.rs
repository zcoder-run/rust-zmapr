// region:    --- Modules

mod derive_aliases;
mod event_base;
mod macros;
mod webc;

#[allow(unused)]
use derive_aliases::*;

mod error;
mod fetchr;
mod mapr;
mod process;
mod support;

pub use error::{Error, Result};
pub use fetchr::*;
pub use mapr::*;
pub use process::*;

// endregion: --- Modules
