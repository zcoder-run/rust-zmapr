# Map Prompt APIs

This module defines the versioned prompt contract used to describe source files for a content map. [`render_file_prompt`] fills the embedded template, and [`parse_file_info`] converts its response into a [`FileMapEntry`].

## Response contract

The embedded template requests a `<FILE_INFO>` block containing a JSON object with `file_path`, `kind`, `summary`, `when_to_use`, and optional `public_types`, `public_functions`, and `topics` fields.

The parser stores `summary`, `when_to_use`, `public_types`, `public_functions`, and `topics` in the returned entry. It ignores other fields, including `file_path` and `kind`. Missing summary or usage text defaults to an empty string, and missing list fields default to empty lists.

List fields accept JSON arrays or comma-separated strings. The parser trims and removes empty public type and function names. It normalizes topics to at most seven entries, keeping at most the first three words of each entry.

The JSON may be enclosed in Markdown code fences, with or without a language label.

## Errors

Parsing fails if the response does not contain a `<FILE_INFO>` block with a closing tag, or if the block does not contain valid JSON. Prompt rendering can fail if the embedded template replacement engine cannot be initialized.

[`PROMPT_VERSION`] identifies the embedded prompt version.
