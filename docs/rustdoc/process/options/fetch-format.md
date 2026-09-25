`FetchFormat` selects the representation used for fetched content. Serde serializes and deserializes the variants using snake_case names.

- `Raw` preserves content in its raw representation.
- `Slim` selects a compact representation.
- `Md` selects Markdown and is returned by `FetchFormat::default()`.
