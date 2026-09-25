# Process Item State

This module defines the identity and recorded state for items handled by the process workflow. An item can have state for its Fetch, Sanitize, and Map stages.

## Item identity

`ItemId` provides the numeric index assigned to an item within a process run. Use `index()` to retrieve that value. It is an in-run identifier, not a persistent identifier across runs.

`ItemState` combines the identifier with the source information and any recorded stage states.

## Stage state

`ItemStatus` describes a stage's lifecycle. A stage's `ItemStageState` also records its output path, reported token usage, and error details when those values are available.

The Fetch, Sanitize, and Map fields on `ItemState` are optional. A missing state means no state is recorded for that stage on the item. Use `stage()` to access a stage by its `ProcessStage` value.

## Content path

`content_path()` returns the Sanitize output path when present, or the Fetch output path otherwise. It selects based on path presence, regardless of stage status, and does not consider Map output paths.
