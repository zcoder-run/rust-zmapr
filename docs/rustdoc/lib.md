# zmapr

`zmapr` is a Rust library for mapping code and content into AI-oriented context.

The public workflow runs the selected stages in fixed order: Fetch, Sanitize, then Map. A disabled stage passes artifacts through unchanged. When Fetch is disabled, downstream processing uses a valid prior Fetch cache.

## Workflow

Use [`process_content`] to start a processing workflow. Configure its behavior with [`ProcessContentOptions`]. Sources may be local or web content, represented by [`ContentSource`], and Fetch output can be configured with [`FetchFormat`].

The returned [`ProcessContentHandle`] exposes progress through [`ProgressRx`], live state through [`ProcessQuery`], and final results through [`ProcessContentHandle::wait_output`]. Use [`ProcessStateSnapshot`] for a point-in-time view.

## Mapping

Mapped content is represented by [`ContentMap`]. The mapping API also provides AI client selection through [`MaprAiClient`] and [`MaprAiSelector`], prompt construction, and journal support. Use [`set_active_ai_selector`] to choose the active client selector.

## Errors

Fallible public APIs use the crate's [`Result`] alias and [`Error`] type.
