use serde::{Deserialize, Serialize};

// region:    --- Types

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FetchFormat {
	Raw,
	Slim,

	#[default]
	Markdown,
}

// endregion: --- Types
