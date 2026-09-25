# Sanitize Prompt

`SanitizePrompt` supplies custom instructions for the Sanitize stage. Choose either a file-backed prompt or inline text. When supplied through `ProcessContentOptions::sanitize_prompt`, the custom instructions replace the built-in Sanitize instructions.

- `FilePath` identifies a file containing the instructions.
- `Content` stores the instructions directly as text.

During process configuration validation, inline content must not be empty or whitespace-only, and a file-backed prompt must point to an existing file. Invalid prompt values cause configuration validation to fail.

Use [`SanitizePrompt::file`] to create a file-backed prompt or [`SanitizePrompt::content`] to create an inline prompt.
