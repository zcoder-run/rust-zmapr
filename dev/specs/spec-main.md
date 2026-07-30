# zmapr Codebase Specification

## Purpose

`zmapr` is a Rust library for mapping code and content into AI-optimized context. The crate exposes a high-level content-processing workflow that is designed to process local filesystem content or website content through independently optional stages.

The intended workflow is:

```text
Source
  -> Fetch
  -> Sanitize
  -> AI Augment
  -> AI Content Map
```

The public API hides pipeline construction and stage ordering. Internal pipeline contracts remain private implementation details.

## Project Metadata

- Package name: `zmapr`
- Current version: `0.0.2-WIP`
- Rust edition: `2024`
- License: `MIT OR Apache-2.0`
- Authors: `Jeremy Chone`
- Repository: `https://github.com/zcoder-run/rust-zmapr`
- Homepage: `https://github.com/zcoder-run/rust-zmapr`
- Library doctests: disabled
- Unsafe Rust: forbidden
- Current project status: work in progress

## Public Entry Point

The single public workflow entry point is:

```rust
pub async fn process_content(
    source: impl Into<ContentSource>,
    options: ProcessContentOptions,
) -> Result<ProcessContentResponse>;
```

The crate reexports the public process API from `src/lib.rs`.

```rust
pub use error::{Error, Result};
pub use process::*;
```

## Source Model

Source definitions are implemented in `src/process/source.rs`.

### ContentSource

```rust
#[derive(Debug, Clone)]
pub enum ContentSource {
    LocalPath(LocalContentSource),
    Website(WebsiteContentSource),
}
```

`ContentSource` identifies the acquisition source for the workflow.

Supported source variants:

- `LocalPath`, a local file or directory.
- `Website`, a website URL used as the starting point for crawling.

### LocalContentSource

```rust
#[derive(Debug, Clone)]
pub struct LocalContentSource {
    pub path: SPath,
}
```

The `path` identifies a local file or directory. `SPath` is supplied by the `simple-fs` dependency.

Constructor:

```rust
pub fn new(path: impl Into<SPath>) -> Self;
```

### WebsiteContentSource

```rust
#[derive(Debug, Clone)]
pub struct WebsiteContentSource {
    pub url: String,
}
```

The `url` identifies the absolute website URL at which website Fetch starts.

Constructor:

```rust
pub fn new(url: impl Into<String>) -> Self;
```

### ContentSource Constructors

```rust
impl ContentSource {
    pub fn local_path(path: impl Into<SPath>) -> Self;
    pub fn website(url: impl Into<String>) -> Self;
}
```

### Source Conversions

The following conversions are implemented:

```rust
impl From<LocalContentSource> for ContentSource;
impl From<WebsiteContentSource> for ContentSource;
impl From<SPath> for ContentSource;
```

String conversions are implemented for `WebRequest`, not for `ContentSource`, to avoid ambiguity between local paths and website URLs.

## Workflow Options

Workflow options are implemented in `src/process/options.rs`.

### ProcessContentOptions

```rust
#[derive(Debug, Clone)]
pub struct ProcessContentOptions {
    pub destination: SPath,
    pub fetch: Option<FetchOptions>,
    pub sanitize: Option<SanitizeOptions>,
    pub ai_augment: Option<AiAugmentOptions>,
    pub content_map: Option<ContentMapOptions>,
    pub resume: bool,
    pub max_concurrency: usize,
}
```

Field behavior:

- `destination`, root directory for generated workflow artifacts.
- `fetch`, enables source selection and acquisition when present.
- `sanitize`, enables mechanical content preparation when present.
- `ai_augment`, enables AI cleanup and formatting when present.
- `content_map`, enables AI content-map generation when present.
- `resume`, controls reuse of successful unchanged work.
- `max_concurrency`, limits parallel item processing within a stage.

Constructor:

```rust
pub fn new(destination: impl Into<SPath>) -> Self;
```

Current constructor defaults:

- `fetch: None`
- `sanitize: None`
- `ai_augment: None`
- `content_map: None`
- `resume: false`
- `max_concurrency: 8`

### Chainable Workflow Methods

