Runs a content-processing workflow configured by [`ProcessContentOptions`] and returns a handle for observing progress and collecting the result.

## Stage selection

Providing a source selects Fetch. The `sanitize` and `map` options select their respective stages. Selected stages run in Fetch, Sanitize, then Map order. Disabled stages pass artifacts through unchanged.

When no source is provided and Sanitize or Map is selected, the workflow loads prior Fetch state from the destination. At least one stage must be selected.

## Observing the workflow

The function returns a [`ProcessContentHandle`] after validation and workflow startup. Use the handle to take the single-consumer [`crate::process::ProgressRx`], query authoritative state through [`ProcessQuery`], and await the final [`ProcessContentOutput`].

Progress notifications are best-effort. The query handle provides access to live state even if notifications are dropped. Item-level failures can be reported in the final output when the workflow completes successfully.

## Final content

After all selected stages succeed, the workflow copies the final artifacts to paths relative to the configured destination. Intermediate Fetch and Sanitize artifacts remain under `.tmp-zmapr/`.

Existing files at published artifact paths are replaced. Unrelated files and stale files from previous runs are not removed. The `.tmp-zmapr/...` paths and root `content-map.json` path are reserved and cannot be used as final artifact paths.

Publication errors are returned by `ProcessContentHandle::wait_output`. A failure can leave some final artifacts published and others unpublished.

## Validation and errors

Before starting the background workflow, the function validates that:

- At least one stage is selected and `max_concurrency` is greater than zero.
- A selected Fetch source is a valid local path or HTTP(S) URL.
- A local directory Fetch source does not resolve to the existing destination directory.
- Each selected AI stage resolves to a nonempty model.
- A configured Sanitize prompt file exists, or inline prompt content is nonempty.
- When Fetch is not selected but a downstream stage is, prior Fetch state is available. Its validity is checked when the workflow loads it.

Configuration and source validation errors are returned directly from `process_content`. Errors encountered while running the workflow are returned by [`ProcessContentHandle::wait_output`]. A retained query handle remains useful for inspecting partial state after a workflow error.
