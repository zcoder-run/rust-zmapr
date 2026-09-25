# Process Progress Notifications

This module exposes progress events, statistics snapshots, and a receiver for notifications from a running workflow.

## Progress updates

A `ProgressUpdate` pairs an event with a sequence number and a snapshot of workflow statistics. Events describe workflow and stage lifecycle changes, item registration or exclusion, and item status changes.

Use `ProcessContentHandle::take_progress_rx()` to take the workflow's progress receiver. The receiver can be taken only once. A `ProgressRx` receives updates asynchronously with `recv()` and can report whether its channel has disconnected with `is_disconnected()`.

## Notification delivery and state

Progress notifications are best-effort. The workflow publishes them to a bounded channel without waiting for the receiver, and an update may not arrive if the channel cannot accept it or the receiver is disconnected. Sequence numbers can help identify gaps in the notifications received.

Notifications are not the authoritative workflow state. Use `ProcessQuery` to inspect current state. Each update also includes a `ProgressStats` snapshot, which may not reflect updates that were not delivered.