```rust
impl ProcessContentOptions {
    pub fn with_fetch(self, options: FetchOptions) -> Self;
    pub fn with_sanitize(self, options: SanitizeOptions) -> Self;
    pub fn with_ai_augment(self, options: AiAugmentOptions) -> Self;
    pub fn with_content_map(self, options: ContentMapOptions) -> Self;
}
```

Providing stage options enables that stage. Leaving the corresponding field as `None` disables it.

An empty workflow is invalid because it cannot produce an outcome.

### FetchOptions

```rust
#[derive(Debug, Clone, Default)]
pub struct FetchOptions {
    pub include: Vec<String>,
    pub exclude: Vec<String>,
    pub copy_local_files: bool,
    pub same_host_only: bool,
    pub max_depth: usize,
    pub follow_links: bool,
}
```

Field behavior:

- `include`, glob patterns selecting files or website paths.
- `exclude`, glob patterns excluding selected content.
- `copy_local_files`, copies selected local files into the deterministic Fetch cache when enabled.
- `same_host_only`, restricts website crawling to the source host.
- `max_depth`, maximum website link depth.
- `follow_links`, enables linked website-page discovery.

The intended default behavior is:

- Empty include patterns select all files.
- Exclusions take precedence.
- Local files are copied into the cache.
- Website crawling is restricted to the source host.
- Maximum website depth is zero.
- Link following is disabled.

The source currently derives `Default` but does not define explicit field values. The intended defaults above are part of the workflow specification and should be aligned with implementation before Fetch becomes executable.

### SanitizeOptions

```rust
#[derive(Debug, Clone, Default)]
pub struct SanitizeOptions {
    pub slim_html: bool,
    pub convert_to_markdown: bool,
}
```

Field behavior:

- `slim_html`, removes nonessential HTML structure and metadata.
- `convert_to_markdown`, converts supported content into Markdown.

The intended default behavior is to enable both HTML slimming and Markdown conversion. The source currently derives `Default` without explicit field defaults, so this behavior remains an implementation requirement.

### AiAugmentOptions

```rust
#[derive(Debug, Clone)]
pub struct AiAugmentOptions {
    pub provider: String,
    pub model: String,
}
```

Constructor:

```rust
pub fn new(provider: impl Into<String>, model: impl Into<String>) -> Self;
```

AI augmentation requires a nonempty provider and model.

### ContentMapOptions

```rust
#[derive(Debug, Clone)]
pub struct ContentMapOptions {
    pub provider: String,
    pub model: String,
    pub journal_path: Option<SPath>,
    pub reuse_unchanged_records: bool,
    pub retain_journal: bool,
}
```

Constructor:

```rust
pub fn new(provider: impl Into<String>, model: impl Into<String>) -> Self;
```

Constructor defaults:

- `journal_path: None`
- `reuse_unchanged_records: true`
- `retain_journal: true`

AI content-map generation requires a nonempty provider and model.

## Response Model

Response definitions are implemented in `src/process/response.rs`.

### ProcessContentResponse

```rust
#[derive(Debug, Clone)]
pub struct ProcessContentResponse {
    pub destination: SPath,
    pub manifest_path: Option<SPath>,
    pub content_root: SPath,
    pub content_map_path: Option<SPath>,
    pub completed_items: Vec<ProcessItem>,
    pub skipped_items: Vec<ProcessItem>,
    pub failures: Vec<ProcessFailure>,
}
```

Fields:

- `destination`, root directory containing generated workflow artifacts.
- `manifest_path`, durable workflow manifest when written.
- `content_root`, latest content artifact root produced by selected stages.
- `content_map_path`, published `content-map.json` when mapping is selected.
- `completed_items`, items completed by selected stages.
- `skipped_items`, items intentionally skipped or reused.
- `failures`, item-level failures retained for observability and retry.

### ProcessItem

```rust
#[derive(Debug, Clone)]
pub struct ProcessItem {
    pub source: String,
    pub output_path: Option<SPath>,
    pub stage: ProcessStage,
}
```

`source` is a stable source identity or source-relative path.

### ProcessFailure

