use genai::Client as GenaiClient;
use genai::chat::{ChatMessage, ChatRequest};
use std::fmt::Debug;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, RwLock};

pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

// region:    --- Types

/// Trait abstracting AI completion calls for content mapping.
pub trait MaprAiClient: Debug + Send + Sync {
	fn complete<'a>(&'a self, prompt: &'a str) -> BoxFuture<'a, crate::Result<String>>;
}

/// Selector determining which AI client implementation to instantiate.
#[derive(Debug, Clone, Default)]
pub enum MaprAiSelector {
	#[default]
	Real,

	Stub,

	Custom(Arc<dyn MaprAiClient>),
}

/// Deterministic stub AI client for offline testing.
#[derive(Debug, Clone, Default)]
pub struct StubAiClient {
	custom_response: Option<String>,
}

/// Real genai-backed AI client placeholder.
#[derive(Debug, Clone)]
pub struct GenaiAiClient {
	model: String,
	client: Option<GenaiClient>,
}

// endregion: --- Types

// region:    --- Implementations

impl MaprAiSelector {
	pub fn create_client(&self, model: &str) -> Arc<dyn MaprAiClient> {
		match self {
			Self::Real => Arc::new(GenaiAiClient::new(model)),
			Self::Stub => Arc::new(StubAiClient::default()),
			Self::Custom(client) => client.clone(),
		}
	}
}

impl StubAiClient {
	pub fn from_response(response: impl Into<String>) -> Self {
		Self {
			custom_response: Some(response.into()),
		}
	}

	pub fn with_response(mut self, response: impl Into<String>) -> Self {
		self.custom_response = Some(response.into());
		self
	}
}

impl GenaiAiClient {
	pub fn new(model: impl Into<String>) -> Self {
		Self {
			model: model.into(),
			client: GenaiClient::new().ok(),
		}
	}

	pub fn with_client(mut self, client: GenaiClient) -> Self {
		self.client = Some(client);
		self
	}

	pub fn model(&self) -> &str {
		&self.model
	}
}

// endregion: --- Implementations

// region:    --- Trait Implementations

impl<T: ?Sized + MaprAiClient> MaprAiClient for Arc<T> {
	fn complete<'a>(&'a self, prompt: &'a str) -> BoxFuture<'a, crate::Result<String>> {
		(**self).complete(prompt)
	}
}

impl MaprAiClient for StubAiClient {
	fn complete<'a>(&'a self, prompt: &'a str) -> BoxFuture<'a, crate::Result<String>> {
		let response = if let Some(custom) = &self.custom_response {
			custom.clone()
		} else {
			let hash = blake3::hash(prompt.as_bytes()).to_hex();
			let short_hash = &hash[..8];
			format!(
				"<FILE_INFO>\n{{\n  \"summary\": \"Stub summary for content ({short_hash})\",\n  \"when_to_use\": \"Consult when evaluating stub mapping.\",\n  \"public_types\": [],\n  \"public_functions\": [],\n  \"topics\": [\"stub\", \"test\"]\n}}\n</FILE_INFO>"
			)
		};

		Box::pin(async move { Ok(response) })
	}
}

impl MaprAiClient for GenaiAiClient {
	fn complete<'a>(&'a self, prompt: &'a str) -> BoxFuture<'a, crate::Result<String>> {
		let model_name = self.model.clone();
		let client = self.client.clone();
		let prompt_owned = prompt.to_string();

		Box::pin(async move {
			let client = match client {
				Some(c) => c,
				None => GenaiClient::new()
					.map_err(|err| crate::Error::custom(format!("genai client initialization error: {err}")))?,
			};

			let chat_req = ChatRequest::new(vec![ChatMessage::user(prompt_owned)]);
			let chat_res = client
				.exec_chat(&model_name, chat_req, None)
				.await
				.map_err(|err| crate::Error::custom(format!("genai completion error: {err}")))?;

			let text = chat_res
				.first_text()
				.ok_or_else(|| crate::Error::custom("genai response did not contain text content"))?
				.to_string();

			Ok(text)
		})
	}
}

// endregion: --- Trait Implementations

// region:    --- Support

static ACTIVE_SELECTOR: RwLock<Option<MaprAiSelector>> = RwLock::new(None);

pub fn set_active_ai_selector(selector: Option<MaprAiSelector>) {
	if let Ok(mut guard) = ACTIVE_SELECTOR.write() {
		*guard = selector;
	}
}

pub fn get_active_ai_selector() -> MaprAiSelector {
	ACTIVE_SELECTOR.read().ok().and_then(|guard| guard.clone()).unwrap_or_default()
}

pub fn select_ai_client(selector: Option<&MaprAiSelector>, model: &str) -> Arc<dyn MaprAiClient> {
	if let Some(explicit) = selector {
		explicit.create_client(model)
	} else {
		let active = get_active_ai_selector();
		active.create_client(model)
	}
}

pub fn select_active_ai_client(model: &str) -> Arc<dyn MaprAiClient> {
	select_ai_client(None, model)
}

// endregion: --- Support

// region:    --- Tests

#[cfg(test)]
mod tests {
	use super::*;

	#[tokio::test]
	async fn test_stub_ai_client_default() -> crate::Result<()> {
		let client = StubAiClient::default();
		let response = client.complete("Sample file content to summarize").await?;

		assert!(response.contains("<FILE_INFO>"));
		assert!(response.contains("</FILE_INFO>"));
		assert!(response.contains("\"summary\":"));
		assert!(response.contains("\"when_to_use\":"));
		assert!(response.contains("\"topics\":"));

		Ok(())
	}

	#[tokio::test]
	async fn test_stub_ai_client_custom_response() -> crate::Result<()> {
		let custom = "<FILE_INFO>\n{\"summary\": \"Custom summary\"}\n</FILE_INFO>";
		let client = StubAiClient::from_response(custom);
		let response = client.complete("irrelevant input").await?;

		assert_eq!(response, custom);

		Ok(())
	}

	#[tokio::test]
	async fn test_genai_client_creation_and_error() -> crate::Result<()> {
		let client = GenaiAiClient::new("mock-model");
		assert_eq!(client.model(), "mock-model");

		let result = client.complete("irrelevant input").await;
		assert!(result.is_err());

		Ok(())
	}

	#[tokio::test]
	async fn test_selector_and_active_override() -> crate::Result<()> {
		let client = select_ai_client(Some(&MaprAiSelector::Stub), "gpt-5");
		let response = client.complete("test prompt").await?;
		assert!(response.contains("<FILE_INFO>"));

		set_active_ai_selector(Some(MaprAiSelector::Stub));
		let active_client = select_active_ai_client("gpt-5");
		let active_resp = active_client.complete("test prompt").await?;
		assert!(active_resp.contains("<FILE_INFO>"));

		// Reset active selector
		set_active_ai_selector(None);

		Ok(())
	}
}

// endregion: --- Tests
