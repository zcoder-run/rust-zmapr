# Process State Queries

This module provides read-only access to the authoritative in-memory state of a running process workflow.

## Querying workflow state

`ProcessQuery` provides methods to retrieve copies of the current workflow statistics and registered items. It can look up an item by its run-scoped [`ItemId`] or stored relative path, list items in id order, and return ids matching a stage status.

These methods return copies. They do not provide mutable access to the workflow's state.

## Item lookup

An `ItemId` identifies an item only within its process run. `item_by_path()` looks up an item using its stored relative path, which may differ from the original path as processing advances.

## Snapshots

`ProcessStateSnapshot` contains the workflow's [`ProgressStats`] and the [`ItemState`] values for all registered items. `ProcessQuery::snapshot()` returns these together as a point-in-time copy, so the returned snapshot remains unchanged as the workflow continues.