```rust
#[derive(Debug, Clone)]
pub struct ProcessFailure {
    pub item: ProcessItem,
    pub message: String,
}
```

`message` contains human-readable failure details.

### ProcessStage

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcessStage {
    Fetch,
    Sanitize,
    AiAugment,
    AiContentMap,
}
```

## Content Map Contract

Content-map definitions are implemented in `src/process/map.rs`.

### ContentMap

```rust
#[derive(Debug, Clone, Default)]
pub struct ContentMap {
    pub file_map: BTreeMap<String, FileMapEntry>,
    pub folder_map: BTreeMap<String, FolderMapEntry>,
}
```

The serialized contract uses `file_map` and `folder_map`.

### FileMapEntry

```rust
#[derive(Debug, Clone, Default)]
pub struct FileMapEntry {
    pub summary: String,
    pub when_to_use: String,
    pub public_types: Vec<String>,
    pub public_functions: Vec<String>,
    pub topics: Vec<String>,
}
```

### FolderMapEntry

```rust
#[derive(Debug, Clone, Default)]
pub struct FolderMapEntry {
    pub summary: String,
    pub when_to_use: String,
    pub topics: Vec<String>,
}
```

Folder records do not require public type or public function fields.

## Internal Module Structure

The crate root is implemented in `src/lib.rs`.

Current modules:

```text
src/
├── lib.rs
├── error.rs
├── derive_aliases.rs
├── macros/
│   ├── mod.rs
│   └── from_optional.rs
├── process/
│   ├── mod.rs
│   ├── map.rs
│   ├── options.rs
│   ├── pipeline.rs
│   ├── process_impl.rs
│   ├── response.rs
│   └── source.rs
└── webc/
    ├── mod.rs
    ├── web_client.rs
    └── web_request.rs
```

### Process Module

`src/process/mod.rs` privately declares implementation modules and publicly reexports their public APIs:

- `map`
- `options`
- `pipeline`
- `process_impl`
- `response`
- `source`

The pipeline module remains private.

### Web Client Module

`src/webc/mod.rs` privately declares and publicly reexports:

- `web_client`
- `web_request`

The web client API is currently separate from the process pipeline.

## Internal Pipeline Contracts

Internal pipeline types are implemented in `src/process/pipeline.rs`.

### WorkflowContext

```rust
pub(crate) struct WorkflowContext {
    pub(crate) destination: SPath,
    pub(crate) fetch_cache: SPath,
    pub(crate) sanitize_output: SPath,
    pub(crate) ai_augment_output: SPath,
    pub(crate) manifest: SPath,
    pub(crate) journal: SPath,
    pub(crate) content_map: SPath,
    pub(crate) max_concurrency: usize,
    pub(crate) resume: bool,
}
```

`WorkflowContext` contains resolved paths and execution controls shared by stages.

### ArtifactSet

```rust
pub(crate) struct ArtifactSet {
    pub(crate) root: SPath,
    pub(crate) items: Vec<ArtifactItem>,
}
```

`ArtifactSet` identifies the current local artifact root and its items.

### ArtifactItem

```rust
pub(crate) struct ArtifactItem {
    pub(crate) source: String,
    pub(crate) relative_path: String,
    pub(crate) local_path: SPath,
    pub(crate) media_type: Option<String>,
    pub(crate) source_hash: Option<String>,
}
```

Each artifact records:

- Stable source identity.
- Stable source-relative path.
- Current local path.
- Optional media type.
- Optional source hash.

### StageOutput

```rust
pub(crate) struct StageOutput {
    pub(crate) artifacts: ArtifactSet,
    pub(crate) completed_items: Vec<ProcessItem>,
    pub(crate) skipped_items: Vec<ProcessItem>,
    pub(crate) failures: Vec<ProcessFailure>,
}
```

`StageOutput` carries the next artifact set and item-level outcomes.

### ProcessingStage

The private asynchronous stage contract is implemented without `async-trait`:

```rust
trait ProcessingStage {
    fn execute<'a>(
        &'a self,
        context: &'a WorkflowContext,
        input: ArtifactSet,
    ) -> StageFuture<'a>;
}
```

The boxed future type is:

```rust
type StageFuture<'a> =
    Pin<Box<dyn Future<Output = Result<StageOutput>> + Send + 'a>>;
