# Map Journal

The Map journal is a newline-delimited JSON cache of successful file and folder mapping results. Its header identifies the model, prompt version, and artifact root. Those values determine whether a journal can be reused for a later run.

## Loading and recovery

Use [`init_or_load_journal`] to obtain a [`JournalReuseIndex`] and a [`JournalAppender`]. A missing, empty, or incompatible journal is initialized with a new header. An incompatible header does not reuse prior entries.

A truncated or malformed final record is treated as an incomplete append. Loading through `init_or_load_journal` discards that record and truncates the file to the last valid record. Malformed records before the final line are treated as invalid cache data and return an error. [`load_journal`] reads without repairing the file. It returns no index for missing, empty, or incompatible journals, and it ignores a malformed final line.

## Reuse index

The [`JournalReuseIndex`] contains successful file and folder entries. Lookups require both the source-relative path and the current source hash to match. Failed records remove any previous cached entry for their path. Applying a header record leaves the index unchanged.

The index stores only successful entries, so its file and folder counts do not include failed records.

## Appending records

[`JournalAppender::create_new`] creates parent directories as needed, truncates the target, and writes the header. [`JournalAppender::open_existing`] opens a file for appending without validating its content. [`JournalAppender::append`] writes each record as one JSON line and flushes it.

Cloned appenders share a synchronized file handle. [`JournalAppender::empty`] truncates that file without writing a replacement header. A later call to [`init_or_load_journal`] initializes an empty journal with a header.

## Record construction and maintenance

[`JournalHeader::new`] computes a fingerprint from the model, prompt version, and artifact root. [`JournalRecord`] constructors create header, successful, and failed records for files and folders. The record status and optional entry or error fields represent the result.

[`remove_journal`] removes an existing journal and succeeds when the file is already absent. [`empty_journal`] truncates an existing file and leaves an absent file unchanged.
