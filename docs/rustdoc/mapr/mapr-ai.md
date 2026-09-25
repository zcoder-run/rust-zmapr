# AI Client APIs

This module provides the completion interface used by content mapping, built-in client implementations, and helpers for selecting a client.

## Client abstraction

Implement [`MaprAiClient`] to provide a completion backend. Its `complete` method receives the full prompt and returns a sendable [`BoxFuture`] containing either a [`MaprAiResponse`] or the crate's error type. Custom implementations can leave response usage unset when token information is unavailable.

[`MaprAiResponse::new`] creates a response without usage information. Use [`MaprAiResponse::with_usage`] to attach usage reported by a provider or calculated by a client.

## Client selection

[`MaprAiSelector`] has three choices:

- `Real` creates a [`GenaiAiClient`] for the requested model.
- `Stub` creates a deterministic local [`StubAiClient`].
- `Custom` uses the supplied shared [`MaprAiClient`] implementation. The model argument does not alter a custom client.

[`MaprAiSelector::create_client`] creates a client directly from a selector. The selection helpers also support a process-wide active selector:

- [`select_ai_client`] uses its explicit selector when provided. Otherwise, it uses [`get_active_ai_selector`].
- [`select_active_ai_client`] selects using the active selector.
- [`set_active_ai_selector`] sets the selector for later default selections. Pass `None` to clear the override and restore the default `Real` selection.

The active selector is process-wide. Select an explicit selector when a call should not depend on that shared setting.

## Built-in clients

[`StubAiClient`] works without a provider. By default, it recognizes content enclosed in `<SANITIZE_INPUT>` tags and returns it inside `<SANITIZED_CONTENT>` tags. For other prompts it returns stub file metadata whose summary includes a short hash of the prompt. It supplies estimated token usage unless usage or a fixed response is configured. `from_response` and `with_response` configure a fixed response, while `with_usage` configures the usage value.

[`GenaiAiClient`] sends a user chat message containing the supplied prompt to genai using its configured model. `new` creates a client for a model, and `with_client` accepts an already configured genai client. Completion can fail if client initialization or the provider request fails, or if the provider response contains no text. Successful responses include the usage returned by genai.
