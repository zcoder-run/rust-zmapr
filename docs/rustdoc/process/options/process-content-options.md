`ProcessContentOptions` configures the Fetch, Sanitize, and Map stages of a content-processing workflow. The stages run in that order. A disabled stage passes the current artifacts through unchanged.

```rust
ProcessContentOptions::new("docs")
    .with_dest("target/zmapr-docs")
    .with_sanitize(true)
    .with_map(true)
    .with_model("gpt-5-mini");
```

## Defaults

`ProcessContentOptions::new(source)` sets `destination` to `None`; when no destination is provided, it is derived as follows:

- A local directory such as `docs` uses the sibling destination `docs-zmapr`.

- A local file such as `notes/guide.md` uses the sibling destination `notes/guide-zmapr`.

- A web URL such as `https://example.com/docs/` uses `zmapr/example.com/docs` relative to the current directory. URL path segments are sanitized.

- `fetch` is `true`; `sanitize`, `map`, and `resume` are `false`.

- `include` and `exclude` are empty.
- `format` is `FetchFormat::Md`.
- `max_depth` is `0`, so web Fetch starts with the requested resource and does not follow linked pages.
- `llms` is `true`, enabling `llms.txt` discovery for web sources.
- `model`, `sanitize_model`, `map_model`, and `sanitize_prompt` are `None`.
- `concurrency` is `8`.

## Fetch selection

Fetch uses the required source passed to `new(source)`. Include patterns are applied before exclusions, and exclusions take precedence. The supported patterns use `*` within a path segment and `**` across zero or more path segments. A leading `!` on an include pattern adds an exclusion.

`format` selects how fetched HTML is stored. Non-HTML files are stored unchanged. For web sources, `max_depth` controls link crawling from the starting URL, and `llms` controls discovery of `llms.txt` entries.

With `with_fetch(false)`, the workflow skips Fetch and loads a prior Fetch cache from the resolved destination. The source configured with `new(source)` must match the source recorded in that cache's manifest.

## AI stages and models

Set `sanitize` and `map` to enable their respective stages. Each enabled AI stage needs a nonempty resolved model. Sanitize uses `sanitize_model` when set, otherwise `model`. Map uses `map_model` when set, otherwise `model`.

A custom `sanitize_prompt` replaces the built-in Sanitize instructions. Inline prompt content must not be empty or whitespace-only. A file-backed prompt must identify an existing file.

## Resume and concurrency

When `resume` is enabled, successful unchanged work may be reused when the stage's saved state and inputs remain compatible. Reuse rules depend on the stage.

Sanitize records each completed item's result in `.tmp-zmapr/sanitize.journal.jsonl`. Resume reuses an entry only when the journal header, input hash, and existing output hash match the current run. For the same path, the latest journal record wins.

`concurrency` limits parallel item processing for web Fetch, Sanitize, and Map. Local Fetch processes items sequentially and does not use this limit.

Only one workflow run per destination directory is supported at a time. Journals do not provide cross-process locking.

## Configuration validation

The workflow validates these options before stage execution. At least one stage must be selected, `concurrency` must be greater than zero, and every enabled AI stage must resolve to a nonempty model. When Fetch is enabled, the source must be a valid local file or directory, or a structurally valid HTTP(S) URL. A local source directory must not equal or contain the resolved destination. When Fetch is disabled and a later stage is enabled, the prior Fetch cache must exist and its manifest source must match the configured source. Invalid Sanitize prompt values also cause configuration validation to fail.
