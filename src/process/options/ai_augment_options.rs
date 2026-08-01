// region:    --- Types

#[derive(Debug, Clone)]
pub struct AiAugmentOptions {
	/// Identifies the AI provider used to augment content.
	pub provider: String,
	/// Identifies the provider model used to augment content.
	pub model: String,
}

// endregion: --- Types

// region:    --- Constructors

impl AiAugmentOptions {
	/// Creates AI augmentation options for a provider and model.
	pub fn new(provider: impl Into<String>, model: impl Into<String>) -> Self {
		Self {
			provider: provider.into(),
			model: model.into(),
		}
	}
}

// endregion: --- Constructors

impl AiAugmentOptions {
	pub fn with_provider(mut self, provider: impl Into<String>) -> Self {
		self.provider = provider.into();
		self
	}

	pub fn with_model(mut self, model: impl Into<String>) -> Self {
		self.model = model.into();
		self
	}
}
