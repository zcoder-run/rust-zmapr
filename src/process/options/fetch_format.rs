use serde::{Deserialize, Serialize};

// region:    --- Types

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[doc = include_str!("../../../docs/rustdoc/process/options/fetch-format.md")]
pub enum FetchFormat {
	/// Preserve the fetched content in its raw representation.
	Raw,

	/// Use a compact representation of the fetched content.
	Slim,

	/// Use Markdown. This is the default Rust format.
	#[default]
	Md,
}

// endregion: --- Types
