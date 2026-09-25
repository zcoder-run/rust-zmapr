# Content Map Types

This module defines the in-memory content map and the serialized document used to store generated file and directory guidance.

## Map entries

[`ContentMap`] groups file and folder entries. Its `file_map` keys are source-relative file paths, and its `folder_map` keys are source-relative directory paths.

[`FileMapEntry`] stores a file summary, usage guidance, public type and function names, and searchable topics. [`FolderMapEntry`] stores a directory summary, usage guidance, and searchable topics.

## Serialized documents and provenance

[`ContentMapDocument`] carries the file and folder maps together with generation provenance: the schema version, model name, prompt version, and generation timestamp. [`ContentMapDocument::new`] sets the schema version to `1` and initializes file metadata as empty. [`ContentMapDocument::from_content_map`] transfers both maps from an in-memory [`ContentMap`].

File metadata is optional. Empty `file_metadata` is omitted when serialized, and an absent field defaults to an empty map when deserialized. [`FileMapMetadata`] records an optional source modification time in Unix nanoseconds and a source hash. Metadata entries are indexed by source-relative file path. [`ContentMapDocument::with_file_metadata`] replaces the document's file metadata map.
