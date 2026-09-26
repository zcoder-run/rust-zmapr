`ProcessContentOptions` configures the Fetch, Sanitize, and Map stages of a content-processing workflow. The stages run in that order. A disabled stage passes the current artifacts through unchanged.

```rust
ProcessContentOptions::new("target/zmapr-docs")
    .with_source("docs")
    .with_sanitize(true)
    .with_map(true)
    .with_model("gpt-5-mini");
```

## Defaults

`ProcessContentOptions::new(destination)` sets the required destination and initializes the remaining options as follows:

- `source` is `None`. When Fetch is not selected and a later stage is enabled, the workflow uses a prior Fetch cache.
- `include` and `exclude` are empty.
- `format` is `FetchFormat::Md`.
- `max_depth` is `0`, so web Fetch starts with the requested resource and does not follow linked pages.
- `llms` is `true`, enabling `llms.txt` discovery for web sources.
- `sanitize`, `map`, and `resume` are `false`.
- `model`, `sanitize_model`, `map_model`, and `sanitize_prompt` are `None`.
- `concurrency` is `8`.

## Fetch selection

Set `source` to a local path or an HTTP(S) URL to select Fetch. Include patterns are applied before exclusions, and exclusions take precedence. The supported patterns use `*` within a path segment and `**` across zero or more path segments. A leading `!` on an include pattern adds an exclusion.

`format` selects how fetched HTML is stored. Non-HTML files are stored unchanged. For web sources, `max_depth` controls link crawling from the starting URL, and `llms` controls discovery of `llms.txt` entries.

## AI stages and models

Set `sanitize` and `map` to enable their respective stages. Each enabled AI stage needs a nonempty resolved model. Sanitize uses `sanitize_model` when set, otherwise `model`. Map uses `map_model` when set, otherwise `model`.

A custom `sanitize_prompt` replaces the built-in Sanitize instructions. Inline prompt content must not be empty or whitespace-only. A file-backed prompt must identify an existing file.

## Resume and concurrency

When `resume` is enabled, successful unchanged work may be reused when the stage's saved state and inputs remain compatible. Reuse rules depend on the stage.

Sanitize records each completed item's result in `.tmp-zmapr/sanitize.journal.jsonl`. Resume reuses an entry only when the journal header, input hash, and existing output hash match the current run. For the same path, the latest journal record wins.

`concurrency` limits parallel item processing for web Fetch, Sanitize, and Map. Local Fetch processes items sequentially and does not use this limit.

Only one workflow run per destination directory is supported at a time. Journals do not provide cross-process locking.

## Configuration validation

The workflow validates these options before stage execution. At least one stage must be selected, `max_concurrency` must be greater than zero, and every enabled AI stage must resolve to a nonempty model. A configured source must be a valid local file or directory, or a structurally valid HTTP(S) URL. Invalid Sanitize prompt values and missing or invalid prior Fetch state also cause configuration validation to fail.
