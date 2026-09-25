# Process Statistics

Process statistics provide live stage and workflow counters during execution, followed by validated final counts and durations when the workflow finishes. Live values are available through [`ProgressStats`]; completed results are represented by [`FinalStats`].

## Stage Status and Counts

[`StageStatus`] describes whether a stage is unselected, pending, running, completed, or failed. Each [`StageProgress`] contains that status along with per-outcome counters.

[`StageProgress::registered_items`] sums pending, running, completed, reused, skipped, and failed items. Excluded items are reported separately and are not part of that sum. If `total_items` is present, finalization checks it against the combined completed, reused, skipped, and failed outcomes. Excluded items are also kept separate from that outcome count.

A stage's usage is optional because a stage may not report token usage. Workflow-level usage aggregates the usage that is available.

## Durations and Final Statistics

Stage and workflow start and end timestamps use epoch microseconds. Live duration methods measure through the current time when an end timestamp is not yet available. A stage duration is unavailable until its start timestamp is set.

[`FinalStats`] contains a [`StageFinal`] for each selected stage. A stage that was not selected has no final statistics. Final statistics require a workflow end time, completed selected stages, no pending or running stage items, and consistent stage outcome totals. The final duration methods use the recorded start and end timestamps.