```

### Stage Ordering

Stages execute in fixed order:

1. Fetch
2. Sanitize
3. AI Augment
4. AI Content Map

Disabled stages after Fetch should pass the current artifact set through unchanged.

AI Content Map is terminal. It analyzes the current artifact set and does not replace it.

### Current Deferred Stage Adapter

`DeferredStage` currently implements `ProcessingStage` and returns `Error::Unsupported` identifying the stage and deferred operation.

At the current implementation state, selecting any stage causes the pipeline to invoke the deferred adapter.

## Workflow Validation

Validation is implemented in `src/process/process_impl.rs` before stage execution.

Current validation rules:

- At least one processing stage must be enabled.
- `max_concurrency` must be greater than zero.
- AI Augment requires a nonempty provider and model.
- AI Content Map requires a nonempty provider and model.
- Website sources require Fetch.
- Website Fetch currently returns an unsupported execution error.
- Downstream processing without Fetch requires an existing Fetch cache directory.
- Downstream processing without Fetch also requires an existing Fetch manifest file.

Website Fetch additionally requires `same_host_only` to be enabled.

Validation is intended to occur before destination mutation or stage execution.

## Deterministic Workflow Layout

The layout is resolved beneath `ProcessContentOptions::destination`.

```text
<destination>/
├── .zmapr/
│   ├── fetch/
│   ├── stages/
│   │   ├── sanitize/
│   │   └── ai-augment/
│   ├── manifest.json
│   └── content-map.journal.jsonl
└── content-map.json
```

Resolved paths:

- Fetch cache: `<destination>/.zmapr/fetch`
- Sanitize output: `<destination>/.zmapr/stages/sanitize`
- AI Augment output: `<destination>/.zmapr/stages/ai-augment`
- Manifest: `<destination>/.zmapr/manifest.json`
- Content-map journal: `<destination>/.zmapr/content-map.journal.jsonl`
- Published content map: `<destination>/content-map.json`

A custom content-map journal path is represented by `ContentMapOptions::journal_path`, although the current layout resolver uses the default internal journal path.

## Intended Fetch Behavior

The first executable vertical slice is local Fetch.

### Local Input

Local Fetch accepts:

- A single regular file.
- A directory root.

Directory traversal is recursive.

Symbolic links are skipped initially to avoid traversal cycles and content escaping the selected root.

### Selection

Fetch behavior:

1. Discover regular files.
2. Compute stable source-relative paths.
3. Apply include glob patterns.
4. Apply exclude glob patterns.
5. Sort selected files by source-relative path.

Empty include patterns select all files. Exclude patterns take precedence.

### Path Safety

Source-relative paths must be normalized and validated so copied files cannot escape the Fetch cache.

The implementation must reject unsafe relative paths and avoid path traversal through `..` components.

### Copying

When `copy_local_files` is enabled:

- Selected files are copied into `.zmapr/fetch`.
- Relative source structure is preserved.
- Source files remain unchanged.

When copying is disabled:

- Artifact items retain their original local paths.
- The source is not mutated.
- The artifact set still contains stable relative paths and source metadata.

### Hashing

Fetch computes stable source hashes for durable identity and future resume behavior.

### Error Handling

Item-level read and copy failures should become `ProcessFailure` records where continued processing is safe.

The stage should fail when its artifact root or required durable state cannot be established.

## Intended Sanitize Behavior

Sanitize processes the current artifact set into a separate output root.

Supported behavior:

- Read UTF-8 text files.
- Read UTF-8 HTML files.
- Optionally slim HTML.
- Optionally convert HTML to Markdown.
- Preserve ordinary UTF-8 text when no transformation applies.
- Preserve relative source structure.
- Select an appropriate Markdown extension for converted output.
- Do not mutate Fetch artifacts or original source files.
- Return a new artifact set rooted at the Sanitize output.

Unsupported or non-UTF-8 artifacts are reported as skipped.

Item-level transformation errors are retained as failures without discarding successful outputs, unless publication of the stage output itself fails.

The existing `htmlr` dependency is intended for HTML conversion and processing.

## Deferred Behavior

The following functionality is publicly modeled but currently deferred:

- Website Fetch execution.
- AI Augment execution.
- AI Content Map execution.
- Durable manifest creation and publication.
- Durable content-map journal handling.
- Atomic content-map publication.
- Complete Fetch resume behavior.
- Complete end-to-end response assembly.

Selecting a deferred stage must return a structured application error rather than panic or use a reachable `todo!`.

## Current Process Execution State

`process_content` currently:

1. Converts the source into `ContentSource`.
2. Validates the request.
3. Resolves the workflow layout.
4. Builds `WorkflowContext`.
5. Runs the internal pipeline skeleton.
6. Returns `Error::Unsupported` when no executable processing result is available.

The current pipeline starts with an empty Fetch artifact set rooted at the Fetch cache. It does not yet perform filesystem discovery or stage execution.

## Error Model

Error definitions are implemented in `src/error.rs`.

### Result Alias

```rust
pub type Result<T> = core::result::Result<T, Error>;
```

### Error Variants

```rust
#[derive(Debug, Display, From)]
#[display("{self:?}")]
pub enum Error {
    Custom(String),
    InvalidConfiguration(String),
    Unsupported(String),
    InvalidCache(String),
    MalformedState(String),
    Io(std::io::Error),
    Reqwest(reqwest::Error),
    InvalidHeaderName(reqwest::header::InvalidHeaderName),
    InvalidHeaderValue(reqwest::header::InvalidHeaderValue),
}
```

Process-specific errors:

- `InvalidConfiguration`, invalid workflow or stage configuration.
- `Unsupported`, selected functionality is not implemented.
- `InvalidCache`, required prior cache is missing or invalid.
- `MalformedState`, required durable state is missing or malformed.

External errors:

- Standard I/O errors.
- HTTP client errors.
- Invalid HTTP header names.
- Invalid HTTP header values.

Helper methods:

```rust
impl Error {
    pub fn custom(val: impl Into<String>) -> Self;
    pub fn custom_from_err(err: impl std::error::Error) -> Self;
}
```

## Web Client API

The web client is implemented in `src/webc/web_client.rs`.

### WebClient

```rust
pub struct WebClient {
    reqwest: reqwest::Client,
}
```

Constructor function:

```rust
pub fn new_client(
    opts: impl Into<super::WebClientOptions>,
) -> crate::Result<WebClient>;
```

The current constructor converts options and builds a default `reqwest::Client`.

### WebClientOptions

```rust
#[derive(Debug, Default, FromOptional!)]
pub struct WebClientOptions {}
```

The `FromOptional!` macro implements:

```rust
impl From<Option<WebClientOptions>> for WebClientOptions {
    fn from(value: Option<WebClientOptions>) -> Self {
        value.unwrap_or_default()
    }
}
```

### WebClient GET

```rust
impl WebClient {
    pub fn get(
        &self,
        request: impl Into<super::WebRequest>,
    ) -> crate::Result<()>;
}
```

The current method creates a `reqwest` GET request but does not send it or return a response.

## Web Request API

Web request definitions are implemented in `src/webc/web_request.rs`.

### Type Aliases

```rust
pub type WebHeaders = reqwest::header::HeaderMap;
pub type WebParams = Vec<(String, String)>;
```

### WebRequest

```rust
pub struct WebRequest {
    pub url: String,
    pub headers: Option<WebHeaders>,
    pub params: Option<WebParams>,
}
```

Constructor:

```rust
pub fn new(url: impl Into<String>) -> Self;
```

### WebRequest Builder Methods

```rust
impl WebRequest {
    pub fn with_headers(self, headers: WebHeaders) -> Self;

