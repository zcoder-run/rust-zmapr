# Content Sources

`ContentSource` represents the origin from which the Fetch stage selects content. It distinguishes local paths from web URLs, with `LocalContentSource` and `WebContentSource` carrying the corresponding source value.

Local paths can identify a file or directory. Fetch selects a file directly or discovers selected files recursively from a directory. Web sources provide the absolute URL where crawling begins.

The constructors and conversions in this module only wrap their input. They do not access the filesystem or validate URLs. Workflow request validation checks the selected source before stage execution. In workflow options, a source is represented as a local path or URL string.
