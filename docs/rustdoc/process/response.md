# Workflow Handle and Output

The workflow handle provides access to progress notifications and authoritative in-memory state while processing runs. Its output describes the artifacts and final item state produced by a successful workflow.

## Workflow Handle

[`ProcessContentHandle`] owns the workflow's single progress receiver. Call `take_progress_rx` to transfer it to a progress consumer. The receiver can be taken only once. Use `query` to obtain a read-only handle for inspecting current workflow state.

Call `wait_output` to await completion. It consumes the workflow handle and returns the final output on success. Workflow failures and completion-channel errors are returned as errors.

## Output

[`ProcessContentOutput`] contains the destination directory, which is also `content_root` and contains the published final content, optional paths to the durable manifest and published content map, final item states, and validated final statistics. Optional paths are absent when the corresponding artifact was not written or published.

## Stages

[`ProcessStage`] identifies the workflow stage associated with progress and item state:

- `Fetch` retrieves source content.
- `Sanitize` sanitizes fetched content.
- `Map` builds a content map from processed content.