    pub fn append_header(
        self,
        name: impl Into<String>,
        value: impl Into<String>,
    ) -> Result<Self>;

    pub fn with_params(
        self,
        params: impl IntoIterator<Item = (String, String)>,
    ) -> Self;

    pub fn append_param(
        self,
        name: impl Into<String>,
        value: impl Into<String>,
    ) -> Self;

    pub fn append_params(
        self,
        params: impl IntoIterator<Item = (String, String)>,
    ) -> Self;
}
```

`append_header` validates the header name and value and returns crate `Result`.

### WebRequest Conversions

```rust
impl From<String> for WebRequest;
impl From<&str> for WebRequest;
impl From<&String> for WebRequest;
```

## Macros

Macros are implemented in `src/macros`.

### FromOptional

`FromOptional!` generates an implementation that converts `Option<T>` into `T` using `Default`:

```rust
FromOptional! {
    struct Example {
        ...
    }
}
```

Generated behavior:

```rust
impl From<Option<Example>> for Example {
    fn from(value: Option<Example>) -> Self {
        value.unwrap_or_default()
    }
}
```

The macro is crate-private.

### Derive Aliases

`src/derive_aliases.rs` imports `macro_rules_attribute::derive_alias` and declares an empty derive-alias namespace.

The crate root imports the generated aliases with an unused allowance.

## Dependencies

Dependencies declared in `Cargo.toml`:

- `tokio`, asynchronous runtime with full features.
- `genai`, AI provider integration.
- `serde`, serialization and derive support.
- `serde_with`, serialization helpers and macros.
- `htmlr`, HTML parsing and conversion.
- `reqwest`, HTTP client.
- `simple-fs`, filesystem abstraction and `SPath`.
- `derive_more`, derived conversions and display behavior.
- `macro_rules_attribute`, declarative macro and derive-alias support.

The current dependency list does not yet include a dedicated glob-matching or hashing crate. Fetch implementation must first verify whether existing dependencies provide suitable functionality before adding new dependencies.

## Serialization Status

Public content-map types currently derive `Debug`, `Clone`, and in some cases `Default`, but they do not currently derive or implement `Serialize` or `Deserialize`.

The intended durable manifest and content-map publication require deterministic serialization support. This is an implementation gap to resolve before durable response handling is complete.

## Documentation Status

`README.md` currently contains:

- Project title.
- Short project description.
- Placeholder text indicating more content will be added.
- License information.
- Repository link.

The README does not yet document:

- Local filesystem usage.
- Workflow option construction.
- Fetch and Sanitize behavior.
- Deferred Website, AI Augment, and AI Content Map stages.

The initial implementation scope requires adding one concise local usage example and identifying deferred stages.

## Implementation Constraints

The codebase follows these constraints:

- Rust edition 2024.
- Avoid `.unwrap()` and `.expect(...)`.
- Use crate `Result` and `Error`.
- Preserve source comments unless explicitly requested otherwise.
- Keep public types in focused modules.
- Keep internal pipeline types private.
- Keep fixed stage ordering private.
- Use separate implementation blocks for constructors and chainable methods when useful.
- Group `From` implementations in `Froms` regions.
- Use `Support` regions for private file-local helpers.
- Use asynchronous filesystem operations where they avoid blocking.
- Bound parallel work using `max_concurrency`.
- Keep deterministic ordering independent of task completion order.
- Do not mutate source files or prior-stage artifacts.
- Return structured errors for deferred functionality.
- Do not expose custom pipeline construction through the public API.

## Planned Completion Criteria

The initial local vertical slice is complete when all of the following are true:

- A local file can be fetched.
- A local directory can be recursively traversed.
- Symbolic links are skipped.
- Include and exclude patterns work deterministically.
- Selected files are sorted by stable relative path.
- Local files can be copied into the Fetch cache.
- Local files can alternatively remain referenced at their source paths.
- Stable source hashes are recorded.
- Sanitize can process supported UTF-8 text and HTML.
- HTML slimming and Markdown conversion work according to options.
- Each stage writes to a separate output location.
- Fetch-only workflows return `ProcessContentResponse`.
- Fetch-plus-Sanitize workflows return `ProcessContentResponse`.
- Downstream Sanitize without Fetch validates and uses a prior Fetch result.
- Deferred Website and AI stages return structured errors.
- Workflow state is durably represented.
- README contains a local usage example and deferred-stage documentation.
- `cargo fmt --check` passes.
- `cargo check` passes.

